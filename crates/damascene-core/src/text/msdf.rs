//! MSDF generation for outline glyphs.
//!
//! Builds a multi-channel signed distance field for one glyph at a fixed
//! base em size. The output is sampled at arbitrary logical sizes by the
//! text shader, so each `(font, glyph)` pair is rasterized once and reused
//! across every UI size and display scale.
//!
//! Built on `fdsm` (pure-Rust MSDF generator) + `fdsm-ttf-parser` for
//! outline loading. Generation runs once per glyph at atlas-build time;
//! per-frame cost is just an atlas lookup.
//!
//! ## Output coordinate system
//!
//! All metrics on [`MsdfGlyph`] are in **base-em pixels**, the same
//! coordinate system the bitmap is stored in. To place a quad at logical
//! size `s`, multiply every metric by `s / base_em`.
//!
//! The bitmap origin is **y-down**: row 0 is at the top, increasing y
//! goes down. `bearing_y` is the offset from the line baseline (y-down)
//! to the bitmap's top edge — typically negative because the glyph
//! extends *above* the baseline.

use fdsm::{
    bezier::scanline::FillRule,
    correct_error::{ErrorCorrectionConfig, correct_error_mtsdf},
    generate::generate_mtsdf,
    render::correct_sign_mtsdf,
    shape::Shape,
    transform::Transform,
};
use fdsm_ttf_parser::load_shape_from_face;
use nalgebra::{Affine2, Matrix3};
use ttf_parser::{Face, GlyphId, Tag};

/// Apply a `wght` variation to a face before outline extraction, and
/// return the weight value that should key the resulting raster.
///
/// Returns `weight` when the face is variable and has a `wght` axis
/// (the variation is then active on `face` — `outline_glyph`,
/// `glyph_bounding_box`, and `glyph_hor_advance` all honour it), or
/// `0` when the face is static or lacks the axis (nothing applied; the
/// face's default instance is the only design available). `0` is the
/// canonical "default instance" bucket in
/// [`crate::text::msdf_atlas::MsdfGlyphKey::weight`], so every
/// requested weight of a static face shares one cached raster.
pub fn apply_weight_variation(face: &mut Face<'_>, weight: u16) -> u16 {
    if weight != 0
        && face
            .set_variation(Tag::from_bytes(b"wght"), f32::from(weight))
            .is_some()
    {
        weight
    } else {
        0
    }
}

/// One rasterized glyph in MTSDF form, plus the metrics needed to place
/// its quad. All metrics are in **base-em pixels**.
///
/// MTSDF = MSDF + true single-channel SDF in the alpha channel. The
/// shader uses the alpha channel to detect and reject MSDF artifacts at
/// sharp corners (median(R,G,B) can flip to "outside" inside the glyph
/// when the per-channel coloring disagrees; the alpha channel never
/// has this problem).
#[derive(Clone, Debug, PartialEq)]
pub struct MsdfGlyph {
    /// RGBA MTSDF bitmap, packed `[r,g,b,a, …]`, length `w*h*4`. RGB
    /// channels carry the MSDF, A carries the true single-channel SDF.
    pub rgba: Vec<u8>,
    /// Bitmap width in atlas pixels.
    pub width: u32,
    /// Bitmap height in atlas pixels.
    pub height: u32,
    /// Pen-relative X offset of the bitmap's top-left, in base-em pixels.
    /// Includes the SDF spread margin.
    pub bearing_x: f32,
    /// Baseline-relative Y offset of the bitmap's top edge, in base-em
    /// pixels (y-down). Typically negative — the glyph rises above the
    /// baseline. Includes the SDF spread margin.
    pub bearing_y: f32,
    /// Horizontal advance width in base-em pixels.
    pub advance: f32,
    /// MSDF spread radius in base-em pixels — the same value the shader
    /// uses to map the encoded distance back to signed distance.
    pub spread: f32,
}

/// Generate an MSDF for one glyph.
///
/// `base_em` is the target em size in atlas pixels. Recommended values
/// are 32–48; larger values trade atlas memory for fidelity at huge
/// rendered sizes. `spread` is the MSDF radius in atlas pixels —
/// typical 4. Returns `None` for glyphs with no bounding box (whitespace,
/// notdef without outlines) or when fdsm fails to load the outline.
///
/// The advance width is still reported in the `None` case via
/// [`glyph_advance`] so callers can lay out spaces correctly.
///
/// `correct_error` runs fdsm's MTSDF error-correction pass. It is
/// ~22% of generation cost and, for the bundled sans/mono faces, makes
/// no perceptible difference once the shader's alpha-channel SDF
/// fallback (`text_msdf.wgsl`) handles sign-disagreement artifacts — so
/// the atlas leaves it off by default and only unusual faces benefit.
pub fn build_glyph_msdf(
    face: &Face<'_>,
    glyph_id: u16,
    base_em: u32,
    spread: f64,
    correct_error: bool,
) -> Option<MsdfGlyph> {
    let gid = GlyphId(glyph_id);
    let bbox = face.glyph_bounding_box(gid)?;
    let mut shape = load_shape_from_face(face, gid)?;

    let upem = face.units_per_em() as f64;
    let scale = base_em as f64 / upem;

    let bb_w = (bbox.x_max - bbox.x_min) as f64 * scale;
    let bb_h = (bbox.y_max - bbox.y_min) as f64 * scale;
    let width = (bb_w + 2.0 * spread).ceil() as u32;
    let height = (bb_h + 2.0 * spread).ceil() as u32;
    if width == 0 || height == 0 {
        return None;
    }

    // Place the glyph's bbox at (spread, spread) in y-down image space:
    // x' = scale * x + (spread - x_min*scale)
    // y' = -scale * y + (height - spread + y_min*scale)
    let tx = spread - bbox.x_min as f64 * scale;
    let ty = height as f64 - spread + bbox.y_min as f64 * scale;
    let m = Matrix3::new(scale, 0.0, tx, 0.0, -scale, ty, 0.0, 0.0, 1.0);
    let transform = Affine2::from_matrix_unchecked(m);
    shape.transform(&transform);

    let colored = Shape::edge_coloring_simple(shape, 0.03, 0);
    let prepared = colored.prepare();
    // Generate MTSDF at f32 precision: RGB carries the standard MSDF,
    // alpha carries a true single-channel SDF. The shader picks the
    // true SDF wherever median(RGB) disagrees with it, eliminating the
    // false-outside artifacts that appear near sharp corners (e.g. the
    // join between a glyph's stem and crossbar). Generation is followed
    // by an optional error-correction pass, then sign correction (which
    // must run last per fdsm's API contract).
    let mut buf_f = image::Rgba32FImage::new(width, height);
    generate_mtsdf(&prepared, spread, &mut buf_f);
    if correct_error {
        correct_error_mtsdf(
            &mut buf_f,
            &colored,
            &prepared,
            spread,
            &ErrorCorrectionConfig::default(),
        );
    }
    correct_sign_mtsdf(&mut buf_f, &prepared, FillRule::Nonzero);
    let buf = rgba32f_to_rgba8(&buf_f);

    let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale as f32;
    let bearing_x = bbox.x_min as f32 * scale as f32 - spread as f32;
    // y-down: top of bitmap (y=0 in image) is `spread` pixels above the
    // glyph's highest point. The glyph's highest point is `bbox.y_max *
    // scale` above the baseline. In y-down: -bbox.y_max * scale - spread.
    let bearing_y = -(bbox.y_max as f32 * scale as f32) - spread as f32;

    Some(MsdfGlyph {
        rgba: buf.into_raw(),
        width,
        height,
        bearing_x,
        bearing_y,
        advance,
        spread: spread as f32,
    })
}

fn rgba32f_to_rgba8(src: &image::Rgba32FImage) -> image::RgbaImage {
    let mut dst = image::RgbaImage::new(src.width(), src.height());
    for (x, y, p) in src.enumerate_pixels() {
        let r = (p[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (p[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (p[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        let a = (p[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        dst.put_pixel(x, y, image::Rgba([r, g, b, a]));
    }
    dst
}

/// Advance width for a glyph in base-em pixels. Useful for whitespace
/// glyphs, where [`build_glyph_msdf`] returns `None` because there is no
/// bounding box.
pub fn glyph_advance(face: &Face<'_>, glyph_id: u16, base_em: u32) -> f32 {
    let gid = GlyphId(glyph_id);
    let upem = face.units_per_em() as f64;
    let scale = base_em as f64 / upem;
    face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale as f32
}

// The fixture face is Inter, so these tests only build when the `inter`
// font feature is on (it is in the default set).
#[cfg(all(test, feature = "inter"))]
mod tests {
    use super::*;

    fn test_face() -> ttf_parser::Face<'static> {
        ttf_parser::Face::parse(damascene_fonts::INTER_VARIABLE, 0).unwrap()
    }

    #[test]
    fn produces_msdf_for_letter_a() {
        let face = test_face();
        let glyph_id = face.glyph_index('A').unwrap().0;
        let glyph = build_glyph_msdf(&face, glyph_id, 32, 4.0, false).expect("MSDF for A");

        assert_eq!(glyph.spread, 4.0);
        assert_eq!(glyph.rgba.len() as u32, glyph.width * glyph.height * 4);
        // 32px em with 4px spread on each side: bitmap height covers
        // cap_height + spread*2 in the same ~28 px range across our
        // bundled Latin sans faces.
        assert!(glyph.height >= 24 && glyph.height <= 36, "{}", glyph.height);
        assert!(glyph.width >= 16 && glyph.width <= 32, "{}", glyph.width);
        // 'A' has no horizontal bearing offset to speak of.
        assert!(glyph.bearing_x.abs() < 6.0, "{}", glyph.bearing_x);
        // Baseline-relative top: well above baseline (negative in y-down)
        // and roughly equal to the cap-height portion plus spread.
        assert!(glyph.bearing_y < 0.0, "{}", glyph.bearing_y);
        // Advance is positive and reasonable.
        assert!(
            glyph.advance > 10.0 && glyph.advance < 30.0,
            "{}",
            glyph.advance
        );
    }

    #[test]
    fn whitespace_returns_none_but_keeps_advance() {
        let face = test_face();
        let glyph_id = face.glyph_index(' ').unwrap().0;
        assert!(build_glyph_msdf(&face, glyph_id, 32, 4.0, false).is_none());
        let advance = glyph_advance(&face, glyph_id, 32);
        assert!(advance > 0.0);
    }

    #[test]
    fn bitmap_has_inside_pixels() {
        // Sanity: the median-of-RGB inside the glyph should be > 128
        // somewhere (positive distance = inside the glyph).
        let face = test_face();
        let glyph_id = face.glyph_index('O').unwrap().0;
        let glyph = build_glyph_msdf(&face, glyph_id, 32, 4.0, false).unwrap();
        let mut found_inside = false;
        for px in glyph.rgba.chunks_exact(4) {
            let mut v = [px[0], px[1], px[2]];
            v.sort_unstable();
            if v[1] > 200 {
                found_inside = true;
                break;
            }
        }
        assert!(found_inside, "expected interior pixels in 'O'");
    }

    #[test]
    fn bitmap_has_outside_pixels() {
        // The corners of the bitmap should be far outside (median ≈ 0).
        let face = test_face();
        let glyph_id = face.glyph_index('A').unwrap().0;
        let glyph = build_glyph_msdf(&face, glyph_id, 32, 4.0, false).unwrap();
        let stride = glyph.width as usize * 4;
        let corner = &glyph.rgba[0..3];
        let mut v = [corner[0], corner[1], corner[2]];
        v.sort_unstable();
        assert!(
            v[1] < 60,
            "top-left corner median should be far outside, got {v:?}"
        );
        let last_row = (glyph.height as usize - 1) * stride;
        let bottom_right = &glyph.rgba[last_row + stride - 4..last_row + stride - 1];
        let mut v = [bottom_right[0], bottom_right[1], bottom_right[2]];
        v.sort_unstable();
        assert!(
            v[1] < 60,
            "bottom-right corner median should be far outside, got {v:?}"
        );
    }

    #[test]
    fn distinct_glyphs_have_distinct_bitmaps() {
        let face = test_face();
        let a = build_glyph_msdf(&face, face.glyph_index('A').unwrap().0, 32, 4.0, false).unwrap();
        let b = build_glyph_msdf(&face, face.glyph_index('B').unwrap().0, 32, 4.0, false).unwrap();
        // Different shapes ⇒ different pixel content (or, very loosely,
        // not identical).
        assert!(a.rgba != b.rgba || a.width != b.width || a.height != b.height);
    }
}
