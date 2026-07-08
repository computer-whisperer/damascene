//! Readback regression test for per-weight glyph rasterization.
//!
//! Through 0.4.6 the MSDF cache was keyed by `(font, glyph)` only and
//! never applied the `wght` variation, so every Medium/Semibold/Bold
//! label painted Regular outlines (with heavier-weight advances) — all
//! emphasized text rendered visibly lighter than intended. This test
//! drives the real record→atlas→shader path headlessly at two weights
//! and requires the bold frame to carry measurably more ink than the
//! regular one.
//!
//! Skips cleanly (passes) when no adapter is available, so CI without a
//! GPU doesn't fail — but it runs for real wherever a Vulkan/Metal/DX
//! adapter exists.

use damascene_core::prelude::*;
use damascene_core::tree::FontWeight;
use damascene_wgpu::Runner;

const W: u32 = 160;
const H: u32 = 48;
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
        label: Some("text_weight_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

/// Render one frame of black `text` at `weight` on a white backdrop and
/// return the frame's total ink: Σ (255 − r) over every pixel, i.e. 0
/// for an empty frame and larger the more/heavier the glyph coverage.
fn ink_for_weight(device: &wgpu::Device, queue: &wgpu::Queue, weight: FontWeight) -> u64 {
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(W, H);

    let tree = stack([text("HHHH")
        .font_size(28.0)
        .font_weight(weight)
        .text_color(Color::srgb_u8(0, 0, 0))])
    .fill(Color::srgb_u8(255, 255, 255))
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
        label: Some("text_weight_render_target"),
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
        label: Some("text_weight_render_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("text_weight_render"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &target_view,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
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
    let mut ink: u64 = 0;
    for row in 0..H {
        let off = (row * bytes_per_row) as usize;
        for px in 0..W {
            ink += 255 - u64::from(data[off + (px * 4) as usize]);
        }
    }
    drop(data);
    readback.unmap();
    ink
}

#[test]
fn bold_text_renders_more_ink_than_regular() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("text_weight_render: no GPU adapter, skipping");
        return;
    };

    let regular = ink_for_weight(&device, &queue, FontWeight::Regular);
    let bold = ink_for_weight(&device, &queue, FontWeight::Bold);

    eprintln!("text_weight_render: regular ink {regular}, bold ink {bold}");
    assert!(regular > 0, "regular frame must contain glyphs");
    // Inter's Bold stems are ~1.7× Regular's; even with AA and identical
    // counters a 20% total-ink margin is far below the true delta while
    // sitting far above frame-to-frame noise (rendering is
    // deterministic). Pre-fix, both frames used identical Regular
    // outlines and differed only by advance-driven subpixel phase — well
    // under this margin.
    assert!(
        bold as f64 >= regular as f64 * 1.20,
        "bold ink ({bold}) must exceed regular ink ({regular}) by ≥20% — \
         equal ink means the wght variation never reached the rasterizer"
    );
}
