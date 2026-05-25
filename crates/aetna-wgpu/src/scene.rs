//! GPU rendering for [`DrawOp::Scene3D`](aetna_core::ir::DrawOp::Scene3D).
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
//!    the [`BackdropSnapshot`](aetna_core::paint::PaintItem::BackdropSnapshot)
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

use aetna_core::color::{Color, ColorSpace};
use aetna_core::paint::{PhysicalScissor, rgba_f32_in};
use aetna_core::scene::{
    LineDraw, Material, MeshDraw, PointDraw, PointShape, Scene3DData, SceneStyle, SizeMode,
};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::Rect;

use aetna_core::scene::glam::{Mat4, Vec3};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Linear fp16 — the working colour space with HDR headroom. See module docs.
const SCENE_COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const SCENE_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Uniform dynamic-offset stride. 256 is the common
/// `min_uniform_buffer_offset_alignment` ceiling; every per-draw uniform
/// struct here is smaller, so each lands in its own 256-byte slot.
const UNIFORM_STRIDE: u64 = 256;

// ---- GPU-side POD structs ----

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct PointUniform {
    mvp: [[f32; 4]; 4],
    screen_size_px: [f32; 2],
    point_size_px: f32,
    size_mode: u32,
    shape: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct LineUniform {
    mvp: [[f32; 4]; 4],
    screen_size: [f32; 2],
    width_mode: u32,
    default_width: f32,
    dash_length: f32,
    gap_length: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MeshUniform {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    base_color: [f32; 4],
    light_dir: [f32; 4],
    key_color: [f32; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct PointInstance {
    position: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct LineInstance {
    start: [f32; 3],
    end: [f32; 3],
    color: [f32; 4],
    width: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MeshVertexGpu {
    position: [f32; 3],
    normal: [f32; 3],
}

/// Composite instance — identical layout to [`crate::surface`]'s, so the
/// stock `surface` shader's vertex stage reads it unchanged.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CompositeInstance {
    rect: [f32; 4],
    matrix: [f32; 4],
    translation: [f32; 2],
}

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
    used_frame: u64,
}

// ---- Per-draw command + per-scene run ----

enum DrawCmd {
    Mesh { geo: u64, uniform_slot: u32 },
    Points { geo: u64, uniform_slot: u32 },
    Lines { geo: u64, uniform_slot: u32 },
    /// Reference grid + axes, generated per frame into `grid_buf` rather
    /// than cached by GeometryId (the geometry follows `SceneStyle`, not an
    /// app handle). `first..first+count` indexes the shared grid buffer.
    Grid { uniform_slot: u32, first: u32, count: u32 },
}

pub(crate) struct Scene3DRun {
    target_id: String,
    pub scissor: Option<PhysicalScissor>,
    sample_count: u32,
    clear: wgpu::Color,
    cmds: Vec<DrawCmd>,
    /// Index into the composite instance buffer.
    pub composite_instance: u32,
}

/// One scene-shader pipeline set, built per MSAA sample count seen.
struct ScenePipelines {
    point: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
}

pub(crate) struct Scene3DPaint {
    working: ColorSpace,

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
/// Clamp on grid lines per direction, guarding pathological extent/spacing.
const MAX_GRID_LINES: i32 = 256;

impl Scene3DPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        frame_bind_layout: &wgpu::BindGroupLayout,
        working: ColorSpace,
    ) -> Self {
        // Billboard quad: corner (-1..1) + uv (0..1), triangle strip.
        let point_quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aetna_wgpu::scene::point_quad"),
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
            label: Some("aetna_wgpu::scene::line_quad"),
            contents: bytemuck::cast_slice::<f32, u8>(&[
                0.0, -1.0, // start left
                1.0, -1.0, // end left
                0.0, 1.0, // start right
                1.0, 1.0, // end right
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aetna_wgpu::scene::uniform_layout"),
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
                label: Some("aetna_wgpu::scene::pipeline_layout"),
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
                label: Some("aetna_wgpu::scene::composite_tex_layout"),
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
                label: Some("aetna_wgpu::scene::composite_pipeline_layout"),
                bind_group_layouts: &[Some(frame_bind_layout), Some(&composite_bind_layout)],
                immediate_size: 0,
            });
        let surface_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene::composite (stock surface)"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::SURFACE)),
        });
        let composite_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("aetna_wgpu::scene::composite_pipeline"),
                layout: Some(&composite_pipeline_layout),
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
            });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aetna_wgpu::scene::sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let composite_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::scene::composite_instances"),
            size: (INITIAL_COMPOSITE_CAP * std::mem::size_of::<CompositeInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let grid_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::scene::grid_lines"),
            size: (INITIAL_GRID_CAP * std::mem::size_of::<LineInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            working,
            point_quad_vbo,
            line_quad_vbo,
            uniform_layout,
            point_shader,
            line_shader,
            mesh_shader,
            scene_pipeline_layout,
            pipelines: HashMap::new(),
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
        self.ensure_target(device, id, px, sample_count);

        let aspect = px.0 as f32 / px.1 as f32;
        let view_proj = scene.camera.view_proj(aspect);
        let screen = [px.0 as f32, px.1 as f32];
        let working = self.working;

        let mut cmds = Vec::new();

        // Reference grid + axes first, so the data draws over them. The
        // line pipeline depth-tests (no write), so meshes still occlude
        // grid behind them.
        let first = self.grid_instances.len() as u32;
        build_grid_lines(&scene.style, working, &mut self.grid_instances);
        let count = self.grid_instances.len() as u32 - first;
        if count > 0 {
            let slot = self.push_line_uniform(LineUniform {
                mvp: view_proj.to_cols_array_2d(),
                screen_size: screen,
                width_mode: 0, // grid/axes are screen-space px
                default_width: 1.0,
                dash_length: 0.0,
                gap_length: 0.0,
                _pad: [0.0; 2],
            });
            cmds.push(DrawCmd::Grid { uniform_slot: slot, first, count });
        }

        for m in &scene.meshes {
            self.ensure_mesh_geometry(device, m);
            let slot = self.push_mesh_uniform(mesh_uniform(view_proj, m, scene, working));
            cmds.push(DrawCmd::Mesh { geo: m.geometry.id().0, uniform_slot: slot });
        }
        for p in &scene.points {
            self.ensure_point_geometry(device, p, working);
            let slot = self.push_point_uniform(point_uniform(view_proj * p.transform, screen, p));
            cmds.push(DrawCmd::Points { geo: p.geometry.id().0, uniform_slot: slot });
        }
        for l in &scene.lines {
            self.ensure_line_geometry(device, l, working);
            let slot = self.push_line_uniform(line_uniform(view_proj * l.transform, screen, l));
            cmds.push(DrawCmd::Lines { geo: l.geometry.id().0, uniform_slot: slot });
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
        self.composite_instances.push(CompositeInstance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            matrix: [1.0, 0.0, 0.0, 1.0],
            translation: [0.0, 0.0],
        });

        self.runs.push(Scene3DRun {
            target_id: id.to_string(),
            scissor,
            sample_count,
            clear,
            cmds,
            composite_instance,
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
    ) {
        if let Some(t) = self.targets.get_mut(id)
            && t.size == px
            && t.sample_count == sample_count
        {
            t.used_frame = self.frame_counter;
            return;
        }
        let extent = wgpu::Extent3d {
            width: px.0,
            height: px.1,
            depth_or_array_layers: 1,
        };
        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aetna_wgpu::scene::resolve"),
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
                    label: Some("aetna_wgpu::scene::msaa_color"),
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
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("aetna_wgpu::scene::depth"),
                size: extent,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aetna_wgpu::scene::composite_bind_group"),
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
        let verts: Vec<MeshVertexGpu> = data
            .vertices
            .iter()
            .map(|v| MeshVertexGpu {
                position: v.position.to_array(),
                normal: v.normal.to_array(),
            })
            .collect();
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aetna_wgpu::scene::mesh_vbuf"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let (ibuf, icount) = match &data.indices {
            Some(indices) => (
                Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("aetna_wgpu::scene::mesh_ibuf"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                })),
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

    fn ensure_point_geometry(&mut self, device: &wgpu::Device, draw: &PointDraw, working: ColorSpace) {
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
        let instances: Vec<PointInstance> = data
            .points
            .iter()
            .map(|p| PointInstance {
                position: p.position.to_array(),
                color: to_linear(p.color, working),
            })
            .collect();
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aetna_wgpu::scene::point_ibuf"),
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

    fn ensure_line_geometry(&mut self, device: &wgpu::Device, draw: &LineDraw, working: ColorSpace) {
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
        let instances: Vec<LineInstance> = data
            .segments
            .iter()
            .map(|s| LineInstance {
                start: s.start.to_array(),
                end: s.end.to_array(),
                color: to_linear(s.color, working),
                width: 0.0, // style width comes from the uniform default
            })
            .collect();
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aetna_wgpu::scene::line_ibuf"),
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
            queue.write_buffer(&self.point_ubo, i as u64 * UNIFORM_STRIDE, bytemuck::bytes_of(u));
        }
        for (i, u) in self.line_uniforms.iter().enumerate() {
            queue.write_buffer(&self.line_ubo, i as u64 * UNIFORM_STRIDE, bytemuck::bytes_of(u));
        }
        for (i, u) in self.mesh_uniforms.iter().enumerate() {
            queue.write_buffer(&self.mesh_ubo, i as u64 * UNIFORM_STRIDE, bytemuck::bytes_of(u));
        }

        if self.composite_instances.len() > self.composite_instance_cap {
            let cap = self.composite_instances.len().next_power_of_two();
            self.composite_instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::scene::composite_instances (resized)"),
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
                label: Some("aetna_wgpu::scene::grid_lines (resized)"),
                size: (cap * std::mem::size_of::<LineInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.grid_cap = cap;
        }
        if !self.grid_instances.is_empty() {
            queue.write_buffer(&self.grid_buf, 0, bytemuck::cast_slice(&self.grid_instances));
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
                label: Some("aetna_wgpu::scene::offscreen"),
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
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            for cmd in &run.cmds {
                match *cmd {
                    DrawCmd::Mesh { geo, uniform_slot } => {
                        let Some(CachedGeometry { buffers: GeoBuffers::Mesh(m), .. }) =
                            self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        pass.set_pipeline(&pipelines.mesh);
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
                    DrawCmd::Points { geo, uniform_slot } => {
                        let Some(CachedGeometry { buffers: GeoBuffers::Points(p), .. }) =
                            self.geometry.get(&geo)
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
                        let Some(CachedGeometry { buffers: GeoBuffers::Lines(l), .. }) =
                            self.geometry.get(&geo)
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
                    DrawCmd::Grid { uniform_slot, first, count } => {
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

/// Interpret an authoring-space sRGBA `[f32; 4]` (the geometry colour
/// contract) and convert it into the working linear space.
fn to_linear(srgba: [f32; 4], working: ColorSpace) -> [f32; 4] {
    rgba_f32_in(
        Color::in_space(ColorSpace::SRGB, srgba[0], srgba[1], srgba[2], srgba[3]),
        working,
    )
}

/// Generate the reference grid + axes as line instances (colours already in
/// the working space). Grid segments carry width 0 so the uniform's default
/// width applies; axes carry an explicit, slightly bolder width.
fn build_grid_lines(style: &SceneStyle, working: ColorSpace, out: &mut Vec<LineInstance>) {
    let g = &style.grid;
    let extent = g.extent.max(0.0);
    if extent > 0.0 {
        let grid_color = rgba_f32_in(g.color, working);
        let step = (g.spacing / g.subdivisions.max(1) as f32).max(1e-4);
        let n = ((extent / step).floor() as i32).clamp(0, MAX_GRID_LINES);
        if g.planes.xz {
            plane_grid(out, Vec3::X, Vec3::Z, n, step, extent, grid_color);
        }
        if g.planes.xy {
            plane_grid(out, Vec3::X, Vec3::Y, n, step, extent, grid_color);
        }
        if g.planes.yz {
            plane_grid(out, Vec3::Y, Vec3::Z, n, step, extent, grid_color);
        }
    }

    if style.show_axes {
        // Muted R/G/B for X/Y/Z — readable without the neon look. Axis
        // styling gets configurable in M4; this is the polished default.
        let ax = extent.max(g.spacing).max(1.0);
        for (dir, rgb) in [
            (Vec3::X, Color::srgb_u8(206, 86, 86)),
            (Vec3::Y, Color::srgb_u8(120, 190, 110)),
            (Vec3::Z, Color::srgb_u8(110, 150, 225)),
        ] {
            push_seg(out, -dir * ax, dir * ax, rgba_f32_in(rgb, working), 1.6);
        }
    }
}

/// Grid lines for one world plane spanned by unit axes `u`, `v`: lines
/// parallel to each axis at every `step` offset within `[-extent, extent]`.
fn plane_grid(
    out: &mut Vec<LineInstance>,
    u: Vec3,
    v: Vec3,
    n: i32,
    step: f32,
    extent: f32,
    color: [f32; 4],
) {
    for i in -n..=n {
        let off = i as f32 * step;
        push_seg(out, v * off - u * extent, v * off + u * extent, color, 0.0);
        push_seg(out, u * off - v * extent, u * off + v * extent, color, 0.0);
    }
}

fn push_seg(out: &mut Vec<LineInstance>, a: Vec3, b: Vec3, color: [f32; 4], width: f32) {
    out.push(LineInstance {
        start: a.to_array(),
        end: b.to_array(),
        color,
        width,
    });
}

fn point_uniform(mvp: Mat4, screen: [f32; 2], draw: &PointDraw) -> PointUniform {
    PointUniform {
        mvp: mvp.to_cols_array_2d(),
        screen_size_px: screen,
        point_size_px: draw.style.size,
        size_mode: size_mode_code(draw.style.size_mode),
        shape: match draw.style.shape {
            PointShape::Circle => 0,
            PointShape::Square => 1,
        },
        _pad: [0; 3],
    }
}

fn line_uniform(mvp: Mat4, screen: [f32; 2], draw: &LineDraw) -> LineUniform {
    use aetna_core::scene::LinePattern;
    let (dash, gap) = match draw.style.pattern {
        LinePattern::Solid => (0.0, 0.0),
        // Screen-pixel dash cadence; world-unit dashing would scale with
        // zoom, which reads worse for reference strokes.
        LinePattern::Dashed => (8.0, 6.0),
    };
    LineUniform {
        mvp: mvp.to_cols_array_2d(),
        screen_size: screen,
        width_mode: size_mode_code(draw.style.size_mode),
        default_width: draw.style.width,
        dash_length: dash,
        gap_length: gap,
        _pad: [0.0; 2],
    }
}

fn mesh_uniform(
    view_proj: Mat4,
    draw: &MeshDraw,
    scene: &Scene3DData,
    working: ColorSpace,
) -> MeshUniform {
    let light = &scene.lights;
    let dir = light.key_direction.normalize_or_zero();
    // Flat is unlit: fold it into the lit shader as ambient=1, no key.
    let (base, ambient, key_intensity) = match &draw.material {
        Material::Matte { base } => (*base, light.ambient, light.key_intensity),
        Material::Flat { color } => (*color, 1.0, 0.0),
        // Custom material shaders are post-V1 (plan M5); render as Matte so
        // the mesh is still visible rather than dropped.
        Material::Custom { .. } => (
            Color::srgb_u8(214, 220, 230),
            light.ambient,
            light.key_intensity,
        ),
    };
    let key = rgba_f32_in(light.key_color, working);
    MeshUniform {
        view_proj: view_proj.to_cols_array_2d(),
        model: draw.transform.to_cols_array_2d(),
        base_color: rgba_f32_in(base, working),
        light_dir: [dir.x, dir.y, dir.z, 0.0],
        key_color: [key[0], key[1], key[2], key_intensity],
        params: [ambient, 0.0, 0.0, 0.0],
    }
}

fn size_mode_code(mode: SizeMode) -> u32 {
    match mode {
        SizeMode::ScreenSpace => 0,
        SizeMode::World => 1,
    }
}

fn make_uniform_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    capacity: usize,
    tag: &str,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aetna_wgpu::scene::uniform_buf"),
        size: capacity as u64 * UNIFORM_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let _ = tag;
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("aetna_wgpu::scene::uniform_bind_group"),
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
        label: Some("aetna_wgpu::scene::point_pipeline"),
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
        label: Some("aetna_wgpu::scene::line_pipeline"),
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
        depth_stencil: Some(depth_no_write),
        multisample,
        multiview_mask: None,
        cache: None,
    });

    let mesh = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("aetna_wgpu::scene::mesh_pipeline"),
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
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(depth_write),
        multisample,
        multiview_mask: None,
        cache: None,
    });

    ScenePipelines { point, line, mesh }
}
