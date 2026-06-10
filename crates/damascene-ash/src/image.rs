use std::collections::HashMap;
use std::ffi::CString;
use std::ops::Range;

use ash::vk;
use bytemuck::{Pod, Zeroable};
use damascene_core::color::ColorSpace;
use damascene_core::image::Image as RasterImage;
use damascene_core::paint::{DEFAULT_WORKING_COLOR_SPACE, PhysicalScissor, rgba_f32_in};
use damascene_core::shader::stock_wgsl;
use damascene_core::tree::{Color, Corners, Rect};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocator;

use crate::buffer::{GpuBuffer, GpuImage};
use crate::naga_compile::wgsl_to_spirv;
use crate::runner::{Error, Result, TargetInfo};

const INITIAL_IMAGE_INSTANCE_CAPACITY: usize = 64;
const MAX_RETIRED_UPLOADS: usize = 128;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct ImageInstance {
    rect: [f32; 4],
    tint: [f32; 4],
    params: [f32; 4],
    uv: [f32; 4],
    /// `(content peak, resolved luminance limit)` in working-space
    /// units — the shader remasters (BT.2390) when peak > limit.
    range: [f32; 2],
}

pub(crate) struct ImageRun {
    pub texture_idx: usize,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

pub(crate) struct ImageRecord<'a> {
    pub rect: Rect,
    pub scissor: Option<PhysicalScissor>,
    pub image: &'a RasterImage,
    pub tint: Option<Color>,
    pub radius: Corners,
    pub range_limit: damascene_core::image::DynamicRangeLimit,
}

struct CachedTexture {
    image: GpuImage,
    descriptor_set: vk::DescriptorSet,
    layout: vk::ImageLayout,
    /// Measured content peak (max linear RGB channel, working-space
    /// units) — the image's effective MaxCLL, computed once at upload.
    /// `1.0` for the 8-bit sRGB fast path by construction.
    peak: f32,
    last_used_frame: u64,
}

struct PendingUpload {
    hash: u64,
    width: u32,
    height: u32,
    staging: GpuBuffer,
}

pub(crate) struct ImagePaint {
    instances: Vec<ImageInstance>,
    instance_buf: GpuBuffer,
    instance_capacity: usize,
    runs: Vec<ImageRun>,
    texture_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    cache: HashMap<u64, CachedTexture>,
    bind_lookup: Vec<u64>,
    frame_counter: u64,
    /// Output luminance headroom (multiples of reference white; 1.0 on
    /// SDR) each draw's `DynamicRangeLimit` resolves against. Kept in
    /// sync with the owning `Runner` via `set_output_luminance`.
    headroom: f32,
    /// Working color space image tints are converted into at record
    /// time. Kept in sync with the owning `Runner` via
    /// `set_working_color_space`.
    working_color_space: ColorSpace,
    /// Device `max_image_dimension2_d` — images larger than this fail
    /// `vkCreateImage` validation, so oversized sources are downscaled
    /// to fit before upload (issue #78).
    max_image_dim: u32,
    pending_uploads: Vec<PendingUpload>,
    retired_uploads: Vec<GpuBuffer>,
}

impl ImagePaint {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        frame_set_layout: vk::DescriptorSetLayout,
        target: TargetInfo,
        max_image_dim: u32,
    ) -> Result<Self> {
        let texture_set_layout = create_texture_set_layout(device)?;
        let layouts = [frame_set_layout, texture_set_layout];
        let pipeline_layout = create_pipeline_layout(device, &layouts)?;
        let pipeline = build_image_pipeline(device, pipeline_layout, target)?;
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
        let instance_capacity = INITIAL_IMAGE_INSTANCE_CAPACITY;
        let instance_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::image_instances",
            (instance_capacity * std::mem::size_of::<ImageInstance>()) as vk::DeviceSize,
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
            pipeline,
            descriptor_pool,
            sampler,
            cache: HashMap::new(),
            bind_lookup: Vec::new(),
            frame_counter: 0,
            headroom: 1.0,
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
            max_image_dim,
            pending_uploads: Vec::new(),
            retired_uploads: Vec::new(),
        })
    }

    /// Update the working color space subsequent tint packing converts
    /// into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    /// Update the output headroom subsequent draws resolve their
    /// `DynamicRangeLimit` against. Called by
    /// `Runner::set_output_luminance`.
    pub(crate) fn set_headroom(&mut self, headroom: f32) {
        self.headroom = headroom;
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
        allocator: &mut Allocator,
        record: ImageRecord<'_>,
    ) -> Result<Range<usize>> {
        let ImageRecord {
            rect,
            scissor,
            image,
            tint,
            radius,
            range_limit,
        } = record;
        let start = self.runs.len();
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return Ok(start..start);
        }
        let (texture_idx, peak) = self.ensure_texture(device, allocator, image)?;
        let first = self.instances.len() as u32;
        self.instances.push(ImageInstance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            tint: tint
                .map(|c| rgba_f32_in(c, self.working_color_space))
                .unwrap_or([1.0, 1.0, 1.0, 1.0]),
            params: [
                radius.tl.max(0.0),
                radius.tr.max(0.0),
                radius.br.max(0.0),
                radius.bl.max(0.0),
            ],
            uv: [0.0, 0.0, 1.0, 1.0],
            range: [peak, range_limit.resolve(self.headroom)],
        });
        self.runs.push(ImageRun {
            texture_idx,
            scissor,
            first,
            count: 1,
        });
        Ok(start..self.runs.len())
    }

    pub(crate) fn flush(&mut self, device: &ash::Device, allocator: &mut Allocator) -> Result<()> {
        // Evict textures untouched this frame — same policy as the
        // wgpu/vulkano backends and this crate's surface cache. Without
        // it the 256-set descriptor pool exhausts after 256 distinct
        // images over the app lifetime. Immediate destruction is safe
        // under `Runner::prepare`'s synchronization contract (prior
        // submissions have retired before prepare runs).
        let frame = self.frame_counter;
        let stale_keys: Vec<u64> = self
            .cache
            .iter()
            .filter(|(_, c)| c.last_used_frame != frame)
            .map(|(k, _)| *k)
            .collect();
        if !stale_keys.is_empty() {
            let mut stale_sets = Vec::with_capacity(stale_keys.len());
            for key in stale_keys {
                if let Some(mut cached) = self.cache.remove(&key) {
                    stale_sets.push(cached.descriptor_set);
                    unsafe {
                        cached.image.destroy(device, allocator);
                    }
                }
            }
            unsafe {
                device.free_descriptor_sets(self.descriptor_pool, &stale_sets)?;
            }
        }
        while self.retired_uploads.len() > MAX_RETIRED_UPLOADS {
            let mut staging = self.retired_uploads.remove(0);
            unsafe {
                staging.destroy(device, allocator);
            }
        }
        self.ensure_instance_capacity(device, allocator)?;
        self.instance_buf
            .write_bytes(bytemuck::cast_slice(&self.instances))?;
        Ok(())
    }

    pub(crate) unsafe fn record_pending_uploads(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
    ) {
        for upload in self.pending_uploads.drain(..) {
            let Some(texture) = self.cache.get_mut(&upload.hash) else {
                // The texture was evicted before its upload recorded;
                // retire the staging buffer instead of leaking it.
                self.retired_uploads.push(upload.staging);
                continue;
            };
            unsafe {
                transition_image(
                    device,
                    cmd,
                    texture.image.image,
                    texture.layout,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                );
                let region = vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width: upload.width,
                        height: upload.height,
                        depth: 1,
                    });
                device.cmd_copy_buffer_to_image(
                    cmd,
                    upload.staging.buffer,
                    texture.image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[region],
                );
                transition_image(
                    device,
                    cmd,
                    texture.image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
            texture.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
            self.retired_uploads.push(upload.staging);
        }
    }

    /// Returns an index into the per-frame `bind_lookup` table, plus
    /// the image's measured content peak (cached with the texture).
    fn ensure_texture(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        image: &RasterImage,
    ) -> Result<(usize, f32)> {
        let hash = image.content_hash();
        if !self.cache.contains_key(&hash) {
            let cached = self.create_texture(device, allocator, image)?;
            self.cache.insert(hash, cached);
        }
        let entry = self.cache.get_mut(&hash).expect("just inserted");
        entry.last_used_frame = self.frame_counter;
        let peak = entry.peak;
        let idx = if let Some(idx) = self.bind_lookup.iter().position(|&h| h == hash) {
            idx
        } else {
            self.bind_lookup.push(hash);
            self.bind_lookup.len() - 1
        };
        Ok((idx, peak))
    }

    fn create_texture(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        image: &RasterImage,
    ) -> Result<CachedTexture> {
        // The pending upload is keyed on the *original* image's hash —
        // the same key `ensure_texture` inserted into the cache — so
        // capture it before fitting may replace `image` below.
        let upload_hash = image.content_hash();
        // Fit to the device texture limit first (a cheap clone when it
        // already fits); oversized images come back downscaled in the
        // scRGB f16 path (issue #78).
        let image = image.downscaled_to_fit(self.max_image_dim);
        let width = image.width();
        let height = image.height();
        // Same convention as the wgpu side: 8-bit sRGB art uploads
        // as-is and the sampler decodes to linear at sample time;
        // wide-gamut / HDR / deep sources normalize on the CPU to
        // scRGB f16 (see `damascene_core::image` module docs).
        let scrgb: Option<(Vec<u8>, f32)> = (!image.is_srgb8()).then(|| {
            let (bits, peak) = image.to_scrgb_f16_with_peak();
            let bytes = bits.iter().flat_map(|v| v.to_ne_bytes()).collect();
            (bytes, peak)
        });
        let (format, data, peak) = match &scrgb {
            // 8-bit sRGB peaks at reference white by construction.
            None => (vk::Format::R8G8B8A8_SRGB, image.pixels(), 1.0),
            Some((bytes, peak)) => (vk::Format::R16G16B16A16_SFLOAT, bytes.as_slice(), *peak),
        };
        let gpu_image = GpuImage::new(
            device,
            allocator,
            "damascene_ash::image_texture",
            format,
            vk::Extent2D { width, height },
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        )?;
        let set_layouts = [self.texture_set_layout];
        let allocate_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&set_layouts);
        let descriptor_set = unsafe { device.allocate_descriptor_sets(&allocate_info) }?[0];
        let image_info = vk::DescriptorImageInfo::default()
            .image_view(gpu_image.view)
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
        let mut staging = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::image_staging",
            data.len() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        staging.write_bytes(data)?;
        self.pending_uploads.push(PendingUpload {
            hash: upload_hash,
            width,
            height,
            staging,
        });
        Ok(CachedTexture {
            image: gpu_image,
            descriptor_set,
            layout: vk::ImageLayout::UNDEFINED,
            peak,
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
            "damascene_ash::image_instances",
            (next * std::mem::size_of::<ImageInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        self.instance_capacity = next;
        Ok(())
    }

    pub(crate) fn run(&self, index: usize) -> &ImageRun {
        &self.runs[index]
    }

    pub(crate) fn pipeline(&self) -> vk::Pipeline {
        self.pipeline
    }

    pub(crate) fn pipeline_layout(&self) -> vk::PipelineLayout {
        self.pipeline_layout
    }

    pub(crate) fn instance_buffer(&self) -> vk::Buffer {
        self.instance_buf.buffer
    }

    pub(crate) fn descriptor_for_run(&self, run: &ImageRun) -> vk::DescriptorSet {
        let hash = self.bind_lookup[run.texture_idx];
        self.cache
            .get(&hash)
            .expect("cache entry alive for frame")
            .descriptor_set
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            for mut upload in self.pending_uploads.drain(..) {
                upload.staging.destroy(device, allocator);
            }
            for mut upload in self.retired_uploads.drain(..) {
                upload.destroy(device, allocator);
            }
            for (_, mut texture) in self.cache.drain() {
                texture.image.destroy(device, allocator);
            }
            self.instance_buf.destroy(device, allocator);
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.texture_set_layout, None);
        }
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

fn build_image_pipeline(
    device: &ash::Device,
    pipeline_layout: vk::PipelineLayout,
    target: TargetInfo,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv("stock::image", stock_wgsl::IMAGE)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_pipeline_with_module(device, pipeline_layout, target, shader);
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
            stride: std::mem::size_of::<ImageInstance>() as u32,
            input_rate: vk::VertexInputRate::INSTANCE,
        },
    ];
    let attrs = [
        attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT),
        attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT),
        attr(3, 1, 32, vk::Format::R32G32B32A32_SFLOAT),
        attr(4, 1, 48, vk::Format::R32G32B32A32_SFLOAT),
        // location 5: range = (content peak, luminance limit)
        attr(5, 1, 64, vk::Format::R32G32_SFLOAT),
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
            name: "stock::image".to_string(),
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
