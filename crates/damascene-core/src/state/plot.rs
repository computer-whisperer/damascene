//! Persistent pan/zoom state accessors and GC for
//! [`plot()`](crate::tree::plot) nodes — the plot counterpart of
//! [`viewport`](super::viewport). The per-node [`PlotView`] survives
//! rebuilds (keyed by `computed_id`, LRU-bounded); `draw_ops` seeds it by
//! auto-fitting the data on first show, and the gesture router mutates it.

use crate::plot::{AxisView, PlotView};
use crate::tree::{El, Rect};

use super::UiState;
use super::types::{PlotMetrics, PlotPanDrag, PlotState, PlotZoomDrag};

/// Maximum number of plot identities the persistent `views` map retains,
/// mirroring [`VIEWPORT_LRU_CAP`](super::viewport::VIEWPORT_LRU_CAP).
pub(crate) const PLOT_LRU_CAP: usize = 4096;

/// Wheel-notch zoom multiplier — one notch zooms in/out by this factor,
/// matching the viewport / 3D-camera feel.
const PLOT_WHEEL_STEP: f64 = 1.1;

impl UiState {
    /// Read the persisted [`PlotView`] for the plot keyed `id`
    /// (`computed_id`), or `None` if it has not been resolved yet (a plot is
    /// seeded on its first `draw_ops` pass).
    pub fn plot_view(&self, id: &str) -> Option<PlotView> {
        self.plot.views.get(id).copied()
    }

    /// Read a plot's [`PlotView`] by its `.key(...)` — the ergonomic path
    /// for app `build` / `on_event` code. This is the **virtual-data pull**
    /// hook: read the visible window each frame, and if it has drifted from
    /// what you last loaded, resample your source over the new range and
    /// `set` the series handle (see `docs/PLOT2D_PLAN.md`, decision 5).
    /// `None` when no laid-out node carries `key`, or it has not been
    /// resolved yet.
    pub fn plot_view_by_key(&self, key: &str) -> Option<PlotView> {
        let id = self.layout.key_index.get(key)?;
        self.plot.views.get(id.as_ref()).copied()
    }

    /// Seed or overwrite the [`PlotView`] for the plot keyed `id`. Lets an
    /// app pre-frame a plot (e.g. to a fixed time window) before the first
    /// resolve, or drive the view programmatically. From `build` /
    /// `on_event` code (which has no `&mut UiState`), push a
    /// [`PlotRequest`](crate::plot::PlotRequest) via
    /// [`App::drain_plot_requests`](crate::event::App::drain_plot_requests)
    /// instead.
    ///
    /// A seeded view is a deliberate framing: it takes manual X control so
    /// [`x_autoscale`](crate::plot::PlotSpec::x_autoscale) doesn't re-fit
    /// over it next frame. A double-click reset or
    /// [`PlotRequest::FitAll`](crate::plot::PlotRequest::FitAll) restores
    /// the tracking.
    pub fn set_plot_view(&mut self, id: impl Into<String>, view: PlotView) {
        let id = id.into();
        self.plot.x_manual.insert(id.clone());
        self.plot.views.insert(id, view);
    }

    /// Queue programmatic [`PlotRequest`](crate::plot::PlotRequest)s
    /// (fit-all, set-X-window). Each is consumed during
    /// `prepare_plots` by the plot whose `.key(...)`
    /// it names, where the live data bounds are known. Push once per
    /// frame; unmatched requests are dropped by
    /// [`Self::clear_pending_plot_requests`].
    pub fn push_plot_requests(&mut self, requests: Vec<crate::plot::PlotRequest>) {
        self.plot.pending_requests.extend(requests);
    }

    /// Drop any plot requests still queued after the prepare walk —
    /// requests targeting a plot that wasn't in the tree this frame don't
    /// fire against a later re-mount with the same key.
    pub fn clear_pending_plot_requests(&mut self) {
        self.plot.pending_requests.clear();
    }

    /// Store the resolved [`PlotView`] for `id` (called by `draw_ops` after
    /// auto-fit / Y-autoscale so the next frame and gestures see it).
    pub(crate) fn store_plot_view(&mut self, id: impl Into<String>, view: PlotView) {
        self.plot.views.insert(id.into(), view);
    }

    /// Record the resolved per-frame layout (data rect + scales) for plot
    /// `id`, so the gesture router and the by-key readback can unproject the
    /// cursor and report the window.
    pub(crate) fn store_plot_metrics(&mut self, id: impl Into<String>, metrics: PlotMetrics) {
        self.plot.metrics.insert(id.into(), metrics);
    }

    /// The last resolved metrics for plot `id`, if any.
    pub(crate) fn plot_metrics(&self, id: &str) -> Option<PlotMetrics> {
        self.plot.metrics.get(id).cloned()
    }

    /// Apply one consumed [`PlotRequest`](crate::plot::PlotRequest) to the
    /// plot keyed `id` (`computed_id`). `FitAll` drops the persisted view
    /// and both manual-axis overrides so the next resolve re-fits the data
    /// — the programmatic double-click. `SetXWindow` pins the horizontal
    /// window and takes manual X control (empty / non-finite windows are
    /// ignored). `lane_rect_h` is the lane plot's data-rect height (unused
    /// for plain plots): a `SetXWindow` landing before the first resolve
    /// seeds Y as the readable-lane initial stack view, not a data fit.
    fn apply_plot_request(
        &mut self,
        id: &str,
        spec: &crate::plot::PlotSpec,
        req: crate::plot::PlotRequest,
        lane_rect_h: f32,
    ) {
        match req {
            crate::plot::PlotRequest::FitAll { .. } => {
                self.plot.x_manual.remove(id);
                self.plot.y_manual.remove(id);
                self.plot.views.remove(id);
            }
            crate::plot::PlotRequest::SetXWindow { min, max, .. } => {
                if !min.is_finite() || !max.is_finite() || max <= min {
                    return;
                }
                self.plot.x_manual.insert(id.to_string());
                // Base Y on the current view (or, before the first resolve,
                // what that resolve would seed: the initial stack view for a
                // lane plot, a data fit otherwise); Y-autoscale refits a
                // plain plot's Y to the new window in the resolve that
                // follows unless the user holds it.
                let base = self.plot_view(id).unwrap_or_else(|| {
                    if spec.is_lane_plot() {
                        crate::plot::PlotView::new(
                            AxisView::new(min, max),
                            crate::plot::resolve::lane_stack_view(spec, None, lane_rect_h),
                        )
                    } else {
                        crate::plot::resolve::autofit(
                            crate::plot::resolve::data_bounds(spec),
                            spec.x.scale,
                            spec.y.scale,
                        )
                    }
                });
                self.plot
                    .views
                    .insert(id.to_string(), base.with_x(AxisView::new(min, max)));
            }
        }
    }

    /// Consume every pending [`PlotRequest`](crate::plot::PlotRequest)
    /// naming this plot's `key`, applying each before the frame's resolve.
    fn consume_plot_requests(
        &mut self,
        id: &str,
        key: Option<&str>,
        spec: &crate::plot::PlotSpec,
        lane_rect_h: f32,
    ) {
        let Some(key) = key else {
            return;
        };
        let mut i = 0;
        while i < self.plot.pending_requests.len() {
            if self.plot.pending_requests[i].key() == key {
                let req = self.plot.pending_requests.remove(i);
                self.apply_plot_request(id, spec, req, lane_rect_h);
            } else {
                i += 1;
            }
        }
    }

    /// Resolve every plot node's view + layout for this frame, **before**
    /// `draw_ops` reads them. For each [`plot()`](crate::tree::plot) node:
    /// seed the [`PlotView`] by auto-fitting the data on first show, refit
    /// the Y axis to the visible window when `y_autoscale` is on, and record
    /// the data rect + scales as metrics (for the gesture router and the
    /// by-key readback). Reads each node's laid-out
    /// [`computed_rect`](crate::tree::El::computed_rect), so it must run
    /// after layout. Walks the tree mutating `self.plot`; the `&El`
    /// borrow of `root` is independent of the `self.plot` writes.
    pub(crate) fn prepare_plots(&mut self, node: &El) {
        if let Some(spec) = &node.plot_source {
            let rect = node.computed_rect;
            let id = node.computed_id.clone();
            if spec.is_lane_plot() {
                self.prepare_lane_plot(spec, &id, node.key.as_deref(), rect);
            } else {
                // Consume programmatic requests naming this plot's key
                // before resolving, so the resolve below honors what they
                // set.
                self.consume_plot_requests(&id, node.key.as_deref(), spec, 0.0);
                // Effective autoscale: the spec's choice per axis, unless
                // the user has taken manual control of that axis (an X
                // gesture / seeded view for X, a Y box-zoom for Y) until a
                // double-click reset or `PlotRequest::FitAll`.
                let autoscale_x = spec.x_autoscale && !self.plot.x_manual.contains(&*id);
                let autoscale_y = spec.y_autoscale && !self.plot.y_manual.contains(&*id);
                let view = crate::plot::resolve::resolve_view(
                    spec,
                    self.plot_view(&id),
                    autoscale_x,
                    autoscale_y,
                );
                self.store_plot_view(id.to_string(), view);
                // Size the left gutter to the resolved view's Y labels so
                // wide values don't clip.
                let gutter = crate::plot::resolve::left_gutter(spec, &view);
                self.store_plot_metrics(
                    id.to_string(),
                    PlotMetrics {
                        data_rect: crate::plot::resolve::data_rect(rect, gutter),
                        x_scale: spec.x.scale,
                        y_scale: spec.y.scale,
                        crosshair: spec.crosshair,
                        controls: spec.controls,
                        lanes: None,
                    },
                );
            }
        }
        for c in &node.children {
            self.prepare_plots(c);
        }
    }

    /// The lane-plot arm of [`prepare_plots`](Self::prepare_plots). The
    /// gutter is label-sized (view-independent), so the data rect is known
    /// *before* the resolve — which needs its height for the readable-lane
    /// initial clamp. The vertical "scale" recorded for the gesture router
    /// is linear stack space; the spec's `y` axis is ignored on lane plots.
    fn prepare_lane_plot(
        &mut self,
        spec: &crate::plot::PlotSpec,
        id: &str,
        key: Option<&str>,
        rect: crate::tree::Rect,
    ) {
        use crate::plot::resolve;
        let gutter = resolve::lane_gutter(spec);
        let data_rect = resolve::data_rect(rect, gutter);
        self.consume_plot_requests(id, key, spec, data_rect.h);
        let autoscale_x = spec.x_autoscale && !self.plot.x_manual.contains(id);
        let view = resolve::resolve_lane_view(spec, self.plot_view(id), autoscale_x, data_rect.h);
        self.store_plot_view(id.to_string(), view);
        let gutter_rect = crate::tree::Rect::new(
            rect.x,
            data_rect.y,
            (data_rect.x - rect.x).max(0.0),
            data_rect.h,
        );
        self.store_plot_metrics(
            id.to_string(),
            PlotMetrics {
                data_rect,
                x_scale: spec.x.scale,
                y_scale: crate::plot::Scale::linear(),
                crosshair: spec.crosshair,
                controls: spec.controls,
                lanes: Some(super::types::PlotLaneMetrics {
                    bands: resolve::lane_bands(spec),
                    total: resolve::stack_total(spec),
                    snap: spec.lane_zoom_snap,
                    scroll: spec.stack_scroll,
                    gutter: gutter_rect,
                }),
            },
        );
    }

    /// Deepest plot whose data rect contains `(x, y)`, with its resolved
    /// metrics — the gesture router's entry point for pan / zoom. "Deepest"
    /// is the longest `computed_id` path, so a plot nested in another picks
    /// the inner one.
    pub(crate) fn plot_at(&self, x: f32, y: f32) -> Option<(String, PlotMetrics)> {
        let mut best: Option<(&String, &PlotMetrics)> = None;
        for (id, m) in &self.plot.metrics {
            if m.data_rect.contains(x, y) && best.as_ref().is_none_or(|(b, _)| id.len() > b.len()) {
                best = Some((id, m));
            }
        }
        best.map(|(id, m)| (id.clone(), m.clone()))
    }

    /// Whether `(x, y)` is over a plot that draws a crosshair — so the
    /// runtime can request a redraw on every hover-move, letting the
    /// crosshair track the cursor even when no hover identity changes (the
    /// plot analogue of `pointer_over_hover_scene`).
    pub(crate) fn pointer_over_crosshair_plot(&self, x: f32, y: f32) -> bool {
        self.plot
            .metrics
            .values()
            .any(|m| m.crosshair && m.data_rect.contains(x, y))
    }

    /// Begin a pan drag on the plot keyed `id`, anchoring on the current
    /// view so the data tracks the cursor 1:1. No-op if the plot has no
    /// resolved view yet.
    pub(crate) fn begin_plot_pan(&mut self, id: String, x: f32, y: f32) {
        let Some(view) = self.plot_view(&id) else {
            return;
        };
        self.plot.pan_drag = Some(PlotPanDrag {
            plot_id: id,
            start_pointer: (x, y),
            start_view: view,
        });
    }

    /// True while a plot pan drag is in flight.
    pub(crate) fn plot_pan_active(&self) -> bool {
        self.plot.pan_drag.is_some()
    }

    /// Update the active pan drag to the current cursor — the live view is
    /// the start view panned by the absolute cursor delta. When `y_autoscale`
    /// is on, the next `prepare_plots` re-fits the Y window, so effectively
    /// only the X (time) axis pans. Returns whether the view moved.
    pub(crate) fn drag_plot_to(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = self.plot.pan_drag.clone() else {
            return false;
        };
        let Some(m) = self.plot_metrics(&drag.plot_id) else {
            return false;
        };
        let delta = (x - drag.start_pointer.0, y - drag.start_pointer.1);
        let next = drag
            .start_view
            .pan_pixels(delta, m.x_scale, m.y_scale, m.data_rect);
        let moved = next != drag.start_view;
        if next.x != drag.start_view.x {
            // A pan that moved the *time axis* takes manual X control —
            // without this, `x_autoscale` would snap the window back next
            // frame. A purely vertical pan doesn't count: Y-autoscale
            // absorbs it, and it must not silently freeze a streaming
            // plot's tail-follow.
            self.plot.x_manual.insert(drag.plot_id.clone());
        }
        self.store_plot_view(&drag.plot_id, next);
        moved
    }

    /// End any active plot pan drag. Returns whether one was in flight.
    pub(crate) fn end_plot_pan(&mut self) -> bool {
        self.plot.pan_drag.take().is_some()
    }

    /// Abandon both plot gestures — the platform cancelled the pointer
    /// sequence. The pan stays where it dragged to (it applies
    /// incrementally); the box-zoom selection is *discarded*, never
    /// applied (unlike [`Self::end_plot_zoom`], which zooms on release).
    pub(crate) fn cancel_plot_gestures(&mut self) {
        self.plot.pan_drag = None;
        self.plot.zoom_drag = None;
    }

    /// Begin a directional box-zoom selection on the plot keyed `id` — the
    /// scientific click-drag-to-zoom gesture. No-op if the plot has no
    /// resolved view yet.
    pub(crate) fn begin_plot_zoom(&mut self, id: String, x: f32, y: f32) {
        if self.plot_view(&id).is_none() {
            return;
        }
        self.plot.zoom_drag = Some(PlotZoomDrag {
            plot_id: id,
            start_pointer: (x, y),
            cur_pointer: (x, y),
        });
    }

    /// True while a box-zoom selection is in flight.
    pub(crate) fn plot_zoom_active(&self) -> bool {
        self.plot.zoom_drag.is_some()
    }

    /// Track the cursor for the active box-zoom selection. Returns whether the
    /// selection rectangle moved (so the band overlay redraws).
    pub(crate) fn drag_plot_zoom_to(&mut self, x: f32, y: f32) -> bool {
        if let Some(d) = self.plot.zoom_drag.as_mut() {
            let moved = d.cur_pointer != (x, y);
            d.cur_pointer = (x, y);
            moved
        } else {
            false
        }
    }

    /// The selection band to highlight for the active box-zoom drag on plot
    /// `id`, or `None` when no selection is in flight, it is on another plot,
    /// or the swept extent is still sub-threshold (so a click doesn't flash a
    /// band). A `ZoomAxis::X` selection spans the full data-rect height; a
    /// `ZoomAxis::Y` selection spans the full width.
    pub(crate) fn plot_zoom_band(&self, id: &str) -> Option<Rect> {
        let d = self.plot.zoom_drag.as_ref()?;
        if d.plot_id != id {
            return None;
        }
        let m = self.plot_metrics(id)?;
        let axis = zoom_axis(d.start_pointer, d.cur_pointer);
        if axis_extent(axis, d.start_pointer, d.cur_pointer) < MIN_ZOOM_PX {
            return None;
        }
        Some(band_rect(axis, d.start_pointer, d.cur_pointer, m.data_rect))
    }

    /// End the active box-zoom selection, applying the zoom on release. A drag
    /// shorter than [`MIN_ZOOM_PX`] along the chosen axis is treated as a click
    /// (no zoom). Returns whether a selection was in flight.
    pub(crate) fn end_plot_zoom(&mut self) -> bool {
        let Some(drag) = self.plot.zoom_drag.take() else {
            return false;
        };
        let (Some(m), Some(view)) = (
            self.plot_metrics(&drag.plot_id),
            self.plot_view(&drag.plot_id),
        ) else {
            return true;
        };
        let axis = zoom_axis(drag.start_pointer, drag.cur_pointer);
        if axis_extent(axis, drag.start_pointer, drag.cur_pointer) < MIN_ZOOM_PX {
            return true; // a click, not a zoom
        }
        // Unproject both ends of the drag to data space and frame the span on
        // the selected axis, leaving the other axis untouched (Y-autoscale
        // refits it next frame for an X zoom).
        let a = view.unproject(drag.start_pointer, m.x_scale, m.y_scale, m.data_rect);
        let b = view.unproject(drag.cur_pointer, m.x_scale, m.y_scale, m.data_rect);
        let next = match axis {
            ZoomAxis::X => {
                // Framing an X span takes manual control of the time axis
                // (mirroring the Y arm below). Cleared by `reset_plot_view`.
                self.plot.x_manual.insert(drag.plot_id.clone());
                PlotView::new(AxisView::new(a.0.min(b.0), a.0.max(b.0)), view.y)
            }
            ZoomAxis::Y => {
                // Zooming the value axis takes manual Y control — otherwise
                // `y_autoscale` would refit it away on the next frame. Cleared
                // by `reset_plot_view` (double-click). A lane plot has no
                // stack-level autoscale to hold off, so it skips the flag —
                // a stale entry would misapply if a keyed plot's spec later
                // morphed from lanes back to marks.
                if m.lanes.is_none() {
                    self.plot.y_manual.insert(drag.plot_id.clone());
                }
                let (mut lo, mut hi) = (a.1.min(b.1), a.1.max(b.1));
                if let Some(l) = &m.lanes {
                    // Lane plot: optionally snap outward to lane boundaries
                    // (select whole lanes), and keep the window inside the
                    // stack either way.
                    if l.snap {
                        (lo, hi) = crate::plot::resolve::snap_to_lane_bounds((lo, hi), &l.bands);
                    }
                    let c =
                        crate::plot::resolve::clamp_stack_window(AxisView::new(lo, hi), l.total);
                    (lo, hi) = (c.min, c.max);
                }
                PlotView::new(view.x, AxisView::new(lo, hi))
            }
        };
        self.store_plot_view(&drag.plot_id, next);
        true
    }

    /// Reset the plot keyed `id` to its full data extent — the double-click
    /// gesture. Drops the persisted view so the next `prepare_plots` re-fits
    /// the data (and cancels any in-flight selection). Returns whether a
    /// persisted view was cleared.
    pub(crate) fn reset_plot_view(&mut self, id: &str) -> bool {
        self.plot.zoom_drag = None;
        // Restore per-axis autoscale: a reset returns to the data-driven
        // framing on both axes.
        self.plot.x_manual.remove(id);
        self.plot.y_manual.remove(id);
        self.plot.views.remove(id).is_some()
    }

    /// Apply one incremental pinch step to the plot keyed `id`: per-axis
    /// window factors about `anchor` (same convention as
    /// [`PlotView::zoom_about`](crate::plot::PlotView::zoom_about): `< 1.0`
    /// zooms in, `1.0` locks the axis) plus a centroid pan in pixels (same
    /// content-follows-finger convention as [`Self::drag_plot_to`]).
    /// Manual-control flags mirror the existing gestures: any X change
    /// takes `x_manual` (like pan / wheel); a Y *zoom* takes `y_manual`
    /// (like a Y box-zoom, skipped for lane plots), while a purely
    /// vertical pan component stays autoscale-absorbed. Lane plots clamp
    /// the resulting Y window to the stack. Returns whether the view
    /// changed.
    pub(crate) fn plot_pinch_step(
        &mut self,
        id: &str,
        factors: (f64, f64),
        anchor: (f32, f32),
        pan_px: (f32, f32),
    ) -> bool {
        let Some(m) = self.plot_metrics(id) else {
            return false;
        };
        let Some(view) = self.plot_view(id) else {
            return false;
        };
        let mut next = view
            .pan_pixels(pan_px, m.x_scale, m.y_scale, m.data_rect)
            .zoom_about(factors, anchor, m.x_scale, m.y_scale, m.data_rect);
        if let Some(l) = &m.lanes {
            next = next.with_y(crate::plot::resolve::clamp_stack_window(next.y, l.total));
        }
        let moved = next != view;
        if next.x != view.x {
            self.plot.x_manual.insert(id.to_string());
        }
        if (factors.1 - 1.0).abs() > f64::EPSILON && next.y != view.y && m.lanes.is_none() {
            self.plot.y_manual.insert(id.to_string());
        }
        self.store_plot_view(id.to_string(), next);
        moved
    }

    /// Zoom the plot under `(x, y)` by one wheel notch, anchored so the data
    /// under the cursor stays fixed. `dy > 0` (Damascene wheel convention)
    /// zooms out. Returns `true` when a plot consumed the wheel (so it
    /// doesn't also scroll an enclosing container). The wheel zooms the **X
    /// (time) axis only** — the common time-series gesture; the value axis is
    /// left to `y_autoscale` (or a Y box-zoom). Use a box-zoom to scale Y.
    pub(crate) fn plot_wheel_zoom(&mut self, root: &El, x: f32, y: f32, dy: f32) -> bool {
        if dy.abs() <= f32::EPSILON {
            return false;
        }
        // A lane plot's label gutter takes the wheel as stack scroll (when
        // configured) — it sits outside the data rect, so check it first.
        if self.lane_gutter_scroll_at(root, x, y, dy) {
            return true;
        }
        let Some((id, m)) = self.plot_at(x, y) else {
            return false;
        };
        // An overlay floated over the plot takes the wheel as scroll, not
        // zoom — yield to scroll routing.
        if crate::hit_test::occluded_by_overlay(root, (x, y), &id) {
            return false;
        }
        // Shift+wheel over a lane plot scrolls the stack (when configured)
        // instead of zooming the time axis. A stack that already fits
        // entirely can't scroll, so the gesture falls through to the plain
        // X zoom rather than dead-ending.
        if let Some(l) = &m.lanes
            && l.scroll.shift_wheel
            && self.modifiers.shift
            && self.lane_stack_scrollable(&id, l.total)
        {
            self.scroll_lane_stack(&id, &m, dy);
            return true;
        }
        let Some(view) = self.plot_view(&id) else {
            return false;
        };
        // `factor` multiplies the window *width*, so the mapping is the
        // inverse of the viewport's zoom-multiplier: zoom in (dy < 0) shrinks
        // the window (factor < 1); zoom out (dy > 0) grows it. `1.0` on Y
        // locks the value axis.
        let factor = if dy > 0.0 {
            PLOT_WHEEL_STEP
        } else {
            1.0 / PLOT_WHEEL_STEP
        };
        let next = view.zoom_about((factor, 1.0), (x, y), m.x_scale, m.y_scale, m.data_rect);
        // Wheel-zooming the time axis takes manual X control, like a pan.
        self.plot.x_manual.insert(id.clone());
        self.store_plot_view(&id, next);
        true
    }

    /// Scroll a lane plot's stack window by a wheel delta. `dy` is
    /// pixel-denominated (hosts feed pixel deltas, matching scroll
    /// containers): wheel-down (`dy > 0`) reveals lower lanes, clamped to
    /// the stack ends. Returns whether the window moved.
    fn scroll_lane_stack(&mut self, id: &str, m: &PlotMetrics, dy: f32) -> bool {
        let Some(l) = &m.lanes else {
            return false;
        };
        let Some(view) = self.plot_view(id) else {
            return false;
        };
        let panned = view.pan_pixels((0.0, -dy), m.x_scale, m.y_scale, m.data_rect);
        let next_y = crate::plot::resolve::clamp_stack_window(panned.y, l.total);
        let moved = next_y != view.y;
        self.store_plot_view(id.to_string(), view.with_y(next_y));
        moved
    }

    /// Whether a lane plot's stack window can scroll at all — its span is
    /// narrower than the stack. A fully-fitting stack has nothing to
    /// scroll, so wheel gestures should fall through instead of
    /// dead-ending (a permanently inert gutter inside a scrollable page).
    fn lane_stack_scrollable(&self, id: &str, total: f64) -> bool {
        self.plot_view(id)
            .is_some_and(|v| (v.y.max - v.y.min).abs() < total)
    }

    /// Route a wheel over a lane plot's label gutter to stack scroll (the
    /// deepest gutter under the cursor, when its spec opted in and the
    /// stack actually scrolls). While scrollable, the gesture is consumed
    /// even at the stack ends, like the wheel over the data rect — a plot
    /// never half-yields the wheel mid-interaction.
    fn lane_gutter_scroll_at(&mut self, root: &El, x: f32, y: f32, dy: f32) -> bool {
        let mut best: Option<(&String, &PlotMetrics)> = None;
        for (id, m) in &self.plot.metrics {
            if m.lanes
                .as_ref()
                .is_some_and(|l| l.scroll.gutter_wheel && l.gutter.contains(x, y))
                && best.as_ref().is_none_or(|(b, _)| id.len() > b.len())
            {
                best = Some((id, m));
            }
        }
        let Some((id, m)) = best.map(|(id, m)| (id.clone(), m.clone())) else {
            return false;
        };
        let scrollable = m
            .lanes
            .as_ref()
            .is_some_and(|l| self.lane_stack_scrollable(&id, l.total));
        if !scrollable || crate::hit_test::occluded_by_overlay(root, (x, y), &id) {
            return false;
        }
        self.scroll_lane_stack(&id, &m, dy);
        true
    }

    /// Bound the persistent plot `views` map (LRU over absent identities),
    /// the counterpart of [`Self::gc_viewport_state`]. Called once per frame
    /// from `RunnerCore::prepare_layout`.
    /// The map-side half of the plot GC, driven by the fused
    /// single-walk GC (`gc_side_maps`).
    pub(crate) fn gc_plot_with_live(&mut self, live: &rustc_hash::FxHashSet<&str>) {
        // Per-frame metrics are scratch: only keep live plots' entries.
        self.plot.metrics.retain(|id, _| live.contains(id.as_str()));
        self.plot.gc(live);
        // Manual-Y overrides follow the persistent views' (LRU) lifetime, so
        // they survive a keyed plot briefly leaving the tree.
        let views = &self.plot.views;
        self.plot.y_manual.retain(|id| views.contains_key(id));
        self.plot.x_manual.retain(|id| views.contains_key(id));
    }
}

/// Minimum drag extent (logical px) along the selected axis for a box-zoom to
/// register; a shorter drag is a click (and double-clicks reset the view).
const MIN_ZOOM_PX: f32 = 4.0;

/// Which axis a directional box-zoom selection acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ZoomAxis {
    X,
    Y,
}

/// Choose the box-zoom axis from the drag delta: the value (Y) axis when the
/// vertical delta dominates, else the X (time) axis. Matches InfluxDB's "X or Y
/// by the larger drag delta" selection. A Y selection opts the plot out of
/// `y_autoscale` on commit (see [`UiState::end_plot_zoom`]).
fn zoom_axis(start: (f32, f32), cur: (f32, f32)) -> ZoomAxis {
    let dx = (cur.0 - start.0).abs();
    let dy = (cur.1 - start.1).abs();
    if dy > dx { ZoomAxis::Y } else { ZoomAxis::X }
}

/// The swept pixel extent of a drag along `axis`.
fn axis_extent(axis: ZoomAxis, start: (f32, f32), cur: (f32, f32)) -> f32 {
    match axis {
        ZoomAxis::X => (cur.0 - start.0).abs(),
        ZoomAxis::Y => (cur.1 - start.1).abs(),
    }
}

/// The selection-band rectangle for a drag along `axis`, clamped to `data`: a
/// full-height vertical band for an X selection, a full-width horizontal band
/// for a Y selection.
fn band_rect(axis: ZoomAxis, start: (f32, f32), cur: (f32, f32), data: Rect) -> Rect {
    match axis {
        ZoomAxis::X => {
            let lo = start.0.min(cur.0).clamp(data.x, data.x + data.w);
            let hi = start.0.max(cur.0).clamp(data.x, data.x + data.w);
            Rect::new(lo, data.y, (hi - lo).max(0.0), data.h)
        }
        ZoomAxis::Y => {
            let lo = start.1.min(cur.1).clamp(data.y, data.y + data.h);
            let hi = start.1.max(cur.1).clamp(data.y, data.y + data.h);
            Rect::new(data.x, lo, data.w, (hi - lo).max(0.0))
        }
    }
}

impl PlotState {
    /// LRU pass over the persistent `views` map — same policy as
    /// [`ViewportState::gc`](super::types::ViewportState): live identities
    /// are stamped fresh and never evicted; once `views` exceeds
    /// [`PLOT_LRU_CAP`], the longest-unseen absent identities are dropped.
    pub(crate) fn gc(&mut self, live: &rustc_hash::FxHashSet<&str>) {
        self.frame += 1;
        let frame = self.frame;

        let mut stamp: Vec<String> = Vec::new();
        for id in self.views.keys() {
            if live.contains(id.as_str()) || !self.last_seen.contains_key(id) {
                stamp.push(id.clone());
            }
        }
        for id in stamp {
            self.last_seen.insert(id, frame);
        }

        let views = &self.views;
        self.last_seen.retain(|id, _| views.contains_key(id));

        if self.last_seen.len() <= PLOT_LRU_CAP {
            return;
        }

        let mut absent: Vec<(u64, String)> = self
            .last_seen
            .iter()
            .filter(|(id, _)| !live.contains(id.as_str()))
            .map(|(id, f)| (*f, id.clone()))
            .collect();
        absent.sort_unstable_by(|a, b| (a.0, a.1.as_str()).cmp(&(b.0, b.1.as_str())));
        let overflow = self.last_seen.len() - PLOT_LRU_CAP;
        for (_, id) in absent.into_iter().take(overflow) {
            self.views.remove(&id);
            self.last_seen.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::plot::{PlotSpec, Sample, Scale, SeriesHandle, line};
    use crate::tree::Rect;
    use crate::tree::plot as plot_widget;

    fn setup() -> (El, UiState) {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::linear())
            .add_mark(line(&h));
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        (tree, state)
    }

    #[test]
    fn wheel_zoom_in_shrinks_the_window() {
        let (tree, mut state) = setup();
        let before = state.plot_view_by_key("p").expect("view");
        // dy < 0 zooms in, anchored at the data-rect centre.
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -1.0));
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            (after.x.max - after.x.min) < (before.x.max - before.x.min),
            "zoom in shrinks x window: {:?} -> {:?}",
            before.x,
            after.x
        );
    }

    #[test]
    fn wheel_zoom_leaves_y_untouched() {
        // The wheel zooms the time (X) axis only; Y is left to autoscale.
        let (tree, mut state) = setup();
        let before = state.plot_view_by_key("p").expect("view");
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -1.0));
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            (after.x.max - after.x.min) < (before.x.max - before.x.min),
            "x zooms"
        );
        assert_eq!(after.y, before.y, "y untouched by the wheel");
    }

    #[test]
    fn pan_drag_shifts_the_window_and_releases() {
        let (_tree, mut state) = setup();
        let before = state.plot_view_by_key("p").expect("view");
        let (id, _) = state.plot_at(200.0, 150.0).expect("plot under cursor");
        state.begin_plot_pan(id, 200.0, 150.0);
        assert!(state.plot_pan_active());
        // Drag the content left → the window moves toward larger x.
        assert!(state.drag_plot_to(150.0, 150.0));
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            after.x.min > before.x.min,
            "{:?} -> {:?}",
            before.x,
            after.x
        );
        assert!(state.end_plot_pan());
        assert!(!state.plot_pan_active());
    }

    #[test]
    fn plot_at_finds_the_data_rect() {
        let (_tree, state) = setup();
        // Centre is inside the data rect; far corner (in the gutter) is not.
        assert!(state.plot_at(200.0, 150.0).is_some());
        assert!(state.plot_at(2.0, 2.0).is_none());
    }

    #[test]
    fn crosshair_plot_requests_redraw_on_hover() {
        // A plot *with* a crosshair flags hover-moves for redraw so it
        // tracks the cursor; one without does not.
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let with = PlotSpec::new().add_mark(line(&h)).crosshair(true);
        let mut tree = plot_widget(with).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        assert!(state.pointer_over_crosshair_plot(200.0, 150.0));
        assert!(!state.pointer_over_crosshair_plot(2.0, 2.0)); // in the gutter

        let without = PlotSpec::new().add_mark(line(&h));
        let mut tree2 = plot_widget(without).key("q");
        let mut state2 = UiState::new();
        layout(&mut tree2, &mut state2, Rect::new(0.0, 0.0, 400.0, 300.0));
        state2.prepare_plots(&tree2);
        assert!(!state2.pointer_over_crosshair_plot(200.0, 150.0));
    }

    /// A plot with Y autoscale off, so the Y axis is gesture-navigable.
    fn setup_manual_y() -> (El, UiState) {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::linear())
            .add_mark(line(&h))
            .y_autoscale(false);
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        (tree, state)
    }

    #[test]
    fn box_zoom_x_narrows_to_the_selection() {
        let (_tree, mut state) = setup();
        let before = state.plot_view_by_key("p").expect("view");
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        state.begin_plot_zoom(id.clone(), 100.0, 150.0);
        assert!(state.plot_zoom_active());
        assert!(state.drag_plot_zoom_to(250.0, 150.0));
        // Past the threshold a selection band is shown.
        assert!(state.plot_zoom_band(&id).is_some());
        assert!(state.end_plot_zoom());
        assert!(!state.plot_zoom_active());

        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            (after.x.max - after.x.min) < (before.x.max - before.x.min),
            "x window narrows: {:?} -> {:?}",
            before.x,
            after.x
        );
        assert!(after.x.min > before.x.min && after.x.max < before.x.max);
        // An X zoom leaves the Y window untouched.
        assert_eq!(after.y, before.y);
    }

    #[test]
    fn vertical_drag_zooms_y_and_takes_manual_control() {
        // With Y autoscaling on (the default), a vertical-dominant box-zoom
        // still zooms Y — by opting the plot out of autoscale, so the refit
        // doesn't erase it on the next frame.
        let (tree, mut state) = setup();
        let full = state.plot_view_by_key("p").expect("view");
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        state.begin_plot_zoom(id.clone(), 200.0, 60.0);
        state.drag_plot_zoom_to(206.0, 220.0); // dy >> dx
        state.end_plot_zoom();
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            (after.y.max - after.y.min) < (full.y.max - full.y.min),
            "y window narrows: {:?} -> {:?}",
            full.y,
            after.y
        );
        assert_eq!(after.x, full.x, "x untouched by a Y zoom");
        // The manual Y window survives the next resolve (autoscale opted out).
        state.prepare_plots(&tree);
        let resolved = state.plot_view_by_key("p").expect("view");
        assert_eq!(resolved.y, after.y, "manual Y is not refit away");

        // Double-click reset restores autoscale (and the full extent).
        assert!(state.reset_plot_view(&id));
        state.prepare_plots(&tree);
        let reset = state.plot_view_by_key("p").expect("view");
        assert_eq!(reset.y, full.y, "reset re-autoscales Y");
    }

    #[test]
    fn box_zoom_y_when_axis_is_navigable() {
        let (_tree, mut state) = setup_manual_y();
        let before = state.plot_view_by_key("p").expect("view");
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        // A vertical-dominant drag now selects the Y axis.
        state.begin_plot_zoom(id, 200.0, 60.0);
        state.drag_plot_zoom_to(206.0, 220.0);
        state.end_plot_zoom();
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            (after.y.max - after.y.min) < (before.y.max - before.y.min),
            "y window narrows: {:?} -> {:?}",
            before.y,
            after.y
        );
        // A Y zoom leaves the X window untouched.
        assert_eq!(after.x, before.x);
    }

    #[test]
    fn subthreshold_drag_is_a_click_not_a_zoom() {
        let (_tree, mut state) = setup();
        let before = state.plot_view_by_key("p").expect("view");
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        state.begin_plot_zoom(id.clone(), 200.0, 150.0);
        state.drag_plot_zoom_to(202.0, 151.0); // under MIN_ZOOM_PX
        assert!(
            state.plot_zoom_band(&id).is_none(),
            "no band below threshold"
        );
        assert!(state.end_plot_zoom());
        let after = state.plot_view_by_key("p").expect("view");
        assert_eq!(after.x, before.x);
        assert_eq!(after.y, before.y);
    }

    #[test]
    fn reset_refits_to_full_extent() {
        let (tree, mut state) = setup();
        let full = state.plot_view_by_key("p").expect("view");
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        // Zoom into a narrow window first.
        state.begin_plot_zoom(id.clone(), 100.0, 150.0);
        state.drag_plot_zoom_to(160.0, 150.0);
        state.end_plot_zoom();
        let zoomed = state.plot_view_by_key("p").expect("view");
        assert!((zoomed.x.max - zoomed.x.min) < (full.x.max - full.x.min));

        // Reset drops the persisted view; the next prepare re-fits the data.
        assert!(state.reset_plot_view(&id));
        state.prepare_plots(&tree);
        let after = state.plot_view_by_key("p").expect("view");
        assert_eq!(after.x, full.x);
    }

    // ---- lanes (M6) ----

    fn lane_setup(n: usize) -> (El, UiState) {
        use crate::plot::Lane;
        let mut spec = PlotSpec::new().x(Scale::linear());
        for i in 0..n {
            let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 1.0)]);
            spec = spec.lane(Lane::digital(format!("ch{i}"), &h));
        }
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        (tree, state)
    }

    /// The prepare pass seeds a lane plot with the readable-lane clamp:
    /// anchored at the top, min_lane_px per unit lane, never fit-all
    /// slivers.
    #[test]
    fn lane_plot_prepare_seeds_clamped_stack_view() {
        let (_tree, state) = lane_setup(100);
        let view = state.plot_view_by_key("p").expect("view");
        assert_eq!(view.y.max, 100.0, "anchored at the top");
        // data_rect.h = 300 − 10 − 28 = 262; 262 / 24 ≈ 10.9 unit lanes.
        let span = view.y.max - view.y.min;
        assert!((span - 262.0 / 24.0).abs() < 1e-6, "span {span}");
        // The metrics carry the lane geometry for the gesture router.
        let (_, m) = state.plot_at(200.0, 150.0).expect("plot under cursor");
        let lanes = m.lanes.expect("lane metrics");
        assert_eq!(lanes.bands.len(), 100);
        assert_eq!(lanes.total, 100.0);
        assert!(lanes.gutter.w > 0.0 && lanes.gutter.x < m.data_rect.x);
        // A short stack fits entirely.
        let (_tree, state) = lane_setup(4);
        let view = state.plot_view_by_key("p").expect("view");
        assert_eq!((view.y.min, view.y.max), (0.0, 4.0));
    }

    /// A `SetXWindow` request landing before the first resolve seeds the
    /// lane plot's Y as the readable initial stack view — not a data-bounds
    /// fit that would pin 100 lanes into view forever (the la-web
    /// live-follow path fires within the first second).
    #[test]
    fn lane_plot_set_x_window_before_first_frame_seeds_stack_y() {
        use crate::plot::Lane;
        let mut spec = PlotSpec::new().x(Scale::linear());
        for i in 0..100 {
            let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 1.0)]);
            spec = spec.lane(Lane::digital(format!("ch{i}"), &h));
        }
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        // Request consumed by the very first prepare.
        state.push_plot_requests(vec![crate::plot::PlotRequest::SetXWindow {
            key: "p".into(),
            min: 2.0,
            max: 4.0,
        }]);
        state.prepare_plots(&tree);
        let view = state.plot_view_by_key("p").expect("view");
        assert_eq!((view.x.min, view.x.max), (2.0, 4.0));
        assert_eq!(view.y.max, 100.0, "top-anchored stack window");
        assert!(
            view.y.max - view.y.min < 15.0,
            "readable clamp, not a data fit: {:?}",
            view.y
        );
    }

    /// la-web repro: keyed lane plot, series initially EMPTY, seed
    /// SetXWindow at epoch scale, then data arrives; the seeded X must hold.
    #[test]
    fn lane_plot_epoch_seed_survives_data_arrival() {
        use crate::plot::Lane;
        let to = 1.784436e9_f64;
        let hs: Vec<SeriesHandle> = (0..23).map(|_| SeriesHandle::new(Vec::new())).collect();
        let build = |hs: &[SeriesHandle]| {
            let mut spec = PlotSpec::new().x(Scale::time()).crosshair(true);
            for (i, h) in hs.iter().enumerate() {
                spec = spec.lane(Lane::digital(format!("GP{i}"), h));
            }
            plot_widget(spec).key("wave")
        };
        let mut state = UiState::new();
        // Frame 1: empty data, no requests yet.
        let mut tree = build(&hs);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 1600.0, 1000.0));
        state.prepare_plots(&tree);
        let v1 = state.plot_view_by_key("wave").expect("view");
        // Seed (the app's first /api/status): last 10 s.
        state.push_plot_requests(vec![crate::plot::PlotRequest::SetXWindow {
            key: "wave".into(),
            min: to - 10.0,
            max: to,
        }]);
        // Frame 2: rebuilt tree (the app rebuilds the spec every frame).
        let mut tree = build(&hs);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 1600.0, 1000.0));
        state.prepare_plots(&tree);
        let v2 = state.plot_view_by_key("wave").expect("view");
        assert_eq!(
            (v2.x.min, v2.x.max),
            (to - 10.0, to),
            "seed applied (was {:?})",
            v1.x
        );
        // Data arrives for the seeded window.
        for h in &hs {
            h.set(vec![
                Sample::new(to - 10.0, 0.0),
                Sample::new(to - 5.0, 1.0),
                Sample::new(to, 0.0),
            ]);
        }
        // Frame 3: rebuilt tree again; the manual X hold must survive.
        let mut tree = build(&hs);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 1600.0, 1000.0));
        state.prepare_plots(&tree);
        let v3 = state.plot_view_by_key("wave").expect("view");
        assert_eq!(
            (v3.x.min, v3.x.max),
            (to - 10.0, to),
            "manual X held after data change"
        );
    }

    /// A vertical pan on a lane plot is a stack scroll: it persists (no
    /// stack-level Y autoscale erases it) and clamps to the stack ends.
    #[test]
    fn lane_plot_vertical_pan_scrolls_and_clamps() {
        let (tree, mut state) = lane_setup(100);
        let before = state.plot_view_by_key("p").expect("view");
        let (id, _) = state.plot_at(200.0, 150.0).expect("plot");
        state.begin_plot_pan(id, 200.0, 150.0);
        // Drag the content up: lower (later-declared) lanes come into view.
        assert!(state.drag_plot_to(200.0, 50.0));
        state.end_plot_pan();
        state.prepare_plots(&tree);
        let after = state.plot_view_by_key("p").expect("view");
        assert!(after.y.max < before.y.max, "scrolled down the stack");
        assert!(
            (after.y.max - after.y.min) - (before.y.max - before.y.min) < 1e-9,
            "span preserved"
        );
        // A huge pan clamps at the bottom rather than scrolling into void.
        let (id, _) = state.plot_at(200.0, 150.0).expect("plot");
        state.begin_plot_pan(id, 200.0, 150.0);
        state.drag_plot_to(200.0, -100_000.0);
        state.end_plot_pan();
        state.prepare_plots(&tree);
        let clamped = state.plot_view_by_key("p").expect("view");
        assert_eq!(clamped.y.min, 0.0, "clamped at the stack bottom");
    }

    /// Shift+wheel over the data rect scrolls the stack (X untouched);
    /// without Shift the wheel stays an X zoom (stack untouched).
    #[test]
    fn lane_plot_shift_wheel_scrolls_the_stack() {
        let (tree, mut state) = lane_setup(100);
        let before = state.plot_view_by_key("p").expect("view");
        // Plain wheel: X zoom, stack untouched.
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -1.0));
        let zoomed = state.plot_view_by_key("p").expect("view");
        assert!(zoomed.x.max - zoomed.x.min < before.x.max - before.x.min);
        assert_eq!(zoomed.y, before.y, "stack untouched by a plain wheel");
        // Shift+wheel-down: the stack scrolls toward lower lanes, X holds.
        state.modifiers.shift = true;
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, 48.0));
        let scrolled = state.plot_view_by_key("p").expect("view");
        assert_eq!(scrolled.x, zoomed.x, "x untouched by a stack scroll");
        assert!(
            scrolled.y.max < zoomed.y.max,
            "wheel-down reveals lower lanes: {:?} -> {:?}",
            zoomed.y,
            scrolled.y
        );
        // Scrolling hard the other way clamps at the top.
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -100_000.0));
        let top = state.plot_view_by_key("p").expect("view");
        assert_eq!(top.y.max, 100.0, "clamped at the stack top");
    }

    /// The wheel over the lane-label gutter scrolls the stack without any
    /// modifier; both gestures obey the spec's `stack_scroll` opt-outs.
    #[test]
    fn lane_plot_gutter_wheel_and_config_opt_outs() {
        let (tree, mut state) = lane_setup(100);
        let before = state.plot_view_by_key("p").expect("view");
        // (20, 150) is inside the label gutter (left of the data rect).
        assert!(state.plot_wheel_zoom(&tree, 20.0, 150.0, 48.0));
        let scrolled = state.plot_view_by_key("p").expect("view");
        assert!(scrolled.y.max < before.y.max, "gutter wheel scrolls");
        assert_eq!(scrolled.x, before.x);

        // All stack-scroll gestures disabled: the gutter wheel is not
        // consumed (falls through to scroll routing), and Shift+wheel
        // behaves as a plain X zoom.
        use crate::plot::{Lane, StackScroll};
        let mut spec = PlotSpec::new()
            .x(Scale::linear())
            .stack_scroll(StackScroll {
                shift_wheel: false,
                gutter_wheel: false,
            });
        for i in 0..100 {
            let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 1.0)]);
            spec = spec.lane(Lane::digital(format!("ch{i}"), &h));
        }
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        assert!(
            !state.plot_wheel_zoom(&tree, 20.0, 150.0, 48.0),
            "disabled gutter wheel is not consumed"
        );
        let v0 = state.plot_view_by_key("p").expect("view");
        state.modifiers.shift = true;
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -1.0));
        let v1 = state.plot_view_by_key("p").expect("view");
        assert!(
            v1.x.max - v1.x.min < v0.x.max - v0.x.min,
            "disabled shift-wheel falls through to X zoom"
        );
        assert_eq!(v1.y, v0.y);
    }

    /// A stack that fits entirely has nothing to scroll: the gutter wheel
    /// falls through (so an enclosing page can scroll) and Shift+wheel
    /// degrades to the plain X zoom instead of dead-ending.
    #[test]
    fn lane_plot_fitting_stack_wheel_falls_through() {
        let (tree, mut state) = lane_setup(4); // 4 lanes: fits in 262px
        let before = state.plot_view_by_key("p").expect("view");
        assert_eq!((before.y.min, before.y.max), (0.0, 4.0), "full stack");
        assert!(
            !state.plot_wheel_zoom(&tree, 20.0, 150.0, 48.0),
            "gutter wheel not consumed when nothing can scroll"
        );
        state.modifiers.shift = true;
        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -1.0));
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            after.x.max - after.x.min < before.x.max - before.x.min,
            "shift+wheel degrades to X zoom on a fitting stack"
        );
        assert_eq!(after.y, before.y);
    }

    /// A Y box-zoom on a lane plot snaps outward to lane boundaries by
    /// default, and stays continuous with `.lane_zoom_snap(false)`.
    #[test]
    fn lane_plot_y_box_zoom_snaps_to_lane_bounds() {
        let (tree, mut state) = lane_setup(10);
        // Full stack visible (10 ≤ 262/24 lanes? no — 10 < 10.9, fits).
        let full = state.plot_view_by_key("p").expect("view");
        assert_eq!((full.y.min, full.y.max), (0.0, 10.0));
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        // A vertical drag from py=60 to py=220 sweeps stack ≈ 1.98..8.09.
        state.begin_plot_zoom(id.clone(), 200.0, 60.0);
        state.drag_plot_zoom_to(206.0, 220.0);
        state.end_plot_zoom();
        let after = state.plot_view_by_key("p").expect("view");
        assert_eq!(
            (after.y.min, after.y.max),
            (1.0, 9.0),
            "snapped outward to whole lanes"
        );
        // The snapped window survives the next prepare (clamp is a no-op).
        state.prepare_plots(&tree);
        assert_eq!(state.plot_view_by_key("p").expect("view").y, after.y);

        // Continuous selection with snapping off.
        use crate::plot::Lane;
        let mut spec = PlotSpec::new().x(Scale::linear()).lane_zoom_snap(false);
        for i in 0..10 {
            let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 1.0)]);
            spec = spec.lane(Lane::digital(format!("ch{i}"), &h));
        }
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        state.begin_plot_zoom(id, 200.0, 60.0);
        state.drag_plot_zoom_to(206.0, 220.0);
        state.end_plot_zoom();
        let cont = state.plot_view_by_key("p").expect("view");
        assert!(
            (cont.y.min - 1.985).abs() < 0.05 && (cont.y.max - 8.092).abs() < 0.05,
            "continuous: {:?}",
            cont.y
        );
    }

    /// Double-click reset on a lane plot returns to the readable clamped
    /// initial view, not to fit-all slivers.
    #[test]
    fn lane_plot_reset_returns_to_clamped_view() {
        let (tree, mut state) = lane_setup(100);
        let initial = state.plot_view_by_key("p").expect("view");
        let id = state.plot_at(200.0, 150.0).expect("plot").0;
        // Zoom into two lanes, then reset.
        state.begin_plot_zoom(id.clone(), 200.0, 60.0);
        state.drag_plot_zoom_to(206.0, 220.0);
        state.end_plot_zoom();
        assert_ne!(state.plot_view_by_key("p").expect("view").y, initial.y);
        assert!(state.reset_plot_view(&id));
        state.prepare_plots(&tree);
        let reset = state.plot_view_by_key("p").expect("view");
        assert_eq!(reset.y, initial.y, "reset re-seeds the readable clamp");
    }

    // ---- x_autoscale / PlotRequest (#116) ----

    /// The reported streaming freeze: series grow via `append`, and the X
    /// window must follow across prepare passes instead of sticking at the
    /// first-seed extent.
    #[test]
    fn streaming_appends_stay_in_view() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::linear())
            .add_mark(line(&h));
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);
        let first = state.plot_view_by_key("p").expect("view");
        assert!(first.x.max < 20.0);

        // The worker thread publishes more samples; next frame's prepare
        // must extend the window.
        h.append(&[Sample::new(500.0, 3.0)]);
        state.prepare_plots(&tree);
        let next = state.plot_view_by_key("p").expect("view");
        assert!(next.x.max > 500.0, "follows the tail: {:?}", next.x);
    }

    /// Any manual X gesture stops the tracking: the wheel here, standing in
    /// for pan and X box-zoom which share the same `x_manual` mark.
    #[test]
    fn wheel_zoom_takes_manual_x_and_stops_tracking() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::linear())
            .add_mark(line(&h));
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);

        assert!(state.plot_wheel_zoom(&tree, 200.0, 150.0, -1.0));
        let zoomed = state.plot_view_by_key("p").expect("view");
        h.append(&[Sample::new(500.0, 3.0)]);
        state.prepare_plots(&tree);
        let after = state.plot_view_by_key("p").expect("view");
        assert_eq!(after.x, zoomed.x, "manual window holds against appends");

        // Double-click reset restores the data-driven framing.
        let (id, _) = state.plot_at(200.0, 150.0).expect("plot");
        assert!(state.reset_plot_view(&id));
        state.prepare_plots(&tree);
        let reset = state.plot_view_by_key("p").expect("view");
        assert!(reset.x.max > 500.0, "reset re-arms tracking: {:?}", reset.x);
    }

    /// `PlotRequest::SetXWindow` pins the window (taking manual X);
    /// `FitAll` re-fits and restores tracking. Both consumed by key during
    /// the prepare walk.
    #[test]
    fn plot_requests_drive_the_view() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::linear())
            .add_mark(line(&h));
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);

        state.push_plot_requests(vec![crate::plot::PlotRequest::SetXWindow {
            key: "p".into(),
            min: 2.0,
            max: 4.0,
        }]);
        state.prepare_plots(&tree);
        let pinned = state.plot_view_by_key("p").expect("view");
        assert_eq!((pinned.x.min, pinned.x.max), (2.0, 4.0));
        // ...and it holds against growth (manual X).
        h.append(&[Sample::new(500.0, 3.0)]);
        state.prepare_plots(&tree);
        let held = state.plot_view_by_key("p").expect("view");
        assert_eq!((held.x.min, held.x.max), (2.0, 4.0));

        state.push_plot_requests(vec![crate::plot::PlotRequest::FitAll { key: "p".into() }]);
        state.prepare_plots(&tree);
        let fit = state.plot_view_by_key("p").expect("view");
        assert!(
            fit.x.max > 500.0,
            "FitAll re-frames everything: {:?}",
            fit.x
        );
        // Degenerate window is ignored.
        state.push_plot_requests(vec![crate::plot::PlotRequest::SetXWindow {
            key: "p".into(),
            min: 4.0,
            max: 4.0,
        }]);
        state.prepare_plots(&tree);
        let unchanged = state.plot_view_by_key("p").expect("view");
        assert_eq!(unchanged.x, fit.x, "empty window ignored");
    }

    /// A purely vertical pan must not freeze X tracking: Y-autoscale
    /// absorbs it next frame, so treating it as manual X control would
    /// silently stop a streaming plot's tail-follow with no visual cue.
    #[test]
    fn vertical_pan_does_not_take_manual_x() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let spec = PlotSpec::new()
            .x(Scale::linear())
            .y(Scale::linear())
            .add_mark(line(&h));
        let mut tree = plot_widget(spec).key("p");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.prepare_plots(&tree);

        let (id, _) = state.plot_at(200.0, 150.0).expect("plot");
        state.begin_plot_pan(id, 200.0, 150.0);
        assert!(state.drag_plot_to(200.0, 100.0)); // vertical only
        state.end_plot_pan();

        h.append(&[Sample::new(500.0, 3.0)]);
        state.prepare_plots(&tree);
        let view = state.plot_view_by_key("p").expect("view");
        assert!(view.x.max > 500.0, "tail-follow survives: {:?}", view.x);
    }
}
