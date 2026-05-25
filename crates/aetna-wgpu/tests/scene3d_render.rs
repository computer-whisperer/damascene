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

use aetna_core::prelude::*;
use aetna_core::scene::glam::Vec3;
use aetna_core::scene::{
    LineData, LineSegment, LinesHandle, MeshData, MeshHandle, MeshVertex, PointData, PointStyle,
    PointsHandle, SceneSpec, ScenePoint,
};
use aetna_wgpu::Runner;

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

/// Unit cube centred at the origin, side 2, with per-face outward normals
/// so back-face culling + directional lighting both have something to bite.
fn cube() -> MeshData {
    // (position, normal) per face, 4 verts/face.
    let faces: [([f32; 3], [(f32, f32, f32); 4]); 6] = [
        ([0.0, 0.0, 1.0], [(-1., -1., 1.), (1., -1., 1.), (1., 1., 1.), (-1., 1., 1.)]),
        ([0.0, 0.0, -1.0], [(1., -1., -1.), (-1., -1., -1.), (-1., 1., -1.), (1., 1., -1.)]),
        ([1.0, 0.0, 0.0], [(1., -1., 1.), (1., -1., -1.), (1., 1., -1.), (1., 1., 1.)]),
        ([-1.0, 0.0, 0.0], [(-1., -1., -1.), (-1., -1., 1.), (-1., 1., 1.), (-1., 1., -1.)]),
        ([0.0, 1.0, 0.0], [(-1., 1., 1.), (1., 1., 1.), (1., 1., -1.), (-1., 1., -1.)]),
        ([0.0, -1.0, 0.0], [(-1., -1., -1.), (1., -1., -1.), (1., -1., 1.), (-1., -1., 1.)]),
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
    MeshData { vertices, indices: Some(indices) }
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
            ScenePoint { position: Vec3::new(2.0, 0.0, 0.0), color: [1.0, 0.2, 0.2, 1.0] },
            ScenePoint { position: Vec3::new(0.0, 2.0, 0.0), color: [0.2, 1.0, 0.2, 1.0] },
            ScenePoint { position: Vec3::new(0.0, 0.0, 2.0), color: [0.3, 0.4, 1.0, 1.0] },
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
        .points_styled(points, PointStyle { size: 14.0, ..Default::default() })
        .lines(lines);

    let mut tree = chart3d(spec);

    runner.prepare(
        &device,
        &queue,
        &mut tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );

    // Target + readback buffer.
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene3d_test_target"),
        size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
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
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let bytes_per_row = unpadded.div_ceil(align) * align;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene3d_test_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("scene3d_test") });
    // Clear to opaque black; the scene composites premultiplied over it.
    runner.render(
        &device,
        &mut encoder,
        &target,
        &target_view,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range();

    // Count pixels brighter than the black clear — i.e. content the scene
    // drew + composited. Anything more than a sliver proves the offscreen
    // render, resolve, and composite all ran.
    let mut lit = 0usize;
    for row in 0..SIZE as usize {
        let start = row * bytes_per_row as usize;
        let row_pixels = &data[start..start + unpadded as usize];
        for px in row_pixels.chunks_exact(4) {
            if px[0] as u32 + px[1] as u32 + px[2] as u32 > 24 {
                lit += 1;
            }
        }
    }
    let total = (SIZE * SIZE) as usize;
    drop(data);
    readback.unmap();

    eprintln!("scene3d_render: {lit}/{total} non-black pixels");
    assert!(
        lit > total / 100,
        "scene composited almost nothing ({lit}/{total} lit) — offscreen render or composite is broken"
    );
}
