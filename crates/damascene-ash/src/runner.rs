use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ash::vk;
use damascene_core::event::{KeyChord, KeyModifiers, LogicalKey, PhysicalKey, Pointer, UiEvent};
use damascene_core::icons::svg::IconSource;
use damascene_core::ir::TextAnchor;
use damascene_core::paint::{IconRunKind, PaintItem, PhysicalScissor, QuadInstance};
use damascene_core::runtime::{RecordedPaint, RunnerCore, TextRecorder};
use damascene_core::shader::{ShaderHandle, StockShader, stock_wgsl};
use damascene_core::state::{AnimationMode, UiState};
use damascene_core::surface::SurfaceAlpha;
use damascene_core::text::atlas::RunStyle;
use damascene_core::theme::Theme;
use damascene_core::tree::{Color, El, FontWeight, Rect, TextWrap};
use damascene_core::vector::IconMaterial;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocator;
use web_time::Instant;

use crate::buffer::GpuBuffer;
use crate::icon::IconPaint;
use crate::image::{ImagePaint, ImageRecord};
use crate::naga_compile::{CompileError, wgsl_to_spirv};
use crate::pipeline::{FrameUniforms, build_quad_pipeline, build_quad_pipeline_from_spirv};
use crate::scene::Scene3DPaint;
use crate::surface::SurfacePaint;
use crate::text::{TextPaint, TextRunKind};

const INITIAL_INSTANCE_CAPACITY: usize = 256;
const QUAD_VERTICES: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

pub type Result<T> = std::result::Result<T, Error>;

/// Errors surfaced by the ash backend.
#[derive(Debug)]
pub enum Error {
    ShaderCompile(CompileError),
    Vulkan {
        op: &'static str,
        result: vk::Result,
    },
    Allocation(gpu_allocator::AllocationError),
    AllocatorPoisoned,
    BufferTooSmall {
        requested: usize,
        capacity: usize,
    },
    UnmappedAllocation,
    ResourceDestroyed(&'static str),
    IncompatibleTarget {
        expected_format: vk::Format,
        actual_format: vk::Format,
        expected_sample_count: vk::SampleCountFlags,
        actual_sample_count: vk::SampleCountFlags,
    },
    MissingPipeline(ShaderHandle),
    PipelineCreationReturnedEmpty {
        name: String,
    },
    Unsupported(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ShaderCompile(err) => err.fmt(f),
            Error::Vulkan { op, result } => write!(f, "Vulkan error in {op}: {result:?}"),
            Error::Allocation(err) => err.fmt(f),
            Error::AllocatorPoisoned => write!(f, "Vulkan allocator mutex was poisoned"),
            Error::BufferTooSmall {
                requested,
                capacity,
            } => {
                write!(
                    f,
                    "buffer upload requested {requested} bytes but capacity is {capacity} bytes"
                )
            }
            Error::UnmappedAllocation => write!(f, "GPU allocation is not host-mapped"),
            Error::ResourceDestroyed(name) => write!(f, "resource was already destroyed: {name}"),
            Error::IncompatibleTarget {
                expected_format,
                actual_format,
                expected_sample_count,
                actual_sample_count,
            } => write!(
                f,
                "render target is incompatible: expected {expected_format:?}/{expected_sample_count:?}, got {actual_format:?}/{actual_sample_count:?}"
            ),
            Error::MissingPipeline(handle) => {
                write!(f, "no ash pipeline registered for {handle:?}")
            }
            Error::PipelineCreationReturnedEmpty { name } => {
                write!(
                    f,
                    "Vulkan pipeline creation for `{name}` returned no pipeline"
                )
            }
            Error::Unsupported(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ShaderCompile(err) => Some(err),
            Error::Allocation(err) => Some(err),
            Error::Unsupported(_) => None,
            _ => None,
        }
    }
}

impl From<CompileError> for Error {
    fn from(value: CompileError) -> Self {
        Error::ShaderCompile(value)
    }
}

impl From<vk::Result> for Error {
    fn from(value: vk::Result) -> Self {
        Error::Vulkan {
            op: "vulkan call",
            result: value,
        }
    }
}

impl From<gpu_allocator::AllocationError> for Error {
    fn from(value: gpu_allocator::AllocationError) -> Self {
        Error::Allocation(value)
    }
}

/// Host-owned Vulkan context that `damascene-ash` borrows for resource
/// creation and command recording.
///
/// The runner does not submit queues or own frame synchronization. The
/// allocator is explicit because ash does not provide memory/resource
/// ownership. The runner uses it for its internal buffers while leaving
/// allocator construction and lifetime with the host.
pub struct AshContext {
    pub device: Arc<ash::Device>,
    pub allocator: Arc<Mutex<Allocator>>,
    pub queue_family_index: u32,
    /// The selected physical device's `max_image_dimension2_d` limit.
    /// `damascene-ash` holds only the logical device, so the caller —
    /// which has the instance + physical device — must query
    /// `get_physical_device_properties(...).limits.max_image_dimension2_d`
    /// and pass it here. Raster-image uploads larger than this would
    /// fail `vkCreateImage` validation, so they're downscaled to fit
    /// before upload (issue #78).
    pub max_image_dimension_2d: u32,
}

impl AshContext {
    pub fn new(
        device: Arc<ash::Device>,
        allocator: Arc<Mutex<Allocator>>,
        queue_family_index: u32,
        max_image_dimension_2d: u32,
    ) -> Self {
        Self {
            device,
            allocator,
            queue_family_index,
            max_image_dimension_2d,
        }
    }
}

/// Pipeline/target compatibility information supplied at construction.
#[derive(Clone, Copy, Debug)]
pub struct TargetInfo {
    pub format: vk::Format,
    pub sample_count: vk::SampleCountFlags,
}

impl TargetInfo {
    pub fn new(format: vk::Format) -> Self {
        Self {
            format,
            sample_count: vk::SampleCountFlags::TYPE_1,
        }
    }

    pub fn with_sample_count(mut self, sample_count: vk::SampleCountFlags) -> Self {
        self.sample_count = sample_count;
        self
    }
}

/// A host-owned image/view pair Damascene can render into.
#[derive(Clone, Copy, Debug)]
pub struct AshRenderTarget {
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub sample_count: vk::SampleCountFlags,
    /// Layout the host guarantees before Damascene records its commands.
    pub initial_layout: vk::ImageLayout,
    /// Layout Damascene should leave the image in after rendering.
    pub final_layout: vk::ImageLayout,
}

impl AshRenderTarget {
    pub fn color_attachment(
        image: vk::Image,
        view: vk::ImageView,
        format: vk::Format,
        extent: vk::Extent2D,
    ) -> Self {
        Self {
            image,
            view,
            format,
            extent,
            sample_count: vk::SampleCountFlags::TYPE_1,
            initial_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            final_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }
    }
}

/// Load operation for the first Damascene-owned rendering scope.
#[derive(Clone, Copy, Debug)]
pub enum LoadOp {
    Clear([f32; 4]),
    Load,
}

/// Lightweight diagnostic returned by [`Runner::prepared_frame`].
#[derive(Clone, Copy, Debug, Default)]
pub struct PreparedFrame {
    pub quad_instances: usize,
    pub paint_items: usize,
    pub runs: usize,
}

/// Ash runtime owned by a custom host. One runner per compatible target.
pub struct Runner {
    context: AshContext,
    target: TargetInfo,
    frame_buf: GpuBuffer,
    quad_vbo: GpuBuffer,
    instance_buf: GpuBuffer,
    instance_capacity: usize,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    pipeline_layout: vk::PipelineLayout,
    pipelines: HashMap<ShaderHandle, vk::Pipeline>,
    text_paint: TextPaint,
    icon_paint: IconPaint,
    image_paint: ImagePaint,
    surface_paint: SurfacePaint,
    scene_paint: Scene3DPaint,
    backdrop_shaders: HashSet<&'static str>,
    time_shaders: HashSet<&'static str>,
    start_time: Instant,

    /// Output white-level scale written into `FrameUniforms.white_scale`.
    /// 1.0 on SDR targets; a host driving a Windows-scRGB swapchain sets
    /// 203/80 so UI white lands at the encoding's assumed reference
    /// white. See docs/COLOR_MANAGEMENT.md.
    white_scale: f32,
    /// Output luminance headroom (`target_max / reference`, 1.0 on SDR)
    /// and reference white in cd/m², written into
    /// `FrameUniforms.headroom/ref_nits` and (headroom) mirrored into
    /// the image paint for the per-image HDR remaster. See
    /// [`Self::set_output_luminance`].
    headroom: f32,
    ref_nits: f32,
    core: RunnerCore,
}

impl Runner {
    pub fn new(context: AshContext, target: TargetInfo) -> Result<Self> {
        let (frame_buf, quad_vbo, instance_buf) = {
            let mut allocator = context
                .allocator
                .lock()
                .map_err(|_| Error::AllocatorPoisoned)?;
            let frame_buf = GpuBuffer::new(
                &context.device,
                &mut allocator,
                "damascene_ash::frame_uniforms",
                std::mem::size_of::<FrameUniforms>() as vk::DeviceSize,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            let mut quad_vbo = GpuBuffer::new(
                &context.device,
                &mut allocator,
                "damascene_ash::quad_vbo",
                std::mem::size_of_val(&QUAD_VERTICES) as vk::DeviceSize,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            quad_vbo.write_bytes(bytemuck::cast_slice(&QUAD_VERTICES))?;
            let instance_buf = GpuBuffer::new(
                &context.device,
                &mut allocator,
                "damascene_ash::instance_buf",
                (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<QuadInstance>()) as vk::DeviceSize,
                vk::BufferUsageFlags::VERTEX_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            (frame_buf, quad_vbo, instance_buf)
        };

        let descriptor_set_layout = create_frame_descriptor_set_layout(&context.device)?;
        let descriptor_pool = create_descriptor_pool(&context.device)?;
        let descriptor_set =
            allocate_frame_descriptor_set(&context.device, descriptor_pool, descriptor_set_layout)?;
        update_frame_descriptor_set(&context.device, descriptor_set, frame_buf.buffer);
        let pipeline_layout = create_pipeline_layout(&context.device, descriptor_set_layout)?;
        let text_paint = {
            let mut allocator = context
                .allocator
                .lock()
                .map_err(|_| Error::AllocatorPoisoned)?;
            TextPaint::new(
                &context.device,
                &mut allocator,
                descriptor_set_layout,
                target,
            )?
        };
        let icon_paint = {
            let mut allocator = context
                .allocator
                .lock()
                .map_err(|_| Error::AllocatorPoisoned)?;
            IconPaint::new(
                &context.device,
                &mut allocator,
                descriptor_set_layout,
                target,
            )?
        };
        let image_paint = {
            let mut allocator = context
                .allocator
                .lock()
                .map_err(|_| Error::AllocatorPoisoned)?;
            ImagePaint::new(
                &context.device,
                &mut allocator,
                descriptor_set_layout,
                target,
                context.max_image_dimension_2d,
            )?
        };
        let surface_paint = {
            let mut allocator = context
                .allocator
                .lock()
                .map_err(|_| Error::AllocatorPoisoned)?;
            SurfacePaint::new(
                &context.device,
                &mut allocator,
                descriptor_set_layout,
                target,
            )?
        };
        let scene_paint = {
            let mut allocator = context
                .allocator
                .lock()
                .map_err(|_| Error::AllocatorPoisoned)?;
            Scene3DPaint::new(
                &context.device,
                &mut allocator,
                descriptor_set_layout,
                target,
                damascene_core::paint::DEFAULT_WORKING_COLOR_SPACE,
            )?
        };

        let mut runner = Self {
            context,
            target,
            frame_buf,
            quad_vbo,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
            pipeline_layout,
            pipelines: HashMap::new(),
            text_paint,
            icon_paint,
            image_paint,
            surface_paint,
            scene_paint,
            backdrop_shaders: HashSet::new(),
            time_shaders: HashSet::new(),
            start_time: Instant::now(),
            white_scale: 1.0,
            headroom: 1.0,
            ref_nits: damascene_core::color::BT2408_REFERENCE_WHITE_NITS,
            core: RunnerCore::new(),
        };
        runner.register_stock_shaders()?;
        Ok(runner)
    }

    pub fn target(&self) -> TargetInfo {
        self.target
    }

    pub fn device(&self) -> &ash::Device {
        &self.context.device
    }

    pub fn queue_family_index(&self) -> u32 {
        self.context.queue_family_index
    }

    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.core.set_surface_size(width, height);
    }

    /// Set the color space the renderer composites in. Hosts call this
    /// once after negotiating a swapchain format with the display
    /// server and before the first frame. Updates the shared quad path
    /// (via `RunnerCore`) and this backend's text / icon / image /
    /// scene color recorders so every color crosses the working-space
    /// boundary consistently.
    ///
    /// The working space must match how the swapchain interprets the
    /// pixels the renderer writes: `SRGB_LINEAR` for an `*_SRGB`
    /// format (the default), `SCRGB_LINEAR` for
    /// `R16G16B16A16_SFLOAT` + `EXTENDED_SRGB_LINEAR_EXT`, etc.
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

    /// Set the output white-level scale (default 1.0) written into
    /// `FrameUniforms.white_scale`. Hosts driving a Windows-scRGB
    /// (`R16G16B16A16_SFLOAT`) swapchain pass
    /// `damascene_core::color::WINDOWS_SCRGB_WHITE_SCALE` so SDR-referred UI
    /// white lands at the encoding's assumed reference white (203 cd/m²)
    /// instead of 80 cd/m². See docs/COLOR_MANAGEMENT.md.
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

    pub fn set_theme(&mut self, theme: Theme) {
        self.icon_paint.set_material(theme.icon_material());
        self.core.set_theme(theme);
    }

    pub fn theme(&self) -> &Theme {
        self.core.theme()
    }

    /// Select the stock material used by the vector-icon painter.
    pub fn set_icon_material(&mut self, material: IconMaterial) {
        self.icon_paint.set_material(material);
    }

    pub fn icon_material(&self) -> IconMaterial {
        self.icon_paint.material()
    }

    pub fn register_shader(&mut self, name: &'static str, wgsl: &str) -> Result<()> {
        self.register_shader_with(name, wgsl, false, false)
    }

    pub fn register_shader_with(
        &mut self,
        name: &'static str,
        wgsl: &str,
        samples_backdrop: bool,
        samples_time: bool,
    ) -> Result<()> {
        if samples_backdrop {
            return Err(Error::Unsupported(
                "damascene-ash backdrop-sampling custom shaders are not implemented yet",
            ));
        }
        let words = wgsl_to_spirv(name, wgsl)?;
        let pipeline = build_quad_pipeline_from_spirv(
            &self.context.device,
            self.pipeline_layout,
            self.target,
            name,
            &words,
        )?;
        if let Some(old) = self.pipelines.insert(ShaderHandle::Custom(name), pipeline) {
            unsafe {
                self.context.device.destroy_pipeline(old, None);
            }
        }
        self.backdrop_shaders.remove(name);
        if samples_time {
            self.time_shaders.insert(name);
        } else {
            self.time_shaders.remove(name);
        }
        // Named uniform routing + Rust↔WGSL drift detection (issue
        // #99); failure is non-fatal — positional vec_a..vec_e routing
        // still works.
        match damascene_core::paint::slots::introspect_wgsl(wgsl) {
            Ok(map) => self.core.register_shader_slots(name, map),
            Err(e) => log::warn!(
                "damascene-ash: could not introspect shader `{name}` for named uniform \
                 routing ({e}); positional vec_a..vec_e routing still applies"
            ),
        }
        Ok(())
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

    /// Pointer cursor resolved from the snapshot tree `prepare` just
    /// stored. Call after `prepare`; paint-only frames keep the
    /// previously resolved cursor.
    pub fn snapshot_cursor(&self) -> damascene_core::cursor::Cursor {
        self.core.snapshot_cursor()
    }

    pub fn prepared_frame(&self) -> PreparedFrame {
        PreparedFrame {
            quad_instances: self.core.quad_scratch.len(),
            paint_items: self.core.paint_items.len(),
            runs: self.core.runs.len(),
        }
    }

    /// Lay out the tree and lower it into this frame's paint data.
    ///
    /// # Synchronization
    ///
    /// `prepare` writes persistently-mapped vertex/uniform buffers and
    /// immediately destroys GPU resources it evicts or regrows
    /// (instance buffers, scene geometry, cached textures, descriptor
    /// sets). It does **not** fence-gate any of that, so the host must
    /// guarantee that every previously submitted command buffer that
    /// references this runner's resources has completed before calling
    /// it — in a classic frames-in-flight loop, wait the frame fence
    /// *before* `prepare`, not just before recording.
    pub fn prepare(
        &mut self,
        mut root: El,
        viewport: Rect,
        scale_factor: f32,
    ) -> damascene_core::runtime::PrepareResult {
        let mut timings = damascene_core::runtime::PrepareTimings::default();

        // Install any scene depth maps the previous frame's capture finished
        // reading back, so this frame's `draw_ops` can occlude scene-anchored
        // labels behind geometry. Done before `prepare_layout`; stale maps for
        // scenes that left the tree are GC'd.
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

        let time_shaders = &self.time_shaders;
        let damascene_core::runtime::LayoutPrepared {
            ops,
            mut needs_redraw,
            mut next_layout_redraw_in,
            next_paint_redraw_in,
        } = self.core.prepare_layout(
            &mut root,
            viewport,
            scale_factor,
            &mut timings,
            |handle| matches!(handle, ShaderHandle::Custom(name) if time_shaders.contains(name)),
        );

        self.text_paint.frame_begin();
        self.icon_paint.frame_begin();
        self.image_paint.frame_begin();
        self.surface_paint.frame_begin();
        self.scene_paint.frame_begin();
        let pipelines = &self.pipelines;
        let backdrop_shaders = &self.backdrop_shaders;
        {
            let mut allocator = self
                .context
                .allocator
                .lock()
                .expect("damascene-ash: allocator mutex poisoned during paint recording");
            let mut recorder = PaintRecorder {
                text: &mut self.text_paint,
                icons: &mut self.icon_paint,
                images: &mut self.image_paint,
                surfaces: &mut self.surface_paint,
                scenes: &mut self.scene_paint,
                device: &self.context.device,
                allocator: &mut allocator,
            };
            self.core.prepare_paint(
                &ops,
                |shader| match shader {
                    ShaderHandle::Stock(StockShader::RoundedRect)
                    | ShaderHandle::Stock(StockShader::Spinner)
                    | ShaderHandle::Stock(StockShader::Skeleton)
                    | ShaderHandle::Stock(StockShader::ProgressIndeterminate) => {
                        pipelines.contains_key(shader)
                    }
                    ShaderHandle::Custom(_) => pipelines.contains_key(shader),
                    _ => false,
                },
                |shader| matches!(shader, ShaderHandle::Custom(name) if backdrop_shaders.contains(name)),
                &mut recorder,
                scale_factor,
                &mut timings,
            );
        }
        let t_gpu_start = Instant::now();
        self.upload_frame_data(viewport, scale_factor)
            .expect("damascene-ash: failed to upload prepared frame data");
        timings.gpu_upload = Instant::now() - t_gpu_start;

        self.core.snapshot_owned(root, &mut timings);
        self.core.last_ops = ops;

        // Keep frames coming until every labelled scene has a depth map for its
        // current pose — the async-ish read-back needs a couple of frames, and
        // a capture started in `render` would sit unread after the camera
        // settles. Drive the *layout* lane (the paint-only `repaint` path skips
        // `collect_depth_maps`), not just `needs_redraw`. Settled + current
        // scenes report `false`, so lazy idle is preserved. Mirrors wgpu/vulkano.
        if self.scene_paint.occlusion_unsettled() {
            needs_redraw = true;
            next_layout_redraw_in = Some(std::time::Duration::ZERO);
        }

        let next_redraw_in = match (next_layout_redraw_in, next_paint_redraw_in) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };

        damascene_core::runtime::PrepareResult {
            needs_redraw,
            next_redraw_in,
            next_layout_redraw_in,
            next_paint_redraw_in,
            timings,
        }
    }

    pub fn repaint(
        &mut self,
        _viewport: Rect,
        scale_factor: f32,
    ) -> damascene_core::runtime::PrepareResult {
        let mut timings = damascene_core::runtime::PrepareTimings::default();
        let time_shaders = &self.time_shaders;
        self.text_paint.frame_begin();
        self.icon_paint.frame_begin();
        self.image_paint.frame_begin();
        self.surface_paint.frame_begin();
        self.scene_paint.frame_begin();
        let pipelines = &self.pipelines;
        let backdrop_shaders = &self.backdrop_shaders;
        {
            let mut allocator = self
                .context
                .allocator
                .lock()
                .expect("damascene-ash: allocator mutex poisoned during paint recording");
            let mut recorder = PaintRecorder {
                text: &mut self.text_paint,
                icons: &mut self.icon_paint,
                images: &mut self.image_paint,
                surfaces: &mut self.surface_paint,
                scenes: &mut self.scene_paint,
                device: &self.context.device,
                allocator: &mut allocator,
            };
            self.core.prepare_paint_cached(
                |shader| match shader {
                    ShaderHandle::Stock(StockShader::RoundedRect)
                    | ShaderHandle::Stock(StockShader::Spinner)
                    | ShaderHandle::Stock(StockShader::Skeleton)
                    | ShaderHandle::Stock(StockShader::ProgressIndeterminate) => {
                        pipelines.contains_key(shader)
                    }
                    ShaderHandle::Custom(_) => pipelines.contains_key(shader),
                    _ => false,
                },
                |shader| matches!(shader, ShaderHandle::Custom(name) if backdrop_shaders.contains(name)),
                &mut recorder,
                scale_factor,
                &mut timings,
            );
        }
        let next_paint_redraw_in = self.core.scan_continuous_shaders(
            |handle| matches!(handle, ShaderHandle::Custom(name) if time_shaders.contains(name)),
        );
        self.upload_frame_data(_viewport, scale_factor)
            .expect("damascene-ash: failed to upload repainted frame data");
        damascene_core::runtime::PrepareResult {
            needs_redraw: next_paint_redraw_in.is_some(),
            next_redraw_in: next_paint_redraw_in,
            next_layout_redraw_in: None,
            next_paint_redraw_in,
            timings,
        }
    }

    /// Record Damascene draw commands into a host-opened dynamic-rendering scope.
    ///
    /// **3D scenes need the pre-pass.** `Scene3D` paint items composite
    /// from offscreen targets that must be rendered before the host's
    /// rendering scope begins — call [`Self::record_scene_prepass`]
    /// first (outside the scope, after [`Self::record_uploads`]), or
    /// every scene in the frame samples a never-rendered target and
    /// composites blank.
    ///
    /// # Safety
    ///
    /// The command buffer must be recording, graphics-compatible, and
    /// inside a dynamic-rendering scope whose color attachment format
    /// and sample count match this runner's target. The host is
    /// responsible for synchronization and image layouts. Classic
    /// render-pass/framebuffer compatibility is not wired yet.
    pub unsafe fn draw(&self, _cmd: vk::CommandBuffer) -> Result<()> {
        unsafe { self.draw_items(_cmd, &self.core.paint_items) }
    }

    /// Record the offscreen pre-pass for any 3D scenes in this frame's
    /// paint stream: each `Scene3D` renders into its own offscreen
    /// target, and label-bearing scenes capture depth for next frame's
    /// label occlusion. No-op when the frame has no scenes.
    ///
    /// Hosts using [`Self::render`] do not need to call this directly.
    /// Hosts using [`Self::draw`] should call it after
    /// [`Self::record_uploads`] and before beginning their
    /// dynamic-rendering scope.
    ///
    /// # Safety
    ///
    /// The command buffer must be recording and outside a render pass
    /// or dynamic-rendering scope (the pre-pass opens and closes its
    /// own scopes).
    pub unsafe fn record_scene_prepass(&mut self, cmd: vk::CommandBuffer) {
        if self.scene_paint.has_runs() {
            unsafe {
                self.scene_paint.encode_offscreen(&self.context.device, cmd);
                // Capture each label-bearing scene's depth into its
                // read-back buffer (the depth is still stored from the
                // pass above). The CPU read happens next frame in
                // `prepare`, after the fence.
                self.scene_paint
                    .encode_depth_capture(&self.context.device, cmd);
            }
        }
    }

    /// Record pending atlas/texture uploads before drawing.
    ///
    /// Hosts using [`Self::render`] do not need to call this directly;
    /// `render` records uploads before opening its dynamic-rendering scope.
    /// Hosts using [`Self::draw`] inside their own rendering scope should
    /// call this after `prepare`/`repaint` and before beginning rendering.
    ///
    /// # Safety
    ///
    /// The command buffer must be recording and outside a render pass or
    /// dynamic-rendering scope. The host must keep submitted work ordered
    /// so retained staging buffers are not dropped before the copy commands
    /// complete.
    pub unsafe fn record_uploads(&mut self, cmd: vk::CommandBuffer) -> Result<()> {
        unsafe {
            self.text_paint
                .record_pending_uploads(&self.context.device, cmd)?;
            self.icon_paint
                .record_pending_uploads(&self.context.device, cmd)?;
            self.image_paint
                .record_pending_uploads(&self.context.device, cmd);
        }
        Ok(())
    }

    /// Record Damascene-owned rendering into `target`.
    ///
    /// # Safety
    ///
    /// The command buffer must be recording and graphics-compatible.
    /// The target image/view must be valid for color attachment use,
    /// with layouts and synchronization matching `AshRenderTarget`.
    pub unsafe fn render(
        &mut self,
        _cmd: vk::CommandBuffer,
        _target: AshRenderTarget,
        _load: LoadOp,
    ) -> Result<()> {
        if _target.format != self.target.format || _target.sample_count != self.target.sample_count
        {
            return Err(Error::IncompatibleTarget {
                expected_format: self.target.format,
                actual_format: _target.format,
                expected_sample_count: self.target.sample_count,
                actual_sample_count: _target.sample_count,
            });
        }

        let (load_op, clear_value) = match _load {
            LoadOp::Clear(color) => (
                vk::AttachmentLoadOp::CLEAR,
                vk::ClearValue {
                    color: vk::ClearColorValue { float32: color },
                },
            ),
            LoadOp::Load => (vk::AttachmentLoadOp::LOAD, vk::ClearValue::default()),
        };
        unsafe {
            self.record_uploads(_cmd)?;
            // 3D scenes render into their own offscreen targets first, ahead
            // of the main rendering scope (scopes can't nest), leaving each
            // resolved image ready to sample for the composite below.
            self.record_scene_prepass(_cmd);
            transition_color_target(
                &self.context.device,
                _cmd,
                _target.image,
                _target.initial_layout,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            );
        }

        let color_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(_target.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(load_op)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(clear_value);
        let color_attachments = [color_attachment];
        let rendering_info = vk::RenderingInfo::default()
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: _target.extent,
            })
            .layer_count(1)
            .color_attachments(&color_attachments);

        unsafe {
            self.context
                .device
                .cmd_begin_rendering(_cmd, &rendering_info);
            self.draw_items(_cmd, &self.core.paint_items)?;
            self.context.device.cmd_end_rendering(_cmd);
            transition_color_target(
                &self.context.device,
                _cmd,
                _target.image,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                _target.final_layout,
            );
        }
        Ok(())
    }

    pub fn pointer_moved(&mut self, p: Pointer) -> damascene_core::runtime::PointerMove {
        self.core.pointer_moved(p)
    }

    pub fn pointer_left(&mut self) -> Vec<UiEvent> {
        self.core.pointer_left()
    }

    pub fn pointer_cancelled(&mut self) -> Vec<UiEvent> {
        self.core.pointer_cancelled()
    }

    pub fn file_hovered(&mut self, path: PathBuf, x: f32, y: f32) -> Vec<UiEvent> {
        self.core.file_hovered(path, x, y)
    }

    pub fn file_hover_cancelled(&mut self) -> Vec<UiEvent> {
        self.core.file_hover_cancelled()
    }

    pub fn file_dropped(&mut self, path: PathBuf, x: f32, y: f32) -> Vec<UiEvent> {
        self.core.file_dropped(path, x, y)
    }

    pub fn would_press_focus_text_input(&self, x: f32, y: f32) -> bool {
        self.core.would_press_focus_text_input(x, y)
    }

    pub fn focused_captures_keys(&self) -> bool {
        self.core.focused_captures_keys()
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

    pub fn key_down(
        &mut self,
        logical: LogicalKey,
        physical: PhysicalKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Vec<UiEvent> {
        self.core.key_down(logical, physical, modifiers, repeat)
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

    pub fn selected_text(&self) -> Option<String> {
        self.core.selected_text()
    }

    pub fn selected_text_for(
        &self,
        selection: &damascene_core::selection::Selection,
    ) -> Option<String> {
        self.core.selected_text_for(selection)
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

    pub fn push_viewport_requests(
        &mut self,
        requests: Vec<damascene_core::viewport::ViewportRequest>,
    ) {
        self.core.push_viewport_requests(requests);
    }

    pub fn push_plot_requests(&mut self, requests: Vec<damascene_core::plot::PlotRequest>) {
        self.core.push_plot_requests(requests);
    }

    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.core.set_animation_mode(mode);
    }

    /// Replace the host-reported user accessibility preferences (the
    /// CSS `prefers-*` family). Call at startup and again whenever a
    /// platform setting changes; see [`damascene_core::a11y`] for what
    /// the runtime honors automatically.
    pub fn set_accessibility_preferences(
        &mut self,
        prefs: damascene_core::a11y::AccessibilityPreferences,
    ) {
        self.core.set_accessibility_preferences(prefs);
    }

    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.core.pointer_wheel(x, y, dy)
    }

    pub fn pointer_wheel_event(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Option<UiEvent> {
        self.core.pointer_wheel_event(x, y, dx, dy)
    }

    pub fn poll_input(&mut self, now: Instant) -> Vec<UiEvent> {
        self.core.poll_input(now)
    }

    fn upload_frame_data(&mut self, viewport: Rect, scale_factor: f32) -> Result<()> {
        self.ensure_instance_capacity(self.core.quad_scratch.len())?;
        let frame = FrameUniforms {
            viewport: [viewport.w, viewport.h],
            time: (Instant::now() - self.start_time).as_secs_f32(),
            scale_factor,
            white_scale: self.white_scale,
            headroom: self.headroom,
            ref_nits: self.ref_nits,
            _reserved: 0.0,
        };
        self.frame_buf.write_bytes(bytemuck::bytes_of(&frame))?;
        self.instance_buf
            .write_bytes(bytemuck::cast_slice(&self.core.quad_scratch))?;
        let mut allocator = self
            .context
            .allocator
            .lock()
            .map_err(|_| Error::AllocatorPoisoned)?;
        self.text_paint
            .flush(&self.context.device, &mut allocator)?;
        self.icon_paint
            .flush(&self.context.device, &mut allocator)?;
        self.image_paint
            .flush(&self.context.device, &mut allocator)?;
        self.surface_paint
            .flush(&self.context.device, &mut allocator)?;
        self.scene_paint
            .flush(&self.context.device, &mut allocator)?;
        Ok(())
    }

    fn ensure_instance_capacity(&mut self, instances: usize) -> Result<()> {
        if instances <= self.instance_capacity {
            return Ok(());
        }
        let mut next = self.instance_capacity.max(1);
        while next < instances {
            next *= 2;
        }
        let mut allocator = self
            .context
            .allocator
            .lock()
            .map_err(|_| Error::AllocatorPoisoned)?;
        unsafe {
            self.instance_buf
                .destroy(&self.context.device, &mut allocator);
        }
        self.instance_buf = GpuBuffer::new(
            &self.context.device,
            &mut allocator,
            "damascene_ash::instance_buf",
            (next * std::mem::size_of::<QuadInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        self.instance_capacity = next;
        Ok(())
    }

    unsafe fn draw_items(&self, cmd: vk::CommandBuffer, items: &[PaintItem]) -> Result<()> {
        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: self.core.viewport_px.0,
            h: self.core.viewport_px.1,
        };
        for item in items {
            match *item {
                PaintItem::QuadRun(index) => {
                    let run = &self.core.runs[index];
                    let pipeline = self
                        .pipelines
                        .get(&run.handle)
                        .ok_or(Error::MissingPipeline(run.handle))?;
                    unsafe {
                        self.context.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            *pipeline,
                        );
                        self.set_viewport(cmd);
                        self.set_scissor(cmd, run.scissor, full);
                        self.context.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            self.pipeline_layout,
                            0,
                            &[self.descriptor_set],
                            &[],
                        );
                        self.context.device.cmd_bind_vertex_buffers(
                            cmd,
                            0,
                            &[self.quad_vbo.buffer, self.instance_buf.buffer],
                            &[0, 0],
                        );
                        self.context
                            .device
                            .cmd_draw(cmd, 4, run.count, 0, run.first);
                    }
                }
                PaintItem::Text(index) => {
                    let run = self.text_paint.run(index);
                    let pipeline = self.text_paint.pipeline_for(run.kind);
                    let layout = self.text_paint.pipeline_layout_for(run.kind);
                    unsafe {
                        self.context.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipeline,
                        );
                        self.set_viewport(cmd);
                        self.set_scissor(cmd, run.scissor, full);
                        match run.kind {
                            TextRunKind::Color | TextRunKind::Msdf => {
                                let page = self.text_paint.page_descriptor(run.kind, run.page);
                                self.context.device.cmd_bind_descriptor_sets(
                                    cmd,
                                    vk::PipelineBindPoint::GRAPHICS,
                                    layout,
                                    0,
                                    &[self.descriptor_set, page],
                                    &[],
                                );
                            }
                            TextRunKind::Highlight => {
                                self.context.device.cmd_bind_descriptor_sets(
                                    cmd,
                                    vk::PipelineBindPoint::GRAPHICS,
                                    layout,
                                    0,
                                    &[self.descriptor_set],
                                    &[],
                                );
                            }
                        }
                        self.context.device.cmd_bind_vertex_buffers(
                            cmd,
                            0,
                            &[
                                self.quad_vbo.buffer,
                                self.text_paint.instance_buffer(run.kind),
                            ],
                            &[0, 0],
                        );
                        self.context
                            .device
                            .cmd_draw(cmd, 4, run.count, 0, run.first);
                    }
                }
                PaintItem::Image(index) => {
                    let run = self.image_paint.run(index);
                    let pipeline = self.image_paint.pipeline();
                    let layout = self.image_paint.pipeline_layout();
                    let texture_set = self.image_paint.descriptor_for_run(run);
                    unsafe {
                        self.context.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipeline,
                        );
                        self.set_viewport(cmd);
                        self.set_scissor(cmd, run.scissor, full);
                        self.context.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            layout,
                            0,
                            &[self.descriptor_set, texture_set],
                            &[],
                        );
                        self.context.device.cmd_bind_vertex_buffers(
                            cmd,
                            0,
                            &[self.quad_vbo.buffer, self.image_paint.instance_buffer()],
                            &[0, 0],
                        );
                        self.context
                            .device
                            .cmd_draw(cmd, 4, run.count, 0, run.first);
                    }
                }
                PaintItem::IconRun(index) | PaintItem::Vector(index) => {
                    let run = self.icon_paint.run(index);
                    match run.kind {
                        IconRunKind::Msdf => {
                            let page = self.icon_paint.page_descriptor(run.page);
                            unsafe {
                                self.context.device.cmd_bind_pipeline(
                                    cmd,
                                    vk::PipelineBindPoint::GRAPHICS,
                                    self.icon_paint.pipeline(),
                                );
                                self.set_viewport(cmd);
                                self.set_scissor(cmd, run.scissor, full);
                                self.context.device.cmd_bind_descriptor_sets(
                                    cmd,
                                    vk::PipelineBindPoint::GRAPHICS,
                                    self.icon_paint.pipeline_layout(),
                                    0,
                                    &[self.descriptor_set, page],
                                    &[],
                                );
                                self.context.device.cmd_bind_vertex_buffers(
                                    cmd,
                                    0,
                                    &[self.quad_vbo.buffer, self.icon_paint.instance_buffer()],
                                    &[0, 0],
                                );
                                self.context
                                    .device
                                    .cmd_draw(cmd, 4, run.count, 0, run.first);
                            }
                        }
                        IconRunKind::Tess => unsafe {
                            self.context.device.cmd_bind_pipeline(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                self.icon_paint.tess_pipeline(run.material),
                            );
                            self.set_viewport(cmd);
                            self.set_scissor(cmd, run.scissor, full);
                            self.context.device.cmd_bind_descriptor_sets(
                                cmd,
                                vk::PipelineBindPoint::GRAPHICS,
                                self.icon_paint.tess_pipeline_layout(),
                                0,
                                &[
                                    self.descriptor_set,
                                    self.icon_paint.gradient_descriptor_set(),
                                ],
                                &[],
                            );
                            self.context.device.cmd_bind_vertex_buffers(
                                cmd,
                                0,
                                &[self.icon_paint.tess_vertex_buffer()],
                                &[0],
                            );
                            self.context
                                .device
                                .cmd_draw(cmd, run.count, 1, run.first, 0);
                        },
                    }
                }
                PaintItem::Scene3D(index) => {
                    // The scene already rendered + resolved offscreen ahead of
                    // this scope; composite the resolved texture in via the
                    // stock surface pipeline, like an AppTexture.
                    let run = self.scene_paint.run(index);
                    let pipeline = self.scene_paint.composite_pipeline();
                    let layout = self.scene_paint.composite_pipeline_layout();
                    let texture_set = self.scene_paint.composite_descriptor(run);
                    unsafe {
                        self.context.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipeline,
                        );
                        self.set_viewport(cmd);
                        self.set_scissor(cmd, run.scissor, full);
                        self.context.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            layout,
                            0,
                            &[self.descriptor_set, texture_set],
                            &[],
                        );
                        self.context.device.cmd_bind_vertex_buffers(
                            cmd,
                            0,
                            &[
                                self.quad_vbo.buffer,
                                self.scene_paint.composite_instance_buffer(),
                            ],
                            &[0, 0],
                        );
                        self.context
                            .device
                            .cmd_draw(cmd, 4, 1, 0, run.composite_instance);
                    }
                }
                PaintItem::BackdropSnapshot => {
                    return Err(Error::Unsupported(
                        "damascene-ash backdrop snapshot rendering is not implemented yet",
                    ));
                }
                PaintItem::AppTexture(index) => {
                    let run = self.surface_paint.run(index);
                    let pipeline = self.surface_paint.pipeline_for(run.alpha);
                    let layout = self.surface_paint.pipeline_layout();
                    let texture_set = self.surface_paint.descriptor_for_run(run);
                    unsafe {
                        self.context.device.cmd_bind_pipeline(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            pipeline,
                        );
                        self.set_viewport(cmd);
                        self.set_scissor(cmd, run.scissor, full);
                        self.context.device.cmd_bind_descriptor_sets(
                            cmd,
                            vk::PipelineBindPoint::GRAPHICS,
                            layout,
                            0,
                            &[self.descriptor_set, texture_set],
                            &[],
                        );
                        self.context.device.cmd_bind_vertex_buffers(
                            cmd,
                            0,
                            &[self.quad_vbo.buffer, self.surface_paint.instance_buffer()],
                            &[0, 0],
                        );
                        self.context
                            .device
                            .cmd_draw(cmd, 4, run.count, 0, run.first);
                    }
                }
            }
        }
        Ok(())
    }

    unsafe fn set_viewport(&self, cmd: vk::CommandBuffer) {
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.core.viewport_px.0 as f32,
            height: self.core.viewport_px.1 as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        unsafe {
            self.context.device.cmd_set_viewport(cmd, 0, &[viewport]);
        }
    }

    unsafe fn set_scissor(
        &self,
        cmd: vk::CommandBuffer,
        scissor: Option<PhysicalScissor>,
        full: PhysicalScissor,
    ) {
        let s = scissor.unwrap_or(full);
        let rect = vk::Rect2D {
            offset: vk::Offset2D {
                x: s.x as i32,
                y: s.y as i32,
            },
            extent: vk::Extent2D {
                width: s.w.max(1),
                height: s.h.max(1),
            },
        };
        unsafe {
            self.context.device.cmd_set_scissor(cmd, 0, &[rect]);
        }
    }

    fn register_stock_shaders(&mut self) -> Result<()> {
        self.insert_stock_pipeline(StockShader::RoundedRect, stock_wgsl::ROUNDED_RECT)?;
        self.insert_stock_pipeline(StockShader::Spinner, stock_wgsl::SPINNER)?;
        self.insert_stock_pipeline(StockShader::Skeleton, stock_wgsl::SKELETON)?;
        self.insert_stock_pipeline(
            StockShader::ProgressIndeterminate,
            stock_wgsl::PROGRESS_INDETERMINATE,
        )?;
        Ok(())
    }

    fn insert_stock_pipeline(&mut self, shader: StockShader, wgsl: &str) -> Result<()> {
        let pipeline = build_quad_pipeline(
            &self.context.device,
            self.pipeline_layout,
            self.target,
            shader.name(),
            wgsl,
        );
        self.pipelines
            .insert(ShaderHandle::Stock(shader), pipeline?);
        Ok(())
    }
}

struct PaintRecorder<'a> {
    text: &'a mut TextPaint,
    icons: &'a mut IconPaint,
    images: &'a mut ImagePaint,
    surfaces: &'a mut SurfacePaint,
    scenes: &'a mut Scene3DPaint,
    device: &'a ash::Device,
    allocator: &'a mut Allocator,
}

impl TextRecorder for PaintRecorder<'_> {
    fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        style: &RunStyle,
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
        self.images
            .record(
                self.device,
                self.allocator,
                ImageRecord {
                    rect,
                    scissor,
                    image,
                    tint,
                    radius,
                    range_limit,
                },
            )
            .expect("damascene-ash: failed to record image")
    }

    fn record_icon(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &IconSource,
        color: Color,
        size: f32,
        stroke_width: f32,
        scale_factor: f32,
    ) -> RecordedPaint {
        let range = self
            .icons
            .record(rect, scissor, source, color, stroke_width);
        if !range.is_empty() {
            return RecordedPaint::Icon(range);
        }
        record_icon_text_fallback(self.text, rect, scissor, source, color, size, scale_factor)
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

    fn record_app_texture(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        texture: &damascene_core::surface::AppTexture,
        alpha: SurfaceAlpha,
        transform: damascene_core::affine::Affine2,
        _scale_factor: f32,
    ) -> std::ops::Range<usize> {
        self.surfaces
            .record(self.device, rect, scissor, texture, alpha, transform)
            .expect("damascene-ash: failed to record app texture")
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
            .record(
                self.device,
                self.allocator,
                rect,
                scissor,
                id,
                scene,
                scale_factor,
            )
            .expect("damascene-ash: failed to record scene")
    }
}

fn record_icon_text_fallback(
    text: &mut TextPaint,
    rect: Rect,
    scissor: Option<PhysicalScissor>,
    source: &IconSource,
    color: Color,
    size: f32,
    scale_factor: f32,
) -> RecordedPaint {
    let glyph = match source {
        IconSource::Builtin(name) => name.fallback_glyph(),
        IconSource::Custom(_) => "?",
        IconSource::UnknownName(_) => damascene_core::tree::IconName::AlertCircle.fallback_glyph(),
    };
    RecordedPaint::Text(text.record(
        rect,
        scissor,
        &RunStyle::new(FontWeight::Regular, color),
        glyph,
        size,
        damascene_core::text::metrics::line_height(size),
        TextWrap::NoWrap,
        TextAnchor::Middle,
        scale_factor,
    ))
}

impl Drop for Runner {
    fn drop(&mut self) {
        unsafe {
            for (_, pipeline) in self.pipelines.drain() {
                self.context.device.destroy_pipeline(pipeline, None);
            }
            if self.pipeline_layout != vk::PipelineLayout::null() {
                self.context
                    .device
                    .destroy_pipeline_layout(self.pipeline_layout, None);
            }
            if self.descriptor_pool != vk::DescriptorPool::null() {
                self.context
                    .device
                    .destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if self.descriptor_set_layout != vk::DescriptorSetLayout::null() {
                self.context
                    .device
                    .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            }
            if let Ok(mut allocator) = self.context.allocator.lock() {
                self.scene_paint
                    .destroy(&self.context.device, &mut allocator);
                self.text_paint
                    .destroy(&self.context.device, &mut allocator);
                self.icon_paint
                    .destroy(&self.context.device, &mut allocator);
                self.image_paint
                    .destroy(&self.context.device, &mut allocator);
                self.surface_paint
                    .destroy(&self.context.device, &mut allocator);
                self.instance_buf
                    .destroy(&self.context.device, &mut allocator);
                self.quad_vbo.destroy(&self.context.device, &mut allocator);
                self.frame_buf.destroy(&self.context.device, &mut allocator);
            }
        }
    }
}

fn create_frame_descriptor_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout> {
    let binding = vk::DescriptorSetLayoutBinding::default()
        .binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(1)
        .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT);
    let bindings = [binding];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    unsafe { device.create_descriptor_set_layout(&info, None) }.map_err(|result| Error::Vulkan {
        op: "create_descriptor_set_layout",
        result,
    })
}

fn create_descriptor_pool(device: &ash::Device) -> Result<vk::DescriptorPool> {
    let pool_size = vk::DescriptorPoolSize {
        ty: vk::DescriptorType::UNIFORM_BUFFER,
        descriptor_count: 1,
    };
    let pool_sizes = [pool_size];
    let info = vk::DescriptorPoolCreateInfo::default()
        .max_sets(1)
        .pool_sizes(&pool_sizes);
    unsafe { device.create_descriptor_pool(&info, None) }.map_err(|result| Error::Vulkan {
        op: "create_descriptor_pool",
        result,
    })
}

fn allocate_frame_descriptor_set(
    device: &ash::Device,
    pool: vk::DescriptorPool,
    layout: vk::DescriptorSetLayout,
) -> Result<vk::DescriptorSet> {
    let layouts = [layout];
    let info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(&layouts);
    let sets =
        unsafe { device.allocate_descriptor_sets(&info) }.map_err(|result| Error::Vulkan {
            op: "allocate_descriptor_sets",
            result,
        })?;
    sets.into_iter().next().ok_or(Error::Unsupported(
        "descriptor set allocation returned no sets",
    ))
}

fn update_frame_descriptor_set(
    device: &ash::Device,
    descriptor_set: vk::DescriptorSet,
    frame_buffer: vk::Buffer,
) {
    let buffer_info = vk::DescriptorBufferInfo {
        buffer: frame_buffer,
        offset: 0,
        range: std::mem::size_of::<FrameUniforms>() as vk::DeviceSize,
    };
    let buffer_infos = [buffer_info];
    let write = vk::WriteDescriptorSet::default()
        .dst_set(descriptor_set)
        .dst_binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
        .buffer_info(&buffer_infos);
    unsafe {
        device.update_descriptor_sets(&[write], &[]);
    }
}

fn create_pipeline_layout(
    device: &ash::Device,
    frame_layout: vk::DescriptorSetLayout,
) -> Result<vk::PipelineLayout> {
    let set_layouts = [frame_layout];
    let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts);
    unsafe { device.create_pipeline_layout(&info, None) }.map_err(|result| Error::Vulkan {
        op: "create_pipeline_layout",
        result,
    })
}

unsafe fn transition_color_target(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    if old_layout == new_layout {
        return;
    }

    let (src_stage, src_access) = match old_layout {
        vk::ImageLayout::UNDEFINED => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::PRESENT_SRC_KHR => (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        _ => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
    };
    let (dst_stage, dst_access) = match new_layout {
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::PRESENT_SRC_KHR => (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        _ => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
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
