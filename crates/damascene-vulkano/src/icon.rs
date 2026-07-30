//! GPU vector-icon rendering for the Vulkano backend.
//!
//! Two paths share one [`IconPaint`]:
//!
//! - **MSDF**: pre-rasterised once per `(icon source, stroke_width)`
//!   into an MTSDF atlas (RGB = 3-channel SDF, A = true single-channel
//!   SDF) and rendered through `stock::text_msdf` (one quad per icon).
//!   Used for the default `Flat` material. App-supplied
//!   [`damascene_core::SvgIcon`]s share the same path, keyed on their
//!   content hash.
//! - **Tessellated**: lyon-tessellated triangles with analytic-AA
//!   fringes drawn through the `stock::vector*` shader family. Kept
//!   for the `Relief` and `Glass` materials, whose fragment shaders
//!   need the per-fragment view-box coordinate the MSDF path doesn't
//!   carry. Authored-paint custom SVG icons and painted app vectors also
//!   use this path so per-path colour and gradients survive.
//!
//! Each [`IconRun`] carries a `kind` so the runner knows which path to
//! draw it through.

use std::ops::Range;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use damascene_core::color::ColorSpace;
use damascene_core::icons::msdf_atlas::{
    DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD, IconMsdfAtlas, IconMsdfPage, IconMsdfSlot, IconRect,
};
use damascene_core::icons::svg::{IconSource, SvgIconPaintMode};
use damascene_core::paint::{
    DEFAULT_WORKING_COLOR_SPACE, IconRun, IconRunKind, PhysicalScissor, rgba_f32_in,
};
use damascene_core::shader::stock_wgsl;
use damascene_core::tree::{Color, Rect};
use damascene_core::vector::{
    GRADIENT_RAMP_WIDTH, IconMaterial, MAX_FRAME_GRADIENTS, VectorAsset, VectorGradientFrame,
    VectorGradientGpuParams, VectorMeshOptions, VectorMeshVertex, VectorRenderMode,
    append_vector_asset_mesh,
};
use smallvec::smallvec;
use vulkano::{
    buffer::{
        Buffer, BufferCreateInfo, BufferUsage, Subbuffer,
        allocator::{SubbufferAllocator, SubbufferAllocatorCreateInfo},
    },
    command_buffer::{
        AutoCommandBufferBuilder, BufferImageCopy, CommandBufferUsage, CopyBufferInfo,
        CopyBufferToImageInfo, allocator::StandardCommandBufferAllocator,
    },
    descriptor_set::{
        DescriptorSet, WriteDescriptorSet, allocator::StandardDescriptorSetAllocator,
    },
    device::{Device, Queue},
    format::Format,
    image::{
        Image, ImageAspects, ImageCreateInfo, ImageSubresourceLayers, ImageType, ImageUsage,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::ImageView,
    },
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        DynamicState, GraphicsPipeline, Pipeline, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{
                AttachmentBlend, BlendFactor, BlendOp, ColorBlendAttachmentState, ColorBlendState,
            },
            input_assembly::{InputAssemblyState, PrimitiveTopology},
            rasterization::RasterizationState,
            subpass::PipelineSubpassType,
            vertex_input::{
                VertexInputAttributeDescription, VertexInputBindingDescription, VertexInputRate,
                VertexInputState,
            },
            viewport::ViewportState,
        },
    },
    render_pass::Subpass,
    shader::{ShaderModule, ShaderModuleCreateInfo},
    sync::{self, GpuFuture},
};

use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::{build_shared_pipeline_layout, multisample_state};

const TESS_VERTEX_ARENA_SIZE: u64 = 256 * 1024;
const MSDF_INSTANCE_ARENA_SIZE: u64 = 64 * 1024;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct MsdfIconInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

struct MsdfPageGpu {
    image: Arc<Image>,
    descriptor_set: Arc<DescriptorSet>,
}

pub(crate) struct IconPaint {
    // Tess path.
    tess_vertices: Vec<VectorMeshVertex>,
    tess_vertex_alloc: SubbufferAllocator,
    tess_vertex_buf: Option<Subbuffer<[VectorMeshVertex]>>,
    flat_pipeline: Arc<GraphicsPipeline>,
    relief_pipeline: Arc<GraphicsPipeline>,
    glass_pipeline: Arc<GraphicsPipeline>,

    // Fragment-stage gradient table (issues #140/#141), bound at set 1
    // of the tess pipelines. Device-local and updated via fence-waited
    // copies only when the frame's table content actually changes, so
    // the descriptor set is built once and steady-state frames upload
    // nothing.
    gradient_frame: VectorGradientFrame,
    gradient_uniform_buf: Subbuffer<[VectorGradientGpuParams]>,
    gradient_ramp_image: Arc<Image>,
    gradient_descriptor_set: Arc<DescriptorSet>,
    gradient_uploaded_params: Vec<VectorGradientGpuParams>,
    gradient_uploaded_ramps: Vec<u16>,

    // MSDF path.
    msdf_atlas: IconMsdfAtlas,
    msdf_pages: Vec<MsdfPageGpu>,
    msdf_instances: Vec<MsdfIconInstance>,
    msdf_instance_alloc: SubbufferAllocator,
    msdf_instance_buf: Option<Subbuffer<[MsdfIconInstance]>>,
    msdf_pipeline: Arc<GraphicsPipeline>,
    msdf_sampler: Arc<Sampler>,

    runs: Vec<IconRun>,
    material: IconMaterial,

    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    queue: Arc<Queue>,

    /// Working color space MSDF icon mask colors are converted into. Kept
    /// in sync with the owning `Runner` via `set_working_color_space`.
    working_color_space: ColorSpace,
}

impl IconPaint {
    pub(crate) fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
        cmd_alloc: Arc<StandardCommandBufferAllocator>,
        subpass: Subpass,
        sample_count: u32,
    ) -> Self {
        let flat_pipeline = build_tess_pipeline(
            device.clone(),
            subpass.clone(),
            sample_count,
            "stock::vector",
            stock_wgsl::VECTOR,
        );
        let relief_pipeline = build_tess_pipeline(
            device.clone(),
            subpass.clone(),
            sample_count,
            "stock::vector_relief",
            stock_wgsl::VECTOR_RELIEF,
        );
        let glass_pipeline = build_tess_pipeline(
            device.clone(),
            subpass.clone(),
            sample_count,
            "stock::vector_glass",
            stock_wgsl::VECTOR_GLASS,
        );
        let tess_vertex_alloc = SubbufferAllocator::new(
            memory_alloc.clone(),
            SubbufferAllocatorCreateInfo {
                arena_size: TESS_VERTEX_ARENA_SIZE,
                buffer_usage: BufferUsage::VERTEX_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );

        let msdf_pipeline = build_msdf_pipeline(device.clone(), subpass, sample_count);
        let msdf_sampler = Sampler::new(
            device,
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: icon msdf sampler");
        let msdf_instance_alloc = SubbufferAllocator::new(
            memory_alloc.clone(),
            SubbufferAllocatorCreateInfo {
                arena_size: MSDF_INSTANCE_ARENA_SIZE,
                buffer_usage: BufferUsage::VERTEX_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );

        // ---- Gradient table (fixed-size, device-local) ----
        let gradient_uniform_buf = Buffer::new_slice::<VectorGradientGpuParams>(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::UNIFORM_BUFFER | BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            MAX_FRAME_GRADIENTS as u64,
        )
        .expect("damascene-vulkano: icon gradient uniform buf");
        let gradient_ramp_image = Image::new(
            memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: Format::R16G16B16A16_SFLOAT,
                extent: [GRADIENT_RAMP_WIDTH as u32, MAX_FRAME_GRADIENTS as u32, 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: icon gradient ramp image");
        // Set 1 of the three tess pipelines is the identical
        // reflection-derived gradient block, so a set built against the
        // flat pipeline's layout is compatible with all of them. The
        // clamp + bilinear MSDF sampler is exactly what ramp rows need.
        let gradient_view = ImageView::new_default(gradient_ramp_image.clone())
            .expect("damascene-vulkano: icon gradient ramp view");
        let gradient_descriptor_set = DescriptorSet::new(
            descriptor_alloc.clone(),
            flat_pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::buffer(0, gradient_uniform_buf.clone()),
                WriteDescriptorSet::image_view(1, gradient_view),
                WriteDescriptorSet::sampler(2, msdf_sampler.clone()),
            ],
            [],
        )
        .expect("damascene-vulkano: icon gradient descriptor set");

        // Define the uniform buffer's contents up front — slots are
        // never indexed until allocated, but the whole binding range is
        // visible to the shader.
        upload_gradient_table(
            &queue,
            &cmd_alloc,
            &memory_alloc,
            &gradient_uniform_buf,
            &gradient_ramp_image,
            &[],
            &[],
        );

        Self {
            tess_vertices: Vec::new(),
            tess_vertex_alloc,
            tess_vertex_buf: None,
            flat_pipeline,
            relief_pipeline,
            glass_pipeline,
            gradient_frame: VectorGradientFrame::new(),
            gradient_uniform_buf,
            gradient_ramp_image,
            gradient_descriptor_set,
            gradient_uploaded_params: Vec::new(),
            gradient_uploaded_ramps: Vec::new(),
            msdf_atlas: IconMsdfAtlas::new(DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD),
            msdf_pages: Vec::new(),
            msdf_instances: Vec::new(),
            msdf_instance_alloc,
            msdf_instance_buf: None,
            msdf_pipeline,
            msdf_sampler,
            runs: Vec::new(),
            material: IconMaterial::Flat,
            memory_alloc,
            descriptor_alloc,
            cmd_alloc,
            queue,
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        }
    }

    /// Update the working color space subsequent icon color packing
    /// converts into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    pub(crate) fn set_material(&mut self, material: IconMaterial) {
        self.material = material;
    }

    pub(crate) fn material(&self) -> IconMaterial {
        self.material
    }

    pub(crate) fn frame_begin(&mut self) {
        self.tess_vertices.clear();
        self.msdf_instances.clear();
        self.runs.clear();
        self.gradient_frame.begin(self.working_color_space);
    }

    pub(crate) fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &IconSource,
        color: Color,
        stroke_width: f32,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();

        let use_msdf = matches!(self.material, IconMaterial::Flat)
            && matches!(source.paint_mode(), SvgIconPaintMode::CurrentColorMask);

        if use_msdf {
            if let Some(slot) = self.msdf_atlas.ensure(source, stroke_width) {
                let (page_w, page_h) = self.msdf_page_dims(slot.page);
                let instance = msdf_instance_for_icon(
                    rect,
                    color,
                    &slot,
                    page_w,
                    page_h,
                    self.working_color_space,
                );
                let first = self.msdf_instances.len() as u32;
                self.msdf_instances.push(instance);
                self.runs.push(IconRun {
                    kind: IconRunKind::Msdf,
                    scissor,
                    first,
                    count: 1,
                    page: slot.page,
                    material: IconMaterial::Flat,
                });
            }
        } else {
            let material = self.material;
            let asset = source.vector_asset();
            let first = self.tess_vertices.len() as u32;
            let mesh_run = append_vector_asset_mesh(
                asset,
                VectorMeshOptions::icon(rect, color, stroke_width, self.working_color_space),
                &mut self.tess_vertices,
                Some(&mut self.gradient_frame),
            );
            if mesh_run.count > 0 {
                self.runs.push(IconRun {
                    kind: IconRunKind::Tess,
                    scissor,
                    first,
                    count: mesh_run.count,
                    page: 0,
                    material,
                });
            }
        }
        start..self.runs.len()
    }

    /// Record an app-supplied [`VectorAsset`] for paint. The render mode
    /// is chosen by core/app code, not inferred from vector contents.
    pub(crate) fn record_vector(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        asset: &VectorAsset,
        render_mode: VectorRenderMode,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();

        match render_mode {
            VectorRenderMode::Mask { color } => {
                if let Some(slot) = self.msdf_atlas.ensure_vector_asset(asset) {
                    let (page_w, page_h) = self.msdf_page_dims(slot.page);
                    let instance = msdf_instance_for_icon(
                        rect,
                        color,
                        &slot,
                        page_w,
                        page_h,
                        self.working_color_space,
                    );
                    let first = self.msdf_instances.len() as u32;
                    self.msdf_instances.push(instance);
                    self.runs.push(IconRun {
                        kind: IconRunKind::Msdf,
                        scissor,
                        first,
                        count: 1,
                        page: slot.page,
                        material: IconMaterial::Flat,
                    });
                }
            }
            VectorRenderMode::Painted => {
                let first = self.tess_vertices.len() as u32;
                let mesh_run = append_vector_asset_mesh(
                    asset,
                    VectorMeshOptions::icon(
                        rect,
                        Color::srgb_u8(255, 255, 255),
                        1.0,
                        self.working_color_space,
                    ),
                    &mut self.tess_vertices,
                    Some(&mut self.gradient_frame),
                );
                if mesh_run.count > 0 {
                    self.runs.push(IconRun {
                        kind: IconRunKind::Tess,
                        scissor,
                        first,
                        count: mesh_run.count,
                        page: 0,
                        material: IconMaterial::Flat,
                    });
                }
            }
        }
        start..self.runs.len()
    }

    fn msdf_page_dims(&self, page_idx: u32) -> (u32, u32) {
        let page = self
            .msdf_atlas
            .page(page_idx)
            .expect("freshly-ensured slot references a missing atlas page");
        (page.width, page.height)
    }

    pub(crate) fn flush(&mut self) {
        // ---- Tess vertex buffer ----
        if self.tess_vertices.is_empty() {
            self.tess_vertex_buf = None;
        } else {
            let buf = self
                .tess_vertex_alloc
                .allocate_slice::<VectorMeshVertex>(self.tess_vertices.len() as u64)
                .expect("damascene-vulkano: icon tess vertex suballocate");
            buf.write()
                .expect("damascene-vulkano: icon tess vertex suballocation write")
                .copy_from_slice(&self.tess_vertices);
            self.tess_vertex_buf = Some(buf);
        }

        // ---- MSDF atlas pages: create new GPU images, upload dirty regions ----
        while self.msdf_pages.len() < self.msdf_atlas.pages().len() {
            let i = self.msdf_pages.len();
            let page = &self.msdf_atlas.pages()[i];
            let new_page = self.create_msdf_page(page.width, page.height);
            self.msdf_pages.push(new_page);
        }
        let dirty = self.msdf_atlas.take_dirty();
        if !dirty.is_empty() {
            let mut builder = AutoCommandBufferBuilder::primary(
                self.cmd_alloc.clone(),
                self.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .expect("damascene-vulkano: icon msdf upload cmd builder");
            for (page_idx, rect) in &dirty {
                if rect.w == 0 || rect.h == 0 {
                    continue;
                }
                let page = &self.msdf_atlas.pages()[*page_idx];
                let bytes = pack_rect_bytes(page, *rect);
                let staging = Buffer::from_iter(
                    self.memory_alloc.clone(),
                    BufferCreateInfo {
                        usage: BufferUsage::TRANSFER_SRC,
                        ..Default::default()
                    },
                    AllocationCreateInfo {
                        memory_type_filter: MemoryTypeFilter::PREFER_HOST
                            | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                        ..Default::default()
                    },
                    bytes,
                )
                .expect("damascene-vulkano: icon msdf staging buf");
                let copy_info = CopyBufferToImageInfo {
                    regions: smallvec![BufferImageCopy {
                        buffer_offset: 0,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: ImageSubresourceLayers {
                            aspects: ImageAspects::COLOR,
                            mip_level: 0,
                            array_layers: 0..1,
                        },
                        image_offset: [rect.x, rect.y, 0],
                        image_extent: [rect.w, rect.h, 1],
                        ..Default::default()
                    }],
                    ..CopyBufferToImageInfo::buffer_image(
                        staging,
                        self.msdf_pages[*page_idx].image.clone(),
                    )
                };
                builder
                    .copy_buffer_to_image(copy_info)
                    .expect("damascene-vulkano: icon msdf copy_buffer_to_image");
            }
            let cb = builder
                .build()
                .expect("damascene-vulkano: icon msdf upload cmd build");
            let future = sync::now(self.queue.device().clone())
                .then_execute(self.queue.clone(), cb)
                .expect("damascene-vulkano: icon msdf upload then_execute")
                .then_signal_fence_and_flush()
                .expect("damascene-vulkano: icon msdf upload flush");
            future
                .wait(None)
                .expect("damascene-vulkano: icon msdf upload fence wait");
        }

        // ---- Gradient table ----
        // Upload only when the frame's table differs from what the GPU
        // holds: table content is stable across steady-state frames, so
        // this is a first-frame / theme-change event, and the fence-wait
        // inside the upload matches the MSDF dirty-page pattern.
        if !self.gradient_frame.is_empty()
            && (self.gradient_frame.params() != &self.gradient_uploaded_params[..]
                || self.gradient_frame.ramp_data() != &self.gradient_uploaded_ramps[..])
        {
            upload_gradient_table(
                &self.queue,
                &self.cmd_alloc,
                &self.memory_alloc,
                &self.gradient_uniform_buf,
                &self.gradient_ramp_image,
                self.gradient_frame.params(),
                self.gradient_frame.ramp_data(),
            );
            self.gradient_uploaded_params = self.gradient_frame.params().to_vec();
            self.gradient_uploaded_ramps = self.gradient_frame.ramp_data().to_vec();
        }

        // ---- MSDF instance buffer ----
        if self.msdf_instances.is_empty() {
            self.msdf_instance_buf = None;
        } else {
            let buf = self
                .msdf_instance_alloc
                .allocate_slice::<MsdfIconInstance>(self.msdf_instances.len() as u64)
                .expect("damascene-vulkano: icon msdf instance suballocate");
            buf.write()
                .expect("damascene-vulkano: icon msdf instance suballocation write")
                .copy_from_slice(&self.msdf_instances);
            self.msdf_instance_buf = Some(buf);
        }
    }

    fn create_msdf_page(&self, width: u32, height: u32) -> MsdfPageGpu {
        let image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                // Linear (NOT sRGB) — distance bytes shouldn't pass
                // through the sRGB EOTF.
                format: Format::R8G8B8A8_UNORM,
                extent: [width, height, 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: icon msdf atlas page image");
        let view =
            ImageView::new_default(image.clone()).expect("damascene-vulkano: icon msdf page view");
        let descriptor_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            self.msdf_pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.msdf_sampler.clone()),
            ],
            [],
        )
        .expect("damascene-vulkano: icon msdf page descriptor set");
        MsdfPageGpu {
            image,
            descriptor_set,
        }
    }

    pub(crate) fn run(&self, index: usize) -> IconRun {
        self.runs[index]
    }

    pub(crate) fn tess_pipeline(&self, material: IconMaterial) -> &Arc<GraphicsPipeline> {
        match material {
            IconMaterial::Flat => &self.flat_pipeline,
            IconMaterial::Relief => &self.relief_pipeline,
            IconMaterial::Glass => &self.glass_pipeline,
        }
    }

    /// Per-frame tess vertex suballocation. Panics if called for a
    /// frame with no tess draws — bind sites are gated by the
    /// `IconRunKind::Tess` arm, which `record(...)` only emits when
    /// `tess_vertices` is non-empty.
    pub(crate) fn tess_vertex_buf(&self) -> &Subbuffer<[VectorMeshVertex]> {
        self.tess_vertex_buf
            .as_ref()
            .expect("damascene-vulkano: icon tess_vertex_buf accessed with no draws")
    }

    /// Gradient table descriptor set for set 1 of the tess pipelines.
    /// Always valid — pipelines statically reference the set, so it is
    /// bound for every tess draw even in gradient-free frames.
    pub(crate) fn gradient_descriptor_set(&self) -> &Arc<DescriptorSet> {
        &self.gradient_descriptor_set
    }

    pub(crate) fn msdf_pipeline(&self) -> &Arc<GraphicsPipeline> {
        &self.msdf_pipeline
    }

    /// Per-frame MSDF instance suballocation. Panics if called for a
    /// frame with no MSDF draws — bind sites are gated by the
    /// `IconRunKind::Msdf` arm.
    pub(crate) fn msdf_instance_buf(&self) -> &Subbuffer<[MsdfIconInstance]> {
        self.msdf_instance_buf
            .as_ref()
            .expect("damascene-vulkano: icon msdf_instance_buf accessed with no draws")
    }

    pub(crate) fn msdf_page_descriptor(&self, page: u32) -> &Arc<DescriptorSet> {
        &self.msdf_pages[page as usize].descriptor_set
    }
}

fn msdf_instance_for_icon(
    rect: Rect,
    color: Color,
    slot: &IconMsdfSlot,
    page_w: u32,
    page_h: u32,
    working_color_space: ColorSpace,
) -> MsdfIconInstance {
    let [_, _, vw, vh] = slot.view_box;
    let logical_per_unit_x = rect.w / vw.max(0.001);
    let logical_per_unit_y = rect.h / vh.max(0.001);
    let spread_x = slot.spread * logical_per_unit_x / slot.px_per_unit.max(0.001);
    let spread_y = slot.spread * logical_per_unit_y / slot.px_per_unit.max(0.001);

    let bx = rect.x - spread_x;
    let by = rect.y - spread_y;
    let bw = rect.w + 2.0 * spread_x;
    let bh = rect.h + 2.0 * spread_y;

    let pw = page_w as f32;
    let ph = page_h as f32;
    let uv = [
        slot.rect.x as f32 / pw,
        slot.rect.y as f32 / ph,
        slot.rect.w as f32 / pw,
        slot.rect.h as f32 / ph,
    ];

    MsdfIconInstance {
        rect: [bx, by, bw, bh],
        uv,
        color: rgba_f32_in(color, working_color_space),
        params: [slot.spread, 0.0, 0.0, 0.0],
    }
}

fn pack_rect_bytes(page: &IconMsdfPage, rect: IconRect) -> Vec<u8> {
    const BPP: usize = 4;
    let row_bytes = rect.w as usize * BPP;
    let mut bytes = Vec::with_capacity(row_bytes * rect.h as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y as usize * page.width as usize + rect.x as usize) * BPP;
        let end = start + row_bytes;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    bytes
}

fn tess_vertex_input_state() -> VertexInputState {
    let bind_vertex = VertexInputBindingDescription {
        stride: std::mem::size_of::<VectorMeshVertex>() as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let attr = |offset: u32, format: Format| VertexInputAttributeDescription {
        binding: 0,
        offset,
        format,
        ..Default::default()
    };

    VertexInputState::new()
        .binding(0, bind_vertex)
        .attribute(0, attr(0, Format::R32G32_SFLOAT))
        .attribute(1, attr(8, Format::R32G32_SFLOAT))
        .attribute(2, attr(16, Format::R32G32B32A32_SFLOAT))
        .attribute(3, attr(32, Format::R32G32B32A32_SFLOAT))
}

/// Copy `params` (zero-padded to [`MAX_FRAME_GRADIENTS`]) into the
/// gradient uniform buffer and `ramps` (row-major f16 bits) into the top
/// rows of the ramp image, fence-waited like the MSDF dirty-page path.
/// Empty inputs still define the buffer's full contents (zeros).
#[allow(clippy::too_many_arguments)]
fn upload_gradient_table(
    queue: &Arc<Queue>,
    cmd_alloc: &Arc<StandardCommandBufferAllocator>,
    memory_alloc: &Arc<StandardMemoryAllocator>,
    uniform_buf: &Subbuffer<[VectorGradientGpuParams]>,
    ramp_image: &Arc<Image>,
    params: &[VectorGradientGpuParams],
    ramps: &[u16],
) {
    let mut padded = vec![VectorGradientGpuParams::zeroed(); MAX_FRAME_GRADIENTS];
    padded[..params.len()].copy_from_slice(params);

    let mut builder = AutoCommandBufferBuilder::primary(
        cmd_alloc.clone(),
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .expect("damascene-vulkano: icon gradient upload cmd builder");

    let param_staging = Buffer::from_iter(
        memory_alloc.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        },
        padded,
    )
    .expect("damascene-vulkano: icon gradient param staging");
    builder
        .copy_buffer(CopyBufferInfo::buffers(param_staging, uniform_buf.clone()))
        .expect("damascene-vulkano: icon gradient param copy");

    let row_count = (ramps.len() / (GRADIENT_RAMP_WIDTH * 4)) as u32;
    if row_count > 0 {
        let ramp_staging = Buffer::from_iter(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            ramps.iter().copied(),
        )
        .expect("damascene-vulkano: icon gradient ramp staging");
        let copy_info = CopyBufferToImageInfo {
            regions: smallvec![BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: ImageSubresourceLayers {
                    aspects: ImageAspects::COLOR,
                    mip_level: 0,
                    array_layers: 0..1,
                },
                image_offset: [0, 0, 0],
                image_extent: [GRADIENT_RAMP_WIDTH as u32, row_count, 1],
                ..Default::default()
            }],
            ..CopyBufferToImageInfo::buffer_image(ramp_staging, ramp_image.clone())
        };
        builder
            .copy_buffer_to_image(copy_info)
            .expect("damascene-vulkano: icon gradient ramp copy");
    }

    let cb = builder
        .build()
        .expect("damascene-vulkano: icon gradient upload cmd build");
    let future = sync::now(queue.device().clone())
        .then_execute(queue.clone(), cb)
        .expect("damascene-vulkano: icon gradient upload then_execute")
        .then_signal_fence_and_flush()
        .expect("damascene-vulkano: icon gradient upload flush");
    future
        .wait(None)
        .expect("damascene-vulkano: icon gradient upload fence wait");
}

fn build_tess_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
    name: &str,
    wgsl: &str,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv(name, wgsl)
        .unwrap_or_else(|e| panic!("damascene-vulkano: icon WGSL compile for `{name}`: {e}"));
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words)).unwrap_or_else(|e| {
            panic!("damascene-vulkano: icon ShaderModule::new for `{name}`: {e}")
        })
    };
    let vs = module
        .entry_point("vs_main")
        .unwrap_or_else(|| panic!("{name}: missing vs_main"));
    let fs = module
        .entry_point("fs_main")
        .unwrap_or_else(|| panic!("{name}: missing fs_main"));
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(tess_vertex_input_state()),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(multisample_state(sample_count)),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(AttachmentBlend::alpha()),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap_or_else(|e| panic!("damascene-vulkano: icon GraphicsPipeline::new for `{name}`: {e:?}"))
}

fn build_msdf_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv("stock::text_msdf (icon)", stock_wgsl::TEXT_MSDF)
        .expect("damascene-vulkano: icon msdf WGSL compile");
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("damascene-vulkano: icon msdf ShaderModule::new")
    };
    let vs = module
        .entry_point("vs_main")
        .expect("text_msdf.wgsl: missing vs_main");
    let fs = module
        .entry_point("fs_main")
        .expect("text_msdf.wgsl: missing fs_main");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<MsdfIconInstance>() as u32,
        input_rate: VertexInputRate::Instance { divisor: 1 },
        ..Default::default()
    };
    let attr = |binding: u32, offset: u32, format: Format| VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    };
    let vertex_input_state = VertexInputState::new()
        .binding(0, bind_vertex)
        .binding(1, bind_instance)
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        .attribute(3, attr(1, 32, Format::R32G32B32A32_SFLOAT))
        .attribute(4, attr(1, 48, Format::R32G32B32A32_SFLOAT));

    let premultiplied = AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(multisample_state(sample_count)),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(premultiplied),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .expect("damascene-vulkano: icon msdf GraphicsPipeline::new")
}
