//! Pan/zoom state accessors and GC for [`viewport()`](crate::tree::viewport)
//! containers — the counterpart of [`scroll`](super::scroll) for the 2D
//! pan/zoom transform.

use crate::tree::El;
use crate::viewport::{ViewportConfig, ViewportRequest, ViewportView};

use super::UiState;
use super::types::{ViewportPanDrag, ViewportState};

/// Wheel notch zoom multiplier — one notch zooms in/out by this factor,
/// matching the geometric feel of the 3D camera dolly.
const VIEWPORT_WHEEL_STEP: f32 = 1.1;

/// Maximum number of viewport identities the persistent `views` map
/// retains, mirroring [`SCROLL_LRU_CAP`](super::scroll::SCROLL_LRU_CAP).
/// Live viewports are always kept; the cap only evicts identities absent
/// the longest, so pan/zoom restoration across unmount/remount survives
/// until thousands of dead viewports accumulate.
pub(crate) const VIEWPORT_LRU_CAP: usize = 4096;

impl UiState {
    /// Read the current pan/zoom of the viewport keyed `id`
    /// (`computed_id`). Returns the reset framing
    /// ([`ViewportView::default`]) when the viewport has no stored view
    /// yet. Apps use this to display a zoom percentage or drive their own
    /// overlays in content coordinates.
    pub fn viewport_view(&self, id: &str) -> ViewportView {
        self.viewport.views.get(id).copied().unwrap_or_default()
    }

    /// Read a viewport's pan/zoom by its `.key(...)` rather than its
    /// `computed_id` — the ergonomic path for app `build` / `on_event`
    /// code (e.g. a zoom-percentage readout). Returns the reset view for
    /// a known but not-yet-positioned viewport, and `None` when no laid-out
    /// node carries `key`.
    pub fn viewport_view_by_key(&self, key: &str) -> Option<ViewportView> {
        let id = self.layout.key_index.get(key)?;
        Some(self.viewport.views.get(id.as_ref()).copied().unwrap_or_default())
    }

    /// The bounding box of a viewport's laid-out content in **content
    /// space** (pre-transform: pan `(0,0)`, zoom `1.0`), from the last
    /// layout. Combine with [`Self::viewport_view`] to project content
    /// into screen space — e.g. to draw a minimap or implement custom
    /// framing. `None` until the viewport has been laid out with
    /// measurable content.
    pub fn viewport_content_bounds(&self, id: &str) -> Option<crate::tree::Rect> {
        self.viewport.metrics.get(id).and_then(|m| m.content)
    }

    /// Seed or overwrite the pan/zoom for the viewport keyed `id`. Call
    /// after [`crate::layout::assign_ids`] (so `computed_id`s exist) to
    /// pre-position a viewport before the first layout. The next layout
    /// pass clamps the value against the live viewport rect and content
    /// extents.
    pub fn set_viewport_view(&mut self, id: impl Into<String>, view: ViewportView) {
        self.viewport.views.insert(id.into(), view);
    }

    /// Queue programmatic [`ViewportRequest`]s (fit-to-content, reset,
    /// center). Each is consumed during layout of the viewport whose
    /// `.key(...)` it names, where the live inner rect and content
    /// extents are known. Push once per build; unmatched requests are
    /// dropped by [`Self::clear_pending_viewport_requests`].
    pub fn push_viewport_requests(&mut self, requests: Vec<ViewportRequest>) {
        self.viewport.pending_requests.extend(requests);
    }

    /// Drop any viewport requests still queued after layout — requests
    /// targeting a viewport that wasn't in the tree this frame don't fire
    /// against a later re-mount with the same key.
    pub fn clear_pending_viewport_requests(&mut self) {
        self.viewport.pending_requests.clear();
    }

    /// Deepest viewport whose inner rect contains `(x, y)`, with its
    /// config — the input pass's entry point for routing pan / zoom
    /// gestures. "Deepest" is resolved by `computed_id` path length so
    /// nested viewports pick the inner one. Reads the per-frame metrics
    /// written by layout, so it reflects the last laid-out tree.
    pub(crate) fn viewport_at(&self, x: f32, y: f32) -> Option<(String, ViewportConfig)> {
        let mut best: Option<(&String, ViewportConfig)> = None;
        for (id, m) in &self.viewport.metrics {
            if m.inner.contains(x, y) && best.as_ref().is_none_or(|(b, _)| id.len() > b.len()) {
                best = Some((id, m.cfg));
            }
        }
        best.map(|(id, cfg)| (id.clone(), cfg))
    }

    /// Begin a pan drag on the viewport keyed `id`, anchoring on the
    /// current pan so the content tracks the cursor 1:1. Captured at
    /// `pointer_down`; pre-empts hit-test like the camera drag.
    pub(crate) fn begin_viewport_pan(&mut self, id: String, x: f32, y: f32) {
        let start_pan = self.viewport_view(&id).pan;
        self.viewport.pan_drag = Some(ViewportPanDrag {
            viewport_id: id,
            start_pointer: (x, y),
            start_pan,
        });
    }

    /// True while a viewport pan drag is in flight.
    pub(crate) fn viewport_pan_active(&self) -> bool {
        self.viewport.pan_drag.is_some()
    }

    /// Update the active pan drag to the current cursor. The stored pan
    /// is the start pan plus the cursor delta; the next layout clamps it
    /// against the content bounds before paint. Returns whether the pan
    /// moved.
    pub(crate) fn drag_viewport_to(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = self.viewport.pan_drag.clone() else {
            return false;
        };
        let mut view = self.viewport_view(&drag.viewport_id);
        let next = (
            drag.start_pan.0 + (x - drag.start_pointer.0),
            drag.start_pan.1 + (y - drag.start_pointer.1),
        );
        let moved = (next.0 - view.pan.0).abs() > f32::EPSILON
            || (next.1 - view.pan.1).abs() > f32::EPSILON;
        view.pan = next;
        self.viewport.views.insert(drag.viewport_id, view);
        moved
    }

    /// End any active pan drag. Returns whether one was in flight.
    pub(crate) fn end_viewport_pan(&mut self) -> bool {
        self.viewport.pan_drag.take().is_some()
    }

    /// Zoom the viewport under `(x, y)` by one wheel notch, anchored so
    /// the content point under the cursor stays fixed. `dy > 0` (scroll
    /// down, Damascene wheel convention) zooms out. Returns `true` when a
    /// viewport consumed the wheel — even at a zoom limit, so the wheel
    /// doesn't also scroll an enclosing container.
    pub(crate) fn viewport_wheel_zoom(&mut self, root: &El, x: f32, y: f32, dy: f32) -> bool {
        if dy.abs() <= f32::EPSILON {
            return false;
        }
        let Some((id, cfg)) = self.viewport_at(x, y) else {
            return false;
        };
        // An overlay (modal/dialog/popover) floated over the canvas takes
        // the wheel as scroll, not zoom — yield to scroll routing.
        if crate::hit_test::occluded_by_overlay(root, self, (x, y), &id) {
            return false;
        }
        let Some(metrics) = self.viewport.metrics.get(&id).copied() else {
            return false;
        };
        let view = self.viewport_view(&id);
        let factor = if dy > 0.0 {
            1.0 / VIEWPORT_WHEEL_STEP
        } else {
            VIEWPORT_WHEEL_STEP
        };
        let new_zoom = (view.zoom * factor).clamp(cfg.min_zoom, cfg.max_zoom);
        if (new_zoom - view.zoom).abs() > f32::EPSILON {
            let origin = (metrics.inner.x, metrics.inner.y);
            let next = view.zoom_about(new_zoom, (x, y), origin);
            self.viewport.views.insert(id, next);
        }
        true
    }

}

impl ViewportState {
    /// LRU pass over the persistent `views` map. Same policy as
    /// [`ScrollState::gc`](super::types::ScrollState): live identities
    /// are stamped fresh and never evicted; once `views` exceeds
    /// [`VIEWPORT_LRU_CAP`], the longest-unseen absent identities are
    /// dropped.
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

        // Drop registry entries whose identity no longer keys `views`.
        let views = &self.views;
        self.last_seen.retain(|id, _| views.contains_key(id));

        if self.last_seen.len() <= VIEWPORT_LRU_CAP {
            return;
        }

        let mut absent: Vec<(u64, String)> = self
            .last_seen
            .iter()
            .filter(|(id, _)| !live.contains(id.as_str()))
            .map(|(id, f)| (*f, id.clone()))
            .collect();
        absent.sort_unstable_by(|a, b| (a.0, a.1.as_str()).cmp(&(b.0, b.1.as_str())));
        let overflow = self.last_seen.len() - VIEWPORT_LRU_CAP;
        for (_, id) in absent.into_iter().take(overflow) {
            self.views.remove(&id);
            self.last_seen.remove(&id);
        }
    }
}
