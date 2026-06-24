//! The plot's pan/zoom view state: a visible data-space window per axis,
//! plus the projection, pan, and zoom algebra that maps between data space
//! and the pixels of the plot's data rect.
//!
//! [`PlotView`] is to a `plot` what
//! [`ViewportView`](crate::viewport::ViewportView) is to a `viewport`:
//! per-node interaction state, persisted across rebuilds in
//! [`UiState`](crate::state::UiState) and keyed by the plot's `.key(...)`.
//! The difference — and the reason a plot does **not** reuse the
//! `viewport()` widget — is that a plot zooms each axis **independently and
//! non-uniformly**: you can stretch the time axis while the value axis
//! autoscales, and a log axis zooms affine-in-`log` space. So `PlotView`
//! carries a separate [`AxisView`] per axis and does all of its panning and
//! zooming in **scale space** (through the axis [`Scale`]), exactly the
//! coordinate in which those operations stay affine (see
//! `docs/PLOT2D_PLAN.md`, decisions 2 and 4).

#![warn(missing_docs)]

use crate::Rect;
use crate::plot::scale::Scale;

/// The visible window of one axis, in **data space** (the units the app
/// measures in). For a [`Scale::Time`](crate::plot::Scale::Time) axis these
/// are epoch seconds; for [`Scale::Log`](crate::plot::Scale::Log) they are
/// the raw positive values, not their logarithms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisView {
    /// The window's lower bound, in data space.
    pub min: f64,
    /// The window's upper bound, in data space.
    pub max: f64,
}

impl AxisView {
    /// A window spanning `min..=max`.
    pub fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    /// The window in scale space: `(forward(min), forward(max))`.
    fn forward(self, scale: Scale) -> (f64, f64) {
        (scale.forward(self.min), scale.forward(self.max))
    }

    /// Zoom about a scale-space fraction `t` of the window (`0.0` = `min`
    /// edge, `1.0` = `max` edge), scaling the window width by `factor`
    /// (`< 1.0` zooms in, `> 1.0` zooms out) while keeping the data under
    /// `t` fixed.
    fn zoomed(self, factor: f64, t: f64, scale: Scale) -> Self {
        let (fa, fb) = self.forward(scale);
        let anchor = fa + t * (fb - fa);
        Self {
            min: scale.inverse(anchor + (fa - anchor) * factor),
            max: scale.inverse(anchor + (fb - anchor) * factor),
        }
    }

    /// Pan by `frac` of the window width, measured in scale space. Positive
    /// `frac` moves the window toward larger values.
    fn panned(self, frac: f64, scale: Scale) -> Self {
        let (fa, fb) = self.forward(scale);
        let d = frac * (fb - fa);
        Self {
            min: scale.inverse(fa + d),
            max: scale.inverse(fb + d),
        }
    }

    /// Map a data value to a `0.0..=1.0` scale-space fraction of the window
    /// (`0.0` at `min`, `1.0` at `max`). Returns the unclamped fraction so
    /// out-of-window values land outside `[0, 1]`.
    fn fraction(self, v: f64, scale: Scale) -> f64 {
        let (fa, fb) = self.forward(scale);
        if fb == fa {
            0.0
        } else {
            (scale.forward(v) - fa) / (fb - fa)
        }
    }

    /// Inverse of [`fraction`](Self::fraction): the data value at scale-space
    /// fraction `t`.
    fn at_fraction(self, t: f64, scale: Scale) -> f64 {
        let (fa, fb) = self.forward(scale);
        scale.inverse(fa + t * (fb - fa))
    }
}

/// A plot's pan/zoom state: the visible window of each axis. Persisted per
/// node and readable with
/// [`UiState::plot_view`](crate::state::UiState::plot_view) — e.g. for the
/// virtual-data pull, where the app loads or resamples its source to cover
/// the window the user has scrolled into (see `docs/PLOT2D_PLAN.md`,
/// decision 5).
///
/// All projection is parameterised by the axis [`Scale`]s and the plot's
/// **data rect** (the inner area the marks draw into, excluding axes and
/// legend), so the same view projects correctly as the layout resizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotView {
    /// The visible window of the horizontal axis.
    pub x: AxisView,
    /// The visible window of the vertical axis.
    pub y: AxisView,
}

impl PlotView {
    /// A view with the given per-axis windows.
    pub fn new(x: AxisView, y: AxisView) -> Self {
        Self { x, y }
    }

    /// A view framing the data bounds `((x_min, x_max), (y_min, y_max))`
    /// with a fractional `pad` of headroom added to each axis (e.g. `0.05`
    /// for 5%). Degenerate (zero-width) bounds are nudged to a unit window so
    /// the view is always usable.
    pub fn fit(x: (f64, f64), y: (f64, f64), pad: f64) -> Self {
        Self {
            x: pad_axis(x, pad),
            y: pad_axis(y, pad),
        }
    }

    /// Project a data-space point to a pixel within `rect`. The vertical
    /// axis is screen-inverted (data grows upward, pixels grow downward).
    pub fn project(self, p: (f64, f64), x: Scale, y: Scale, rect: Rect) -> (f32, f32) {
        let fx = self.x.fraction(p.0, x);
        let fy = self.y.fraction(p.1, y);
        (
            rect.x + (fx as f32) * rect.w,
            rect.y + ((1.0 - fy) as f32) * rect.h,
        )
    }

    /// Inverse of [`project`](Self::project): map a pixel within `rect` back
    /// to a data-space point (e.g. for a crosshair readout).
    pub fn unproject(self, s: (f32, f32), x: Scale, y: Scale, rect: Rect) -> (f64, f64) {
        let tx = if rect.w != 0.0 {
            ((s.0 - rect.x) / rect.w) as f64
        } else {
            0.0
        };
        let ty = if rect.h != 0.0 {
            1.0 - ((s.1 - rect.y) / rect.h) as f64
        } else {
            0.0
        };
        (self.x.at_fraction(tx, x), self.y.at_fraction(ty, y))
    }

    /// Pan the view by a pixel delta within `rect` — the drag gesture.
    /// Dragging the content right (`dpx > 0`) reveals earlier data on the
    /// left; dragging it down (`dpy > 0`) reveals larger values at the top.
    pub fn pan_pixels(self, d: (f32, f32), x: Scale, y: Scale, rect: Rect) -> Self {
        let frac_x = if rect.w != 0.0 {
            -(d.0 as f64) / rect.w as f64
        } else {
            0.0
        };
        let frac_y = if rect.h != 0.0 {
            (d.1 as f64) / rect.h as f64
        } else {
            0.0
        };
        Self {
            x: self.x.panned(frac_x, x),
            y: self.y.panned(frac_y, y),
        }
    }

    /// Zoom about a pixel `anchor` within `rect`, keeping the data under the
    /// cursor fixed — the wheel/pinch gesture. `factor` is per-axis (`< 1.0`
    /// zooms in); pass equal factors for uniform zoom, or `1.0` on one axis
    /// to lock it.
    pub fn zoom_about(
        self,
        factor: (f64, f64),
        anchor: (f32, f32),
        x: Scale,
        y: Scale,
        rect: Rect,
    ) -> Self {
        let tx = if rect.w != 0.0 {
            ((anchor.0 - rect.x) / rect.w) as f64
        } else {
            0.5
        };
        let ty = if rect.h != 0.0 {
            1.0 - ((anchor.1 - rect.y) / rect.h) as f64
        } else {
            0.5
        };
        Self {
            x: self.x.zoomed(factor.0, tx, x),
            y: self.y.zoomed(factor.1, ty, y),
        }
    }

    /// Replace the vertical window — used by `Y::autoscale`, which refits the
    /// value axis to the data visible in the current horizontal window each
    /// frame.
    pub fn with_y(self, y: AxisView) -> Self {
        Self { y, ..self }
    }
}

/// Frame a `(min, max)` data span with fractional `pad` of headroom, nudging
/// a degenerate span to a unit window.
fn pad_axis((min, max): (f64, f64), pad: f64) -> AxisView {
    if !min.is_finite() || !max.is_finite() || max <= min {
        // Degenerate: center a unit window on the (finite) value.
        let c = if min.is_finite() { min } else { 0.0 };
        return AxisView::new(c - 0.5, c + 0.5);
    }
    let m = (max - min) * pad;
    AxisView::new(min - m, max + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };

    fn lin_view() -> PlotView {
        PlotView::new(AxisView::new(0.0, 100.0), AxisView::new(0.0, 50.0))
    }

    #[test]
    fn project_corners_and_center() {
        let v = lin_view();
        let (xs, ys) = (Scale::linear(), Scale::linear());
        // data (0,0) → bottom-left pixel
        assert_eq!(v.project((0.0, 0.0), xs, ys, RECT), (0.0, 100.0));
        // data (100,50) → top-right pixel
        assert_eq!(v.project((100.0, 50.0), xs, ys, RECT), (200.0, 0.0));
        // center
        assert_eq!(v.project((50.0, 25.0), xs, ys, RECT), (100.0, 50.0));
    }

    #[test]
    fn project_unproject_roundtrip() {
        let v = lin_view();
        let (xs, ys) = (Scale::linear(), Scale::linear());
        let p = (37.5, 12.25);
        let s = v.project(p, xs, ys, RECT);
        let back = v.unproject(s, xs, ys, RECT);
        assert!((back.0 - p.0).abs() < 1e-4, "x {back:?}");
        assert!((back.1 - p.1).abs() < 1e-4, "y {back:?}");
    }

    #[test]
    fn zoom_keeps_anchor_data_fixed() {
        let v = lin_view();
        let (xs, ys) = (Scale::linear(), Scale::linear());
        let anchor = (150.0, 25.0); // some pixel
        let before = v.unproject(anchor, xs, ys, RECT);
        let zoomed = v.zoom_about((0.5, 0.5), anchor, xs, ys, RECT);
        let after = zoomed.unproject(anchor, xs, ys, RECT);
        assert!((before.0 - after.0).abs() < 1e-6, "x {before:?} {after:?}");
        assert!((before.1 - after.1).abs() < 1e-6, "y {before:?} {after:?}");
        // zooming in shrinks the windows
        assert!(zoomed.x.max - zoomed.x.min < 100.0);
    }

    #[test]
    fn pan_shifts_window_opposite_to_drag_x() {
        let v = lin_view();
        let (xs, ys) = (Scale::linear(), Scale::linear());
        // drag content right by a full rect width → window moves left by its
        // full span
        let panned = v.pan_pixels((200.0, 0.0), xs, ys, RECT);
        assert!((panned.x.min - -100.0).abs() < 1e-6);
        assert!((panned.x.max - 0.0).abs() < 1e-6);
    }

    #[test]
    fn pan_down_reveals_higher_values_at_top() {
        let v = lin_view();
        let (xs, ys) = (Scale::linear(), Scale::linear());
        // drag content down by full height → window moves up by full span
        let panned = v.pan_pixels((0.0, 100.0), xs, ys, RECT);
        assert!((panned.y.min - 50.0).abs() < 1e-6);
        assert!((panned.y.max - 100.0).abs() < 1e-6);
    }

    #[test]
    fn log_zoom_anchor_fixed() {
        let v = PlotView::new(AxisView::new(1.0, 1000.0), AxisView::new(0.0, 1.0));
        let (xs, ys) = (Scale::log(), Scale::linear());
        let anchor = (50.0, 50.0);
        let before = v.unproject(anchor, xs, ys, RECT);
        let zoomed = v.zoom_about((0.5, 1.0), anchor, xs, ys, RECT);
        let after = zoomed.unproject(anchor, xs, ys, RECT);
        assert!((before.0 - after.0).abs() / before.0 < 1e-6, "{before:?} {after:?}");
    }

    #[test]
    fn fit_adds_padding_and_handles_degenerate() {
        let v = PlotView::fit((0.0, 100.0), (5.0, 5.0), 0.1);
        assert!((v.x.min - -10.0).abs() < 1e-9);
        assert!((v.x.max - 110.0).abs() < 1e-9);
        // degenerate y → unit window centered on 5.0
        assert_eq!(v.y, AxisView::new(4.5, 5.5));
    }
}
