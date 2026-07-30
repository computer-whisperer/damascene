use std::ffi::CString;
use std::ops::Range;

use ash::vk;
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
    IconMaterial, VectorAsset, VectorMeshOptions, VectorMeshVertex, VectorRenderMode,
    append_vector_asset_mesh,
};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocator;

use crate::buffer::{GpuBuffer, GpuImage};
use crate::naga_compile::wgsl_to_spirv;
use crate::runner::{Error, Result, TargetInfo};

const INITIAL_ICON_INSTANCE_CAPACITY: usize = 256;
const MAX_RETIRED_UPLOADS: usize = 128;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct MsdfIconInstance {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
}

struct PageGpu {
    image: GpuImage,
    descriptor_set: vk::DescriptorSet,
    layout: vk::ImageLayout,
}

struct PendingUpload {
    page: usize,
    rect: [u32; 4],
    staging: GpuBuffer,
}

pub(crate) struct IconPaint {
    tess_vertices: Vec<VectorMeshVertex>,
    tess_vertex_buf: GpuBuffer,
    tess_capacity: usize,
    tess_pipeline_layout: vk::PipelineLayout,
    flat_pipeline: vk::Pipeline,
    relief_pipeline: vk::Pipeline,
    glass_pipeline: vk::Pipeline,
    msdf_atlas: IconMsdfAtlas,
    msdf_pages: Vec<PageGpu>,
    msdf_instances: Vec<MsdfIconInstance>,
    msdf_instance_buf: GpuBuffer,
    msdf_capacity: usize,
    atlas_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    runs: Vec<IconRun>,
    material: IconMaterial,
    pending_uploads: Vec<PendingUpload>,
    retired_uploads: Vec<GpuBuffer>,
    /// Working color space MSDF icon mask colors are converted into. Kept
    /// in sync with the owning `Runner` via `set_working_color_space`.
    working_color_space: ColorSpace,
}

impl IconPaint {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        frame_set_layout: vk::DescriptorSetLayout,
        target: TargetInfo,
    ) -> Result<Self> {
        let atlas_set_layout = create_atlas_set_layout(device)?;
        let tess_pipeline_layout = create_pipeline_layout(device, &[frame_set_layout])?;
        let flat_pipeline = build_tess_pipeline(
            device,
            tess_pipeline_layout,
            target,
            "stock::vector",
            stock_wgsl::VECTOR,
        )?;
        let relief_pipeline = build_tess_pipeline(
            device,
            tess_pipeline_layout,
            target,
            "stock::vector_relief",
            stock_wgsl::VECTOR_RELIEF,
        )?;
        let glass_pipeline = build_tess_pipeline(
            device,
            tess_pipeline_layout,
            target,
            "stock::vector_glass",
            stock_wgsl::VECTOR_GLASS,
        )?;
        let pipeline_layout =
            create_pipeline_layout(device, &[frame_set_layout, atlas_set_layout])?;
        let pipeline = build_msdf_pipeline(device, pipeline_layout, target)?;
        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: 256,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 256,
            },
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(256)
            .pool_sizes(&pool_sizes);
        let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None) }?;
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        let sampler = unsafe { device.create_sampler(&sampler_info, None) }?;
        let msdf_capacity = INITIAL_ICON_INSTANCE_CAPACITY;
        let msdf_instance_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::icon_msdf_instances",
            (msdf_capacity * std::mem::size_of::<MsdfIconInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        let tess_capacity = INITIAL_ICON_INSTANCE_CAPACITY * 6;
        let tess_vertex_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::icon_tess_vertices",
            (tess_capacity * std::mem::size_of::<VectorMeshVertex>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;

        Ok(Self {
            tess_vertices: Vec::new(),
            tess_vertex_buf,
            tess_capacity,
            tess_pipeline_layout,
            flat_pipeline,
            relief_pipeline,
            glass_pipeline,
            msdf_atlas: IconMsdfAtlas::new(DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD),
            msdf_pages: Vec::new(),
            msdf_instances: Vec::new(),
            msdf_instance_buf,
            msdf_capacity,
            atlas_set_layout,
            pipeline_layout,
            pipeline,
            descriptor_pool,
            sampler,
            runs: Vec::new(),
            material: IconMaterial::Flat,
            pending_uploads: Vec::new(),
            retired_uploads: Vec::new(),
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        })
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
    }

    pub(crate) fn can_record_msdf(&self, source: &IconSource) -> bool {
        matches!(self.material, IconMaterial::Flat)
            && matches!(source.paint_mode(), SvgIconPaintMode::CurrentColorMask)
    }

    pub(crate) fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &IconSource,
        color: Color,
        stroke_width: f32,
    ) -> Range<usize> {
        let start = self.runs.len();
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return start..start;
        }

        if self.can_record_msdf(source)
            && let Some(slot) = self.msdf_atlas.ensure(source, stroke_width)
        {
            let (page_w, page_h) = self.msdf_page_dims(slot.page);
            let first = self.msdf_instances.len() as u32;
            self.msdf_instances.push(msdf_instance_for_icon(
                rect,
                color,
                &slot,
                page_w,
                page_h,
                self.working_color_space,
            ));
            self.runs.push(IconRun {
                kind: IconRunKind::Msdf,
                scissor,
                first,
                count: 1,
                page: slot.page,
                material: IconMaterial::Flat,
            });
        } else {
            let material = self.material;
            let first = self.tess_vertices.len() as u32;
            let mesh_run = append_vector_asset_mesh(
                source.vector_asset(),
                VectorMeshOptions::icon(rect, color, stroke_width, self.working_color_space),
                &mut self.tess_vertices,
                None,
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

    pub(crate) fn record_vector(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        asset: &VectorAsset,
        render_mode: VectorRenderMode,
    ) -> Range<usize> {
        let start = self.runs.len();
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return start..start;
        }

        match render_mode {
            VectorRenderMode::Mask { color } => {
                if let Some(slot) = self.msdf_atlas.ensure_vector_asset(asset) {
                    let (page_w, page_h) = self.msdf_page_dims(slot.page);
                    let first = self.msdf_instances.len() as u32;
                    self.msdf_instances.push(msdf_instance_for_icon(
                        rect,
                        color,
                        &slot,
                        page_w,
                        page_h,
                        self.working_color_space,
                    ));
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
                    None,
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

    pub(crate) fn flush(&mut self, device: &ash::Device, allocator: &mut Allocator) -> Result<()> {
        while self.retired_uploads.len() > MAX_RETIRED_UPLOADS {
            let mut staging = self.retired_uploads.remove(0);
            unsafe {
                staging.destroy(device, allocator);
            }
        }

        while self.msdf_pages.len() < self.msdf_atlas.pages().len() {
            let i = self.msdf_pages.len();
            let page = &self.msdf_atlas.pages()[i];
            let new_page = self.create_page(device, allocator, page.width, page.height)?;
            self.msdf_pages.push(new_page);
        }

        for (page_idx, rect) in self.msdf_atlas.take_dirty() {
            if rect.w == 0 || rect.h == 0 {
                continue;
            }
            let page = &self.msdf_atlas.pages()[page_idx];
            let bytes = pack_rect_bytes(page, rect);
            self.queue_upload(
                device,
                allocator,
                page_idx,
                [rect.x, rect.y, rect.w, rect.h],
                &bytes,
            )?;
        }

        self.ensure_tess_capacity(device, allocator)?;
        self.tess_vertex_buf
            .write_bytes(bytemuck::cast_slice(&self.tess_vertices))?;
        self.ensure_msdf_capacity(device, allocator)?;
        self.msdf_instance_buf
            .write_bytes(bytemuck::cast_slice(&self.msdf_instances))?;
        Ok(())
    }

    pub(crate) unsafe fn record_pending_uploads(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
    ) -> Result<()> {
        for upload in self.pending_uploads.drain(..) {
            let page = &mut self.msdf_pages[upload.page];
            unsafe {
                transition_image(
                    device,
                    cmd,
                    page.image.image,
                    page.layout,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                );
                let region = vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_offset(vk::Offset3D {
                        x: upload.rect[0] as i32,
                        y: upload.rect[1] as i32,
                        z: 0,
                    })
                    .image_extent(vk::Extent3D {
                        width: upload.rect[2],
                        height: upload.rect[3],
                        depth: 1,
                    });
                device.cmd_copy_buffer_to_image(
                    cmd,
                    upload.staging.buffer,
                    page.image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
                transition_image(
                    device,
                    cmd,
                    page.image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
            page.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            self.retired_uploads.push(upload.staging);
        }
        Ok(())
    }

    fn create_page(
        &self,
        device: &ash::Device,
        allocator: &mut Allocator,
        width: u32,
        height: u32,
    ) -> Result<PageGpu> {
        let image = GpuImage::new(
            device,
            allocator,
            "damascene_ash::icon_msdf_page",
            vk::Format::R8G8B8A8_UNORM,
            vk::Extent2D { width, height },
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        )?;
        let set_layouts = [self.atlas_set_layout];
        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&set_layouts);
        let descriptor_set = unsafe { device.allocate_descriptor_sets(&allocate_info) }?[0];
        let image_info = vk::DescriptorImageInfo::default()
            .image_view(image.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let sampler_info = vk::DescriptorImageInfo::default().sampler(self.sampler);
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(std::slice::from_ref(&image_info)),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .image_info(std::slice::from_ref(&sampler_info)),
        ];
        unsafe {
            device.update_descriptor_sets(&writes, &[]);
        }
        Ok(PageGpu {
            image,
            descriptor_set,
            layout: vk::ImageLayout::UNDEFINED,
        })
    }

    fn queue_upload(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        page: usize,
        rect: [u32; 4],
        bytes: &[u8],
    ) -> Result<()> {
        let mut staging = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::icon_msdf_staging",
            bytes.len() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write_bytes(bytes)?;
        self.pending_uploads.push(PendingUpload {
            page,
            rect,
            staging,
        });
        Ok(())
    }

    fn ensure_tess_capacity(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
    ) -> Result<()> {
        if self.tess_vertices.len() <= self.tess_capacity {
            return Ok(());
        }
        let mut next = self.tess_capacity.max(1);
        while next < self.tess_vertices.len() {
            next *= 2;
        }
        unsafe {
            self.tess_vertex_buf.destroy(device, allocator);
        }
        self.tess_vertex_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::icon_tess_vertices",
            (next * std::mem::size_of::<VectorMeshVertex>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        self.tess_capacity = next;
        Ok(())
    }

    fn ensure_msdf_capacity(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
    ) -> Result<()> {
        if self.msdf_instances.len() <= self.msdf_capacity {
            return Ok(());
        }
        let mut next = self.msdf_capacity.max(1);
        while next < self.msdf_instances.len() {
            next *= 2;
        }
        unsafe {
            self.msdf_instance_buf.destroy(device, allocator);
        }
        self.msdf_instance_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::icon_msdf_instances",
            (next * std::mem::size_of::<MsdfIconInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        self.msdf_capacity = next;
        Ok(())
    }

    fn msdf_page_dims(&self, page_idx: u32) -> (u32, u32) {
        let page = self
            .msdf_atlas
            .page(page_idx)
            .expect("freshly ensured icon slot references a missing atlas page");
        (page.width, page.height)
    }

    pub(crate) fn run(&self, index: usize) -> IconRun {
        self.runs[index]
    }

    pub(crate) fn pipeline(&self) -> vk::Pipeline {
        self.pipeline
    }

    pub(crate) fn tess_pipeline(&self, material: IconMaterial) -> vk::Pipeline {
        match material {
            IconMaterial::Flat => self.flat_pipeline,
            IconMaterial::Relief => self.relief_pipeline,
            IconMaterial::Glass => self.glass_pipeline,
        }
    }

    pub(crate) fn pipeline_layout(&self) -> vk::PipelineLayout {
        self.pipeline_layout
    }

    pub(crate) fn tess_pipeline_layout(&self) -> vk::PipelineLayout {
        self.tess_pipeline_layout
    }

    pub(crate) fn instance_buffer(&self) -> vk::Buffer {
        self.msdf_instance_buf.buffer
    }

    pub(crate) fn tess_vertex_buffer(&self) -> vk::Buffer {
        self.tess_vertex_buf.buffer
    }

    pub(crate) fn page_descriptor(&self, page: u32) -> vk::DescriptorSet {
        self.msdf_pages[page as usize].descriptor_set
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            for mut upload in self.pending_uploads.drain(..) {
                upload.staging.destroy(device, allocator);
            }
            for mut staging in self.retired_uploads.drain(..) {
                staging.destroy(device, allocator);
            }
            for page in &mut self.msdf_pages {
                page.image.destroy(device, allocator);
            }
            self.tess_vertex_buf.destroy(device, allocator);
            self.msdf_instance_buf.destroy(device, allocator);
            device.destroy_pipeline(self.flat_pipeline, None);
            device.destroy_pipeline(self.relief_pipeline, None);
            device.destroy_pipeline(self.glass_pipeline, None);
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.tess_pipeline_layout, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.atlas_set_layout, None);
        }
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

fn create_atlas_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout> {
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
    ];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    unsafe { device.create_descriptor_set_layout(&info, None) }.map_err(Into::into)
}

fn create_pipeline_layout(
    device: &ash::Device,
    layouts: &[vk::DescriptorSetLayout],
) -> Result<vk::PipelineLayout> {
    let info = vk::PipelineLayoutCreateInfo::default().set_layouts(layouts);
    unsafe { device.create_pipeline_layout(&info, None) }.map_err(Into::into)
}

fn build_tess_pipeline(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    wgsl: &str,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv(name, wgsl)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_tess_pipeline_with_module(device, pipeline_layout, target, name, shader);
    unsafe {
        device.destroy_shader_module(shader, None);
    }
    result
}

fn build_tess_pipeline_with_module(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    shader: vk::ShaderModule,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("static string has no nul");
    let fs_main = CString::new("fs_main").expect("static string has no nul");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(shader)
            .name(&vs_main),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(shader)
            .name(&fs_main),
    ];
    let bindings = [vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<VectorMeshVertex>() as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    }];
    let attrs = [
        attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 0, 8, vk::Format::R32G32_SFLOAT),
        attr(2, 0, 16, vk::Format::R32G32B32A32_SFLOAT),
        attr(3, 0, 32, vk::Format::R32G32B32A32_SFLOAT),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attrs);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::empty())
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(target.sample_count)
        .sample_shading_enable(target.sample_count != vk::SampleCountFlags::TYPE_1)
        .min_sample_shading(1.0);
    let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        );
    let blend_attachments = [blend_attachment];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats = [target.format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&color_formats);
    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(pipeline_layout)
        .push_next(&mut rendering);
    let pipelines = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
    }
    .map_err(|(_pipelines, err)| Error::Vulkan {
        op: "create_graphics_pipelines",
        result: err,
    })?;
    pipelines
        .into_iter()
        .next()
        .ok_or(Error::PipelineCreationReturnedEmpty {
            name: name.to_string(),
        })
}

fn build_msdf_pipeline(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv("stock::text_msdf", stock_wgsl::TEXT_MSDF)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_msdf_pipeline_with_module(device, pipeline_layout, target, shader);
    unsafe {
        device.destroy_shader_module(shader, None);
    }
    result
}

fn build_msdf_pipeline_with_module(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    shader: vk::ShaderModule,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("static string has no nul");
    let fs_main = CString::new("fs_main").expect("static string has no nul");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(shader)
            .name(&vs_main),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(shader)
            .name(&fs_main),
    ];
    let bindings = [
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: (2 * std::mem::size_of::<f32>()) as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        },
        vk::VertexInputBindingDescription {
            binding: 1,
            stride: std::mem::size_of::<MsdfIconInstance>() as u32,
            input_rate: vk::VertexInputRate::INSTANCE,
        },
    ];
    let attrs = [
        attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
        attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
        attr(3, 1, 32, vk::Format::R32G32B32A32_SFLOAT),
        attr(4, 1, 48, vk::Format::R32G32B32A32_SFLOAT),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attrs);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_STRIP);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::empty())
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(target.sample_count)
        .sample_shading_enable(target.sample_count != vk::SampleCountFlags::TYPE_1)
        .min_sample_shading(1.0);
    let blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        );
    let blend_attachments = [blend_attachment];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats = [target.format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&color_formats);
    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(pipeline_layout)
        .push_next(&mut rendering);
    let pipelines = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
    }
    .map_err(|(_pipelines, err)| Error::Vulkan {
        op: "create_graphics_pipelines",
        result: err,
    })?;
    pipelines
        .into_iter()
        .next()
        .ok_or(Error::PipelineCreationReturnedEmpty {
            name: "stock::text_msdf".to_string(),
        })
}

fn attr(
    location: u32,
    binding: u32,
    offset: u32,
    format: vk::Format,
) -> vk::VertexInputAttributeDescription {
    vk::VertexInputAttributeDescription {
        location,
        binding,
        format,
        offset,
    }
}

unsafe fn transition_image(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    if old_layout == new_layout {
        return;
    }
    let (src_access, src_stage) = match old_layout {
        vk::ImageLayout::UNDEFINED => (
            vk::AccessFlags::empty(),
            vk::PipelineStageFlags::TOP_OF_PIPE,
        ),
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => (
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    let (dst_access, dst_stage) = match new_layout {
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => (
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1),
        );
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }
}
