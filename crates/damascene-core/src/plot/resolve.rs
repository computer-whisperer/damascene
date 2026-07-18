//! Resolving a [`PlotSpec`] against its data and laid-out rect: the
//! data-space bounds of its marks, the auto-fit and Y-autoscale view, and
//! the data rect (the inner area marks draw into, inside the axis gutters).
//!
//! These are the pure pieces the per-frame prepare pass
//! ([`UiState::prepare_plots`](crate::state::UiState)) uses to seed and
//! update each plot's [`PlotView`] before `draw_ops` reads it. Kept here,
//! separate and unit-tested, rather than buried in the state walk.

#![warn(missing_docs)]

use crate::plot::scale::Scale;
use crate::plot::series::{Sample, SeriesBounds};
use crate::plot::spec::{Curve, Mark, PlotSpec};
use crate::plot::view::{AxisView, PlotView};
use crate::tree::Rect;

/// Fractional headroom added around the data when auto-fitting a view,
/// measured in scale space (so a log axis pads by ratio — see
/// [`AxisView::fit`]).
pub const FIT_PADDING: f64 = 0.05;

/// Axis-gutter insets that separate a plot node's rect from its data rect,
/// in logical px: room for the X tick labels (bottom) and a small margin
/// (top/right). The left gutter (Y tick labels) is sized adaptively by
/// [`left_gutter`].
const GUTTER_BOTTOM: f32 = 28.0;
const MARGIN_TOP: f32 = 10.0;
const MARGIN_RIGHT: f32 = 12.0;

/// Floor for the adaptive left gutter — it never shrinks below this even when
/// the Y labels are narrow (keeps a tidy axis margin).
pub const GUTTER_LEFT_MIN: f32 = 40.0;
/// Y tick label font size — mirrors the `size` used by `draw_ops`' tick
/// chrome, so the measured gutter matches the labels actually drawn.
const Y_TICK_LABEL_SIZE: f32 = 11.0;
/// Gap reserved beyond the widest Y label: the label's right pad to the data
/// rect plus a small left margin inside the node.
const Y_LABEL_GAP: f32 = 12.0;
/// Number of Y ticks targeted — mirrors `draw_ops`.
const Y_TICK_TARGET: usize = 6;

/// The left gutter needed to fit `view`'s Y tick labels without clipping,
/// floored at [`GUTTER_LEFT_MIN`]. Measures the widest formatted Y tick label
/// in the same font/size/count `draw_ops` draws them.
pub fn left_gutter(spec: &PlotSpec, view: &PlotView) -> f32 {
    let ys = spec.y.scale;
    let mut widest = 0.0_f32;
    for t in ys.ticks((view.y.min, view.y.max), Y_TICK_TARGET) {
        let w = crate::text::metrics::line_width(
            &t.label,
            Y_TICK_LABEL_SIZE,
            crate::tree::FontWeight::default(),
            false,
        );
        widest = widest.max(w);
    }
    (widest + Y_LABEL_GAP).max(GUTTER_LEFT_MIN)
}

/// The data rect for a plot laid out at `node_rect`, inset by `gutter_left`
/// (from [`left_gutter`]) and the fixed bottom/top/right gutters. Clamped to
/// non-negative size.
pub fn data_rect(node_rect: Rect, gutter_left: f32) -> Rect {
    let x = node_rect.x + gutter_left;
    let y = node_rect.y + MARGIN_TOP;
    let w = (node_rect.w - gutter_left - MARGIN_RIGHT).max(0.0);
    let h = (node_rect.h - MARGIN_TOP - GUTTER_BOTTOM).max(0.0);
    Rect::new(x, y, w, h)
}

/// The union of every mark's series bounds — the full data extent of the
/// plot, used to auto-fit the initial view.
pub fn data_bounds(spec: &PlotSpec) -> SeriesBounds {
    let mut bounds = SeriesBounds::default();
    for mark in &spec.marks {
        bounds = bounds.union(series_of(mark).bounds());
    }
    bounds
}

/// The series a mark reads from.
fn series_of(mark: &Mark) -> &crate::plot::series::SeriesHandle {
    match mark {
        Mark::Line(m) => &m.series,
        Mark::Scatter(m) => &m.series,
    }
}

/// An auto-fit [`PlotView`] framing `bounds` with [`FIT_PADDING`] headroom
/// added in scale space (per-axis, through `xs`/`ys`). Missing per-axis
/// bounds fall back to a unit window (via [`PlotView::fit`]).
pub fn autofit(bounds: SeriesBounds, xs: Scale, ys: Scale) -> PlotView {
    PlotView::fit(
        bounds.x.unwrap_or((0.0, 1.0)),
        bounds.y.unwrap_or((0.0, 1.0)),
        FIT_PADDING,
        xs,
        ys,
    )
}

/// The `(min, max)` of `y` over what the spec's marks *draw* within the
/// horizontal window `x` — what `Y::autoscale` fits the value axis to each
/// frame. For every mark this includes the samples inside the window; for a
/// line mark it also includes the y where a segment crosses each window
/// edge, so a polyline that enters or spans the view with no vertex inside
/// it still holds the frame. (Point-only filtering made a marker line whose
/// vertices sat on a previous, wider window vanish from autoscale for the
/// frame after a zoom-in — the la-web one-frame vertical jump.) `None` when
/// nothing is drawn in the window.
pub fn visible_y(spec: &PlotSpec, x: AxisView) -> Option<(f64, f64)> {
    let (lo, hi) = (x.min.min(x.max), x.min.max(x.max));
    let mut acc: Option<(f64, f64)> = None;
    let mut add = |y: f64| {
        acc = Some(match acc {
            Some((ylo, yhi)) => (ylo.min(y), yhi.max(y)),
            None => (y, y),
        });
    };
    for mark in &spec.marks {
        let curve = mark.curve();
        let (samples, _) = series_of(mark).snapshot();
        let mut prev: Option<Sample> = None;
        for &s in samples.iter() {
            if s.x.is_finite() && s.y.is_finite() && s.x >= lo && s.x <= hi {
                add(s.y);
            }
            if let Some(curve) = curve {
                if let Some(p) = prev {
                    for edge in [lo, hi] {
                        edge_crossing_y(p, s, edge, curve, spec.x.scale, spec.y.scale, &mut add);
                    }
                }
                prev = Some(s);
            }
        }
    }
    acc
}

/// Feed `add` the y value(s) the drawn segment `a`→`b` has where it crosses
/// the vertical line `x = edge`; a no-op when it doesn't straddle the edge
/// strictly (an endpoint sitting exactly on the edge is already counted as
/// an in-window sample). A [`Curve::Linear`] segment is straight in **scale
/// space** (that is what [`lower_line`](crate::plot::lower::lower_line)
/// uploads), so the interpolation warps through the axis scales and stays
/// exact on log axes. A step curve holds sample values, so the crossing is
/// the held level — both levels when the riser sits exactly on the edge.
fn edge_crossing_y(
    a: Sample,
    b: Sample,
    edge: f64,
    curve: Curve,
    xs: Scale,
    ys: Scale,
    add: &mut impl FnMut(f64),
) {
    let finite = a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite();
    let straddles = (a.x < edge && edge < b.x) || (b.x < edge && edge < a.x);
    if !finite || !straddles {
        return;
    }
    match curve {
        Curve::Linear => {
            let (fa, fb) = (xs.forward(a.x), xs.forward(b.x));
            let t = (xs.forward(edge) - fa) / (fb - fa);
            let fy = ys.forward(a.y) + t * (ys.forward(b.y) - ys.forward(a.y));
            let y = ys.inverse(fy);
            if y.is_finite() {
                add(y);
            }
        }
        Curve::StepAfter => add(a.y),
        Curve::StepBefore => add(b.y),
        Curve::StepMid => {
            let fe = xs.forward(edge);
            let mid = (xs.forward(a.x) + xs.forward(b.x)) * 0.5;
            if fe <= mid {
                add(a.y);
            }
            if fe >= mid {
                add(b.y);
            }
        }
    }
}

/// Pad a `(min, max)` value span into an [`AxisView`] with [`FIT_PADDING`]
/// headroom added in scale space (through the axis `scale`), nudging a
/// degenerate span to a unit scale-space window.
pub fn pad_y(span: (f64, f64), scale: Scale) -> AxisView {
    AxisView::fit(span, FIT_PADDING, scale)
}

/// Resolve the view for a plot this frame: start from the persisted view
/// (or an auto-fit if there is none), refit the X axis to the full data
/// extent when `autoscale_x` is set (so streaming appends stay in view),
/// then refit the Y axis to the visible data when `autoscale_y` is set.
/// The caller passes the *effective* autoscale state per axis —
/// `spec.{x,y}_autoscale` unless the user has taken manual control of that
/// axis (an X gesture, or box-zooming the value axis), which opts the plot
/// out until reset. `scale` selection lives on the spec; this only moves
/// the windows.
pub fn resolve_view(
    spec: &PlotSpec,
    persisted: Option<PlotView>,
    autoscale_x: bool,
    autoscale_y: bool,
) -> PlotView {
    let bounds = data_bounds(spec);
    let fit = autofit(bounds, spec.x.scale, spec.y.scale);
    let mut view = persisted.unwrap_or(fit);
    if autoscale_x && bounds.x.is_some() {
        view = view.with_x(fit.x);
    }
    if autoscale_y && let Some(span) = visible_y(spec, view.x) {
        view = view.with_y(pad_y(span, spec.y.scale));
    }
    view
}

/// The horizontal-axis [`Scale`] of a spec (a convenience for the prepare
/// pass / metrics).
pub fn x_scale(spec: &PlotSpec) -> Scale {
    spec.x.scale
}

/// The vertical-axis [`Scale`] of a spec.
pub fn y_scale(spec: &PlotSpec) -> Scale {
    spec.y.scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plot::series::{Sample, SeriesHandle};
    use crate::plot::spec::line;

    fn spec_with(samples: Vec<Sample>) -> PlotSpec {
        let h = SeriesHandle::new(samples);
        PlotSpec::new().add_mark(line(&h))
    }

    #[test]
    fn data_rect_insets_gutters() {
        let g = 52.0;
        let r = data_rect(Rect::new(0.0, 0.0, 200.0, 100.0), g);
        assert_eq!(r.x, g);
        assert_eq!(r.y, MARGIN_TOP);
        assert_eq!(r.w, 200.0 - g - MARGIN_RIGHT);
        assert_eq!(r.h, 100.0 - MARGIN_TOP - GUTTER_BOTTOM);
    }

    #[test]
    fn data_rect_clamps_to_nonnegative() {
        let r = data_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 52.0);
        assert_eq!(r.w, 0.0);
        assert_eq!(r.h, 0.0);
    }

    #[test]
    fn left_gutter_grows_for_wide_labels_and_floors() {
        // A series with large Y values needs a wider gutter than one with
        // small values; both are floored at the minimum.
        let wide = spec_with(vec![Sample::new(0.0, 0.0), Sample::new(1.0, 1_000_000.0)]);
        let narrow = spec_with(vec![Sample::new(0.0, 0.0), Sample::new(1.0, 9.0)]);
        let view_w = resolve_view(&wide, None, true, true);
        let view_n = resolve_view(&narrow, None, true, true);
        let g_wide = left_gutter(&wide, &view_w);
        let g_narrow = left_gutter(&narrow, &view_n);
        assert!(
            g_wide > g_narrow,
            "wide labels widen the gutter: {g_wide} vs {g_narrow}"
        );
        assert!(
            g_narrow >= GUTTER_LEFT_MIN,
            "floored at the minimum: {g_narrow}"
        );
    }

    #[test]
    fn data_bounds_union_over_marks() {
        let a = SeriesHandle::new(vec![Sample::new(0.0, 1.0), Sample::new(5.0, 3.0)]);
        let b = SeriesHandle::new(vec![Sample::new(-2.0, 0.0), Sample::new(3.0, 9.0)]);
        let spec = PlotSpec::new().line(&a).line(&b);
        let bounds = data_bounds(&spec);
        assert_eq!(bounds.x, Some((-2.0, 5.0)));
        assert_eq!(bounds.y, Some((0.0, 9.0)));
    }

    #[test]
    fn autofit_pads_the_window() {
        let bounds = SeriesBounds {
            x: Some((0.0, 100.0)),
            y: Some((0.0, 10.0)),
        };
        let v = autofit(bounds, Scale::linear(), Scale::linear());
        assert!(v.x.min < 0.0 && v.x.max > 100.0);
        assert!(v.y.min < 0.0 && v.y.max > 10.0);
    }

    #[test]
    fn visible_y_frames_drawn_content_not_out_of_window_peaks() {
        let spec = spec_with(vec![
            Sample::new(0.0, 1.0),
            Sample::new(5.0, 100.0), // outside the x window below
            Sample::new(1.0, 2.0),
        ]);
        let span = visible_y(&spec, AxisView::new(-0.5, 1.5)).unwrap();
        // The peak itself is out of view, but the segments toward it are
        // drawn up to the window edge: (0,1)→(5,100) reaches y=30.7 at
        // x=1.5. The frame covers the drawn portion, not the peak.
        assert_eq!(span.0, 1.0);
        assert!((span.1 - 30.7).abs() < 1e-9, "edge-clipped max: {span:?}");
    }

    /// The la-web zoom-in transient: a marker line whose two vertices sit
    /// exactly on a previously fetched, wider window still spans the view
    /// after a zoom-in. It must keep holding the Y frame — with point-only
    /// filtering it vanished for one frame and snapped back when the
    /// refetch re-clamped its endpoints to the new window.
    #[test]
    fn visible_y_keeps_window_spanning_segments_in_frame() {
        let lane = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 1.0)]);
        let gap = SeriesHandle::new(vec![Sample::new(0.0, 5.0), Sample::new(10.0, 5.0)]);
        let spec = PlotSpec::new().line(&lane).line(&gap);

        // Steady state: window == the vertices' extent (boundary samples).
        let steady = visible_y(&spec, AxisView::new(0.0, 10.0)).unwrap();
        // One wheel notch in: both gap vertices now sit outside.
        let zoomed = visible_y(&spec, AxisView::new(0.4545, 9.5455)).unwrap();
        assert_eq!(steady.1, 5.0);
        assert_eq!(zoomed.1, 5.0, "spanning segment stays framed: {zoomed:?}");
    }

    #[test]
    fn visible_y_ignores_segments_fully_outside() {
        // Both vertices on the same side of the window: nothing is drawn
        // inside, nothing contributes.
        let spec = spec_with(vec![Sample::new(-5.0, 7.0), Sample::new(-2.0, 9.0)]);
        assert_eq!(visible_y(&spec, AxisView::new(0.0, 10.0)), None);
    }

    #[test]
    fn visible_y_scatter_marks_stay_point_filtered() {
        // A scatter "pair" spanning the window draws nothing inside it —
        // no phantom segment contribution.
        let h = SeriesHandle::new(vec![Sample::new(-1.0, 5.0), Sample::new(11.0, 5.0)]);
        let spec = PlotSpec::new().scatter(&h);
        assert_eq!(visible_y(&spec, AxisView::new(0.0, 10.0)), None);
    }

    #[test]
    fn visible_y_step_crossings_use_held_levels() {
        // One step from y=3 to y=7 spanning the window [4, 6].
        let h = SeriesHandle::new(vec![Sample::new(0.0, 3.0), Sample::new(10.0, 7.0)]);
        // Step-after holds y=3 across the window (the riser at x=10 is
        // outside); no diagonal interpolation.
        let after = PlotSpec::new().add_mark(line(&h).step_after());
        assert_eq!(visible_y(&after, AxisView::new(4.0, 6.0)), Some((3.0, 3.0)));
        // Step-before already jumped at x=0: the window sees y=7.
        let before = PlotSpec::new().add_mark(line(&h).step_before());
        assert_eq!(
            visible_y(&before, AxisView::new(4.0, 6.0)),
            Some((7.0, 7.0))
        );
        // Step-mid: the riser at x=5 is inside the window — both levels.
        let mid = PlotSpec::new().add_mark(line(&h).step_mid());
        assert_eq!(visible_y(&mid, AxisView::new(4.0, 6.0)), Some((3.0, 7.0)));
    }

    #[test]
    fn visible_y_edge_crossing_interpolates_in_scale_space() {
        // With a log Y axis, segments are straight in log space: the
        // crossing halfway (in x) between y=1 and y=100 is y=10, not the
        // linear-space 50.5.
        let h = SeriesHandle::new(vec![Sample::new(0.0, 1.0), Sample::new(2.0, 100.0)]);
        let spec = PlotSpec::new().y(Scale::log()).line(&h);
        let span = visible_y(&spec, AxisView::new(-2.0, 1.0)).unwrap();
        assert!((span.1 - 10.0).abs() < 1e-9, "log-space crossing: {span:?}");
    }

    #[test]
    fn resolve_view_autoscales_y_to_visible() {
        let spec = spec_with(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 1000.0)]);
        // Persist a narrow x window around x=0; Y fits what is drawn there —
        // the segment climbs to y=100 by the right edge (x=1) — not the
        // out-of-view 1000 peak.
        let persisted = PlotView::new(AxisView::new(-1.0, 1.0), AxisView::new(-5.0, 5.0));
        let v = resolve_view(&spec, Some(persisted), false, true);
        assert!(v.y.max < 200.0, "y autoscaled to visible: {:?}", v.y);
        assert!(v.y.max > 100.0, "edge crossing framed with pad: {:?}", v.y);
    }

    #[test]
    fn resolve_view_autofits_when_unpersisted() {
        let spec = spec_with(vec![Sample::new(0.0, 0.0), Sample::new(4.0, 8.0)]);
        let v = resolve_view(&spec, None, true, true);
        assert!(v.x.min < 0.0 && v.x.max > 4.0);
    }
}

#[cfg(test)]
mod log_fit_tests {
    use super::*;
    use crate::plot::series::{Sample, SeriesHandle};
    use crate::plot::spec::line;
    use crate::tree::Rect;

    /// Issue #124: a log-Y plot over data spanning many decades collapsed
    /// onto the top edge with unusable ticks, because the fit padded 5% of
    /// the *raw* span (pushing `y.min` to −3275 for data in 1..=65536) and
    /// the clamped log warp then stretched the window to ~305 decades.
    #[test]
    fn log_y_autofit_keeps_marks_spread_and_ticks_sane() {
        // The issue's shape: a line descending 65536 → 1 over x ∈ [0, 36000].
        let samples: Vec<Sample> = (0..149)
            .map(|i| {
                let x = f64::from(i) * (36000.0 / 148.0);
                let y = 65536.0 * (1.0f64 / 65536.0).powf(f64::from(i) / 148.0);
                Sample::new(x, y)
            })
            .collect();
        let h = SeriesHandle::new(samples);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::log())
            .add_mark(line(&h));

        let view = resolve_view(&spec, None, true, true);
        assert!(
            view.y.min > 0.0,
            "log-y window stays positive: {:?}",
            view.y
        );

        // The data's extremes project across most of the rect, not onto a
        // single edge.
        let rect = Rect::new(0.0, 0.0, 400.0, 300.0);
        let (xs, ys) = (Scale::linear(), Scale::log());
        let top = view.project((0.0, 65536.0), xs, ys, rect).1;
        let bottom = view.project((36000.0, 1.0), xs, ys, rect).1;
        assert!(
            (bottom - top).abs() > rect.h * 0.8,
            "marks span the rect: {top} .. {bottom}"
        );

        // Ticks are the decades of the data, each with a distinct label.
        let ticks = ys.ticks((view.y.min, view.y.max), 6);
        let values: Vec<f64> = ticks.iter().map(|t| t.value).collect();
        assert_eq!(values, vec![1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0]);
        assert!(ticks.iter().all(|t| t.label != "0"), "labels: {ticks:?}");
    }
}

#[cfg(test)]
mod x_autoscale_tests {
    use super::*;
    use crate::plot::Sample;
    use crate::plot::series::SeriesHandle;

    fn spec_of(h: &SeriesHandle) -> PlotSpec {
        PlotSpec::new().line(h)
    }

    /// The #116 regression: a persisted view must track a growing X extent
    /// when autoscale-X is on — streaming appends stay in view.
    #[test]
    fn resolve_view_x_autoscale_tracks_growing_data() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(1.0, 1.0)]);
        let spec = spec_of(&h);
        let first = resolve_view(&spec, None, true, true);
        assert!(first.x.max < 2.0, "seeded around the initial extent");

        // The stream advances well past the seeded window.
        h.append(&[Sample::new(100.0, 5.0)]);
        let next = resolve_view(&spec, Some(first), true, true);
        assert!(
            next.x.max > 100.0,
            "x window follows the data: {:?}",
            next.x
        );
    }

    /// With autoscale-X off (or manual control taken), the persisted X
    /// window holds regardless of data growth — today's sticky behavior.
    #[test]
    fn resolve_view_manual_x_holds_the_window() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(1.0, 1.0)]);
        let spec = spec_of(&h);
        let first = resolve_view(&spec, None, false, true);
        h.append(&[Sample::new(100.0, 5.0)]);
        let next = resolve_view(&spec, Some(first), false, true);
        assert_eq!(next.x, first.x, "manual x window is sticky");
    }
}
