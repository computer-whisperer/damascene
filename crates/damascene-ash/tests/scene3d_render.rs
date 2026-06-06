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

fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: &mut El) -> Vec<u8> {
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
        .chunks_exact(4)
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
    let context = AshContext::new(
        gpu.device.clone(),
        gpu.allocator.clone(),
        gpu.queue_family_index,
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
    let mut tree = chart3d(spec);
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("scene3d_render(ash): {lit}/{total} non-black pixels");
    drop(runner);
    assert!(
        lit > total / 100,
        "scene composited almost nothing ({lit}/{total} lit) — offscreen render or composite is broken"
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
    let mut tree = chart3d(
        SceneSpec::new()
            .mesh_with(
                shell,
                Material::flat(Color::srgb_u8(0, 0, 255).with_alpha(0.3)),
            )
            .mesh_with(inner, Material::flat(Color::srgb_u8(255, 0, 0)))
            .style(style),
    );

    let px = render_to_pixels(&gpu, &mut runner, &mut tree);
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
    let mut tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    // Capture is recorded in `render` and read in the next `prepare`, so pump
    // frames until the map appears.
    let mut captured = None;
    for _ in 0..10 {
        let _ = render_to_pixels(&gpu, &mut runner, &mut tree);
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
    let mut tree = chart3d(SceneSpec::new().mesh(mesh).no_grid());
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("uv_sphere_winds_outward(ash): {lit}/{total} lit");
    drop(runner);
    assert!(
        lit > total / 6,
        "sphere barely rendered ({lit} px) — winding likely inverted (front faces culled)"
    );
}
