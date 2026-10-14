//! Readback regression test for per-side borders on rounded corners.
//!
//! The border must follow the corner arc (CSS: outer radius r, inner
//! r − w). The first implementation painted each side as a straight
//! quad spanning the side minus the adjacent radii, leaving the arcs
//! bare: at radius 8 the bottom-left arc pixel below read background.
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
        label: Some("border_corner_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

/// A 64×40 box at the origin with radius 8 and a white bottom border,
/// on black; returns red bytes row-major.
fn render_border_b(device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(W, H);
    let tree = column([El::new(Kind::Group)
        .border_b()
        .border_color(Color::srgb_u8(255, 255, 255))
        .radius(8.0)
        .width(Size::Fixed(W as f32))
        .height(Size::Fixed(40.0))])
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
        label: Some("border_corner_target"),
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
        label: Some("border_corner_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("border_corner_render"),
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

fn at(px: &[u8], x: u32, y: u32) -> u8 {
    px[(y * W + x) as usize]
}

#[test]
fn bottom_border_follows_the_rounded_corner() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let px = render_border_b(&device, &queue);
    // Straight run: the bottom row of the box is the border.
    assert!(
        at(&px, 32, 39) >= 250,
        "straight run row 39 = {}",
        at(&px, 32, 39)
    );
    // Arc: pixel (2, 37) has its centre 7.78px from the bottom-left arc
    // centre (8, 32), inside the 1px ring at radius 7–8.
    assert!(
        at(&px, 2, 37) >= 128,
        "bottom-left arc pixel (2,37) = {} — the border does not follow the corner",
        at(&px, 2, 37)
    );
    // Outside the rounded shape at the very corner: nothing.
    assert!(
        at(&px, 0, 39) <= 5,
        "corner outside the arc = {}",
        at(&px, 0, 39)
    );
    // Interior: the node has no fill, and the ring is scissored to the
    // bottom band — rows above it stay black, including the left edge.
    assert!(at(&px, 32, 20) <= 5, "interior = {}", at(&px, 32, 20));
    assert!(
        at(&px, 0, 20) <= 5,
        "left edge above the band = {}",
        at(&px, 0, 20)
    );
    assert!(at(&px, 32, 41) <= 5, "below the box = {}", at(&px, 32, 41));
}
