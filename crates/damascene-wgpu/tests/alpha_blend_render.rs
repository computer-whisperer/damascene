//! Readback regression test for translucent-fill compositing.
//!
//! `rounded_rect.wgsl` accumulates its layers premultiplied and must
//! hand the quad pipeline STRAIGHT alpha, because every backend blends
//! `src_alpha / one_minus_src_alpha`. Before 2026-10-14 the shader
//! returned the premultiplied rgb as-is, so a fill with alpha `a`
//! composited at `a²`: a 50% white over white read 225 instead of 255,
//! a 10% hover wash landed at 1%, and a half-covered hairline at a
//! quarter of its value (the kiosk's "1px geometry at roughly a third").
//! These assertions hold the straight-alpha contract at both sample
//! counts users run.
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
        label: Some("alpha_blend_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}


/// Render a full-viewport fill of `color` over `clear`, at the given
/// MSAA sample count; return the red byte at the viewport centre.
fn render_fill_center(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: Color,
    clear: wgpu::Color,
    sample_count: u32,
) -> u8 {
    let mut runner = Runner::with_sample_count(device, queue, FORMAT, sample_count);
    runner.set_surface_size(W, H);
    let tree = El::new(Kind::Group)
        .fill(color)
        .width(Size::Fixed(W as f32))
        .height(Size::Fixed(H as f32));
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
    );

    let make_target = |samples: u32, label: &str| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    let target = make_target(1, "alpha_blend_target");
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let msaa = (sample_count > 1).then(|| make_target(sample_count, "alpha_blend_msaa"));
    let msaa_view = msaa
        .as_ref()
        .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));

    let unpadded = W * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("alpha_blend_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("alpha_blend_render"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &target_view,
        msaa_view.as_ref(),
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
    let center = data[((H / 2) * bytes_per_row + (W / 2) * 4) as usize];
    drop(data);
    readback.unmap();
    center
}

#[test]
fn half_alpha_fills_composite_at_their_authored_alpha() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let half_white = Color::srgb_u8(255, 255, 255).with_alpha_u8(128);
    for sample_count in [1u32, 4] {
        // White over white is white at any alpha under straight
        // blending; alpha² gave 225.
        let over_white = render_fill_center(
            &device,
            &queue,
            half_white,
            wgpu::Color::WHITE,
            sample_count,
        );
        assert!(
            over_white >= 253,
            "msaa {sample_count}: 50% white over white read {over_white}, expected 255 (alpha applied twice?)"
        );
        // White at alpha 128/255 over black is 0.502 linear, which
        // encodes to 188 in sRGB; alpha² gave 137.
        let over_black = render_fill_center(
            &device,
            &queue,
            half_white,
            wgpu::Color::BLACK,
            sample_count,
        );
        assert!(
            (184..=192).contains(&over_black),
            "msaa {sample_count}: 50% white over black read {over_black}, expected ~188"
        );
    }
}
