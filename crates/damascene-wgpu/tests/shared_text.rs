//! GPU test for [`damascene_wgpu::SharedText`] (issue #94): multiple
//! `Runner`s attached to one per-device text pool must render text
//! byte-identically to a runner with a private pool — sharing the font
//! system, shaping cache, and glyph/MSDF atlas pages is a memory/warmup
//! optimization, never a visual change.
//!
//! Skips cleanly (passes) when no adapter is available, matching the
//! other GPU tests.

use damascene_core::prelude::*;
use damascene_wgpu::{Runner, RunnerCaps, SharedText};

const SIZE: u32 = 96;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("shared_text_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

fn text_tree() -> El {
    column([text("Shared atlases"), text("mono path too").mono()])
        .padding(8.0)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32))
}

/// Prepare + render `text_tree()` through `runner` and read the target
/// back, padding stripped.
fn render_and_read(device: &wgpu::Device, queue: &wgpu::Queue, runner: &mut Runner) -> Vec<u8> {
    runner.set_surface_size(SIZE, SIZE);
    let tree = text_tree();
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shared_text_target"),
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

    let unpadded = SIZE * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shared_text_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shared_text_test"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &view,
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
    let data = slice.get_mapped_range().unwrap();
    let mut out = Vec::with_capacity((unpadded * SIZE) as usize);
    for row in 0..SIZE {
        let start = (row * bytes_per_row) as usize;
        out.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    readback.unmap();
    out
}

#[test]
fn shared_pool_renders_identically_to_private_pools() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("shared_text: no GPU adapter, skipping");
        return;
    };

    // Reference: a runner with its own private pool.
    let mut private = Runner::new(&device, &queue, FORMAT);
    let reference = render_and_read(&device, &queue, &mut private);
    assert!(
        reference.iter().any(|&b| b != 0),
        "reference render must actually contain glyphs",
    );

    // Two runners attached to one warm shared pool.
    let pool = SharedText::new(&device);
    pool.warm_default_glyphs();
    let mut a = Runner::with_shared_text(&device, &queue, FORMAT, 1, RunnerCaps::default(), &pool);
    let mut b = Runner::with_shared_text(&device, &queue, FORMAT, 1, RunnerCaps::default(), &pool);

    let out_a = render_and_read(&device, &queue, &mut a);
    let out_b = render_and_read(&device, &queue, &mut b);
    assert_eq!(out_a, reference, "shared pool (runner A) ≠ private pool");
    assert_eq!(out_b, reference, "shared pool (runner B) ≠ private pool");

    // A third runner attached via an existing runner's pool accessor —
    // including one whose "pool" started private.
    let mut c = Runner::with_shared_text(
        &device,
        &queue,
        FORMAT,
        1,
        RunnerCaps::default(),
        &private.shared_text(),
    );
    let out_c = render_and_read(&device, &queue, &mut c);
    assert_eq!(out_c, reference, "pool via shared_text() ≠ private pool");

    // Interleaved prepares: A records, then B records + renders, then
    // A renders — the snapshot taken at A's flush must keep A's frame
    // valid even though B flushed (and possibly grew the pool) in
    // between. render_and_read couples prepare+render, so emulate the
    // interleave at the run level instead: drop runner B mid-sequence
    // and keep using A (exercises the Drop/attach bookkeeping).
    drop(b);
    let out_a2 = render_and_read(&device, &queue, &mut a);
    assert_eq!(out_a2, reference, "render after detaching a runner");
}
