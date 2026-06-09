//! Headless end-to-end render test for the Scene3D pipeline on vulkano.
//!
//! Twin of `damascene-wgpu`'s `scene3d_render`. Builds a real vulkano device,
//! renders a `chart3d` (lit mesh + point scatter + a line + grid) into an
//! offscreen target, reads the pixels back, and asserts the scene actually
//! composited content into its rect. This is the only place the scene WGSL is
//! compiled through naga on the vulkano side, and the whole offscreen →
//! resolve → composite path runs against the GPU.
//!
//! Skips cleanly (passes) when no Vulkan device is available, so CI without a
//! GPU doesn't fail.

use std::sync::Arc;

use damascene_core::prelude::*;
use damascene_core::scene::glam::Vec3;
use damascene_core::scene::{
    GridPlanes, GridSettings, LineData, LineSegment, LinesHandle, Material, MeshData, MeshHandle,
    MeshVertex, PointData, PointStyle, PointsHandle, ScenePoint, SceneSpec, SceneStyle,
};
use damascene_core::{AnimationMode, Rect};
use damascene_vulkano::Runner;
use vulkano::{
    VulkanLibrary,
    buffer::{Buffer, BufferCreateInfo, BufferUsage},
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo,
        allocator::StandardCommandBufferAllocator,
    },
    device::{Device, DeviceCreateInfo, Queue, QueueCreateInfo, QueueFlags},
    format::Format,
    image::{Image, ImageCreateInfo, ImageType, ImageUsage, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    render_pass::{Framebuffer, FramebufferCreateInfo},
    sync::{self, GpuFuture},
};

const SIZE: u32 = 160;
const FORMAT: Format = Format::R8G8B8A8_SRGB;

struct Gpu {
    device: Arc<Device>,
    queue: Arc<Queue>,
    memory_alloc: Arc<StandardMemoryAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
}

fn headless_gpu() -> Option<Gpu> {
    let library = VulkanLibrary::new().ok()?;
    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            ..Default::default()
        },
    )
    .ok()?;
    let (physical_device, queue_family_index) = instance
        .enumerate_physical_devices()
        .ok()?
        .filter_map(|p| {
            p.queue_family_properties()
                .iter()
                .position(|q| q.queue_flags.intersects(QueueFlags::GRAPHICS))
                .map(|i| (p, i as u32))
        })
        .next()?;
    let (device, mut queues) = Device::new(
        physical_device,
        DeviceCreateInfo {
            enabled_features: damascene_vulkano::required_device_features(),
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .ok()?;
    let queue = queues.next()?;
    let memory_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
    let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));
    Some(Gpu {
        device,
        queue,
        memory_alloc,
        cmd_alloc,
    })
}

/// Render `tree` to an `SIZE×SIZE` offscreen target on a single-sample main
/// pass and return the RGBA8 pixels. The scene applies its own MSAA in its
/// offscreen pass, so the main pass stays single-sample (one attachment).
fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: &mut El) -> Vec<u8> {
    let target = Image::new(
        gpu.memory_alloc.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: FORMAT,
            extent: [SIZE, SIZE, 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .expect("target image");
    let view = ImageView::new_default(target.clone()).expect("target view");
    let framebuffer = Framebuffer::new(
        runner.render_pass().clone(),
        FramebufferCreateInfo {
            attachments: vec![view],
            ..Default::default()
        },
    )
    .expect("framebuffer");

    let viewport = Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32);
    runner.prepare(tree, viewport, 1.0);

    let readback = Buffer::new_slice::<u8>(
        gpu.memory_alloc.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_RANDOM_ACCESS,
            ..Default::default()
        },
        (SIZE * SIZE * 4) as u64,
    )
    .expect("readback buffer");

    let mut builder = AutoCommandBufferBuilder::primary(
        gpu.cmd_alloc.clone(),
        gpu.queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .expect("command buffer builder");
    runner.render(
        &mut builder,
        framebuffer,
        target.clone(),
        [0.0, 0.0, 0.0, 1.0],
    );
    builder
        .copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
            target,
            readback.clone(),
        ))
        .expect("copy image to buffer");
    let command_buffer = builder.build().expect("build command buffer");

    sync::now(gpu.device.clone())
        .then_execute(gpu.queue.clone(), command_buffer)
        .expect("execute")
        .then_signal_fence_and_flush()
        .expect("flush")
        .wait(None)
        .expect("wait");

    readback.read().expect("read pixels").to_vec()
}

/// Count pixels whose RGB rises clearly above the black clear colour.
fn count_lit(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 16 || px[1] > 16 || px[2] > 16)
        .count()
}

/// UV sphere with smooth (position-direction) normals, CCW outward winding —
/// the same geometry the `scene3d` example uses.
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

#[test]
fn scene3d_composites_visible_content() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("scene3d_render(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

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
    eprintln!("scene3d_render(vulkano): {lit}/{total} non-black pixels");
    assert!(
        lit > total / 100,
        "scene composited almost nothing ({lit}/{total} lit) — offscreen render or composite is broken"
    );
}

/// Axis lines nearer than a solid mesh must draw over it — twin of the wgpu
/// `axis_lines_in_front_of_mesh_are_visible` test. The grid/axes batch
/// depth-tests without writing depth, so it must be recorded *after* the
/// opaque meshes; recorded first, a later mesh painted over nearer strokes
/// unconditionally.
#[test]
fn axis_lines_in_front_of_mesh_are_visible() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("axis_over_mesh(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

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

    let a = render_to_pixels(&gpu, &mut runner, &mut with_axes(true));
    let b = render_to_pixels(&gpu, &mut runner, &mut with_axes(false));

    // Pixels that are pure cube in the axes-off render but carry an axis
    // stroke (raised red or green) in the axes-on render.
    let crossing = a
        .chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(pa, pb)| {
            let cube_px = pb[2] > 150 && pb[0] < 60 && pb[1] < 60;
            let axis_px = pa[0] > 120 || pa[1] > 120;
            cube_px && axis_px
        })
        .count();
    eprintln!("axis_over_mesh(vulkano): {crossing} axis-over-cube pixels");
    assert!(
        crossing > 0,
        "axis lines nearer than the mesh must draw over it (0 axis-coloured \
         pixels found on the cube — grid batch likely recorded before meshes)"
    );
}

/// A translucent mesh (material alpha < 1) must not hide geometry behind it.
/// Twin of the wgpu `translucent_mesh_shows_opaque_geometry_through` test: a
/// red unlit cube inside a blue translucent shell listed *first* in the spec.
/// Under an opaque-only mesh path (depth write, spec order) the shell would
/// depth-reject the cube and the centre would read pure blue; through the
/// translucent path the centre shows the red cube tinted by the shell.
#[test]
fn translucent_mesh_shows_opaque_geometry_through() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("translucent_mesh(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

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
    eprintln!("translucent_mesh(vulkano): centre = ({r}, {g}, {b})");
    assert!(
        r > 120,
        "cube must show through the translucent shell, got ({r}, {g}, {b})"
    );
    assert!(
        b > 40,
        "shell must tint the cube behind it, got ({r}, {g}, {b})"
    );
}

/// Axis-aligned unit cube (flat per-face normals, CCW outward). Used by the
/// occlusion test: a framed cube fills the centre and leaves the corners empty,
/// so the centre depth lands on the cube and the corner reads far.
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
/// it back a frame later. This pumps frames until the map lands, then checks it
/// encodes the geometry and that `SceneDepthMap::occludes` agrees: the framed
/// cube is captured (centre near, corner far), a point inside it is occluded, a
/// point by the eye is not. Twin of the wgpu
/// `scene_depth_map_captures_geometry_for_occlusion` test.
#[test]
fn scene_depth_map_captures_geometry_for_occlusion() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("scene_depth_map(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

    // Axis titles flag the scene for depth capture.
    let mesh = MeshHandle::new(cube());
    let mut tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    // The read-back is a frame late (capture recorded in `render`, read in the
    // next `prepare`), so pump frames until the map appears.
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

    let Some((w, h, center, corner, origin_occluded, near_eye_occluded)) = captured else {
        panic!("no scene depth map was captured after pumping frames");
    };
    assert_eq!((w, h), (SIZE, SIZE), "depth map matches the offscreen size");
    eprintln!("scene_depth_map(vulkano): centre={center}, corner={corner}");
    // Centre sits on the cube (nearer than the far plane); the corner is
    // empty background (cleared to far = 1.0).
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
        eprintln!("uv_sphere_winds_outward(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

    let mesh = MeshHandle::new(uv_sphere(1.0, 24, 32));
    let mut tree = chart3d(SceneSpec::new().mesh(mesh).no_grid());
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("uv_sphere_winds_outward(vulkano): {lit}/{total} lit");
    // A framed sphere fills a big fraction of the view; inverted winding
    // (front faces culled) collapses this to near-zero.
    assert!(
        lit > total / 6,
        "sphere barely rendered ({lit} px) — winding likely inverted (front faces culled)"
    );
}

#[test]
fn image_draws_render_via_batched_uploads() {
    // Regression for #60: image textures are staged during `prepare`
    // and copied through the frame's command buffer (recorded by
    // `render` via `record_uploads`) instead of a per-image submit +
    // fence wait. This proves a staged upload lands before the pass
    // samples it — a broken ordering would composite uninitialized
    // (black) memory.
    let Some(gpu) = headless_gpu() else {
        eprintln!("image_render(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

    let red = damascene_core::image::Image::from_rgba8(8, 8, vec![[255u8, 0, 0, 255]; 64].concat());
    let mut tree = image(red)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32));
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);
    let center = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    assert!(
        pixels[center] > 200 && pixels[center + 1] < 40,
        "center pixel should be the uploaded red image, got {:?}",
        &pixels[center..center + 4]
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
        eprintln!("working_color_space(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);
    let p3 = damascene_core::color::ColorSpace::DISPLAY_P3_LINEAR;
    runner.set_working_color_space(p3);

    let white =
        damascene_core::image::Image::from_rgba8(8, 8, vec![[255u8, 255, 255, 255]; 64].concat());
    let tint = Color::srgb_u8(255, 0, 0);
    let mut tree = image(white)
        .image_tint(tint)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32));
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);

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
    eprintln!("working_color_space(vulkano): got {got:?}, expected {expected:?}");
    for (g, e) in got.iter().zip(&expected) {
        assert!(
            (*g as i16 - *e as i16).abs() <= 3,
            "center pixel {got:?} should be the P3-converted tint {expected:?}"
        );
    }
}
