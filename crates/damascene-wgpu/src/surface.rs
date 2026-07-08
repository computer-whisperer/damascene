//! GPU compositing for app-owned [`AppTexture`]s.
//!
//! Where [`crate::image::ImagePaint`] uploads + content-hash-caches a
//! CPU pixel buffer per `Image`, this module samples a *pre-existing*
//! GPU texture the app allocated and filled itself. There is no upload
//! path; the bind-group cache is keyed on
//! [`AppTextureId`] and entries unreferenced for one frame are dropped
//! at flush.
//!
//! Three render pipelines are built up front, one per
//! [`SurfaceAlpha`] mode: they share the vertex stage, sampler, and
//! bind-group layout, and differ only in fragment entry point and
//! blend state. Per-instance data is just the destination rect — no
//! tint, no radius (those are deliberately out of 0.3.x scope).
//!
//! Per-frame lifecycle:
//! 1. `frame_begin()` clears the per-frame instance + run buffers.
//! 2. `record(...)` is called once per `DrawOp::AppTexture`. The first
//!    call for an [`AppTextureId`] builds a bind group from the
//!    texture's view; subsequent calls reuse the cached one.
//! 3. `flush()` writes the instance buffer and drops cache entries
//!    that weren't touched this frame.
//! 4. The render loop dispatches each `SurfaceRun` with its alpha-
//!    mode pipeline and the cached bind group active.

use std::any::Any;
use std::borrow::Cow;
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

const INITIAL_INSTANCE_CAPACITY: usize = 16;

const SURFACE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    1 => Float32x4, // rect (xy = top-left logical px, zw = size)
    2 => Float32x4, // affine matrix (a, b, c, d)
    3 => Float32x2, // affine translation (tx, ty)
];

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

struct CachedBindGroup {
    bind_group: wgpu::BindGroup,
    /// Frame index of the most recent `record` call for this texture
    /// id. Slots not touched in the current frame are dropped at flush.
    last_used_frame: u64,
}

pub(crate) struct SurfacePaint {
    instances: Vec<SurfaceInstance>,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    runs: Vec<SurfaceRun>,

    pipeline_premul: wgpu::RenderPipeline,
    pipeline_straight: wgpu::RenderPipeline,
    pipeline_opaque: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,

    // Pipeline layout + sample count retained so the three
    // swapchain-format-bound pipelines above (one per `SurfaceAlpha` mode)
    // can be rebuilt in place on a surface-format renegotiation
    // (`set_target_format`). The shader module is cheap to recreate, so it
    // is not stored. The texture bind-group layout is unchanged, so cached
    // per-texture bind groups stay valid.
    pipeline_layout: wgpu::PipelineLayout,
    sample_count: u32,

    /// AppTextureId(u64) → cached bind group for that texture's view.
    cache: HashMap<u64, CachedBindGroup>,
    /// Parallel per-frame index so `SurfaceRun::texture_idx` names a
    /// stable slot. Rebuilt each `frame_begin`.
    bind_group_lookup: Vec<u64>,
    frame_counter: u64,
}

impl SurfacePaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("damascene_wgpu::surface::texture_bind_layout"),
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
            label: Some("damascene_wgpu::surface::pipeline_layout"),
            bind_group_layouts: &[Some(frame_bind_layout), Some(&bind_layout)],
            immediate_size: 0,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::surface"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SURFACE)),
        });

        let pipeline_premul = build_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            sample_count,
            "fs_premul",
            premultiplied_blend(),
            "damascene_wgpu::surface::pipeline_premul",
        );
        let pipeline_straight = build_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            sample_count,
            "fs_straight",
            premultiplied_blend(),
            "damascene_wgpu::surface::pipeline_straight",
        );
        let pipeline_opaque = build_pipeline(
            device,
            &pipeline_layout,
            &shader,
            target_format,
            sample_count,
            "fs_opaque",
            // SurfaceAlpha::Opaque replaces destination pixels — skip
            // blending entirely so the surface texture overwrites
            // whatever was painted underneath it within the rect.
            opaque_blend(),
            "damascene_wgpu::surface::pipeline_opaque",
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("damascene_wgpu::surface::sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::surface::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<SurfaceInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            runs: Vec::new(),
            pipeline_premul,
            pipeline_straight,
            pipeline_opaque,
            bind_layout,
            sampler,
            pipeline_layout,
            sample_count,
            cache: HashMap::new(),
            bind_group_lookup: Vec::new(),
            frame_counter: 0,
        }
    }

    /// Rebuild the three swapchain-format-bound pipelines (one per
    /// `SurfaceAlpha` mode) for a new target format, preserving the
    /// per-texture bind-group cache, instance buffer, and sampler. Called by
    /// `Runner::set_target_format`. The shader module + blend modes are
    /// re-derived identically; the pipeline + texture bind-group layouts are
    /// unchanged, so cached bind groups stay valid.
    pub(crate) fn set_target_format(
        &mut self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::surface"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SURFACE)),
        });
        self.pipeline_premul = build_pipeline(
            device,
            &self.pipeline_layout,
            &shader,
            target_format,
            self.sample_count,
            "fs_premul",
            premultiplied_blend(),
            "damascene_wgpu::surface::pipeline_premul",
        );
        self.pipeline_straight = build_pipeline(
            device,
            &self.pipeline_layout,
            &shader,
            target_format,
            self.sample_count,
            "fs_straight",
            premultiplied_blend(),
            "damascene_wgpu::surface::pipeline_straight",
        );
        self.pipeline_opaque = build_pipeline(
            device,
            &self.pipeline_layout,
            &shader,
            target_format,
            self.sample_count,
            "fs_opaque",
            opaque_blend(),
            "damascene_wgpu::surface::pipeline_opaque",
        );
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.bind_group_lookup.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
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
        let texture_idx = self.ensure_bind_group(device, texture);
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

    fn ensure_bind_group(&mut self, device: &wgpu::Device, texture: &AppTexture) -> usize {
        let id = texture.id().0;
        if !self.cache.contains_key(&id) {
            let backend = texture.backend();
            let wgpu_tex = backend.as_any().downcast_ref::<WgpuAppTexture>().unwrap_or_else(|| {
                panic!(
                    "AppTexture passed to damascene-wgpu was not constructed by damascene_wgpu::app_texture \
                     (actual backend: {}); mixing backends in one runtime is unsupported",
                    texture.backend_name(),
                )
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("damascene_wgpu::surface::bind_group"),
                layout: &self.bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&wgpu_tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.cache.insert(
                id,
                CachedBindGroup {
                    bind_group,
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

    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let frame = self.frame_counter;
        self.cache.retain(|_, v| v.last_used_frame == frame);

        if self.instances.len() > self.instance_capacity {
            let new_cap = self.instances.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::surface::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<SurfaceInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }
        if !self.instances.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(&self.instances));
        }
    }

    pub(crate) fn run(&self, index: usize) -> &SurfaceRun {
        &self.runs[index]
    }

    pub(crate) fn pipeline_for(&self, alpha: SurfaceAlpha) -> &wgpu::RenderPipeline {
        match alpha {
            SurfaceAlpha::Premultiplied => &self.pipeline_premul,
            SurfaceAlpha::Straight => &self.pipeline_straight,
            SurfaceAlpha::Opaque => &self.pipeline_opaque,
        }
    }

    pub(crate) fn instance_buf(&self) -> &wgpu::Buffer {
        &self.instance_buf
    }

    pub(crate) fn bind_group_for_run(&self, run: &SurfaceRun) -> &wgpu::BindGroup {
        let id = self.bind_group_lookup[run.texture_idx];
        &self
            .cache
            .get(&id)
            .expect("cache entry alive for the frame")
            .bind_group
    }
}

#[allow(clippy::too_many_arguments)]
fn build_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    fs_entry: &'static str,
    blend: Option<wgpu::BlendState>,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                    }],
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SurfaceInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &SURFACE_INSTANCE_ATTRS,
                }),
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend,
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
    })
}

fn premultiplied_blend() -> Option<wgpu::BlendState> {
    Some(wgpu::BlendState {
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
    })
}

fn opaque_blend() -> Option<wgpu::BlendState> {
    Some(wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        },
    })
}

// ---- Public AppTexture constructor ----

/// Concrete wgpu-side [`AppTextureBackend`]. Holds the texture +
/// view + a cached id so the runtime can downcast and pull what it
/// needs without re-creating views per frame.
#[derive(Debug)]
pub struct WgpuAppTexture {
    /// The app-owned texture. Held as `Arc` so `AppTexture` can be
    /// cheaply cloned into the El tree without releasing the GPU
    /// resource.
    pub texture: Arc<wgpu::Texture>,
    /// Default 2D view over the full texture, created once at
    /// construction so the per-frame record path doesn't allocate.
    pub view: Arc<wgpu::TextureView>,
    id: AppTextureId,
    size: (u32, u32),
    format: SurfaceFormat,
}

impl AppTextureBackend for WgpuAppTexture {
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

/// Wrap an app-allocated `wgpu::Texture` for compositing via a
/// [`damascene_core::tree::surface`] widget.
///
/// The texture must have `TEXTURE_BINDING` usage and one of the three
/// supported RGBA8 formats: `Rgba8UnormSrgb`, `Bgra8UnormSrgb`, or
/// `Rgba8Unorm`. Sample count must be 1 — Damascene composites the texture
/// into its own (possibly multisampled) render pass; multisampled
/// source textures aren't supported in 0.3.x.
///
/// # Panics
///
/// Panics if the texture is missing `TEXTURE_BINDING` usage, its format
/// is outside the supported set, or its sample count is not 1. These are
/// app-side mistakes, not runtime errors — fail loudly rather than
/// silently miscompositing.
pub fn app_texture(texture: Arc<wgpu::Texture>) -> AppTexture {
    let format = match texture.format() {
        wgpu::TextureFormat::Rgba8UnormSrgb => SurfaceFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8UnormSrgb => SurfaceFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm => SurfaceFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba16Float => SurfaceFormat::Rgba16Float,
        f => panic!(
            "damascene_wgpu::app_texture: unsupported texture format {:?} \
             (expected Rgba8UnormSrgb / Bgra8UnormSrgb / Rgba8Unorm / Rgba16Float)",
            f
        ),
    };
    assert!(
        texture
            .usage()
            .contains(wgpu::TextureUsages::TEXTURE_BINDING),
        "damascene_wgpu::app_texture: source texture must include TEXTURE_BINDING usage (got {:?})",
        texture.usage(),
    );
    assert_eq!(
        texture.sample_count(),
        1,
        "damascene_wgpu::app_texture: source texture must be single-sampled (got sample_count = {})",
        texture.sample_count(),
    );
    let extent = texture.size();
    let size = (extent.width, extent.height);
    let view = Arc::new(texture.create_view(&wgpu::TextureViewDescriptor::default()));
    AppTexture::from_backend(Arc::new(WgpuAppTexture {
        texture,
        view,
        id: next_app_texture_id(),
        size,
        format,
    }))
}

/// An app-owned texture updated from CPU pixel data every frame or so —
/// video frames, animated images, camera feeds, software-rendered
/// panels. Owns the texture lifecycle that every media consumer
/// otherwise hand-rolls around [`app_texture`]:
///
/// - creates (and on size/format change, recreates) the
///   `TEXTURE_BINDING | COPY_DST` texture;
/// - uploads frames with [`Self::write_frame`], or
///   [`Self::write_frame_if_changed`] for sources that repeat frames
///   (paused video, looping animations) where a cheap content hash
///   saves the redundant upload;
/// - hands out a stable [`AppTexture`] handle via
///   [`Self::app_texture`] for the [`surface()`] tree builder. The
///   handle's identity is stable across `write_frame` calls — which is
///   what keeps the per-texture bind-group cache warm — and changes
///   only when a size/format change forces a new GPU texture (one cold
///   bind-group rebuild, then warm again).
///
/// `data` is tightly-packed rows of `width * height` pixels in the
/// texture's format (RGBA8 variants: 4 bytes/pixel; `Rgba16Float`:
/// 8 bytes/pixel). Formats follow [`app_texture`]'s supported set.
///
/// [`surface()`]: damascene_core::surface
pub struct StreamingTexture {
    texture: Arc<wgpu::Texture>,
    handle: AppTexture,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    last_hash: Option<u64>,
}

impl StreamingTexture {
    /// Create with an initial size. The texture contents are undefined
    /// until the first [`Self::write_frame`]; draw order guarantees
    /// nothing samples it before the first queue submission that
    /// follows a write.
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let (texture, handle) = Self::create(device, format, width, height);
        Self {
            texture,
            handle,
            format,
            size: (width, height),
            last_hash: None,
        }
    }

    fn create(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> (Arc<wgpu::Texture>, AppTexture) {
        let texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("damascene-streaming-texture"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        }));
        let handle = app_texture(texture.clone());
        (texture, handle)
    }

    /// Bytes per pixel for the texture's format (validated by
    /// [`app_texture`] to be one of the supported set).
    fn bytes_per_pixel(&self) -> u32 {
        match self.format {
            wgpu::TextureFormat::Rgba16Float => 8,
            _ => 4,
        }
    }

    /// Upload one frame of tightly-packed pixel rows, recreating the
    /// texture first if `width`/`height` differ from the current size.
    /// Panics if `data` is not exactly `width * height *
    /// bytes_per_pixel` long.
    pub fn write_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        data: &[u8],
    ) {
        if (width, height) != self.size {
            let (texture, handle) = Self::create(device, self.format, width, height);
            self.texture = texture;
            self.handle = handle;
            self.size = (width, height);
            self.last_hash = None;
        }
        let bpp = self.bytes_per_pixel();
        assert_eq!(
            data.len() as u64,
            width as u64 * height as u64 * bpp as u64,
            "StreamingTexture::write_frame: data length {} != {}x{} * {} bytes/px",
            data.len(),
            width,
            height,
            bpp,
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bpp),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// [`Self::write_frame`], skipped when `data` hashes identically
    /// to the previous accepted frame. Worth it for sources that
    /// repeat frames (paused video, looping GIFs at rest); for live
    /// video where every frame differs, prefer [`Self::write_frame`]
    /// and skip the hash. Returns `true` when an upload happened.
    pub fn write_frame_if_changed(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        data: &[u8],
    ) -> bool {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (width, height).hash(&mut hasher);
        data.hash(&mut hasher);
        let hash = hasher.finish();
        if self.last_hash == Some(hash) && (width, height) == self.size {
            return false;
        }
        self.write_frame(device, queue, width, height, data);
        self.last_hash = Some(hash);
        true
    }

    /// The handle to embed in the tree via
    /// [`damascene_core::surface`]. Cheap clone; identity stays stable
    /// across [`Self::write_frame`] calls so backend bind-group caches
    /// stay warm.
    pub fn app_texture(&self) -> AppTexture {
        self.handle.clone()
    }

    /// Current texture size `(width, height)` in texels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}
