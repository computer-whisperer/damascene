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
use crate::plot::spec::Curve;
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

/// Expand `samples` into the square-edged polyline a step [`Curve`] draws —
/// alternating horizontal holds and vertical risers, as data-space points —
/// ready for [`lower_line`]. [`Curve::Linear`] returns the samples
/// unchanged.
///
/// Consecutive equal-`y` samples merge into one horizontal run (no interior
/// vertices), so an idle digital channel lowers to a single segment no
/// matter how many samples cover it — and the join discs [`lower_line`]
/// emits per vertex land only on actual corners. Riser placement follows
/// the raw sample pair, not the merged run: [`Curve::StepBefore`] rises at
/// the previous sample's `x`, [`Curve::StepMid`] halfway in **scale space**
/// (`xs`) between the previous and current samples. Non-finite samples pass
/// through unchanged (the same garbage-in-garbage-out as linear lowering)
/// and restart the run.
pub fn step_points(samples: &[Sample], curve: Curve, xs: Scale) -> Vec<Sample> {
    if !curve.is_step() {
        return samples.to_vec();
    }
    let finite = |s: Sample| s.x.is_finite() && s.y.is_finite();
    // StepMid emits up to 3 points per sample (two riser corners + the
    // sample); the other modes at most 2.
    let per_sample = if curve == Curve::StepMid { 3 } else { 2 };
    let mut out: Vec<Sample> = Vec::with_capacity(samples.len() * per_sample);
    // Skip exact duplicates of the last emitted point (zero-length
    // segments from duplicate-x input).
    let push = |out: &mut Vec<Sample>, p: Sample| {
        if out.last() != Some(&p) {
            out.push(p);
        }
    };
    let mut prev: Option<Sample> = None;
    for &s in samples {
        match prev {
            None => out.push(s),
            Some(p) if !finite(p) || !finite(s) => {
                // A gap boundary: terminate a merged run at `p` before the
                // break (no-op when `p` was emitted), so the hold up to the
                // gap still draws — the gap analogue of the tail fix-up.
                if finite(p) {
                    push(&mut out, p);
                }
                push(&mut out, s);
            }
            Some(p) => {
                // Equal level: the horizontal run extends implicitly (the
                // tail fix-up below terminates a run that ends the series).
                if s.y != p.y {
                    match curve {
                        Curve::Linear => unreachable!("returned above"),
                        Curve::StepAfter => {
                            push(&mut out, Sample::new(s.x, p.y));
                            push(&mut out, s);
                        }
                        Curve::StepBefore => {
                            // The riser stands at `p` — terminate a merged
                            // run there first (no-op when `p` was emitted).
                            push(&mut out, p);
                            push(&mut out, Sample::new(p.x, s.y));
                            push(&mut out, s);
                        }
                        Curve::StepMid => {
                            let xm = xs.inverse((xs.forward(p.x) + xs.forward(s.x)) * 0.5);
                            push(&mut out, Sample::new(xm, p.y));
                            push(&mut out, Sample::new(xm, s.y));
                            push(&mut out, s);
                        }
                    }
                }
            }
        }
        prev = Some(s);
    }
    // A series ending in a merged constant run still terminates at the true
    // last sample.
    if let Some(&last) = samples.last()
        && finite(last)
        && out.last() != Some(&last)
    {
        out.push(last);
    }
    out
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
    fn step_after_expands_holds_and_risers() {
        let pts = [s(0.0, 0.0), s(1.0, 1.0), s(2.0, 0.0)];
        let out = step_points(&pts, Curve::StepAfter, Scale::linear());
        assert_eq!(
            out,
            vec![
                s(0.0, 0.0),
                s(1.0, 0.0),
                s(1.0, 1.0),
                s(2.0, 1.0),
                s(2.0, 0.0)
            ]
        );
    }

    #[test]
    fn step_before_rises_at_the_previous_sample() {
        let pts = [s(0.0, 5.0), s(3.0, 8.0)];
        let out = step_points(&pts, Curve::StepBefore, Scale::linear());
        assert_eq!(out, vec![s(0.0, 5.0), s(0.0, 8.0), s(3.0, 8.0)]);
    }

    #[test]
    fn step_mid_switches_at_the_scale_space_midpoint() {
        // Log x: the visual midpoint of 1..100 is the geometric mean, 10.
        let pts = [s(1.0, 0.0), s(100.0, 1.0)];
        let out = step_points(&pts, Curve::StepMid, Scale::log());
        assert_eq!(out.len(), 4);
        assert!((out[1].x - 10.0).abs() < 1e-12, "riser at {}", out[1].x);
        assert_eq!((out[1].y, out[2].y), (0.0, 1.0));
    }

    #[test]
    fn step_collinear_runs_merge_to_one_segment() {
        let pts = [s(0.0, 5.0), s(1.0, 5.0), s(2.0, 5.0), s(3.0, 5.0)];
        let out = step_points(&pts, Curve::StepAfter, Scale::linear());
        assert_eq!(out, vec![s(0.0, 5.0), s(3.0, 5.0)]);
    }

    #[test]
    fn step_merged_run_places_risers_at_raw_samples() {
        // A merged 5-run ending in a step to 8: the riser placement follows
        // the raw sample pair (2,5)→(3,8), not the run start.
        let pts = [s(0.0, 5.0), s(2.0, 5.0), s(3.0, 8.0)];
        assert_eq!(
            step_points(&pts, Curve::StepAfter, Scale::linear()),
            vec![s(0.0, 5.0), s(3.0, 5.0), s(3.0, 8.0)]
        );
        assert_eq!(
            step_points(&pts, Curve::StepBefore, Scale::linear()),
            vec![s(0.0, 5.0), s(2.0, 5.0), s(2.0, 8.0), s(3.0, 8.0)]
        );
        assert_eq!(
            step_points(&pts, Curve::StepMid, Scale::linear()),
            vec![s(0.0, 5.0), s(2.5, 5.0), s(2.5, 8.0), s(3.0, 8.0)]
        );
    }

    #[test]
    fn step_duplicate_x_riser_input_passes_through() {
        // Hand-rolled riser samples (duplicate x) survive unchanged.
        let pts = [s(0.0, 0.0), s(5.0, 0.0), s(5.0, 1.0), s(9.0, 1.0)];
        let out = step_points(&pts, Curve::StepAfter, Scale::linear());
        assert_eq!(out, pts.to_vec());
    }

    #[test]
    fn step_constant_tail_terminates_at_the_last_sample() {
        let pts = [s(0.0, 5.0), s(4.0, 5.0)];
        let out = step_points(&pts, Curve::StepMid, Scale::linear());
        assert_eq!(out, pts.to_vec());
    }

    #[test]
    fn step_linear_returns_input_unchanged() {
        let pts = [s(0.0, 0.0), s(1.0, 1.0)];
        assert_eq!(
            step_points(&pts, Curve::Linear, Scale::linear()),
            pts.to_vec()
        );
    }

    #[test]
    fn step_nonfinite_passes_through() {
        let pts = [s(0.0, 1.0), s(f64::NAN, f64::NAN), s(2.0, 3.0)];
        let out = step_points(&pts, Curve::StepAfter, Scale::linear());
        assert_eq!(out.len(), 3);
        assert!(out[1].x.is_nan());
        assert_eq!(out[2], s(2.0, 3.0));
    }

    #[test]
    fn step_gap_terminates_the_merged_run() {
        // A merged constant run cut by a NaN gap still draws its hold up to
        // the last real sample — the gap boundary terminates the run just
        // like the series tail does.
        let pts = [s(0.0, 5.0), s(1.0, 5.0), s(f64::NAN, f64::NAN), s(3.0, 7.0)];
        let out = step_points(&pts, Curve::StepAfter, Scale::linear());
        assert_eq!(out.len(), 4);
        assert_eq!((out[0], out[1]), (s(0.0, 5.0), s(1.0, 5.0)));
        assert!(out[2].x.is_nan());
        assert_eq!(out[3], s(3.0, 7.0));
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
