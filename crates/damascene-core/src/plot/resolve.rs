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
use crate::plot::series::SeriesBounds;
use crate::plot::spec::{Mark, PlotSpec};
use crate::plot::view::{AxisView, PlotView};
use crate::tree::Rect;

/// Fractional headroom added around the data when auto-fitting a view.
pub const FIT_PADDING: f64 = 0.05;

/// Axis-gutter insets that separate a plot node's rect from its data rect,
/// in logical px: room for the Y tick labels + title (left), the X tick
/// labels (bottom), and a small margin (top/right). Fixed for now; a later
/// pass can size them from measured label extents.
const GUTTER_LEFT: f32 = 52.0;
const GUTTER_BOTTOM: f32 = 28.0;
const MARGIN_TOP: f32 = 10.0;
const MARGIN_RIGHT: f32 = 12.0;

/// The data rect for a plot laid out at `node_rect`: the node rect inset by
/// the axis gutters. Clamped to non-negative size.
pub fn data_rect(node_rect: Rect) -> Rect {
    let x = node_rect.x + GUTTER_LEFT;
    let y = node_rect.y + MARGIN_TOP;
    let w = (node_rect.w - GUTTER_LEFT - MARGIN_RIGHT).max(0.0);
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

/// An auto-fit [`PlotView`] framing `bounds` with [`FIT_PADDING`] headroom.
/// Missing per-axis bounds fall back to a unit window (via
/// [`PlotView::fit`]).
pub fn autofit(bounds: SeriesBounds) -> PlotView {
    PlotView::fit(
        bounds.x.unwrap_or((0.0, 1.0)),
        bounds.y.unwrap_or((0.0, 1.0)),
        FIT_PADDING,
    )
}

/// The `(min, max)` of `y` over every sample whose `x` lies within the
/// horizontal window `x` — what `Y::autoscale` fits the value axis to each
/// frame. `None` when no sample is in range.
pub fn visible_y(spec: &PlotSpec, x: AxisView) -> Option<(f64, f64)> {
    let (lo, hi) = (x.min.min(x.max), x.min.max(x.max));
    let mut acc: Option<(f64, f64)> = None;
    for mark in &spec.marks {
        let (samples, _) = series_of(mark).snapshot();
        for s in samples.iter() {
            if s.x.is_finite() && s.y.is_finite() && s.x >= lo && s.x <= hi {
                acc = Some(match acc {
                    Some((ylo, yhi)) => (ylo.min(s.y), yhi.max(s.y)),
                    None => (s.y, s.y),
                });
            }
        }
    }
    acc
}

/// Pad a `(min, max)` value span into an [`AxisView`] with [`FIT_PADDING`]
/// headroom, nudging a degenerate span to a unit window.
pub fn pad_y(span: (f64, f64)) -> AxisView {
    let (lo, hi) = span;
    if hi <= lo {
        return AxisView::new(lo - 0.5, hi + 0.5);
    }
    let m = (hi - lo) * FIT_PADDING;
    AxisView::new(lo - m, hi + m)
}

/// Resolve the view for a plot this frame: start from the persisted view
/// (or an auto-fit if there is none), then refit the Y axis to the visible
/// data when `y_autoscale` is on. `scale` selection lives on the spec; this
/// only moves the windows.
pub fn resolve_view(spec: &PlotSpec, persisted: Option<PlotView>) -> PlotView {
    let bounds = data_bounds(spec);
    let mut view = persisted.unwrap_or_else(|| autofit(bounds));
    if spec.y_autoscale {
        if let Some(span) = visible_y(spec, view.x) {
            view = view.with_y(pad_y(span));
        }
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
        let r = data_rect(Rect::new(0.0, 0.0, 200.0, 100.0));
        assert_eq!(r.x, GUTTER_LEFT);
        assert_eq!(r.y, MARGIN_TOP);
        assert_eq!(r.w, 200.0 - GUTTER_LEFT - MARGIN_RIGHT);
        assert_eq!(r.h, 100.0 - MARGIN_TOP - GUTTER_BOTTOM);
    }

    #[test]
    fn data_rect_clamps_to_nonnegative() {
        let r = data_rect(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(r.w, 0.0);
        assert_eq!(r.h, 0.0);
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
        let v = autofit(bounds);
        assert!(v.x.min < 0.0 && v.x.max > 100.0);
        assert!(v.y.min < 0.0 && v.y.max > 10.0);
    }

    #[test]
    fn visible_y_only_counts_in_window() {
        let spec = spec_with(vec![
            Sample::new(0.0, 1.0),
            Sample::new(5.0, 100.0), // outside the x window below
            Sample::new(1.0, 2.0),
        ]);
        let span = visible_y(&spec, AxisView::new(-0.5, 1.5)).unwrap();
        assert_eq!(span, (1.0, 2.0)); // the 100.0 at x=5 is excluded
    }

    #[test]
    fn resolve_view_autoscales_y_to_visible() {
        let spec = spec_with(vec![
            Sample::new(0.0, 0.0),
            Sample::new(10.0, 1000.0),
        ]);
        // Persist a narrow x window around x=0; Y should fit ~0, not 1000.
        let persisted = PlotView::new(AxisView::new(-1.0, 1.0), AxisView::new(-5.0, 5.0));
        let v = resolve_view(&spec, Some(persisted));
        assert!(v.y.max < 100.0, "y autoscaled to visible: {:?}", v.y);
    }

    #[test]
    fn resolve_view_autofits_when_unpersisted() {
        let spec = spec_with(vec![Sample::new(0.0, 0.0), Sample::new(4.0, 8.0)]);
        let v = resolve_view(&spec, None);
        assert!(v.x.min < 0.0 && v.x.max > 4.0);
    }
}
