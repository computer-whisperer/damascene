//! Readback test: a node's opacity reaches the paint no op colour
//! carries — a painted vector's own colours and gradients, an
//! authored-colour SVG icon, an app texture in each alpha mode, and a
//! 3D scene.
//!
//! Each case renders the same tile three ways over the theme
//! background: at full opacity, at half, and not at all. The half
//! frame must sit midway between the other two in linear light. A
//! backend that ignores the opacity leaves the half frame equal to the
//! full one; one that applies it twice leaves it at a quarter.
//!
//! Skips cleanly (passes) when no adapter is available.

use damascene_core::prelude::*;
use damascene_core::scene::{GridPlanes, GridSettings, SceneSpec, SceneStyle};
use damascene_core::vector::parse_svg_asset;
use damascene_wgpu::StreamingTexture;
use damascene_wgpu::headless::Headless;

const SIZE: f32 = 64.0;

fn srgb_to_linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Centre pixel of one frame, in linear light: `tile` filling the
/// frame, or the bare theme background.
fn centre(gpu: &Headless, tile: Option<El>) -> [f32; 3] {
    let mut runner = gpu.runner(1);
    let tree = stack(tile)
        .width(Size::Fixed(SIZE))
        .height(Size::Fixed(SIZE));
    let (w, h, px) = gpu
        .render_rgba8(&mut runner, tree, Rect::new(0.0, 0.0, SIZE, SIZE), 1.0)
        .expect("headless render");
    let at = ((h / 2 * w + w / 2) * 4) as usize;
    [
        srgb_to_linear(px[at]),
        srgb_to_linear(px[at + 1]),
        srgb_to_linear(px[at + 2]),
    ]
}

/// The three frames of one case: full opacity, half, and background.
fn frames(gpu: &Headless, tile: impl Fn(f32) -> El) -> ([f32; 3], [f32; 3], [f32; 3]) {
    (
        centre(gpu, Some(tile(1.0))),
        centre(gpu, Some(tile(0.5))),
        centre(gpu, None),
    )
}

fn assert_fades_halfway(what: &str, (full, half, none): ([f32; 3], [f32; 3], [f32; 3])) {
    let contrast: f32 = (0..3).map(|i| (full[i] - none[i]).abs()).sum();
    assert!(
        contrast > 0.3,
        "{what}: the tile must stand out from the background to measure a fade \
         (full {full:?}, background {none:?})"
    );
    for i in 0..3 {
        let want = (full[i] + none[i]) * 0.5;
        assert!(
            (half[i] - want).abs() < 0.02,
            "{what}: channel {i} at half opacity is {:.3}, want {want:.3} \
             (full {:.3}, background {:.3})",
            half[i],
            full[i],
            none[i],
        );
    }
}

fn solid_texture(gpu: &Headless, rgba: [u8; 4]) -> StreamingTexture {
    let side = SIZE as u32;
    let mut tex = StreamingTexture::new(
        gpu.device(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
        side,
        side,
    );
    let data: Vec<u8> = rgba.repeat((side * side) as usize);
    tex.write_frame(gpu.device(), gpu.queue(), side, side, &data);
    tex
}

#[test]
fn a_painted_vector_fades_with_its_node() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    // One solid fill and one gradient fill: the two places a painted
    // asset keeps a colour (vertex colour, gradient table).
    let solid = parse_svg_asset(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <rect width="10" height="10" fill="#ff4020"/></svg>"##,
    )
    .expect("solid svg");
    let gradient = parse_svg_asset(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0" stop-color="#20c0ff"/><stop offset="1" stop-color="#20ffc0"/>
            </linearGradient></defs>
            <rect width="10" height="10" fill="url(#g)"/></svg>"##,
    )
    .expect("gradient svg");
    for (what, asset) in [("solid fill", solid), ("gradient fill", gradient)] {
        let shots = frames(&gpu, |opacity| {
            vector(asset.clone())
                .width(Size::Fixed(SIZE))
                .height(Size::Fixed(SIZE))
                .opacity(opacity)
        });
        assert_fades_halfway(&format!("painted vector, {what}"), shots);
    }
}

#[test]
fn an_authored_colour_icon_fades_with_its_node() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    let logo = SvgIcon::parse(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <rect width="24" height="24" fill="#ffb000"/></svg>"##,
    )
    .expect("icon svg");
    let shots = frames(&gpu, |opacity| {
        icon(logo.clone()).icon_size(SIZE).opacity(opacity)
    });
    assert_fades_halfway("authored-colour icon", shots);
}

/// A one-colour icon's colour already carries the opacity. The op's
/// own opacity must not be applied on top of it.
#[test]
fn a_one_colour_icon_fades_once() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    let mask = SvgIcon::parse_current_color(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
            <rect width="24" height="24" fill="#000"/></svg>"##,
    )
    .expect("icon svg");
    let shots = frames(&gpu, |opacity| {
        icon(mask.clone())
            .icon_size(SIZE)
            .text_color(Color::srgb_u8(255, 176, 0))
            .opacity(opacity)
    });
    assert_fades_halfway("one-colour icon", shots);
}

#[test]
fn an_app_texture_fades_with_its_node_in_every_alpha_mode() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    let cases = [
        // Premultiplied and straight both describe 80%-opaque orange.
        (
            "premultiplied",
            SurfaceAlpha::Premultiplied,
            [204u8, 102, 0, 204],
        ),
        ("straight", SurfaceAlpha::Straight, [255, 128, 0, 204]),
        // An opaque surface ignores the texture's alpha, here zero.
        ("opaque", SurfaceAlpha::Opaque, [0, 255, 0, 0]),
    ];
    for (what, alpha, rgba) in cases {
        let tex = solid_texture(&gpu, rgba);
        let shots = frames(&gpu, |opacity| {
            surface(tex.app_texture())
                .surface_alpha(alpha)
                .opacity(opacity)
        });
        if matches!(alpha, SurfaceAlpha::Opaque) {
            let full = shots.0;
            assert!(
                full[0] < 0.01 && full[1] > 0.99 && full[2] < 0.01,
                "an opaque surface at full opacity replaces what is under it, got {full:?}"
            );
        }
        assert_fades_halfway(&format!("app texture, {what}"), shots);
    }
}

#[test]
fn a_scene_fades_with_its_node() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::NONE,
            ..Default::default()
        },
        background: Some(Color::srgb_u8(230, 70, 30)),
        msaa_samples: 4,
        show_axes: false,
    };
    let shots = frames(&gpu, |opacity| {
        chart3d(SceneSpec::new().style(style)).opacity(opacity)
    });
    assert_fades_halfway("3D scene", shots);
}
