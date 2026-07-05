//! Scroll offset, scrollbar, and wheel helpers for [`UiState`](super::UiState).

use crate::hit_test::scroll_targets_at;
use crate::tree::{El, Rect};

use super::{
    UiState,
    types::{SCROLL_MOMENTUM_DECAY_PER_SEC, SCROLL_MOMENTUM_STOP_VELOCITY, ScrollMomentum},
};
use web_time::Instant;

const WHEEL_EPSILON: f32 = 0.5;

/// Maximum number of scrollable / virtual-list identities the
/// persistent scroll maps retain (issue #57). Entries for nodes
/// currently in the tree are always kept; the cap only evicts
/// identities that have been absent the longest, so scroll-position
/// restoration across unmount/remount (tab switches, page navigation)
/// survives until an app has accumulated thousands of dead
/// scrollables. Each identity costs roughly its id string plus a few
/// floats — except virtual lists, whose per-row measurement cache is
/// the real weight; eviction drops that whole subtree.
pub(crate) const SCROLL_LRU_CAP: usize = 4096;

#[derive(Clone, Debug)]
pub(crate) struct ScrollStep {
    pub scroll_id: String,
    pub applied_delta: f32,
}

impl UiState {
    /// Bound the persistent scroll side-maps (issue #57). Counterpart
    /// of the per-frame GC in `state/animation.rs`, but LRU rather
    /// than live-tree-only: scroll offsets, anchors, pins, and
    /// virtual-list row measurements deliberately outlive their node
    /// so keyed content re-entering the tree restores its scroll
    /// position. Identities seen in `root` are stamped fresh; once the
    /// registry exceeds [`SCROLL_LRU_CAP`], the longest-unseen absent
    /// identities are evicted from every persistent map. Called once
    /// per frame from `RunnerCore::prepare_layout`, right after
    /// layout.
    #[cfg(test)]
    pub(crate) fn gc_scroll_state(&mut self, root: &El) {
        let mut live: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        collect_scroll_ids(root, &mut live);
        self.scroll.gc(&live);
    }

    /// Run all three per-frame side-map GCs (scroll, viewport pan/zoom,
    /// plot views) off a single tree walk. The individual `gc_*_state`
    /// entry points remain for tests; production calls this once per
    /// `prepare_layout` — traversal count dominates on large trees.
    pub(crate) fn gc_side_maps(&mut self, root: &El) {
        let mut scroll_live: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        let mut viewport_live: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        let mut plot_live: rustc_hash::FxHashSet<&str> = rustc_hash::FxHashSet::default();
        collect_gc_ids(root, &mut scroll_live, &mut viewport_live, &mut plot_live);
        self.scroll.gc(&scroll_live);
        self.viewport.gc(&viewport_live);
        self.gc_plot_with_live(&plot_live);
    }

    /// Seed or read the persistent scroll offset for `id`. Use this to
    /// pre-position a scroll viewport before [`crate::layout::layout`]
    /// runs (call [`crate::layout::assign_ids`] first to populate the
    /// node's `computed_id`).
    pub fn set_scroll_offset(&mut self, id: impl Into<String>, value: f32) {
        self.scroll.offsets.insert(id.into(), value);
    }

    /// Read the current scroll offset for `id`. Defaults to `0.0`.
    pub fn scroll_offset(&self, id: &str) -> f32 {
        self.scroll.offsets.get(id).copied().unwrap_or(0.0)
    }

    /// Queue programmatic scroll-to-row requests targeting virtual
    /// lists by key. Each request is consumed during layout of the
    /// matching list — viewport height and row heights are only known
    /// then, especially for `virtual_list_dyn` where unmeasured rows
    /// use the configured estimate. Hosts call this once per frame
    /// from [`crate::event::App::drain_scroll_requests`]; apps that
    /// own a `Runner` can also push directly for tests.
    pub fn push_scroll_requests(&mut self, requests: Vec<crate::scroll::ScrollRequest>) {
        self.scroll.pending_requests.extend(requests);
    }

    /// Drop any scroll requests still queued after the layout pass
    /// completed. Called by `prepare_layout` so requests targeting a
    /// list that wasn't in the tree this frame don't silently fire
    /// against a re-mounted list with the same key on a later frame.
    pub fn clear_pending_scroll_requests(&mut self) {
        self.scroll.pending_requests.clear();
    }

    /// Iterate `(scroll_node_id, track_rect)` for every scrollable
    /// whose visible scrollbar is currently active. Hosts use this to
    /// drive cursor changes (e.g., a vertical-resize cursor over the
    /// thumb), to drive screenshot tools, or to test interaction
    /// flows. The map is rebuilt every layout pass.
    pub fn scrollbar_tracks(&self) -> impl Iterator<Item = (&str, &Rect)> {
        self.scroll
            .thumb_tracks
            .iter()
            .map(|(id, rect)| (id.as_str(), rect))
    }

    /// Look up the scrollable whose track rect contains `(x, y)`,
    /// returning its `computed_id`, the track rect, and the visible
    /// thumb rect. Returns `None` if no track is currently visible at
    /// that point. The track rect is wider than the visible thumb
    /// (Fitts's law) and spans the full viewport height so callers
    /// can branch on whether `y` lands inside the thumb (grab) or
    /// above/below (click-to-page).
    pub fn thumb_at(&self, x: f32, y: f32) -> Option<(String, Rect, Rect)> {
        for (id, track) in &self.scroll.thumb_tracks {
            if track.contains(x, y) {
                let thumb = self
                    .scroll
                    .thumb_rects
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| Rect::new(track.x, track.y, track.w, 0.0));
                return Some((id.clone(), *track, thumb));
            }
        }
        None
    }

    /// Increment the scroll offset for the deepest scrollable container
    /// under `point` that can move in `dy`'s direction. If the deepest
    /// container is already at that edge (or has no overflow), the wheel
    /// bubbles to the nearest scrollable ancestor that can move.
    ///
    /// Returns `true` if any scrollable consumed the wheel and updated
    /// its stored offset. Hosts use this to decide whether to request a
    /// redraw.
    pub fn pointer_wheel(&mut self, root: &El, point: (f32, f32), dy: f32) -> bool {
        self.scroll_by_pointer(root, point, dy).is_some()
    }

    pub(crate) fn scroll_by_pointer(
        &mut self,
        root: &El,
        point: (f32, f32),
        dy: f32,
    ) -> Option<ScrollStep> {
        if dy.abs() <= f32::EPSILON {
            return None;
        }
        for id in scroll_targets_at(root, point).into_iter().rev() {
            if let Some(step) = self.scroll_by_id(&id, dy) {
                return Some(step);
            }
        }
        None
    }

    pub(crate) fn scroll_by_id(&mut self, id: &str, dy: f32) -> Option<ScrollStep> {
        if dy.abs() <= f32::EPSILON {
            return None;
        }
        let metrics = self.scroll.metrics.get(id).copied()?;
        if metrics.max_offset <= WHEEL_EPSILON {
            return None;
        }
        let current = self
            .scroll
            .offsets
            .get(id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, metrics.max_offset);
        let can_scroll = if dy > 0.0 {
            current < metrics.max_offset - WHEEL_EPSILON
        } else {
            current > WHEEL_EPSILON
        };
        if !can_scroll {
            return None;
        }
        let next = (current + dy).clamp(0.0, metrics.max_offset);
        if (next - current).abs() <= f32::EPSILON {
            return None;
        }
        self.scroll.offsets.insert(id.to_owned(), next);
        Some(ScrollStep {
            scroll_id: id.to_owned(),
            applied_delta: next - current,
        })
    }

    pub(crate) fn start_scroll_momentum(
        &mut self,
        scroll_id: Option<String>,
        velocity: f32,
        now: Instant,
    ) {
        let Some(scroll_id) = scroll_id else {
            self.scroll.momentum = None;
            return;
        };
        if velocity.abs() < super::types::SCROLL_MOMENTUM_MIN_VELOCITY {
            self.scroll.momentum = None;
            return;
        }
        self.scroll.momentum = Some(ScrollMomentum {
            scroll_id,
            velocity,
            last_tick: now,
        });
    }

    pub(crate) fn cancel_scroll_momentum(&mut self) {
        self.scroll.momentum = None;
    }

    pub(crate) fn has_scroll_momentum(&self) -> bool {
        self.scroll.momentum.is_some()
    }

    pub(crate) fn tick_scroll_momentum(&mut self, now: Instant) -> bool {
        let Some(mut momentum) = self.scroll.momentum.take() else {
            return false;
        };
        let dt = now
            .duration_since(momentum.last_tick)
            .as_secs_f32()
            .clamp(0.0, 0.050);
        momentum.last_tick = now;
        if dt <= f32::EPSILON {
            self.scroll.momentum = Some(momentum);
            return true;
        }

        let Some(metrics) = self.scroll.metrics.get(&momentum.scroll_id).copied() else {
            return false;
        };
        if metrics.max_offset <= WHEEL_EPSILON {
            return false;
        }
        let current = self
            .scroll
            .offsets
            .get(&momentum.scroll_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, metrics.max_offset);
        let next = (current + momentum.velocity * dt).clamp(0.0, metrics.max_offset);
        let changed = (next - current).abs() > f32::EPSILON;
        if changed {
            self.scroll.offsets.insert(momentum.scroll_id.clone(), next);
        }

        let hit_edge = (next <= WHEEL_EPSILON && momentum.velocity < 0.0)
            || (next >= metrics.max_offset - WHEEL_EPSILON && momentum.velocity > 0.0);
        momentum.velocity *= (-SCROLL_MOMENTUM_DECAY_PER_SEC * dt).exp();
        if !hit_edge && momentum.velocity.abs() > SCROLL_MOMENTUM_STOP_VELOCITY {
            self.scroll.momentum = Some(momentum);
        }
        changed || self.scroll.momentum.is_some()
    }
}

/// Collect the `computed_id`s of every node that keys the persistent
/// scroll maps — scroll containers and virtual lists.
fn collect_gc_ids<'a>(
    node: &'a El,
    scroll: &mut rustc_hash::FxHashSet<&'a str>,
    viewport: &mut rustc_hash::FxHashSet<&'a str>,
    plot: &mut rustc_hash::FxHashSet<&'a str>,
) {
    if node.scrollable || node.virtual_items.is_some() {
        scroll.insert(node.computed_id.as_ref());
    }
    if node.viewport.is_some() {
        viewport.insert(node.computed_id.as_ref());
    }
    if node.plot_source.is_some() {
        plot.insert(node.computed_id.as_ref());
    }
    for child in &node.children {
        collect_gc_ids(child, scroll, viewport, plot);
    }
}

#[cfg(test)]
fn collect_scroll_ids<'a>(node: &'a El, out: &mut rustc_hash::FxHashSet<&'a str>) {
    if node.scrollable || node.virtual_items.is_some() {
        out.insert(node.computed_id.as_ref());
    }
    for child in &node.children {
        collect_scroll_ids(child, out);
    }
}

impl super::types::ScrollState {
    /// LRU pass over the identity-keyed persistent maps. See
    /// [`UiState::gc_scroll_state`] for the policy rationale.
    pub(crate) fn gc(&mut self, live: &rustc_hash::FxHashSet<&str>) {
        self.frame += 1;
        let frame = self.frame;

        // Register / refresh stamps. Identities in the live tree are
        // always fresh; an identity first noticed while absent (e.g.
        // seeded via `set_scroll_offset` before its node mounts) ages
        // from now.
        let maps_keys = self
            .offsets
            .keys()
            .chain(self.measured_row_heights.keys())
            .chain(self.dyn_height_index.keys())
            .chain(self.virtual_anchors.keys())
            .chain(self.scroll_anchors.keys())
            .chain(self.pin_active.keys())
            .chain(self.pin_prev_max.keys());
        let mut stamp: Vec<String> = Vec::new();
        for id in maps_keys {
            if live.contains(id.as_str()) || !self.last_seen.contains_key(id) {
                stamp.push(id.clone());
            }
        }
        for id in stamp {
            self.last_seen.insert(id, frame);
        }

        // Drop registry entries whose identity no longer keys any map
        // (pins removed by `resolve_pin`, measurement subtrees emptied
        // by `prune_dynamic_measurements`), so they don't count toward
        // the cap.
        let offsets = &self.offsets;
        let measured = &self.measured_row_heights;
        let dyn_idx = &self.dyn_height_index;
        let v_anchors = &self.virtual_anchors;
        let s_anchors = &self.scroll_anchors;
        let pin_a = &self.pin_active;
        let pin_m = &self.pin_prev_max;
        self.last_seen.retain(|id, _| {
            offsets.contains_key(id)
                || measured.contains_key(id)
                || dyn_idx.contains_key(id)
                || v_anchors.contains_key(id)
                || s_anchors.contains_key(id)
                || pin_a.contains_key(id)
                || pin_m.contains_key(id)
        });

        if self.last_seen.len() <= SCROLL_LRU_CAP {
            return;
        }

        // Over cap: evict the longest-unseen identities that are not
        // in the live tree. Live identities are never evicted, so a
        // single frame with more than CAP live scrollables degrades to
        // "no eviction" rather than thrashing.
        let mut absent: Vec<(u64, String)> = self
            .last_seen
            .iter()
            .filter(|(id, _)| !live.contains(id.as_str()))
            .map(|(id, f)| (*f, id.clone()))
            .collect();
        absent.sort_unstable_by(|a, b| (a.0, a.1.as_str()).cmp(&(b.0, b.1.as_str())));
        let overflow = self.last_seen.len() - SCROLL_LRU_CAP;
        for (_, id) in absent.into_iter().take(overflow) {
            self.offsets.remove(&id);
            self.measured_row_heights.remove(&id);
            self.dyn_height_index.remove(&id);
            self.virtual_anchors.remove(&id);
            self.scroll_anchors.remove(&id);
            self.pin_active.remove(&id);
            self.pin_prev_max.remove(&id);
            self.last_seen.remove(&id);
        }
    }
}
