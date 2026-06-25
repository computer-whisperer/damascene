//! Lowering plot samples to scene geometry.
//!
//! A plot's data marks render by reusing the [`scene`](crate::scene) GPU
//! pipelines (the plan's decision 1), so a mark's `f64` samples must become
//! the scene's logical geometry: [`LineData`] (segments) and [`PointData`]
//! (markers / join discs), positioned in **scale space** at `z = 0`.
//!
//! ## Coordinates
//!
//! Each sample `(x, y)` maps through the axis [`Scale`]s, relative to a
//! per-axis `origin` subtracted before the cast to `f32`, so large absolute
//! coordinates — epoch timestamps especially — keep precision on the GPU
//! (the plan's decision 7). The orthographic plot camera then maps the
//! visible scale-space window to the data rect.
//!
//! ## Line joins (the reuse-the-pipelines decision)
//!
//! The scene line pipeline draws each segment as an anti-aliased quad with
//! *butt* caps, which leave wedge-gaps at angled joins. [`lower_line`]
//! fills those by also emitting a round disc (the scene point pipeline draws
//! anti-aliased circles) at **every** vertex, so joins and end-caps read as
//! clean rounds with no new GPU pipeline. The disc diameter equals the line
//! width and is applied as the [`PointStyle`](crate::scene::PointStyle) size
//! by the caller (see `docs/PLOT2D_PLAN.md`, the resolved polyline risk).

#![warn(missing_docs)]

use glam::Vec3;

use crate::color::{Color, ColorSpace};
use crate::plot::scale::Scale;
use crate::plot::series::Sample;
use crate::scene::geometry::{LineData, LineSegment, PointData, ScenePoint};

/// The lowered geometry of a line mark: the connecting segments plus the
/// round join/cap discs that clean up the butt-cap joins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoweredLine {
    /// One segment per consecutive sample pair.
    pub segments: LineData,
    /// One disc per vertex (sized to the line width by the caller).
    pub joins: PointData,
}

/// Convert a `Color` to the authoring-space sRGBA `[f32; 4]` the scene
/// geometry stores (the backend converts to working-linear at upload).
fn srgba(color: Color) -> [f32; 4] {
    let c = color.convert_to(ColorSpace::SRGB);
    [c.r, c.g, c.b, c.a]
}

/// Map a sample to a scale-space position at `z = 0`.
fn position(s: Sample, x: Scale, y: Scale, origin: (f64, f64)) -> Vec3 {
    Vec3::new(x.map(s.x, origin.0), y.map(s.y, origin.1), 0.0)
}

/// Lower a line mark's `samples` to connecting [`LineData`] plus round
/// join/cap discs ([`PointData`]), all in scale space relative to `origin`.
/// `color` is applied to every segment and disc.
///
/// Degenerate inputs: zero samples yield empty geometry; a single sample
/// yields no segments and one disc (a dot).
pub fn lower_line(
    samples: &[Sample],
    x: Scale,
    y: Scale,
    origin: (f64, f64),
    color: Color,
) -> LoweredLine {
    let rgba = srgba(color);
    let positions: Vec<Vec3> = samples.iter().map(|&s| position(s, x, y, origin)).collect();

    let segments = positions
        .windows(2)
        .map(|w| LineSegment {
            start: w[0],
            end: w[1],
            color: rgba,
        })
        .collect();

    let joins = positions
        .iter()
        .map(|&p| ScenePoint {
            position: p,
            color: rgba,
        })
        .collect();

    LoweredLine {
        segments: LineData { segments },
        joins: PointData { points: joins },
    }
}

/// Lower a scatter mark's `samples` to [`PointData`] in scale space relative
/// to `origin`, each marker coloured `color`.
pub fn lower_scatter(
    samples: &[Sample],
    x: Scale,
    y: Scale,
    origin: (f64, f64),
    color: Color,
) -> PointData {
    let rgba = srgba(color);
    PointData {
        points: samples
            .iter()
            .map(|&s| ScenePoint {
                position: position(s, x, y, origin),
                color: rgba,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: f64, y: f64) -> Sample {
        Sample::new(x, y)
    }

    #[test]
    fn line_segment_and_join_counts() {
        let pts = [s(0.0, 0.0), s(1.0, 1.0), s(2.0, 0.0)];
        let l = lower_line(
            &pts,
            Scale::linear(),
            Scale::linear(),
            (0.0, 0.0),
            Color::srgb_u8(255, 255, 255),
        );
        assert_eq!(l.segments.segments.len(), 2); // n-1 segments
        assert_eq!(l.joins.points.len(), 3); // one disc per vertex (round caps+joins)
    }

    #[test]
    fn line_positions_are_origin_relative_scale_space() {
        let pts = [s(100.0, 10.0), s(101.0, 12.0)];
        let l = lower_line(
            &pts,
            Scale::linear(),
            Scale::linear(),
            (100.0, 10.0),
            Color::srgb_u8(255, 255, 255),
        );
        // first vertex sits at the origin → (0,0,0)
        assert_eq!(l.segments.segments[0].start, Vec3::new(0.0, 0.0, 0.0));
        // second vertex is +1 in x, +2 in y
        assert_eq!(l.segments.segments[0].end, Vec3::new(1.0, 2.0, 0.0));
    }

    #[test]
    fn time_origin_keeps_f32_precision() {
        // epoch seconds ~1.78e9: absolute value would lose sub-second
        // precision in f32, but origin-relative stays exact.
        let base = 1_780_000_000.0_f64;
        let pts = [s(base, 1.0), s(base + 0.5, 2.0)];
        let l = lower_line(
            &pts,
            Scale::time(),
            Scale::linear(),
            (base, 0.0),
            Color::srgb_u8(255, 255, 255),
        );
        assert_eq!(l.segments.segments[0].start.x, 0.0);
        assert_eq!(l.segments.segments[0].end.x, 0.5);
    }

    #[test]
    fn single_sample_is_a_dot() {
        let l = lower_line(
            &[s(5.0, 5.0)],
            Scale::linear(),
            Scale::linear(),
            (0.0, 0.0),
            Color::srgb_u8(255, 255, 255),
        );
        assert!(l.segments.segments.is_empty());
        assert_eq!(l.joins.points.len(), 1);
    }

    #[test]
    fn empty_is_empty() {
        let l = lower_line(
            &[],
            Scale::linear(),
            Scale::linear(),
            (0.0, 0.0),
            Color::srgb_u8(255, 255, 255),
        );
        assert!(l.segments.segments.is_empty());
        assert!(l.joins.points.is_empty());
    }

    #[test]
    fn log_x_maps_through_warp() {
        // x = 1000 with a log10 axis, origin at 1.0 → log10(1000) - log10(1) = 3
        let l = lower_line(
            &[s(1.0, 0.0), s(1000.0, 0.0)],
            Scale::log(),
            Scale::linear(),
            (1.0, 0.0),
            Color::srgb_u8(255, 255, 255),
        );
        assert!((l.segments.segments[0].end.x - 3.0).abs() < 1e-5);
    }

    #[test]
    fn scatter_maps_each_sample() {
        let pts = [s(0.0, 0.0), s(2.0, 4.0)];
        let p = lower_scatter(
            &pts,
            Scale::linear(),
            Scale::linear(),
            (0.0, 0.0),
            Color::srgb_u8(255, 255, 255),
        );
        assert_eq!(p.points.len(), 2);
        assert_eq!(p.points[1].position, Vec3::new(2.0, 4.0, 0.0));
    }
}
