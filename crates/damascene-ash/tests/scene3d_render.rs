//! Headless end-to-end render test for the Scene3D pipeline on ash.
//!
//! Twin of the wgpu/vulkano `scene3d_render` tests. Builds a real Vulkan 1.3
//! device (dynamic rendering), renders a `chart3d` (lit sphere + scatter +
//! line + grid) into an offscreen target via `Runner::render`, copies it to a
//! host-visible buffer, and asserts the scene composited content. This is the
//! only place the scene WGSL is compiled through naga on the ash side and the
//! whole offscreen → resolve → composite path runs against the GPU.
//!
//! Skips cleanly (passes) when no suitable Vulkan device is available.

use std::sync::{Arc, Mutex};

use ash::vk;
use damascene_ash::{AshContext, AshRenderTarget, LoadOp, Runner, TargetInfo};
use damascene_core::prelude::*;
use damascene_core::scene::glam::Vec3;
use damascene_core::scene::{
    GridPlanes, GridSettings, LineData, LineSegment, LinesHandle, Material, MeshData, MeshHandle,
    MeshVertex, PointData, PointStyle, PointsHandle, ScenePoint, SceneSpec, SceneStyle,
};
use damascene_core::{AnimationMode, Rect};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{
    Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};

const SIZE: u32 = 160;
const FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;

/// Holds the Vulkan objects alive for the test. We deliberately don't destroy
/// the device/instance/pool: raw ash handles have no `Drop`, and gpu-allocator's
/// `Allocator` needs the device alive when *it* drops. So we free every
/// allocation (see `render_to_pixels`) — which is all gpu-allocator checks —
/// and let the device/instance leak; the process exits right after.
#[allow(dead_code)]
struct Gpu {
    device: Arc<ash::Device>,
    allocator: Arc<Mutex<Allocator>>,
    queue: vk::Queue,
    queue_family_index: u32,
    command_pool: vk::CommandPool,
    physical_device: vk::PhysicalDevice,
    instance: ash::Instance,
    entry: ash::Entry,
}

fn headless_gpu() -> Option<Gpu> {
    let entry = unsafe { ash::Entry::load() }.ok()?;
    let app = std::ffi::CString::new("damascene-ash-test").ok()?;
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app)
        .api_version(vk::API_VERSION_1_3);
    let instance_info = vk::InstanceCreateInfo::default().application_info(&app_info);
    let instance = unsafe { entry.create_instance(&instance_info, None) }.ok()?;

    let physical_devices = unsafe { instance.enumerate_physical_devices() }.ok()?;
    let (physical_device, queue_family_index) = physical_devices.into_iter().find_map(|pd| {
        let props = unsafe { instance.get_physical_device_queue_family_properties(pd) };
        let qf = props
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))?;
        // Require Vulkan 1.3 dynamic rendering.
        let mut features13 = vk::PhysicalDeviceVulkan13Features::default();
        let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut features13);
        unsafe { instance.get_physical_device_features2(pd, &mut features) };
        (features13.dynamic_rendering == vk::TRUE).then_some((pd, qf as u32))
    })?;

    let priorities = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family_index)
        .queue_priorities(&priorities);
    let queue_infos = [queue_info];
    let features = damascene_ash::required_device_features();
    let mut features13 = damascene_ash::required_vulkan_13_features();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_infos)
        .enabled_features(&features)
        .push_next(&mut features13);
    let device = unsafe { instance.create_device(physical_device, &device_info, None) }.ok()?;
    let device = Arc::new(device);
    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    let allocator = Allocator::new(&AllocatorCreateDesc {
        instance: instance.clone(),
        device: (*device).clone(),
        physical_device,
        debug_settings: Default::default(),
        buffer_device_address: false,
        allocation_sizes: Default::default(),
    })
    .ok()?;
    let allocator = Arc::new(Mutex::new(allocator));

    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(queue_family_index);
    let command_pool = unsafe { device.create_command_pool(&pool_info, None) }.ok()?;

    Some(Gpu {
        device,
        allocator,
        queue,
        queue_family_index,
        command_pool,
        physical_device,
        instance,
        entry,
    })
}

/// Allocate a GpuOnly image + default colour view.
fn make_image(gpu: &Gpu, usage: vk::ImageUsageFlags) -> (vk::Image, vk::ImageView, Allocation) {
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width: SIZE,
            height: SIZE,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { gpu.device.create_image(&info, None) }.expect("create image");
    let req = unsafe { gpu.device.get_image_memory_requirements(image) };
    let alloc = gpu
        .allocator
        .lock()
        .unwrap()
        .allocate(&AllocationCreateDesc {
            name: "test_target",
            requirements: req,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })
        .expect("allocate image");
    unsafe {
        gpu.device
            .bind_image_memory(image, alloc.memory(), alloc.offset())
            .expect("bind image");
    }
    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(FORMAT)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
    let view = unsafe { gpu.device.create_image_view(&view_info, None) }.expect("create view");
    (image, view, alloc)
}

fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: El) -> Vec<u8> {
    let (image, view, image_alloc) = make_image(
        gpu,
        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
    );

    // Host-visible readback buffer.
    let buf_size = (SIZE * SIZE * 4) as vk::DeviceSize;
    let buf_info = vk::BufferCreateInfo::default()
        .size(buf_size)
        .usage(vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let readback = unsafe { gpu.device.create_buffer(&buf_info, None) }.expect("readback buffer");
    let req = unsafe { gpu.device.get_buffer_memory_requirements(readback) };
    let readback_alloc = gpu
        .allocator
        .lock()
        .unwrap()
        .allocate(&AllocationCreateDesc {
            name: "test_readback",
            requirements: req,
            location: MemoryLocation::GpuToCpu,
            linear: true,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })
        .expect("allocate readback");
    unsafe {
        gpu.device
            .bind_buffer_memory(readback, readback_alloc.memory(), readback_alloc.offset())
            .expect("bind readback");
    }

    let viewport = Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32);
    runner.set_surface_size(SIZE, SIZE);
    runner.prepare(tree, viewport, 1.0);

    // Record + submit a one-shot command buffer.
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(gpu.command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmd = unsafe { gpu.device.allocate_command_buffers(&alloc_info) }.expect("alloc cmd")[0];
    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe {
        gpu.device.begin_command_buffer(cmd, &begin).expect("begin");
        let target = AshRenderTarget {
            image,
            view,
            format: FORMAT,
            extent: vk::Extent2D {
                width: SIZE,
                height: SIZE,
            },
            sample_count: vk::SampleCountFlags::TYPE_1,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        };
        runner
            .render(cmd, target, LoadOp::Clear([0.0, 0.0, 0.0, 1.0]))
            .expect("render");
        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: SIZE,
                height: SIZE,
                depth: 1,
            });
        gpu.device.cmd_copy_image_to_buffer(
            cmd,
            image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            readback,
            &[region],
        );
        gpu.device.end_command_buffer(cmd).expect("end");

        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().command_buffers(&cmds);
        let fence = gpu
            .device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .expect("fence");
        gpu.device
            .queue_submit(gpu.queue, &[submit], fence)
            .expect("submit");
        gpu.device
            .wait_for_fences(&[fence], true, u64::MAX)
            .expect("wait");
        gpu.device.destroy_fence(fence, None);
        gpu.device.free_command_buffers(gpu.command_pool, &cmds);
    }

    let pixels = readback_alloc
        .mapped_slice()
        .expect("readback mapped")
        .to_vec();

    // Free everything (gpu-allocator complains about leaked allocations).
    unsafe {
        gpu.device.destroy_image_view(view, None);
        gpu.device.destroy_image(image, None);
        gpu.device.destroy_buffer(readback, None);
    }
    let mut allocator = gpu.allocator.lock().unwrap();
    allocator.free(image_alloc).expect("free image");
    allocator.free(readback_alloc).expect("free readback");

    pixels
}

fn count_lit(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] > 16 || px[1] > 16 || px[2] > 16)
        .count()
}

fn uv_sphere(radius: f32, rings: u32, sectors: u32) -> MeshData {
    use std::f32::consts::{PI, TAU};
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=rings {
        let theta = i as f32 / rings as f32 * PI;
        let (st, ct) = theta.sin_cos();
        for j in 0..=sectors {
            let phi = j as f32 / sectors as f32 * TAU;
            let (sp, cp) = phi.sin_cos();
            let n = Vec3::new(st * cp, ct, st * sp);
            vertices.push(MeshVertex {
                position: n * radius,
                normal: n,
            });
        }
    }
    let stride = sectors + 1;
    for i in 0..rings {
        for j in 0..sectors {
            let a = i * stride + j;
            let b = a + stride;
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    MeshData {
        vertices,
        indices: Some(indices),
    }
}

fn make_runner(gpu: &Gpu) -> Runner {
    let max_image_dimension_2d = unsafe {
        gpu.instance
            .get_physical_device_properties(gpu.physical_device)
    }
    .limits
    .max_image_dimension2_d;
    let context = AshContext::new(
        gpu.device.clone(),
        gpu.allocator.clone(),
        gpu.queue_family_index,
        max_image_dimension_2d,
    );
    let mut runner = Runner::new(context, TargetInfo::new(FORMAT)).expect("runner");
    runner.set_animation_mode(AnimationMode::Settled);
    runner
}

#[test]
fn scene3d_composites_visible_content() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("scene3d_render(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    let mesh = MeshHandle::new(uv_sphere(0.9, 24, 32));
    let points = PointsHandle::new(PointData {
        points: vec![
            ScenePoint {
                position: Vec3::new(1.6, 0.0, 0.0),
                color: [1.0, 0.2, 0.2, 1.0],
            },
            ScenePoint {
                position: Vec3::new(0.0, 1.6, 0.0),
                color: [0.2, 1.0, 0.2, 1.0],
            },
            ScenePoint {
                position: Vec3::new(0.0, 0.0, 1.6),
                color: [0.3, 0.4, 1.0, 1.0],
            },
        ],
    });
    let lines = LinesHandle::new(LineData {
        segments: vec![LineSegment {
            start: Vec3::new(-1.6, -1.6, 0.0),
            end: Vec3::new(1.6, 1.6, 0.0),
            color: [1.0, 1.0, 1.0, 1.0],
        }],
    });
    let spec = SceneSpec::new()
        .mesh(mesh)
        .points_styled(
            points,
            PointStyle {
                size: 14.0,
                ..Default::default()
            },
        )
        .lines(lines);
    let tree = chart3d(spec);
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("scene3d_render(ash): {lit}/{total} non-black pixels");
    drop(runner);
    assert!(
        lit > total / 100,
        "scene composited almost nothing ({lit}/{total} lit) — offscreen render or composite is broken"
    );
}

/// Lit-pixel count and the p10/p90 luminance of a rendered mesh — the shared
/// analysis for `lit_cube_shades_axis_aligned_faces`. Luminance is the mean
/// channel `(r+g+b)/3`; pixels at or below 10 are treated as background.
/// Returns `(lit_count, p10, p90)`, or `(0, 0, 0)` when nothing lit.
fn cube_face_shading(px: &[u8]) -> (usize, u16, u16) {
    let mut lums: Vec<u16> = px
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (p[0] as u16 + p[1] as u16 + p[2] as u16) / 3)
        .filter(|&l| l > 10)
        .collect();
    lums.sort_unstable();
    let lit = lums.len();
    if lit == 0 {
        return (0, 0, 0);
    }
    (lit, lums[lit / 10], lums[lit * 9 / 10])
}

/// A lit (matte) cube must shade its faces by their normals — the regression
/// guard for the mesh-normal vertex-attribute offset. Every face of an
/// axis-aligned cube has an exactly axis-aligned normal, so a normal fetched
/// at the wrong byte offset feeds the shader `normalize((0,0,0)) = NaN` on
/// those faces (the tear a sibling renderer shipped for months while every
/// CPU-side audit passed). NaN encodes to black, so the cube would render
/// largely unlit; a normal flattened to a constant would instead shade every
/// visible face identically. The other cube tests here shade *flat* (unlit) or
/// read depth only, so none of them exercise the normal fetch — this one does.
/// A mid-tone base keeps the key-lit face clear of sRGB saturation so the
/// top/side contrast survives to the pixels.
#[test]
fn lit_cube_shades_axis_aligned_faces() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("lit_cube_shades(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    // Only the cube paints: no grid, no axes.
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::NONE,
            ..Default::default()
        },
        background: None,
        msaa_samples: 4,
        show_axes: false,
    };
    // Mid-tone matte (forward-lit): bright enough to read, dark enough that the
    // key-lit face doesn't clip to white and flatten the top/side contrast.
    let draw = damascene_core::scene::MeshDraw {
        geometry: MeshHandle::new(cube()),
        transform: damascene_core::scene::glam::Mat4::IDENTITY,
        material: Material::matte(Color::srgb_u8(90, 110, 140)),
    };
    let tree = chart3d(SceneSpec::new().add_mesh(draw).style(style));

    let px = render_to_pixels(&gpu, &mut runner, tree);
    drop(runner);
    let (lit, p10, p90) = cube_face_shading(&px);
    let total = (SIZE * SIZE) as usize;
    eprintln!("lit_cube_shades(ash): {lit}/{total} lit, luminance p10={p10} p90={p90}");

    // NaN normals encode to black, so a torn cube collapses the lit area.
    assert!(
        lit > total / 16,
        "lit cube barely rendered ({lit}/{total} lit) — axis-aligned face \
         normals likely read at the wrong offset (normalize((0,0,0)) = NaN)"
    );
    // Distinct face brightnesses: the key light shades a +Y top face well
    // brighter than a +X/+Z side. A normal collapsed to a constant (a wrong
    // offset that doesn't NaN) would flatten this spread to ~0.
    assert!(
        p90 - p10 >= 12,
        "cube faces must shade differently by normal (p10={p10}, p90={p90}); a \
         flat spread means the normal attribute is misread"
    );
}

/// Axis lines nearer than a solid mesh must draw over it — twin of the
/// wgpu/vulkano `axis_lines_in_front_of_mesh_are_visible` tests. The
/// grid/axes batch depth-tests without writing depth, so it must be recorded
/// *after* the opaque meshes; recorded first, a later mesh painted over
/// nearer strokes unconditionally.
#[test]
fn axis_lines_in_front_of_mesh_are_visible() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("axis_over_mesh(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    // Pure-blue unlit cube pushed back behind the origin; axes on, grid
    // planes off so only the three axis lines stroke the scene.
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::NONE,
            ..Default::default()
        },
        background: None,
        msaa_samples: 1,
        show_axes: true,
    };
    let draw = damascene_core::scene::MeshDraw {
        geometry: MeshHandle::new(cube()),
        transform: damascene_core::scene::glam::Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0))
            * damascene_core::scene::glam::Mat4::from_scale(Vec3::splat(1.5)),
        material: Material::flat(Color::srgb_u8(0, 0, 255)),
    };
    let with_axes = |on: bool| {
        chart3d(SceneSpec::new().add_mesh(draw.clone()).style(SceneStyle {
            show_axes: on,
            ..style
        }))
    };

    let a = render_to_pixels(&gpu, &mut runner, with_axes(true));
    let b = render_to_pixels(&gpu, &mut runner, with_axes(false));
    drop(runner);

    // Pixels that are pure cube in the axes-off render but carry an axis
    // stroke (raised red or green) in the axes-on render.
    let crossing = a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0.iter())
        .filter(|(pa, pb)| {
            let cube_px = pb[2] > 150 && pb[0] < 60 && pb[1] < 60;
            let axis_px = pa[0] > 120 || pa[1] > 120;
            cube_px && axis_px
        })
        .count();
    eprintln!("axis_over_mesh(ash): {crossing} axis-over-cube pixels");
    assert!(
        crossing > 0,
        "axis lines nearer than the mesh must draw over it (0 axis-coloured \
         pixels found on the cube — grid batch likely recorded before meshes)"
    );
}

/// A translucent mesh (material alpha < 1) must not hide geometry behind it.
/// Twin of the wgpu/vulkano `translucent_mesh_shows_opaque_geometry_through`
/// tests: a red unlit cube inside a blue translucent shell listed *first* in
/// the spec. Under an opaque-only mesh path (depth write, spec order) the
/// shell would depth-reject the cube and the centre would read pure blue;
/// through the translucent path the centre shows the red cube tinted by the
/// shell.
#[test]
fn translucent_mesh_shows_opaque_geometry_through() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("translucent_mesh(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    // No grid/axes: reference lines would cross the centre sample.
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::NONE,
            ..Default::default()
        },
        background: None,
        msaa_samples: 1,
        show_axes: false,
    };
    let shell = MeshHandle::new(uv_sphere(2.5, 24, 32));
    let inner = MeshHandle::new(cube());
    let tree = chart3d(
        SceneSpec::new()
            .mesh_with(
                shell,
                Material::flat(Color::srgb_u8(0, 0, 255).with_alpha(0.3)),
            )
            .mesh_with(inner, Material::flat(Color::srgb_u8(255, 0, 0)))
            .style(style),
    );

    let px = render_to_pixels(&gpu, &mut runner, tree);
    let mid = SIZE / 2;
    let i = ((mid * SIZE + mid) * 4) as usize;
    let [r, g, b] = [px[i], px[i + 1], px[i + 2]];
    eprintln!("translucent_mesh(ash): centre = ({r}, {g}, {b})");
    drop(runner);
    assert!(
        r > 120,
        "cube must show through the translucent shell, got ({r}, {g}, {b})"
    );
    assert!(
        b > 40,
        "shell must tint the cube behind it, got ({r}, {g}, {b})"
    );
}

/// Axis-aligned unit cube (flat per-face normals, CCW outward). The occlusion
/// test frames it so the centre lands on the cube and the corners read far.
#[allow(clippy::type_complexity)]
fn cube() -> MeshData {
    let faces: [([f32; 3], [(f32, f32, f32); 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [(-1., -1., 1.), (1., -1., 1.), (1., 1., 1.), (-1., 1., 1.)],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                (1., -1., -1.),
                (-1., -1., -1.),
                (-1., 1., -1.),
                (1., 1., -1.),
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [(1., -1., 1.), (1., -1., -1.), (1., 1., -1.), (1., 1., 1.)],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                (-1., -1., -1.),
                (-1., -1., 1.),
                (-1., 1., 1.),
                (-1., 1., -1.),
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [(-1., 1., 1.), (1., 1., 1.), (1., 1., -1.), (-1., 1., -1.)],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                (-1., -1., -1.),
                (1., -1., -1.),
                (1., -1., 1.),
                (-1., -1., 1.),
            ],
        ),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (normal, corners) in faces {
        let base = vertices.len() as u32;
        for (x, y, z) in corners {
            vertices.push(MeshVertex {
                position: Vec3::new(x, y, z),
                normal: Vec3::from_array(normal),
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    MeshData {
        vertices,
        indices: Some(indices),
    }
}

/// The backend captures a per-scene depth map (for label occlusion) and reads
/// it back a frame later. Pump frames until the map lands, then check it
/// encodes the geometry and that `SceneDepthMap::occludes` agrees: the framed
/// cube is captured (centre near, corner far), a point inside it is occluded, a
/// point by the eye is not. Twin of the wgpu/vulkano occlusion tests.
#[test]
fn scene_depth_map_captures_geometry_for_occlusion() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("scene_depth_map(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    // Axis titles flag the scene for depth capture.
    let mesh = MeshHandle::new(cube());
    let tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    // Capture is recorded in `render` and read in the next `prepare`, so pump
    // frames until the map appears.
    let mut captured = None;
    for _ in 0..10 {
        let _ = render_to_pixels(&gpu, &mut runner, tree.clone());
        if let Some((_, m)) = runner.ui_state().scene_depth_maps().next() {
            let center = m.depth[(m.height / 2 * m.width + m.width / 2) as usize];
            let corner = m.depth[0];
            let eye = m.camera.eye;
            let near_eye = eye + (m.camera.target - eye) * 0.05;
            captured = Some((
                m.width,
                m.height,
                center,
                corner,
                m.occludes(Vec3::ZERO),
                m.occludes(near_eye),
            ));
            break;
        }
    }
    drop(runner);

    let Some((w, h, center, corner, origin_occluded, near_eye_occluded)) = captured else {
        panic!("no scene depth map was captured after pumping frames");
    };
    assert_eq!((w, h), (SIZE, SIZE), "depth map matches the offscreen size");
    eprintln!("scene_depth_map(ash): centre={center}, corner={corner}");
    assert!(
        center < 0.99,
        "cube centre should be captured, got {center}"
    );
    assert!(corner > 0.99, "empty corner should read far, got {corner}");
    assert!(origin_occluded, "a point inside the cube is occluded");
    assert!(!near_eye_occluded, "a point by the eye is not occluded");
}

#[test]
fn uv_sphere_winds_outward() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("uv_sphere_winds_outward(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);
    let mesh = MeshHandle::new(uv_sphere(1.0, 24, 32));
    let tree = chart3d(SceneSpec::new().mesh(mesh).no_grid());
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("uv_sphere_winds_outward(ash): {lit}/{total} lit");
    drop(runner);
    assert!(
        lit > total / 6,
        "sphere barely rendered ({lit} px) — winding likely inverted (front faces culled)"
    );
}

#[test]
fn working_color_space_reaches_the_painters() {
    // Regression for #61: `set_working_color_space` must reach this
    // backend's color recorders, not just the shared quad path. A white
    // image tinted sRGB-red while compositing in Display-P3-linear must
    // land at sRGB red's P3 coordinates — an unplumbed image painter
    // would leave the tint at (1, 0, 0) and the green channel at zero.
    let Some(gpu) = headless_gpu() else {
        eprintln!("working_color_space(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);
    let p3 = damascene_core::color::ColorSpace::DISPLAY_P3_LINEAR;
    runner.set_working_color_space(p3);

    let white =
        damascene_core::image::Image::from_rgba8(8, 8, vec![[255u8, 255, 255, 255]; 64].concat());
    let tint = Color::srgb_u8(255, 0, 0);
    let tree = image(white)
        .image_tint(tint)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32));
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    drop(runner);

    // The `*_SRGB` target encodes on store, so the expected bytes are
    // the sRGB-encoded P3-linear tint (the white texel is identity).
    let expected: Vec<u8> = damascene_core::paint::rgba_f32_in(tint, p3)[..3]
        .iter()
        .map(|&c| {
            let e = if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            };
            (e * 255.0).round() as u8
        })
        .collect();
    let center = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let got = &pixels[center..center + 3];
    eprintln!("working_color_space(ash): got {got:?}, expected {expected:?}");
    for (g, e) in got.iter().zip(&expected) {
        assert!(
            (*g as i16 - *e as i16).abs() <= 3,
            "center pixel {got:?} should be the P3-converted tint {expected:?}"
        );
    }
}

// ---- A node's opacity reaches the paint no op colour carries ----
//
// Twin of `damascene-wgpu`'s `opacity_render`. Each case renders a
// tile over a light backdrop at full opacity, at half, and not at
// all. Summed over the frame, the half frame must have moved half the
// way from the bare backdrop to the full frame, in linear light. A
// backend that ignores the opacity gives a ratio of 1; one that
// applies it twice gives a quarter; one that scales the tile without
// blending it over what is underneath falls short of a half by the
// backdrop's share.

fn srgb_to_linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// One frame in linear light, RGB per pixel: `tile` over a light
/// backdrop, or the backdrop alone.
fn fade_frame(gpu: &Gpu, tile: Option<El>) -> Vec<f32> {
    let mut runner = make_runner(gpu);
    let backdrop = El::new(Kind::Group)
        .fill(Color::srgb_u8(200, 215, 255))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));
    let tree = stack(std::iter::once(backdrop).chain(tile))
        .align(Align::Center)
        .justify(Justify::Center)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32));
    let pixels = render_to_pixels(gpu, &mut runner, tree);
    drop(runner);
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .map(srgb_to_linear)
        .collect()
}

/// How far the half-opacity frame has moved from the backdrop towards
/// the full-opacity frame must be 0.5. Returns the full-opacity
/// frame's centre pixel for callers with more to check.
fn assert_fades_halfway(gpu: &Gpu, what: &str, tile: &dyn Fn(f32) -> El) -> [f32; 3] {
    let full = fade_frame(gpu, Some(tile(1.0)));
    let half = fade_frame(gpu, Some(tile(0.5)));
    let none = fade_frame(gpu, None);
    let (mut moved, mut span) = (0.0f64, 0.0f64);
    for i in 0..full.len() {
        let d = (full[i] - none[i]) as f64;
        moved += (half[i] - none[i]) as f64 * d.signum();
        span += d.abs();
    }
    assert!(
        span / full.len() as f64 > 0.002,
        "{what}: the tile must show against the backdrop to measure a fade (span {span:.2})"
    );
    let ratio = moved / span;
    assert!(
        (ratio - 0.5).abs() < 0.04,
        "{what}: at half opacity the frame moved {ratio:.3} of the way to the full frame, \
         want 0.5"
    );
    let at = ((SIZE / 2 * SIZE + SIZE / 2) * 3) as usize;
    [full[at], full[at + 1], full[at + 2]]
}

/// A host-owned image cleared to one colour and left ready to sample.
struct SolidTexture {
    texture: AppTexture,
    image: vk::Image,
    view: vk::ImageView,
    alloc: Allocation,
}

impl SolidTexture {
    /// `rgba` is linear; the sRGB image stores it encoded and the
    /// sampler hands the same linear values back.
    fn new(gpu: &Gpu, rgba: [f32; 4]) -> Self {
        let (image, view, alloc) = make_image(
            gpu,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
        );
        let range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let barrier = |old, new, src_access, dst_access| {
            vk::ImageMemoryBarrier::default()
                .old_layout(old)
                .new_layout(new)
                .src_access_mask(src_access)
                .dst_access_mask(dst_access)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(range)
        };
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(gpu.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        unsafe {
            let cmd = gpu
                .device
                .allocate_command_buffers(&alloc_info)
                .expect("alloc cmd")[0];
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            gpu.device.begin_command_buffer(cmd, &begin).expect("begin");
            gpu.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::AccessFlags::empty(),
                    vk::AccessFlags::TRANSFER_WRITE,
                )],
            );
            gpu.device.cmd_clear_color_image(
                cmd,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue { float32: rgba },
                &[range],
            );
            gpu.device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier(
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::SHADER_READ,
                )],
            );
            gpu.device.end_command_buffer(cmd).expect("end");
            let cmds = [cmd];
            let submit = vk::SubmitInfo::default().command_buffers(&cmds);
            let fence = gpu
                .device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .expect("fence");
            gpu.device
                .queue_submit(gpu.queue, &[submit], fence)
                .expect("submit");
            gpu.device
                .wait_for_fences(&[fence], true, u64::MAX)
                .expect("wait");
            gpu.device.destroy_fence(fence, None);
            gpu.device.free_command_buffers(gpu.command_pool, &cmds);
        }
        let texture = damascene_ash::app_texture(
            image,
            view,
            FORMAT,
            vk::Extent2D {
                width: SIZE,
                height: SIZE,
            },
        );
        Self {
            texture,
            image,
            view,
            alloc,
        }
    }

    /// Free the image once no runner can still sample it.
    fn destroy(self, gpu: &Gpu) {
        unsafe {
            gpu.device.destroy_image_view(self.view, None);
            gpu.device.destroy_image(self.image, None);
        }
        gpu.allocator
            .lock()
            .unwrap()
            .free(self.alloc)
            .expect("free texture");
    }
}

#[test]
fn a_painted_vector_fades_with_its_node() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    // One solid fill and one gradient fill: the two places a painted
    // asset keeps a colour (vertex colour, gradient table).
    let solid = damascene_core::vector::parse_svg_asset(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <rect width="10" height="10" fill="#ff4020"/></svg>"##,
    )
    .expect("solid svg");
    let gradient = damascene_core::vector::parse_svg_asset(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0" stop-color="#20c0ff"/><stop offset="1" stop-color="#20ffc0"/>
            </linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/></svg>"##,
    )
    .expect("gradient svg");
    for (what, asset) in [("solid fill", solid), ("gradient fill", gradient)] {
        assert_fades_halfway(&gpu, &format!("painted vector, {what}"), &|opacity| {
            vector(asset.clone())
                .width(Size::Fixed(120.0))
                .height(Size::Fixed(120.0))
                .opacity(opacity)
        });
    }
}

#[test]
fn an_authored_colour_icon_fades_with_its_node() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let logo = SvgIcon::parse(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <rect width="24" height="24" fill="#ffb000"/></svg>"##,
    )
    .expect("icon svg");
    assert_fades_halfway(&gpu, "authored-colour icon", &|opacity| {
        icon(logo.clone()).icon_size(120.0).opacity(opacity)
    });
}

#[test]
fn an_app_texture_fades_with_its_node_in_every_alpha_mode() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let cases = [
        // Premultiplied and straight both describe 80%-opaque orange.
        (
            "premultiplied",
            SurfaceAlpha::Premultiplied,
            [0.8, 0.4, 0.0, 0.8],
        ),
        ("straight", SurfaceAlpha::Straight, [1.0, 0.5, 0.0, 0.8]),
        // An opaque surface ignores the texture's alpha, here zero.
        ("opaque", SurfaceAlpha::Opaque, [0.0, 1.0, 0.0, 0.0]),
    ];
    for (what, alpha, rgba) in cases {
        let tex = SolidTexture::new(&gpu, rgba);
        let full = assert_fades_halfway(&gpu, &format!("app texture, {what}"), &|opacity| {
            surface(tex.texture.clone())
                .width(Size::Fixed(120.0))
                .height(Size::Fixed(120.0))
                .surface_alpha(alpha)
                .opacity(opacity)
        });
        tex.destroy(&gpu);
        if matches!(alpha, SurfaceAlpha::Opaque) {
            assert!(
                full[0] < 0.01 && full[1] > 0.99 && full[2] < 0.01,
                "an opaque surface at full opacity replaces what is under it, got {full:?}"
            );
        }
    }
}

#[test]
fn a_scene_fades_with_its_node() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::NONE,
            ..Default::default()
        },
        background: Some(Color::srgb_u8(230, 70, 30)),
        msaa_samples: 4,
        show_axes: false,
    };
    assert_fades_halfway(&gpu, "3D scene", &|opacity| {
        chart3d(SceneSpec::new().style(style)).opacity(opacity)
    });
}

/// The stock shaders that paint from uniforms of their own. Their
/// colours reach the shader unfaded; the shader applies the opacity.
#[test]
fn spinner_skeleton_and_indeterminate_progress_fade_with_their_node() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let ink = Color::srgb_u8(20, 40, 160);
    type Build = fn(Color) -> El;
    let cases: [(&str, Build); 3] = [
        ("spinner", |c| spinner_with_color(c)),
        ("skeleton", |c| skeleton::skeleton_with_color(c)),
        ("indeterminate progress", |c| {
            progress_indeterminate_with_color(c)
        }),
    ];
    for (what, build) in cases {
        assert_fades_halfway(&gpu, what, &|opacity| {
            build(ink)
                .width(Size::Fixed(120.0))
                .height(Size::Fixed(120.0))
                .opacity(opacity)
        });
    }
}

/// A colour glyph keeps its own RGB and takes the alpha of the text
/// colour around it.
#[test]
fn a_colour_emoji_fades_with_its_text() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    assert_fades_halfway(&gpu, "colour emoji", &|opacity| {
        text("\u{1F600}").font_size(100.0).opacity(opacity)
    });
}

/// A drop shadow is CSS `box-shadow`: cast outside the surface and
/// never drawn under it, whatever the fill's alpha. A fill that is
/// translucent by design shows the backdrop through, not a ground its
/// own shadow darkened. Twin of
/// `damascene-wgpu/tests/shadow_render.rs`.
#[test]
fn a_shadow_is_cast_outside_its_surface_and_never_under_it() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("shadow(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    const TILE: f32 = 80.0;
    let tile = |shadow: f32| {
        El::new(Kind::Group)
            .fill(Color::srgb_u8(0, 90, 255).with_alpha(0.15))
            .radius(8.0)
            .shadow(shadow)
            .width(Size::Fixed(TILE))
            .height(Size::Fixed(TILE))
    };
    let lit = fade_frame(&gpu, Some(tile(damascene_core::tokens::SHADOW_MD)));
    let flat = fade_frame(&gpu, Some(tile(0.0)));
    let side = SIZE as usize;
    let luma = |frame: &[f32], x: usize, y: usize| {
        let i = (y * side + x) * 3;
        0.2126 * frame[i] + 0.7152 * frame[i + 1] + 0.0722 * frame[i + 2]
    };
    let (cx, cy) = (side / 2, side / 2);

    // Under the tint: the same pixel with the shadow and without it.
    // Painted under the silhouette, the shadow took a fifth of the
    // light out of this pixel.
    let under = (luma(&lit, cx, cy) - luma(&flat, cx, cy)).abs();
    assert!(
        under < 0.01,
        "the tint sits on the backdrop, not on its own shadow (differs by {under:.3})"
    );

    // Just below the tile: the halo is there, at full strength.
    let below = cy + TILE as usize / 2 + 3;
    let halo = luma(&flat, cx, below) - luma(&lit, cx, below);
    assert!(
        halo > 0.02,
        "a translucent surface still casts its shadow (halo {halo:.3})"
    );
}
