//! GPU rendering for [`DrawOp::Scene3D`](damascene_core::ir::DrawOp::Scene3D) on
//! the ash backend. Mirrors `damascene-wgpu`/`damascene-vulkano`'s `scene.rs`.
//!
//! A 3D scene is a two-phase draw (offscreen render → composite), the same
//! shape as the other backends. The ash specifics:
//!
//! - **No render passes** — everything is Vulkan 1.3 dynamic rendering
//!   (`cmd_begin_rendering`). Each scene transitions its offscreen images by
//!   hand, opens a rendering scope (multisampled fp16 colour + D32 depth,
//!   resolving to a single-sample sampled image), draws grid/mesh/points/
//!   lines, ends the scope, and barriers the resolved image to
//!   `SHADER_READ_ONLY` for the composite. All of this is recorded ahead of
//!   the runner's main rendering scope (scopes can't nest).
//! - **Per-draw uniforms** ride a single `UNIFORM_BUFFER_DYNAMIC` descriptor
//!   set bound with a per-draw dynamic offset — the same effect as wgpu's
//!   dynamic-offset uniforms, without allocating a descriptor set per draw.
//! - **Composite** reuses the stock `surface` shader (fs_premul) into the
//!   runner's main pass, exactly like the AppTexture path.
//!
//! Backend-neutral byte layouts + CPU packing come from
//! [`damascene_core::scene::gpu`]; only GPU resource management lives here.

use std::collections::HashMap;
use std::ffi::CString;
use std::ops::Range;

use damascene_core::color::ColorSpace;
use damascene_core::paint::PhysicalScissor;
use damascene_core::scene::{
    LineDraw, MeshDraw, PointDraw, ResolvedCamera, Scene3DData, SceneDepthMap, gpu,
};
use damascene_core::shader::stock_wgsl;
use damascene_core::tree::Rect;
use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocator;

use crate::buffer::{GpuBuffer, GpuImage};
use crate::naga_compile::wgsl_to_spirv;
use crate::runner::{Error, Result, TargetInfo};

const SCENE_COLOR_FORMAT: vk::Format = vk::Format::R16G16B16A16_SFLOAT;
const SCENE_DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;
/// Single-channel float the scene depth resolves into for label-occlusion
/// read-back; stores normalised device depth in `[0, 1]`. Mirrors the other
/// backends' `SCENE_OCC_FORMAT`.
const SCENE_OCC_FORMAT: vk::Format = vk::Format::R32_SFLOAT;
/// Per-draw uniform slot stride. 256 ≥ every scene uniform (MeshUniform is
/// 224) and is a multiple of any real `minUniformBufferOffsetAlignment`, so
/// `slot * STRIDE` is always a valid dynamic offset.
const UNIFORM_STRIDE: usize = 256;

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
        vbuf: GpuBuffer,
        ibuf: Option<GpuBuffer>,
        vcount: u32,
        icount: u32,
    },
    Points {
        ibuf: GpuBuffer,
        count: u32,
    },
    Lines {
        ibuf: GpuBuffer,
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
    /// Multisampled colour, `None` when `sample_count == 1`.
    msaa: Option<GpuImage>,
    /// Single-sample colour the composite samples.
    resolve: GpuImage,
    depth: GpuImage,
    composite_set: vk::DescriptorSet,
    /// Depth read-back resources, allocated the first frame this target's
    /// scene asks for label occlusion. `None` for label-free scenes.
    occlusion: Option<OcclusionResources>,
    used_frame: u64,
}

// ---- label-occlusion depth read-back ----

/// Per-target resources for capturing the scene depth and streaming it to the
/// CPU. The (possibly multisampled) depth is resolved into a single-sample
/// [`SCENE_OCC_FORMAT`] image by a fullscreen triangle, copied into `readback`,
/// and read one frame later. Like vulkano (and unlike wgpu's `map_async`),
/// there's no async map: the host waits on each frame's fence before the next
/// `prepare`, so the copy is complete when [`Scene3DPaint::collect_depth_maps`]
/// reads the host-visible buffer.
struct OcclusionResources {
    /// Single-sample `R32_SFLOAT` image the fullscreen triangle writes.
    color: GpuImage,
    /// Host-visible buffer the resolved depth is copied into.
    readback: GpuBuffer,
    /// Resolve pipeline's set 0: the target's depth view, sampled.
    depth_set: vk::DescriptorSet,
    width: u32,
    height: u32,
    state: ReadbackState,
    /// The (camera, rect) of the most recent capture; a fresh capture only
    /// fires when the pose changes, so a settled scene goes idle.
    last_captured: Option<(ResolvedCamera, Rect)>,
}

/// Lifecycle of one target's read-back buffer. The host fence-waits each
/// frame, so a copy encoded this frame is readable next `prepare` (no async
/// map intermediate, unlike wgpu).
enum ReadbackState {
    Free,
    Pending { camera: ResolvedCamera, rect: Rect },
}

struct ScenePipelines {
    point: vk::Pipeline,
    line: vk::Pipeline,
    mesh: vk::Pipeline,
}

pub(crate) struct Scene3DPaint {
    working: ColorSpace,

    point_quad_vbo: GpuBuffer,
    line_quad_vbo: GpuBuffer,

    scene_uniform_layout: vk::DescriptorSetLayout,
    scene_pipeline_layout: vk::PipelineLayout,
    passes: HashMap<u32, ScenePipelines>,

    // Depth-resolve (label occlusion): set 0 = one sampled depth image.
    resolve_set_layout: vk::DescriptorSetLayout,
    resolve_pipeline_layout: vk::PipelineLayout,
    /// Depth-resolve pipelines keyed by sample count (the depth binding is
    /// multisampled vs not, so the pipeline differs).
    resolve_pipelines: HashMap<u32, vk::Pipeline>,
    /// Pool the per-target depth descriptor sets come from.
    occ_pool: vk::DescriptorPool,

    uniform_buf: GpuBuffer,
    uniform_capacity_slots: usize,
    uniform_pool: vk::DescriptorPool,
    uniform_set: vk::DescriptorSet,

    composite_set_layout: vk::DescriptorSetLayout,
    composite_pipeline_layout: vk::PipelineLayout,
    composite_pipeline: vk::Pipeline,
    composite_pool: vk::DescriptorPool,
    sampler: vk::Sampler,

    grid_buf: GpuBuffer,
    grid_capacity: usize,
    composite_inst_buf: GpuBuffer,
    composite_inst_capacity: usize,

    // Per-frame scratch (cleared in `frame_begin`).
    uniform_bytes: Vec<u8>,
    uniform_slots: usize,
    grid_instances: Vec<gpu::LineInstance>,
    composite_instances: Vec<gpu::CompositeInstance>,
    runs: Vec<Scene3DRun>,

    geometry: HashMap<u64, CachedGeometry>,
    targets: HashMap<String, OffscreenTarget>,
    frame_counter: u64,
}

const INITIAL_UNIFORM_SLOTS: usize = 32;
const INITIAL_GRID_CAP: usize = 256;
const INITIAL_COMPOSITE_CAP: usize = 8;
const MAX_TARGETS: u32 = 64;

impl Scene3DPaint {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        frame_set_layout: vk::DescriptorSetLayout,
        target: TargetInfo,
        working: ColorSpace,
    ) -> Result<Self> {
        let mut point_quad_vbo = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::scene_point_quad",
            (16 * std::mem::size_of::<f32>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        point_quad_vbo.write_bytes(bytemuck::cast_slice::<f32, u8>(&[
            -1.0, -1.0, 0.0, 0.0, // bl
            1.0, -1.0, 1.0, 0.0, // br
            -1.0, 1.0, 0.0, 1.0, // tl
            1.0, 1.0, 1.0, 1.0, // tr
        ]))?;
        let mut line_quad_vbo = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::scene_line_quad",
            (8 * std::mem::size_of::<f32>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        line_quad_vbo.write_bytes(bytemuck::cast_slice::<f32, u8>(&[
            0.0, -1.0, // start left
            1.0, -1.0, // end left
            0.0, 1.0, // start right
            1.0, 1.0, // end right
        ]))?;

        // Scene uniform: one dynamic uniform buffer at set 0, binding 0.
        let scene_uniform_layout = {
            let binding = vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT);
            let bindings = [binding];
            let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
            unsafe { device.create_descriptor_set_layout(&info, None) }?
        };
        let scene_pipeline_layout = {
            let layouts = [scene_uniform_layout];
            let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts);
            unsafe { device.create_pipeline_layout(&info, None) }?
        };

        // Depth-resolve: one sampled depth image at set 0, binding 0 (the
        // fullscreen triangle reads it with textureLoad — no sampler).
        let resolve_set_layout = {
            let binding = vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT);
            let bindings = [binding];
            let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
            unsafe { device.create_descriptor_set_layout(&info, None) }?
        };
        let resolve_pipeline_layout = {
            let layouts = [resolve_set_layout];
            let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts);
            unsafe { device.create_pipeline_layout(&info, None) }?
        };
        let occ_pool = {
            let size = vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLED_IMAGE,
                descriptor_count: MAX_TARGETS,
            };
            let sizes = [size];
            let info = vk::DescriptorPoolCreateInfo::default()
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                .max_sets(MAX_TARGETS)
                .pool_sizes(&sizes);
            unsafe { device.create_descriptor_pool(&info, None) }?
        };

        let uniform_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::scene_uniforms",
            (INITIAL_UNIFORM_SLOTS * UNIFORM_STRIDE) as vk::DeviceSize,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        let uniform_pool = {
            let size = vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
                descriptor_count: 1,
            };
            let sizes = [size];
            let info = vk::DescriptorPoolCreateInfo::default()
                .max_sets(1)
                .pool_sizes(&sizes);
            unsafe { device.create_descriptor_pool(&info, None) }?
        };
        let uniform_set = {
            let layouts = [scene_uniform_layout];
            let info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(uniform_pool)
                .set_layouts(&layouts);
            unsafe { device.allocate_descriptor_sets(&info) }?[0]
        };
        update_uniform_set(device, uniform_set, &uniform_buf);

        // Composite: surface shader (fs_premul), set 0 = FrameUniforms (the
        // runner's), set 1 = resolved scene texture + sampler.
        let composite_set_layout = {
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
            unsafe { device.create_descriptor_set_layout(&info, None) }?
        };
        let composite_pipeline_layout = {
            let layouts = [frame_set_layout, composite_set_layout];
            let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts);
            unsafe { device.create_pipeline_layout(&info, None) }?
        };
        let composite_pipeline =
            build_composite_pipeline(device, composite_pipeline_layout, target)?;
        let composite_pool = {
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLED_IMAGE,
                    descriptor_count: MAX_TARGETS,
                },
                vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::SAMPLER,
                    descriptor_count: MAX_TARGETS,
                },
            ];
            let info = vk::DescriptorPoolCreateInfo::default()
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                .max_sets(MAX_TARGETS)
                .pool_sizes(&sizes);
            unsafe { device.create_descriptor_pool(&info, None) }?
        };
        let sampler = {
            let info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
            unsafe { device.create_sampler(&info, None) }?
        };

        let grid_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::scene_grid",
            (INITIAL_GRID_CAP * std::mem::size_of::<gpu::LineInstance>()) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        let composite_inst_buf = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::scene_composite_instances",
            (INITIAL_COMPOSITE_CAP * std::mem::size_of::<gpu::CompositeInstance>())
                as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;

        Ok(Self {
            working,
            point_quad_vbo,
            line_quad_vbo,
            scene_uniform_layout,
            scene_pipeline_layout,
            passes: HashMap::new(),
            resolve_set_layout,
            resolve_pipeline_layout,
            resolve_pipelines: HashMap::new(),
            occ_pool,
            uniform_buf,
            uniform_capacity_slots: INITIAL_UNIFORM_SLOTS,
            uniform_pool,
            uniform_set,
            composite_set_layout,
            composite_pipeline_layout,
            composite_pipeline,
            composite_pool,
            sampler,
            grid_buf,
            grid_capacity: INITIAL_GRID_CAP,
            composite_inst_buf,
            composite_inst_capacity: INITIAL_COMPOSITE_CAP,
            uniform_bytes: Vec::new(),
            uniform_slots: 0,
            grid_instances: Vec::new(),
            composite_instances: Vec::new(),
            runs: Vec::new(),
            geometry: HashMap::new(),
            targets: HashMap::new(),
            frame_counter: 0,
        })
    }

    pub(crate) fn frame_begin(&mut self) {
        self.uniform_bytes.clear();
        self.uniform_slots = 0;
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

    pub(crate) fn composite_pipeline(&self) -> vk::Pipeline {
        self.composite_pipeline
    }

    pub(crate) fn composite_pipeline_layout(&self) -> vk::PipelineLayout {
        self.composite_pipeline_layout
    }

    pub(crate) fn composite_descriptor(&self, run: &Scene3DRun) -> vk::DescriptorSet {
        self.targets
            .get(&run.target_id)
            .expect("scene target alive for the frame")
            .composite_set
    }

    pub(crate) fn composite_instance_buffer(&self) -> vk::Buffer {
        self.composite_inst_buf.buffer
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        id: &str,
        scene: &Scene3DData,
        scale_factor: f32,
    ) -> Result<Range<usize>> {
        let start = self.runs.len();
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return Ok(start..start);
        }
        let px = (
            (rect.w * scale_factor).round().max(1.0) as u32,
            (rect.h * scale_factor).round().max(1.0) as u32,
        );
        let sample_count = scene.style.msaa_samples.max(1);
        self.ensure_pass(device, sample_count)?;
        if scene.capture_depth {
            self.ensure_resolve_pipeline(device, sample_count)?;
        }
        self.ensure_target(device, allocator, id, px, sample_count, scene.capture_depth)?;

        let aspect = px.0 as f32 / px.1 as f32;
        let view_proj = scene.camera.view_proj(aspect);
        let screen = [px.0 as f32, px.1 as f32];
        let working = self.working;

        let mut cmds = Vec::new();

        let first = self.grid_instances.len() as u32;
        gpu::build_grid_lines(&scene.style, working, &mut self.grid_instances);
        let count = self.grid_instances.len() as u32 - first;
        if count > 0 {
            let slot = self.push_uniform(gpu::grid_uniform(view_proj, screen));
            cmds.push(DrawCmd::Grid { slot, first, count });
        }

        for m in &scene.meshes {
            self.ensure_mesh_geometry(device, allocator, m)?;
            let slot = self.push_uniform(gpu::mesh_uniform(view_proj, m, scene, working));
            cmds.push(DrawCmd::Mesh {
                geo: m.geometry.id().0,
                slot,
            });
        }
        for p in &scene.points {
            self.ensure_point_geometry(device, allocator, p, working)?;
            let slot = self.push_uniform(gpu::point_uniform(view_proj * p.transform, screen, p));
            cmds.push(DrawCmd::Points {
                geo: p.geometry.id().0,
                slot,
            });
        }
        for l in &scene.lines {
            self.ensure_line_geometry(device, allocator, l, working)?;
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
                let [r, g, b, a] = damascene_core::paint::rgba_f32_in(c, working);
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
        Ok(start..self.runs.len())
    }

    fn ensure_resolve_pipeline(&mut self, device: &ash::Device, sample_count: u32) -> Result<()> {
        if self.resolve_pipelines.contains_key(&sample_count) {
            return Ok(());
        }
        let pipeline =
            build_depth_resolve_pipeline(device, self.resolve_pipeline_layout, sample_count > 1)?;
        self.resolve_pipelines.insert(sample_count, pipeline);
        Ok(())
    }

    fn push_uniform<T: bytemuck::Pod>(&mut self, u: T) -> usize {
        let slot = self.uniform_slots;
        let off = slot * UNIFORM_STRIDE;
        self.uniform_bytes.resize(off + UNIFORM_STRIDE, 0);
        self.uniform_bytes[off..off + std::mem::size_of::<T>()]
            .copy_from_slice(bytemuck::bytes_of(&u));
        self.uniform_slots += 1;
        slot
    }

    fn ensure_pass(&mut self, device: &ash::Device, sample_count: u32) -> Result<()> {
        if self.passes.contains_key(&sample_count) {
            return Ok(());
        }
        let samples = sample_flags(sample_count);
        let point = build_scene_pipeline(
            device,
            self.scene_pipeline_layout,
            samples,
            "stock::scene_point",
            stock_wgsl::SCENE_POINT,
            ScenePipelineKind::Point,
        )?;
        let line = build_scene_pipeline(
            device,
            self.scene_pipeline_layout,
            samples,
            "stock::scene_line",
            stock_wgsl::SCENE_LINE,
            ScenePipelineKind::Line,
        )?;
        let mesh = build_scene_pipeline(
            device,
            self.scene_pipeline_layout,
            samples,
            "stock::scene_mesh",
            stock_wgsl::SCENE_MESH,
            ScenePipelineKind::Mesh,
        )?;
        self.passes
            .insert(sample_count, ScenePipelines { point, line, mesh });
        Ok(())
    }

    fn ensure_target(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        id: &str,
        px: (u32, u32),
        sample_count: u32,
        capture_depth: bool,
    ) -> Result<()> {
        if let Some(t) = self.targets.get(id)
            && t.size == px
            && t.sample_count == sample_count
        {
            // Allocate occlusion resources lazily if the scene only now asked
            // for labels. Read what we need and drop the borrow before
            // `build_occlusion_resources` re-borrows `self`.
            let need_occ = capture_depth && t.occlusion.is_none();
            let depth_view = t.depth.view;
            if need_occ {
                let occ = self.build_occlusion_resources(
                    device,
                    allocator,
                    depth_view,
                    px,
                    sample_count,
                )?;
                self.targets
                    .get_mut(id)
                    .expect("target just matched")
                    .occlusion = Some(occ);
            }
            self.targets
                .get_mut(id)
                .expect("target just matched")
                .used_frame = self.frame_counter;
            return Ok(());
        }
        if let Some(mut old) = self.targets.remove(id) {
            unsafe { old.destroy(device, allocator, self.composite_pool, self.occ_pool) };
        }
        let extent = vk::Extent2D {
            width: px.0,
            height: px.1,
        };
        let samples = sample_flags(sample_count);
        let resolve = GpuImage::new(
            device,
            allocator,
            "damascene_ash::scene_resolve",
            SCENE_COLOR_FORMAT,
            extent,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
        )?;
        let depth = GpuImage::new_attachment(
            device,
            allocator,
            "damascene_ash::scene_depth",
            SCENE_DEPTH_FORMAT,
            extent,
            // SAMPLED so the occlusion resolve pass can read the stored depth;
            // cheap and unconditional (label-free scenes never build the
            // resolve resources).
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            samples,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let msaa = if sample_count > 1 {
            Some(GpuImage::new_attachment(
                device,
                allocator,
                "damascene_ash::scene_msaa",
                SCENE_COLOR_FORMAT,
                extent,
                vk::ImageUsageFlags::COLOR_ATTACHMENT,
                samples,
                vk::ImageAspectFlags::COLOR,
            )?)
        } else {
            None
        };

        let composite_set = {
            let layouts = [self.composite_set_layout];
            let info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.composite_pool)
                .set_layouts(&layouts);
            let set = unsafe { device.allocate_descriptor_sets(&info) }?[0];
            let image_info = vk::DescriptorImageInfo::default()
                .image_view(resolve.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
            let sampler_info = vk::DescriptorImageInfo::default().sampler(self.sampler);
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(std::slice::from_ref(&image_info)),
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .image_info(std::slice::from_ref(&sampler_info)),
            ];
            unsafe { device.update_descriptor_sets(&writes, &[]) };
            set
        };

        let occlusion = if capture_depth {
            Some(self.build_occlusion_resources(device, allocator, depth.view, px, sample_count)?)
        } else {
            None
        };

        self.targets.insert(
            id.to_string(),
            OffscreenTarget {
                size: px,
                sample_count,
                msaa,
                resolve,
                depth,
                composite_set,
                occlusion,
                used_frame: self.frame_counter,
            },
        );
        Ok(())
    }

    /// Allocate the per-target occlusion resources: a single-sample
    /// `R32_SFLOAT` image, a host-visible read-back buffer, and the resolve
    /// pipeline's set-0 descriptor over the target's (sampled) depth view.
    fn build_occlusion_resources(
        &self,
        device: &ash::Device,
        allocator: &mut Allocator,
        depth_view: vk::ImageView,
        px: (u32, u32),
        _sample_count: u32,
    ) -> Result<OcclusionResources> {
        let (width, height) = px;
        let color = GpuImage::new(
            device,
            allocator,
            "damascene_ash::scene_occlusion",
            SCENE_OCC_FORMAT,
            vk::Extent2D { width, height },
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        )?;
        // Tightly packed (Vulkan image→buffer copies have no row-alignment
        // requirement), so `width * height` f32s with no padding.
        let readback = GpuBuffer::new(
            device,
            allocator,
            "damascene_ash::scene_occlusion_readback",
            (width * height * 4) as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuToCpu,
        )?;
        let depth_set = {
            let layouts = [self.resolve_set_layout];
            let info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.occ_pool)
                .set_layouts(&layouts);
            let set = unsafe { device.allocate_descriptor_sets(&info) }?[0];
            let image_info = vk::DescriptorImageInfo::default()
                .image_view(depth_view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
            let write = vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(std::slice::from_ref(&image_info));
            unsafe { device.update_descriptor_sets(&[write], &[]) };
            set
        };
        Ok(OcclusionResources {
            color,
            readback,
            depth_set,
            width,
            height,
            state: ReadbackState::Free,
            last_captured: None,
        })
    }

    fn ensure_mesh_geometry(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        draw: &MeshDraw,
    ) -> Result<()> {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && matches!(c.buffers, GeoBuffers::Mesh { .. })
        {
            c.used_frame = self.frame_counter;
            return Ok(());
        }
        let verts = gpu::mesh_vertices(&data);
        let vbuf = upload_buffer(
            device,
            allocator,
            "damascene_ash::scene_mesh_vbuf",
            bytemuck::cast_slice(&verts),
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        let (ibuf, icount) = match &data.indices {
            Some(indices) if !indices.is_empty() => {
                let ibuf = upload_buffer(
                    device,
                    allocator,
                    "damascene_ash::scene_mesh_ibuf",
                    bytemuck::cast_slice(indices),
                    vk::BufferUsageFlags::INDEX_BUFFER,
                )?;
                (Some(ibuf), indices.len() as u32)
            }
            _ => (None, 0),
        };
        self.replace_geometry(
            device,
            allocator,
            id,
            GeoBuffers::Mesh {
                vbuf,
                ibuf,
                vcount: verts.len() as u32,
                icount,
            },
            rev,
            self.working,
        );
        Ok(())
    }

    fn ensure_point_geometry(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        draw: &PointDraw,
        working: ColorSpace,
    ) -> Result<()> {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && c.space == working
            && matches!(c.buffers, GeoBuffers::Points { .. })
        {
            c.used_frame = self.frame_counter;
            return Ok(());
        }
        let instances = gpu::point_instances(&data, working);
        let ibuf = upload_buffer(
            device,
            allocator,
            "damascene_ash::scene_point_ibuf",
            bytemuck::cast_slice(&instances),
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        self.replace_geometry(
            device,
            allocator,
            id,
            GeoBuffers::Points {
                ibuf,
                count: instances.len() as u32,
            },
            rev,
            working,
        );
        Ok(())
    }

    fn ensure_line_geometry(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        draw: &LineDraw,
        working: ColorSpace,
    ) -> Result<()> {
        let id = draw.geometry.id().0;
        let (data, rev) = draw.geometry.snapshot();
        if let Some(c) = self.geometry.get_mut(&id)
            && c.revision == rev
            && c.space == working
            && matches!(c.buffers, GeoBuffers::Lines { .. })
        {
            c.used_frame = self.frame_counter;
            return Ok(());
        }
        let instances = gpu::line_instances(&data, working);
        let ibuf = upload_buffer(
            device,
            allocator,
            "damascene_ash::scene_line_ibuf",
            bytemuck::cast_slice(&instances),
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        self.replace_geometry(
            device,
            allocator,
            id,
            GeoBuffers::Lines {
                ibuf,
                count: instances.len() as u32,
            },
            rev,
            working,
        );
        Ok(())
    }

    fn replace_geometry(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        id: u64,
        buffers: GeoBuffers,
        revision: u64,
        space: ColorSpace,
    ) {
        if let Some(mut old) = self.geometry.remove(&id) {
            unsafe { old.buffers.destroy(device, allocator) };
        }
        self.geometry.insert(
            id,
            CachedGeometry {
                buffers,
                revision,
                space,
                used_frame: self.frame_counter,
            },
        );
    }

    /// Upload per-frame grid + composite instances, the uniform block, and
    /// drop cache entries untouched this frame.
    pub(crate) fn flush(&mut self, device: &ash::Device, allocator: &mut Allocator) -> Result<()> {
        let frame = self.frame_counter;

        // Evict geometry/targets untouched this frame.
        let stale_geo: Vec<u64> = self
            .geometry
            .iter()
            .filter(|(_, c)| c.used_frame != frame)
            .map(|(id, _)| *id)
            .collect();
        for id in stale_geo {
            if let Some(mut c) = self.geometry.remove(&id) {
                unsafe { c.buffers.destroy(device, allocator) };
            }
        }
        let stale_targets: Vec<String> = self
            .targets
            .iter()
            .filter(|(_, t)| t.used_frame != frame)
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale_targets {
            if let Some(mut t) = self.targets.remove(&id) {
                unsafe { t.destroy(device, allocator, self.composite_pool, self.occ_pool) };
            }
        }

        // Grow + write the dynamic uniform buffer.
        let need_slots = self.uniform_slots.max(1);
        if need_slots > self.uniform_capacity_slots {
            let mut next = self.uniform_capacity_slots.max(1);
            while next < need_slots {
                next *= 2;
            }
            unsafe { self.uniform_buf.destroy(device, allocator) };
            self.uniform_buf = GpuBuffer::new(
                device,
                allocator,
                "damascene_ash::scene_uniforms",
                (next * UNIFORM_STRIDE) as vk::DeviceSize,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                MemoryLocation::CpuToGpu,
            )?;
            self.uniform_capacity_slots = next;
            update_uniform_set(device, self.uniform_set, &self.uniform_buf);
        }
        if !self.uniform_bytes.is_empty() {
            self.uniform_buf.write_bytes(&self.uniform_bytes)?;
        }

        grow_and_write(
            device,
            allocator,
            &mut self.grid_buf,
            &mut self.grid_capacity,
            "damascene_ash::scene_grid",
            bytemuck::cast_slice(&self.grid_instances),
            self.grid_instances.len(),
            std::mem::size_of::<gpu::LineInstance>(),
        )?;
        grow_and_write(
            device,
            allocator,
            &mut self.composite_inst_buf,
            &mut self.composite_inst_capacity,
            "damascene_ash::scene_composite_instances",
            bytemuck::cast_slice(&self.composite_instances),
            self.composite_instances.len(),
            std::mem::size_of::<gpu::CompositeInstance>(),
        )?;
        Ok(())
    }

    /// Record every scene's offscreen pass (one dynamic-rendering scope each)
    /// ahead of the runner's main scope, then leave each resolved image ready
    /// to sample for the composite.
    ///
    /// # Safety
    /// `cmd` must be recording, graphics-capable, and outside any
    /// dynamic-rendering scope.
    pub(crate) unsafe fn encode_offscreen(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        for run in &self.runs {
            let Some(target) = self.targets.get(&run.target_id) else {
                continue;
            };
            let Some(pass) = self.passes.get(&run.sample_count) else {
                continue;
            };
            let (w, h) = target.size;
            let extent = vk::Extent2D {
                width: w,
                height: h,
            };

            unsafe {
                if let Some(msaa) = &target.msaa {
                    barrier(
                        device,
                        cmd,
                        msaa.image,
                        vk::ImageAspectFlags::COLOR,
                        vk::ImageLayout::UNDEFINED,
                        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    );
                }
                barrier(
                    device,
                    cmd,
                    target.resolve.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                );
                barrier(
                    device,
                    cmd,
                    target.depth.image,
                    vk::ImageAspectFlags::DEPTH,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                );
            }

            let mut color = vk::RenderingAttachmentInfo::default()
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    color: vk::ClearColorValue { float32: run.clear },
                });
            color = match &target.msaa {
                Some(msaa) => color
                    .image_view(msaa.view)
                    .resolve_mode(vk::ResolveModeFlags::AVERAGE)
                    .resolve_image_view(target.resolve.view)
                    .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
                None => color.image_view(target.resolve.view),
            };
            let color_attachments = [color];
            let depth = vk::RenderingAttachmentInfo::default()
                .image_view(target.depth.view)
                .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                // Stored (not DontCare) so the occlusion resolve pass can
                // sample it for label depth-occlusion.
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                });
            let rendering_info = vk::RenderingInfo::default()
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent,
                })
                .layer_count(1)
                .color_attachments(&color_attachments)
                .depth_attachment(&depth);

            unsafe {
                device.cmd_begin_rendering(cmd, &rendering_info);
                let viewport = vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: w as f32,
                    height: h as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                };
                device.cmd_set_viewport(cmd, 0, &[viewport]);
                device.cmd_set_scissor(
                    cmd,
                    0,
                    &[vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent,
                    }],
                );

                for c in &run.cmds {
                    self.encode_cmd(device, cmd, pass, target, c);
                }

                device.cmd_end_rendering(cmd);

                // Resolved colour is now sampled by the composite.
                barrier(
                    device,
                    cmd,
                    target.resolve.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
            }
        }
    }

    /// Resolve + copy each capture-enabled scene's stored depth into its
    /// read-back buffer. Recorded right after [`Self::encode_offscreen`] (the
    /// depth is still stored from the offscreen pass) and ahead of the
    /// runner's main scope. Only targets whose buffer is `Free` and whose pose
    /// changed capture this frame; a busy buffer keeps serving its prior map.
    ///
    /// # Safety
    /// `cmd` must be recording, graphics-capable, and outside any
    /// dynamic-rendering scope.
    pub(crate) unsafe fn encode_depth_capture(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
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
            let Some(&resolve_pipeline) = self.resolve_pipelines.get(&sample_count) else {
                continue;
            };
            let resolve_layout = self.resolve_pipeline_layout;
            let Some(target) = self.targets.get_mut(&id) else {
                continue;
            };
            let depth_image = target.depth.image;
            let Some(occ) = target.occlusion.as_mut() else {
                continue;
            };
            if !matches!(occ.state, ReadbackState::Free) {
                continue; // a capture is already in flight; reuse it
            }
            if occ.last_captured == Some((camera, rect)) {
                continue; // the current map already matches this pose
            }
            let (w, h) = (occ.width, occ.height);
            let extent = vk::Extent2D {
                width: w,
                height: h,
            };

            unsafe {
                // Depth: attachment-write → shader-read; occlusion colour:
                // undefined → colour attachment.
                barrier(
                    device,
                    cmd,
                    depth_image,
                    vk::ImageAspectFlags::DEPTH,
                    vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                );
                barrier(
                    device,
                    cmd,
                    occ.color.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                );

                let color = vk::RenderingAttachmentInfo::default()
                    .image_view(occ.color.view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    // Clear to far (1.0) so empty background reads "no surface".
                    .clear_value(vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [1.0, 0.0, 0.0, 0.0],
                        },
                    });
                let color_attachments = [color];
                let rendering_info = vk::RenderingInfo::default()
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent,
                    })
                    .layer_count(1)
                    .color_attachments(&color_attachments);
                device.cmd_begin_rendering(cmd, &rendering_info);
                device.cmd_set_viewport(
                    cmd,
                    0,
                    &[vk::Viewport {
                        x: 0.0,
                        y: 0.0,
                        width: w as f32,
                        height: h as f32,
                        min_depth: 0.0,
                        max_depth: 1.0,
                    }],
                );
                device.cmd_set_scissor(
                    cmd,
                    0,
                    &[vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent,
                    }],
                );
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, resolve_pipeline);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    resolve_layout,
                    0,
                    &[occ.depth_set],
                    &[],
                );
                // Fullscreen triangle (3 verts, no vertex buffers).
                device.cmd_draw(cmd, 3, 1, 0, 0);
                device.cmd_end_rendering(cmd);

                // Colour attachment → transfer source for the copy.
                barrier(
                    device,
                    cmd,
                    occ.color.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                );
                let region = vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width: w,
                        height: h,
                        depth: 1,
                    });
                device.cmd_copy_image_to_buffer(
                    cmd,
                    occ.color.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    occ.readback.buffer,
                    &[region],
                );
            }

            occ.state = ReadbackState::Pending { camera, rect };
            occ.last_captured = Some((camera, rect));
        }
    }

    /// Read back any depth captures the previous frame submitted, returning
    /// them as [`SceneDepthMap`]s. Called at the top of the host's `prepare`,
    /// after the previous frame's fence signalled, so the host-visible
    /// read-back buffer holds completed data.
    pub(crate) fn collect_depth_maps(&mut self) -> Vec<(String, SceneDepthMap)> {
        let mut ready = Vec::new();
        for (id, target) in self.targets.iter_mut() {
            let Some(occ) = target.occlusion.as_mut() else {
                continue;
            };
            let ReadbackState::Pending { camera, rect } = occ.state else {
                continue;
            };
            let Ok(bytes) = occ.readback.read_bytes() else {
                continue;
            };
            let count = (occ.width * occ.height) as usize;
            let depth: Vec<f32> = bytemuck::cast_slice::<u8, f32>(&bytes[..count * 4]).to_vec();
            occ.state = ReadbackState::Free;
            ready.push((
                id.clone(),
                SceneDepthMap {
                    camera,
                    rect,
                    width: occ.width,
                    height: occ.height,
                    depth: std::sync::Arc::from(depth),
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
    /// the read-back can finish even after the camera settles; `false` once
    /// every labelled scene has a current map (and for label-free scenes), so
    /// lazy rendering still idles.
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

    unsafe fn encode_cmd(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        pass: &ScenePipelines,
        _target: &OffscreenTarget,
        draw: &DrawCmd,
    ) {
        let bind = |pipeline: vk::Pipeline, slot: usize| unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.scene_pipeline_layout,
                0,
                &[self.uniform_set],
                &[(slot * UNIFORM_STRIDE) as u32],
            );
        };
        match *draw {
            DrawCmd::Grid { slot, first, count } => unsafe {
                bind(pass.line, slot);
                device.cmd_bind_vertex_buffers(
                    cmd,
                    0,
                    &[self.line_quad_vbo.buffer, self.grid_buf.buffer],
                    &[0, 0],
                );
                device.cmd_draw(cmd, 4, count, 0, first);
            },
            DrawCmd::Points { geo, slot } => {
                let Some(CachedGeometry {
                    buffers: GeoBuffers::Points { ibuf, count },
                    ..
                }) = self.geometry.get(&geo)
                else {
                    return;
                };
                unsafe {
                    bind(pass.point, slot);
                    device.cmd_bind_vertex_buffers(
                        cmd,
                        0,
                        &[self.point_quad_vbo.buffer, ibuf.buffer],
                        &[0, 0],
                    );
                    device.cmd_draw(cmd, 4, *count, 0, 0);
                }
            }
            DrawCmd::Lines { geo, slot } => {
                let Some(CachedGeometry {
                    buffers: GeoBuffers::Lines { ibuf, count },
                    ..
                }) = self.geometry.get(&geo)
                else {
                    return;
                };
                unsafe {
                    bind(pass.line, slot);
                    device.cmd_bind_vertex_buffers(
                        cmd,
                        0,
                        &[self.line_quad_vbo.buffer, ibuf.buffer],
                        &[0, 0],
                    );
                    device.cmd_draw(cmd, 4, *count, 0, 0);
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
                    return;
                };
                unsafe {
                    bind(pass.mesh, slot);
                    device.cmd_bind_vertex_buffers(cmd, 0, &[vbuf.buffer], &[0]);
                    match ibuf {
                        Some(ibuf) => {
                            device.cmd_bind_index_buffer(
                                cmd,
                                ibuf.buffer,
                                0,
                                vk::IndexType::UINT32,
                            );
                            device.cmd_draw_indexed(cmd, *icount, 1, 0, 0, 0);
                        }
                        None => device.cmd_draw(cmd, *vcount, 1, 0, 0),
                    }
                }
            }
        }
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            for (_, mut g) in self.geometry.drain() {
                g.buffers.destroy(device, allocator);
            }
            for (_, mut t) in self.targets.drain() {
                t.destroy(device, allocator, self.composite_pool, self.occ_pool);
            }
            for (_, p) in self.passes.drain() {
                device.destroy_pipeline(p.point, None);
                device.destroy_pipeline(p.line, None);
                device.destroy_pipeline(p.mesh, None);
            }
            for (_, p) in self.resolve_pipelines.drain() {
                device.destroy_pipeline(p, None);
            }
            device.destroy_pipeline(self.composite_pipeline, None);
            device.destroy_pipeline_layout(self.composite_pipeline_layout, None);
            device.destroy_pipeline_layout(self.resolve_pipeline_layout, None);
            device.destroy_pipeline_layout(self.scene_pipeline_layout, None);
            device.destroy_descriptor_set_layout(self.composite_set_layout, None);
            device.destroy_descriptor_set_layout(self.resolve_set_layout, None);
            device.destroy_descriptor_set_layout(self.scene_uniform_layout, None);
            device.destroy_descriptor_pool(self.composite_pool, None);
            device.destroy_descriptor_pool(self.occ_pool, None);
            device.destroy_descriptor_pool(self.uniform_pool, None);
            device.destroy_sampler(self.sampler, None);
            self.point_quad_vbo.destroy(device, allocator);
            self.line_quad_vbo.destroy(device, allocator);
            self.uniform_buf.destroy(device, allocator);
            self.grid_buf.destroy(device, allocator);
            self.composite_inst_buf.destroy(device, allocator);
        }
    }
}

impl GeoBuffers {
    unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            match self {
                GeoBuffers::Mesh { vbuf, ibuf, .. } => {
                    vbuf.destroy(device, allocator);
                    if let Some(ibuf) = ibuf {
                        ibuf.destroy(device, allocator);
                    }
                }
                GeoBuffers::Points { ibuf, .. } | GeoBuffers::Lines { ibuf, .. } => {
                    ibuf.destroy(device, allocator);
                }
            }
        }
    }
}

impl OffscreenTarget {
    unsafe fn destroy(
        &mut self,
        device: &ash::Device,
        allocator: &mut Allocator,
        composite_pool: vk::DescriptorPool,
        occ_pool: vk::DescriptorPool,
    ) {
        unsafe {
            let _ = device.free_descriptor_sets(composite_pool, &[self.composite_set]);
            if let Some(mut occ) = self.occlusion.take() {
                let _ = device.free_descriptor_sets(occ_pool, &[occ.depth_set]);
                occ.color.destroy(device, allocator);
                occ.readback.destroy(device, allocator);
            }
            if let Some(msaa) = &mut self.msaa {
                msaa.destroy(device, allocator);
            }
            self.resolve.destroy(device, allocator);
            self.depth.destroy(device, allocator);
        }
    }
}

// ---- free helpers ----

fn sample_flags(sample_count: u32) -> vk::SampleCountFlags {
    match sample_count {
        1 => vk::SampleCountFlags::TYPE_1,
        2 => vk::SampleCountFlags::TYPE_2,
        4 => vk::SampleCountFlags::TYPE_4,
        8 => vk::SampleCountFlags::TYPE_8,
        16 => vk::SampleCountFlags::TYPE_16,
        _ => vk::SampleCountFlags::TYPE_4,
    }
}

fn update_uniform_set(device: &ash::Device, set: vk::DescriptorSet, buf: &GpuBuffer) {
    let info = vk::DescriptorBufferInfo {
        buffer: buf.buffer,
        offset: 0,
        range: UNIFORM_STRIDE as vk::DeviceSize,
    };
    let infos = [info];
    let write = vk::WriteDescriptorSet::default()
        .dst_set(set)
        .dst_binding(0)
        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
        .buffer_info(&infos);
    unsafe { device.update_descriptor_sets(&[write], &[]) };
}

fn upload_buffer(
    device: &ash::Device,
    allocator: &mut Allocator,
    name: &'static str,
    bytes: &[u8],
    usage: vk::BufferUsageFlags,
) -> Result<GpuBuffer> {
    let mut buf = GpuBuffer::new(
        device,
        allocator,
        name,
        bytes.len().max(1) as vk::DeviceSize,
        usage,
        MemoryLocation::CpuToGpu,
    )?;
    if !bytes.is_empty() {
        buf.write_bytes(bytes)?;
    }
    Ok(buf)
}

/// Grow `buf` in place to fit `len` elements of `elem_size` if needed, then
/// write `bytes`.
#[allow(clippy::too_many_arguments)]
fn grow_and_write(
    device: &ash::Device,
    allocator: &mut Allocator,
    buf: &mut GpuBuffer,
    capacity: &mut usize,
    name: &'static str,
    bytes: &[u8],
    len: usize,
    elem_size: usize,
) -> Result<()> {
    if len > *capacity {
        let mut next = (*capacity).max(1);
        while next < len {
            next *= 2;
        }
        unsafe { buf.destroy(device, allocator) };
        *buf = GpuBuffer::new(
            device,
            allocator,
            name,
            (next * elem_size) as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        *capacity = next;
    }
    if !bytes.is_empty() {
        buf.write_bytes(bytes)?;
    }
    Ok(())
}

/// Image layout transition for the scene's offscreen attachments. Derives
/// stages/access from the layout pair; covers the colour/depth attachment and
/// shader-read transitions the renderer needs.
unsafe fn barrier(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    aspect: vk::ImageAspectFlags,
    old: vk::ImageLayout,
    new: vk::ImageLayout,
) {
    let (src_stage, src_access) = match old {
        vk::ImageLayout::UNDEFINED => (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
        ),
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        ),
        _ => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
    };
    let (dst_stage, dst_access) = match new {
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL => (
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::AccessFlags::SHADER_READ,
        ),
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL => (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_READ,
        ),
        _ => (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        ),
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old)
        .new_layout(new)
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: aspect,
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

// ---- pipeline construction ----

#[derive(Clone, Copy)]
enum ScenePipelineKind {
    Point,
    Line,
    Mesh,
}

fn build_scene_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    samples: vk::SampleCountFlags,
    name: &str,
    wgsl: &str,
    kind: ScenePipelineKind,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv(name, wgsl)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_scene_pipeline_inner(device, layout, samples, name, shader, kind);
    unsafe { device.destroy_shader_module(shader, None) };
    result
}

fn build_scene_pipeline_inner(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    samples: vk::SampleCountFlags,
    name: &str,
    shader: vk::ShaderModule,
    kind: ScenePipelineKind,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("no nul");
    let fs_main = CString::new("fs_main").expect("no nul");
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

    let (bindings, attrs, topology, cull) = scene_vertex_layout(kind);
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attrs);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(topology);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(if cull {
            vk::CullModeFlags::BACK
        } else {
            vk::CullModeFlags::empty()
        })
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(samples)
        .sample_shading_enable(samples != vk::SampleCountFlags::TYPE_1)
        .min_sample_shading(1.0);
    let depth_write = matches!(kind, ScenePipelineKind::Mesh);
    let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(true)
        .depth_write_enable(depth_write)
        .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
    let blend_attachment = premultiplied_blend();
    let blend_attachments = [blend_attachment];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let color_formats = [SCENE_COLOR_FORMAT];
    let mut rendering = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(&color_formats)
        .depth_attachment_format(SCENE_DEPTH_FORMAT);

    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth_stencil)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(layout)
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
            name: name.to_string(),
        })
}

fn scene_vertex_layout(
    kind: ScenePipelineKind,
) -> (
    Vec<vk::VertexInputBindingDescription>,
    Vec<vk::VertexInputAttributeDescription>,
    vk::PrimitiveTopology,
    bool,
) {
    let vert = |binding: u32, stride: usize, rate: vk::VertexInputRate| {
        vk::VertexInputBindingDescription {
            binding,
            stride: stride as u32,
            input_rate: rate,
        }
    };
    match kind {
        ScenePipelineKind::Point => (
            vec![
                vert(0, 4 * 4, vk::VertexInputRate::VERTEX),
                vert(
                    1,
                    std::mem::size_of::<gpu::PointInstance>(),
                    vk::VertexInputRate::INSTANCE,
                ),
            ],
            vec![
                attr(0, 0, 0, vk::Format::R32G32_SFLOAT),        // corner
                attr(1, 0, 8, vk::Format::R32G32_SFLOAT),        // uv
                attr(2, 1, 0, vk::Format::R32G32B32_SFLOAT),     // position
                attr(3, 1, 12, vk::Format::R32G32B32A32_SFLOAT), // color
            ],
            vk::PrimitiveTopology::TRIANGLE_STRIP,
            false,
        ),
        ScenePipelineKind::Line => (
            vec![
                vert(0, 2 * 4, vk::VertexInputRate::VERTEX),
                vert(
                    1,
                    std::mem::size_of::<gpu::LineInstance>(),
                    vk::VertexInputRate::INSTANCE,
                ),
            ],
            vec![
                attr(0, 0, 0, vk::Format::R32G32_SFLOAT),        // corner
                attr(1, 1, 0, vk::Format::R32G32B32_SFLOAT),     // start
                attr(2, 1, 12, vk::Format::R32G32B32_SFLOAT),    // end
                attr(3, 1, 24, vk::Format::R32G32B32A32_SFLOAT), // color
                attr(4, 1, 40, vk::Format::R32_SFLOAT),          // width
            ],
            vk::PrimitiveTopology::TRIANGLE_STRIP,
            false,
        ),
        ScenePipelineKind::Mesh => (
            vec![vert(
                0,
                std::mem::size_of::<gpu::MeshVertexGpu>(),
                vk::VertexInputRate::VERTEX,
            )],
            vec![
                attr(0, 0, 0, vk::Format::R32G32B32_SFLOAT),  // position
                attr(1, 0, 12, vk::Format::R32G32B32_SFLOAT), // normal
            ],
            vk::PrimitiveTopology::TRIANGLE_LIST,
            true,
        ),
    }
}

// ---- label-occlusion depth resolve ----

/// WGSL for the depth-resolve pass: a fullscreen triangle that reads the scene
/// depth (sample 0) and writes it to an `R32Float` target. The depth binding
/// type differs for MSAA vs single-sample, so it's templated. Mirrors the
/// wgpu/vulkano backends' `resolve_wgsl`.
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

/// Build the depth-resolve pipeline for one MSAA sample count (dynamic
/// rendering into a single-sample `R32_SFLOAT` target, no depth, no vertex
/// inputs — fullscreen triangle from the vertex index).
fn build_depth_resolve_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    multisampled: bool,
) -> Result<vk::Pipeline> {
    let wgsl = resolve_wgsl(multisampled);
    let words = wgsl_to_spirv("stock::scene_depth_resolve", &wgsl)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_depth_resolve_inner(device, layout, shader);
    unsafe { device.destroy_shader_module(shader, None) };
    result
}

fn build_depth_resolve_inner(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    shader: vk::ShaderModule,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("no nul");
    let fs_main = CString::new("fs_main").expect("no nul");
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
    // No vertex inputs — the fullscreen triangle is generated in the shader.
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::empty())
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    // The R32 resolve target is always single-sample.
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let blend_attachment = vk::PipelineColorBlendAttachmentState::default().color_write_mask(
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
    let color_formats = [SCENE_OCC_FORMAT];
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
        .layout(layout)
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
            name: "stock::scene_depth_resolve".to_string(),
        })
}

fn build_composite_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    target: TargetInfo,
) -> Result<vk::Pipeline> {
    let words = wgsl_to_spirv("stock::surface", stock_wgsl::SURFACE)?;
    let shader_info = vk::ShaderModuleCreateInfo::default().code(&words);
    let shader = unsafe { device.create_shader_module(&shader_info, None) }?;
    let result = build_composite_inner(device, layout, target, shader);
    unsafe { device.destroy_shader_module(shader, None) };
    result
}

fn build_composite_inner(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    target: TargetInfo,
    shader: vk::ShaderModule,
) -> Result<vk::Pipeline> {
    let vs_main = CString::new("vs_main").expect("no nul");
    let fs_premul = CString::new("fs_premul").expect("no nul");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(shader)
            .name(&vs_main),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(shader)
            .name(&fs_premul),
    ];
    let bindings = [
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: (2 * std::mem::size_of::<f32>()) as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        },
        vk::VertexInputBindingDescription {
            binding: 1,
            stride: std::mem::size_of::<gpu::CompositeInstance>() as u32,
            input_rate: vk::VertexInputRate::INSTANCE,
        },
    ];
    let attrs = [
        attr(0, 0, 0, vk::Format::R32G32_SFLOAT),
        attr(1, 1, 0, vk::Format::R32G32B32A32_SFLOAT), // rect
        attr(2, 1, 16, vk::Format::R32G32B32A32_SFLOAT), // matrix
        attr(3, 1, 32, vk::Format::R32G32_SFLOAT),      // translation
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
    let blend_attachment = premultiplied_blend();
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
        .layout(layout)
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
            name: "stock::surface::scene_composite".to_string(),
        })
}

fn premultiplied_blend() -> vk::PipelineColorBlendAttachmentState {
    vk::PipelineColorBlendAttachmentState::default()
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
        )
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
