//! Scroll offset, scrollbar, and wheel helpers for [`UiState`](super::UiState).

use crate::hit_test::scroll_targets_at;
use crate::tree::{El, Rect};

use super::{
    UiState,
    types::{SCROLL_MOMENTUM_DECAY_PER_SEC, SCROLL_MOMENTUM_STOP_VELOCITY, ScrollMomentum},
};
use web_time::Instant;

const WHEEL_EPSILON: f32 = 0.5;

#[derive(Clone, Debug)]
pub(crate) struct ScrollStep {
    pub scroll_id: String,
    pub applied_delta: f32,
}

impl UiState {
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
        for id in scroll_targets_at(root, self, point).into_iter().rev() {
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
