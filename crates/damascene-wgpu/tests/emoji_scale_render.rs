//! Readback regression test for colour-emoji sizing under hidpi.
//!
//! Through 0.4.6 `push_color_glyph` divided bitmap metrics by
//! `scale_factor` while the atlas rasterized at *logical* size — the
//! two only agreed at scale 1, so emoji drew at half size on 2×
//! displays. The recorder now ensures colour glyphs at physical size
//! (`GlyphKey::at_scale`); this test renders the same logical-size
//! emoji at scale 1 and scale 2 and requires the scale-2 footprint to
//! quadruple in physical pixels (double per axis), which the old code
//! failed (≈1× — same physical size at both scales).
//!
//! Skips cleanly (passes) when no adapter is available.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const LOGICAL: u32 = 64;
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
        label: Some("emoji_scale_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

/// Render a 32px-logical emoji on a white backdrop at `scale_factor`
/// and return how many physical pixels differ from the backdrop.
fn emoji_pixels(device: &wgpu::Device, queue: &wgpu::Queue, scale_factor: f32) -> u64 {
    let pw = (LOGICAL as f32 * scale_factor) as u32;
    let ph = pw;
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(pw, ph);

    let tree = stack([text("🙂").font_size(32.0)])
        .fill(Color::srgb_u8(255, 255, 255))
        .width(Size::Fixed(LOGICAL as f32))
        .height(Size::Fixed(LOGICAL as f32));
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, LOGICAL as f32, LOGICAL as f32),
        scale_factor,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("emoji_scale_render_target"),
        size: wgpu::Extent3d {
            width: pw,
            height: ph,
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
    let unpadded = pw * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("emoji_scale_render_readback"),
        size: (bytes_per_row * ph) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("emoji_scale_render"),
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
                rows_per_image: Some(ph),
            },
        },
        wgpu::Extent3d {
            width: pw,
            height: ph,
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
    let mut count: u64 = 0;
    for row in 0..ph {
        let off = (row * bytes_per_row) as usize;
        for px in 0..pw {
            let p = off + (px * 4) as usize;
            // Anything visibly off-white counts as emoji coverage.
            if data[p] < 245 || data[p + 1] < 245 || data[p + 2] < 245 {
                count += 1;
            }
        }
    }
    drop(data);
    readback.unmap();
    count
}

#[test]
fn emoji_doubles_per_axis_at_scale_two() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("emoji_scale_render: no GPU adapter, skipping");
        return;
    };

    let at_1x = emoji_pixels(&device, &queue, 1.0);
    let at_2x = emoji_pixels(&device, &queue, 2.0);

    eprintln!("emoji_scale_render: 1x pixels {at_1x}, 2x pixels {at_2x}");
    assert!(at_1x > 100, "scale-1 frame must contain an emoji");
    // Correct hidpi rendering quadruples the physical footprint (2× per
    // axis). The old bug rendered the same physical size at both scales
    // (ratio ≈ 1); a 2.5× floor separates the two regimes with margin
    // for AA and bitmap-strike quantization.
    let ratio = at_2x as f64 / at_1x as f64;
    assert!(
        ratio >= 2.5,
        "scale-2 emoji footprint must grow ~4× over scale-1 (got {ratio:.2}×) — \
         a ~1× ratio means colour bitmaps are still ensured at logical size"
    );
}
