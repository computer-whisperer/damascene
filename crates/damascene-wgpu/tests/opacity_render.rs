//! Readback test: a node's opacity reaches the paint no op colour
//! carries — a painted vector's own colours and gradients, an
//! authored-colour SVG icon, an app texture in each alpha mode, a 3D
//! scene, a colour emoji, and the stock shaders that paint from their
//! own uniforms (spinner, skeleton, indeterminate progress).
//!
//! Each case renders the same tile three ways over a light backdrop:
//! at full opacity, at half, and not at all. Summed over the frame,
//! the half frame must have moved half the way from the bare backdrop
//! to the full frame, in linear light. A backend that ignores the
//! opacity gives a ratio of 1; one that applies it twice gives a
//! quarter; one that scales the tile without blending it over what is
//! underneath falls short of a half by the backdrop's share.
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

/// One frame in linear light, RGB per pixel: `tile` over a light
/// backdrop, or the backdrop alone.
fn frame(gpu: &Headless, tile: Option<El>) -> Vec<f32> {
    let mut runner = gpu.runner(1);
    let backdrop = El::new(Kind::Group)
        .fill(Color::srgb_u8(200, 215, 255))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));
    let tree = stack(std::iter::once(backdrop).chain(tile))
        .align(Align::Center)
        .justify(Justify::Center)
        .width(Size::Fixed(SIZE))
        .height(Size::Fixed(SIZE));
    let (_, _, px) = gpu
        .render_rgba8(&mut runner, tree, Rect::new(0.0, 0.0, SIZE, SIZE), 1.0)
        .expect("headless render");
    px.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .map(srgb_to_linear)
        .collect()
}

/// The three frames of one case: full opacity, half, and backdrop.
fn frames(gpu: &Headless, tile: impl Fn(f32) -> El) -> [Vec<f32>; 3] {
    [
        frame(gpu, Some(tile(1.0))),
        frame(gpu, Some(tile(0.5))),
        frame(gpu, None),
    ]
}

/// How far the half-opacity frame has moved from the backdrop towards
/// the full-opacity frame: 0.5 for a tile that fades once.
fn assert_fades_halfway(what: &str, [full, half, none]: &[Vec<f32>; 3]) {
    let (mut moved, mut span) = (0.0f64, 0.0f64);
    for i in 0..full.len() {
        let d = (full[i] - none[i]) as f64;
        moved += (half[i] - none[i]) as f64 * d.signum();
        span += d.abs();
    }
    assert!(
        span / full.len() as f64 > 0.002,
        "{what}: the tile must show against the backdrop to measure a fade (span {span:.2})"
    );
    let ratio = moved / span;
    assert!(
        (ratio - 0.5).abs() < 0.04,
        "{what}: at half opacity the frame moved {ratio:.3} of the way to the full frame, \
         want 0.5"
    );
}

/// Centre pixel of a frame.
fn centre(frame: &[f32]) -> [f32; 3] {
    let side = SIZE as usize;
    let at = (side / 2 * side + side / 2) * 3;
    [frame[at], frame[at + 1], frame[at + 2]]
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
        assert_fades_halfway(&format!("painted vector, {what}"), &shots);
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
    assert_fades_halfway("authored-colour icon", &shots);
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
    assert_fades_halfway("one-colour icon", &shots);
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
            let full = centre(&shots[0]);
            assert!(
                full[0] < 0.01 && full[1] > 0.99 && full[2] < 0.01,
                "an opaque surface at full opacity replaces what is under it, got {full:?}"
            );
        }
        assert_fades_halfway(&format!("app texture, {what}"), &shots);
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
    assert_fades_halfway("3D scene", &shots);
}

/// The stock shaders that paint from uniforms of their own. Their
/// colours reach the shader unfaded; the shader applies the opacity.
#[test]
fn spinner_skeleton_and_indeterminate_progress_fade_with_their_node() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    let ink = Color::srgb_u8(20, 40, 160);
    type Build = fn(Color) -> El;
    let cases: [(&str, Build); 3] = [
        ("spinner", |c| spinner_with_color(c)),
        ("skeleton", |c| skeleton::skeleton_with_color(c)),
        ("indeterminate progress", |c| {
            progress_indeterminate_with_color(c)
        }),
    ];
    for (what, build) in cases {
        let shots = frames(&gpu, |opacity| {
            build(ink)
                .width(Size::Fixed(48.0))
                .height(Size::Fixed(48.0))
                .opacity(opacity)
        });
        assert_fades_halfway(what, &shots);
    }
}

/// A colour glyph keeps its own RGB and takes the alpha of the text
/// colour around it.
#[test]
fn a_colour_emoji_fades_with_its_text() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("opacity_render: no GPU adapter, skipping");
        return;
    };
    let shots = frames(&gpu, |opacity| {
        text("\u{1F600}").font_size(40.0).opacity(opacity)
    });
    assert_fades_halfway("colour emoji", &shots);
}
