//! GPU compositing for app-owned [`AppTexture`]s on the vulkano
//! backend. Mirrors `damascene-wgpu/src/surface.rs`.
//!
//! Three pipelines are built up front (one per [`SurfaceAlpha`])
//! sharing a single `stock::surface` shader module; the dispatch loop
//! picks the matching pipeline per run. Bind groups (descriptor sets
//! here) are cached on [`AppTextureId`]; cache entries unreferenced
//! for one frame are dropped at flush.

use std::any::Any;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use damascene_core::affine::Affine2;
use damascene_core::paint::PhysicalScissor;
use damascene_core::shader::stock_wgsl;
use damascene_core::surface::{
    AppTexture, AppTextureBackend, AppTextureId, SurfaceAlpha, SurfaceFormat, next_app_texture_id,
};
use damascene_core::tree::Rect;
use bytemuck::{Pod, Zeroable};
use vulkano::{
    buffer::{
        BufferUsage, Subbuffer,
        allocator::{SubbufferAllocator, SubbufferAllocatorCreateInfo},
    },
    descriptor_set::{
        DescriptorSet, WriteDescriptorSet, allocator::StandardDescriptorSetAllocator,
    },
    device::Device,
    format::Format,
    image::{
        Image as VkImage, ImageAspects, ImageSubresourceRange, ImageUsage,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::{ImageView, ImageViewCreateInfo},
    },
    memory::allocator::{MemoryTypeFilter, StandardMemoryAllocator},
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
};

use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::{build_shared_pipeline_layout, multisample_state};

const INSTANCE_ARENA_SIZE: u64 = 32 * 1024;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct SurfaceInstance {
    rect: [f32; 4],
    matrix: [f32; 4],
    translation: [f32; 2],
}

pub(crate) struct SurfaceRun {
    pub texture_idx: usize,
    pub scissor: Option<PhysicalScissor>,
    pub alpha: SurfaceAlpha,
    pub first: u32,
    pub count: u32,
}

struct CachedDescriptor {
    descriptor_set: Arc<DescriptorSet>,
    last_used_frame: u64,
}

pub(crate) struct SurfacePaint {
    instances: Vec<SurfaceInstance>,
    instance_alloc: SubbufferAllocator,
    instance_buf: Option<Subbuffer<[SurfaceInstance]>>,
    runs: Vec<SurfaceRun>,

    pipeline_premul: Arc<GraphicsPipeline>,
    pipeline_straight: Arc<GraphicsPipeline>,
    pipeline_opaque: Arc<GraphicsPipeline>,
    sampler: Arc<Sampler>,

    cache: HashMap<u64, CachedDescriptor>,
    bind_group_lookup: Vec<u64>,
    frame_counter: u64,

    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
}

impl SurfacePaint {
    pub(crate) fn new(
        device: Arc<Device>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
        subpass: Subpass,
        sample_count: u32,
    ) -> Self {
        let (pipeline_premul, pipeline_straight, pipeline_opaque) =
            build_surface_pipelines(device.clone(), subpass, sample_count);
        let sampler = Sampler::new(
            device,
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Linear,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: surface sampler");
        let instance_alloc = SubbufferAllocator::new(
            memory_alloc,
            SubbufferAllocatorCreateInfo {
                arena_size: INSTANCE_ARENA_SIZE,
                buffer_usage: BufferUsage::VERTEX_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );

        Self {
            instances: Vec::new(),
            instance_alloc,
            instance_buf: None,
            runs: Vec::new(),
            pipeline_premul,
            pipeline_straight,
            pipeline_opaque,
            sampler,
            cache: HashMap::new(),
            bind_group_lookup: Vec::new(),
            frame_counter: 0,
            descriptor_alloc,
        }
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.bind_group_lookup.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    pub(crate) fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        texture: &AppTexture,
        alpha: SurfaceAlpha,
        transform: Affine2,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();
        let texture_idx = self.ensure_descriptor(texture);
        let instance = SurfaceInstance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            matrix: [transform.a, transform.b, transform.c, transform.d],
            translation: [transform.tx, transform.ty],
        };
        let first = self.instances.len() as u32;
        self.instances.push(instance);
        self.runs.push(SurfaceRun {
            texture_idx,
            scissor,
            alpha,
            first,
            count: 1,
        });
        start..self.runs.len()
    }

    fn ensure_descriptor(&mut self, texture: &AppTexture) -> usize {
        let id = texture.id().0;
        if !self.cache.contains_key(&id) {
            let backend = texture.backend();
            let vk_tex = backend
                .as_any()
                .downcast_ref::<VulkanoAppTexture>()
                .unwrap_or_else(|| {
                    panic!(
                        "AppTexture passed to damascene-vulkano was not constructed by \
                         damascene_vulkano::app_texture (actual backend: {}); mixing \
                         backends in one runtime is unsupported",
                        texture.backend_name(),
                    )
                });
            let descriptor_set = DescriptorSet::new(
                self.descriptor_alloc.clone(),
                self.pipeline_premul.layout().set_layouts()[1].clone(),
                [
                    WriteDescriptorSet::image_view(0, vk_tex.view.clone()),
                    WriteDescriptorSet::sampler(1, self.sampler.clone()),
                ],
                [],
            )
            .expect("damascene-vulkano: surface descriptor set");
            self.cache.insert(
                id,
                CachedDescriptor {
                    descriptor_set,
                    last_used_frame: 0,
                },
            );
        }
        let entry = self.cache.get_mut(&id).expect("just inserted");
        entry.last_used_frame = self.frame_counter;
        if let Some(idx) = self.bind_group_lookup.iter().position(|&i| i == id) {
            idx
        } else {
            self.bind_group_lookup.push(id);
            self.bind_group_lookup.len() - 1
        }
    }

    pub(crate) fn flush(&mut self) {
        let frame = self.frame_counter;
        self.cache.retain(|_, v| v.last_used_frame == frame);

        if self.instances.is_empty() {
            self.instance_buf = None;
            return;
        }
        let buf = self
            .instance_alloc
            .allocate_slice::<SurfaceInstance>(self.instances.len() as u64)
            .expect("damascene-vulkano: surface instance suballocate");
        buf.write()
            .expect("damascene-vulkano: surface instance suballocation write")
            .copy_from_slice(&self.instances);
        self.instance_buf = Some(buf);
    }

    pub(crate) fn run(&self, index: usize) -> &SurfaceRun {
        &self.runs[index]
    }

    pub(crate) fn pipeline_for(&self, alpha: SurfaceAlpha) -> &Arc<GraphicsPipeline> {
        match alpha {
            SurfaceAlpha::Premultiplied => &self.pipeline_premul,
            SurfaceAlpha::Straight => &self.pipeline_straight,
            SurfaceAlpha::Opaque => &self.pipeline_opaque,
        }
    }

    /// Per-frame instance suballocation. Panics if called for a frame
    /// with no surface draws — bind sites are gated by
    /// `PaintItem::AppTexture`, which `record(...)` only emits when
    /// `instances` is non-empty.
    pub(crate) fn instance_buf(&self) -> &Subbuffer<[SurfaceInstance]> {
        self.instance_buf
            .as_ref()
            .expect("damascene-vulkano: surface instance_buf accessed with no draws")
    }

    pub(crate) fn descriptor_for_run(&self, run: &SurfaceRun) -> &Arc<DescriptorSet> {
        let id = self.bind_group_lookup[run.texture_idx];
        &self
            .cache
            .get(&id)
            .expect("cache entry alive for the frame")
            .descriptor_set
    }
}

fn build_surface_pipelines(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> (
    Arc<GraphicsPipeline>,
    Arc<GraphicsPipeline>,
    Arc<GraphicsPipeline>,
) {
    let words = wgsl_to_spirv("stock::surface", stock_wgsl::SURFACE)
        .expect("damascene-vulkano: surface WGSL compile");
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("damascene-vulkano: surface ShaderModule::new")
    };
    let premul = build_one(
        device.clone(),
        subpass.clone(),
        sample_count,
        &module,
        "fs_premul",
        Some(premultiplied_blend()),
    );
    let straight = build_one(
        device.clone(),
        subpass.clone(),
        sample_count,
        &module,
        "fs_straight",
        Some(premultiplied_blend()),
    );
    let opaque = build_one(
        device,
        subpass,
        sample_count,
        &module,
        "fs_opaque",
        Some(opaque_blend()),
    );
    (premul, straight, opaque)
}

fn build_one(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
    module: &Arc<ShaderModule>,
    fs_entry: &'static str,
    blend: Option<AttachmentBlend>,
) -> Arc<GraphicsPipeline> {
    let vs = module
        .entry_point("vs_main")
        .expect("surface.wgsl: missing vs_main");
    let fs = module
        .entry_point(fs_entry)
        .unwrap_or_else(|| panic!("surface.wgsl: missing {fs_entry}"));
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
        stride: std::mem::size_of::<SurfaceInstance>() as u32,
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
        // location 1: rect @ offset 0 (4*f32 = 16)
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        // location 2: affine matrix @ offset 16 (4*f32 = 16)
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        // location 3: affine translation @ offset 32 (2*f32 = 8)
        .attribute(3, attr(1, 32, Format::R32G32_SFLOAT));

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
                    blend,
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
    .expect("damascene-vulkano: surface GraphicsPipeline::new")
}

fn premultiplied_blend() -> AttachmentBlend {
    AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    }
}

fn opaque_blend() -> AttachmentBlend {
    AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::Zero,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::Zero,
        alpha_blend_op: BlendOp::Add,
    }
}

// ---- Public AppTexture constructor ----

/// Concrete vulkano-side [`AppTextureBackend`]. Holds the image + a
/// default-view + cached id so the runtime can downcast and bind the
/// view directly into a descriptor set.
#[derive(Debug)]
pub struct VulkanoAppTexture {
    /// The app-owned image. Held as `Arc` so `AppTexture` can be
    /// cheaply cloned into the El tree without releasing the GPU
    /// resource.
    pub image: Arc<VkImage>,
    /// Default 2D view over the full image, created once at
    /// construction so the per-frame record path doesn't allocate.
    pub view: Arc<ImageView>,
    id: AppTextureId,
    size: (u32, u32),
    format: SurfaceFormat,
}

impl AppTextureBackend for VulkanoAppTexture {
    fn id(&self) -> AppTextureId {
        self.id
    }
    fn size_px(&self) -> (u32, u32) {
        self.size
    }
    fn format(&self) -> SurfaceFormat {
        self.format
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Wrap an app-allocated `vulkano::image::Image` for compositing via a
/// [`damascene_core::tree::surface`] widget.
///
/// The image must have `SAMPLED` usage and one of the three supported
/// RGBA8 formats: `R8G8B8A8_SRGB`, `B8G8R8A8_SRGB`, or
/// `R8G8B8A8_UNORM`. Sample count must be 1.
///
/// # Panics
///
/// Panics if the image is missing `SAMPLED` usage, its format is outside
/// the supported set, or its sample count is not 1. These are app-side
/// mistakes, not runtime errors — fail loudly rather than silently
/// miscompositing.
pub fn app_texture(image: Arc<VkImage>) -> AppTexture {
    let format = match image.format() {
        Format::R8G8B8A8_SRGB => SurfaceFormat::Rgba8UnormSrgb,
        Format::B8G8R8A8_SRGB => SurfaceFormat::Bgra8UnormSrgb,
        Format::R8G8B8A8_UNORM => SurfaceFormat::Rgba8Unorm,
        Format::R16G16B16A16_SFLOAT => SurfaceFormat::Rgba16Float,
        f => panic!(
            "damascene_vulkano::app_texture: unsupported image format {:?} \
             (expected R8G8B8A8_SRGB / B8G8R8A8_SRGB / R8G8B8A8_UNORM / R16G16B16A16_SFLOAT)",
            f
        ),
    };
    assert!(
        image.usage().intersects(ImageUsage::SAMPLED),
        "damascene_vulkano::app_texture: source image must include SAMPLED usage (got {:?})",
        image.usage(),
    );
    let samples = image.samples();
    assert_eq!(
        samples as u32, 1,
        "damascene_vulkano::app_texture: source image must be single-sampled (got {:?})",
        samples,
    );
    let extent = image.extent();
    let size = (extent[0], extent[1]);
    let view = ImageView::new(
        image.clone(),
        ImageViewCreateInfo {
            subresource_range: ImageSubresourceRange {
                aspects: ImageAspects::COLOR,
                mip_levels: 0..1,
                array_layers: 0..1,
            },
            ..ImageViewCreateInfo::from_image(&image)
        },
    )
    .expect("damascene-vulkano: app_texture image view");
    AppTexture::from_backend(Arc::new(VulkanoAppTexture {
        image,
        view,
        id: next_app_texture_id(),
        size,
        format,
    }))
}
