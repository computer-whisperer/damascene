//! Headless end-to-end render test for the Scene3D pipeline.
//!
//! Builds a real wgpu device, renders a `chart3d` (lit cube + point
//! scatter + a line) into an offscreen target, reads the pixels back, and
//! asserts the scene actually composited content into its rect. This is the
//! only place the scene WGSL is compiled (naga validation) and the whole
//! offscreen → resolve → composite path exercises against the GPU.
//!
//! Skips cleanly (passes) when no adapter is available, so CI without a GPU
//! doesn't fail — but it runs for real wherever a Vulkan/Metal/DX adapter
//! exists.

use damascene_core::prelude::*;
use damascene_core::scene::glam::Vec3;
use damascene_core::scene::{
    GridPlanes, GridSettings, LineData, LineSegment, LinesHandle, MeshData, MeshHandle, MeshVertex,
    PointData, PointStyle, PointsHandle, ScenePoint, SceneSpec, SceneStyle,
};
use damascene_wgpu::Runner;

const SIZE: u32 = 160;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let backend = format!("{:?}", adapter.get_info().backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("scene3d_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue, backend))
}

/// One cube face: (outward normal, 4 corner positions).
type Face = ([f32; 3], [(f32, f32, f32); 4]);

/// Unit cube centred at the origin, side 2, with per-face outward normals
/// so back-face culling + directional lighting both have something to bite.
fn cube() -> MeshData {
    // (normal, 4 verts) per face.
    let faces: [Face; 6] = [
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

/// UV sphere with smooth (position-direction) normals. Winding must be CCW
/// when viewed from outside so back-face culling keeps the *front* faces —
/// this test guards exactly that (an inverted sphere renders near-empty).
/// The `scene3d` example uses the identical winding.
fn uv_sphere(radius: f32, rings: u32, sectors: u32) -> MeshData {
    use std::f32::consts::{PI, TAU};
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=rings {
        let theta = i as f32 / rings as f32 * PI; // 0 (top) .. PI (bottom)
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
fn uv_sphere_winds_outward() {
    let Some((device, queue, _)) = headless_device() else {
        eprintln!("uv_sphere_winds_outward: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    let mesh = MeshHandle::new(uv_sphere(1.0, 24, 32));
    let mut tree = chart3d(SceneSpec::new().mesh(mesh).no_grid());
    let lit = render_and_count_lit(&device, &queue, &mut runner, &mut tree);
    // A framed sphere should fill a big fraction of the view. Inverted
    // winding (front faces culled) collapses this to near-zero.
    eprintln!("uv_sphere_winds_outward: {lit}/{} lit", (SIZE * SIZE));
    assert!(
        lit > (SIZE * SIZE) as usize / 6,
        "sphere barely rendered ({lit} px) — winding likely inverted (front faces culled)"
    );
}

#[test]
fn transparent_background_composites_over_backdrop() {
    let Some((device, queue, _)) = headless_device() else {
        eprintln!("transparent_background: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    // Just an opaque cube — no grid, no axes — so the corners stay empty
    // and `background: None` leaves them transparent.
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::NONE,
            ..Default::default()
        },
        background: None,
        msaa_samples: 4,
        show_axes: false,
    };
    let mesh = MeshHandle::new(cube());
    let mut on_black_tree = chart3d(SceneSpec::new().mesh(mesh.clone()).style(style));
    let mut on_purple_tree = chart3d(SceneSpec::new().mesh(mesh).style(style));

    let purple = wgpu::Color {
        r: 0.10,
        g: 0.02,
        b: 0.45,
        a: 1.0,
    };
    let on_black = render_to_pixels(
        &device,
        &queue,
        &mut runner,
        &mut on_black_tree,
        wgpu::Color::BLACK,
    );
    let on_purple = render_to_pixels(&device, &queue, &mut runner, &mut on_purple_tree, purple);

    let at = |x: u32, y: u32, buf: &[u8]| {
        let i = ((y * SIZE + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2]]
    };

    // Corner: scene drew nothing, so the backdrop shows straight through.
    // Over black it's ~black; over purple it's the purple backdrop.
    let corner_black = at(2, 2, &on_black);
    let corner_purple = at(2, 2, &on_purple);
    assert!(
        corner_black.iter().all(|&v| v < 16),
        "transparent corner over black should stay ~black, got {corner_black:?}"
    );
    assert!(
        corner_purple[2] > 120
            && corner_purple[2] > corner_purple[0]
            && corner_purple[2] > corner_purple[1],
        "transparent corner must show the purple backdrop, got {corner_purple:?}"
    );

    // Centre: opaque mesh covers the backdrop, so it's identical either way.
    let mid = SIZE / 2;
    let centre_black = at(mid, mid, &on_black);
    let centre_purple = at(mid, mid, &on_purple);
    assert!(
        centre_black.iter().any(|&v| v > 24),
        "centre should carry mesh content, got {centre_black:?}"
    );
    let independent = centre_black
        .iter()
        .zip(&centre_purple)
        .all(|(a, b)| (*a as i32 - *b as i32).abs() <= 4);
    assert!(
        independent,
        "opaque mesh centre must not depend on the backdrop: {centre_black:?} vs {centre_purple:?}"
    );
}

#[test]
fn scene3d_composites_visible_content() {
    let Some((device, queue, backend)) = headless_device() else {
        eprintln!("scene3d_render: no GPU adapter, skipping");
        return;
    };
    eprintln!("scene3d_render: using {backend} adapter");

    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    let mesh: MeshHandle = MeshHandle::new(cube());
    let points: PointsHandle = PointsHandle::new(PointData {
        points: vec![
            ScenePoint {
                position: Vec3::new(2.0, 0.0, 0.0),
                color: [1.0, 0.2, 0.2, 1.0],
            },
            ScenePoint {
                position: Vec3::new(0.0, 2.0, 0.0),
                color: [0.2, 1.0, 0.2, 1.0],
            },
            ScenePoint {
                position: Vec3::new(0.0, 0.0, 2.0),
                color: [0.3, 0.4, 1.0, 1.0],
            },
        ],
    });
    let lines: LinesHandle = LinesHandle::new(LineData {
        segments: vec![LineSegment {
            start: Vec3::new(-2.0, -2.0, 0.0),
            end: Vec3::new(2.0, 2.0, 0.0),
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
    let lit = render_and_count_lit(&device, &queue, &mut runner, &mut tree);
    let total = (SIZE * SIZE) as usize;
    eprintln!("scene3d_render: {lit}/{total} non-black pixels");
    assert!(
        lit > total / 100,
        "scene composited almost nothing ({lit}/{total} lit) — offscreen render or composite is broken"
    );
}

/// The backend captures a per-scene depth map (for label occlusion) and
/// streams it back to the CPU a few frames late. This drives several frames
/// until the map lands, then checks it actually encodes the geometry and
/// that `SceneDepthMap::occludes` agrees: the framed cube is captured (centre
/// near, corner far), a point inside it is occluded, a point by the eye isn't.
#[test]
fn scene_depth_map_captures_geometry_for_occlusion() {
    let Some((device, queue, _)) = headless_device() else {
        eprintln!("scene_depth_map: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    // Axis labels (`axis_titles`) flag the scene for depth capture.
    let mesh = MeshHandle::new(cube());
    let mut tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    // The read-back is async (one capture in flight, mapped a frame later),
    // so pump frames until the map appears.
    let mut captured = None;
    for _ in 0..10 {
        let _ = render_to_pixels(&device, &queue, &mut runner, &mut tree, wgpu::Color::BLACK);
        device.poll(wgpu::PollType::wait_indefinitely()).ok();
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
    eprintln!("scene_depth_map: centre={center}, corner={corner}");
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

/// Damascene renders lazily, so a labelled scene must keep requesting redraws
/// until its async depth read-back resolves — otherwise a capture started in
/// `render` sits unmapped after the camera settles and the labels never show.
///
/// The signal that actually schedules the next frame is the *layout* redraw
/// deadline (`next_layout_redraw_in == Some(ZERO)`), not `needs_redraw` (the
/// winit host ignores that), and it must be the layout lane — only a full
/// `prepare` advances the read-back, the paint-only path doesn't. This checks
/// the first frame parks a zero layout deadline, the loop settles to no
/// deadline (lazy idle preserved), and a depth map exists once it does.
#[test]
fn occlusion_keeps_redrawing_until_depth_resolves() {
    let Some((device, queue, _)) = headless_device() else {
        eprintln!("occlusion_redraw: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    let mesh = MeshHandle::new(cube());
    let mut tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    let first = pump_frame(&device, &queue, &mut runner, &mut tree);
    assert_eq!(
        first,
        Some(std::time::Duration::ZERO),
        "a labelled scene must park a zero layout-redraw deadline until its depth map resolves"
    );

    let mut settled = false;
    for _ in 0..16 {
        if pump_frame(&device, &queue, &mut runner, &mut tree).is_none() {
            settled = true;
            break;
        }
    }
    assert!(
        settled,
        "occlusion redraw loop never settled (would spin forever)"
    );
    assert!(
        runner.ui_state().scene_depth_maps().next().is_some(),
        "a depth map should exist once the scene settles"
    );
}

/// With `depth_readback == false` (the WebGL2 degradation — naga's GLSL
/// target can't `textureLoad` depth textures, so the resolve pipeline must
/// never be built), a labelled scene must still settle: `collect_depth_maps`
/// synthesizes a 1×1 all-far map so labels render unoccluded instead of
/// being hidden forever by the occlude-on-missing fail-safe.
#[test]
fn no_depth_readback_settles_with_synthetic_map() {
    let Some((device, queue, _)) = headless_device() else {
        eprintln!("no_depth_readback: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::with_caps(&device, &queue, FORMAT, 1, true, false);
    runner.set_surface_size(SIZE, SIZE);
    let mesh = MeshHandle::new(cube());
    let mut tree = chart3d(
        SceneSpec::new()
            .mesh(mesh)
            .no_grid()
            .axis_titles("X", "Y", "Z"),
    );

    let first = pump_frame(&device, &queue, &mut runner, &mut tree);
    assert_eq!(
        first,
        Some(std::time::Duration::ZERO),
        "the first frame still owes a (synthetic) depth map"
    );

    let mut settled = false;
    for _ in 0..4 {
        if pump_frame(&device, &queue, &mut runner, &mut tree).is_none() {
            settled = true;
            break;
        }
    }
    assert!(settled, "synthetic-map path never settled");
    let (_, map) = runner
        .ui_state()
        .scene_depth_maps()
        .next()
        .expect("a synthetic depth map is installed");
    assert_eq!(
        (map.width, map.height),
        (1, 1),
        "degraded path installs the 1×1 all-far map"
    );
    assert!(
        !map.occludes(Vec3::ZERO),
        "the all-far map must not occlude a point in view"
    );
}

/// Run one full frame (prepare → render → submit → wait) and return the
/// layout-redraw deadline the host schedules off (`Some(ZERO)` = redraw now,
/// `None` = idle).
fn pump_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    runner: &mut Runner,
    tree: &mut El,
) -> Option<std::time::Duration> {
    let res = runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("occlusion_redraw_target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("occlusion_redraw"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &view,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
    );
    queue.submit([encoder.finish()]);
    device.poll(wgpu::PollType::wait_indefinitely()).ok();
    res.next_layout_redraw_in
}

/// Prepare + render `tree` into a fresh target and count pixels brighter
/// than the black clear — i.e. content the scene drew + composited.
fn render_and_count_lit(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    runner: &mut Runner,
    tree: &mut El,
) -> usize {
    let px = render_to_pixels(device, queue, runner, tree, wgpu::Color::BLACK);
    px.chunks_exact(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 24)
        .count()
}

/// Prepare + render `tree` over `clear` into a fresh target; return tightly
/// packed RGBA (SIZE*SIZE*4, no row padding) for per-pixel inspection.
fn render_to_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    runner: &mut Runner,
    tree: &mut El,
    clear: wgpu::Color,
) -> Vec<u8> {
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene3d_test_target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    // Row pitch must respect COPY_BYTES_PER_ROW_ALIGNMENT (256); pad and
    // stride over the padding on readback.
    let unpadded = SIZE * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene3d_test_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("scene3d_test"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &target_view,
        None,
        wgpu::LoadOp::Clear(clear),
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range();

    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for row in 0..SIZE as usize {
        let start = row * bytes_per_row as usize;
        out.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    readback.unmap();
    out
}
