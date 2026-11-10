//! `damascene_wgpu::headless::Headless` regression tests: the
//! single-sample path and fractional scale factors both used to panic
//! (a 1-sample resolve source; a target one pixel short of the
//! runner's rounded-up viewport), and the runner used to be private to
//! the render call, so a custom shader could not be registered on it
//! (issue #154). Skips cleanly when no adapter is available.

use damascene_core::prelude::*;
use damascene_wgpu::headless::Headless;

const GRADIENT_WGSL: &str = include_str!("../../damascene-core/shaders/gradient.wgsl");

fn tree() -> El {
    column([text("headless").key("t")]).padding(Sides::all(8.0))
}

#[test]
fn renders_at_sample_count_one_and_fractional_scale() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    for (sample_count, scale, w, h) in [
        (1u32, 1.0f32, 641.0f32, 300.0f32),
        (4, 1.5, 641.0, 300.0),
        (1, 1.5, 641.0, 300.0),
    ] {
        let mut runner = gpu.runner(sample_count);
        let (width, height, pixels) = gpu
            .render_rgba8(&mut runner, tree(), Rect::new(0.0, 0.0, w, h), scale)
            .unwrap_or_else(|e| panic!("msaa {sample_count} scale {scale}: {e}"));
        assert_eq!(
            width,
            (w * scale).ceil() as u32,
            "msaa {sample_count} scale {scale}: width"
        );
        assert_eq!(
            height,
            (h * scale).ceil() as u32,
            "msaa {sample_count} scale {scale}: height"
        );
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        assert!(
            pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 255),
            "msaa {sample_count} scale {scale}: the frame is opaque"
        );
    }
}

/// `icon("star").filled()` paints the star's interior: the filled
/// glyph lights many more pixels of its box than the outline, in the
/// same colour. Also writes the pair to `out/filled_star.png` for the
/// eye. Skips cleanly when no adapter is available.
#[test]
fn a_filled_icon_lights_its_interior() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let size = 48.0;
    let tree = row([
        icon("star")
            .key("outline")
            .icon_size(size)
            .text_color(tokens::WARNING),
        icon("star")
            .filled()
            .key("filled")
            .icon_size(size)
            .text_color(tokens::WARNING),
    ])
    .gap(16.0)
    .padding(Sides::all(16.0));
    let mut runner = gpu.runner(4);
    let (width, height, pixels) = gpu
        .render_rgba8(&mut runner, tree, Rect::new(0.0, 0.0, 160.0, 80.0), 1.0)
        .expect("render");
    let bg = {
        let i = 0;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let lit = |x0: u32, x1: u32| {
        let mut n = 0;
        for y in 0..height {
            for x in x0..x1 {
                let i = ((y * width + x) * 4) as usize;
                let px = [pixels[i], pixels[i + 1], pixels[i + 2]];
                if px.iter().zip(bg).any(|(a, b)| a.abs_diff(b) > 24) {
                    n += 1;
                }
            }
        }
        n
    };
    // Layout: 16px padding, then 48px outline, 16px gap, 48px filled.
    let outline = lit(16, 64);
    let filled = lit(80, 128);
    if let Ok(dir) = std::env::var("DAMASCENE_TEST_OUT") {
        let path = std::path::PathBuf::from(dir).join("filled_star.png");
        let file = std::fs::File::create(&path).expect("create png");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .and_then(|mut w| w.write_image_data(&pixels))
            .expect("write png");
    }
    assert!(outline > 0, "the outline star paints");
    assert!(
        filled > outline * 3 / 2,
        "filled {filled} lit pixels vs outline {outline}"
    );
}

/// Issue #154: the caller owns the runner, so it can register the
/// custom shaders its tree binds. A swatch bound to `gradient` paints
/// its colour once the shader is registered, and nothing — the draw is
/// dropped — on a runner that was never told about it.
#[test]
fn a_custom_shader_paints_once_registered_on_the_runner() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let red = Color::srgb_u8(255, 0, 0);
    let tree = || {
        column([El::new(Kind::Custom("swatch"))
            .key("swatch")
            .width(Size::Fixed(64.0))
            .height(Size::Fixed(64.0))
            .shader(
                ShaderBinding::custom("gradient")
                    .color("vec_a", red)
                    .color("vec_b", red)
                    .f32("vec_c", 0.0),
            )])
        .padding(Sides::all(16.0))
    };
    let viewport = Rect::new(0.0, 0.0, 96.0, 96.0);
    // Centre of the swatch: 16px padding + half of 64px.
    let centre = |pixels: &[u8], width: u32| {
        let i = ((48 * width + 48) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };

    let mut bare = gpu.runner(4);
    let (width, _, pixels) = gpu
        .render_rgba8(&mut bare, tree(), viewport, 1.0)
        .expect("render without the shader");
    let background = [pixels[0], pixels[1], pixels[2]];
    assert_eq!(
        centre(&pixels, width),
        background,
        "an unregistered shader drops the draw"
    );

    let mut registered = gpu.runner(4);
    registered.register_shader(gpu.device(), "gradient", GRADIENT_WGSL);
    let (width, _, pixels) = gpu
        .render_rgba8(&mut registered, tree(), viewport, 1.0)
        .expect("render with the shader");
    assert_eq!(
        centre(&pixels, width),
        [255, 0, 0],
        "the registered shader paints the swatch"
    );
}

/// The runner outlives the call: after one frame it answers layout
/// questions from that frame, and it renders again.
#[test]
fn the_runner_keeps_its_frame_between_renders() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let viewport = Rect::new(0.0, 0.0, 200.0, 80.0);
    let mut runner = gpu.runner(1);
    assert!(runner.rect_of_key("t").is_none(), "nothing laid out yet");
    let (_, _, first) = gpu
        .render_rgba8(&mut runner, tree(), viewport, 1.0)
        .expect("first render");
    assert!(
        runner.rect_of_key("t").is_some(),
        "the first frame's layout is readable afterwards"
    );
    let (_, _, second) = gpu
        .render_rgba8(&mut runner, tree(), viewport, 1.0)
        .expect("second render");
    assert_eq!(
        first, second,
        "the same tree renders the same settled frame"
    );
}

/// A runner built for some other target is refused rather than
/// handed to wgpu to fail validation.
#[test]
fn a_foreign_runner_is_refused() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let mut foreign = damascene_wgpu::Runner::new(
        gpu.device(),
        gpu.queue(),
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let err = gpu
        .render_rgba8(&mut foreign, tree(), Rect::new(0.0, 0.0, 64.0, 64.0), 1.0)
        .expect_err("a Bgra8 runner cannot draw the Rgba8 headless target");
    assert!(err.to_string().contains("Headless::runner"), "{err}");
}
