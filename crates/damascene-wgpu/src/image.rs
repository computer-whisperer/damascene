//! GPU raster-image rendering.
//!
//! One pipeline (stock::image) plus a per-image GPU texture cache
//! keyed on [`damascene_core::image::Image::content_hash`]. Two equal
//! `Image` values share a slot; cache entries unreferenced for one
//! frame are dropped at flush, so transient images don't pin memory.
//!
//! Per-frame lifecycle:
//! 1. `frame_begin()` clears the per-frame instance + run buffers.
//! 2. `record(...)` is called once per `DrawOp::Image`. The first
//!    call for a content hash uploads the texture; subsequent calls
//!    reuse the cached bind group. Returns the `runs` index.
//! 3. `flush()` writes the instance buffer and drops cache entries
//!    that weren't touched this frame.
//! 4. The render loop dispatches each `ImageRun` with its texture's
//!    bind group active.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use damascene_core::color::ColorSpace;
use damascene_core::image::Image;
use damascene_core::paint::{DEFAULT_WORKING_COLOR_SPACE, PhysicalScissor, rgba_f32_in};
use damascene_core::shader::stock_wgsl;
use damascene_core::tree::{Color, Corners, Rect};

use bytemuck::{Pod, Zeroable};

const INITIAL_INSTANCE_CAPACITY: usize = 32;

const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4, // rect (xy = top-left logical px, zw = size)
    2 => Float32x4, // tint linear rgba — (1,1,1,1) when no app tint
    3 => Float32x4, // params = per-corner radii (tl, tr, br, bl) in logical px
    4 => Float32x4, // uv subrect (always (0,0,1,1) for v1; reserved for atlasing)
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct ImageInstance {
    rect: [f32; 4],
    tint: [f32; 4],
    params: [f32; 4],
    uv: [f32; 4],
}

pub(crate) struct ImageRun {
    pub texture_idx: usize,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

struct CachedTexture {
    bind_group: wgpu::BindGroup,
    /// Frame index of the most recent `record` call against this slot.
    /// Slots not touched in the current frame are dropped at flush.
    last_used_frame: u64,
}

pub(crate) struct ImagePaint {
    instances: Vec<ImageInstance>,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    runs: Vec<ImageRun>,

    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,

    /// content_hash → cached GPU texture + bind group.
    cache: HashMap<u64, CachedTexture>,
    /// Parallel index into `cache` keyed by hash, but stable across
    /// the frame so `ImageRun::texture_idx` can name a slot. Rebuilt
    /// each `frame_begin`.
    bind_group_lookup: Vec<u64>,
    frame_counter: u64,
    /// Working color space image tint colors are converted into. Kept in
    /// sync with the owning `Runner` via `set_working_color_space`.
    working_color_space: ColorSpace,
}

impl ImagePaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("damascene_wgpu::image::texture_bind_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("damascene_wgpu::image::pipeline_layout"),
            bind_group_layouts: &[Some(frame_bind_layout), Some(&bind_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::image"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::IMAGE)),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("damascene_wgpu::image::pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                        }],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ImageInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &IMAGE_INSTANCE_ATTRS,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // Premultiplied output (matches stock::text_msdf).
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("damascene_wgpu::image::sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::image::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<ImageInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            runs: Vec::new(),
            pipeline,
            bind_layout,
            sampler,
            cache: HashMap::new(),
            bind_group_lookup: Vec::new(),
            frame_counter: 0,
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
        }
    }

    /// Update the working color space subsequent tint color packing
    /// converts into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.bind_group_lookup.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        image: &Image,
        tint: Option<Color>,
        radius: Corners,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();
        let texture_idx = self.ensure_texture(device, queue, image);
        let tint_rgba = tint
            .map(|c| rgba_f32_in(c, self.working_color_space))
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let instance = ImageInstance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            tint: tint_rgba,
            params: [
                radius.tl.max(0.0),
                radius.tr.max(0.0),
                radius.br.max(0.0),
                radius.bl.max(0.0),
            ],
            uv: [0.0, 0.0, 1.0, 1.0],
        };
        let first = self.instances.len() as u32;
        self.instances.push(instance);
        self.runs.push(ImageRun {
            texture_idx,
            scissor,
            first,
            count: 1,
        });
        start..self.runs.len()
    }

    /// Look up or upload a texture for `image`. Returns an index into
    /// the per-frame `bind_group_lookup` table — the renderer reads
    /// the texture bind group via `bind_group_for_run(idx)`.
    fn ensure_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
    ) -> usize {
        let hash = image.content_hash();
        if !self.cache.contains_key(&hash) {
            let cached = upload_image(device, queue, &self.bind_layout, &self.sampler, image);
            self.cache.insert(hash, cached);
        }
        let entry = self.cache.get_mut(&hash).expect("just inserted");
        entry.last_used_frame = self.frame_counter;
        // Index into the per-frame lookup table.
        if let Some(idx) = self.bind_group_lookup.iter().position(|&h| h == hash) {
            idx
        } else {
            self.bind_group_lookup.push(hash);
            self.bind_group_lookup.len() - 1
        }
    }

    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // GC cache entries not used this frame.
        let frame = self.frame_counter;
        self.cache.retain(|_, v| v.last_used_frame == frame);

        // Resize + write instance buffer.
        if self.instances.len() > self.instance_capacity {
            let new_cap = self.instances.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::image::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<ImageInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }
        if !self.instances.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(&self.instances));
        }
    }

    pub(crate) fn run(&self, index: usize) -> &ImageRun {
        &self.runs[index]
    }

    pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    pub(crate) fn instance_buf(&self) -> &wgpu::Buffer {
        &self.instance_buf
    }

    /// Bind group for the texture referenced by `run.texture_idx`.
    pub(crate) fn bind_group_for_run(&self, run: &ImageRun) -> &wgpu::BindGroup {
        let hash = self.bind_group_lookup[run.texture_idx];
        &self
            .cache
            .get(&hash)
            .expect("cache entry alive for the frame")
            .bind_group
    }
}

/// Upload an `Image` to a fresh GPU texture and assemble its bind
/// group. Called on cache miss inside `ensure_texture`.
fn upload_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bind_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    image: &Image,
) -> CachedTexture {
    let (w, h) = (image.width(), image.height());
    // Two upload shapes (see `damascene_core::image` module docs):
    // - 8-bit sRGB art uploads as-is; the sRGB texture format decodes
    //   to linear on sample, keeping the tint multiply in the same
    //   colour space as the rest of the pipeline (rounded_rect, text).
    // - Wide-gamut / HDR / deep sources normalize on the CPU to scRGB
    //   f16 (linear sRGB primaries, extended range) so sampling needs
    //   no conversion and out-of-gamut / >1.0 values survive to an
    //   extended-range swapchain.
    let scrgb = (!image.is_srgb8()).then(|| image.to_scrgb_f16());
    let (format, data, bytes_per_pixel): (_, &[u8], u32) = match &scrgb {
        None => (wgpu::TextureFormat::Rgba8UnormSrgb, image.pixels(), 4),
        Some(f16_bits) => (
            wgpu::TextureFormat::Rgba16Float,
            bytemuck::cast_slice(f16_bits),
            8,
        ),
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::image::texture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_pixel * w),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("damascene_wgpu::image::bind_group"),
        layout: bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    CachedTexture {
        bind_group,
        last_used_frame: 0,
    }
}
