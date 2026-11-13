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
fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: El) -> Vec<u8> {
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
        .as_chunks::<4>()
        .0
        .iter()
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

    let tree = chart3d(spec);
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("scene3d_render(vulkano): {lit}/{total} non-black pixels");
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
        eprintln!("lit_cube_shades(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

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
    let (lit, p10, p90) = cube_face_shading(&px);
    let total = (SIZE * SIZE) as usize;
    eprintln!("lit_cube_shades(vulkano): {lit}/{total} lit, luminance p10={p10} p90={p90}");

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

    let a = render_to_pixels(&gpu, &mut runner, with_axes(true));
    let b = render_to_pixels(&gpu, &mut runner, with_axes(false));

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
    let tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    // The read-back is a frame late (capture recorded in `render`, read in the
    // next `prepare`), so pump frames until the map appears.
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
    let tree = chart3d(SceneSpec::new().mesh(mesh).no_grid());
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
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
    let tree = image(red)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32));
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
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
    let tree = image(white)
        .image_tint(tint)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32));
    let pixels = render_to_pixels(&gpu, &mut runner, tree);

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
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);
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

/// An 8×8 app texture cleared to `rgba` (UNORM, so the values are the
/// ones the shader samples).
fn solid_texture(gpu: &Gpu, rgba: [f32; 4]) -> AppTexture {
    use vulkano::command_buffer::ClearColorImageInfo;
    use vulkano::format::ClearColorValue;
    let image = Image::new(
        gpu.memory_alloc.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: Format::R8G8B8A8_UNORM,
            extent: [8, 8, 1],
            usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .expect("texture image");
    let mut builder = AutoCommandBufferBuilder::primary(
        gpu.cmd_alloc.clone(),
        gpu.queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .expect("command buffer builder");
    builder
        .clear_color_image(ClearColorImageInfo {
            clear_value: ClearColorValue::Float(rgba),
            ..ClearColorImageInfo::image(image.clone())
        })
        .expect("clear texture");
    sync::now(gpu.device.clone())
        .then_execute(gpu.queue.clone(), builder.build().expect("build clear"))
        .expect("execute clear")
        .then_signal_fence_and_flush()
        .expect("flush clear")
        .wait(None)
        .expect("wait clear");
    damascene_vulkano::app_texture(image)
}

#[test]
fn a_painted_vector_fades_with_its_node() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("opacity(vulkano): no Vulkan device, skipping");
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
        eprintln!("opacity(vulkano): no Vulkan device, skipping");
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
        eprintln!("opacity(vulkano): no Vulkan device, skipping");
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
        let tex = solid_texture(&gpu, rgba);
        let full = assert_fades_halfway(&gpu, &format!("app texture, {what}"), &|opacity| {
            surface(tex.clone())
                .width(Size::Fixed(120.0))
                .height(Size::Fixed(120.0))
                .surface_alpha(alpha)
                .opacity(opacity)
        });
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
        eprintln!("opacity(vulkano): no Vulkan device, skipping");
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
        eprintln!("opacity(vulkano): no Vulkan device, skipping");
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
        eprintln!("opacity(vulkano): no Vulkan device, skipping");
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
        eprintln!("shadow(vulkano): no Vulkan device, skipping");
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
