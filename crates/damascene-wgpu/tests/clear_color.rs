//! Readback regression test for host clear colors (issue #45).
//!
//! Hosts clear the surface with the theme background token routed through
//! `damascene_core::paint::rgba_f32_in` — the same conversion the paint
//! stream applies to fills — so the cleared backdrop is byte-identical to
//! a painted `tokens::BACKGROUND` fill. The 0.3.x → 0.4.0 `Color` rework
//! (u8 0–255 → f32 0–1) silently broke every hand-rolled
//! `srgb_to_linear(c.r / 255.0)` helper (`f32 / 255.0` type-checks);
//! this test locks the invariant down at the GPU boundary: clear an sRGB
//! target with the converted token, read the pixel back, and require the
//! token's own 8-bit encoding.
//!
//! Skips cleanly (passes) when no adapter is available, so CI without a
//! GPU doesn't fail — but it runs for real wherever a Vulkan/Metal/DX
//! adapter exists.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const SIZE: u32 = 16;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("clear_color_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
fn background_clear_roundtrips_to_token_bytes() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("clear_color: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    // Empty tree: nothing paints, so every pixel is the host clear.
    let mut tree = stack(Vec::<El>::new());
    runner.prepare(
        &device,
        &queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );

    // The exact host-side clear computation: token → working space.
    let token = damascene_core::tokens::BACKGROUND;
    let [r, g, b, a] = damascene_core::paint::rgba_f32_in(token, runner.working_color_space());
    let clear = wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    };

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("clear_color_test_target"),
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
        label: Some("clear_color_test_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("clear_color_test"),
    });
    runner.render(
        &device,
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
    let pixel = [data[0], data[1], data[2], data[3]];
    drop(data);
    readback.unmap();

    // The sRGB store must round-trip to the token's own 8-bit encoding —
    // ±1 for GPU transfer-function rounding. The 0.3.x-era bug crushed
    // (9, 9, 11, 255) to (0, 0, 0, ~1), far outside this band.
    let expected = token.to_srgb_u8a();
    let close = pixel
        .iter()
        .zip(&expected)
        .all(|(p, e)| (*p as i32 - *e as i32).abs() <= 1);
    assert!(
        close,
        "cleared pixel {pixel:?} must match background token bytes {expected:?}"
    );
}
