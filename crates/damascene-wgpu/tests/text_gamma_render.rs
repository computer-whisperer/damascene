//! Readback regression test for gamma-aware glyph coverage.
//!
//! Browsers composite glyph masks in encoded sRGB space; damascene's
//! targets blend in linear light. Uncompensated, identical coverage
//! renders dark-on-light text visibly thinner and light-on-dark text
//! fatter than the browser reference — the canonical GPU-text delta.
//! `text_msdf.wgsl` remaps coverage toward the gamma-space result
//! (assuming a contrasting background, exact for black-on-white and
//! white-on-black); this test locks the asymmetry in.
//!
//! Skips cleanly (passes) when no adapter is available.

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
        label: Some("text_gamma_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

/// Render black-on-white or white-on-black "HHHH" and return the red
/// channel of every pixel (sRGB-encoded bytes).
fn render(device: &wgpu::Device, queue: &wgpu::Queue, dark_text: bool) -> Vec<u8> {
    let (fg, bg) = if dark_text {
        (Color::srgb_u8(0, 0, 0), Color::srgb_u8(255, 255, 255))
    } else {
        (Color::srgb_u8(255, 255, 255), Color::srgb_u8(0, 0, 0))
    };
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(W, H);
    let tree = stack([text("HHHH")
        .font_size(28.0)
        .font_weight(FontWeight::Regular)
        .text_color(fg)])
    .fill(bg)
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
        label: Some("text_gamma_render_target"),
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
        label: Some("text_gamma_render_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("text_gamma_render"),
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

#[test]
fn coverage_remap_matches_gamma_space_compositing_direction() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("text_gamma_render: no GPU adapter, skipping");
        return;
    };

    let dark_on_light = render(&device, &queue, true);
    let light_on_dark = render(&device, &queue, false);

    // Ink carried by the glyphs in each polarity: darkness added on the
    // white frame, lightness added on the black frame. Identical glyph
    // geometry, so any difference comes from how edge coverage
    // composites.
    let darkness: u64 = dark_on_light.iter().map(|&r| 255 - u64::from(r)).sum();
    let lightness: u64 = light_on_dark.iter().map(|&r| u64::from(r)).sum();

    // Full-coverage interiors must stay pure — the remap only reshapes
    // partial coverage, never the glyph body.
    assert!(
        dark_on_light.contains(&0),
        "black-on-white glyphs must reach pure black in their interior"
    );
    assert!(
        light_on_dark.contains(&255),
        "white-on-black glyphs must reach pure white in their interior"
    );

    let ratio = darkness as f64 / lightness as f64;
    eprintln!("text_gamma_render: darkness {darkness}, lightness {lightness}, ratio {ratio:.3}");

    // In gamma-space compositing (the browser look) dark-on-light edges
    // carry noticeably more ink than light-on-dark edges carry light.
    // Measured before the shader remap (pure linear compositing) the
    // ratio was 1.004 — symmetric to within noise; with the remap it
    // must clear a distinctly higher bar. Rendering is deterministic,
    // so a fixed threshold is stable.
    assert!(
        ratio >= 1.30,
        "darkness/lightness ratio {ratio:.3} too low — gamma-aware \
         coverage remap is not reaching the shader"
    );
}
