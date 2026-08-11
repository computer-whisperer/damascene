//! `wgpu` backend for custom Damascene hosts.
//!
//! Most applications should implement `damascene_core::App` and run it
//! through `damascene-winit-wgpu`. Use this crate directly when you are
//! writing your own host, embedding Damascene into an existing `wgpu`
//! renderer, or producing headless render artifacts.
//!
//! The public entry point is [`Runner`]. It owns:
//!
//! - GPU resources: pipelines, buffers, text atlas, and icon atlas.
//! - Backend-agnostic interaction state shared through
//!   `damascene_core::runtime::RunnerCore`.
//! - A snapshot of the last laid-out tree so input arriving between
//!   frames hit-tests against the geometry the user can see.
//!
//! # Custom host loop
//!
//! The runner does not own the device, queue, swapchain, window, or
//! event loop. A host creates those resources, forwards input into the
//! runner, builds a fresh `El` tree, prepares GPU buffers, and renders:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use damascene_wgpu::Runner;
//!
//! let mut runner = Runner::new(&device, &queue, surface_format);
//! runner.set_surface_size(surface_width, surface_height);
//!
//! // Per frame:
//! app.before_build();
//! let theme = app.theme();
//! let mut tree = app.build(&damascene_core::BuildCx::new(&theme));
//! runner.set_hotkeys(app.hotkeys());
//! runner.set_theme(theme);
//! runner.prepare(&device, &queue, tree, viewport, scale_factor);
//! runner.render(&device, &mut encoder, target_texture, target_view, None, load_op);
//! ```
//!
//! `prepare` is split from `render`/`draw` so all `queue.write_buffer`
//! calls and atlas uploads happen before render-pass recording, matching
//! `wgpu`'s expected order. Coordinates passed to pointer methods are
//! logical pixels; render targets are physical pixels, so pass the host
//! scale factor to [`Runner::prepare`].
//!
//! Use [`Runner::render`] when Damascene should own pass boundaries. This is
//! required for backdrop-sampling custom shaders. Use [`Runner::draw`]
//! only when you are already inside a host-owned pass and do not need
//! backdrop sampling.
//!
//! # Custom shaders
//!
//! Call [`Runner::register_shader`] with a name and WGSL source. The
//! shader's vertex/fragment must use the shared instance layout — see
//! `shaders/rounded_rect.wgsl` (in damascene-core) for the canonical
//! example. Bind the shader at a node via
//! `El::shader(ShaderBinding::custom(name).with(...))`. Per-instance
//! uniforms map to three generic `vec4` slots:
//!
//! | Uniform key | Slot (`@location`) | Accepted types |
//! |---|---|---|
//! | `vec_a` | 2 | `Color` (rgba 0..1) or `Vec4` |
//! | `vec_b` | 3 | `Color` or `Vec4` |
//! | `vec_c` | 4 | `Vec4` (or fall back to scalar `f32` packed in `.x`) |
//!
//! Stock `rounded_rect` reuses the same layout but reads its own named
//! uniforms (`fill`, `stroke`, `stroke_width`, `radius`, `shadow`).

mod icon;
mod image;
mod instance;
mod msaa;
mod pipeline;
mod scene;
mod surface;
mod text;

pub use crate::msaa::MsaaTarget;
pub use crate::surface::{StreamingTexture, WgpuAppTexture, app_texture};
pub use crate::text::SharedText;

use std::collections::{HashMap, HashSet};
// `web_time::Instant` is API-identical to `std::time::Instant` on
// native and uses `performance.now()` on wasm32 — std's `Instant::now()`
// panics in the browser because there is no monotonic clock there.
use web_time::Instant;

use wgpu::util::DeviceExt;

use damascene_core::event::{KeyChord, KeyModifiers, LogicalKey, PhysicalKey, Pointer, UiEvent};
use damascene_core::ir::TextAnchor;
use damascene_core::paint::{IconRunKind, PhysicalScissor, QuadInstance};
use damascene_core::runtime::{RecordedPaint, RunnerCore, TextRecorder};
use damascene_core::shader::{ShaderHandle, StockShader, stock_wgsl};
use damascene_core::state::{AnimationMode, UiState};
use damascene_core::text::atlas::RunStyle;
use damascene_core::theme::Theme;
use damascene_core::tree::{Color, El, Rect, TextWrap};
use damascene_core::vector::IconMaterial;

pub use damascene_core::paint::PaintItem;
pub use damascene_core::runtime::{LayoutPrepared, PointerMove, PrepareResult, PrepareTimings};

use crate::icon::IconPaint;
use crate::image::ImagePaint;
use crate::instance::set_scissor;
use crate::pipeline::{FrameUniforms, build_quad_pipeline};
use crate::scene::Scene3DPaint;
use crate::surface::SurfacePaint;
use crate::text::TextPaint;

/// Initial size for the dynamic instance buffer (grows as needed).
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Adapter-derived capabilities the [`Runner`] adapts its pipelines to.
///
/// Defaults to everything supported — correct for native Vulkan/Metal/DX
/// adapters. Hosts that can land on GL or browser adapters should derive
/// the real values with [`RunnerCaps::from_adapter`] and build the runner
/// via [`Runner::with_caps`].
#[derive(Clone, Copy, Debug)]
pub struct RunnerCaps {
    /// Whether the adapter supports per-sample MSAA shading
    /// (`DownlevelFlags::MULTISAMPLED_SHADING`). When `false`, every
    /// pipeline (stock and later-registered custom) has
    /// `@interpolate(perspective, sample)` rewritten to
    /// `@interpolate(perspective)` before WGSL compilation. The shader
    /// then interpolates at pixel centre instead of per MSAA sample —
    /// MSAA coverage still works at `sample_count > 1`; only the
    /// per-sub-sample brightness pass is skipped, slightly thickening
    /// the AA band on curved SDF edges.
    pub per_sample_shading: bool,
    /// Whether the backend can read a scene depth *attachment* back for
    /// `Scene3D` label occlusion. Must be `false` on GL backends
    /// (WebGL2): naga's GLSL target can't `textureLoad` depth textures
    /// (so building the resolve pipeline panics the device), and GLES 3.0
    /// can't create multisampled depth *textures* at all. When `false`,
    /// occlusion still works — the capture re-renders the scene's meshes
    /// with a fragment stage that packs depth into an RGBA8 colour target
    /// instead of resolving the depth attachment. Costs one extra
    /// mesh-only pass per camera-pose change on those backends.
    pub depth_readback: bool,
}

impl Default for RunnerCaps {
    fn default() -> Self {
        Self {
            per_sample_shading: true,
            depth_readback: true,
        }
    }
}

impl RunnerCaps {
    /// Derive the caps from the adapter the host actually got.
    ///
    /// GL is treated as unsupported across the board regardless of the
    /// reported downlevel flags: Chrome's SwiftShader WebGL2 fallback
    /// reports `MULTISAMPLED_SHADING` through wgpu, but the GLSL ES
    /// target still rejects the sample interpolation qualifier (and can
    /// never `textureLoad` a depth texture). WebGPU/native keep trusting
    /// the adapter flags.
    pub fn from_adapter(adapter: &wgpu::Adapter) -> Self {
        let gl = adapter.get_info().backend == wgpu::Backend::Gl;
        Self {
            per_sample_shading: !gl
                && adapter
                    .get_downlevel_capabilities()
                    .flags
                    .contains(wgpu::DownlevelFlags::MULTISAMPLED_SHADING),
            depth_readback: !gl,
        }
    }
}

/// Wgpu runtime owned by the host. One instance per surface/format.
///
/// All backend-agnostic state — interaction state, paint-stream scratch,
/// per-stage layout/animation hooks — lives in `core: RunnerCore` and
/// is shared with the vulkano backend. The fields below are wgpu-specific
/// resources only.
pub struct Runner {
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    /// [`RunnerCaps::per_sample_shading`], kept past construction because
    /// later-registered custom shaders go through [`build_quad_pipeline`]
    /// too. (`depth_readback` lives on in [`Scene3DPaint`].)
    per_sample_shading: bool,

    // Shared resources.
    pipeline_layout: wgpu::PipelineLayout,
    /// Pipeline layout for `samples_backdrop` custom shaders — adds
    /// `@group(1)` for the snapshot texture + sampler.
    backdrop_pipeline_layout: wgpu::PipelineLayout,
    quad_bind_group: wgpu::BindGroup,
    backdrop_bind_layout: wgpu::BindGroupLayout,
    backdrop_sampler: wgpu::Sampler,
    frame_buf: wgpu::Buffer,
    quad_vbo: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,

    // One pipeline per registered shader (stock + custom).
    pipelines: HashMap<ShaderHandle, wgpu::RenderPipeline>,
    // Custom shader names registered with `samples_backdrop=true`. The
    // paint scheduler queries this to insert pass boundaries before the
    // first backdrop-sampling draw.
    backdrop_shaders: HashSet<&'static str>,
    // Custom shader names registered with `samples_time=true`. Mirrors
    // `backdrop_shaders` but feeds `prepare_layout`'s continuous-redraw
    // scan instead of the paint scheduler.
    time_shaders: HashSet<&'static str>,
    // Retained WGSL source per registered custom shader, keyed by name
    // (re-registering replaces the entry). `register_shader_with` builds
    // the pipeline *and* stashes the source here so
    // [`Self::set_target_format`] can rebuild every custom pipeline against
    // the new swapchain format. The bool is the `samples_backdrop` flag,
    // which selects the same pipeline layout the original registration used.
    custom_shaders: HashMap<&'static str, (String, bool)>,

    // stock::text resources — atlas, page textures, glyph instances.
    text_paint: TextPaint,
    // stock::icon_line resources — vector icon stroke instances.
    icon_paint: IconPaint,
    // stock::image resources — per-image texture cache + instance buf.
    image_paint: ImagePaint,
    surface_paint: SurfacePaint,
    // stock::scene resources — geometry buffer cache, per-node offscreen
    // targets, scene pipelines. Renders DrawOp::Scene3D offscreen and
    // composites the resolved texture through the surface path.
    scene_paint: Scene3DPaint,

    /// Lazily-allocated snapshot of the color target, sized to match
    /// the current target on each `render()`. Backdrop-sampling
    /// shaders read this via `@group(1)` after Pass A.
    snapshot: Option<SnapshotTexture>,
    /// Bind group binding the snapshot view + sampler. Rebuilt each
    /// time the snapshot texture is reallocated.
    backdrop_bind_group: Option<wgpu::BindGroup>,
    /// One-shot flag for the "target lacks COPY_SRC" degrade warning
    /// in [`Self::render`], so it logs once instead of every frame.
    backdrop_copy_unsupported_warned: bool,

    /// Wall-clock origin for the `time` field in `FrameUniforms`.
    /// `prepare()` writes `(now - start_time).as_secs_f32()`.
    start_time: Instant,

    /// Output white-level scale written into `FrameUniforms.white_scale`.
    /// 1.0 whenever the surface puts reference white at signal 1.0 —
    /// 8-bit sRGB, and Wayland's anchored parametric ext-linear float
    /// swapchain. A host whose surface reads as genuine Windows scRGB
    /// (signal 1.0 = 80 cd/m² absolute) sets 203/80 so UI white lands
    /// at the encoding's assumed reference white. See
    /// [`Self::set_white_scale`] and docs/COLOR_MANAGEMENT.md.
    white_scale: f32,
    /// Output luminance headroom (`target_max / reference`, 1.0 on SDR)
    /// and reference white in cd/m², written into
    /// `FrameUniforms.headroom/ref_nits` and (headroom) mirrored into
    /// the image paint for the per-image HDR remaster. See
    /// [`Self::set_output_luminance`].
    headroom: f32,
    ref_nits: f32,

    // Backend-agnostic state shared with damascene-vulkano: interaction
    // state, paint-stream scratch (quad_scratch / runs / paint_items),
    // viewport_px, last_tree, the 13 input plumbing methods.
    core: RunnerCore,
}

struct SnapshotTexture {
    texture: wgpu::Texture,
    extent: (u32, u32),
}

struct PaintRecorder<'a> {
    text: &'a mut TextPaint,
    icons: &'a mut IconPaint,
    images: &'a mut ImagePaint,
    surfaces: &'a mut SurfacePaint,
    scenes: &'a mut Scene3DPaint,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
}

impl TextRecorder for PaintRecorder<'_> {
    fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        style: &damascene_core::text::atlas::RunStyle,
        text: &str,
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.text.record(
            rect,
            scissor,
            style,
            text,
            size,
            line_height,
            wrap,
            anchor,
            scale_factor,
        )
    }

    fn record_runs(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        runs: &[(String, RunStyle)],
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.text.record_runs(
            rect,
            scissor,
            runs,
            size,
            line_height,
            wrap,
            anchor,
            scale_factor,
        )
    }

    fn record_icon(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &damascene_core::icons::svg::IconSource,
        color: Color,
        _size: f32,
        stroke_width: f32,
        _scale_factor: f32,
    ) -> RecordedPaint {
        RecordedPaint::Icon(
            self.icons
                .record(rect, scissor, source, color, stroke_width),
        )
    }

    fn record_image(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        image: &damascene_core::image::Image,
        tint: Option<Color>,
        radius: damascene_core::tree::Corners,
        _fit: damascene_core::image::ImageFit,
        range_limit: damascene_core::image::DynamicRangeLimit,
        _scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.images.record(
            self.device,
            self.queue,
            rect,
            scissor,
            image,
            tint,
            radius,
            range_limit,
        )
    }

    fn record_app_texture(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        texture: &damascene_core::surface::AppTexture,
        alpha: damascene_core::surface::SurfaceAlpha,
        transform: damascene_core::affine::Affine2,
        _scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.surfaces
            .record(self.device, rect, scissor, texture, alpha, transform)
    }

    fn record_vector(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        asset: &damascene_core::vector::VectorAsset,
        render_mode: damascene_core::vector::VectorRenderMode,
        _scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.icons.record_vector(rect, scissor, asset, render_mode)
    }

    fn record_scene3d(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        id: &str,
        scene: &std::sync::Arc<damascene_core::scene::Scene3DData>,
        scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.scenes
            .record(self.device, rect, scissor, id, scene, scale_factor)
    }
}

/// Build the four stock rect-shaped quad pipelines (rounded_rect, spinner,
/// skeleton, progress_indeterminate) into `pipelines`, replacing any
/// existing entries. Shared by [`Runner::with_caps`] and
/// [`Runner::set_target_format`] so the catalog stays a single source of
/// truth — only `target_format` varies across the two call sites.
fn build_stock_quad_pipelines(
    pipelines: &mut HashMap<ShaderHandle, wgpu::RenderPipeline>,
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
    per_sample_shading: bool,
) {
    for (handle, label, wgsl) in [
        (
            StockShader::RoundedRect,
            "stock::rounded_rect",
            stock_wgsl::ROUNDED_RECT,
        ),
        (StockShader::Spinner, "stock::spinner", stock_wgsl::SPINNER),
        (
            StockShader::Skeleton,
            "stock::skeleton",
            stock_wgsl::SKELETON,
        ),
        (
            StockShader::ProgressIndeterminate,
            "stock::progress_indeterminate",
            stock_wgsl::PROGRESS_INDETERMINATE,
        ),
    ] {
        let pipeline = build_quad_pipeline(
            device,
            layout,
            target_format,
            sample_count,
            label,
            wgsl,
            per_sample_shading,
        );
        pipelines.insert(ShaderHandle::Stock(handle), pipeline);
    }
}

impl Runner {
    /// Create a runner for the given target color format. The host
    /// passes its swapchain/render-target format here so pipelines and
    /// the glyph atlas are built compatible.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        Self::with_sample_count(device, queue, target_format, 1)
    }

    /// Like [`Self::new`], but builds all pipelines with `sample_count`
    /// MSAA samples. The host must provide a matching multisampled
    /// render target and a single-sample resolve target. `sample_count`
    /// of 1 is the non-MSAA default.
    ///
    /// Defaults to [`RunnerCaps::default`] (everything supported) —
    /// appropriate for native adapters. Hosts that can land on GL or
    /// browser adapters must instead route through [`Self::with_caps`]
    /// with [`RunnerCaps::from_adapter`], otherwise stock pipelines fail
    /// naga validation on shader-module creation.
    pub fn with_sample_count(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        Self::with_caps(
            device,
            queue,
            target_format,
            sample_count,
            RunnerCaps::default(),
        )
    }

    /// Like [`Self::with_sample_count`], but with the adapter caps
    /// supplied explicitly — see [`RunnerCaps`] for what each cap gates:
    ///
    /// ```ignore
    /// Runner::with_caps(&device, &queue, format, sample_count,
    ///                   RunnerCaps::from_adapter(&adapter))
    /// ```
    pub fn with_caps(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        caps: RunnerCaps,
    ) -> Self {
        Self::with_caps_inner(device, queue, target_format, sample_count, caps, None)
    }

    /// Like [`Self::with_caps`], but attached to an existing
    /// [`SharedText`] pool instead of creating a private one (issue
    /// #94). Every runner attached to the same pool shares one font
    /// system, one shaping cache, and one set of glyph/MSDF atlas
    /// pages on the GPU — a multi-window host pays glyph
    /// rasterization, warm-up, and atlas VRAM once per *device*
    /// instead of once per window:
    ///
    /// ```ignore
    /// let text = SharedText::new(&device);
    /// text.warm_default_glyphs(); // once, off the open path
    /// let runner_a = Runner::with_shared_text(&device, &queue, fmt_a, 1, caps, &text);
    /// let runner_b = Runner::with_shared_text(&device, &queue, fmt_b, 1, caps, &text);
    /// ```
    ///
    /// The pool is device-scoped and format/sample-count independent —
    /// runners with different swapchain formats or MSAA settings can
    /// share one pool. An existing runner's pool is available via
    /// [`Self::shared_text`]. The pool must have been created on the
    /// same `wgpu::Device`.
    pub fn with_shared_text(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        caps: RunnerCaps,
        shared: &SharedText,
    ) -> Self {
        Self::with_caps_inner(
            device,
            queue,
            target_format,
            sample_count,
            caps,
            Some(shared.clone()),
        )
    }

    fn with_caps_inner(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        caps: RunnerCaps,
        shared_text: Option<SharedText>,
    ) -> Self {
        let RunnerCaps {
            per_sample_shading,
            depth_readback,
        } = caps;
        // ---- Shared resources ----
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::frame_uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let frame_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("damascene_wgpu::frame_bind_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let quad_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("damascene_wgpu::frame_bind_group"),
            layout: &frame_bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });

        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("damascene_wgpu::quad_vbo"),
            // Triangle strip: 4 corners, uv 0..1.
            contents: bytemuck::cast_slice::<f32, u8>(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("damascene_wgpu::pipeline_layout"),
            bind_group_layouts: &[Some(&frame_bind_layout)],
            immediate_size: 0,
        });

        // ---- Backdrop sampling resources ----
        //
        // Custom shaders that opt into backdrop sampling (registered
        // via `register_shader_with(..samples_backdrop=true)`) get a
        // pipeline layout with `@group(1)` for the snapshot texture
        // and sampler. The bind group is rebuilt whenever the
        // snapshot is (re)allocated.
        let backdrop_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("damascene_wgpu::backdrop_bind_layout"),
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
        let backdrop_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("damascene_wgpu::backdrop_pipeline_layout"),
                bind_group_layouts: &[Some(&frame_bind_layout), Some(&backdrop_bind_layout)],
                immediate_size: 0,
            });
        let backdrop_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("damascene_wgpu::backdrop_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Build stock rect-shaped pipelines up-front; custom shaders are
        // added on demand by the host.
        let mut pipelines = HashMap::new();
        build_stock_quad_pipelines(
            &mut pipelines,
            device,
            &pipeline_layout,
            target_format,
            sample_count,
            per_sample_shading,
        );

        // Text pipeline + atlas (replaces glyphon). Attaches to the
        // caller's shared pool when given one; otherwise the runner
        // gets a private pool (single-window behavior, unchanged).
        let text_paint = match shared_text {
            Some(shared) => TextPaint::with_shared(
                device,
                target_format,
                sample_count,
                &frame_bind_layout,
                shared,
            ),
            None => TextPaint::new(device, target_format, sample_count, &frame_bind_layout),
        };
        let icon_paint = IconPaint::new(device, target_format, sample_count, &frame_bind_layout);
        let image_paint = ImagePaint::new(device, target_format, sample_count, &frame_bind_layout);
        let surface_paint =
            SurfacePaint::new(device, target_format, sample_count, &frame_bind_layout);
        let scene_paint = Scene3DPaint::new(
            device,
            target_format,
            sample_count,
            &frame_bind_layout,
            damascene_core::paint::DEFAULT_WORKING_COLOR_SPACE,
            depth_readback,
        );

        let mut core = RunnerCore::new();
        core.quad_scratch = Vec::with_capacity(INITIAL_INSTANCE_CAPACITY);

        Self {
            target_format,
            sample_count,
            per_sample_shading,
            pipeline_layout,
            backdrop_pipeline_layout,
            quad_bind_group,
            backdrop_bind_layout,
            backdrop_sampler,
            frame_buf,
            quad_vbo,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            pipelines,
            backdrop_shaders: HashSet::new(),
            time_shaders: HashSet::new(),
            custom_shaders: HashMap::new(),
            text_paint,
            icon_paint,
            image_paint,
            surface_paint,
            scene_paint,
            snapshot: None,
            backdrop_bind_group: None,
            backdrop_copy_unsupported_warned: false,
            start_time: Instant::now(),
            white_scale: 1.0,
            headroom: 1.0,
            ref_nits: damascene_core::color::BT2408_REFERENCE_WHITE_NITS,
            core,
        }
    }

    /// Tell the runner the swapchain texture size in physical pixels.
    /// Call this once after `surface.configure(...)` and again on every
    /// `WindowEvent::Resized`. The runner uses this as the canonical
    /// `viewport_px` for scissor math; without it, the value is derived
    /// from `viewport.w * scale_factor`, which can drift by one pixel
    /// when `scale_factor` is fractional and trip wgpu's
    /// `set_scissor_rect` validation.
    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.core.set_surface_size(width, height);
    }

    /// Set the color space the renderer composites in. Hosts call this
    /// once after negotiating a surface format with the display server
    /// (see `damascene-winit-wgpu`) and before the first frame. Updates the
    /// shared quad path (via `RunnerCore`) and this backend's text /
    /// icon / image color recorders so every color crosses the working-
    /// space boundary consistently.
    ///
    /// The working space must match how the swapchain interprets the
    /// pixels the renderer writes: `SRGB_LINEAR` for an `*_unorm_srgb`
    /// surface (the default), `SCRGB_LINEAR` / `DISPLAY_P3_LINEAR` for
    /// an `Rgba16Float` surface, etc.
    pub fn set_working_color_space(&mut self, space: damascene_core::color::ColorSpace) {
        self.core.set_working_color_space(space);
        self.text_paint.set_working_color_space(space);
        self.icon_paint.set_working_color_space(space);
        self.image_paint.set_working_color_space(space);
        self.scene_paint.set_working_color_space(space);
    }

    /// The color space the renderer currently composites in.
    pub fn working_color_space(&self) -> damascene_core::color::ColorSpace {
        self.core.working_color_space()
    }

    /// Rebuild every swapchain-format-bound render pipeline for a new
    /// surface format, in place, preserving all other runner state.
    ///
    /// The `damascene-winit-wgpu` host calls this on **live color
    /// renegotiation** — when the display server hands back a different
    /// surface format than the one the runner was built with (e.g.
    /// `Bgra8UnormSrgb` ↔ `Rgba16Float` when HDR turns on or off). The
    /// swapchain format is baked into every pipeline's `ColorTargetState`,
    /// so those pipelines must be recreated; everything else can stay.
    ///
    /// **What survives:** all interaction state in `RunnerCore` (hover,
    /// focus, press, selection, scroll, hotkeys, the laid-out tree
    /// snapshot), the glyph + icon MSDF atlases and their GPU page
    /// textures, the per-image and app-texture/surface bind-group caches,
    /// the scene geometry caches and per-node offscreen targets, and every
    /// instance/uniform/vertex buffer. No atlas re-rasterization, no
    /// texture re-upload, no layout recompute.
    ///
    /// **What's rebuilt:** the four stock quad pipelines (rounded_rect,
    /// spinner, skeleton, progress_indeterminate), every retained custom
    /// shader pipeline, and the swapchain-bound pipelines inside each paint
    /// module (text color/MSDF/highlight, icon flat/relief/glass/MSDF,
    /// image, surface premul/straight/opaque, and the scene composite —
    /// the scene's offscreen point/line/mesh + occlusion pipelines render
    /// to fixed formats and are left alone). The backdrop snapshot texture
    /// is dropped so it reallocates in the new format on the next
    /// backdrop-sampling frame.
    ///
    /// Early-returns when `format` already matches the current target.
    /// `sample_count` and `per_sample_shading` are unaffected.
    pub fn set_target_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if format == self.target_format {
            return;
        }
        self.target_format = format;

        // Stock quad pipelines (replaces the four entries in place).
        build_stock_quad_pipelines(
            &mut self.pipelines,
            device,
            &self.pipeline_layout,
            format,
            self.sample_count,
            self.per_sample_shading,
        );

        // Retained custom shader pipelines. Same layout selection as
        // `register_shader_with`: backdrop-sampling shaders bind `@group(1)`.
        for (name, (wgsl, samples_backdrop)) in &self.custom_shaders {
            let layout = if *samples_backdrop {
                &self.backdrop_pipeline_layout
            } else {
                &self.pipeline_layout
            };
            let pipeline = build_quad_pipeline(
                device,
                layout,
                format,
                self.sample_count,
                &format!("custom::{name}"),
                wgsl,
                self.per_sample_shading,
            );
            self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
        }

        // Per-paint-module swapchain-bound pipelines.
        self.text_paint.set_target_format(device, format);
        self.icon_paint.set_target_format(device, format);
        self.image_paint.set_target_format(device, format);
        self.surface_paint.set_target_format(device, format);
        self.scene_paint.set_target_format(device, format);

        // The backdrop snapshot texture is created in the target format
        // (see `ensure_snapshot`); drop it so the next backdrop-sampling
        // frame lazily reallocates it in the new format. The bind group
        // referencing it goes too — it's rebuilt alongside the texture.
        self.snapshot = None;
        self.backdrop_bind_group = None;
    }

    /// Set the output white-level scale (default 1.0). Leave at 1.0
    /// whenever the surface puts reference white at signal 1.0: 8-bit
    /// sRGB by definition, and Wayland float swapchains tagged as
    /// parametric ext-linear (the WSI default — the compositor anchors
    /// signal 1.0 to the output reference; scaling on top double-lifts
    /// ~2.5×). Pass
    /// [`damascene_core::color::WINDOWS_SCRGB_WHITE_SCALE`] only when
    /// the surface genuinely reads as Windows scRGB — signal 1.0 =
    /// 80 cd/m² *absolute*, assumed reference white at 2.5375 (203
    /// cd/m², BT.2408) — so SDR-referred UI white lands at the
    /// reference level instead of 80 nits.
    pub fn set_white_scale(&mut self, scale: f32) {
        self.white_scale = scale;
    }

    /// Set the output's luminance frame: `headroom` = usable range in
    /// multiples of reference white (`target_max / reference`; 1.0 on
    /// SDR — the default — or `f32::INFINITY` when the output declared
    /// no maximum) and `reference_nits` = the output's reference white
    /// in cd/m² (default 203, BT.2408). Feeds
    /// `FrameUniforms.headroom/ref_nits` and the per-image HDR
    /// remaster: image draws whose measured content peak exceeds their
    /// [`damascene_core::image::DynamicRangeLimit`] resolved against
    /// this headroom are rolled off (BT.2390) to fit. Hosts re-call
    /// this whenever the output's preferred description changes.
    pub fn set_output_luminance(&mut self, headroom: f32, reference_nits: f32) {
        self.headroom = headroom;
        self.ref_nits = reference_nits;
        self.image_paint.set_headroom(headroom);
    }

    /// Set the theme used to resolve implicit widget surfaces to shaders.
    /// Pre-rasterize printable ASCII for the bundled default faces
    /// (Inter Variable + JetBrains Mono Variable). Pays the ~40ms
    /// one-time MSDF-generation cost up-front so the first frame that
    /// introduces each character doesn't take a 20-30ms paint hit.
    /// Hosts that interactively render UI text (the showcase, custom
    /// apps, etc.) should call this once after constructing the
    /// `Runner` and before the first frame; headless fixtures that
    /// render only static content can skip it. MSDF keys are
    /// size-independent so each character is rasterized exactly once
    /// and reused for every size + weight afterwards.
    pub fn warm_default_glyphs(&mut self) {
        let start = std::time::Instant::now();
        self.text_paint.warm_default_glyphs();
        // ~40ms optimized; ~19s at opt-level 0 (MSDF generation is the
        // cost). A debug build paying the cliff almost always means the
        // consumer workspace is missing the dev-profile overrides — say
        // so instead of reading as "damascene takes 20s to start".
        let elapsed = start.elapsed();
        if elapsed > std::time::Duration::from_secs(2) {
            log::warn!(
                "damascene-wgpu: warm_default_glyphs took {elapsed:.1?} — unoptimized MSDF                  generation. Add the [profile.dev.package] opt-level overrides from                  damascene's README to your workspace root Cargo.toml (and don't call                  this inside a Wayland dispatch callback in debug builds)."
            );
        }
    }

    /// Pre-rasterize a chosen set of `(family, char)` glyphs — the
    /// app-selectable counterpart to [`Self::warm_default_glyphs`], for
    /// fonts you registered yourself or glyph sets beyond printable
    /// ASCII. See [`SharedText::warm_glyphs`].
    pub fn warm_glyphs(&mut self, families: &[damascene_core::tree::FontFamily], chars: &[char]) {
        self.text_paint.warm_glyphs(families, chars);
    }

    /// Serialize the resident outline-glyph atlas into a portable
    /// snapshot blob (keyed by font content hash). Persist it and reload
    /// with [`Self::import_msdf_snapshot`] to skip regenerating those
    /// glyphs on a later run — the app-driven equivalent of the built-in
    /// `prebaked-default-fonts` bake. See [`SharedText::export_msdf_snapshot`].
    pub fn export_msdf_snapshot(&self) -> Vec<u8> {
        self.text_paint.export_msdf_snapshot()
    }

    /// Load a snapshot from [`Self::export_msdf_snapshot`], resolving
    /// fonts by content hash against those currently loaded. Returns the
    /// glyph count loaded, or an error if the blob is stale/unreadable
    /// (warm live in that case). See [`SharedText::import_msdf_snapshot`].
    pub fn import_msdf_snapshot(
        &mut self,
        bytes: &[u8],
    ) -> Result<usize, damascene_core::text::msdf_snapshot::SnapshotError> {
        self.text_paint.import_msdf_snapshot(bytes)
    }

    /// The [`SharedText`] pool this runner records text into. Hand it
    /// to [`Self::with_shared_text`] when constructing further runners
    /// on the same device so they share fonts, shaping, and atlases —
    /// works whether this runner was built with a shared pool or owns
    /// a private one.
    pub fn shared_text(&self) -> SharedText {
        self.text_paint.shared().clone()
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.icon_paint.set_material(theme.icon_material());
        self.core.set_theme(theme);
    }

    pub fn theme(&self) -> &Theme {
        self.core.theme()
    }

    /// Select the stock material used by the vector-icon painter.
    /// Prefer [`Theme::with_icon_material`] for app-level routing; this
    /// remains useful for low-level render fixtures.
    pub fn set_icon_material(&mut self, material: IconMaterial) {
        self.icon_paint.set_material(material);
    }

    pub fn icon_material(&self) -> IconMaterial {
        self.icon_paint.material()
    }

    /// Register a custom shader. `name` is the same string passed to
    /// `damascene_core::shader::ShaderBinding::custom`; nodes bound to it
    /// via [`El::shader`](damascene_core::tree::El) paint through this
    /// pipeline.
    ///
    /// The WGSL source must use the shared `(rect, vec_a, vec_b, vec_c)`
    /// instance layout and the `FrameUniforms` bind group described in
    /// the module docs. Compilation happens at register time — invalid
    /// WGSL panics here, not mid-frame.
    ///
    /// Re-registering the same name replaces the previous pipeline
    /// (useful for hot-reload during development).
    pub fn register_shader(&mut self, device: &wgpu::Device, name: &'static str, wgsl: &str) {
        self.register_shader_with(device, name, wgsl, false, false);
    }

    /// Register a custom shader, with opt-in flags for backdrop
    /// sampling and time-driven motion.
    ///
    /// `samples_backdrop=true` schedules the shader's draws into
    /// Pass B (after a snapshot of Pass A's rendered content) and
    /// binds the snapshot texture as `@group(2) binding=0`
    /// (`backdrop_tex`) plus a sampler at `binding=1`
    /// (`backdrop_smp`). See `docs/SHADER_VISION.md` §"Backdrop
    /// sampling architecture". Backdrop depth is capped at 1.
    ///
    /// `samples_time=true` declares that the shader's output depends
    /// on `frame.time`. The runtime ORs this into
    /// [`PrepareResult::needs_redraw`] for any frame that has at
    /// least one node bound to the shader, so the host idle loop
    /// keeps ticking without a per-El opt-in. Stock shaders self-
    /// report through [`damascene_core::shader::StockShader::is_continuous`];
    /// this flag is the same signal for app-registered WGSL.
    pub fn register_shader_with(
        &mut self,
        device: &wgpu::Device,
        name: &'static str,
        wgsl: &str,
        samples_backdrop: bool,
        samples_time: bool,
    ) {
        let label = format!("custom::{name}");
        let layout = if samples_backdrop {
            &self.backdrop_pipeline_layout
        } else {
            &self.pipeline_layout
        };
        let pipeline = build_quad_pipeline(
            device,
            layout,
            self.target_format,
            self.sample_count,
            &label,
            wgsl,
            self.per_sample_shading,
        );
        self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
        // Retain the source so the pipeline can be rebuilt against a new
        // swapchain format in `set_target_format`. Re-registering replaces
        // the prior entry, matching the pipeline-map replacement above.
        self.custom_shaders
            .insert(name, (wgsl.to_string(), samples_backdrop));
        // Introspect the instance-attribute names so this shader's
        // uniforms route by WGSL field name with Rust↔WGSL drift
        // detection (issue #99). Failure is non-fatal: positional
        // vec_a..vec_e routing still works.
        match damascene_core::paint::slots::introspect_wgsl(wgsl) {
            Ok(map) => self.core.register_shader_slots(name, map),
            Err(e) => log::warn!(
                "damascene-wgpu: could not introspect shader `{name}` for named uniform \
                 routing ({e}); positional vec_a..vec_e routing still applies"
            ),
        }
        if samples_backdrop {
            self.backdrop_shaders.insert(name);
        } else {
            self.backdrop_shaders.remove(name);
        }
        if samples_time {
            self.time_shaders.insert(name);
        } else {
            self.time_shaders.remove(name);
        }
    }

    /// Borrow the internal [`UiState`] — primarily for headless fixtures
    /// that want to look up a node's rect after `prepare` (e.g., to
    /// simulate a pointer at a specific button's center).
    pub fn ui_state(&self) -> &UiState {
        self.core.ui_state()
    }

    /// One-line diagnostic snapshot of interactive state — passes through
    /// to [`UiState::debug_summary`]. Intended for per-frame logging
    /// (e.g., `console.log` from the wasm host while debugging hover /
    /// animation glitches).
    pub fn debug_summary(&self) -> String {
        self.core.debug_summary()
    }

    /// Return the most recently laid-out rectangle for a keyed node.
    ///
    /// Call after [`Self::prepare`]. This is the host-composition hook:
    /// reserve a keyed Damascene element in the UI tree, ask for its rect
    /// here, then record host-owned rendering into that region using the
    /// same encoder / render flow that surrounds Damascene's pass.
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.core.rect_of_key(key)
    }

    /// Pointer cursor resolved from the snapshot tree [`Self::prepare`]
    /// just stored. Call after `prepare`; paint-only frames keep the
    /// previously resolved cursor.
    pub fn snapshot_cursor(&self) -> damascene_core::cursor::Cursor {
        self.core.snapshot_cursor()
    }

    /// Lay out the tree, resolve to draw ops, and upload per-frame
    /// buffers (quad instances + glyph atlas). Must be called before
    /// [`Self::draw`] and outside of any render pass.
    ///
    /// `viewport` is in **logical** pixels — the units the layout pass
    /// works in. `scale_factor` is the HiDPI multiplier (1.0 on a
    /// regular display, 2.0 on most modern HiDPI, can be fractional).
    /// The host's render-pass target should be sized at physical pixels
    /// (`viewport × scale_factor`); the runner maps logical → physical
    /// internally so layout, fonts, and SDF math stay device-independent.
    ///
    /// Takes the tree by value: after layout it becomes the hit-test
    /// snapshot directly (no whole-tree clone). Read post-layout state
    /// through the runner (e.g. [`Self::snapshot_cursor`]).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut root: El,
        viewport: Rect,
        scale_factor: f32,
    ) -> PrepareResult {
        let mut timings = PrepareTimings::default();

        // Install any scene depth maps that finished reading back (a frame
        // or two late) so this frame's `draw_ops` can occlude scene-anchored
        // labels behind geometry. Done before `prepare_layout` runs the
        // draw-op pass. Stale maps for scenes that left the tree are GC'd.
        let ready_depth = self.scene_paint.collect_depth_maps(device);
        if !ready_depth.is_empty() {
            let depth_maps = self.core.ui_state.scene_depth_mut();
            for (id, map) in ready_depth {
                depth_maps.insert(id, map);
            }
        }
        self.core
            .ui_state
            .scene_depth_mut()
            .retain(|id, _| self.scene_paint.has_target(id));

        // Layout + state apply + animation tick + draw_ops resolution.
        // Writes timings.layout + timings.draw_ops. The closure feeds
        // the runtime's continuous-redraw scan: any node bound to a
        // shader registered with `samples_time=true` keeps the host
        // loop ticking even when no animation is settling.
        let time_shaders = &self.time_shaders;
        let LayoutPrepared {
            ops,
            mut needs_redraw,
            mut next_layout_redraw_in,
            next_paint_redraw_in,
        } =
            self.core
                .prepare_layout(&mut root, viewport, scale_factor, &mut timings, |handle| {
                    match handle {
                        ShaderHandle::Custom(name) => time_shaders.contains(name),
                        ShaderHandle::Stock(_) => false,
                    }
                });

        // Paint stream: pack quads, record text, preserve z-order. The
        // closure is the wgpu-specific "is this shader registered?"
        // query (different pipeline types per backend prevent moving the
        // check itself into core).
        self.text_paint.frame_begin();
        self.icon_paint.frame_begin();
        self.image_paint.frame_begin();
        self.surface_paint.frame_begin();
        self.scene_paint.frame_begin();
        let pipelines = &self.pipelines;
        let backdrop_shaders = &self.backdrop_shaders;
        let mut recorder = PaintRecorder {
            text: &mut self.text_paint,
            icons: &mut self.icon_paint,
            images: &mut self.image_paint,
            surfaces: &mut self.surface_paint,
            scenes: &mut self.scene_paint,
            device,
            queue,
        };
        self.core.prepare_paint(
            &ops,
            |shader| pipelines.contains_key(shader),
            |shader| match shader {
                ShaderHandle::Custom(name) => backdrop_shaders.contains(name),
                ShaderHandle::Stock(_) => false,
            },
            &mut recorder,
            scale_factor,
            &mut timings,
        );

        // GPU upload — wgpu-specific. Resize the instance buffer if
        // needed, then write quad_scratch + frame uniforms + flush text
        // atlas dirty regions. Wrapped in its own scope so the
        // `prepare::gpu_upload` span doesn't bleed into the subsequent
        // `snapshot` call (which carries its own span).
        {
            damascene_core::profile_span!("prepare::gpu_upload");
            let t_paint_end = Instant::now();
            if self.core.quad_scratch.len() > self.instance_capacity {
                let new_cap = self.core.quad_scratch.len().next_power_of_two();
                self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("damascene_wgpu::instance_buf (resized)"),
                    size: (new_cap * std::mem::size_of::<QuadInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.instance_capacity = new_cap;
            }
            if !self.core.quad_scratch.is_empty() {
                queue.write_buffer(
                    &self.instance_buf,
                    0,
                    bytemuck::cast_slice(&self.core.quad_scratch),
                );
            }
            self.text_paint.flush(device, queue);
            self.icon_paint.flush(device, queue);
            self.image_paint.flush(device, queue);
            self.surface_paint.flush(device, queue);
            self.scene_paint.flush(device, queue);
            // Pin time to 0 in Settled mode so headless fixtures rendering
            // a time-driven shader (e.g. stock::spinner) stay byte-identical
            // run-to-run, the same way `Animation::settle()` makes the
            // spring/tween path deterministic for SVG/PNG snapshots.
            let time = match self.core.ui_state().animation_mode() {
                damascene_core::AnimationMode::Settled => 0.0,
                damascene_core::AnimationMode::Live => {
                    (Instant::now() - self.start_time).as_secs_f32()
                }
            };
            let frame = FrameUniforms {
                viewport: [viewport.w, viewport.h],
                time,
                scale_factor,
                white_scale: self.white_scale,
                headroom: self.headroom,
                ref_nits: self.ref_nits,
                _reserved: 0.0,
            };
            queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));
            timings.gpu_upload = Instant::now() - t_paint_end;
        }

        // Snapshot the laid-out tree for next-frame hit-testing —
        // moved, not cloned; the tree is rebuilt next frame anyway.
        self.core.snapshot_owned(root, &mut timings);

        // Move resolved ops into the core's cache so a subsequent
        // paint-only frame can reuse them without re-running layout.
        self.core.last_ops = ops;

        // Damascene renders lazily, but the label-occlusion depth read-back needs
        // a few frames to resolve. Keep frames coming until every labelled
        // scene has a depth map matching its current pose — otherwise a
        // capture started in `render` would sit unmapped after the camera
        // settles and the labels would never appear. Settled + current scenes
        // (and label-free ones) report `false`, so lazy idle is preserved.
        //
        // This must drive `next_layout_redraw_in`, not just `needs_redraw`:
        // hosts schedule the next frame off the deadline lanes (the winit
        // host ignores `needs_redraw`), and it must be the *layout* lane, not
        // the paint lane — the paint-only `repaint` path skips
        // `collect_depth_maps`, so only a full `prepare` advances the readback.
        if self.scene_paint.occlusion_unsettled() {
            needs_redraw = true;
            next_layout_redraw_in = Some(std::time::Duration::ZERO);
        }

        let next_redraw_in = match (next_layout_redraw_in, next_paint_redraw_in) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(d), None) | (None, Some(d)) => Some(d),
            (None, None) => None,
        };
        PrepareResult {
            needs_redraw,
            next_redraw_in,
            next_layout_redraw_in,
            next_paint_redraw_in,
            timings,
        }
    }

    /// Paint-only frame: rerun [`RunnerCore::prepare_paint_cached`] +
    /// GPU upload + frame-uniform write against the cached ops from
    /// the most recent [`Self::prepare`] call. Skips rebuild + layout
    /// + draw_ops + snapshot — only `frame.time` advances.
    ///
    /// Hosts call this when [`PrepareResult::next_paint_redraw_in`]
    /// fires (a time-driven shader needs another frame) and no input
    /// has been processed since the last full prepare. Input always
    /// upgrades to the full `prepare(...)` path.
    ///
    /// `viewport` and `scale_factor` must match the values passed to
    /// the most recent `prepare(...)` — a resize must go through the
    /// full layout path. Returns the same shape of [`PrepareResult`]
    /// for diagnostic continuity, with both deadlines re-computed
    /// from the cached signals: `next_layout_redraw_in` is `None` (we
    /// didn't re-evaluate), and `next_paint_redraw_in` is whatever
    /// the cached ops still report. The host owns the layout
    /// deadline across paint-only frames.
    pub fn repaint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: Rect,
        scale_factor: f32,
    ) -> PrepareResult {
        let mut timings = PrepareTimings::default();

        self.text_paint.frame_begin();
        self.icon_paint.frame_begin();
        self.image_paint.frame_begin();
        self.surface_paint.frame_begin();
        self.scene_paint.frame_begin();
        let pipelines = &self.pipelines;
        let backdrop_shaders = &self.backdrop_shaders;
        let mut recorder = PaintRecorder {
            text: &mut self.text_paint,
            icons: &mut self.icon_paint,
            images: &mut self.image_paint,
            surfaces: &mut self.surface_paint,
            scenes: &mut self.scene_paint,
            device,
            queue,
        };
        self.core.prepare_paint_cached(
            |shader| pipelines.contains_key(shader),
            |shader| match shader {
                ShaderHandle::Custom(name) => backdrop_shaders.contains(name),
                ShaderHandle::Stock(_) => false,
            },
            &mut recorder,
            scale_factor,
            &mut timings,
        );

        // Same GPU-upload block as prepare(); time advances even though
        // ops are unchanged so time-driven shaders animate.
        {
            damascene_core::profile_span!("repaint::gpu_upload");
            let t_paint_end = Instant::now();
            if self.core.quad_scratch.len() > self.instance_capacity {
                let new_cap = self.core.quad_scratch.len().next_power_of_two();
                self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("damascene_wgpu::instance_buf (resized)"),
                    size: (new_cap * std::mem::size_of::<QuadInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.instance_capacity = new_cap;
            }
            if !self.core.quad_scratch.is_empty() {
                queue.write_buffer(
                    &self.instance_buf,
                    0,
                    bytemuck::cast_slice(&self.core.quad_scratch),
                );
            }
            self.text_paint.flush(device, queue);
            self.icon_paint.flush(device, queue);
            self.image_paint.flush(device, queue);
            self.surface_paint.flush(device, queue);
            self.scene_paint.flush(device, queue);
            let time = match self.core.ui_state().animation_mode() {
                AnimationMode::Settled => 0.0,
                AnimationMode::Live => (Instant::now() - self.start_time).as_secs_f32(),
            };
            let frame = FrameUniforms {
                viewport: [viewport.w, viewport.h],
                time,
                scale_factor,
                white_scale: self.white_scale,
                headroom: self.headroom,
                ref_nits: self.ref_nits,
                _reserved: 0.0,
            };
            queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));
            timings.gpu_upload = Instant::now() - t_paint_end;
        }

        // Re-evaluate the paint lane against the cached ops so the host
        // can re-arm the deadline. Cheap (one scan over already-resolved
        // ops). The layout lane is left as `None`: we didn't re-run
        // `prepare_layout`, so we have no fresh signal to report — the
        // host's previously-set layout deadline still stands.
        let time_shaders = &self.time_shaders;
        let next_paint_redraw_in = self.core.scan_continuous_shaders(|handle| match handle {
            ShaderHandle::Custom(name) => time_shaders.contains(name),
            ShaderHandle::Stock(_) => false,
        });
        PrepareResult {
            needs_redraw: next_paint_redraw_in.is_some(),
            next_redraw_in: next_paint_redraw_in,
            next_layout_redraw_in: None,
            next_paint_redraw_in,
            timings,
        }
    }

    // ---- Input plumbing ----
    //
    // The host (winit-side) calls these from its event loop.
    // Coordinates are **logical pixels** — divide winit's physical
    // PhysicalPosition by the window scale factor before handing them in.

    /// Update pointer position and recompute the hovered key.
    /// Returns the new hovered key, if any (host can use it for cursor
    /// styling or to decide whether to call `request_redraw`).
    /// Pointer moved to `p.x, p.y` (logical px). Returns the events to
    /// dispatch via `App::on_event` plus a `needs_redraw` flag — see
    /// [`PointerMove`] for why hosts must gate `request_redraw` on
    /// the flag. The hovered node is updated on `ui_state().hovered`
    /// regardless. Mouse-only hosts can construct `p` via
    /// [`Pointer::moving`].
    pub fn pointer_moved(&mut self, p: Pointer) -> PointerMove {
        self.core.pointer_moved(p)
    }

    /// Pointer left the window — clear hover/press. Returns a
    /// `PointerLeave` event for the previously hovered target (when
    /// there was one); hosts should route the events through
    /// `App::on_event` like the other pointer entry points.
    pub fn pointer_left(&mut self) -> Vec<damascene_core::UiEvent> {
        self.core.pointer_left()
    }

    /// The platform cancelled the pointer sequence (touch cancel /
    /// `pointercancel`) — abandons in-flight presses and gesture
    /// captures without applying release effects. Route the events
    /// through `App::on_event` like the other pointer entry points.
    pub fn pointer_cancelled(&mut self) -> Vec<damascene_core::UiEvent> {
        self.core.pointer_cancelled()
    }

    /// File is being dragged over the window. Hosts call this from
    /// `winit::WindowEvent::HoveredFile` (one call per file). Returns
    /// the `FileHovered` event routed to the keyed leaf at the cursor
    /// (or window-level if outside any keyed surface).
    pub fn file_hovered(
        &mut self,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    ) -> Vec<damascene_core::UiEvent> {
        self.core.file_hovered(path, x, y)
    }

    /// File hover ended without a drop — hosts call this from
    /// `winit::WindowEvent::HoveredFileCancelled`. Window-level event
    /// (not routed); apps clear any drop-zone affordance.
    pub fn file_hover_cancelled(&mut self) -> Vec<damascene_core::UiEvent> {
        self.core.file_hover_cancelled()
    }

    /// File was dropped on the window. Hosts call this from
    /// `winit::WindowEvent::DroppedFile` (one call per file).
    pub fn file_dropped(
        &mut self,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    ) -> Vec<damascene_core::UiEvent> {
        self.core.file_dropped(path, x, y)
    }

    /// Whether a primary press at `(x, y)` (logical px) would land
    /// on a node that opted into `capture_keys` — the marker the
    /// library uses for text-input-style widgets. Hosts query this
    /// from a DOM pointerdown handler to decide whether to focus
    /// a hidden textarea (so the soft keyboard can open in the
    /// user-gesture context). See
    /// [`RunnerCore::would_press_focus_text_input`] for details.
    pub fn would_press_focus_text_input(&self, x: f32, y: f32) -> bool {
        self.core.would_press_focus_text_input(x, y)
    }

    /// Whether the currently focused node is a text-input-style
    /// widget (i.e. has `capture_keys` set). Hosts mirror this each
    /// frame into platform affordances such as the on-screen
    /// keyboard or IME compose-window placement.
    pub fn focused_captures_keys(&self) -> bool {
        self.core.focused_captures_keys()
    }

    /// Pointer pressed at `p.x, p.y` (logical px) for `p.button`. For
    /// `Primary`, records the pressed key for press-visual feedback,
    /// updates focus, and returns a `PointerDown` event so widgets that
    /// need to react at down-time (text input selection anchor,
    /// draggable handles) can do so. For `Secondary` / `Middle`, records
    /// on a side channel and returns `None`. The actual click event
    /// fires on `pointer_up`. Mouse-only hosts can construct `p` via
    /// [`Pointer::mouse`].
    pub fn pointer_down(&mut self, p: Pointer) -> Vec<UiEvent> {
        self.core.pointer_down(p)
    }

    /// Replace the tracked modifier mask. Hosts call this from their
    /// platform's "modifiers changed" hook so subsequent pointer
    /// events (PointerDown, Drag, Click, …) stamp the current mask
    /// into `UiEvent.modifiers`.
    pub fn set_modifiers(&mut self, modifiers: KeyModifiers) {
        self.core.ui_state.set_modifiers(modifiers);
    }

    /// Pointer released at `p.x, p.y` for `p.button`. Returns the
    /// events the host should dispatch in order: for `Primary`, always
    /// a `PointerUp` (when there was a corresponding down) followed
    /// by an optional `Click` (when the up landed on the down's
    /// node). For `Secondary` / `Middle`, an optional `SecondaryClick`
    /// / `MiddleClick` on the same-node match. Mouse-only hosts can
    /// construct `p` via [`Pointer::mouse`].
    pub fn pointer_up(&mut self, p: Pointer) -> Vec<UiEvent> {
        self.core.pointer_up(p)
    }

    pub fn key_down(
        &mut self,
        logical: LogicalKey,
        physical: PhysicalKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Vec<UiEvent> {
        self.core.key_down(logical, physical, modifiers, repeat)
    }

    /// Forward an OS-composed text-input string (winit's keyboard event
    /// `.text` field, or an `Ime::Commit`) to the focused element as a
    /// `TextInput` event.
    pub fn text_input(&mut self, text: String) -> Option<UiEvent> {
        self.core.text_input(text)
    }

    /// Replace the hotkey registry. Call once per frame, after `app.build()`,
    /// passing `app.hotkeys()` so chords stay in sync with state.
    ///
    /// The registry is scoped to this `Runner` — in a multi-window
    /// host (one `Runner` per window), pass each window only its own
    /// list and feed each window's key events only to its own
    /// `Runner`; chords then fire per focused window. See
    /// `damascene_core::App::hotkeys` for the full convention.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.core.set_hotkeys(hotkeys);
    }

    /// Push the app's current selection to the runtime so the painter
    /// can draw highlight bands. Hosts call this once per frame
    /// alongside [`Self::set_hotkeys`].
    pub fn set_selection(&mut self, selection: damascene_core::selection::Selection) {
        self.core.set_selection(selection);
    }

    /// Resolve the runtime's current selection to a text payload from
    /// the most recently laid-out tree. See
    /// [`RunnerCore::selected_text`] — virtual-list rows are realized
    /// during layout, so a freshly built app tree would miss them and
    /// a `Ctrl+C` lookup that walked it would silently come back empty.
    pub fn selected_text(&self) -> Option<String> {
        self.core.selected_text()
    }

    /// Resolve an explicit [`damascene_core::selection::Selection`] against
    /// the last laid-out tree. See [`RunnerCore::selected_text_for`].
    pub fn selected_text_for(
        &self,
        selection: &damascene_core::selection::Selection,
    ) -> Option<String> {
        self.core.selected_text_for(selection)
    }

    /// Queue toast specs onto the runtime's toast stack. Hosts call
    /// this once per frame with `app.drain_toasts()`. Each spec is
    /// stamped with a monotonic id and an `expires_at` deadline
    /// (`now + ttl`); the next `prepare` call drops expired entries
    /// and synthesizes a `toast_stack` floating layer over the rest.
    pub fn push_toasts(&mut self, specs: Vec<damascene_core::toast::ToastSpec>) {
        self.core.push_toasts(specs);
    }

    /// Programmatically dismiss a toast by id. Useful for cancelling
    /// long-TTL toasts when an external condition resolves (e.g.,
    /// "reconnecting…" turning into "connected").
    pub fn dismiss_toast(&mut self, id: u64) {
        self.core.dismiss_toast(id);
    }

    /// Queue programmatic focus requests by widget key. Hosts call
    /// this once per frame with `app.drain_focus_requests()`. Each
    /// key is resolved during the next `prepare` against the rebuilt
    /// focus order; unmatched keys drop silently.
    pub fn push_focus_requests(&mut self, keys: Vec<String>) {
        self.core.push_focus_requests(keys);
    }

    /// Queue programmatic scroll-to-row requests targeting virtual
    /// lists by key. Hosts call this once per frame with
    /// `app.drain_scroll_requests()`. Each request is consumed during
    /// the next `prepare` by the layout pass for the matching list,
    /// where viewport height and row heights are known. Unmatched
    /// list keys and out-of-range row indices drop silently.
    pub fn push_scroll_requests(&mut self, requests: Vec<damascene_core::scroll::ScrollRequest>) {
        self.core.push_scroll_requests(requests);
    }

    pub fn push_viewport_requests(
        &mut self,
        requests: Vec<damascene_core::viewport::ViewportRequest>,
    ) {
        self.core.push_viewport_requests(requests);
    }

    pub fn push_plot_requests(&mut self, requests: Vec<damascene_core::plot::PlotRequest>) {
        self.core.push_plot_requests(requests);
    }

    /// Switch animation pacing. Default is [`AnimationMode::Live`].
    /// Headless render binaries should call this with
    /// [`AnimationMode::Settled`] so a single-frame snapshot reflects
    /// the post-animation visual without depending on integrator timing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.core.set_animation_mode(mode);
    }

    /// Apply a wheel delta in **logical** pixels at `(x, y)`. Routes to
    /// the deepest scrollable container under the cursor in the last
    /// laid-out tree. Returns `true` if the event landed on a scrollable
    /// (host should `request_redraw` so the next frame applies the new
    /// offset).
    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.core.pointer_wheel(x, y, dy)
    }

    /// Build a routed wheel event for the keyed target under `(x, y)`.
    ///
    /// Dispatch this before [`Self::pointer_wheel`]; if the app
    /// consumes the event, skip the fallback scroll call.
    pub fn pointer_wheel_event(
        &mut self,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Option<damascene_core::UiEvent> {
        self.core.pointer_wheel_event(x, y, dx, dy)
    }

    /// Drain time-driven input events whose deadline has passed (touch
    /// long-press today; later: hold-to-repeat, etc.). Hosts call this
    /// once per frame before dispatching pointer events. `now` is
    /// `web_time::Instant` rather than `std::time::Instant` so the
    /// signature compiles on wasm32 — `web_time` aliases to std on
    /// native, so existing native callers passing `Instant::now()`
    /// from std still work. See [`damascene_core::RunnerCore::poll_input`].
    pub fn poll_input(&mut self, now: web_time::Instant) -> Vec<damascene_core::UiEvent> {
        self.core.poll_input(now)
    }

    /// Record draws into the host-managed render pass. Call after
    /// [`Self::prepare`]. Paint order follows the draw-op stream.
    ///
    /// **No backdrop sampling.** This entry point cannot honor pass
    /// boundaries (the host owns the pass lifetime), so any
    /// `BackdropSnapshot` items in the paint stream are no-ops and any
    /// shader bound with `samples_backdrop=true` reads an undefined
    /// backdrop binding. Use [`Self::render`] for backdrop-aware
    /// rendering.
    ///
    /// **3D scenes need the pre-pass.** `Scene3D` paint items
    /// composite from offscreen targets that must be rendered before
    /// the host's pass begins — call [`Self::encode_scene_prepass`] on
    /// the encoder first, or every scene in the frame samples a
    /// never-rendered target and composites blank.
    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.draw_items(pass, &self.core.paint_items);
    }

    /// Encode the offscreen pre-pass for any 3D scenes in this frame's
    /// paint stream: each `Scene3D` renders into its own offscreen
    /// target, and label-bearing scenes capture depth for next frame's
    /// label occlusion. No-op when the frame has no scenes.
    ///
    /// [`Self::render`] calls this automatically. Hosts using
    /// [`Self::draw`] must call it on their encoder after
    /// [`Self::prepare`] and *before* beginning the render pass that
    /// `draw` records into.
    pub fn encode_scene_prepass(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if self.scene_paint.has_runs() {
            self.scene_paint.encode_offscreen(encoder);
            // Capture each label-bearing scene's depth into its read-back
            // buffer (the depth is still alive from the pass above). The
            // map + CPU read happens next frame in `prepare`.
            self.scene_paint.encode_depth_capture(device, encoder);
        }
    }

    /// Record draws into a host-supplied encoder, owning pass
    /// lifetimes ourselves so backdrop-sampling shaders can sample a
    /// snapshot of Pass A's content.
    ///
    /// The host hands us:
    /// - the encoder (we record into it),
    /// - the color target's `wgpu::Texture` (used as `copy_src` when
    ///   we snapshot it; include `COPY_SRC` in its usage flags for
    ///   backdrop sampling to work — without it the snapshot copy is
    ///   skipped and backdrop shaders sample transparent black),
    /// - the corresponding `wgpu::TextureView` (we attach it to every
    ///   render pass we begin), and
    /// - the `LoadOp` to use on the *first* pass — `Clear(color)` to
    ///   clear behind us, `Load` to composite onto whatever was
    ///   already in the target.
    ///
    /// Multi-pass schedule when the paint stream contains a
    /// `BackdropSnapshot`:
    ///
    /// 1. Pass A — every paint item before the snapshot, with the
    ///    caller-supplied `LoadOp`.
    /// 2. `copy_texture_to_texture` — target → snapshot.
    /// 3. Pass B — paint items from the snapshot onward, with
    ///    `LoadOp::Load` so Pass A's pixels remain underneath.
    ///
    /// Without a snapshot, this collapses to a single pass and is
    /// equivalent to [`Self::draw`] called inside a host-managed
    /// pass with the same `LoadOp`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_tex: &wgpu::Texture,
        target_view: &wgpu::TextureView,
        msaa_view: Option<&wgpu::TextureView>,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        // When MSAA is in use, the actual color attachment is the
        // multisampled view and `target_view` becomes its resolve
        // target. `target_tex` is always the resolved (single-sample)
        // texture, so the snapshot copy below works whether MSAA is on
        // or not — the resolve happens at end-of-Pass-A.
        let attachment_view = msaa_view.unwrap_or(target_view);
        let resolve_target = msaa_view.map(|_| target_view);

        // Phase 1: render every recorded 3D scene into its own offscreen
        // target. Passes can't nest, so this is encoded on `encoder` ahead
        // of the main composite pass (same discipline as BackdropSnapshot).
        // The `PaintItem::Scene3D` arm below then composites the resolved
        // textures into the main pass.
        self.encode_scene_prepass(device, encoder);

        // Locate the (at most one) snapshot boundary.
        let mut split_at = self
            .core
            .paint_items
            .iter()
            .position(|p| matches!(p, PaintItem::BackdropSnapshot));

        // The snapshot copy needs the target to be a copy source.
        // Hosts normally prevent this pairing by not registering
        // backdrop shaders on COPY_SRC-less surfaces (issue #143), but
        // a host that registers one anyway must degrade, not hit a
        // validation panic at the copy: render single-pass, keeping
        // the (zero-initialized) snapshot texture alive so backdrop
        // shaders bind group 1 and sample transparent black.
        if split_at.is_some() && !target_tex.usage().contains(wgpu::TextureUsages::COPY_SRC) {
            if !self.backdrop_copy_unsupported_warned {
                self.backdrop_copy_unsupported_warned = true;
                log::warn!(
                    "damascene-wgpu: render target lacks COPY_SRC; backdrop-sampling shaders \
                     sample transparent black instead of the frame beneath them"
                );
            }
            self.ensure_snapshot(device, target_tex);
            split_at = None;
        }

        if let Some(idx) = split_at {
            self.ensure_snapshot(device, target_tex);
            // Pass A
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("damascene_wgpu::pass_a"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: attachment_view,
                        resolve_target,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                self.draw_items(&mut pass, &self.core.paint_items[..idx]);
            }
            // Snapshot copy. Target must support COPY_SRC; snapshot
            // texture (created in `ensure_snapshot`) supports COPY_DST
            // + TEXTURE_BINDING.
            let snapshot = self.snapshot.as_ref().expect("snapshot ensured");
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: target_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &snapshot.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: snapshot.extent.0,
                    height: snapshot.extent.1,
                    depth_or_array_layers: 1,
                },
            );
            // Pass B
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("damascene_wgpu::pass_b"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: attachment_view,
                        resolve_target,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // Skip the snapshot item itself; it's a marker, not a draw.
                self.draw_items(&mut pass, &self.core.paint_items[idx + 1..]);
            }
        } else {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("damascene_wgpu::pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: attachment_view,
                    resolve_target,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.draw_items(&mut pass, &self.core.paint_items);
        }
    }

    /// (Re)allocate the snapshot texture to match `target_tex`'s
    /// extent + format. Idempotent when the size matches; rebuilds the
    /// `backdrop_bind_group` whenever the snapshot is recreated.
    fn ensure_snapshot(&mut self, device: &wgpu::Device, target_tex: &wgpu::Texture) {
        let extent = target_tex.size();
        let want = (extent.width, extent.height);
        if let Some(s) = &self.snapshot
            && s.extent == want
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("damascene_wgpu::backdrop_snapshot"),
            size: wgpu::Extent3d {
                width: want.0,
                height: want.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.target_format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("damascene_wgpu::backdrop_bind_group"),
            layout: &self.backdrop_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.backdrop_sampler),
                },
            ],
        });
        self.snapshot = Some(SnapshotTexture {
            texture,
            extent: want,
        });
        self.backdrop_bind_group = Some(bind_group);
    }

    /// Walk a slice of `PaintItem`s into the given pass. Helper shared
    /// by [`Self::draw`] and [`Self::render`]. `BackdropSnapshot`
    /// items are no-ops here; `render()` handles them by splitting
    /// the slice before passing to this helper.
    fn draw_items<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        items: &'pass [PaintItem],
    ) {
        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: self.core.viewport_px.0,
            h: self.core.viewport_px.1,
        };
        // Redundant-state elision. Paint items arrive in z-order, so
        // consecutive items very often share scissor / pipeline / bind
        // groups / vertex buffers; re-setting them per item made wgpu's
        // per-call validation the dominant submit cost at high op
        // counts. Bind-group and buffer identity is by pointer: every
        // arm below binds long-lived objects owned by `self`, so equal
        // pointers mean the identical binding. WebGPU binding state
        // persists across pipeline switches, so skipping an equal
        // rebinding is behavior-identical.
        let mut state = DrawItemState::default();
        for item in items {
            match *item {
                PaintItem::QuadRun(index) => {
                    let run = &self.core.runs[index];
                    state.scissor(pass, run.scissor, full);
                    state.bind(pass, 0, &self.quad_bind_group);
                    let is_backdrop_shader = matches!(
                        run.handle,
                        ShaderHandle::Custom(name) if self.backdrop_shaders.contains(name)
                    );
                    if is_backdrop_shader && let Some(bg) = &self.backdrop_bind_group {
                        state.bind(pass, 1, bg);
                    }
                    state.vbuf(pass, 0, &self.quad_vbo);
                    state.vbuf(pass, 1, &self.instance_buf);
                    let pipeline = self
                        .pipelines
                        .get(&run.handle)
                        .expect("run handle has no pipeline (bug in prepare)");
                    state.pipeline(pass, pipeline);
                    pass.draw(0..4, run.first..run.first + run.count);
                }
                PaintItem::Text(index) => {
                    let run = self.text_paint.run(index);
                    state.scissor(pass, run.scissor, full);
                    state.pipeline(pass, self.text_paint.pipeline_for(run.kind));
                    state.bind(pass, 0, &self.quad_bind_group);
                    // Highlight runs use a frame-uniform-only pipeline.
                    // Glyph kinds bind the active atlas page at group 1.
                    if !matches!(run.kind, crate::text::TextRunKind::Highlight) {
                        state.bind(pass, 1, self.text_paint.page_bind_group(run.kind, run.page));
                    }
                    state.vbuf(pass, 0, &self.quad_vbo);
                    state.vbuf(pass, 1, self.text_paint.instance_buf_for(run.kind));
                    pass.draw(0..4, run.first..run.first + run.count);
                }
                PaintItem::IconRun(index) | PaintItem::Vector(index) => {
                    // `PaintItem::Vector` is structurally identical to
                    // `PaintItem::IconRun` — both index into the same
                    // `IconPaint::runs` Vec since `record_vector`
                    // appends there too. The variant is kept distinct
                    // for paint-stream provenance (icon vs app vector)
                    // but the dispatch is the same.
                    let run = self.icon_paint.run(index);
                    state.scissor(pass, run.scissor, full);
                    match run.kind {
                        IconRunKind::Tess => {
                            state.pipeline(pass, self.icon_paint.tess_pipeline(run.material));
                            state.bind(pass, 0, &self.quad_bind_group);
                            state.bind(pass, 1, self.icon_paint.gradient_bind_group());
                            state.vbuf(pass, 0, self.icon_paint.tess_vertex_buf());
                            pass.draw(run.first..run.first + run.count, 0..1);
                        }
                        IconRunKind::Msdf => {
                            state.pipeline(pass, self.icon_paint.msdf_pipeline());
                            state.bind(pass, 0, &self.quad_bind_group);
                            state.bind(pass, 1, self.icon_paint.msdf_page_bind_group(run.page));
                            state.vbuf(pass, 0, &self.quad_vbo);
                            state.vbuf(pass, 1, self.icon_paint.msdf_instance_buf());
                            pass.draw(0..4, run.first..run.first + run.count);
                        }
                    }
                }
                PaintItem::Image(index) => {
                    let run = self.image_paint.run(index);
                    state.scissor(pass, run.scissor, full);
                    state.pipeline(pass, self.image_paint.pipeline());
                    state.bind(pass, 0, &self.quad_bind_group);
                    state.bind(pass, 1, self.image_paint.bind_group_for_run(run));
                    state.vbuf(pass, 0, &self.quad_vbo);
                    state.vbuf(pass, 1, self.image_paint.instance_buf());
                    pass.draw(0..4, run.first..run.first + run.count);
                }
                PaintItem::AppTexture(index) => {
                    let run = self.surface_paint.run(index);
                    state.scissor(pass, run.scissor, full);
                    state.pipeline(pass, self.surface_paint.pipeline_for(run.alpha));
                    state.bind(pass, 0, &self.quad_bind_group);
                    state.bind(pass, 1, self.surface_paint.bind_group_for_run(run));
                    state.vbuf(pass, 0, &self.quad_vbo);
                    state.vbuf(pass, 1, self.surface_paint.instance_buf());
                    pass.draw(0..4, run.first..run.first + run.count);
                }
                PaintItem::Scene3D(index) => {
                    // The scene already rendered + resolved offscreen in
                    // phase 1; composite that texture over the rect via the
                    // stock surface pipeline (premultiplied).
                    let run = self.scene_paint.run(index);
                    state.scissor(pass, run.scissor, full);
                    state.pipeline(pass, self.scene_paint.composite_pipeline());
                    state.bind(pass, 0, &self.quad_bind_group);
                    state.bind(pass, 1, self.scene_paint.composite_bind_group(run));
                    state.vbuf(pass, 0, &self.quad_vbo);
                    state.vbuf(pass, 1, self.scene_paint.composite_instance_buf());
                    pass.draw(0..4, run.composite_instance..run.composite_instance + 1);
                }
                PaintItem::BackdropSnapshot => {
                    // Marker only — `render()` splits the slice on
                    // these and never includes one in a draw range.
                }
            }
        }
    }
}

/// Last-set render-pass state for [`Renderer::draw_items`]'s
/// redundant-call elision. Identity is by pointer for GPU objects
/// (they're all long-lived fields of the renderer) and by value for
/// the scissor rect.
#[derive(Default)]
struct DrawItemState<'pass> {
    scissor: Option<Option<PhysicalScissor>>,
    pipeline: Option<&'pass wgpu::RenderPipeline>,
    bind_groups: [Option<&'pass wgpu::BindGroup>; 2],
    vertex_bufs: [Option<&'pass wgpu::Buffer>; 2],
}

impl<'pass> DrawItemState<'pass> {
    fn scissor(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        scissor: Option<PhysicalScissor>,
        full: PhysicalScissor,
    ) {
        if self.scissor != Some(scissor) {
            set_scissor(pass, scissor, full);
            self.scissor = Some(scissor);
        }
    }

    fn pipeline(&mut self, pass: &mut wgpu::RenderPass<'_>, pipeline: &'pass wgpu::RenderPipeline) {
        if !self.pipeline.is_some_and(|cur| std::ptr::eq(cur, pipeline)) {
            pass.set_pipeline(pipeline);
            self.pipeline = Some(pipeline);
        }
    }

    fn bind(&mut self, pass: &mut wgpu::RenderPass<'_>, slot: u32, group: &'pass wgpu::BindGroup) {
        let cur = &mut self.bind_groups[slot as usize];
        if !cur.is_some_and(|cur| std::ptr::eq(cur, group)) {
            pass.set_bind_group(slot, group, &[]);
            *cur = Some(group);
        }
    }

    fn vbuf(&mut self, pass: &mut wgpu::RenderPass<'_>, slot: u32, buf: &'pass wgpu::Buffer) {
        let cur = &mut self.vertex_bufs[slot as usize];
        if !cur.is_some_and(|cur| std::ptr::eq(cur, buf)) {
            pass.set_vertex_buffer(slot, buf.slice(..));
            *cur = Some(buf);
        }
    }
}
