//! Persistent pan/zoom state accessors and GC for
//! [`plot()`](crate::tree::plot) nodes — the plot counterpart of
//! [`viewport`](super::viewport). The per-node [`PlotView`] survives
//! rebuilds (keyed by `computed_id`, LRU-bounded); `draw_ops` seeds it by
//! auto-fitting the data on first show, and the gesture router mutates it.

use crate::plot::PlotView;
use crate::tree::El;

use super::UiState;
use super::types::{PlotMetrics, PlotState};

/// Maximum number of plot identities the persistent `views` map retains,
/// mirroring [`VIEWPORT_LRU_CAP`](super::viewport::VIEWPORT_LRU_CAP).
pub(crate) const PLOT_LRU_CAP: usize = 4096;

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
                    },
                );
            }
        }
        for c in &node.children {
            self.prepare_plots(c);
        }
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
