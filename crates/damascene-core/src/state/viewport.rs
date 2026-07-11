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
        Some(
            self.viewport
                .views
                .get(id.as_ref())
                .copied()
                .unwrap_or_default(),
        )
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

    /// [`Self::viewport_content_bounds`] by the viewport's `.key(...)`
    /// rather than its `computed_id` — the ergonomic path for app
    /// `build` / `on_event` code (also forwarded on
    /// [`BuildCx`](crate::event::BuildCx) / [`EventCx`](crate::event::EventCx)).
    /// `None` when no laid-out node carries `key` or the viewport has no
    /// measurable content.
    pub fn viewport_content_bounds_by_key(&self, key: &str) -> Option<crate::tree::Rect> {
        let id = self.layout.key_index.get(key)?;
        self.viewport
            .metrics
            .get(id.as_ref())
            .and_then(|m| m.content)
    }

    /// Whether the viewport keyed `id` (`computed_id`) is still at its
    /// *home framing* — the policy fit under
    /// [`FitPolicy::Contain`](crate::viewport::FitPolicy::Contain) /
    /// [`Lock`](crate::viewport::FitPolicy::Lock), or the last
    /// programmatic fit / reset under
    /// [`Manual`](crate::viewport::FitPolicy::Manual). Turns `false` the
    /// moment the view is steered away — by a user pan / zoom gesture, a
    /// [`CenterOn`](crate::viewport::ViewportRequest::CenterOn) request,
    /// or [`Self::set_viewport_view`] — and `true` again after a
    /// [`ResetView`](crate::viewport::ViewportRequest::ResetView) /
    /// [`FitContent`](crate::viewport::ViewportRequest::FitContent)
    /// request resolves. Chrome uses this to show "Fit" vs a concrete
    /// zoom percentage, or to disable a Reset button at home.
    pub fn viewport_at_home(&self, id: &str) -> bool {
        !self.viewport.taken_over.contains(id)
    }

    /// [`Self::viewport_at_home`] by the viewport's `.key(...)`. `None`
    /// when no laid-out node carries `key`.
    pub fn viewport_at_home_by_key(&self, key: &str) -> Option<bool> {
        let id = self.layout.key_index.get(key)?;
        Some(!self.viewport.taken_over.contains(id.as_ref()))
    }

    /// Seed or overwrite the pan/zoom for the viewport keyed `id`. Call
    /// after [`crate::layout::assign_ids`] (so `computed_id`s exist) to
    /// pre-position a viewport before the first layout. The next layout
    /// pass clamps the value against the live viewport rect and content
    /// extents.
    ///
    /// A seeded view counts as a deliberate framing: it marks the
    /// viewport *taken over* so a
    /// [`FitPolicy::Contain`](crate::viewport::FitPolicy::Contain)
    /// doesn't immediately re-fit over it (e.g. a framing restored from
    /// disk). Push a
    /// [`ResetView`](crate::viewport::ViewportRequest::ResetView) /
    /// [`FitContent`](crate::viewport::ViewportRequest::FitContent)
    /// request to re-arm the policy.
    pub fn set_viewport_view(&mut self, id: impl Into<String>, view: ViewportView) {
        let id = id.into();
        self.viewport.taken_over.insert(id.clone());
        // A direct write is a deliberate framing: it grounds any smooth
        // navigation still in flight.
        self.viewport.flights.remove(&id);
        self.viewport.views.insert(id, view);
    }

    /// Whether the viewport keyed `id` (`computed_id`) is mid-flight on a
    /// smooth programmatic navigation
    /// ([`ViewportBehavior::Smooth`](crate::viewport::ViewportBehavior::Smooth)).
    /// `false` once the flight arrives, and immediately after a user
    /// gesture or a new instant request grounds it. Apps use this to
    /// gate input or chrome during a fly-to.
    pub fn viewport_in_flight(&self, id: &str) -> bool {
        self.viewport.flights.contains_key(id)
    }

    /// [`Self::viewport_in_flight`] by the viewport's `.key(...)`. `None`
    /// when no laid-out node carries `key`.
    pub fn viewport_in_flight_by_key(&self, key: &str) -> Option<bool> {
        let id = self.layout.key_index.get(key)?;
        Some(self.viewport.flights.contains_key(id.as_ref()))
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
    ///
    /// [`FitPolicy::Lock`](crate::viewport::FitPolicy::Lock)ed viewports
    /// are not gesture targets: they're skipped entirely, so the gesture
    /// falls through to an enclosing viewport (or, for the wheel, to
    /// scroll routing).
    pub(crate) fn viewport_at(&self, x: f32, y: f32) -> Option<(String, ViewportConfig)> {
        let mut best: Option<(&String, ViewportConfig)> = None;
        for (id, m) in &self.viewport.metrics {
            if matches!(m.cfg.fit, crate::viewport::FitPolicy::Lock { .. }) {
                continue;
            }
            if m.inner.contains(x, y) && best.as_ref().is_none_or(|(b, _)| id.len() > b.len()) {
                best = Some((id, m.cfg));
            }
        }
        best.map(|(id, cfg)| (id.clone(), cfg))
    }

    /// Begin a pan drag on the viewport keyed `id`, anchoring on the
    /// current pan so the content tracks the cursor 1:1. Captured at
    /// `pointer_down` (or at latent-pan conversion); pre-empts hit-test
    /// like the camera drag. `button` is the press that began the drag —
    /// only its release ends the capture.
    pub(crate) fn begin_viewport_pan(
        &mut self,
        id: String,
        button: crate::event::PointerButton,
        x: f32,
        y: f32,
    ) {
        let start_pan = self.viewport_view(&id).pan;
        self.viewport.pan_drag = Some(ViewportPanDrag {
            viewport_id: id,
            button,
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
        if moved {
            // A real pan (not just a press) steers the view away from
            // home: release any `FitPolicy::Contain`, flip the at-home
            // readback, and ground any smooth navigation in flight —
            // the user wins mid-flight.
            self.viewport.taken_over.insert(drag.viewport_id.clone());
            self.viewport.flights.remove(&drag.viewport_id);
        }
        self.viewport.views.insert(drag.viewport_id, view);
        moved
    }

    /// End the active pan drag if `button` is the one that began it
    /// (issue #128: another button's release falls through to its own
    /// normal handling instead of killing the drag). Returns whether a
    /// drag ended.
    pub(crate) fn end_viewport_pan(&mut self, button: crate::event::PointerButton) -> bool {
        if self
            .viewport
            .pan_drag
            .as_ref()
            .is_some_and(|d| d.button == button)
        {
            self.viewport.pan_drag = None;
            return true;
        }
        false
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
        if crate::hit_test::occluded_by_overlay(root, (x, y), &id) {
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
            // An effective zoom (not a notch at the limit) takes over
            // the view — same release rule as a pan drag, including
            // grounding any smooth navigation in flight.
            self.viewport.taken_over.insert(id.clone());
            self.viewport.flights.remove(&id);
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
        self.prune_flights(live);
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
        // The taken-over flag rides on the view: once a viewport's view
        // is evicted, its takeover state is meaningless too.
        let views = &self.views;
        self.taken_over.retain(|id| views.contains_key(id));
    }

    /// Drop flights whose viewport is no longer in the tree — nothing
    /// samples them, and a lingering entry would pin `needs_redraw`
    /// forever. Called from the per-frame side-map GC with the live set.
    pub(crate) fn prune_flights(&mut self, live: &rustc_hash::FxHashSet<&str>) {
        self.flights.retain(|id, _| live.contains(id.as_str()));
    }

    /// Whether any flight is still inside its animation window at `now` —
    /// the runtime's redraw signal. An **expired** flight that layout
    /// never got to sample (its viewport sat in a scroll-pruned subtree,
    /// which stays in the tree but isn't laid out) must not pin frames:
    /// it lazily snaps to its endpoint on the next layout that actually
    /// reaches the viewport.
    pub(crate) fn any_flight_animating(&self, now: web_time::Instant) -> bool {
        let now = self.clock_override.unwrap_or(now);
        self.flights
            .values()
            .any(|f| now.saturating_duration_since(f.started) < f.duration)
    }
}
