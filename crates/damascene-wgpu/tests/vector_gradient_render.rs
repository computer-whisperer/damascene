//! Headless end-to-end render test for painted vector gradients
//! (issues #140/#141).
//!
//! Renders fullscreen gradient-filled rects through the real tess
//! pipeline (fragment-stage ramp evaluation) and pixel-checks against
//! the reference values from the issue reports:
//!
//! - #141: a two-stop `#754A75 → #F7A983` gradient must interpolate in
//!   sRGB space — midpoint `rgb(182, 122, 124)`, not the linear-space
//!   `rgb(196, 132, 124)`.
//! - #140: interior stops of a five-stop gradient must actually render
//!   (per-vertex sampling only ever hit the endpoint stops).
//!
//! Skips cleanly (passes) when no adapter is available, so CI without a
//! GPU doesn't fail.

use damascene_core::prelude::*;
use damascene_core::tree::vector;
use damascene_core::vector::parse_svg_asset;
use damascene_wgpu::Runner;

const SIZE: u32 = 200;
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
        label: Some("vector_gradient_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

fn render_to_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    runner: &mut Runner,
    tree: El,
) -> Vec<u8> {
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vector_gradient_target"),
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
    let unpadded = SIZE * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vector_gradient_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("vector_gradient_test"),
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
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for row in 0..SIZE as usize {
        let start = row * bytes_per_row as usize;
        out.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    readback.unmap();
    out
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * SIZE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn assert_close(got: [u8; 3], want: [u8; 3], tolerance: i16, context: &str) {
    for (g, w) in got.iter().zip(&want) {
        assert!(
            (*g as i16 - *w as i16).abs() <= tolerance,
            "{context}: got {got:?}, want {want:?} (±{tolerance})"
        );
    }
}

/// Fullscreen vertical gradient rect: viewBox y spans the gradient
/// axis, so a pixel row's `t` is `(row + 0.5) / SIZE`.
fn gradient_tree(stops: &str) -> El {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 200">
            <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="200" gradientUnits="userSpaceOnUse">
                {stops}
            </linearGradient></defs>
            <rect width="100" height="200" fill="url(#g)"/></svg>"##
    );
    vector(parse_svg_asset(&svg).unwrap())
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32))
}

/// Issue #141's verification table: sRGB-space interpolation of
/// `#754A75 → #F7A983` at 1/4, 1/2, 3/4 — the values every browser and
/// the reference Vulkan renderer produce. Linear-space interpolation
/// (the old behavior) is ~14 counts brighter through the midrange and
/// fails the tolerance.
#[test]
fn two_stop_gradient_interpolates_in_srgb() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping vector_gradient_render test");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#754A75"/>
            <stop offset="1" stop-color="#F7A983"/>"##,
    );
    let pixels = render_to_pixels(&device, &queue, &mut runner, tree);

    // Endpoints (sampled a few rows in to stay clear of AA fringes).
    assert_close(pixel(&pixels, 100, 2), [0x75, 0x4A, 0x75], 4, "t≈0");
    assert_close(pixel(&pixels, 100, SIZE - 3), [0xF7, 0xA9, 0x83], 4, "t≈1");
    // The #141 table. ±4 covers f16 ramp texels, 8-bit target
    // quantization, and the half-row t offset.
    assert_close(pixel(&pixels, 100, SIZE / 4), [150, 98, 121], 4, "t=0.25");
    assert_close(pixel(&pixels, 100, SIZE / 2), [182, 122, 124], 4, "t=0.5");
    assert_close(
        pixel(&pixels, 100, SIZE * 3 / 4),
        [214, 145, 128],
        4,
        "t=0.75",
    );
}

/// Issue #140: interior stops must render. Per-vertex sampling only
/// ever evaluated the gradient at the rect's corners, so the three
/// interior stops vanished into an endpoint-to-endpoint lerp.
#[test]
fn five_stop_gradient_renders_interior_stops() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping vector_gradient_render test");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#754A75"/>
            <stop offset="0.25" stop-color="#372960"/>
            <stop offset="0.5" stop-color="#A33861"/>
            <stop offset="0.75" stop-color="#D1956C"/>
            <stop offset="1" stop-color="#F7A983"/>"##,
    );
    let pixels = render_to_pixels(&device, &queue, &mut runner, tree);

    // Each interior stop's authored colour appears at its offset. The
    // old endpoint-lerp behavior puts e.g. lerp(#754A75, #F7A983, 0.25)
    // ≈ (150, 98, 121) at t=0.25 — nowhere near #372960.
    assert_close(
        pixel(&pixels, 100, SIZE / 4),
        [0x37, 0x29, 0x60],
        5,
        "t=0.25",
    );
    assert_close(
        pixel(&pixels, 100, SIZE / 2),
        [0xA3, 0x38, 0x61],
        5,
        "t=0.5",
    );
    assert_close(
        pixel(&pixels, 100, SIZE * 3 / 4),
        [0xD1, 0x95, 0x6C],
        5,
        "t=0.75",
    );
}

/// Translucent gradient stops must composite once, not twice: the
/// vector shaders output premultiplied colour, so the pipeline blend
/// must use One (premultiplied), not SrcAlpha — SrcAlpha yields
/// `a²·rgb` and renders `stop-opacity` paint visibly darker than any
/// browser (adversarial-review finding F1 on the #140/#141 work).
#[test]
fn translucent_stops_blend_premultiplied() {
    use damascene_core::color::ColorSpace;

    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping vector_gradient_render test");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    // Constant-colour gradient at 50% stop opacity over the black
    // clear: one correct composite is 0.5 x the colour in linear space.
    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#F7A983" stop-opacity="0.5"/>
            <stop offset="1" stop-color="#F7A983" stop-opacity="0.5"/>"##,
    );
    let pixels = render_to_pixels(&device, &queue, &mut runner, tree);

    let lin = Color::in_space(
        ColorSpace::SRGB,
        0xF7 as f32 / 255.0,
        0xA9 as f32 / 255.0,
        0x83 as f32 / 255.0,
        1.0,
    )
    .convert_to(ColorSpace::SRGB_LINEAR);
    let [r, g, b, _] = Color::srgb_linear(lin.r * 0.5, lin.g * 0.5, lin.b * 0.5, 1.0).to_srgb_u8a();
    // The double-multiplied (SrcAlpha) rendering lands at 0.25 x the
    // colour instead — tens of counts darker, far outside +-4.
    assert_close(pixel(&pixels, 100, SIZE / 2), [r, g, b], 4, "50% opacity");
}
