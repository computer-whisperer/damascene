//! GPU rendering for [`DrawOp::Scene3D`](aetna_core::ir::DrawOp::Scene3D) on
//! the vulkano backend. Mirrors `aetna-wgpu/src/scene.rs`.
//!
//! A 3D scene is a *two-phase* draw, the same as on wgpu (render passes can't
//! nest, so the scene can't render mid-composite):
//!
//! 1. **Offscreen pre-pass** ([`Scene3DPaint::encode_offscreen`]). Before the
//!    main composite pass begins, each recorded scene renders into its own
//!    offscreen target — an `R16G16B16A16_SFLOAT` colour attachment with a
//!    `D32_SFLOAT` depth buffer, multisampled at the scene's `msaa_samples`
//!    and resolved to a single-sample texture. fp16 + linear keeps the scene
//!    in the runner's working colour space with HDR headroom. The runner
//!    calls this on its command-buffer builder ahead of the main pass.
//! 2. **Composite** (the `PaintItem::Scene3D` arm in the runner's draw loop).
//!    The resolved texture composites into the main pass exactly like an
//!    [`AppTexture`](crate::surface) — same stock `surface` shader, same
//!    premultiplied blend, same logical-rect instance.
//!
//! The backend-neutral byte layouts and CPU packing (uniforms, instances,
//! grid generation, geometry conversion) come from
//! [`aetna_core::scene::gpu`], shared with every backend. Only the GPU
//! resource management (pipelines, render passes, offscreen targets, the
//! geometry/target caches) lives here.
//!
//! Where wgpu uses dynamic-offset uniforms, this backend builds one small
//! descriptor set per draw: a scene has only a handful of draws (grid + a few
//! marks), so the per-draw set is cheaper to reason about than overriding
//! reflection to make the uniform binding dynamic.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use aetna_core::color::ColorSpace;
use aetna_core::paint::PhysicalScissor;
use aetna_core::scene::{
    LineDraw, MeshDraw, PointDraw, ResolvedCamera, Scene3DData, SceneDepthMap, gpu,
};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::Rect;

use smallvec::smallvec;
use vulkano::{
    buffer::{
        Buffer, BufferCreateInfo, BufferUsage, Subbuffer,
        allocator::{SubbufferAllocator, SubbufferAllocatorCreateInfo},
    },
    command_buffer::{
        AutoCommandBufferBuilder, CopyImageToBufferInfo, PrimaryAutoCommandBuffer,
        RenderPassBeginInfo, SubpassBeginInfo, SubpassContents, SubpassEndInfo,
    },
    descriptor_set::{
        DescriptorSet, WriteDescriptorSet, allocator::StandardDescriptorSetAllocator,
        layout::DescriptorSetLayout,
    },
    device::Device,
    format::{ClearValue, Format},
    image::{
        Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::ImageView,
    },
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        DynamicState, GraphicsPipeline, Pipeline, PipelineLayout, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{
                AttachmentBlend, BlendFactor, BlendOp, ColorBlendAttachmentState, ColorBlendState,
            },
            depth_stencil::{CompareOp, DepthState, DepthStencilState},
            input_assembly::{InputAssemblyState, PrimitiveTopology},
            rasterization::{CullMode, FrontFace, RasterizationState},
            subpass::PipelineSubpassType,
            vertex_input::{
                VertexInputAttributeDescription, VertexInputBindingDescription, VertexInputRate,
                VertexInputState,
            },
            viewport::{Scissor, Viewport, ViewportState},
        },
        layout::PipelineDescriptorSetLayoutCreateInfo,
    },
    render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass, Subpass},
    shader::{ShaderModule, ShaderModuleCreateInfo},
};

use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::{build_shared_pipeline_layout, multisample_state};

/// Linear fp16 — the working colour space with HDR headroom. Matches the
/// wgpu backend's `SCENE_COLOR_FORMAT`.
const SCENE_COLOR_FORMAT: Format = Format::R16G16B16A16_SFLOAT;
const SCENE_DEPTH_FORMAT: Format = Format::D32_SFLOAT;
/// Single-channel float the scene depth resolves into for label-occlusion
/// read-back; stores normalised device depth in `[0, 1]`. Mirrors the wgpu
/// backend's `SCENE_OCC_FORMAT`.
const SCENE_OCC_FORMAT: Format = Format::R32_SFLOAT;

// ---- per-frame draw plan ----

enum DrawCmd {
    Mesh { geo: u64, slot: usize },
    Points { geo: u64, slot: usize },
    Lines { geo: u64, slot: usize },
    Grid { slot: usize, first: u32, count: u32 },
}

pub(crate) struct Scene3DRun {
    target_id: String,
    pub scissor: Option<PhysicalScissor>,
    sample_count: u32,
    clear: [f32; 4],
    cmds: Vec<DrawCmd>,
    pub composite_instance: u32,
    /// Capture this scene's depth for label occlusion (gated on the scene
    /// asking for labels — see [`Scene3DData::capture_depth`]).
    capture_depth: bool,
    /// Resolved camera + logical rect at record time, stamped into the
    /// captured [`SceneDepthMap`] so occlusion is judged in capture space.
    camera: ResolvedCamera,
    rect: Rect,
}

// ---- cached geometry (versioned by handle revision) ----

enum GeoBuffers {
    Mesh {
        vbuf: Subbuffer<[gpu::MeshVertexGpu]>,
        ibuf: Option<Subbuffer<[u32]>>,
        vcount: u32,
        icount: u32,
    },
    Points {
        ibuf: Subbuffer<[gpu::PointInstance]>,
        count: u32,
    },
    Lines {
        ibuf: Subbuffer<[gpu::LineInstance]>,
        count: u32,
    },
}

struct CachedGeometry {
    buffers: GeoBuffers,
    revision: u64,
    space: ColorSpace,
    used_frame: u64,
}

// ---- per-node offscreen target ----

struct OffscreenTarget {
    size: (u32, u32),
    sample_count: u32,
    framebuffer: Arc<Framebuffer>,
    /// The scene's depth attachment view, kept so the occlusion resolve
    /// pass can sample it (the offscreen pass stores depth).
    depth_view: Arc<ImageView>,
    /// Set 1 for the composite pipeline: resolved scene view + sampler.
    composite_set: Arc<DescriptorSet>,
    /// Depth read-back resources, allocated the first frame this target's
    /// scene asks for label occlusion. `None` for label-free scenes.
    occlusion: Option<OcclusionResources>,
    used_frame: u64,
}

// ---- label-occlusion depth read-back ----

/// Per-target resources for capturing the scene depth buffer and streaming
/// it back to the CPU. The (possibly multisampled) depth is resolved into a
/// single-sample [`SCENE_OCC_FORMAT`] image via a fullscreen triangle, copied
/// into `readback`, and read one frame later — see [`SceneDepthMap`] for the
/// latency contract. Unlike wgpu (`map_async`), vulkano has no async map: the
/// aetna host waits on each frame's fence before the next `prepare`, so the
/// copy recorded into the frame's command buffer is complete by the time
/// [`Scene3DPaint::collect_depth_maps`] reads the buffer.
struct OcclusionResources {
    /// Resolve-pass framebuffer over the `R32_SFLOAT` `color` view.
    framebuffer: Arc<Framebuffer>,
    /// The single-sample resolve target the fullscreen triangle writes.
    color: Arc<Image>,
    /// Host-visible buffer the resolved depth is copied into.
    readback: Subbuffer<[f32]>,
    /// Set 0 for the resolve pipeline: the target's depth view, sampled.
    depth_set: Arc<DescriptorSet>,
    width: u32,
    height: u32,
    state: ReadbackState,
    /// The (camera, rect) of the most recent capture. A fresh capture only
    /// fires when the pose changes, so a settled scene with a current map
    /// stops capturing and the renderer can go idle.
    last_captured: Option<(ResolvedCamera, Rect)>,
}

/// Lifecycle of one target's read-back buffer. The host fence-waits each
/// frame, so there's no async-map intermediate as on wgpu: a copy encoded
/// this frame is readable next `prepare`.
enum ReadbackState {
    /// Idle — eligible to receive a fresh depth copy this frame.
    Free,
    /// A copy was encoded; read it next `prepare` (after the host submits).
    Pending { camera: ResolvedCamera, rect: Rect },
}

// ---- offscreen render pass + scene pipelines, keyed by sample count ----

struct ScenePass {
    render_pass: Arc<RenderPass>,
    point: Arc<GraphicsPipeline>,
    line: Arc<GraphicsPipeline>,
    mesh: Arc<GraphicsPipeline>,
}

pub(crate) struct Scene3DPaint {
    device: Arc<Device>,
    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
    working: ColorSpace,

    /// Billboard quad (corner ∈ [-1,1] + uv ∈ [0,1]) for the point shader.
    point_quad_vbo: Subbuffer<[f32]>,
    /// Line quad template (corner.x = 0/1 start/end, corner.y = ±1 side).
    line_quad_vbo: Subbuffer<[f32]>,

    passes: HashMap<u32, ScenePass>,
    /// Set-0 layout shared by every scene pipeline (forced `VERTEX|FRAGMENT`),
    /// captured from the first pass built so per-draw uniform descriptor sets
    /// bind into any scene pipeline.
    uniform_set_layout: Option<Arc<DescriptorSetLayout>>,

    /// Single-sample `R32_SFLOAT` render pass for the depth-resolve step.
    resolve_pass: Arc<RenderPass>,
    /// Depth-resolve pipelines, keyed by sample count (the depth binding is
    /// `multisampled` vs not, so the pipeline differs).
    resolve_pipelines: HashMap<u32, Arc<GraphicsPipeline>>,

    /// Composite into the runner's main pass — stock `surface` (fs_premul).
    composite_pipeline: Arc<GraphicsPipeline>,
    sampler: Arc<Sampler>,

    uniform_alloc: SubbufferAllocator,
    instance_alloc: SubbufferAllocator,

    // Per-frame scratch (cleared in `frame_begin`).
    uniform_sets: Vec<Arc<DescriptorSet>>,
    grid_instances: Vec<gpu::LineInstance>,
    grid_buf: Option<Subbuffer<[gpu::LineInstance]>>,
    composite_instances: Vec<gpu::CompositeInstance>,
    composite_instance_buf: Option<Subbuffer<[gpu::CompositeInstance]>>,
    runs: Vec<Scene3DRun>,

    geometry: HashMap<u64, CachedGeometry>,
    targets: HashMap<String, OffscreenTarget>,
    frame_counter: u64,
}

const SUBALLOC_ARENA_SIZE: u64 = 256 * 1024;

impl Scene3DPaint {
    pub(crate) fn new(
        device: Arc<Device>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
        composite_subpass: Subpass,
        composite_sample_count: u32,
        working: ColorSpace,
    ) -> Self {
        let point_quad_vbo = Buffer::from_iter(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            host_alloc(),
            [
                -1.0_f32, -1.0, 0.0, 0.0, // bl
                1.0, -1.0, 1.0, 0.0, // br
                -1.0, 1.0, 0.0, 1.0, // tl
                1.0, 1.0, 1.0, 1.0, // tr
            ],
        )
        .expect("aetna-vulkano: scene point quad VBO");
        let line_quad_vbo = Buffer::from_iter(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            host_alloc(),
            [
                0.0_f32, -1.0, // start left
                1.0, -1.0, // end left
                0.0, 1.0, // start right
                1.0, 1.0, // end right
            ],
        )
        .expect("aetna-vulkano: scene line quad VBO");

        let composite_pipeline =
            build_composite_pipeline(device.clone(), composite_subpass, composite_sample_count);
        let resolve_pass = build_resolve_render_pass(device.clone());
        let sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Linear,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: scene composite sampler");

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

        Self {
            device,
            memory_alloc,
            descriptor_alloc,
            working,
            point_quad_vbo,
            line_quad_vbo,
            passes: HashMap::new(),
            uniform_set_layout: None,
            resolve_pass,
            resolve_pipelines: HashMap::new(),
            composite_pipeline,
            sampler,
            uniform_alloc,
            instance_alloc,
            uniform_sets: Vec::new(),
            grid_instances: Vec::new(),
            grid_buf: None,
            composite_instances: Vec::new(),
            composite_instance_buf: None,
            runs: Vec::new(),
            geometry: HashMap::new(),
            targets: HashMap::new(),
            frame_counter: 0,
        }
    }

    pub(crate) fn frame_begin(&mut self) {
        self.uniform_sets.clear();
        self.grid_instances.clear();
        self.composite_instances.clear();
        self.runs.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    pub(crate) fn has_runs(&self) -> bool {
        !self.runs.is_empty()
    }

    pub(crate) fn run(&self, index: usize) -> &Scene3DRun {
        &self.runs[index]
    }

    pub(crate) fn composite_pipeline(&self) -> &Arc<GraphicsPipeline> {
        &self.composite_pipeline
    }

    pub(crate) fn composite_descriptor(&self, run: &Scene3DRun) -> &Arc<DescriptorSet> {
        &self
            .targets
            .get(&run.target_id)
            .expect("scene target alive for the frame")
            .composite_set
    }

    pub(crate) fn composite_instance_buf(&self) -> &Subbuffer<[gpu::CompositeInstance]> {
        self.composite_instance_buf
            .as_ref()
            .expect("aetna-vulkano: scene composite_instance_buf accessed with no draws")
    }

    /// Record one scene: ensure pipelines + offscreen target, upload any
    /// geometry whose revision advanced, pack per-draw uniforms, and build
    /// the composite instance. Returns the single-run range.
    pub(crate) fn record(
        &mut self,
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
        self.ensure_pass(sample_count);
        if scene.capture_depth {
            self.ensure_resolve_pipeline(sample_count);
        }
        self.ensure_target(id, px, sample_count, scene.capture_depth);

        let aspect = px.0 as f32 / px.1 as f32;
        let view_proj = scene.camera.view_proj(aspect);
        let screen = [px.0 as f32, px.1 as f32];
        let working = self.working;

        let mut cmds = Vec::new();

        // Reference grid + axes first, so data draws over them. The line
        // pipeline depth-tests (no write), so meshes still occlude grid.
        let first = self.grid_instances.len() as u32;
        gpu::build_grid_lines(&scene.style, working, &mut self.grid_instances);
        let count = self.grid_instances.len() as u32 - first;
        if count > 0 {
            let slot = self.push_uniform(gpu::grid_uniform(view_proj, screen));
            cmds.push(DrawCmd::Grid { slot, first, count });
        }

        for m in &scene.meshes {
            self.ensure_mesh_geometry(m);
            let slot = self.push_uniform(gpu::mesh_uniform(view_proj, m, scene, working));
            cmds.push(DrawCmd::Mesh {
                geo: m.geometry.id().0,
                slot,
            });
        }
        for p in &scene.points {
            self.ensure_point_geometry(p, working);
            let slot = self.push_uniform(gpu::point_uniform(view_proj * p.transform, screen, p));
            cmds.push(DrawCmd::Points {
                geo: p.geometry.id().0,
                slot,
            });
        }
        for l in &scene.lines {
            self.ensure_line_geometry(l, working);
            let slot = self.push_uniform(gpu::line_uniform(view_proj * l.transform, screen, l));
            cmds.push(DrawCmd::Lines {
                geo: l.geometry.id().0,
                slot,
            });
        }

        let clear = scene
            .style
            .background
            .map(|c| {
                let [r, g, b, a] = aetna_core::paint::rgba_f32_in(c, working);
                // Premultiplied, to match the premultiplied scene output the
                // composite blends.
                [r * a, g * a, b * a, a]
            })
            .unwrap_or([0.0, 0.0, 0.0, 0.0]);

        let composite_instance = self.composite_instances.len() as u32;
        self.composite_instances.push(gpu::CompositeInstance::new(
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

    /// Allocate a per-frame uniform subbuffer, write `u`, and build a set-0
    /// descriptor set bound to it. Returns the slot index into `uniform_sets`.
    fn push_uniform<T>(&mut self, u: T) -> usize
    where
        T: vulkano::buffer::BufferContents,
    {
        let buf = self
            .uniform_alloc
            .allocate_sized::<T>()
            .expect("aetna-vulkano: scene uniform suballocate");
        *buf.write()
            .expect("aetna-vulkano: scene uniform suballocation write") = u;
        let layout = self
            .uniform_set_layout
            .clone()
            .expect("uniform_set_layout captured by ensure_pass");
        let set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            layout,
            [WriteDescriptorSet::buffer(0, buf)],
            [],
        )
        .expect("aetna-vulkano: scene uniform descriptor set");
        let slot = self.uniform_sets.len();
        self.uniform_sets.push(set);
        slot
    }

    fn ensure_pass(&mut self, sample_count: u32) {
        if self.passes.contains_key(&sample_count) {
            return;
        }
        let render_pass = build_scene_render_pass(self.device.clone(), sample_count);
        let subpass = Subpass::from(render_pass.clone(), 0)
            .expect("aetna-vulkano: scene offscreen subpass 0");
        let point = build_scene_pipeline(
            self.device.clone(),
            subpass.clone(),
            sample_count,
            "stock::scene_point",
            stock_wgsl::SCENE_POINT,
            point_vertex_input(),
            PrimitiveTopology::TriangleStrip,
            /* depth_write: */ false,
            /* cull: */ false,
        );
        let line = build_scene_pipeline(
            self.device.clone(),
            subpass.clone(),
            sample_count,
            "stock::scene_line",
            stock_wgsl::SCENE_LINE,
            line_vertex_input(),
            PrimitiveTopology::TriangleStrip,
            false,
            false,
        );
        let mesh = build_scene_pipeline(
            self.device.clone(),
            subpass,
            sample_count,
            "stock::scene_mesh",
            stock_wgsl::SCENE_MESH,
            mesh_vertex_input(),
            PrimitiveTopology::TriangleList,
            /* depth_write: */ true,
            /* cull: */ true,
        );
        if self.uniform_set_layout.is_none() {
            self.uniform_set_layout = Some(point.layout().set_layouts()[0].clone());
        }
        self.passes.insert(
            sample_count,
            ScenePass {
                render_pass,
                point,
                line,
                mesh,
            },
        );
    }

    fn ensure_target(&mut self, id: &str, px: (u32, u32), sample_count: u32, capture_depth: bool) {
        if let Some(t) = self.targets.get(id)
            && t.size == px
            && t.sample_count == sample_count
        {
            // Allocate occlusion resources lazily if the scene only now
            // started asking for label occlusion. Read what we need and drop
            // the borrow before `build_occlusion_resources` re-borrows `self`.
            let need_occ = capture_depth && t.occlusion.is_none();
            let depth_view = t.depth_view.clone();
            if need_occ {
                let occ = self.build_occlusion_resources(depth_view, px, sample_count);
                self.targets
                    .get_mut(id)
                    .expect("target just matched")
                    .occlusion = Some(occ);
            }
            self.targets
                .get_mut(id)
                .expect("target just matched")
                .used_frame = self.frame_counter;
            return;
        }
        let pass = self.passes.get(&sample_count).expect("pass ensured");
        let extent = [px.0, px.1, 1];

        let resolve_image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: SCENE_COLOR_FORMAT,
                extent,
                // TRANSFER_DST: under MSAA, vulkano's `single_pass_renderpass!`
                // gives the resolve attachment a `TransferDstOptimal` initial
                // layout, which requires this usage even though we never copy
                // into it (we sample it for the composite).
                usage: ImageUsage::COLOR_ATTACHMENT
                    | ImageUsage::SAMPLED
                    | ImageUsage::TRANSFER_DST,
                ..Default::default()
            },
            device_alloc(),
        )
        .expect("aetna-vulkano: scene resolve image");
        let resolve_view = ImageView::new_default(resolve_image).expect("scene resolve view");

        let depth_image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: SCENE_DEPTH_FORMAT,
                extent,
                samples: SampleCount::try_from(sample_count).unwrap_or(SampleCount::Sample1),
                // SAMPLED so the occlusion resolve pass can read the stored
                // depth back; cheap and unconditional (label-free scenes just
                // never build the resolve resources).
                usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT | ImageUsage::SAMPLED,
                ..Default::default()
            },
            device_alloc(),
        )
        .expect("aetna-vulkano: scene depth image");
        let depth_view = ImageView::new_default(depth_image).expect("scene depth view");

        let attachments = if sample_count > 1 {
            let msaa_image = Image::new(
                self.memory_alloc.clone(),
                ImageCreateInfo {
                    image_type: ImageType::Dim2d,
                    format: SCENE_COLOR_FORMAT,
                    extent,
                    samples: SampleCount::try_from(sample_count).unwrap_or(SampleCount::Sample1),
                    usage: ImageUsage::COLOR_ATTACHMENT,
                    ..Default::default()
                },
                device_alloc(),
            )
            .expect("aetna-vulkano: scene msaa colour image");
            let msaa_view = ImageView::new_default(msaa_image).expect("scene msaa view");
            vec![msaa_view, resolve_view.clone(), depth_view.clone()]
        } else {
            vec![resolve_view.clone(), depth_view.clone()]
        };

        let framebuffer = Framebuffer::new(
            pass.render_pass.clone(),
            FramebufferCreateInfo {
                attachments,
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: scene framebuffer");

        let composite_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            self.composite_pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::image_view(0, resolve_view),
                WriteDescriptorSet::sampler(1, self.sampler.clone()),
            ],
            [],
        )
        .expect("aetna-vulkano: scene composite descriptor set");

        let occlusion = capture_depth
            .then(|| self.build_occlusion_resources(depth_view.clone(), px, sample_count));

        self.targets.insert(
            id.to_string(),
            OffscreenTarget {
                size: px,
                sample_count,
                framebuffer,
                depth_view,
                composite_set,
                occlusion,
                used_frame: self.frame_counter,
            },
        );
    }

    /// Ensure a depth-resolve pipeline exists for `sample_count`.
    fn ensure_resolve_pipeline(&mut self, sample_count: u32) {
        if self.resolve_pipelines.contains_key(&sample_count) {
            return;
        }
        let subpass = Subpass::from(self.resolve_pass.clone(), 0)
            .expect("aetna-vulkano: scene resolve subpass 0");
        let pipeline = build_depth_resolve_pipeline(self.device.clone(), subpass, sample_count);
        self.resolve_pipelines.insert(sample_count, pipeline);
    }

    /// Allocate the per-target occlusion resources: the single-sample
    /// `R32_SFLOAT` resolve image + framebuffer, a host-visible read-back
    /// buffer, and the resolve pipeline's set-0 descriptor over the target's
    /// (sampled) depth view.
    fn build_occlusion_resources(
        &self,
        depth_view: Arc<ImageView>,
        px: (u32, u32),
        sample_count: u32,
    ) -> OcclusionResources {
        let (width, height) = px;
        let color = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: SCENE_OCC_FORMAT,
                extent: [width, height, 1],
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                ..Default::default()
            },
            device_alloc(),
        )
        .expect("aetna-vulkano: scene occlusion image");
        let color_view = ImageView::new_default(color.clone()).expect("scene occlusion view");
        let framebuffer = Framebuffer::new(
            self.resolve_pass.clone(),
            FramebufferCreateInfo {
                attachments: vec![color_view],
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: scene occlusion framebuffer");

        // Tightly-packed (Vulkan image→buffer copies have no 256-byte row
        // alignment requirement), so `width * height` f32s with no padding.
        let readback = Buffer::new_slice::<f32>(
            self.memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                ..Default::default()
            },
            (width * height) as u64,
        )
        .expect("aetna-vulkano: scene occlusion readback buffer");

        let resolve = self
            .resolve_pipelines
            .get(&sample_count)
            .expect("resolve pipeline ensured before occlusion resources");
        let depth_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            resolve.layout().set_layouts()[0].clone(),
            [WriteDescriptorSet::image_view(0, depth_view)],
            [],
        )
        .expect("aetna-vulkano: scene occlusion depth descriptor set");

        OcclusionResources {
            framebuffer,
            color,
            readback,
            depth_set,
            width,
            height,
            state: ReadbackState::Free,
            last_captured: None,
        }
    }

    fn ensure_mesh_geometry(&mut self, draw: &MeshDraw) {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && matches!(c.buffers, GeoBuffers::Mesh { .. })
        {
            c.used_frame = self.frame_counter;
            return;
        }
        let verts = gpu::mesh_vertices(&data);
        let vbuf = Buffer::from_iter(
            self.memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            host_alloc(),
            verts.iter().copied(),
        )
        .expect("aetna-vulkano: scene mesh vbuf");
        let vcount = verts.len() as u32;
        let (ibuf, icount) = match &data.indices {
            Some(indices) if !indices.is_empty() => {
                let ibuf = Buffer::from_iter(
                    self.memory_alloc.clone(),
                    BufferCreateInfo {
                        usage: BufferUsage::INDEX_BUFFER,
                        ..Default::default()
                    },
                    host_alloc(),
                    indices.iter().copied(),
                )
                .expect("aetna-vulkano: scene mesh ibuf");
                (Some(ibuf), indices.len() as u32)
            }
            _ => (None, 0),
        };
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers: GeoBuffers::Mesh {
                    vbuf,
                    ibuf,
                    vcount,
                    icount,
                },
                revision: rev,
                space: self.working,
                used_frame: self.frame_counter,
            },
        );
    }

    fn ensure_point_geometry(&mut self, draw: &PointDraw, working: ColorSpace) {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && c.space == working
            && matches!(c.buffers, GeoBuffers::Points { .. })
        {
            c.used_frame = self.frame_counter;
            return;
        }
        let instances = gpu::point_instances(&data, working);
        let count = instances.len() as u32;
        let ibuf = Buffer::from_iter(
            self.memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            host_alloc(),
            instances,
        )
        .expect("aetna-vulkano: scene point ibuf");
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers: GeoBuffers::Points { ibuf, count },
                revision: rev,
                space: working,
                used_frame: self.frame_counter,
            },
        );
    }

    fn ensure_line_geometry(&mut self, draw: &LineDraw, working: ColorSpace) {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && c.space == working
            && matches!(c.buffers, GeoBuffers::Lines { .. })
        {
            c.used_frame = self.frame_counter;
            return;
        }
        let instances = gpu::line_instances(&data, working);
        let count = instances.len() as u32;
        let ibuf = Buffer::from_iter(
            self.memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            host_alloc(),
            instances,
        )
        .expect("aetna-vulkano: scene line ibuf");
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers: GeoBuffers::Lines { ibuf, count },
                revision: rev,
                space: working,
                used_frame: self.frame_counter,
            },
        );
    }

    /// Upload the per-frame grid + composite instance buffers and drop cache
    /// entries untouched this frame. Mirrors [`SurfacePaint::flush`].
    pub(crate) fn flush(&mut self) {
        let frame = self.frame_counter;
        self.geometry.retain(|_, c| c.used_frame == frame);
        self.targets.retain(|_, t| t.used_frame == frame);

        self.grid_buf = (!self.grid_instances.is_empty()).then(|| {
            let buf = self
                .instance_alloc
                .allocate_slice::<gpu::LineInstance>(self.grid_instances.len() as u64)
                .expect("aetna-vulkano: scene grid suballocate");
            buf.write()
                .expect("aetna-vulkano: scene grid write")
                .copy_from_slice(&self.grid_instances);
            buf
        });
        self.composite_instance_buf = (!self.composite_instances.is_empty()).then(|| {
            let buf = self
                .instance_alloc
                .allocate_slice::<gpu::CompositeInstance>(self.composite_instances.len() as u64)
                .expect("aetna-vulkano: scene composite suballocate");
            buf.write()
                .expect("aetna-vulkano: scene composite write")
                .copy_from_slice(&self.composite_instances);
            buf
        });
    }

    /// Record every recorded scene's offscreen pass into `builder`, ahead of
    /// the runner's main composite pass. Each scene clears its target, draws
    /// grid → meshes → points → lines, and resolves to the sampled texture
    /// the composite reads.
    pub(crate) fn encode_offscreen(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        for run in &self.runs {
            let Some(target) = self.targets.get(&run.target_id) else {
                continue;
            };
            let pass = self
                .passes
                .get(&run.sample_count)
                .expect("pass ensured at record time");

            let clear_values: Vec<Option<ClearValue>> = if run.sample_count > 1 {
                vec![
                    Some(ClearValue::Float(run.clear)),
                    None,
                    Some(ClearValue::Depth(1.0)),
                ]
            } else {
                vec![
                    Some(ClearValue::Float(run.clear)),
                    Some(ClearValue::Depth(1.0)),
                ]
            };

            builder
                .begin_render_pass(
                    RenderPassBeginInfo {
                        clear_values,
                        ..RenderPassBeginInfo::framebuffer(target.framebuffer.clone())
                    },
                    SubpassBeginInfo {
                        contents: SubpassContents::Inline,
                        ..Default::default()
                    },
                )
                .expect("aetna-vulkano: scene begin_render_pass");

            let (w, h) = target.size;
            builder
                .set_viewport(
                    0,
                    smallvec![Viewport {
                        offset: [0.0, 0.0],
                        extent: [w as f32, h as f32],
                        depth_range: 0.0..=1.0,
                    }],
                )
                .expect("scene set_viewport");
            builder
                .set_scissor(
                    0,
                    smallvec![Scissor {
                        offset: [0, 0],
                        extent: [w, h],
                    }],
                )
                .expect("scene set_scissor");

            for cmd in &run.cmds {
                match *cmd {
                    DrawCmd::Grid { slot, first, count } => {
                        let Some(grid) = &self.grid_buf else { continue };
                        bind_set0(builder, &pass.line, &self.uniform_sets[slot]);
                        builder
                            .bind_pipeline_graphics(pass.line.clone())
                            .expect("bind line pipeline");
                        builder
                            .bind_vertex_buffers(0, (self.line_quad_vbo.clone(), grid.clone()))
                            .expect("bind grid buffers");
                        unsafe {
                            builder.draw(4, count, 0, first).expect("draw grid");
                        }
                    }
                    DrawCmd::Points { geo, slot } => {
                        let Some(CachedGeometry {
                            buffers: GeoBuffers::Points { ibuf, count },
                            ..
                        }) = self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        builder
                            .bind_pipeline_graphics(pass.point.clone())
                            .expect("bind point pipeline");
                        bind_set0(builder, &pass.point, &self.uniform_sets[slot]);
                        builder
                            .bind_vertex_buffers(0, (self.point_quad_vbo.clone(), ibuf.clone()))
                            .expect("bind point buffers");
                        unsafe {
                            builder.draw(4, *count, 0, 0).expect("draw points");
                        }
                    }
                    DrawCmd::Lines { geo, slot } => {
                        let Some(CachedGeometry {
                            buffers: GeoBuffers::Lines { ibuf, count },
                            ..
                        }) = self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        builder
                            .bind_pipeline_graphics(pass.line.clone())
                            .expect("bind line pipeline");
                        bind_set0(builder, &pass.line, &self.uniform_sets[slot]);
                        builder
                            .bind_vertex_buffers(0, (self.line_quad_vbo.clone(), ibuf.clone()))
                            .expect("bind line buffers");
                        unsafe {
                            builder.draw(4, *count, 0, 0).expect("draw lines");
                        }
                    }
                    DrawCmd::Mesh { geo, slot } => {
                        let Some(CachedGeometry {
                            buffers:
                                GeoBuffers::Mesh {
                                    vbuf,
                                    ibuf,
                                    vcount,
                                    icount,
                                },
                            ..
                        }) = self.geometry.get(&geo)
                        else {
                            continue;
                        };
                        builder
                            .bind_pipeline_graphics(pass.mesh.clone())
                            .expect("bind mesh pipeline");
                        bind_set0(builder, &pass.mesh, &self.uniform_sets[slot]);
                        builder
                            .bind_vertex_buffers(0, vbuf.clone())
                            .expect("bind mesh vbuf");
                        match ibuf {
                            Some(ibuf) => {
                                builder
                                    .bind_index_buffer(ibuf.clone())
                                    .expect("bind mesh ibuf");
                                unsafe {
                                    builder
                                        .draw_indexed(*icount, 1, 0, 0, 0)
                                        .expect("draw_indexed mesh");
                                }
                            }
                            None => unsafe {
                                builder.draw(*vcount, 1, 0, 0).expect("draw mesh");
                            },
                        }
                    }
                }
            }

            builder
                .end_render_pass(SubpassEndInfo::default())
                .expect("aetna-vulkano: scene end_render_pass");
        }
    }

    /// Resolve + copy each capture-enabled scene's stored depth into its
    /// read-back buffer. Recorded right after [`Self::encode_offscreen`] (the
    /// depth is still alive, stored) and ahead of the runner's main pass —
    /// vulkano's auto-sync inserts the depth-write → shader-read barrier. Only
    /// targets whose buffer is `Free` and whose pose changed capture this
    /// frame; a busy buffer keeps serving its previous map.
    pub(crate) fn encode_depth_capture(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    ) {
        // Snapshot per-run capture info so the immutable `runs` borrow ends
        // before we borrow `targets` mutably.
        let jobs: Vec<(String, ResolvedCamera, Rect, u32)> = self
            .runs
            .iter()
            .filter(|r| r.capture_depth)
            .map(|r| (r.target_id.clone(), r.camera, r.rect, r.sample_count))
            .collect();
        for (id, camera, rect, sample_count) in jobs {
            let Some(resolve) = self.resolve_pipelines.get(&sample_count).cloned() else {
                continue;
            };
            let Some(target) = self.targets.get_mut(&id) else {
                continue;
            };
            let Some(occ) = target.occlusion.as_mut() else {
                continue;
            };
            if !matches!(occ.state, ReadbackState::Free) {
                continue; // a capture is already in flight; reuse it
            }
            if occ.last_captured == Some((camera, rect)) {
                continue; // the current map already matches this pose
            }

            builder
                .begin_render_pass(
                    RenderPassBeginInfo {
                        // Clear to far (1.0) so empty background reads as "no
                        // surface" — same as the wgpu resolve clear.
                        clear_values: vec![Some(ClearValue::Float([1.0, 0.0, 0.0, 0.0]))],
                        ..RenderPassBeginInfo::framebuffer(occ.framebuffer.clone())
                    },
                    SubpassBeginInfo {
                        contents: SubpassContents::Inline,
                        ..Default::default()
                    },
                )
                .expect("aetna-vulkano: occlusion begin_render_pass");
            builder
                .set_viewport(
                    0,
                    smallvec![Viewport {
                        offset: [0.0, 0.0],
                        extent: [occ.width as f32, occ.height as f32],
                        depth_range: 0.0..=1.0,
                    }],
                )
                .expect("occlusion set_viewport");
            builder
                .set_scissor(
                    0,
                    smallvec![Scissor {
                        offset: [0, 0],
                        extent: [occ.width, occ.height],
                    }],
                )
                .expect("occlusion set_scissor");
            builder
                .bind_pipeline_graphics(resolve.clone())
                .expect("bind occlusion pipeline");
            builder
                .bind_descriptor_sets(
                    vulkano::pipeline::PipelineBindPoint::Graphics,
                    resolve.layout().clone(),
                    0,
                    occ.depth_set.clone(),
                )
                .expect("bind occlusion depth set");
            unsafe {
                // Fullscreen triangle (3 verts, no vertex buffers).
                builder.draw(3, 1, 0, 0).expect("draw occlusion resolve");
            }
            builder
                .end_render_pass(SubpassEndInfo::default())
                .expect("aetna-vulkano: occlusion end_render_pass");

            builder
                .copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
                    occ.color.clone(),
                    occ.readback.clone(),
                ))
                .expect("aetna-vulkano: occlusion copy image → buffer");

            occ.state = ReadbackState::Pending { camera, rect };
            occ.last_captured = Some((camera, rect));
        }
    }

    /// Read back any depth captures the previous frame submitted and return
    /// them as [`SceneDepthMap`]s. Called at the top of the host's `prepare`,
    /// after the previous frame's fence has signalled (the aetna host waits on
    /// it), so the host-visible read-back buffer holds completed data.
    pub(crate) fn collect_depth_maps(&mut self) -> Vec<(String, SceneDepthMap)> {
        let mut ready = Vec::new();
        for (id, target) in self.targets.iter_mut() {
            let Some(occ) = target.occlusion.as_mut() else {
                continue;
            };
            let ReadbackState::Pending { camera, rect } = occ.state else {
                continue;
            };
            // Reading host-visible memory; `read()` only errs if vulkano still
            // tracks GPU access to the buffer (host hasn't reclaimed the
            // frame's future yet) — leave it Pending and retry next frame.
            let Ok(guard) = occ.readback.read() else {
                continue;
            };
            let depth: Vec<f32> = guard.to_vec();
            drop(guard);
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
        ready
    }

    /// Whether a target is still alive — lets the host GC stale depth maps for
    /// scenes that left the tree.
    pub(crate) fn has_target(&self, id: &str) -> bool {
        self.targets.contains_key(id)
    }

    /// Whether any recorded scene still needs more frames before its label
    /// occlusion is correct (a capture in flight, or the live pose has no
    /// matching map yet). The host ORs this into the layout-redraw deadline so
    /// the read-back can finish even after the camera settles; returns `false`
    /// once every labelled scene has a current map (and for label-free
    /// scenes), so lazy rendering still idles.
    pub(crate) fn occlusion_unsettled(&self) -> bool {
        self.runs.iter().filter(|r| r.capture_depth).any(|r| {
            match self
                .targets
                .get(&r.target_id)
                .and_then(|t| t.occlusion.as_ref())
            {
                None => true,
                Some(occ) => {
                    !matches!(occ.state, ReadbackState::Free)
                        || occ.last_captured != Some((r.camera, r.rect))
                }
            }
        })
    }
}

fn bind_set0(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    pipeline: &Arc<GraphicsPipeline>,
    set: &Arc<DescriptorSet>,
) {
    builder
        .bind_descriptor_sets(
            vulkano::pipeline::PipelineBindPoint::Graphics,
            pipeline.layout().clone(),
            0,
            set.clone(),
        )
        .expect("aetna-vulkano: scene bind set 0");
}

// ---- host/device allocation presets ----

fn host_alloc() -> AllocationCreateInfo {
    AllocationCreateInfo {
        memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
            | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
        ..Default::default()
    }
}

fn device_alloc() -> AllocationCreateInfo {
    AllocationCreateInfo {
        memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
        ..Default::default()
    }
}

// ---- vertex input layouts (offsets mirror aetna_core::scene::gpu) ----

fn attr(binding: u32, offset: u32, format: Format) -> VertexInputAttributeDescription {
    VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    }
}

fn point_vertex_input() -> VertexInputState {
    VertexInputState::new()
        .binding(
            0,
            VertexInputBindingDescription {
                stride: (4 * std::mem::size_of::<f32>()) as u32,
                input_rate: VertexInputRate::Vertex,
                ..Default::default()
            },
        )
        .binding(
            1,
            VertexInputBindingDescription {
                stride: std::mem::size_of::<gpu::PointInstance>() as u32,
                input_rate: VertexInputRate::Instance { divisor: 1 },
                ..Default::default()
            },
        )
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT)) // corner
        .attribute(1, attr(0, 8, Format::R32G32_SFLOAT)) // uv
        .attribute(2, attr(1, 0, Format::R32G32B32_SFLOAT)) // position
        .attribute(3, attr(1, 12, Format::R32G32B32A32_SFLOAT)) // color
}

fn line_vertex_input() -> VertexInputState {
    VertexInputState::new()
        .binding(
            0,
            VertexInputBindingDescription {
                stride: (2 * std::mem::size_of::<f32>()) as u32,
                input_rate: VertexInputRate::Vertex,
                ..Default::default()
            },
        )
        .binding(
            1,
            VertexInputBindingDescription {
                stride: std::mem::size_of::<gpu::LineInstance>() as u32,
                input_rate: VertexInputRate::Instance { divisor: 1 },
                ..Default::default()
            },
        )
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT)) // corner
        .attribute(1, attr(1, 0, Format::R32G32B32_SFLOAT)) // start
        .attribute(2, attr(1, 12, Format::R32G32B32_SFLOAT)) // end
        .attribute(3, attr(1, 24, Format::R32G32B32A32_SFLOAT)) // color
        .attribute(4, attr(1, 40, Format::R32_SFLOAT)) // width
}

fn mesh_vertex_input() -> VertexInputState {
    VertexInputState::new()
        .binding(
            0,
            VertexInputBindingDescription {
                stride: std::mem::size_of::<gpu::MeshVertexGpu>() as u32,
                input_rate: VertexInputRate::Vertex,
                ..Default::default()
            },
        )
        .attribute(0, attr(0, 0, Format::R32G32B32_SFLOAT)) // position
        .attribute(1, attr(0, 12, Format::R32G32B32_SFLOAT)) // normal
}

// ---- pipeline construction ----

#[allow(clippy::too_many_arguments)]
fn build_scene_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
    name: &str,
    wgsl: &str,
    vertex_input_state: VertexInputState,
    topology: PrimitiveTopology,
    depth_write: bool,
    cull: bool,
) -> Arc<GraphicsPipeline> {
    let module = compile(device.clone(), name, wgsl);
    let vs = module
        .entry_point("vs_main")
        .unwrap_or_else(|| panic!("`{name}` has no vs_main"));
    let fs = module
        .entry_point("fs_main")
        .unwrap_or_else(|| panic!("`{name}` has no fs_main"));
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    // Force set-0 to VERTEX|FRAGMENT so one uniform descriptor set binds into
    // point/line/mesh alike (mirrors the rect pipelines' shared frame set).
    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    let rasterization_state = if cull {
        RasterizationState {
            cull_mode: CullMode::Back,
            front_face: FrontFace::CounterClockwise,
            ..Default::default()
        }
    } else {
        RasterizationState::default()
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(InputAssemblyState {
                topology,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(rasterization_state),
            multisample_state: Some(multisample_state(sample_count)),
            depth_stencil_state: Some(DepthStencilState {
                depth: Some(DepthState {
                    write_enable: depth_write,
                    compare_op: CompareOp::LessOrEqual,
                }),
                ..Default::default()
            }),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(premultiplied_blend()),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap_or_else(|e| panic!("aetna-vulkano: scene pipeline `{name}`: {e:?}"))
}

/// The composite pipeline draws a resolved scene texture into the runner's
/// main pass via the stock `surface` shader (premultiplied). Vertex layout is
/// the unit quad + [`gpu::CompositeInstance`] (rect / matrix / translation).
fn build_composite_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> Arc<GraphicsPipeline> {
    let module = compile(device.clone(), "stock::surface", stock_wgsl::SURFACE);
    let vs = module.entry_point("vs_main").expect("surface vs_main");
    let fs = module.entry_point("fs_premul").expect("surface fs_premul");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    let vertex_input_state = VertexInputState::new()
        .binding(
            0,
            VertexInputBindingDescription {
                stride: (2 * std::mem::size_of::<f32>()) as u32,
                input_rate: VertexInputRate::Vertex,
                ..Default::default()
            },
        )
        .binding(
            1,
            VertexInputBindingDescription {
                stride: std::mem::size_of::<gpu::CompositeInstance>() as u32,
                input_rate: VertexInputRate::Instance { divisor: 1 },
                ..Default::default()
            },
        )
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT)) // rect
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT)) // matrix
        .attribute(3, attr(1, 32, Format::R32G32_SFLOAT)); // translation

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(multisample_state(sample_count)),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(premultiplied_blend()),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .expect("aetna-vulkano: scene composite pipeline")
}

fn compile(device: Arc<Device>, name: &str, wgsl: &str) -> Arc<ShaderModule> {
    let words = wgsl_to_spirv(name, wgsl).unwrap_or_else(|e| panic!("WGSL compile `{name}`: {e}"));
    // SAFETY: SPIR-V is the verified output of naga's validator + spv emitter.
    unsafe {
        ShaderModule::new(device, ShaderModuleCreateInfo::new(&words))
            .unwrap_or_else(|e| panic!("ShaderModule::new `{name}`: {e}"))
    }
}

fn premultiplied_blend() -> AttachmentBlend {
    AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    }
}

// ---- label-occlusion depth resolve ----

/// WGSL for the depth-resolve pass: a fullscreen triangle that reads the
/// scene depth (sample 0) and writes it to an `R32Float` target. The depth
/// binding type differs for MSAA vs single-sample, so it's templated. Mirrors
/// the wgpu backend's `resolve_wgsl`.
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

/// Single-sample `R32_SFLOAT` render pass the depth-resolve fullscreen
/// triangle draws into. One colour attachment, cleared to far each capture.
fn build_resolve_render_pass(device: Arc<Device>) -> Arc<RenderPass> {
    vulkano::single_pass_renderpass!(
        device,
        attachments: {
            occ: {
                format: SCENE_OCC_FORMAT,
                samples: 1,
                load_op: Clear,
                store_op: Store,
            },
        },
        pass: {
            color: [occ],
            depth_stencil: {},
        },
    )
    .expect("aetna-vulkano: scene resolve render pass")
}

/// Build the depth-resolve pipeline for one MSAA sample count. The layout
/// comes straight from reflection — set 0 is a single sampled depth image,
/// fragment-only, so no stage broadening is needed (unlike the scene
/// pipelines' shared uniform set).
fn build_depth_resolve_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    sample_count: u32,
) -> Arc<GraphicsPipeline> {
    let module = compile(
        device.clone(),
        "stock::scene_depth_resolve",
        &resolve_wgsl(sample_count > 1),
    );
    let vs = module.entry_point("vs_main").expect("resolve vs_main");
    let fs = module.entry_point("fs_main").expect("resolve fs_main");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = PipelineLayout::new(
        device.clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
            .into_pipeline_layout_create_info(device.clone())
            .expect("aetna-vulkano: resolve layout from stages"),
    )
    .expect("aetna-vulkano: resolve pipeline layout");

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            // Fullscreen triangle generated from the vertex index — no inputs.
            vertex_input_state: Some(VertexInputState::new()),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            // The resolve target is always single-sample.
            multisample_state: Some(multisample_state(1)),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState::default(),
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap_or_else(|e| panic!("aetna-vulkano: depth-resolve pipeline: {e:?}"))
}

/// Build the scene's offscreen render pass: one fp16 colour subpass with a
/// depth attachment, optionally multisampled with a single-sample resolve.
fn build_scene_render_pass(device: Arc<Device>, sample_count: u32) -> Arc<RenderPass> {
    if sample_count <= 1 {
        vulkano::single_pass_renderpass!(
            device,
            attachments: {
                color: {
                    format: SCENE_COLOR_FORMAT,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
                depth: {
                    format: SCENE_DEPTH_FORMAT,
                    samples: 1,
                    load_op: Clear,
                    // Stored (not DontCare) so the occlusion resolve pass can
                    // sample it for label depth-occlusion.
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: { depth },
            },
        )
        .expect("aetna-vulkano: scene single-sample render pass")
    } else {
        vulkano::single_pass_renderpass!(
            device,
            attachments: {
                color_msaa: {
                    format: SCENE_COLOR_FORMAT,
                    samples: sample_count,
                    load_op: Clear,
                    store_op: Store,
                },
                color_resolve: {
                    format: SCENE_COLOR_FORMAT,
                    samples: 1,
                    load_op: DontCare,
                    store_op: Store,
                },
                depth: {
                    format: SCENE_DEPTH_FORMAT,
                    samples: sample_count,
                    load_op: Clear,
                    // Stored so the occlusion resolve pass can sample it (the
                    // multisampled depth, via `texture_depth_multisampled_2d`).
                    store_op: Store,
                },
            },
            pass: {
                color: [color_msaa],
                color_resolve: [color_resolve],
                depth_stencil: { depth },
            },
        )
        .expect("aetna-vulkano: scene multisample render pass")
    }
}
