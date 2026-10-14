//! Readback regression test for thin-geometry coverage.
//!
//! `rounded_rect.wgsl` antialiases with a band CENTERED on the SDF
//! boundary (±aa/2). The prior inside-only band ([-aa, 0]) capped a
//! 1px fill's coverage at ~50% and a 2px fill's at ~75% at every scale
//! factor, washing out hairline rules and per-side borders — the
//! workbench "regions meet on 1px borders" seams measured at 25–35% of
//! their intended contrast. These assertions hold the centered band's
//! contract: integer-aligned thin fills rasterize at full value, and
//! the band does not bleed past the boundary.
//!
//! Skips cleanly (passes) when no adapter is available.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const W: u32 = 64;
const H: u32 = 64;
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
        label: Some("hairline_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

/// Render a white horizontal line of `thickness` logical px whose top
/// edge sits at integer y=16, on black; return red bytes row-major.
fn render_line(device: &wgpu::Device, queue: &wgpu::Queue, thickness: f32) -> Vec<u8> {
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(W, H);
    let tree = column([
        El::new(Kind::Group)
            .height(Size::Fixed(16.0))
            .width(Size::Fixed(W as f32)),
        El::new(Kind::Group)
            .fill(Color::srgb_u8(255, 255, 255))
            .height(Size::Fixed(thickness))
            .width(Size::Fixed(W as f32)),
    ])
    .width(Size::Fixed(W as f32))
    .height(Size::Fixed(H as f32));
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hairline_render_target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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
    let unpadded = W * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hairline_render_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("hairline_render"),
    });
    runner.render(
        device,
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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
    let mut reds = Vec::with_capacity((W * H) as usize);
    for row in 0..H {
        let off = (row * bytes_per_row) as usize;
        for px in 0..W {
            reds.push(data[off + (px * 4) as usize]);
        }
    }
    drop(data);
    readback.unmap();
    reds
}

fn row_center(px: &[u8], y: u32) -> u8 {
    px[(y * W + W / 2) as usize]
}

#[test]
fn integer_aligned_thin_fills_reach_full_value() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no adapter; skipping");
        return;
    };

    // 1px hairline occupying row 16: full value on its row, black on
    // both neighbors (no bleed past the boundary).
    let px = render_line(&device, &queue, 1.0);
    assert!(
        row_center(&px, 16) >= 250,
        "1px hairline dimmed: row 16 = {} (inside-band AA regression?)",
        row_center(&px, 16)
    );
    assert!(
        row_center(&px, 15) <= 5 && row_center(&px, 17) <= 5,
        "1px hairline bleeds: rows 15/17 = {}/{}",
        row_center(&px, 15),
        row_center(&px, 17)
    );

    // 2px rule occupying rows 16..18: both rows full.
    let px = render_line(&device, &queue, 2.0);
    assert!(
        row_center(&px, 16) >= 250 && row_center(&px, 17) >= 250,
        "2px rule dimmed: rows 16/17 = {}/{}",
        row_center(&px, 16),
        row_center(&px, 17)
    );
    assert!(
        row_center(&px, 15) <= 5 && row_center(&px, 18) <= 5,
        "2px rule bleeds: rows 15/18 = {}/{}",
        row_center(&px, 15),
        row_center(&px, 18)
    );
}
