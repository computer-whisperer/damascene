//! GPU compositing for app-owned [`AppTexture`]s on the ash backend.
//!
//! The host owns the Vulkan image and image view. `aetna-ash` only
//! caches descriptor sets that sample the supplied view while recording
//! Aetna's normal paint stream.

use std::any::Any;
use std::collections::HashMap;
use std::ffi::CString;
use std::ops::Range;
use std::sync::Arc;

use aetna_core::affine::Affine2;
use aetna_core::paint::PhysicalScissor;
use aetna_core::shader::stock_wgsl;
use aetna_core::surface::{
    AppTexture, AppTextureBackend, AppTextureId, SurfaceAlpha, SurfaceFormat, next_app_texture_id,
};
use aetna_core::tree::Rect;
use ash::vk;
use bytemuck::{Pod, Zeroable};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocator;

use crate::buffer::GpuBuffer;
use crate::naga_compile::wgsl_to_spirv;
use crate::runner::{Error, Result, TargetInfo};

const INITIAL_SURFACE_INSTANCE_CAPACITY: usize = 16;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct SurfaceInstance {
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
    descriptor_set: vk::DescriptorSet,
    last_used_frame: u64,
}

pub(crate) struct SurfacePaint {
    instances: Vec<SurfaceInstance>,
    instance_buf: GpuBuffer,
    instance_capacity: usize,
    runs: Vec<SurfaceRun>,
    texture_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline_premul: vk::Pipeline,
    pipeline_straight: vk::Pipeline,
    pipeline_opaque: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    cache: HashMap<u64, CachedDescriptor>,
    bind_lookup: Vec<u64>,
    frame_counter: u64,
}

impl SurfacePaint {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        frame_set_layout: vk::DescriptorSetLayout,
        target: TargetInfo,
    ) -> Result<Self> {
        let texture_set_layout = create_texture_set_layout(device)?;
        let layouts = [frame_set_layout, texture_set_layout];
        let pipeline_layout = create_pipeline_layout(device, &layouts)?;
        let pipeline_premul = build_surface_pipeline(
            device,
            pipeline_layout,
            target,
            "stock::surface::premul",
            "fs_premul",
            SurfaceAlpha::Premultiplied,
        )?;
        let pipeline_straight = build_surface_pipeline(
            device,
            pipeline_layout,
            target,
            "stock::surface::straight",
            "fs_straight",
            SurfaceAlpha::Straight,
        )?;
        let pipeline_opaque = build_surface_pipeline(
            device,
            pipeline_layout,
            target,
            "stock::surface::opaque",
            "fs_opaque",
            SurfaceAlpha::Opaque,
        )?;
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
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(256)
            .pool_sizes(&pool_sizes);
        let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None) }?;
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        let sampler = unsafe { device.create_sampler(&sampler_info, None) }?;
        let instance_capacity = INITIAL_SURFACE_INSTANCE_CAPACITY;
        let instance_buf = GpuBuffer::new(
            device,
            allocator,
            "aetna_ash::surface_instances",
            (instance_capacity * std::mem::size_of::<SurfaceInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        Ok(Self {
            instances: Vec::new(),
            instance_buf,
            instance_capacity,
            runs: Vec::new(),
            texture_set_layout,
            pipeline_layout,
            pipeline_premul,
            pipeline_straight,
            pipeline_opaque,
            descriptor_pool,
            sampler,
            cache: HashMap::new(),
            bind_lookup: Vec::new(),
            frame_counter: 0,
        })
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.bind_lookup.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    pub(crate) fn record(
        &mut self,
        device: &ash::Device,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        texture: &AppTexture,
        alpha: SurfaceAlpha,
        transform: Affine2,
    ) -> Result<Range<usize>> {
        let start = self.runs.len();
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return Ok(start..start);
        }
        let texture_idx = self.ensure_descriptor(device, texture)?;
        let first = self.instances.len() as u32;
        self.instances.push(SurfaceInstance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            matrix: [transform.a, transform.b, transform.c, transform.d],
            translation: [transform.tx, transform.ty],
        });
        self.runs.push(SurfaceRun {
            texture_idx,
            scissor,
            alpha,
            first,
            count: 1,
        });
        Ok(start..self.runs.len())
    }

    pub(crate) fn flush(&mut self, device: &ash::Device, allocator: &mut Allocator) -> Result<()> {
        let mut stale = Vec::new();
        self.cache.retain(|_, cached| {
            let keep = cached.last_used_frame == self.frame_counter;
            if !keep {
                stale.push(cached.descriptor_set);
            }
            keep
        });
        if !stale.is_empty() {
            unsafe {
                device.free_descriptor_sets(self.descriptor_pool, &stale)?;
            }
        }
        self.ensure_instance_capacity(device, allocator)?;
        self.instance_buf
            .write_bytes(bytemuck::cast_slice(&self.instances))?;
        Ok(())
    }

    fn ensure_descriptor(&mut self, device: &ash::Device, texture: &AppTexture) -> Result<usize> {
        let backend = texture
            .backend()
            .as_any()
            .downcast_ref::<AshAppTexture>()
            .unwrap_or_else(|| {
                panic!(
                    "aetna-ash expected AshAppTexture, got {}",
                    texture.backend_name()
                )
            });
        let id = backend.id.0;
        if !self.cache.contains_key(&id) {
            let cached = self.create_descriptor(device, backend)?;
            self.cache.insert(id, cached);
        }
        self.cache
            .get_mut(&id)
            .expect("just inserted")
            .last_used_frame = self.frame_counter;
        if let Some(idx) = self.bind_lookup.iter().position(|&h| h == id) {
            Ok(idx)
        } else {
            self.bind_lookup.push(id);
            Ok(self.bind_lookup.len() - 1)
        }
    }

    fn create_descriptor(
        &self,
        device: &ash::Device,
        texture: &AshAppTexture,
    ) -> Result<CachedDescriptor> {
        let set_layouts = [self.texture_set_layout];
        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&set_layouts);
        let descriptor_set = unsafe { device.allocate_descriptor_sets(&allocate_info) }?[0];
        let image_info = vk::DescriptorImageInfo::default()
            .image_view(texture.view)
            .image_layout(texture.image_layout);
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
        Ok(CachedDescriptor {
            descriptor_set,
            last_used_frame: self.frame_counter,
        })
    }

    fn ensure_instance_capacity(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
    ) -> Result<()> {
        if self.instances.len() <= self.instance_capacity {
            return Ok(());
        }
        let mut next = self.instance_capacity.max(1);
        while next < self.instances.len() {
            next *= 2;
        }
        unsafe {
            self.instance_buf.destroy(device, allocator);
        }
        self.instance_buf = GpuBuffer::new(
            device,
            allocator,
            "aetna_ash::surface_instances",
            (next * std::mem::size_of::<SurfaceInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        self.instance_capacity = next;
        Ok(())
    }

    pub(crate) fn run(&self, index: usize) -> &SurfaceRun {
        &self.runs[index]
    }

    pub(crate) fn pipeline_for(&self, alpha: SurfaceAlpha) -> vk::Pipeline {
        match alpha {
            SurfaceAlpha::Premultiplied => self.pipeline_premul,
            SurfaceAlpha::Straight => self.pipeline_straight,
            SurfaceAlpha::Opaque => self.pipeline_opaque,
        }
    }

    pub(crate) fn pipeline_layout(&self) -> vk::PipelineLayout {
        self.pipeline_layout
    }

    pub(crate) fn instance_buffer(&self) -> vk::Buffer {
        self.instance_buf.buffer
    }

    pub(crate) fn descriptor_for_run(&self, run: &SurfaceRun) -> vk::DescriptorSet {
        let id = self.bind_lookup[run.texture_idx];
        self.cache
            .get(&id)
            .expect("cache entry alive for frame")
            .descriptor_set
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            self.instance_buf.destroy(device, allocator);
            device.destroy_pipeline(self.pipeline_premul, None);
            device.destroy_pipeline(self.pipeline_straight, None);
            device.destroy_pipeline(self.pipeline_opaque, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.texture_set_layout, None);
        }
    }
}

/// Host-owned ash texture that Aetna can sample during paint.
///
/// The image must be a single-sampled 2D image created with
/// `vk::ImageUsageFlags::SAMPLED`, and the host must keep both the
/// image and view alive while this wrapper may be painted. Before Aetna
/// draw commands execute, the host must transition the image to the
/// same readable layout recorded in this wrapper.
#[derive(Clone, Copy, Debug)]
pub struct AshAppTexture {
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub image_layout: vk::ImageLayout,
    id: AppTextureId,
    size: (u32, u32),
    format: SurfaceFormat,
}

/// Wrap a host-owned Vulkan image/view pair as an Aetna app texture.
///
/// `aetna-ash` cannot inspect raw ash image creation flags, so this
/// constructor validates only the format and extent. The host remains
/// responsible for usage flags, sample count, synchronization, layout,
/// and lifetime. This shortcut assumes the image will be in
/// `SHADER_READ_ONLY_OPTIMAL` when Aetna samples it; use
/// [`app_texture_with_layout`] if your frame graph leaves the image in
/// another shader-readable layout such as `GENERAL`.
pub fn app_texture(
    image: vk::Image,
    view: vk::ImageView,
    format: vk::Format,
    extent: vk::Extent2D,
) -> AppTexture {
    app_texture_with_layout(
        image,
        view,
        format,
        extent,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    )
}

/// Wrap a host-owned Vulkan image/view pair with an explicit sampled
/// image layout.
///
/// The supplied layout is written into the descriptor set. The host must
/// make sure the image is actually in that layout when Aetna draw
/// commands execute.
pub fn app_texture_with_layout(
    image: vk::Image,
    view: vk::ImageView,
    format: vk::Format,
    extent: vk::Extent2D,
    image_layout: vk::ImageLayout,
) -> AppTexture {
    let surface_format = match format {
        vk::Format::R8G8B8A8_SRGB => SurfaceFormat::Rgba8UnormSrgb,
        vk::Format::B8G8R8A8_SRGB => SurfaceFormat::Bgra8UnormSrgb,
        vk::Format::R8G8B8A8_UNORM => SurfaceFormat::Rgba8Unorm,
        vk::Format::R16G16B16A16_SFLOAT => SurfaceFormat::Rgba16Float,
        _ => panic!("unsupported aetna-ash app texture format: {format:?}"),
    };
    assert!(
        extent.width > 0 && extent.height > 0,
        "aetna-ash app textures must have non-zero extent"
    );
    AppTexture::from_backend(Arc::new(AshAppTexture {
        image,
        view,
        image_layout,
        id: next_app_texture_id(),
        size: (extent.width, extent.height),
        format: surface_format,
    }))
}

impl AppTextureBackend for AshAppTexture {
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

fn create_texture_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout> {
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

fn build_surface_pipeline(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    name: &str,
    fragment_entry: &str,
    alpha: SurfaceAlpha,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv(name, stock_wgsl::SURFACE)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_pipeline_with_module(
        device,
        pipeline_layout,
        target,
        shader,
        name,
        fragment_entry,
        alpha,
    );
    unsafe {
        device.destroy_shader_module(shader, None);
    }
    result
}

fn build_pipeline_with_module(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
    shader: vk::ShaderModule,
    name: &str,
    fragment_entry: &str,
    alpha: SurfaceAlpha,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("static string has no nul");
    let fs_main = CString::new(fragment_entry).expect("static fragment entry has no nul");
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
            stride: std::mem::size_of::<SurfaceInstance>() as u32,
            input_rate: vk::VertexInputRate::INSTANCE,
        },
    ];
    let attrs = [
        attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
        attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
        attr(3, 1, 32, vk::Format::R32G32_SFLOAT),
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
    let blend_attachment = blend_attachment(alpha);
    let blend_attachments = [blend_attachment];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats = [target.format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&color_formats);
    let info = vk::GraphicsPipelineCreateInfo::default()
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
    let pipelines =
        unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) }
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

fn blend_attachment(alpha: SurfaceAlpha) -> vk::PipelineColorBlendAttachmentState {
    let (src, dst) = match alpha {
        SurfaceAlpha::Premultiplied | SurfaceAlpha::Straight => {
            (vk::BlendFactor::ONE, vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        }
        SurfaceAlpha::Opaque => (vk::BlendFactor::ONE, vk::BlendFactor::ZERO),
    };
    vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(true)
        .src_color_blend_factor(src)
        .dst_color_blend_factor(dst)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(src)
        .dst_alpha_blend_factor(dst)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(
            vk::ColorComponentFlags::R
                | vk::ColorComponentFlags::G
                | vk::ColorComponentFlags::B
                | vk::ColorComponentFlags::A,
        )
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
