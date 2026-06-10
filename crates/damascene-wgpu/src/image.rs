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

const IMAGE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    1 => Float32x4, // rect (xy = top-left logical px, zw = size)
    2 => Float32x4, // tint linear rgba — (1,1,1,1) when no app tint
    3 => Float32x4, // params = per-corner radii (tl, tr, br, bl) in logical px
    4 => Float32x4, // uv subrect (always (0,0,1,1) for v1; reserved for atlasing)
    5 => Float32x2, // range = (content peak, luminance limit), working units
];

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

struct CachedTexture {
    bind_group: wgpu::BindGroup,
    /// Measured content peak (max linear RGB channel, working-space
    /// units) — the image's effective MaxCLL, computed once at upload.
    /// `1.0` for the 8-bit sRGB fast path by construction.
    peak: f32,
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

    // Pipeline layout + sample count retained so `pipeline` (its only
    // swapchain-format-bound resource) can be rebuilt in place on a
    // surface-format renegotiation (`set_target_format`). The texture
    // bind-group layout is unchanged, so the cached per-image bind groups
    // stay valid.
    pipeline_layout: wgpu::PipelineLayout,
    sample_count: u32,

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
    /// Output luminance headroom (multiples of reference white; 1.0 on
    /// SDR) each draw's `DynamicRangeLimit` resolves against. Kept in
    /// sync with the owning `Runner` via `set_output_luminance`.
    headroom: f32,
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

        let pipeline = build_pipeline(device, &pipeline_layout, target_format, sample_count);

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
            pipeline_layout,
            sample_count,
            cache: HashMap::new(),
            bind_group_lookup: Vec::new(),
            frame_counter: 0,
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
            headroom: 1.0,
        }
    }

    /// Update the working color space subsequent tint color packing
    /// converts into. Called by `Runner::set_working_color_space`.
    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    /// Update the output headroom subsequent draws resolve their
    /// `DynamicRangeLimit` against. Called by
    /// `Runner::set_output_luminance`.
    pub(crate) fn set_headroom(&mut self, headroom: f32) {
        self.headroom = headroom;
    }

    /// Rebuild the swapchain-format-bound pipeline for a new target format,
    /// preserving the per-image texture cache, instance buffer, and sampler.
    /// Called by `Runner::set_target_format`. The pipeline + texture
    /// bind-group layouts are unchanged, so cached per-image bind groups stay
    /// valid.
    pub(crate) fn set_target_format(
        &mut self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) {
        self.pipeline = build_pipeline(
            device,
            &self.pipeline_layout,
            target_format,
            self.sample_count,
        );
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
        range_limit: damascene_core::image::DynamicRangeLimit,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();
        let (texture_idx, peak) = self.ensure_texture(device, queue, image);
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
            range: [peak, range_limit.resolve(self.headroom)],
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
    /// the texture bind group via `bind_group_for_run(idx)` — plus the
    /// image's measured content peak (cached with the texture).
    fn ensure_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
    ) -> (usize, f32) {
        let hash = image.content_hash();
        if !self.cache.contains_key(&hash) {
            let cached = upload_image(device, queue, &self.bind_layout, &self.sampler, image);
            self.cache.insert(hash, cached);
        }
        let entry = self.cache.get_mut(&hash).expect("just inserted");
        entry.last_used_frame = self.frame_counter;
        let peak = entry.peak;
        // Index into the per-frame lookup table.
        let idx = if let Some(idx) = self.bind_group_lookup.iter().position(|&h| h == hash) {
            idx
        } else {
            self.bind_group_lookup.push(hash);
            self.bind_group_lookup.len() - 1
        };
        (idx, peak)
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

/// Build the image (`stock::image`) pipeline. Shared by `new` and
/// `set_target_format` so the descriptor stays a single source of truth —
/// only `target_format` varies across the two call sites.
fn build_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("stock::image"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::IMAGE)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::image::pipeline"),
        layout: Some(pipeline_layout),
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
    })
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
    let (mut w, mut h) = (image.width(), image.height());
    // An image larger than the device's `max_texture_dimension_2d`
    // would fail `Device::create_texture` validation and panic the
    // whole process — content the user merely clicked on. Downscale
    // CPU-side to fit (issue #78). The resample runs in linear scRGB
    // f16 working space so colour averages correctly and HDR brights
    // survive, so an oversized image routes through the f16 path
    // regardless of source format — the trade is 2× the (now-small)
    // texture's memory for a single resampler.
    let limit = device.limits().max_texture_dimension_2d;
    let oversized = w.max(h) > limit;

    // Two upload shapes (see `damascene_core::image` module docs):
    // - 8-bit sRGB art uploads as-is; the sRGB texture format decodes
    //   to linear on sample, keeping the tint multiply in the same
    //   colour space as the rest of the pipeline (rounded_rect, text).
    // - Wide-gamut / HDR / deep sources normalize on the CPU to scRGB
    //   f16 (linear sRGB primaries, extended range) so sampling needs
    //   no conversion and out-of-gamut / >1.0 values survive to an
    //   extended-range swapchain.
    let mut f16_bits: Option<Vec<u16>> = None;
    let (format, bytes_per_pixel, peak): (_, u32, f32) = if oversized {
        let (bits, nw, nh, peak) = downscale_scrgb_f16(&image.to_scrgb_f16(), w, h, limit);
        log::warn!(
            "damascene_wgpu: image {w}x{h} exceeds device max_texture_dimension_2d \
             ({limit}); downscaled to {nw}x{nh} to avoid a create_texture panic"
        );
        (w, h) = (nw, nh);
        f16_bits = Some(bits);
        (wgpu::TextureFormat::Rgba16Float, 8, peak)
    } else if image.is_srgb8() {
        // 8-bit sRGB peaks at reference white by construction.
        (wgpu::TextureFormat::Rgba8UnormSrgb, 4, 1.0)
    } else {
        let (bits, peak) = image.to_scrgb_f16_with_peak();
        f16_bits = Some(bits);
        (wgpu::TextureFormat::Rgba16Float, 8, peak)
    };
    let data: &[u8] = match &f16_bits {
        Some(bits) => bytemuck::cast_slice(bits),
        None => image.pixels(),
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
        peak,
        last_used_frame: 0,
    }
}

/// Box-average downscale of interleaved scRGB f16 RGBA pixels (raw f16
/// bit patterns, `w * h * 4` long) so the larger side fits `max_dim`.
/// Returns the resampled bits, the new dimensions, and the
/// post-resample content peak (max linear RGB channel) — averaging only
/// lowers the peak, so reusing the pre-resample value would overstate
/// HDR headroom to the luminance remaster.
///
/// Averaging happens in linear working space (the f16 values already
/// are linear), so colour and `>1.0` brights survive; alpha averages
/// straight, matching the upload's straight-alpha contract. Each source
/// pixel is read exactly once — the cost is one pass over the original,
/// paid on the cache-miss path only. Only ever called when downscaling
/// (`max_dim < max(w, h)`), so every target maps to ≥1 source pixel.
fn downscale_scrgb_f16(bits: &[u16], w: u32, h: u32, max_dim: u32) -> (Vec<u16>, u32, u32, f32) {
    let scale = f64::from(max_dim) / f64::from(w.max(h));
    let nw = ((f64::from(w) * scale).floor() as u32).clamp(1, max_dim);
    let nh = ((f64::from(h) * scale).floor() as u32).clamp(1, max_dim);

    let decode = |x: u32, y: u32, c: usize| -> f32 {
        let idx = (y as usize * w as usize + x as usize) * 4 + c;
        half::f16::from_bits(bits[idx]).to_f32()
    };
    // Source-pixel span [s0, s1) covering target index `t` of `n` over a
    // source extent of `src`. With `src >= n` (downscale) the span is
    // always non-empty.
    let span = |t: u32, n: u32, src: u32| {
        let s0 = (u64::from(t) * u64::from(src) / u64::from(n)) as u32;
        let s1 = ((u64::from(t + 1) * u64::from(src) / u64::from(n)) as u32).max(s0 + 1);
        (s0, s1.min(src))
    };

    let mut out = Vec::with_capacity(nw as usize * nh as usize * 4);
    let mut peak = 0.0f32;
    for ty in 0..nh {
        let (sy0, sy1) = span(ty, nh, h);
        for tx in 0..nw {
            let (sx0, sx1) = span(tx, nw, w);
            let mut acc = [0.0f64; 4];
            let mut n = 0.0f64;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += f64::from(decode(sx, sy, c));
                    }
                    n += 1.0;
                }
            }
            for (c, a) in acc.iter().enumerate() {
                let v = (a / n) as f32;
                if c < 3 && v.is_finite() {
                    peak = peak.max(v);
                }
                out.push(half::f16::from_f32(v).to_bits());
            }
        }
    }
    (out, nw, nh, peak)
}

#[cfg(test)]
mod tests {
    use super::downscale_scrgb_f16;

    /// Build interleaved scRGB f16 RGBA bits from per-pixel `[r,g,b,a]`
    /// f32 values (row-major, `w * h` entries).
    fn f16_bits(pixels: &[[f32; 4]]) -> Vec<u16> {
        pixels
            .iter()
            .flat_map(|p| p.iter().map(|&c| half::f16::from_f32(c).to_bits()))
            .collect()
    }

    fn decode(bits: &[u16], i: usize) -> [f32; 4] {
        [
            half::f16::from_bits(bits[i * 4]).to_f32(),
            half::f16::from_bits(bits[i * 4 + 1]).to_f32(),
            half::f16::from_bits(bits[i * 4 + 2]).to_f32(),
            half::f16::from_bits(bits[i * 4 + 3]).to_f32(),
        ]
    }

    #[test]
    fn downscale_preserves_aspect_and_clamps_to_limit() {
        let src = f16_bits(&vec![[0.0; 4]; 12800 * 100]);
        let (_, nw, nh, _) = downscale_scrgb_f16(&src, 12800, 100, 8192);
        assert_eq!(nw, 8192);
        // 100 * 8192 / 12800 = 64.0
        assert_eq!(nh, 64);
        assert!(nw <= 8192 && nh <= 8192);
    }

    #[test]
    fn downscale_preserves_solid_colour_and_reports_peak() {
        // A solid HDR-bright tile: averaging any block reproduces it,
        // and the reported peak is the max linear channel (alpha ignored).
        let src = f16_bits(&vec![[2.0, 0.5, 0.0, 1.0]; 4 * 4]);
        let (out, nw, nh, peak) = downscale_scrgb_f16(&src, 4, 4, 2);
        assert_eq!((nw, nh), (2, 2));
        for i in 0..(nw * nh) as usize {
            let p = decode(&out, i);
            assert!((p[0] - 2.0).abs() < 1e-2, "r={}", p[0]);
            assert!((p[1] - 0.5).abs() < 1e-2, "g={}", p[1]);
            assert!(p[2].abs() < 1e-2, "b={}", p[2]);
            assert!((p[3] - 1.0).abs() < 1e-2, "a={}", p[3]);
        }
        assert!((peak - 2.0).abs() < 1e-2, "peak={peak}");
    }

    #[test]
    fn downscale_averages_in_linear_space() {
        // Two horizontally-adjacent pixels collapse to one: the result
        // is their linear mean, not either endpoint.
        let src = f16_bits(&[[0.0, 0.0, 0.0, 1.0], [2.0, 1.0, 0.0, 1.0]]);
        let (out, nw, nh, peak) = downscale_scrgb_f16(&src, 2, 1, 1);
        assert_eq!((nw, nh), (1, 1));
        let p = decode(&out, 0);
        assert!((p[0] - 1.0).abs() < 1e-2, "r={}", p[0]);
        assert!((p[1] - 0.5).abs() < 1e-2, "g={}", p[1]);
        assert!((peak - 1.0).abs() < 1e-2, "peak={peak}");
    }
}
