//! GPU rendering for [`DrawOp::Scene3D`](damascene_core::ir::DrawOp::Scene3D).
//!
//! Unlike every other paint item, a 3D scene is a *two-phase* draw. WebGPU
//! render passes can't nest, so the scene can't render mid-composite:
//!
//! 1. **Offscreen pre-pass** ([`Scene3DPaint::encode_offscreen`]). Before
//!    the main composite pass begins, each recorded scene renders into its
//!    own offscreen target — an `Rgba16Float` colour attachment with a
//!    depth buffer, multisampled at the scene's `msaa_samples` and resolved
//!    to a single-sample texture. fp16 + linear keeps the scene in the
//!    runner's working colour space with headroom, so HDR turns on with the
//!    swapchain and there's no banding on smooth 3D gradients. This mirrors
//!    the [`BackdropSnapshot`](damascene_core::paint::PaintItem::BackdropSnapshot)
//!    encoding discipline: offscreen work is encoded on the command encoder
//!    first, ahead of the pass that consumes it.
//! 2. **Composite** (the `PaintItem::Scene3D` arm in the render loop). The
//!    resolved texture composites into the main pass exactly like an
//!    [`AppTexture`](crate::surface) — same stock `surface` shader, same
//!    premultiplied blend, same logical-rect instance. The scene shaders
//!    already output premultiplied alpha, so it drops straight in.
//!
//! Geometry follows the versioned-handle contract: vertex/index/instance
//! buffers are cached by [`GeometryId`] and only re-uploaded when the
//! handle's revision advances (or, for the colour-carrying point/line
//! buffers, when the working colour space changes). Per-node offscreen
//! targets are cached by the node's stable id and dropped when a frame
//! doesn't touch them — the same one-frame-eviction policy as
//! [`SurfacePaint`](crate::surface::SurfacePaint).

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use damascene_core::color::ColorSpace;
use damascene_core::paint::{PhysicalScissor, rgba_f32_in};
use damascene_core::scene::gpu::{
    self, CompositeInstance, LineInstance, LineUniform, MeshUniform, MeshVertexGpu, PointInstance,
    PointUniform,
};
use damascene_core::scene::{
    LineDraw, MeshDraw, PointDraw, ResolvedCamera, Scene3DData, SceneDepthMap,
};
use damascene_core::shader::stock_wgsl;
use damascene_core::tree::Rect;

use wgpu::util::DeviceExt;

/// Linear fp16 — the working colour space with HDR headroom. See module docs.
const SCENE_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const SCENE_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Single-channel float target the MSAA depth resolves into for label
/// occlusion read-back. Stores normalised device depth in `[0, 1]`.
const SCENE_OCC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Occlusion target on backends without depth read-back (WebGL2): depth is
/// re-rendered as 24-bit fixed point packed into RGB. `Rgba8Unorm` is the
/// one format whose render + `copy_texture_to_buffer` path is guaranteed
/// everywhere GLES runs (readPixels RGBA/UNSIGNED_BYTE is core).
const SCENE_OCC_PACKED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Depth buffer for the packed depth-as-color pass (single-sample,
/// independent of the scene's MSAA count).
const SCENE_OCC_PACKED_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// `copy_texture_to_buffer` requires each row to be a multiple of this.
const COPY_ROW_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
/// Uniform dynamic-offset stride. 256 is the common
/// `min_uniform_buffer_offset_alignment` ceiling; every per-draw uniform
/// struct here is smaller, so each lands in its own 256-byte slot.
const UNIFORM_STRIDE: u64 = 256;

// ---- Vertex attribute tables ----

const POINT_QUAD_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2]; // corner, uv
const POINT_INSTANCE_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![2 => Float32x3, 3 => Float32x4]; // position, color
const LINE_QUAD_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
const LINE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![1 => Float32x3, 2 => Float32x3, 3 => Float32x4, 4 => Float32];
const MESH_VERTEX_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
const COMPOSITE_INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4, 3 => Float32x2];

// ---- Cached geometry ----

struct MeshBuffers {
    vbuf: wgpu::Buffer,
    ibuf: Option<wgpu::Buffer>,
    vcount: u32,
    icount: u32,
}
struct PointBuffers {
    ibuf: wgpu::Buffer,
    count: u32,
}
struct LineBuffers {
    ibuf: wgpu::Buffer,
    count: u32,
}

enum GeoBuffers {
    Mesh(MeshBuffers),
    Points(PointBuffers),
    Lines(LineBuffers),
}

struct CachedGeometry {
    buffers: GeoBuffers,
    /// Handle revision the buffers were built from.
    revision: u64,
    /// Working colour space the colours were converted into (point/line
    /// only; meshes carry colour in the per-draw uniform, so this is unused
    /// for them but harmless).
    space: ColorSpace,
    used_frame: u64,
}

// ---- Per-node offscreen target ----

struct OffscreenTarget {
    size: (u32, u32),
    sample_count: u32,
    /// Multisampled colour attachment; `None` when `sample_count == 1`
    /// (we then render straight into `resolve`).
    msaa_color: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    resolve_view: wgpu::TextureView,
    /// Stock-`surface` group(1) bind group over the resolved texture.
    composite_bind_group: wgpu::BindGroup,
    /// Depth-readback resources, allocated the first frame this target's
    /// scene asks for label occlusion. `None` for label-free scenes.
    occlusion: Option<OcclusionResources>,
    used_frame: u64,
}

// ---- Label-occlusion depth read-back ----

/// Per-target GPU + CPU resources for capturing the scene depth buffer and
/// streaming it back to the CPU. The MSAA depth is resolved into
/// [`SCENE_OCC_FORMAT`] (sample 0), copied to `readback`, then mapped a
/// frame later — see the [`SceneDepthMap`] docs for the latency contract.
struct OcclusionResources {
    color_view: wgpu::TextureView,
    color: wgpu::Texture,
    readback: wgpu::Buffer,
    /// Padded bytes-per-row of `readback` (256-aligned for the T2B copy).
    padded_bytes_per_row: u32,
    width: u32,
    height: u32,
    /// `true` when `color` is [`SCENE_OCC_PACKED_FORMAT`] written by the
    /// depth-as-color pass (`depth_readback == false`); `false` when it is
    /// [`SCENE_OCC_FORMAT`] written by the depth resolve. Selects the
    /// CPU-side decode in [`Scene3DPaint::collect_depth_maps`].
    packed: bool,
    /// Single-sample depth buffer for the packed depth-as-color pass.
    /// `None` in resolve mode (the pass has no depth attachment).
    pack_depth: Option<wgpu::TextureView>,
    state: ReadbackState,
    /// The (camera, rect) of the most recent capture. A fresh capture only
    /// fires when the pose changes — so a settled scene with a current map
    /// stops capturing and the renderer can go idle.
    last_captured: Option<(ResolvedCamera, Rect)>,
}

/// Lifecycle of one target's single read-back buffer. Only one capture is
/// in flight at a time; the buffer returns to `Free` after it is read,
/// so a busy buffer simply reuses the previous (slightly staler) map.
enum ReadbackState {
    /// Buffer idle — eligible to receive a fresh depth copy this frame.
    Free,
    /// A copy was encoded; map it after the host submits (next frame).
    Pending { camera: ResolvedCamera, rect: Rect },
    /// Map requested; `done` flips when the mapping callback fires.
    Mapping {
        camera: ResolvedCamera,
        rect: Rect,
        done: Arc<AtomicBool>,
    },
}

/// Depth-resolve pipeline (fullscreen triangle sampling the MSAA depth),
/// built per MSAA sample count since the depth binding type differs.
struct ResolvePipeline {
    pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
}

// ---- Per-draw command + per-scene run ----

enum DrawCmd {
    Mesh {
        geo: u64,
        uniform_slot: u32,
        /// Material alpha < 1: render through the translucent path — depth
        /// test without write, two passes (back faces then front faces).
        /// Recorded after all opaque meshes, back-to-front (see
        /// [`gpu::mesh_draw_order`]).
        translucent: bool,
    },
    Points {
        geo: u64,
        uniform_slot: u32,
    },
    Lines {
        geo: u64,
        uniform_slot: u32,
    },
    /// Reference grid + axes, generated per frame into `grid_buf` rather
    /// than cached by GeometryId (the geometry follows `SceneStyle`, not an
    /// app handle). `first..first+count` indexes the shared grid buffer.
    Grid {
        uniform_slot: u32,
        first: u32,
        count: u32,
    },
}

pub(crate) struct Scene3DRun {
    target_id: String,
    pub scissor: Option<PhysicalScissor>,
    sample_count: u32,
    clear: wgpu::Color,
    cmds: Vec<DrawCmd>,
    /// Index into the composite instance buffer.
    pub composite_instance: u32,
    /// Capture this scene's depth for label occlusion (drives `StoreOp` on
    /// the depth attachment and the resolve/read-back pass).
    capture_depth: bool,
    /// Resolved camera + logical rect at record time — stamped into the
    /// captured [`SceneDepthMap`] so occlusion is judged in capture space.
    camera: ResolvedCamera,
    rect: Rect,
}

/// One scene-shader pipeline set, built per MSAA sample count seen.
struct ScenePipelines {
    point: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    /// Opaque meshes: depth write, back-face cull.
    mesh: wgpu::RenderPipeline,
    /// Translucent mesh passes: depth-test only (no write), drawn after the
    /// opaque set back-to-front, each mesh in two passes — back faces
    /// (`mesh_back`, cull front) then front faces (`mesh_front`, cull back)
    /// — so a closed shell blends inside-then-outside correctly.
    mesh_back: wgpu::RenderPipeline,
    mesh_front: wgpu::RenderPipeline,
}

pub(crate) struct Scene3DPaint {
    working: ColorSpace,
    /// Whether the backend can read the scene depth *attachment* for label
    /// occlusion. False on WebGL2: naga's GLSL target can't `textureLoad`
    /// a depth texture, and GLSL ES 3.0 can't bind (or even create)
    /// multisampled depth textures. When false, the capture instead
    /// re-renders the scene's meshes — the only depth-writing geometry —
    /// through [`Self::depth_pack_pipeline`], packing fragment depth into
    /// an `Rgba8Unorm` target the read-back path can copy everywhere.
    depth_readback: bool,

    // Static vertex data for billboard / line-quad expansion.
    point_quad_vbo: wgpu::Buffer,
    line_quad_vbo: wgpu::Buffer,

    // Scene-shader resources.
    uniform_layout: wgpu::BindGroupLayout,
    point_shader: wgpu::ShaderModule,
    line_shader: wgpu::ShaderModule,
    mesh_shader: wgpu::ShaderModule,
    scene_pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<u32, ScenePipelines>,
    /// Depth-resolve pipelines for label occlusion, keyed by sample count.
    /// Built lazily by [`Self::encode_depth_capture`] the first time a
    /// capture actually fires — the shader doesn't translate to GLSL, so
    /// it must never be built on backends without `depth_readback` (and
    /// label-free scenes never pay for it anywhere).
    resolve_pipelines: HashMap<u32, ResolvePipeline>,
    /// The `depth_readback == false` capture pipeline: re-renders meshes
    /// with the stock mesh vertex shader and a fragment stage that packs
    /// `frag.z` into [`SCENE_OCC_PACKED_FORMAT`]. Built lazily on first
    /// capture; `None` on backends with real depth read-back.
    depth_pack_pipeline: Option<wgpu::RenderPipeline>,

    // Dynamic-offset uniform buffers + their (rebuilt-on-grow) bind groups.
    point_uniforms: Vec<PointUniform>,
    line_uniforms: Vec<LineUniform>,
    mesh_uniforms: Vec<MeshUniform>,
    point_ubo: wgpu::Buffer,
    line_ubo: wgpu::Buffer,
    mesh_ubo: wgpu::Buffer,
    point_uniform_cap: usize,
    line_uniform_cap: usize,
    mesh_uniform_cap: usize,
    point_bind_group: wgpu::BindGroup,
    line_bind_group: wgpu::BindGroup,
    mesh_bind_group: wgpu::BindGroup,

    // Composite (resolved texture → main pass), reusing the stock surface shader.
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_layout: wgpu::BindGroupLayout,
    /// Pipeline layout + sample count retained so `composite_pipeline` — the
    /// *only* scene pipeline bound to the swapchain target format — can be
    /// rebuilt in place on a surface-format renegotiation
    /// (`set_target_format`). The offscreen scene pipelines (point/line/mesh)
    /// render into [`SCENE_COLOR_FORMAT`] and the occlusion pipelines into
    /// [`SCENE_OCC_FORMAT`] / [`SCENE_OCC_PACKED_FORMAT`]; none of those
    /// formats track the swapchain, so they are deliberately left untouched.
    composite_pipeline_layout: wgpu::PipelineLayout,
    composite_sample_count: u32,
    sampler: wgpu::Sampler,
    composite_instances: Vec<CompositeInstance>,
    composite_instance_buf: wgpu::Buffer,
    composite_instance_cap: usize,

    // Per-frame reference grid + axes (style-derived, not handle-cached).
    grid_instances: Vec<LineInstance>,
    grid_buf: wgpu::Buffer,
    grid_cap: usize,

    // Caches.
    geometry: HashMap<u64, CachedGeometry>,
    targets: HashMap<String, OffscreenTarget>,

    runs: Vec<Scene3DRun>,
    frame_counter: u64,
}

const INITIAL_UNIFORM_CAP: usize = 16;
const INITIAL_COMPOSITE_CAP: usize = 8;
const INITIAL_GRID_CAP: usize = 256;

impl Scene3DPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
        working: ColorSpace,
        depth_readback: bool,
    ) -> Self {
        // Billboard quad: corner (-1..1) + uv (0..1), triangle strip.
        let point_quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("damascene_wgpu::scene::point_quad"),
            contents: bytemuck::cast_slice::<f32, u8>(&[
                -1.0, -1.0, 0.0, 0.0, // bl
                1.0, -1.0, 1.0, 0.0, // br
                -1.0, 1.0, 0.0, 1.0, // tl
                1.0, 1.0, 1.0, 1.0, // tr
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        // Line quad: corner.x (0=start,1=end), corner.y (-1/+1 side).
        let line_quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("damascene_wgpu::scene::line_quad"),
            contents: bytemuck::cast_slice::<f32, u8>(&[
                0.0, -1.0, // start left
                1.0, -1.0, // end left
                0.0, 1.0, // start right
                1.0, 1.0, // end right
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("damascene_wgpu::scene::uniform_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let point_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene::point"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SCENE_POINT)),
        });
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene::line"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SCENE_LINE)),
        });
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene::mesh"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SCENE_MESH)),
        });

        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("damascene_wgpu::scene::pipeline_layout"),
                bind_group_layouts: &[Some(&uniform_layout)],
                immediate_size: 0,
            });

        let (point_ubo, point_bind_group) =
            make_uniform_buffer(device, &uniform_layout, INITIAL_UNIFORM_CAP, "point");
        let (line_ubo, line_bind_group) =
            make_uniform_buffer(device, &uniform_layout, INITIAL_UNIFORM_CAP, "line");
        let (mesh_ubo, mesh_bind_group) =
            make_uniform_buffer(device, &uniform_layout, INITIAL_UNIFORM_CAP, "mesh");

        // Composite: reuse the stock surface shader + premultiplied blend.
        let composite_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("damascene_wgpu::scene::composite_tex_layout"),
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
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("damascene_wgpu::scene::composite_pipeline_layout"),
                bind_group_layouts: &[Some(frame_bind_layout), Some(&composite_bind_layout)],
                immediate_size: 0,
            });
        let composite_pipeline = build_composite_pipeline(
            device,
            &composite_pipeline_layout,
            target_format,
            sample_count,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("damascene_wgpu::scene::sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let composite_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::scene::composite_instances"),
            size: (INITIAL_COMPOSITE_CAP * std::mem::size_of::<CompositeInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let grid_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("damascene_wgpu::scene::grid_lines"),
            size: (INITIAL_GRID_CAP * std::mem::size_of::<LineInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            working,
            depth_readback,
            point_quad_vbo,
            line_quad_vbo,
            uniform_layout,
            point_shader,
            line_shader,
            mesh_shader,
            scene_pipeline_layout,
            pipelines: HashMap::new(),
            resolve_pipelines: HashMap::new(),
            depth_pack_pipeline: None,
            point_uniforms: Vec::new(),
            line_uniforms: Vec::new(),
            mesh_uniforms: Vec::new(),
            point_ubo,
            line_ubo,
            mesh_ubo,
            point_uniform_cap: INITIAL_UNIFORM_CAP,
            line_uniform_cap: INITIAL_UNIFORM_CAP,
            mesh_uniform_cap: INITIAL_UNIFORM_CAP,
            point_bind_group,
            line_bind_group,
            mesh_bind_group,
            composite_pipeline,
            composite_bind_layout,
            composite_pipeline_layout,
            composite_sample_count: sample_count,
            sampler,
            composite_instances: Vec::new(),
            composite_instance_buf,
            composite_instance_cap: INITIAL_COMPOSITE_CAP,
            grid_instances: Vec::new(),
            grid_buf,
            grid_cap: INITIAL_GRID_CAP,
            geometry: HashMap::new(),
            targets: HashMap::new(),
            runs: Vec::new(),
            frame_counter: 0,
        }
    }

    pub(crate) fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working = space;
    }

    /// Rebuild the composite pipeline for a new target format. This is the
    /// *only* scene pipeline that tracks the swapchain format: the offscreen
    /// point/line/mesh pipelines render into [`SCENE_COLOR_FORMAT`]
    /// (`Rgba16Float`) and the occlusion pipelines into
    /// [`SCENE_OCC_FORMAT`] / [`SCENE_OCC_PACKED_FORMAT`], all independent of
    /// the swapchain, so they are deliberately left alone. Per-node offscreen
    /// targets, geometry caches, uniform/instance buffers, and the composite
    /// bind groups (over the resolved offscreen textures) all survive. Called
    /// by `Runner::set_target_format`.
    pub(crate) fn set_target_format(
        &mut self,
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) {
        self.composite_pipeline = build_composite_pipeline(
            device,
            &self.composite_pipeline_layout,
            target_format,
            self.composite_sample_count,
        );
    }

    pub(crate) fn frame_begin(&mut self) {
        self.runs.clear();
        self.point_uniforms.clear();
        self.line_uniforms.clear();
        self.mesh_uniforms.clear();
        self.composite_instances.clear();
        self.grid_instances.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    /// Record one scene. Uploads any geometry whose revision (or, for
    /// colour buffers, working space) changed, ensures the offscreen
    /// target, and queues the draw commands. Returns the one-element range
    /// the prepare loop turns into a `PaintItem::Scene3D`.
    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        id: &str,
        scene: &Scene3DData,
        scale_factor: f32,
    ) -> Range<usize> {
        let start = self.runs.len();
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return start..start;
        }
        let px = (
            (rect.w * scale_factor).round().max(1.0) as u32,
            (rect.h * scale_factor).round().max(1.0) as u32,
        );
        let sample_count = scene.style.msaa_samples.max(1);
        self.ensure_pipelines(device, sample_count);
        self.ensure_target(device, id, px, sample_count, scene.capture_depth);

        let aspect = px.0 as f32 / px.1 as f32;
        let view_proj = scene.camera.view_proj(aspect);
        let screen = [px.0 as f32, px.1 as f32];
        let working = self.working;

        let mut cmds = Vec::new();

        // Opaque meshes first (spec order), then translucent meshes
        // back-to-front (their no-depth-write blending is order-dependent).
        // The translucent set is held back so the grid slots in between.
        let mut translucent_cmds = Vec::new();
        for (i, translucent) in gpu::mesh_draw_order(scene) {
            let m = &scene.meshes[i];
            self.ensure_mesh_geometry(device, m);
            let slot = self.push_mesh_uniform(gpu::mesh_uniform(view_proj, m, scene, working));
            let cmd = DrawCmd::Mesh {
                geo: m.geometry.id().0,
                uniform_slot: slot,
                translucent,
            };
            if translucent {
                translucent_cmds.push(cmd);
            } else {
                cmds.push(cmd);
            }
        }

        // Reference grid + axes after the opaque meshes: the line pipeline
        // depth-tests (no write), so strokes hide behind solid geometry and
        // show in front of it — drawing them first would let a later mesh
        // paint over a nearer stroke (no depth was written to reject it).
        // Before the translucent meshes and the data marks, so both still
        // read on top of the reference layer.
        let first = self.grid_instances.len() as u32;
        gpu::build_grid_lines(&scene.style, working, &mut self.grid_instances);
        let count = self.grid_instances.len() as u32 - first;
        if count > 0 {
            let slot = self.push_line_uniform(gpu::grid_uniform(view_proj, screen));
            cmds.push(DrawCmd::Grid {
                uniform_slot: slot,
                first,
                count,
            });
        }
        cmds.append(&mut translucent_cmds);
        for p in &scene.points {
            self.ensure_point_geometry(device, p, working);
            let slot =
                self.push_point_uniform(gpu::point_uniform(view_proj * p.transform, screen, p));
            cmds.push(DrawCmd::Points {
                geo: p.geometry.id().0,
                uniform_slot: slot,
            });
        }
        for l in &scene.lines {
            self.ensure_line_geometry(device, l, working);
            let slot =
                self.push_line_uniform(gpu::line_uniform(view_proj * l.transform, screen, l));
            cmds.push(DrawCmd::Lines {
                geo: l.geometry.id().0,
                uniform_slot: slot,
            });
        }

        let clear = match scene.style.background {
            Some(c) => {
                let [r, g, b, a] = rgba_f32_in(c, working);
                // Premultiplied, to match the premultiplied scene output the
                // composite blends.
                wgpu::Color {
                    r: (r * a) as f64,
                    g: (g * a) as f64,
                    b: (b * a) as f64,
                    a: a as f64,
                }
            }
            None => wgpu::Color::TRANSPARENT,
        };

        let composite_instance = self.composite_instances.len() as u32;
        self.composite_instances.push(CompositeInstance::new(
            [rect.x, rect.y, rect.w, rect.h],
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0],
        ));

        self.runs.push(Scene3DRun {
            target_id: id.to_string(),
            scissor,
            sample_count,
            clear,
            cmds,
            composite_instance,
            capture_depth: scene.capture_depth,
            camera: scene.camera,
            rect,
        });
        start..self.runs.len()
    }

    fn push_point_uniform(&mut self, u: PointUniform) -> u32 {
        let slot = self.point_uniforms.len() as u32;
        self.point_uniforms.push(u);
        slot
    }
    fn push_line_uniform(&mut self, u: LineUniform) -> u32 {
        let slot = self.line_uniforms.len() as u32;
        self.line_uniforms.push(u);
        slot
    }
    fn push_mesh_uniform(&mut self, u: MeshUniform) -> u32 {
        let slot = self.mesh_uniforms.len() as u32;
        self.mesh_uniforms.push(u);
        slot
    }

    fn ensure_pipelines(&mut self, device: &wgpu::Device, sample_count: u32) {
        if self.pipelines.contains_key(&sample_count) {
            return;
        }
        let pipelines = build_scene_pipelines(
            device,
            &self.scene_pipeline_layout,
            &self.point_shader,
            &self.line_shader,
            &self.mesh_shader,
            sample_count,
        );
        self.pipelines.insert(sample_count, pipelines);
    }

    fn ensure_target(
        &mut self,
        device: &wgpu::Device,
        id: &str,
        px: (u32, u32),
        sample_count: u32,
        capture_depth: bool,
    ) {
        if let Some(t) = self.targets.get_mut(id)
            && t.size == px
            && t.sample_count == sample_count
        {
            t.used_frame = self.frame_counter;
            // Allocate occlusion resources lazily if the scene only now
            // started asking for label occlusion.
            if capture_depth && t.occlusion.is_none() {
                t.occlusion = Some(build_occlusion_resources(device, px, !self.depth_readback));
            }
            return;
        }
        let extent = wgpu::Extent3d {
            width: px.0,
            height: px.1,
            depth_or_array_layers: 1,
        };
        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("damascene_wgpu::scene::resolve"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let resolve_view = resolve.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa_color = (sample_count > 1).then(|| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("damascene_wgpu::scene::msaa_color"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format: SCENE_COLOR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        });
        // TEXTURE_BINDING so the resolve pass can sample it for the
        // label-occlusion depth read-back — but only where that pass can
        // exist. On GL, render-attachment-only textures become
        // renderbuffers (which WebGL2 *can* multisample); TEXTURE_BINDING
        // forces a real texture, and multisampled textures don't exist in
        // GLES 3.0 at all.
        let depth_usage = if self.depth_readback {
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
        } else {
            wgpu::TextureUsages::RENDER_ATTACHMENT
        };
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("damascene_wgpu::scene::depth"),
                size: extent,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_DEPTH_FORMAT,
                usage: depth_usage,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("damascene_wgpu::scene::composite_bind_group"),
            layout: &self.composite_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&resolve_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.targets.insert(
            id.to_string(),
            OffscreenTarget {
                size: px,
                sample_count,
                msaa_color,
                depth,
                resolve_view,
                composite_bind_group,
                occlusion: capture_depth
                    .then(|| build_occlusion_resources(device, px, !self.depth_readback)),
                used_frame: self.frame_counter,
            },
        );
    }

    fn ensure_mesh_geometry(&mut self, device: &wgpu::Device, draw: &MeshDraw) {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && matches!(c.buffers, GeoBuffers::Mesh(_))
        {
            c.used_frame = self.frame_counter;
            return;
        }
        let verts = gpu::mesh_vertices(&data);
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("damascene_wgpu::scene::mesh_vbuf"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let (ibuf, icount) = match &data.indices {
            Some(indices) => (
                Some(
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("damascene_wgpu::scene::mesh_ibuf"),
                        contents: bytemuck::cast_slice(indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                ),
                indices.len() as u32,
            ),
            None => (None, 0),
        };
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers: GeoBuffers::Mesh(MeshBuffers {
                    vbuf,
                    ibuf,
                    vcount: verts.len() as u32,
                    icount,
                }),
                revision: rev,
                space: self.working,
                used_frame: self.frame_counter,
            },
        );
    }

    fn ensure_point_geometry(
        &mut self,
        device: &wgpu::Device,
        draw: &PointDraw,
        working: ColorSpace,
    ) {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && c.space == working
            && matches!(c.buffers, GeoBuffers::Points(_))
        {
            c.used_frame = self.frame_counter;
            return;
        }
        let instances = gpu::point_instances(&data, working);
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("damascene_wgpu::scene::point_ibuf"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers: GeoBuffers::Points(PointBuffers {
                    ibuf,
                    count: instances.len() as u32,
                }),
                revision: rev,
                space: working,
                used_frame: self.frame_counter,
            },
        );
    }

    fn ensure_line_geometry(
        &mut self,
        device: &wgpu::Device,
        draw: &LineDraw,
        working: ColorSpace,
    ) {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && c.space == working
            && matches!(c.buffers, GeoBuffers::Lines(_))
        {
            c.used_frame = self.frame_counter;
            return;
        }
        let instances = gpu::line_instances(&data, working);
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("damascene_wgpu::scene::line_ibuf"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers: GeoBuffers::Lines(LineBuffers {
                    ibuf,
                    count: instances.len() as u32,
                }),
                revision: rev,
                space: working,
                used_frame: self.frame_counter,
            },
        );
    }

    /// Write uniform/instance buffers and drop cache entries untouched this
    /// frame. Mirrors [`SurfacePaint::flush`](crate::surface).
    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let frame = self.frame_counter;
        self.geometry.retain(|_, c| c.used_frame == frame);
        self.targets.retain(|_, t| t.used_frame == frame);

        // Dynamic-offset uniform buffers: grow + rebuild bind group on
        // overflow, then write each element into its 256-byte slot.
        if self.point_uniforms.len() > self.point_uniform_cap {
            let cap = self.point_uniforms.len().next_power_of_two();
            let (buf, bg) = make_uniform_buffer(device, &self.uniform_layout, cap, "point");
            self.point_ubo = buf;
            self.point_bind_group = bg;
            self.point_uniform_cap = cap;
        }
        if self.line_uniforms.len() > self.line_uniform_cap {
            let cap = self.line_uniforms.len().next_power_of_two();
            let (buf, bg) = make_uniform_buffer(device, &self.uniform_layout, cap, "line");
            self.line_ubo = buf;
            self.line_bind_group = bg;
            self.line_uniform_cap = cap;
        }
        if self.mesh_uniforms.len() > self.mesh_uniform_cap {
            let cap = self.mesh_uniforms.len().next_power_of_two();
            let (buf, bg) = make_uniform_buffer(device, &self.uniform_layout, cap, "mesh");
            self.mesh_ubo = buf;
            self.mesh_bind_group = bg;
            self.mesh_uniform_cap = cap;
        }
        for (i, u) in self.point_uniforms.iter().enumerate() {
            queue.write_buffer(
                &self.point_ubo,
                i as u64 * UNIFORM_STRIDE,
                bytemuck::bytes_of(u),
            );
        }
        for (i, u) in self.line_uniforms.iter().enumerate() {
            queue.write_buffer(
                &self.line_ubo,
                i as u64 * UNIFORM_STRIDE,
                bytemuck::bytes_of(u),
            );
        }
        for (i, u) in self.mesh_uniforms.iter().enumerate() {
            queue.write_buffer(
                &self.mesh_ubo,
                i as u64 * UNIFORM_STRIDE,
                bytemuck::bytes_of(u),
            );
        }

        if self.composite_instances.len() > self.composite_instance_cap {
            let cap = self.composite_instances.len().next_power_of_two();
            self.composite_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::scene::composite_instances (resized)"),
                size: (cap * std::mem::size_of::<CompositeInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.composite_instance_cap = cap;
        }
        if !self.composite_instances.is_empty() {
            queue.write_buffer(
                &self.composite_instance_buf,
                0,
                bytemuck::cast_slice(&self.composite_instances),
            );
        }

        if self.grid_instances.len() > self.grid_cap {
            let cap = self.grid_instances.len().next_power_of_two();
            self.grid_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("damascene_wgpu::scene::grid_lines (resized)"),
                size: (cap * std::mem::size_of::<LineInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.grid_cap = cap;
        }
        if !self.grid_instances.is_empty() {
            queue.write_buffer(
                &self.grid_buf,
                0,
                bytemuck::cast_slice(&self.grid_instances),
            );
        }
    }

    /// Encode each recorded scene's offscreen pass onto `encoder`. Must run
    /// before the main composite pass begins (passes can't nest).
    pub(crate) fn encode_offscreen(&self, encoder: &mut wgpu::CommandEncoder) {
        for run in &self.runs {
            let Some(target) = self.targets.get(&run.target_id) else {
                continue;
            };
            let pipelines = self
                .pipelines
                .get(&run.sample_count)
                .expect("pipelines ensured at record time");

            let (view, resolve_target) = match &target.msaa_color {
                Some(ms) => (ms, Some(&target.resolve_view)),
                None => (&target.resolve_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("damascene_wgpu::scene::offscreen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(run.clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        // Keep depth when this scene's labels need occlusion,
                        // so the resolve pass can read it back. The packed
                        // capture path (`!depth_readback`) re-renders meshes
                        // instead of reading this attachment, so it always
                        // discards.
                        store: if run.capture_depth && self.depth_readback {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            for cmd in &run.cmds {
                match *cmd {
                    DrawCmd::Mesh {
                        geo,
                        uniform_slot,
                        translucent,
                    } => {
                        let Some(CachedGeometry {
                            buffers: GeoBuffers::Mesh(m),
                            ..
                        }) = self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        pass.set_bind_group(
                            0,
                            &self.mesh_bind_group,
                            &[uniform_slot * UNIFORM_STRIDE as u32],
                        );
                        pass.set_vertex_buffer(0, m.vbuf.slice(..));
                        let draw = |pass: &mut wgpu::RenderPass| match &m.ibuf {
                            Some(ibuf) => {
                                pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                                pass.draw_indexed(0..m.icount, 0, 0..1);
                            }
                            None => pass.draw(0..m.vcount, 0..1),
                        };
                        if translucent {
                            // Far wall first, then near wall, so a closed
                            // shell blends back-to-front within itself too.
                            pass.set_pipeline(&pipelines.mesh_back);
                            draw(&mut pass);
                            pass.set_pipeline(&pipelines.mesh_front);
                            draw(&mut pass);
                        } else {
                            pass.set_pipeline(&pipelines.mesh);
                            draw(&mut pass);
                        }
                    }
                    DrawCmd::Points { geo, uniform_slot } => {
                        let Some(CachedGeometry {
                            buffers: GeoBuffers::Points(p),
                            ..
                        }) = self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        pass.set_pipeline(&pipelines.point);
                        pass.set_bind_group(
                            0,
                            &self.point_bind_group,
                            &[uniform_slot * UNIFORM_STRIDE as u32],
                        );
                        pass.set_vertex_buffer(0, self.point_quad_vbo.slice(..));
                        pass.set_vertex_buffer(1, p.ibuf.slice(..));
                        pass.draw(0..4, 0..p.count);
                    }
                    DrawCmd::Lines { geo, uniform_slot } => {
                        let Some(CachedGeometry {
                            buffers: GeoBuffers::Lines(l),
                            ..
                        }) = self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        pass.set_pipeline(&pipelines.line);
                        pass.set_bind_group(
                            0,
                            &self.line_bind_group,
                            &[uniform_slot * UNIFORM_STRIDE as u32],
                        );
                        pass.set_vertex_buffer(0, self.line_quad_vbo.slice(..));
                        pass.set_vertex_buffer(1, l.ibuf.slice(..));
                        pass.draw(0..4, 0..l.count);
                    }
                    DrawCmd::Grid {
                        uniform_slot,
                        first,
                        count,
                    } => {
                        pass.set_pipeline(&pipelines.line);
                        pass.set_bind_group(
                            0,
                            &self.line_bind_group,
                            &[uniform_slot * UNIFORM_STRIDE as u32],
                        );
                        pass.set_vertex_buffer(0, self.line_quad_vbo.slice(..));
                        pass.set_vertex_buffer(1, self.grid_buf.slice(..));
                        pass.draw(0..4, first..first + count);
                    }
                }
            }
        }
    }

    /// Capture each capture-enabled scene's depth into its read-back
    /// buffer, for targets whose buffer is `Free` — a busy buffer keeps
    /// serving its previous map. Two capture strategies:
    ///
    /// - `depth_readback == true`: resolve the stored depth attachment
    ///   into the R32F occlusion target via a fullscreen triangle.
    ///   Encoded right after [`Self::encode_offscreen`], while the stored
    ///   depth is still alive.
    /// - `depth_readback == false` (WebGL2): the depth attachment can't be
    ///   sampled, so re-render the scene's meshes — the only depth-writing
    ///   geometry — packing fragment depth into the RGBA8 occlusion target.
    ///   Single-sampled regardless of the scene's MSAA count.
    pub(crate) fn encode_depth_capture(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        if !self.depth_readback
            && self.depth_pack_pipeline.is_none()
            && self.runs.iter().any(|r| r.capture_depth)
        {
            self.depth_pack_pipeline = Some(build_depth_pack_pipeline(
                device,
                &self.scene_pipeline_layout,
                &self.mesh_shader,
            ));
        }
        for run in self.runs.iter().filter(|r| r.capture_depth) {
            let Some(target) = self.targets.get_mut(&run.target_id) else {
                continue;
            };
            let Some(occ) = target.occlusion.as_mut() else {
                continue;
            };
            if !matches!(occ.state, ReadbackState::Free) {
                continue; // a capture is already in flight; reuse it
            }
            let pose = (run.camera, run.rect);
            if occ.last_captured == Some(pose) {
                continue; // the current map already matches this pose
            }
            if self.depth_readback {
                // Resolve the (possibly MSAA) depth into the single-sample
                // R32F target via a fullscreen triangle.
                let resolve = self
                    .resolve_pipelines
                    .entry(run.sample_count)
                    .or_insert_with(|| build_resolve_pipeline(device, run.sample_count));
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("damascene_wgpu::scene::depth_resolve_bind"),
                    layout: &resolve.bind_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&target.depth),
                    }],
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("damascene_wgpu::scene::depth_resolve"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &occ.color_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), // far
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&resolve.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            } else {
                // Depth-as-color: re-draw the meshes with the same vertex
                // transform (same module, same uniforms — identical depth)
                // and a fragment stage that packs `frag.z`.
                let pack = self
                    .depth_pack_pipeline
                    .as_ref()
                    .expect("built above when any run captures depth");
                let depth_view = occ
                    .pack_depth
                    .as_ref()
                    .expect("packed occlusion resources carry a depth buffer");
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("damascene_wgpu::scene::depth_pack"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &occ.color_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            // White unpacks to depth 1.0 — empty background
                            // reads far, occluding nothing.
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pack);
                for cmd in &run.cmds {
                    // Translucent meshes don't write depth in the main pass,
                    // so they must not occlude labels here either.
                    let DrawCmd::Mesh {
                        geo,
                        uniform_slot,
                        translucent: false,
                    } = *cmd
                    else {
                        continue;
                    };
                    let Some(CachedGeometry {
                        buffers: GeoBuffers::Mesh(m),
                        ..
                    }) = self.geometry.get(&geo)
                    else {
                        continue;
                    };
                    pass.set_bind_group(
                        0,
                        &self.mesh_bind_group,
                        &[uniform_slot * UNIFORM_STRIDE as u32],
                    );
                    pass.set_vertex_buffer(0, m.vbuf.slice(..));
                    match &m.ibuf {
                        Some(ibuf) => {
                            pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..m.icount, 0, 0..1);
                        }
                        None => pass.draw(0..m.vcount, 0..1),
                    }
                }
            }
            // Copy the captured depth into the CPU-mappable read-back buffer.
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &occ.color,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &occ.readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(occ.padded_bytes_per_row),
                        rows_per_image: Some(occ.height),
                    },
                },
                wgpu::Extent3d {
                    width: occ.width,
                    height: occ.height,
                    depth_or_array_layers: 1,
                },
            );
            occ.state = ReadbackState::Pending {
                camera: run.camera,
                rect: run.rect,
            };
            occ.last_captured = Some(pose);
        }
    }

    /// Drive the async read-back state machine and return any depth maps
    /// that became ready this frame. Called at the top of the host's
    /// `prepare`, after the previous frame's encoder has been submitted:
    /// `Pending` buffers request a map, `Mapping` buffers whose callback
    /// fired are read into a [`SceneDepthMap`] and freed.
    pub(crate) fn collect_depth_maps(
        &mut self,
        device: &wgpu::Device,
    ) -> Vec<(String, SceneDepthMap)> {
        // Drive mapping callbacks without blocking the frame.
        let _ = device.poll(wgpu::PollType::Poll);
        let mut ready = Vec::new();
        for (id, target) in self.targets.iter_mut() {
            let Some(occ) = target.occlusion.as_mut() else {
                continue;
            };
            enum Step {
                Map(ResolvedCamera, Rect),
                Read(ResolvedCamera, Rect),
                Idle,
            }
            let step = match &occ.state {
                ReadbackState::Pending { camera, rect } => Step::Map(*camera, *rect),
                ReadbackState::Mapping { camera, rect, done } if done.load(Ordering::Acquire) => {
                    Step::Read(*camera, *rect)
                }
                _ => Step::Idle,
            };
            match step {
                Step::Map(camera, rect) => {
                    let done = Arc::new(AtomicBool::new(false));
                    let flag = done.clone();
                    occ.readback
                        .slice(..)
                        .map_async(wgpu::MapMode::Read, move |res| {
                            if res.is_ok() {
                                flag.store(true, Ordering::Release);
                            }
                        });
                    occ.state = ReadbackState::Mapping { camera, rect, done };
                }
                Step::Read(camera, rect) => {
                    let depth = {
                        let view = occ.readback.slice(..).get_mapped_range();
                        if occ.packed {
                            depad_packed_rgba8(
                                &view,
                                occ.width,
                                occ.height,
                                occ.padded_bytes_per_row,
                            )
                        } else {
                            depad_r32(&view, occ.width, occ.height, occ.padded_bytes_per_row)
                        }
                    };
                    occ.readback.unmap();
                    occ.state = ReadbackState::Free;
                    ready.push((
                        id.clone(),
                        SceneDepthMap {
                            camera,
                            rect,
                            width: occ.width,
                            height: occ.height,
                            depth: Arc::from(depth),
                        },
                    ));
                }
                Step::Idle => {}
            }
        }
        ready
    }

    /// Whether a target (offscreen + caches) is still alive — lets the host
    /// GC stale depth maps for scenes that left the tree.
    pub(crate) fn has_target(&self, id: &str) -> bool {
        self.targets.contains_key(id)
    }

    /// Whether any recorded scene still needs more frames before its label
    /// occlusion is correct — a capture is in flight, or the current pose
    /// has no matching depth map yet. The host ORs this into `needs_redraw`
    /// so the async read-back can finish even after the camera settles.
    /// Returns `false` once every labelled scene has a current map (and for
    /// label-free scenes), so lazy rendering still idles.
    pub(crate) fn occlusion_unsettled(&self) -> bool {
        self.runs.iter().filter(|r| r.capture_depth).any(|r| {
            match self
                .targets
                .get(&r.target_id)
                .and_then(|t| t.occlusion.as_ref())
            {
                // No resources / no capture yet → a map is still owed.
                None => true,
                // A capture is resolving, or the live pose differs from the
                // last captured one (a new capture is due).
                Some(occ) => {
                    !matches!(occ.state, ReadbackState::Free)
                        || occ.last_captured != Some((r.camera, r.rect))
                }
            }
        })
    }

    pub(crate) fn run(&self, index: usize) -> &Scene3DRun {
        &self.runs[index]
    }
    pub(crate) fn composite_pipeline(&self) -> &wgpu::RenderPipeline {
        &self.composite_pipeline
    }
    pub(crate) fn composite_instance_buf(&self) -> &wgpu::Buffer {
        &self.composite_instance_buf
    }
    pub(crate) fn composite_bind_group(&self, run: &Scene3DRun) -> &wgpu::BindGroup {
        &self
            .targets
            .get(&run.target_id)
            .expect("target alive for the frame")
            .composite_bind_group
    }
    /// True when any scene was recorded this frame — lets the render loop
    /// skip the offscreen pre-pass entirely on UI-only frames.
    pub(crate) fn has_runs(&self) -> bool {
        !self.runs.is_empty()
    }
}

// ---- free helpers ----

fn make_uniform_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    capacity: usize,
    tag: &str,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("damascene_wgpu::scene::uniform_buf"),
        size: capacity as u64 * UNIFORM_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let _ = tag;
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("damascene_wgpu::scene::uniform_bind_group"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            // Dynamic-offset window: one element's worth, addressed by the
            // 256-byte-aligned offset at draw time.
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buf,
                offset: 0,
                size: wgpu::BufferSize::new(UNIFORM_STRIDE),
            }),
        }],
    });
    (buf, bind_group)
}

fn premultiplied_blend() -> wgpu::BlendState {
    wgpu::BlendState {
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
    }
}

/// Build the composite pipeline (resolved offscreen texture → main pass,
/// reusing the stock `surface` shader at `fs_premul`). Shared by `new` and
/// `set_target_format` so the descriptor stays a single source of truth —
/// only `target_format` varies across the two call sites. This is the only
/// scene pipeline bound to the swapchain target format.
fn build_composite_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let surface_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scene::composite (stock surface)"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SURFACE)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::scene::composite_pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &surface_shader,
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
                    array_stride: std::mem::size_of::<CompositeInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &COMPOSITE_INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &surface_shader,
            entry_point: Some("fs_premul"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(premultiplied_blend()),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
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

/// WGSL for the depth-resolve pass: a fullscreen triangle that reads the
/// scene depth (sample 0) and writes it to an `R32Float` target. The depth
/// binding type differs for MSAA vs single-sample, so it is templated.
fn resolve_wgsl(multisampled: bool) -> String {
    let binding = if multisampled {
        "texture_depth_multisampled_2d"
    } else {
        "texture_depth_2d"
    };
    format!(
        "@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {{
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(p[vid], 0.0, 1.0);
}}
@group(0) @binding(0) var depth_tex: {binding};
@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) f32 {{
    return textureLoad(depth_tex, vec2<i32>(i32(frag.x), i32(frag.y)), 0);
}}
"
    )
}

/// Build the depth-resolve pipeline for one MSAA sample count.
fn build_resolve_pipeline(device: &wgpu::Device, sample_count: u32) -> ResolvePipeline {
    let multisampled = sample_count > 1;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("damascene_wgpu::scene::depth_resolve"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(resolve_wgsl(multisampled))),
    });
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("damascene_wgpu::scene::depth_resolve_bind_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("damascene_wgpu::scene::depth_resolve_layout"),
        bind_group_layouts: &[Some(&bind_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::scene::depth_resolve_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: SCENE_OCC_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    ResolvePipeline {
        pipeline,
        bind_layout,
    }
}

/// WGSL fragment stage for the packed depth-as-color capture: paired with
/// the stock mesh *vertex* shader (same module, same uniforms — identical
/// depth values), it packs `frag.z` as 24-bit fixed point into RGB. Integer
/// packing, not the classic `fract()` trick — exact at `z == 1.0` and free
/// of float rounding seams between bytes.
const DEPTH_PACK_FS: &str = "\
@fragment
fn fs_pack(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    // frag.z is normalised device depth in [0, 1].
    let v = u32(clamp(frag.z, 0.0, 1.0) * 16777215.0);
    return vec4<f32>(
        f32((v >> 16u) & 0xffu) / 255.0,
        f32((v >> 8u) & 0xffu) / 255.0,
        f32(v & 0xffu) / 255.0,
        1.0,
    );
}
";

/// Build the depth-as-color capture pipeline (`depth_readback == false`).
/// Vertex stage is the stock mesh shader's `vs_main` — reusing the module
/// guarantees the capture's depth matches the scene render exactly. Always
/// single-sampled; primitive/cull state mirrors the mesh pipeline so the
/// same faces survive.
fn build_depth_pack_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    mesh_shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    let fs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("damascene_wgpu::scene::depth_pack_fs"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(DEPTH_PACK_FS)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::scene::depth_pack_pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: mesh_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<MeshVertexGpu>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &MESH_VERTEX_ATTRS,
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &fs_module,
            entry_point: Some("fs_pack"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: SCENE_OCC_PACKED_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: SCENE_OCC_PACKED_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// Allocate the per-target occlusion read-back resources for a `px`-sized
/// scene: the resolve target ([`SCENE_OCC_FORMAT`], or
/// [`SCENE_OCC_PACKED_FORMAT`] plus a depth buffer for the packed
/// depth-as-color pass) and a 256-row-aligned mappable buffer sized to
/// hold it. Both formats are 4 bytes/texel, so the buffer math is shared.
fn build_occlusion_resources(
    device: &wgpu::Device,
    px: (u32, u32),
    packed: bool,
) -> OcclusionResources {
    let (width, height) = px;
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::scene::occlusion_depth"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if packed {
            SCENE_OCC_PACKED_FORMAT
        } else {
            SCENE_OCC_FORMAT
        },
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
    let pack_depth = packed.then(|| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("damascene_wgpu::scene::occlusion_pack_depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_OCC_PACKED_DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    });
    let unpadded = width * 4; // R32Float / Rgba8Unorm = 4 bytes/texel
    let padded_bytes_per_row = unpadded.div_ceil(COPY_ROW_ALIGN) * COPY_ROW_ALIGN;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("damascene_wgpu::scene::occlusion_readback"),
        size: (padded_bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    OcclusionResources {
        color_view,
        color,
        readback,
        padded_bytes_per_row,
        width,
        height,
        packed,
        pack_depth,
        state: ReadbackState::Free,
        last_captured: None,
    }
}

/// De-pad a mapped `R32Float` read-back (rows padded to
/// [`COPY_ROW_ALIGN`]) into a tight row-major `width * height` depth vec.
fn depad_r32(bytes: &[u8], width: u32, height: u32, padded_bytes_per_row: u32) -> Vec<f32> {
    let mut out = Vec::with_capacity((width * height) as usize);
    let row_bytes = (width * 4) as usize;
    for y in 0..height as usize {
        let start = y * padded_bytes_per_row as usize;
        let row = &bytes[start..start + row_bytes];
        for px in row.chunks_exact(4) {
            out.push(f32::from_le_bytes([px[0], px[1], px[2], px[3]]));
        }
    }
    out
}

/// De-pad + decode a mapped `Rgba8Unorm` read-back written by
/// [`DEPTH_PACK_FS`]: RGB carry 24-bit fixed-point depth, alpha is ignored.
fn depad_packed_rgba8(
    bytes: &[u8],
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
) -> Vec<f32> {
    let mut out = Vec::with_capacity((width * height) as usize);
    let row_bytes = (width * 4) as usize;
    for y in 0..height as usize {
        let start = y * padded_bytes_per_row as usize;
        let row = &bytes[start..start + row_bytes];
        for px in row.chunks_exact(4) {
            let v = u32::from(px[0]) << 16 | u32::from(px[1]) << 8 | u32::from(px[2]);
            out.push(v as f32 / 16_777_215.0);
        }
    }
    out
}

fn build_scene_pipelines(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    point_shader: &wgpu::ShaderModule,
    line_shader: &wgpu::ShaderModule,
    mesh_shader: &wgpu::ShaderModule,
    sample_count: u32,
) -> ScenePipelines {
    let color_target = |blend| {
        Some(wgpu::ColorTargetState {
            format: SCENE_COLOR_FORMAT,
            blend: Some(blend),
            write_mask: wgpu::ColorWrites::ALL,
        })
    };
    let multisample = wgpu::MultisampleState {
        count: sample_count,
        mask: !0,
        alpha_to_coverage_enabled: false,
    };
    // Points/lines: depth-test against meshes but don't write (transparent
    // AA edges). Meshes: full depth write.
    let depth_no_write = wgpu::DepthStencilState {
        format: SCENE_DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias: Default::default(),
    };
    let depth_write = wgpu::DepthStencilState {
        format: SCENE_DEPTH_FORMAT,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias: Default::default(),
    };

    let point = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::scene::point_pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: point_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: (4 * std::mem::size_of::<f32>()) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &POINT_QUAD_ATTRS,
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PointInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &POINT_INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: point_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[color_target(premultiplied_blend())],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: Some(depth_no_write.clone()),
        multisample,
        multiview_mask: None,
        cache: None,
    });

    let line = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("damascene_wgpu::scene::line_pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: line_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &LINE_QUAD_ATTRS,
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &LINE_INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: line_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[color_target(premultiplied_blend())],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: Some(depth_no_write.clone()),
        multisample,
        multiview_mask: None,
        cache: None,
    });

    // Opaque meshes write depth and cull back faces. Translucent meshes
    // depth-test against the opaque set but don't write, and draw two-sided
    // in two passes — back faces (cull front) then front faces (cull back) —
    // see `ScenePipelines`.
    let mesh_pipeline = |label, depth: &wgpu::DepthStencilState, cull| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: mesh_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<MeshVertexGpu>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &MESH_VERTEX_ATTRS,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: mesh_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[color_target(premultiplied_blend())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(cull),
                ..Default::default()
            },
            depth_stencil: Some(depth.clone()),
            multisample,
            multiview_mask: None,
            cache: None,
        })
    };
    let mesh = mesh_pipeline(
        "damascene_wgpu::scene::mesh_pipeline",
        &depth_write,
        wgpu::Face::Back,
    );
    let mesh_back = mesh_pipeline(
        "damascene_wgpu::scene::mesh_back_pipeline",
        &depth_no_write,
        wgpu::Face::Front,
    );
    let mesh_front = mesh_pipeline(
        "damascene_wgpu::scene::mesh_front_pipeline",
        &depth_no_write,
        wgpu::Face::Back,
    );

    ScenePipelines {
        point,
        line,
        mesh,
        mesh_back,
        mesh_front,
    }
}
