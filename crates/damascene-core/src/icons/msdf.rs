//! MSDF generation for vector icons.
//!
//! Mirrors `text::msdf` for `VectorAsset`s: each icon is rasterized once
//! into a multi-channel signed-distance field covering its source view
//! box, then sampled at any logical size by the same MSDF shader the
//! text pipeline uses. Stroke-only paths (lucide's default style) are
//! expanded to closed outlines via kurbo before being fed into fdsm,
//! since fdsm operates on closed contours.
//!
//! ## Output coordinate system
//!
//! The MSDF bitmap is **y-down** (row 0 at the top). The icon's source
//! view box `[vx, vy, vw, vh]` is mapped to atlas pixels with a fixed
//! `spread_px` margin on every side, so a destination quad placed at
//! logical rect `[dx, dy, dw, dh]` should be expanded to
//! `[dx - dw * spread_px / (vw * px_per_unit), dy - dh * spread_px /
//! (vh * px_per_unit), dw * width / (vw * px_per_unit), dh * height /
//! (vh * px_per_unit)]` to include the spread margin.

use fdsm::{
    bezier::{Point, Segment},
    correct_error::{ErrorCorrectionConfig, correct_error_mtsdf},
    generate::generate_mtsdf,
    render::correct_sign_mtsdf,
    shape::{Contour, Shape},
    transform::Transform,
};
use kurbo::{BezPath, PathEl, Stroke, StrokeOpts, stroke};
use nalgebra::{Affine2, Matrix3};

use crate::vector::{VectorAsset, VectorLineCap, VectorLineJoin, VectorSegment, VectorStroke};

/// One rasterized icon in MTSDF form, plus the metrics needed to place
/// its quad. The bitmap covers the source view box plus a `spread_px`
/// margin on every side. RGB carries the standard MSDF; A carries a
/// true single-channel SDF that the shader uses as a fallback at
/// sharp corners (kurbo's stroke→fill expansion creates plenty of
/// these, e.g. at miter joins where the stem meets the crossbar of a
/// stroked T).
#[derive(Clone, Debug, PartialEq)]
pub struct IconMsdf {
    /// RGBA MTSDF bitmap, packed `[r,g,b,a, …]`, length `width * height * 4`.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// MSDF spread radius in atlas pixels. Pass this through to the
    /// shader's `params.x` slot.
    pub spread: f32,
    /// Atlas pixels per source view-box unit. The icon's view box
    /// (`vw × vh` units) occupies `vw*px_per_unit × vh*px_per_unit`
    /// pixels in the centre of the bitmap.
    pub px_per_unit: f32,
    /// Source view box `[vx, vy, vw, vh]`, copied from the `VectorAsset`.
    pub view_box: [f32; 4],
}

/// Generate an MSDF for one vector asset.
///
/// `px_per_unit` is the atlas-pixel resolution per view-box unit;
/// `64.0 / 24.0` (≈2.67) gives roughly 64-pixel icons for lucide's
/// 24-unit view box. `spread_px` is the SDF half-range in atlas pixels;
/// 4–6 is typical. `default_stroke_width` is used when a path's stroke
/// is `currentColor` (lucide's default), so it inherits the runtime
/// stroke width instead of a baked-in value — this is baked into the
/// MSDF, so customising stroke width per render requires regenerating.
///
/// Returns `None` if the asset has no renderable paths or the resolved
/// bitmap dimensions would be zero.
pub fn build_icon_msdf(
    asset: &VectorAsset,
    px_per_unit: f64,
    spread_px: f64,
    default_stroke_width: f64,
) -> Option<IconMsdf> {
    let [vx, vy, vw, vh] = asset.view_box;
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }

    let width = ((vw as f64) * px_per_unit + 2.0 * spread_px).ceil() as u32;
    let height = ((vh as f64) * px_per_unit + 2.0 * spread_px).ceil() as u32;
    if width == 0 || height == 0 {
        return None;
    }

    // Collect every fill / outlined-stroke contour into one fdsm Shape,
    // still in source view-box coordinates.
    let mut shape: Shape<Contour> = Shape::default();
    for path in &asset.paths {
        let bez = vector_segments_to_kurbo(&path.segments);
        if bez.is_empty() {
            continue;
        }
        if path.fill.is_some() {
            push_bezpath_contours(&bez, &mut shape);
        }
        if let Some(stroke) = path.stroke {
            let outlined = expand_stroke_to_fill(&bez, &stroke, default_stroke_width);
            push_bezpath_contours(&outlined, &mut shape);
        }
    }
    if shape.contours.is_empty() {
        return None;
    }

    // Map view box -> atlas: place (vx, vy) at (spread, spread).
    let s = px_per_unit;
    let tx = spread_px - (vx as f64) * s;
    let ty = spread_px - (vy as f64) * s;
    let m = Matrix3::new(s, 0.0, tx, 0.0, s, ty, 0.0, 0.0, 1.0);
    let transform = Affine2::from_matrix_unchecked(m);
    shape.transform(&transform);

    let colored = Shape::edge_coloring_simple(shape, 0.03, 0);
    let prepared = colored.prepare();
    let mut buf_f = image::Rgba32FImage::new(width, height);
    generate_mtsdf(&prepared, spread_px, &mut buf_f);
    correct_error_mtsdf(
        &mut buf_f,
        &colored,
        &prepared,
        spread_px,
        &ErrorCorrectionConfig::default(),
    );
    correct_sign_mtsdf(
        &mut buf_f,
        &prepared,
        fdsm::bezier::scanline::FillRule::Nonzero,
    );
    let buf = rgba32f_to_rgba8(&buf_f);

    Some(IconMsdf {
        rgba: buf.into_raw(),
        width,
        height,
        spread: spread_px as f32,
        px_per_unit: px_per_unit as f32,
        view_box: asset.view_box,
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

fn vector_segments_to_kurbo(segments: &[VectorSegment]) -> BezPath {
    let mut path = BezPath::new();
    let mut started = false;
    let mut start = kurbo::Point::ZERO;
    let mut last = kurbo::Point::ZERO;
    for seg in segments {
        match *seg {
            VectorSegment::MoveTo(p) => {
                let pt = kurbo::Point::new(p[0] as f64, p[1] as f64);
                path.move_to(pt);
                start = pt;
                last = pt;
                started = true;
            }
            VectorSegment::LineTo(p) => {
                let pt = kurbo::Point::new(p[0] as f64, p[1] as f64);
                if started {
                    path.line_to(pt);
                    last = pt;
                }
            }
            VectorSegment::QuadTo(p1, p) => {
                let c = kurbo::Point::new(p1[0] as f64, p1[1] as f64);
                let pt = kurbo::Point::new(p[0] as f64, p[1] as f64);
                if started {
                    path.quad_to(c, pt);
                    last = pt;
                }
            }
            VectorSegment::CubicTo(p1, p2, p) => {
                let c1 = kurbo::Point::new(p1[0] as f64, p1[1] as f64);
                let c2 = kurbo::Point::new(p2[0] as f64, p2[1] as f64);
                let pt = kurbo::Point::new(p[0] as f64, p[1] as f64);
                if started {
                    path.curve_to(c1, c2, pt);
                    last = pt;
                }
            }
            VectorSegment::Close => {
                if started {
                    path.close_path();
                    last = start;
                }
            }
        }
    }
    let _ = last; // suppress unused warning under some configurations
    path
}

/// Use kurbo to convert a stroked path into a filled outline. fdsm
/// only operates on closed contours, so stroke-only icons (lucide's
/// default) need to be expanded before the MSDF can be generated.
fn expand_stroke_to_fill(
    path: &BezPath,
    stroke_style: &VectorStroke,
    default_stroke_width: f64,
) -> BezPath {
    let width = if matches!(stroke_style.color, crate::vector::VectorColor::CurrentColor) {
        // lucide-style: width comes from the runtime stroke setting.
        default_stroke_width
    } else {
        stroke_style.width as f64
    }
    .max(0.001);

    let style = Stroke::new(width)
        .with_join(match stroke_style.line_join {
            VectorLineJoin::Miter | VectorLineJoin::MiterClip => kurbo::Join::Miter,
            VectorLineJoin::Round => kurbo::Join::Round,
            VectorLineJoin::Bevel => kurbo::Join::Bevel,
        })
        .with_miter_limit(stroke_style.miter_limit.max(1.0) as f64)
        .with_caps(match stroke_style.line_cap {
            VectorLineCap::Butt => kurbo::Cap::Butt,
            VectorLineCap::Round => kurbo::Cap::Round,
            VectorLineCap::Square => kurbo::Cap::Square,
        });
    // Tolerance in source view-box units. 0.05 matches the lyon
    // tessellation tolerance used elsewhere — small enough that the
    // outlined stroke tracks the centre line tightly.
    stroke(path, &style, &StrokeOpts::default(), 0.05)
}

fn push_bezpath_contours(path: &BezPath, shape: &mut Shape<Contour>) {
    let mut start: Option<Point> = None;
    let mut last: Option<Point> = None;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                close_open_contour(shape, &mut start, &mut last);
                let pt = Point::new(p.x, p.y);
                start = Some(pt);
                last = Some(pt);
                shape.contours.push(Contour::default());
            }
            PathEl::LineTo(p) => {
                let pt = Point::new(p.x, p.y);
                if let (Some(c), Some(prev)) = (shape.contours.last_mut(), last) {
                    c.segments.push(Segment::line(prev, pt));
                }
                last = Some(pt);
            }
            PathEl::QuadTo(c1, p) => {
                let cp1 = Point::new(c1.x, c1.y);
                let pt = Point::new(p.x, p.y);
                if let (Some(c), Some(prev)) = (shape.contours.last_mut(), last) {
                    c.segments.push(Segment::quad(prev, cp1, pt));
                }
                last = Some(pt);
            }
            PathEl::CurveTo(c1, c2, p) => {
                let cp1 = Point::new(c1.x, c1.y);
                let cp2 = Point::new(c2.x, c2.y);
                let pt = Point::new(p.x, p.y);
                if let (Some(c), Some(prev)) = (shape.contours.last_mut(), last) {
                    c.segments.push(Segment::cubic(prev, cp1, cp2, pt));
                }
                last = Some(pt);
            }
            PathEl::ClosePath => {
                if let (Some(c), Some(prev), Some(s)) = (shape.contours.last_mut(), last, start)
                    && (prev - s).norm() > 1e-6
                {
                    c.segments.push(Segment::line(prev, s));
                }
                last = start;
            }
        }
    }
    close_open_contour(shape, &mut start, &mut last);

    // Drop empty contours that never got any segments.
    shape.contours.retain(|c| !c.segments.is_empty());
}

fn close_open_contour(
    shape: &mut Shape<Contour>,
    start: &mut Option<Point>,
    last: &mut Option<Point>,
) {
    if let (Some(c), Some(prev), Some(s)) = (shape.contours.last_mut(), *last, *start)
        && (prev - s).norm() > 1e-6
    {
        c.segments.push(Segment::line(prev, s));
    }
    *start = None;
    *last = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::icon_vector_asset;
    use crate::tree::IconName;

    fn build(name: IconName, px_per_unit: f64, spread: f64) -> IconMsdf {
        let asset = icon_vector_asset(name);
        build_icon_msdf(asset, px_per_unit, spread, 2.0).unwrap_or_else(|| {
            panic!("icon {name:?} produced no MSDF");
        })
    }

    #[test]
    fn x_icon_produces_msdf() {
        let m = build(IconName::X, 64.0 / 24.0, 6.0);
        assert_eq!(m.spread, 6.0);
        assert!((m.px_per_unit - (64.0_f32 / 24.0_f32)).abs() < 1e-4);
        assert_eq!(m.rgba.len() as u32, m.width * m.height * 4);
        // 24×24 view box at 64/24 px/unit ≈ 64×64 plus 6 px spread → 76.
        assert!(m.width >= 64 && m.width <= 96, "{}", m.width);
        assert!(m.height >= 64 && m.height <= 96, "{}", m.height);
    }

    #[test]
    fn x_icon_has_inside_pixels_along_diagonal() {
        let m = build(IconName::X, 64.0 / 24.0, 6.0);
        let stride = m.width as usize * 4;
        let cx = (m.width / 2) as usize;
        let cy = (m.height / 2) as usize;
        let off = cy * stride + cx * 4;
        let mut v = [m.rgba[off], m.rgba[off + 1], m.rgba[off + 2]];
        v.sort_unstable();
        assert!(
            v[1] > 200,
            "expected centre to be inside the X stroke, got {v:?}"
        );
    }

    #[test]
    fn x_icon_corners_are_outside() {
        let m = build(IconName::X, 64.0 / 24.0, 6.0);
        let stride = m.width as usize * 4;
        let corner = [m.rgba[0], m.rgba[1], m.rgba[2]];
        let mut v = corner;
        v.sort_unstable();
        assert!(v[1] < 60, "top-left corner should be outside, got {v:?}");
        let last_row = (m.height as usize - 1) * stride;
        let br = [
            m.rgba[last_row + stride - 4],
            m.rgba[last_row + stride - 3],
            m.rgba[last_row + stride - 2],
        ];
        let mut v = br;
        v.sort_unstable();
        assert!(
            v[1] < 60,
            "bottom-right corner should be outside, got {v:?}"
        );
    }

    #[test]
    fn check_icon_produces_msdf() {
        let m = build(IconName::Check, 64.0 / 24.0, 6.0);
        assert!(!m.rgba.is_empty());
    }

    #[test]
    fn info_icon_produces_msdf() {
        let m = build(IconName::Info, 64.0 / 24.0, 6.0);
        assert!(!m.rgba.is_empty());
    }

    #[test]
    fn settings_icon_produces_msdf() {
        let m = build(IconName::Settings, 64.0 / 24.0, 6.0);
        assert!(!m.rgba.is_empty());
    }
}
