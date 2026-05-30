//! Focus traversal helpers for [`UiState`](super::UiState).

use web_time::Instant;

use crate::event::UiTarget;
use crate::focus::focus_order;
use crate::tree::El;

use super::UiState;

impl UiState {
    pub fn sync_focus_order(&mut self, root: &El) {
        let order = focus_order(root, self);
        self.focus.order = order;
        if let Some(focused) = &self.focused {
            if let Some(current) = self
                .focus
                .order
                .iter()
                .find(|t| t.node_id == focused.node_id)
            {
                self.focused = Some(current.clone());
                return;
            }
            // Focus order excludes nodes whose rect doesn't intersect
            // their inherited clip, so a focused widget that scrolled
            // out of view (or whose ancestor scroll viewport just
            // shrunk underneath it) is no longer in `order`. That's
            // not the same as "focus target is gone" — the node still
            // exists in the tree, it's just visually clipped.
            // Clearing focus here would dismiss the soft keyboard the
            // moment a phone's on-screen keyboard shrinks the layout
            // viewport (Android's default), which is exactly what
            // happens when the user taps a text input. Match HTML's
            // shape: only clear when the node truly leaves the tree.
            if !node_exists(root, &focused.node_id) {
                self.focused = None;
            }
        }
    }

    pub fn set_focus(&mut self, target: Option<UiTarget>) {
        let Some(target) = target else {
            self.focused = None;
            return;
        };
        if self.focus.order.iter().any(|f| f.node_id == target.node_id) {
            let changed = self.focused.as_ref().map(|f| &f.node_id) != Some(&target.node_id);
            self.focused = Some(target);
            if changed {
                self.bump_caret_activity(Instant::now());
            }
        }
    }

    /// Queue programmatic focus requests by key. Each entry is
    /// resolved once per `prepare_layout`, after the focus order has
    /// been rebuilt: matching keys focus the corresponding node;
    /// unmatched keys are dropped silently. Hosts call this once per
    /// frame from [`crate::event::App::drain_focus_requests`]; apps
    /// that own a `Runner` can also push directly for tests.
    pub fn push_focus_requests(&mut self, keys: Vec<String>) {
        self.focus.pending_requests.extend(keys);
    }

    /// Drain the queued focus requests, resolving each by key against
    /// the current focus order. The last successfully-resolved key
    /// wins. Called by `prepare_layout` after `sync_popover_focus` so
    /// explicit requests override popover auto-focus.
    pub fn drain_focus_requests(&mut self) {
        let keys = std::mem::take(&mut self.focus.pending_requests);
        for key in keys {
            if let Some(target) = self.focus.order.iter().find(|t| t.key == key).cloned() {
                self.set_focus(Some(target));
            }
        }
    }

    /// Set whether the current focus should display its focus ring.
    /// The runtime calls this from input-handling paths: pointer-down
    /// clears it (`false`), Tab and arrow-nav raise it (`true`). Apps
    /// that move focus programmatically can also flip it explicitly,
    /// e.g. force the ring on after restoring focus from an off-screen
    /// menu close. See [`UiState::focus_visible`].
    pub fn set_focus_visible(&mut self, visible: bool) {
        self.focus_visible = visible;
    }

    pub fn focus_next(&mut self) -> Option<&UiTarget> {
        self.move_focus(1)
    }

    pub fn focus_prev(&mut self) -> Option<&UiTarget> {
        self.move_focus(-1)
    }

    fn move_focus(&mut self, delta: isize) -> Option<&UiTarget> {
        if self.focus.order.is_empty() {
            self.focused = None;
            return None;
        }
        let current = self.focused.as_ref().and_then(|target| {
            self.focus
                .order
                .iter()
                .position(|t| t.node_id == target.node_id)
        });
        let len = self.focus.order.len() as isize;
        let next = match current {
            Some(current) => (current as isize + delta).rem_euclid(len) as usize,
            None if delta < 0 => self.focus.order.len() - 1,
            None => 0,
        };
        self.focused = Some(self.focus.order[next].clone());
        self.focused.as_ref()
    }
}

/// True iff `id` matches the `computed_id` of `root` or any descendant.
/// Used by [`UiState::sync_focus_order`] to distinguish "focused node
/// scrolled out of clip" (keep focus) from "focused node removed from
/// tree" (clear focus).
fn node_exists(root: &El, id: &str) -> bool {
    if root.computed_id == id {
        return true;
    }
    root.children.iter().any(|c| node_exists(c, id))
}

#[cfg(test)]
mod tests {
    use crate::layout::layout;
    use crate::state::UiState;

    /// A focused widget that scrolls out of view (its rect leaves the
    /// scroll's clip rect) must keep focus, not lose it. The web
    /// host's soft-keyboard sync polls focus every frame and dismisses
    /// the keyboard whenever focus is gone, so clearing focus on a
    /// scroll-out turned every quick tap on a phone text input into
    /// "summon then immediately dismiss the keyboard."
    #[test]
    fn focused_node_outside_clip_keeps_focus() {
        use crate::tree::*;
        // Scroll viewport 100px tall containing two 80px-tall
        // focusable rows. Without scrolling, only the first row sits
        // inside the clip; focus the second row and shrink the
        // viewport so it falls outside, then verify focus survives.
        let mut tree = crate::scroll([
            crate::widgets::button::button("a")
                .key("a")
                .height(Size::Fixed(80.0)),
            crate::widgets::button::button("b")
                .key("b")
                .height(Size::Fixed(80.0)),
        ])
        .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        state.sync_focus_order(&tree);
        let target = state
            .focus
            .order
            .iter()
            .find(|t| t.key == "b")
            .cloned()
            .expect("b in focus order");
        state.set_focus(Some(target));
        assert_eq!(
            state.focused.as_ref().map(|t| t.key.as_str()),
            Some("b"),
            "focus should land on b before reflow",
        );
        // Shrink the viewport so the scroll's clip can no longer fit
        // both buttons; b's rect (80..160) is partially inside the
        // 0..120 clip and should still be picked up. Drop further so
        // b is fully outside the clip (clip 0..50, b at 80..160).
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 50.0));
        state.sync_focus_order(&tree);
        assert_eq!(
            state.focused.as_ref().map(|t| t.key.as_str()),
            Some("b"),
            "focus should survive scroll-out clipping",
        );
    }

    /// When the focused node is genuinely removed from the tree (e.g.
    /// the page that hosted it unmounted), focus is cleared. Mirrors
    /// HTML's behavior of blurring an element that's removed from the
    /// document.
    #[test]
    fn focused_node_removed_from_tree_clears_focus() {
        use crate::tree::*;
        let mut tree = crate::column([crate::widgets::button::button("a").key("a")]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        state.sync_focus_order(&tree);
        let target = state
            .focus
            .order
            .iter()
            .find(|t| t.key == "a")
            .cloned()
            .expect("a in focus order");
        state.set_focus(Some(target));
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("a"));
        // Replace the tree with an empty one — the previously focused
        // node is gone.
        let mut empty = crate::column(Vec::<El>::new());
        layout(&mut empty, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        state.sync_focus_order(&empty);
        assert!(
            state.focused.is_none(),
            "focus should clear when the node leaves the tree",
        );
    }
}
