//! Persistent pan/zoom state accessors and GC for
//! [`plot()`](crate::tree::plot) nodes — the plot counterpart of
//! [`viewport`](super::viewport). The per-node [`PlotView`] survives
//! rebuilds (keyed by `computed_id`, LRU-bounded); `draw_ops` seeds it by
//! auto-fitting the data on first show, and the gesture router mutates it.

use crate::plot::PlotView;
use crate::tree::El;

use super::UiState;
use super::types::{PlotMetrics, PlotPanDrag, PlotState};

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
        self.plot.views.get(id).copied()
    }

    /// Seed or overwrite the [`PlotView`] for the plot keyed `id`. Lets an
    /// app pre-frame a plot (e.g. to a fixed time window) before the first
    /// resolve, or drive the view programmatically.
    pub fn set_plot_view(&mut self, id: impl Into<String>, view: PlotView) {
        self.plot.views.insert(id.into(), view);
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
        self.plot.metrics.get(id).copied()
    }

    /// Resolve every plot node's view + layout for this frame, **before**
    /// `draw_ops` reads them. For each [`plot()`](crate::tree::plot) node:
    /// seed the [`PlotView`] by auto-fitting the data on first show, refit
    /// the Y axis to the visible window when `y_autoscale` is on, and record
    /// the data rect + scales as metrics (for the gesture router and the
    /// by-key readback). Reads laid-out rects from
    /// [`computed_rects`](super::types::LayoutState), so it must run after
    /// layout. Walks the tree mutating `self.plot`; the `&El` borrow of
    /// `root` is independent of the `self.plot` writes.
    pub(crate) fn prepare_plots(&mut self, node: &El) {
        if let Some(spec) = &node.plot_source {
            if let Some(rect) = self.layout.computed_rects.get(&node.computed_id).copied() {
                let id = node.computed_id.clone();
                let view = crate::plot::resolve::resolve_view(spec, self.plot_view(&id));
                self.store_plot_view(&id, view);
                self.store_plot_metrics(
                    &id,
                    PlotMetrics {
                        data_rect: crate::plot::resolve::data_rect(rect),
                        x_scale: spec.x.scale,
                        y_scale: spec.y.scale,
                        crosshair: spec.crosshair,
                    },
                );
            }
        }
        for c in &node.children {
            self.prepare_plots(c);
        }
    }

    /// Deepest plot whose data rect contains `(x, y)`, with its resolved
    /// metrics — the gesture router's entry point for pan / zoom. "Deepest"
    /// is the longest `computed_id` path, so a plot nested in another picks
    /// the inner one.
    pub(crate) fn plot_at(&self, x: f32, y: f32) -> Option<(String, PlotMetrics)> {
        let mut best: Option<(&String, PlotMetrics)> = None;
        for (id, m) in &self.plot.metrics {
            if m.data_rect.contains(x, y)
                && best.as_ref().is_none_or(|(b, _)| id.len() > b.len())
            {
                best = Some((id, *m));
            }
        }
        best.map(|(id, m)| (id.clone(), m))
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
        self.store_plot_view(&drag.plot_id, next);
        moved
    }

    /// End any active plot pan drag. Returns whether one was in flight.
    pub(crate) fn end_plot_pan(&mut self) -> bool {
        self.plot.pan_drag.take().is_some()
    }

    /// Zoom the plot under `(x, y)` by one wheel notch, anchored so the data
    /// under the cursor stays fixed. `dy > 0` (Damascene wheel convention)
    /// zooms out. Returns `true` when a plot consumed the wheel (so it
    /// doesn't also scroll an enclosing container). Both axes zoom; with
    /// `y_autoscale` on the Y change is refit away next frame, leaving an
    /// X-axis (time) zoom.
    pub(crate) fn plot_wheel_zoom(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if dy.abs() <= f32::EPSILON {
            return false;
        }
        let Some((id, m)) = self.plot_at(x, y) else {
            return false;
        };
        let Some(view) = self.plot_view(&id) else {
            return false;
        };
        // `factor` multiplies the window *width*, so the mapping is the
        // inverse of the viewport's zoom-multiplier: zoom in (dy < 0) shrinks
        // the window (factor < 1); zoom out (dy > 0) grows it.
        let factor = if dy > 0.0 {
            PLOT_WHEEL_STEP
        } else {
            1.0 / PLOT_WHEEL_STEP
        };
        let next = view.zoom_about((factor, factor), (x, y), m.x_scale, m.y_scale, m.data_rect);
        self.store_plot_view(&id, next);
        true
    }

    /// Bound the persistent plot `views` map (LRU over absent identities),
    /// the counterpart of [`Self::gc_viewport_state`]. Called once per frame
    /// from `RunnerCore::prepare_layout`.
    pub(crate) fn gc_plot_state(&mut self, root: &El) {
        let mut live: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        collect_plot_ids(root, &mut live);
        // Per-frame metrics are scratch: only keep live plots' entries.
        self.plot.metrics.retain(|id, _| live.contains(id.as_str()));
        self.plot.gc(&live);
    }
}

/// Collect the `computed_id`s of every [`plot()`](crate::tree::plot) node.
fn collect_plot_ids<'a>(node: &'a El, out: &mut rustc_hash::FxHashSet<&'a str>) {
    if node.plot_source.is_some() {
        out.insert(node.computed_id.as_str());
    }
    for child in &node.children {
        collect_plot_ids(child, out);
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
        let (_tree, mut state) = setup();
        let before = state.plot_view_by_key("p").expect("view");
        // dy < 0 zooms in, anchored at the data-rect centre.
        assert!(state.plot_wheel_zoom(200.0, 150.0, -1.0));
        let after = state.plot_view_by_key("p").expect("view");
        assert!(
            (after.x.max - after.x.min) < (before.x.max - before.x.min),
            "zoom in shrinks x window: {:?} -> {:?}",
            before.x,
            after.x
        );
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
        assert!(after.x.min > before.x.min, "{:?} -> {:?}", before.x, after.x);
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
