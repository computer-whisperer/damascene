//! `damascene_wgpu::headless::Headless` regression tests: the
//! single-sample path and fractional scale factors both used to panic
//! (a 1-sample resolve source; a target one pixel short of the
//! runner's rounded-up viewport). Skips cleanly when no adapter is
//! available.

use damascene_core::prelude::*;
use damascene_wgpu::headless::Headless;

fn tree() -> El {
    column([text("headless").key("t")]).padding(Sides::all(8.0))
}

#[test]
fn renders_at_sample_count_one_and_fractional_scale() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("no adapter; skipping");
        return;
    };
    let theme = Theme::default();
    let clear = theme.palette().background;
    for (sample_count, scale, w, h) in [
        (1u32, 1.0f32, 641.0f32, 300.0f32),
        (4, 1.5, 641.0, 300.0),
        (1, 1.5, 641.0, 300.0),
    ] {
        let (width, height, pixels) = gpu
            .render_rgba8(
                tree(),
                theme.clone(),
                Rect::new(0.0, 0.0, w, h),
                scale,
                sample_count,
                clear,
            )
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
    let theme = Theme::default();
    let clear = theme.palette().background;
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
    let (width, height, pixels) = gpu
        .render_rgba8(
            tree,
            theme.clone(),
            Rect::new(0.0, 0.0, 160.0, 80.0),
            1.0,
            4,
            clear,
        )
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
