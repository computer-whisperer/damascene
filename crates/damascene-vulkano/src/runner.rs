//! `damascene-vulkano::Runner` — peer to `damascene_wgpu::Runner`.
//!
//! The Runner owns:
//!
//! - a clear-on-begin render pass and a load-on-begin render pass for
//!   Pass B after a `BackdropSnapshot` boundary; both are
//!   attachment-compatible so the same pipelines bind into either;
//! - one `GraphicsPipeline` per registered shader (stock `rounded_rect`
//!   up-front, custom shaders added via `register_shader`); focus
//!   indicators ride on each focusable node's own quad via uniforms on
//!   `stock::rounded_rect`, no separate ring pipeline;
//! - a `TextPaint` that mirrors `damascene-core`'s glyph atlas to a
//!   per-page sampled image, packs glyph instances into its own buffer,
//!   and exposes a text pipeline to the draw loop;
//! - an `IconPaint` that tessellates built-in SVG assets through the
//!   shared vector mesh and exposes flat/relief/glass icon pipelines;
//! - a persistent quad VBO (the unit-quad strip), a persistent frame
//!   uniform buffer (viewport extent + time), a single set-0 descriptor
//!   set bound to it, and a host-visible instance buffer that grows on
//!   demand;
//! - a snapshot color image + set-1 descriptor set for backdrop-sampling
//!   shaders, lazily sized to the current target;
//! - a shared `RunnerCore` (from `damascene-core::runtime`) carrying the
//!   interaction half — paint-stream scratch, hit-test/focus/hotkey
//!   state, the input plumbing methods — so behaviour matches
//!   `damascene-wgpu` by construction rather than by convention.
//!
//! `prepare()` walks the `DrawOp` stream produced by `damascene-core`,
//! packs `QuadInstance`s into the instance buffer and folds text/icon
//! draws through backend painters, then groups consecutive items
//! sharing a pipeline + scissor into runs. `draw()` walks the resulting
//! paint stream and records vulkano commands into the host's primary
//! command-buffer builder.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use damascene_core::{
    AnimationMode, El, KeyChord, KeyModifiers, Pointer, Rect, Theme, UiEvent, UiKey, UiState,
    shader::{ShaderHandle, StockShader, stock_wgsl},
    vector::IconMaterial,
};
use smallvec::smallvec;
use vulkano::{
    buffer::{
        Buffer, BufferCreateInfo, BufferUsage, Subbuffer,
        allocator::{SubbufferAllocator, SubbufferAllocatorCreateInfo},
    },
    command_buffer::{
        AutoCommandBufferBuilder, CopyImageInfo, PrimaryAutoCommandBuffer, RenderPassBeginInfo,
        SubpassBeginInfo, SubpassContents, SubpassEndInfo,
        allocator::StandardCommandBufferAllocator,
    },
    descriptor_set::{
        DescriptorSet, DescriptorSetWithOffsets, WriteDescriptorSet,
        allocator::StandardDescriptorSetAllocator, layout::DescriptorSetLayout,
    },
    device::{Device, Queue},
    format::Format,
    image::{
        Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::ImageView,
    },
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint, graphics::viewport::Viewport},
    render_pass::{Framebuffer, RenderPass, Subpass},
};

use damascene_core::ir::TextAnchor;
use damascene_core::paint::{IconRunKind, PaintItem, PhysicalScissor, QuadInstance};
use damascene_core::runtime::{RecordedPaint, RunnerCore, TextRecorder};
use damascene_core::text::atlas::RunStyle;
use damascene_core::tree::{Color, TextWrap};

pub use damascene_core::runtime::{LayoutPrepared, PointerMove, PrepareResult, PrepareTimings};

use crate::icon::IconPaint;
use crate::image::ImagePaint;
use crate::instance::set_scissor;
use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::{FrameUniforms, build_quad_pipeline};
use crate::scene::Scene3DPaint;
use crate::surface::SurfacePaint;
use crate::text::TextPaint;

/// Initial arena size for the per-frame `SubbufferAllocator`s. Sized so
/// a typical UI frame's instance + uniform uploads fit in a single arena
/// without having to grow; the allocator falls back to a larger arena
/// automatically when a frame asks for more.
const SUBALLOC_ARENA_SIZE: u64 = 1 << 20; // 1 MiB

pub struct Runner {
    device: Arc<Device>,
    _queue: Arc<Queue>,

    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,

    /// Render pass with `load_op: Clear` — used for the first (or only)
    /// pass of a frame. Pipelines are built against subpass 0 of this
    /// pass.
    render_pass: Arc<RenderPass>,
    /// Render pass with `load_op: Load` — used for Pass B when the
    /// frame has a `BackdropSnapshot` boundary, so Pass A's pixels
    /// remain underneath. Attachment-compatible with `render_pass` so
    /// the same pipelines work with both.
    load_render_pass: Arc<RenderPass>,

    pipelines: HashMap<ShaderHandle, Arc<GraphicsPipeline>>,

    text_paint: TextPaint,
    icon_paint: IconPaint,
    image_paint: ImagePaint,
    surface_paint: SurfacePaint,
    scene_paint: Scene3DPaint,

    quad_vbo: Subbuffer<[f32]>,

    /// Layout for set 0 = `FrameUniforms`. Cached so `prepare()` can
    /// rebuild the descriptor set against a fresh per-frame uniform
    /// suballocation without re-querying the pipeline.
    frame_set_layout: Arc<DescriptorSetLayout>,

    /// Per-frame `SubbufferAllocator` for transient host-visible
    /// uploads — quad-instance data and the frame-uniform block.
    /// Allocations from these are valid for the lifetime of the
    /// `Subbuffer` they yield, which the descriptor set / vertex
    /// binding keeps alive across submission. Older arenas are reclaimed
    /// once the frame's GpuFuture is cleaned up by the host loop.
    instance_alloc: SubbufferAllocator,
    uniform_alloc: SubbufferAllocator,

    /// Per-frame quad-instance suballocation written in `prepare()` and
    /// bound in `draw_items`. `None` between construction and the first
    /// non-empty frame.
    instance_buf: Option<Subbuffer<[QuadInstance]>>,
    /// Per-frame descriptor set holding the freshly-allocated uniform
    /// suballocation at binding 0.
    frame_descriptor_set: Option<Arc<DescriptorSet>>,

    /// MSAA samples per pixel. `1` means non-multisampled; `2/4/8/...`
    /// require a multisampled color attachment + resolve target on the
    /// host's framebuffer (see [`Runner::sample_count`]).
    sample_count: u32,

    /// SPIR-V words cached per registered custom shader name. The
    /// pipeline itself is built lazily on first use.
    registered_shaders: HashMap<&'static str, Vec<u32>>,

    /// Custom shader names registered with `samples_backdrop=true`.
    /// `prepare_paint` queries this to insert a `BackdropSnapshot`
    /// marker before the first backdrop-sampling draw, and the inner
    /// draw loop binds the snapshot descriptor set at set=1 when the
    /// run's pipeline expects it.
    backdrop_shaders: HashSet<&'static str>,

    /// Custom shader names registered with `samples_time=true`. Mirrors
    /// `backdrop_shaders`: feeds `prepare_layout`'s continuous-redraw
    /// scan instead of the paint scheduler so any node bound to such a
    /// shader keeps the host loop ticking.
    time_shaders: HashSet<&'static str>,

    /// Linear-filtering sampler bound at `@group(1) @binding(1)` for
    /// every backdrop-sampling pipeline.
    backdrop_sampler: Arc<Sampler>,

    /// Set 1's descriptor-set layout for backdrop-sampling pipelines —
    /// captured from the first backdrop pipeline registered. The same
    /// layout shape (texture_2d + sampler) is shared across all
    /// backdrop shaders, so a single descriptor set built against this
    /// layout binds correctly into any backdrop pipeline.
    backdrop_set_layout: Option<Arc<DescriptorSetLayout>>,

    /// Lazily-allocated snapshot of the color target. Sized to match
    /// the current target on each `render()` call; rebuilt when the
    /// target's extent changes.
    snapshot: Option<SnapshotImage>,
    /// Descriptor set binding the snapshot view + `backdrop_sampler` at
    /// set 1. Rebuilt whenever `snapshot` is recreated.
    backdrop_descriptor_set: Option<Arc<DescriptorSet>>,

    /// Wall-clock origin for the `time` field in `FrameUniforms`.
    start_time: Instant,

    // Backend-agnostic state shared with damascene-wgpu: interaction state,
    // paint-stream scratch (quad_scratch / runs / paint_items),
    // viewport_px, last_tree, the 13 input plumbing methods.
    core: RunnerCore,
}

struct SnapshotImage {
    image: Arc<Image>,
    extent: [u32; 3],
}

struct PaintRecorder<'a> {
    text: &'a mut TextPaint,
    icons: &'a mut IconPaint,
    images: &'a mut ImagePaint,
    surfaces: &'a mut SurfacePaint,
    scenes: &'a mut Scene3DPaint,
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
        _scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.images.record(rect, scissor, image, tint, radius)
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
            .record(rect, scissor, texture, alpha, transform)
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
        self.scenes.record(rect, scissor, id, scene, scale_factor)
    }
}

impl Runner {
    /// Create a runner with MSAA off (`sample_count = 1`). The host's
    /// swapchain must use `target_format`; stock pipelines are built
    /// against a single-pass render pass with a color attachment in that
    /// format. Call [`Self::render_pass`] to get the pass back so you can
    /// build framebuffers against it.
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, target_format: Format) -> Self {
        Self::with_sample_count(device, queue, target_format, 1)
    }

    /// Like [`Self::new`], but builds all stock pipelines and the runner's
    /// render passes with `sample_count` MSAA samples per pixel. When
    /// `sample_count > 1`, the render pass has two attachments — a
    /// multisampled color attachment (load+store of the MSAA pixels) and
    /// a single-sample resolve attachment (the swapchain image) — and
    /// the host's framebuffer must bind both in that order. Use
    /// [`Self::sample_count`] to read the active count back when sizing
    /// the multisampled image.
    pub fn with_sample_count(
        device: Arc<Device>,
        queue: Arc<Queue>,
        target_format: Format,
        sample_count: u32,
    ) -> Self {
        assert!(
            SampleCount::try_from(sample_count).is_ok(),
            "damascene-vulkano: sample_count {sample_count} is not a valid Vulkan sample count \
             (expected 1, 2, 4, 8, 16, 32, or 64)"
        );
        let memory_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let descriptor_alloc = Arc::new(StandardDescriptorSetAllocator::new(
            device.clone(),
            Default::default(),
        ));
        // Used internally for text-atlas dirty-region uploads.
        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            Default::default(),
        ));

        // Clear at pass-begin so the host doesn't need a separate
        // `clear_color_image` step. The host passes its own clear color
        // via `begin_render_pass`'s `clear_values`.
        let render_pass = build_render_pass(
            device.clone(),
            target_format,
            sample_count,
            /* load_op_clear: */ true,
        );
        // Pass B (used when there's a BackdropSnapshot boundary)
        // preserves Pass A's contents — `load_op: Load`. Attachment
        // layout matches `render_pass`, so pipelines built against the
        // Clear pass are render-pass-compatible with this Load pass.
        let load_render_pass = build_render_pass(
            device.clone(),
            target_format,
            sample_count,
            /* load_op_clear: */ false,
        );
        let subpass = Subpass::from(render_pass.clone(), 0)
            .expect("damascene-vulkano: subpass 0 of single-pass render pass");

        let mut pipelines = HashMap::new();
        let rr = build_quad_pipeline(
            device.clone(),
            subpass.clone(),
            sample_count,
            "stock::rounded_rect",
            stock_wgsl::ROUNDED_RECT,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::RoundedRect), rr.clone());

        let spinner = build_quad_pipeline(
            device.clone(),
            subpass.clone(),
            sample_count,
            "stock::spinner",
            stock_wgsl::SPINNER,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::Spinner), spinner);

        let skeleton = build_quad_pipeline(
            device.clone(),
            subpass.clone(),
            sample_count,
            "stock::skeleton",
            stock_wgsl::SKELETON,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::Skeleton), skeleton);

        let progress_indeterminate = build_quad_pipeline(
            device.clone(),
            subpass,
            sample_count,
            "stock::progress_indeterminate",
            stock_wgsl::PROGRESS_INDETERMINATE,
        );
        pipelines.insert(
            ShaderHandle::Stock(StockShader::ProgressIndeterminate),
            progress_indeterminate,
        );

        // Persistent quad VBO — 4 corners of the unit quad as a triangle
        // strip, written once.
        let quad_vbo = Buffer::from_iter(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            [0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .expect("damascene-vulkano: quad VBO");

        // Cached set-0 layout — every rect-shaped pipeline shares it,
        // so one layout drives the per-frame descriptor set rebuild in
        // `prepare()`.
        let frame_set_layout = rr.layout().set_layouts()[0].clone();

        // Per-frame host-visible suballocators. The instance allocator
        // backs `bind_vertex_buffers` slots; the uniform allocator backs
        // `WriteDescriptorSet::buffer` for FrameUniforms. Both yield
        // fresh suballocations every frame so a write in the next
        // `prepare()` never contends with the GPU still reading the
        // previous frame's bytes.
        let instance_alloc = SubbufferAllocator::new(
            memory_alloc.clone(),
            SubbufferAllocatorCreateInfo {
                arena_size: SUBALLOC_ARENA_SIZE,
                buffer_usage: BufferUsage::VERTEX_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );
        let uniform_alloc = SubbufferAllocator::new(
            memory_alloc.clone(),
            SubbufferAllocatorCreateInfo {
                arena_size: SUBALLOC_ARENA_SIZE,
                buffer_usage: BufferUsage::UNIFORM_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );

        let text_subpass =
            Subpass::from(render_pass.clone(), 0).expect("damascene-vulkano: text subpass 0");
        let text_paint = TextPaint::new(
            device.clone(),
            queue.clone(),
            memory_alloc.clone(),
            descriptor_alloc.clone(),
            cmd_alloc.clone(),
            text_subpass,
            sample_count,
        );
        let icon_subpass =
            Subpass::from(render_pass.clone(), 0).expect("damascene-vulkano: icon subpass 0");
        let icon_paint = IconPaint::new(
            device.clone(),
            queue.clone(),
            memory_alloc.clone(),
            descriptor_alloc.clone(),
            cmd_alloc.clone(),
            icon_subpass,
            sample_count,
        );
        let image_subpass =
            Subpass::from(render_pass.clone(), 0).expect("damascene-vulkano: image subpass 0");
        let image_paint = ImagePaint::new(
            device.clone(),
            queue.clone(),
            memory_alloc.clone(),
            descriptor_alloc.clone(),
            cmd_alloc,
            image_subpass,
            sample_count,
        );
        let surface_subpass =
            Subpass::from(render_pass.clone(), 0).expect("damascene-vulkano: surface subpass 0");
        let surface_paint = SurfacePaint::new(
            device.clone(),
            memory_alloc.clone(),
            descriptor_alloc.clone(),
            surface_subpass,
            sample_count,
        );
        let scene_subpass =
            Subpass::from(render_pass.clone(), 0).expect("damascene-vulkano: scene subpass 0");
        let scene_paint = Scene3DPaint::new(
            device.clone(),
            memory_alloc.clone(),
            descriptor_alloc.clone(),
            scene_subpass,
            sample_count,
            damascene_core::paint::DEFAULT_WORKING_COLOR_SPACE,
        );

        // Filtering sampler bound at @group(1) @binding(1) for every
        // backdrop-sampling pipeline. Mirrors the wgpu side: linear
        // mag/min, nearest mipmap (we don't generate mips on the
        // snapshot), clamp-to-edge so blur kernels at the rim don't
        // wrap.
        let backdrop_sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: backdrop sampler");

        Self {
            device,
            _queue: queue,
            memory_alloc,
            descriptor_alloc,
            render_pass,
            load_render_pass,
            pipelines,
            text_paint,
            icon_paint,
            image_paint,
            surface_paint,
            scene_paint,
            quad_vbo,
            frame_set_layout,
            instance_alloc,
            uniform_alloc,
            instance_buf: None,
            frame_descriptor_set: None,
            sample_count,
            registered_shaders: HashMap::new(),
            backdrop_shaders: HashSet::new(),
            time_shaders: HashSet::new(),
            backdrop_sampler,
            backdrop_set_layout: None,
            snapshot: None,
            backdrop_descriptor_set: None,
            start_time: Instant::now(),
            core: {
                let mut c = RunnerCore::new();
                c.quad_scratch = Vec::with_capacity(1024);
                c
            },
        }
    }

    /// The render pass pipelines are built against. The host must
    /// construct framebuffers against this pass and begin/end it around
    /// each call to [`Self::draw`].
    ///
    /// When [`Self::sample_count`] is greater than 1 the pass has two
    /// attachments — a multisampled color attachment and a single-sample
    /// resolve attachment — and the host's framebuffer must bind both
    /// in that order.
    pub fn render_pass(&self) -> &Arc<RenderPass> {
        &self.render_pass
    }

    /// MSAA samples per pixel the runner's pipelines and render passes
    /// were built with. `1` means non-multisampled; values above 1
    /// indicate the host must allocate a multisampled color image and
    /// bind it as the first framebuffer attachment (with the swapchain
    /// image as the second, resolve attachment).
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.core.set_surface_size(width, height);
    }

    /// Set the theme used to resolve implicit widget surfaces to shaders.
    pub fn set_theme(&mut self, theme: Theme) {
        self.icon_paint.set_material(theme.icon_material());
        self.core.set_theme(theme);
    }

    pub fn theme(&self) -> &Theme {
        self.core.theme()
    }

    /// Select the stock material used by the vector-icon painter.
    /// Prefer [`Theme::with_icon_material`] for app-level routing; this
    /// is a low-level fixture/testing override.
    pub fn set_icon_material(&mut self, material: IconMaterial) {
        self.icon_paint.set_material(material);
    }

    pub fn icon_material(&self) -> IconMaterial {
        self.icon_paint.material()
    }

    /// Register a custom shader. WGSL → SPIR-V at register time; bad
    /// WGSL panics here, not mid-frame. The graphics pipeline is built
    /// eagerly so a shader registered for a `key` is ready to draw
    /// immediately.
    pub fn register_shader(&mut self, name: &'static str, wgsl: &str) {
        self.register_shader_with(name, wgsl, false, false);
    }

    /// Register a custom shader with opt-in flags for backdrop
    /// sampling and time-driven motion.
    ///
    /// `samples_backdrop=true` inserts a pass boundary before the
    /// first draw bound to this shader, so `Runner::render` arranges
    /// Pass A → snapshot copy → Pass B and the shader can sample the
    /// post-Pass-A target through `@group(1)`.
    ///
    /// `samples_time=true` declares the shader's output depends on
    /// `frame.time`; the runtime ORs this into
    /// [`PrepareResult::needs_redraw`] so the host loop keeps ticking
    /// while any node is bound to the shader.
    pub fn register_shader_with(
        &mut self,
        name: &'static str,
        wgsl: &str,
        samples_backdrop: bool,
        samples_time: bool,
    ) {
        // Cache the SPIR-V words too — useful for diagnostics + future
        // re-registration without re-running naga.
        let spirv = wgsl_to_spirv(name, wgsl)
            .unwrap_or_else(|e| panic!("damascene-vulkano: WGSL compile failed for `{name}`: {e}"));
        self.registered_shaders.insert(name, spirv);

        let subpass =
            Subpass::from(self.render_pass.clone(), 0).expect("damascene-vulkano: subpass 0");
        let pipeline =
            build_quad_pipeline(self.device.clone(), subpass, self.sample_count, name, wgsl);
        if samples_backdrop {
            // Capture set 1's layout from the first backdrop pipeline
            // we see. Vulkano builds pipeline layouts via reflection,
            // so any backdrop shader declaring `@group(1) binding(0)
            // texture_2d<f32> + @group(1) binding(1) sampler` produces
            // a structurally-identical layout — one descriptor set
            // built against this layout binds correctly into all
            // backdrop pipelines.
            if self.backdrop_set_layout.is_none() {
                let layouts = pipeline.layout().set_layouts();
                let set1 = layouts.get(1).unwrap_or_else(|| {
                    panic!(
                        "damascene-vulkano: backdrop shader `{name}` has no @group(1) — \
                         expected `backdrop_tex` (binding 0) and `backdrop_smp` (binding 1)"
                    )
                });
                self.backdrop_set_layout = Some(set1.clone());
            }
            self.backdrop_shaders.insert(name);
        } else {
            self.backdrop_shaders.remove(name);
        }
        if samples_time {
            self.time_shaders.insert(name);
        } else {
            self.time_shaders.remove(name);
        }
        self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
    }

    pub fn ui_state(&self) -> &UiState {
        self.core.ui_state()
    }

    pub fn debug_summary(&self) -> String {
        self.core.debug_summary()
    }

    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.core.rect_of_key(key)
    }

    /// Lay out the tree, run animation tick, walk the draw-op stream,
    /// and upload per-frame buffers (instance data + frame uniforms).
    /// Must be called before [`Self::draw`] and outside of any render
    /// pass.
    pub fn prepare(&mut self, root: &mut El, viewport: Rect, scale_factor: f32) -> PrepareResult {
        let mut timings = PrepareTimings::default();

        // Install any scene depth maps the previous frame's capture finished
        // reading back, so this frame's `draw_ops` can occlude scene-anchored
        // labels behind geometry. Done before `prepare_layout` runs the
        // draw-op pass; stale maps for scenes that left the tree are GC'd.
        let ready_depth = self.scene_paint.collect_depth_maps();
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

        // Closure feeds `prepare_layout`'s continuous-redraw scan.
        // Any node bound to a shader registered with
        // `samples_time=true` keeps the host loop ticking even when
        // no animation is settling.
        let time_shaders = &self.time_shaders;
        let LayoutPrepared {
            ops,
            mut needs_redraw,
            mut next_layout_redraw_in,
            next_paint_redraw_in,
        } = self
            .core
            .prepare_layout(
                root,
                viewport,
                scale_factor,
                &mut timings,
                |handle| match handle {
                    ShaderHandle::Custom(name) => time_shaders.contains(name),
                    ShaderHandle::Stock(_) => false,
                },
            );

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

        let t_paint_end = Instant::now();
        // Each frame we allocate a fresh instance suballocation (and,
        // below, a fresh uniform suballocation + descriptor set) so
        // a host write here cannot contend with the GPU still reading
        // last frame's bytes. The previous arena is reclaimed once its
        // GpuFuture is cleaned up by the host loop.
        if !self.core.quad_scratch.is_empty() {
            let buf = self
                .instance_alloc
                .allocate_slice::<QuadInstance>(self.core.quad_scratch.len() as u64)
                .expect("damascene-vulkano: instance suballocate");
            buf.write()
                .expect("damascene-vulkano: instance suballocation write")
                .copy_from_slice(&self.core.quad_scratch);
            self.instance_buf = Some(buf);
        } else {
            self.instance_buf = None;
        }
        // Sync atlas dirty regions to GPU images + upload glyph instances.
        // Text uploads run through their own one-shot command buffer
        // submitted+waited inside flush().
        self.text_paint.flush();
        self.icon_paint.flush();
        self.image_paint.flush();
        self.surface_paint.flush();
        self.scene_paint.flush();
        {
            // FrameUniforms.viewport is the **logical** viewport — the
            // vertex shader divides per-instance positions (which layout
            // produced in logical pixels) by it to get clip-space coords.
            // Using physical here would render every quad at scale_factor⁻¹
            // size in the top-left — and silently break hit-testing,
            // because layout's logical rects no longer match what the user
            // sees.
            let buf = self
                .uniform_alloc
                .allocate_sized::<FrameUniforms>()
                .expect("damascene-vulkano: frame uniform suballocate");
            // Pin time to 0 in Settled mode so headless fixtures
            // rendering a time-driven shader (e.g. stock::spinner)
            // stay byte-identical run-to-run.
            let time = match self.core.ui_state().animation_mode() {
                damascene_core::AnimationMode::Settled => 0.0,
                damascene_core::AnimationMode::Live => {
                    (Instant::now() - self.start_time).as_secs_f32()
                }
            };
            *buf.write()
                .expect("damascene-vulkano: frame uniform suballocation write") = FrameUniforms {
                viewport: [viewport.w, viewport.h],
                time,
                scale_factor,
            };
            // Rebuild set 0 against the new suballocation. The 16-byte
            // descriptor write is cheap and the old set keeps last
            // frame's submission alive until its fence completes.
            let set = DescriptorSet::new(
                self.descriptor_alloc.clone(),
                self.frame_set_layout.clone(),
                [WriteDescriptorSet::buffer(0, buf)],
                [],
            )
            .expect("damascene-vulkano: per-frame descriptor set");
            self.frame_descriptor_set = Some(set);
        }
        timings.gpu_upload = Instant::now() - t_paint_end;

        self.core.snapshot(root, &mut timings);

        // Move resolved ops into the core's cache so a subsequent
        // paint-only frame ([`Self::repaint`]) can reuse them.
        self.core.last_ops = ops;

        // Damascene renders lazily, but the label-occlusion depth read-back needs
        // a couple of frames to resolve. Keep frames coming until every
        // labelled scene has a depth map matching its current pose — otherwise
        // a capture started in `render` sits unread after the camera settles
        // and labels never appear. Must drive the *layout* lane, not just
        // `needs_redraw` (the winit host schedules off the deadline lanes), and
        // not the paint lane (the paint-only `repaint` path skips
        // `collect_depth_maps`). Settled + current scenes report `false`, so
        // lazy idle is preserved. Mirrors the wgpu runner.
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

    /// Paint-only frame against the cached ops from the most recent
    /// [`Self::prepare`] call. Skips rebuild + layout + draw_ops +
    /// snapshot — only `frame.time` advances. See the wgpu
    /// `Renderer::repaint` doc for the host-side invariants (no input
    /// since last full prepare; same viewport / scale).
    pub fn repaint(&mut self, viewport: Rect, scale_factor: f32) -> PrepareResult {
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

        let t_paint_end = Instant::now();
        if !self.core.quad_scratch.is_empty() {
            let buf = self
                .instance_alloc
                .allocate_slice::<QuadInstance>(self.core.quad_scratch.len() as u64)
                .expect("damascene-vulkano: instance suballocate");
            buf.write()
                .expect("damascene-vulkano: instance suballocation write")
                .copy_from_slice(&self.core.quad_scratch);
            self.instance_buf = Some(buf);
        } else {
            self.instance_buf = None;
        }
        self.text_paint.flush();
        self.icon_paint.flush();
        self.image_paint.flush();
        self.surface_paint.flush();
        self.scene_paint.flush();
        {
            let buf = self
                .uniform_alloc
                .allocate_sized::<FrameUniforms>()
                .expect("damascene-vulkano: frame uniform suballocate");
            let time = match self.core.ui_state().animation_mode() {
                damascene_core::AnimationMode::Settled => 0.0,
                damascene_core::AnimationMode::Live => {
                    (Instant::now() - self.start_time).as_secs_f32()
                }
            };
            *buf.write()
                .expect("damascene-vulkano: frame uniform suballocation write") = FrameUniforms {
                viewport: [viewport.w, viewport.h],
                time,
                scale_factor,
            };
            let set = DescriptorSet::new(
                self.descriptor_alloc.clone(),
                self.frame_set_layout.clone(),
                [WriteDescriptorSet::buffer(0, buf)],
                [],
            )
            .expect("damascene-vulkano: per-frame descriptor set");
            self.frame_descriptor_set = Some(set);
        }
        timings.gpu_upload = Instant::now() - t_paint_end;

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

    pub fn pointer_moved(&mut self, p: Pointer) -> PointerMove {
        self.core.pointer_moved(p)
    }

    pub fn pointer_left(&mut self) -> Vec<damascene_core::UiEvent> {
        self.core.pointer_left()
    }

    pub fn file_hovered(
        &mut self,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    ) -> Vec<damascene_core::UiEvent> {
        self.core.file_hovered(path, x, y)
    }

    pub fn file_hover_cancelled(&mut self) -> Vec<damascene_core::UiEvent> {
        self.core.file_hover_cancelled()
    }

    pub fn file_dropped(
        &mut self,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    ) -> Vec<damascene_core::UiEvent> {
        self.core.file_dropped(path, x, y)
    }

    pub fn pointer_down(&mut self, p: Pointer) -> Vec<UiEvent> {
        self.core.pointer_down(p)
    }

    pub fn pointer_up(&mut self, p: Pointer) -> Vec<UiEvent> {
        self.core.pointer_up(p)
    }

    pub fn set_modifiers(&mut self, modifiers: KeyModifiers) {
        self.core.ui_state.set_modifiers(modifiers);
    }

    pub fn key_down(&mut self, key: UiKey, modifiers: KeyModifiers, repeat: bool) -> Vec<UiEvent> {
        self.core.key_down(key, modifiers, repeat)
    }

    pub fn text_input(&mut self, text: String) -> Option<UiEvent> {
        self.core.text_input(text)
    }

    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.core.set_hotkeys(hotkeys);
    }

    pub fn set_selection(&mut self, selection: damascene_core::selection::Selection) {
        self.core.set_selection(selection);
    }

    pub fn push_toasts(&mut self, specs: Vec<damascene_core::toast::ToastSpec>) {
        self.core.push_toasts(specs);
    }

    pub fn dismiss_toast(&mut self, id: u64) {
        self.core.dismiss_toast(id);
    }

    pub fn push_focus_requests(&mut self, keys: Vec<String>) {
        self.core.push_focus_requests(keys);
    }

    pub fn push_scroll_requests(&mut self, requests: Vec<damascene_core::scroll::ScrollRequest>) {
        self.core.push_scroll_requests(requests);
    }

    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.core.set_animation_mode(mode);
    }

    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.core.pointer_wheel(x, y, dy)
    }

    pub fn pointer_wheel_event(
        &mut self,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    ) -> Option<damascene_core::UiEvent> {
        self.core.pointer_wheel_event(x, y, dx, dy)
    }

    /// Drain time-driven input events whose deadline has passed.
    /// Vulkano is a native-only backend, so `std::time::Instant` is
    /// fine here — `RunnerCore` takes `web_time::Instant` which is a
    /// transparent alias on native targets. See
    /// [`damascene_core::RunnerCore::poll_input`].
    pub fn poll_input(&mut self, now: std::time::Instant) -> Vec<damascene_core::UiEvent> {
        self.core.poll_input(now)
    }

    /// Record draws into the host-managed primary command-buffer
    /// builder. Call inside the host's `begin_render_pass` /
    /// `end_render_pass` scope, with the runner's `render_pass()`.
    ///
    /// `BackdropSnapshot` markers in the paint stream are no-ops in
    /// this entry point — backdrop-sampling shaders need the multi-pass
    /// scheduling provided by [`Self::render`]. Hosts that want to use
    /// `liquid_glass`-style shaders should call `render()` instead and
    /// let the runner own pass lifetimes.
    pub fn draw(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        if self.core.paint_items.is_empty() {
            return;
        }
        self.draw_items(builder, &self.core.paint_items);
    }

    /// Record draws into a host-supplied command buffer, owning pass
    /// lifetimes ourselves so backdrop-sampling shaders can sample a
    /// snapshot of Pass A's content. Mirrors `damascene_wgpu::Runner::render`.
    ///
    /// The host hands us:
    /// - the command-buffer builder (we record into it),
    /// - the `Framebuffer` matching the current swapchain image,
    /// - the underlying `Image` (used as `copy_src` when we snapshot
    ///   the post-Pass-A content; must be created with
    ///   `ImageUsage::TRANSFER_SRC`),
    /// - the clear color used on the *first* pass (linear sRGB).
    ///
    /// Multi-pass schedule when the paint stream contains a
    /// `BackdropSnapshot`:
    ///
    /// 1. Pass A — every paint item before the snapshot, using the
    ///    runner's Clear render pass with `clear_color`.
    /// 2. `copy_image` — target → snapshot.
    /// 3. Pass B — paint items from the snapshot onward, using the
    ///    runner's Load render pass so Pass A's pixels remain
    ///    underneath.
    ///
    /// Without a snapshot, this collapses to a single Clear pass and is
    /// equivalent to the host wrapping [`Self::draw`] in
    /// `begin_render_pass(Clear) … end_render_pass`.
    pub fn render(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        framebuffer: Arc<Framebuffer>,
        target_image: Arc<Image>,
        clear_color: [f32; 4],
    ) {
        // 3D scenes render into their own offscreen targets first, ahead of
        // any main-pass work — render passes can't nest, so the resolved
        // scene textures must exist before the composite pass samples them.
        if self.scene_paint.has_runs() {
            self.scene_paint.encode_offscreen(builder);
            // Capture each label-bearing scene's depth into its read-back
            // buffer (the depth is still alive from the pass above). The CPU
            // read happens next frame in `prepare`, after the host's fence.
            self.scene_paint.encode_depth_capture(builder);
        }

        if self.core.paint_items.is_empty() {
            // Even with no draws we begin/end the Clear pass so the
            // attachment is cleared — matches `draw()` semantics when
            // the host wraps it in begin_render_pass(Clear).
            self.begin_pass(builder, framebuffer.clone(), Some(clear_color));
            self.end_pass(builder);
            return;
        }

        let split_at = self
            .core
            .paint_items
            .iter()
            .position(|p| matches!(p, PaintItem::BackdropSnapshot));

        if let Some(idx) = split_at {
            self.ensure_snapshot(target_image.clone());
            // Pass A
            self.begin_pass(builder, framebuffer.clone(), Some(clear_color));
            self.draw_items(builder, &self.core.paint_items[..idx]);
            self.end_pass(builder);
            // Snapshot copy. vulkano's auto command buffer inserts the
            // layout transitions for us (ColorAttachmentOptimal →
            // TransferSrcOptimal on `target_image`, then back before
            // Pass B begins).
            let snapshot = self.snapshot.as_ref().expect("snapshot ensured");
            builder
                .copy_image(CopyImageInfo::images(target_image, snapshot.image.clone()))
                .expect("damascene-vulkano: copy target → snapshot");
            // Pass B
            self.begin_pass(builder, framebuffer, None);
            // Skip the BackdropSnapshot marker itself — it's a boundary
            // only, not a draw.
            self.draw_items(builder, &self.core.paint_items[idx + 1..]);
            self.end_pass(builder);
        } else {
            self.begin_pass(builder, framebuffer, Some(clear_color));
            self.draw_items(builder, &self.core.paint_items);
            self.end_pass(builder);
        }
    }

    /// Begin a render pass. `clear_color = Some(_)` uses the Clear
    /// render pass with that color; `None` uses the Load render pass
    /// to preserve previous contents. Both passes are render-pass
    /// compatible with the framebuffer (same attachment format), so
    /// the host's framebuffer (built against `render_pass()`) works
    /// for either by overriding `render_pass` in `RenderPassBeginInfo`.
    fn begin_pass(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        framebuffer: Arc<Framebuffer>,
        clear_color: Option<[f32; 4]>,
    ) {
        // With MSAA on, the pass has a resolve attachment after the
        // multisampled color attachment. Its `load_op` is `DontCare`
        // (Pass A) / not loaded (Pass B), so the matching `clear_values`
        // slot is `None` in both cases.
        let (render_pass, mut clear_values) = match clear_color {
            Some(c) => (self.render_pass.clone(), vec![Some(c.into())]),
            // Load render pass declares `load_op: Load` for its color
            // attachment, so the matching `clear_values` slot must be
            // `None`.
            None => (self.load_render_pass.clone(), vec![None]),
        };
        if self.sample_count > 1 {
            clear_values.push(None);
        }
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    render_pass,
                    clear_values,
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("damascene-vulkano: begin_render_pass");
    }

    fn end_pass(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        builder
            .end_render_pass(SubpassEndInfo::default())
            .expect("damascene-vulkano: end_render_pass");
    }

    fn set_viewport(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        let (px_w, px_h) = self.core.viewport_px;
        builder
            .set_viewport(
                0,
                smallvec![Viewport {
                    offset: [0.0, 0.0],
                    extent: [px_w as f32, px_h as f32],
                    depth_range: 0.0..=1.0,
                }],
            )
            .expect("set_viewport");
    }

    /// Walk a slice of `PaintItem`s and record per-run draw commands.
    /// `BackdropSnapshot` is a marker that `render()` splits on; if it
    /// appears here it's silently skipped (no-op).
    fn draw_items(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        items: &[PaintItem],
    ) {
        let (px_w, px_h) = self.core.viewport_px;
        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: px_w,
            h: px_h,
        };

        for item in items {
            match *item {
                PaintItem::QuadRun(idx) => {
                    let run = &self.core.runs[idx];
                    let pipeline = self
                        .pipelines
                        .get(&run.handle)
                        .expect("run handle has no pipeline (bug in prepare)");
                    let is_backdrop_shader = matches!(
                        run.handle,
                        ShaderHandle::Custom(name) if self.backdrop_shaders.contains(name)
                    );
                    builder
                        .bind_pipeline_graphics(pipeline.clone())
                        .expect("bind_pipeline_graphics");
                    self.set_viewport(builder);
                    set_scissor(builder, run.scissor, full);
                    // Backdrop pipelines expect set 0 = FrameUniforms
                    // and set 1 = (snapshot view + sampler). All
                    // backdrop shaders share a structurally-identical
                    // set 1 layout so one descriptor set serves them
                    // all. If `render()` wasn't used (no snapshot was
                    // built), the bind is skipped — the pipeline will
                    // sample undefined memory, which is a no-op visual
                    // bug rather than a validation error since every
                    // backdrop shader still has the binding declared.
                    let frame_set = self.frame_descriptor_set();
                    let sets: Vec<DescriptorSetWithOffsets> =
                        if is_backdrop_shader && let Some(bg) = &self.backdrop_descriptor_set {
                            vec![frame_set.clone().into(), bg.clone().into()]
                        } else {
                            vec![frame_set.clone().into()]
                        };
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            pipeline.layout().clone(),
                            0,
                            sets,
                        )
                        .expect("bind_descriptor_sets");
                    builder
                        .bind_vertex_buffers(
                            0,
                            (self.quad_vbo.clone(), self.instance_buf().clone()),
                        )
                        .expect("bind_vertex_buffers");
                    // SAFETY: the pipeline expects 4 vertices (the unit
                    // quad strip) and `run.count` instances starting at
                    // `run.first`; the instance buffer was sized to fit
                    // every packed instance in `prepare()`.
                    unsafe {
                        builder.draw(4, run.count, 0, run.first).expect("draw");
                    }
                }
                PaintItem::Scene3D(idx) => {
                    // The scene already rendered + resolved offscreen in
                    // `render()`'s pre-pass; here we composite the resolved
                    // texture into the main pass via the stock surface
                    // pipeline, exactly like an AppTexture.
                    let run = self.scene_paint.run(idx);
                    let pipeline = self.scene_paint.composite_pipeline();
                    builder
                        .bind_pipeline_graphics(pipeline.clone())
                        .expect("bind_pipeline_graphics scene");
                    self.set_viewport(builder);
                    set_scissor(builder, run.scissor, full);
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            pipeline.layout().clone(),
                            0,
                            (
                                self.frame_descriptor_set().clone(),
                                self.scene_paint.composite_descriptor(run).clone(),
                            ),
                        )
                        .expect("bind_descriptor_sets scene");
                    builder
                        .bind_vertex_buffers(
                            0,
                            (
                                self.quad_vbo.clone(),
                                self.scene_paint.composite_instance_buf().clone(),
                            ),
                        )
                        .expect("bind_vertex_buffers scene");
                    unsafe {
                        builder
                            .draw(4, 1, 0, run.composite_instance)
                            .expect("draw scene composite");
                    }
                }
                PaintItem::BackdropSnapshot => {
                    // Marker only — `render()` splits the slice on
                    // these and never includes one in a draw range.
                    // If we're here via `draw()`, the host opted out
                    // of the multi-pass entry point; the boundary is
                    // a no-op and any backdrop draws after it sample
                    // undefined memory.
                }
                PaintItem::Image(idx) => {
                    let run = self.image_paint.run(idx);
                    let pipeline = self.image_paint.pipeline();
                    builder
                        .bind_pipeline_graphics(pipeline.clone())
                        .expect("bind_pipeline_graphics image");
                    self.set_viewport(builder);
                    set_scissor(builder, run.scissor, full);
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            pipeline.layout().clone(),
                            0,
                            (
                                self.frame_descriptor_set().clone(),
                                self.image_paint.descriptor_for_run(run).clone(),
                            ),
                        )
                        .expect("bind_descriptor_sets image");
                    builder
                        .bind_vertex_buffers(
                            0,
                            (
                                self.quad_vbo.clone(),
                                self.image_paint.instance_buf().clone(),
                            ),
                        )
                        .expect("bind_vertex_buffers image");
                    unsafe {
                        builder
                            .draw(4, run.count, 0, run.first)
                            .expect("draw image");
                    }
                }
                PaintItem::AppTexture(idx) => {
                    let run = self.surface_paint.run(idx);
                    let pipeline = self.surface_paint.pipeline_for(run.alpha);
                    builder
                        .bind_pipeline_graphics(pipeline.clone())
                        .expect("bind_pipeline_graphics surface");
                    self.set_viewport(builder);
                    set_scissor(builder, run.scissor, full);
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            pipeline.layout().clone(),
                            0,
                            (
                                self.frame_descriptor_set().clone(),
                                self.surface_paint.descriptor_for_run(run).clone(),
                            ),
                        )
                        .expect("bind_descriptor_sets surface");
                    builder
                        .bind_vertex_buffers(
                            0,
                            (
                                self.quad_vbo.clone(),
                                self.surface_paint.instance_buf().clone(),
                            ),
                        )
                        .expect("bind_vertex_buffers surface");
                    unsafe {
                        builder
                            .draw(4, run.count, 0, run.first)
                            .expect("draw surface");
                    }
                }
                PaintItem::IconRun(idx) | PaintItem::Vector(idx) => {
                    // Vector and IconRun both index into IconPaint::runs
                    // (record_vector appends there); the variant is
                    // kept for paint-stream provenance only.
                    let run = self.icon_paint.run(idx);
                    match run.kind {
                        IconRunKind::Tess => {
                            let pipeline = self.icon_paint.tess_pipeline(run.material);
                            builder
                                .bind_pipeline_graphics(pipeline.clone())
                                .expect("bind_pipeline_graphics icon tess");
                            self.set_viewport(builder);
                            set_scissor(builder, run.scissor, full);
                            builder
                                .bind_descriptor_sets(
                                    PipelineBindPoint::Graphics,
                                    pipeline.layout().clone(),
                                    0,
                                    self.frame_descriptor_set().clone(),
                                )
                                .expect("bind_descriptor_sets icon tess");
                            builder
                                .bind_vertex_buffers(0, self.icon_paint.tess_vertex_buf().clone())
                                .expect("bind_vertex_buffers icon tess");
                            unsafe {
                                builder
                                    .draw(run.count, 1, run.first, 0)
                                    .expect("draw icon tess");
                            }
                        }
                        IconRunKind::Msdf => {
                            let pipeline = self.icon_paint.msdf_pipeline();
                            builder
                                .bind_pipeline_graphics(pipeline.clone())
                                .expect("bind_pipeline_graphics icon msdf");
                            self.set_viewport(builder);
                            set_scissor(builder, run.scissor, full);
                            builder
                                .bind_descriptor_sets(
                                    PipelineBindPoint::Graphics,
                                    pipeline.layout().clone(),
                                    0,
                                    (
                                        self.frame_descriptor_set().clone(),
                                        self.icon_paint.msdf_page_descriptor(run.page).clone(),
                                    ),
                                )
                                .expect("bind_descriptor_sets icon msdf");
                            builder
                                .bind_vertex_buffers(
                                    0,
                                    (
                                        self.quad_vbo.clone(),
                                        self.icon_paint.msdf_instance_buf().clone(),
                                    ),
                                )
                                .expect("bind_vertex_buffers icon msdf");
                            unsafe {
                                builder
                                    .draw(4, run.count, 0, run.first)
                                    .expect("draw icon msdf");
                            }
                        }
                    }
                }
                PaintItem::Text(idx) => {
                    let run = self.text_paint.run(idx);
                    let text_pipeline = self.text_paint.pipeline_for(run.kind);
                    builder
                        .bind_pipeline_graphics(text_pipeline.clone())
                        .expect("bind_pipeline_graphics text");
                    self.set_viewport(builder);
                    set_scissor(builder, run.scissor, full);
                    // Glyph kinds bind set 0 (FrameUniforms) + set 1
                    // (per-page atlas image + sampler). Highlight kind
                    // binds set 0 only — its pipeline carries no set 1.
                    match run.kind {
                        crate::text::TextRunKind::Color | crate::text::TextRunKind::Msdf => {
                            builder
                                .bind_descriptor_sets(
                                    PipelineBindPoint::Graphics,
                                    text_pipeline.layout().clone(),
                                    0,
                                    (
                                        self.frame_descriptor_set().clone(),
                                        self.text_paint.page_descriptor(run.kind, run.page).clone(),
                                    ),
                                )
                                .expect("bind_descriptor_sets text");
                        }
                        crate::text::TextRunKind::Highlight => {
                            builder
                                .bind_descriptor_sets(
                                    PipelineBindPoint::Graphics,
                                    text_pipeline.layout().clone(),
                                    0,
                                    self.frame_descriptor_set().clone(),
                                )
                                .expect("bind_descriptor_sets text highlight");
                        }
                    }
                    match run.kind {
                        crate::text::TextRunKind::Color => {
                            builder
                                .bind_vertex_buffers(
                                    0,
                                    (
                                        self.quad_vbo.clone(),
                                        self.text_paint.instance_buf_color().clone(),
                                    ),
                                )
                                .expect("bind_vertex_buffers text colour");
                        }
                        crate::text::TextRunKind::Msdf => {
                            builder
                                .bind_vertex_buffers(
                                    0,
                                    (
                                        self.quad_vbo.clone(),
                                        self.text_paint.instance_buf_msdf().clone(),
                                    ),
                                )
                                .expect("bind_vertex_buffers text msdf");
                        }
                        crate::text::TextRunKind::Highlight => {
                            builder
                                .bind_vertex_buffers(
                                    0,
                                    (
                                        self.quad_vbo.clone(),
                                        self.text_paint.instance_buf_highlight().clone(),
                                    ),
                                )
                                .expect("bind_vertex_buffers text highlight");
                        }
                    }
                    unsafe {
                        builder.draw(4, run.count, 0, run.first).expect("draw text");
                    }
                }
            }
        }
    }

    /// Per-frame quad-instance suballocation, set in `prepare()`.
    /// Panics if called outside the prepare → draw cycle (i.e. before
    /// the first frame, or after a frame with no quad draws). Bind
    /// sites are gated by the QuadRun / Image / Text PaintItems, all
    /// of which `prepare()` only emits when there's data to upload.
    fn instance_buf(&self) -> &Subbuffer<[QuadInstance]> {
        self.instance_buf
            .as_ref()
            .expect("damascene-vulkano: instance_buf accessed before prepare()")
    }

    /// Per-frame descriptor set holding the freshly-rebuilt FrameUniforms.
    /// Always set after `prepare()` (the uniform write is unconditional);
    /// panics if drawn before any prepare.
    fn frame_descriptor_set(&self) -> &Arc<DescriptorSet> {
        self.frame_descriptor_set
            .as_ref()
            .expect("damascene-vulkano: frame_descriptor_set accessed before prepare()")
    }

    /// (Re)allocate the snapshot image to match `target_image`'s
    /// extent + format. Idempotent when the size matches; rebuilds the
    /// `backdrop_descriptor_set` whenever the snapshot is recreated.
    fn ensure_snapshot(&mut self, target_image: Arc<Image>) {
        let want = target_image.extent();
        if let Some(s) = &self.snapshot
            && s.extent == want
        {
            return;
        }
        let image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: target_image.format(),
                extent: want,
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("damascene-vulkano: backdrop snapshot image");
        let view = ImageView::new_default(image.clone())
            .expect("damascene-vulkano: backdrop snapshot view");
        let layout = self
            .backdrop_set_layout
            .clone()
            .expect("ensure_snapshot called but no backdrop shader registered");
        let set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            layout,
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.backdrop_sampler.clone()),
            ],
            [],
        )
        .expect("damascene-vulkano: backdrop descriptor set");
        self.snapshot = Some(SnapshotImage {
            image,
            extent: want,
        });
        self.backdrop_descriptor_set = Some(set);
    }
}

/// Build the runner's render pass for a single color subpass at the
/// requested sample count. When `sample_count == 1` the pass has one
/// color attachment in `target_format`; when greater, a multisampled
/// color attachment is added and its contents are resolved into a
/// single-sample resolve attachment (also `target_format`) at end of
/// pass. The framebuffer the host binds must match the attachment count
/// — one view for single-sample, two views (msaa, resolve) for MSAA.
///
/// `load_op_clear` picks the per-frame entry behavior: `true` clears at
/// pass-begin (Pass A, no backdrop boundary), `false` loads (Pass B,
/// preserving Pass A's pixels through the snapshot boundary). The MSAA
/// color attachment stores between passes so Pass B's `load_op: Load`
/// has valid contents to resume from.
fn build_render_pass(
    device: Arc<Device>,
    target_format: Format,
    sample_count: u32,
    load_op_clear: bool,
) -> Arc<RenderPass> {
    if sample_count <= 1 {
        return if load_op_clear {
            vulkano::single_pass_renderpass!(
                device,
                attachments: {
                    color: {
                        format: target_format,
                        samples: 1,
                        load_op: Clear,
                        store_op: Store,
                    },
                },
                pass: {
                    color: [color],
                    depth_stencil: {},
                },
            )
        } else {
            vulkano::single_pass_renderpass!(
                device,
                attachments: {
                    color: {
                        format: target_format,
                        samples: 1,
                        load_op: Load,
                        store_op: Store,
                    },
                },
                pass: {
                    color: [color],
                    depth_stencil: {},
                },
            )
        }
        .expect("damascene-vulkano: create single-sample render pass");
    }

    if load_op_clear {
        vulkano::single_pass_renderpass!(
            device,
            attachments: {
                color_msaa: {
                    format: target_format,
                    samples: sample_count,
                    load_op: Clear,
                    store_op: Store,
                },
                color_resolve: {
                    format: target_format,
                    samples: 1,
                    load_op: DontCare,
                    store_op: Store,
                },
            },
            pass: {
                color: [color_msaa],
                color_resolve: [color_resolve],
                depth_stencil: {},
            },
        )
    } else {
        vulkano::single_pass_renderpass!(
            device,
            attachments: {
                color_msaa: {
                    format: target_format,
                    samples: sample_count,
                    load_op: Load,
                    store_op: Store,
                },
                color_resolve: {
                    format: target_format,
                    samples: 1,
                    load_op: DontCare,
                    store_op: Store,
                },
            },
            pass: {
                color: [color_msaa],
                color_resolve: [color_resolve],
                depth_stencil: {},
            },
        )
    }
    .expect("damascene-vulkano: create multisample render pass")
}
