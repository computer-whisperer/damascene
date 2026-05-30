//! Layout/key lookup helpers for [`UiState`](super::UiState).

use crate::event::UiTarget;
use crate::tree::{El, Rect};

use super::UiState;

impl UiState {
    /// Look up the layout-assigned rect for `id`; returns a zero rect
    /// when `id` is unknown (pre-layout, or not in the laid-out tree).
    pub fn rect(&self, id: &str) -> Rect {
        self.layout
            .computed_rects
            .get(id)
            .copied()
            .unwrap_or_default()
    }

    /// Look up the layout-assigned rect for an app-supplied element
    /// key. Returns `None` when the key is absent from `root` or layout
    /// has not written a rect for that node yet.
    pub fn rect_of_key(&self, root: &El, key: &str) -> Option<Rect> {
        find_target_by_key(root, key)
            .and_then(|target| self.layout.computed_rects.get(&target.node_id).copied())
    }

    /// Build a [`UiTarget`] for an app-supplied element key using the
    /// current layout rect. Useful for hosts that need to anchor native
    /// overlays or forward events into externally painted regions.
    pub fn target_of_key(&self, root: &El, key: &str) -> Option<UiTarget> {
        let target = find_target_by_key(root, key)?;
        let rect = self.layout.computed_rects.get(&target.node_id).copied()?;
        Some(UiTarget { rect, ..target })
    }

    /// The keyed leaf currently under the pointer, or `None` when
    /// nothing is hovered. Mirrors pointer hit-test target data, but
    /// is read-only and stable across rebuilds so apps can branch the
    /// build output on "what is hovered right now."
    ///
    /// Returns the *leaf* — the deepest keyed hit-test target. Use
    /// [`Self::is_hovering_within`] for subtree-aware queries
    /// ("is anything inside this row hovered?"), which matches the
    /// semantics of [`crate::tree::El::hover_alpha`].
    pub fn hovered_key(&self) -> Option<&str> {
        self.hovered.as_ref().map(|t| t.key.as_str())
    }

    /// True iff `key`'s node — or any descendant of it — is the
    /// current hover target. Subtree-aware: a focusable card with
    /// keyed icon-buttons inside it reports `true` whether the cursor
    /// is on the card body or on one of the buttons. Same predicate
    /// `El::hover_alpha` uses to drive its declarative reveal, exposed
    /// for app-side reads.
    ///
    /// Reads the underlying tracker, not the eased subtree envelope —
    /// the boolean flips immediately on hit-test identity change. For
    /// reactions tied to the eased animation, drive visuals through
    /// `hover_alpha` instead.
    ///
    /// Returns `false` when `key` isn't in the current tree (pre-
    /// layout, or the keyed node was removed in a recent build).
    pub fn is_hovering_within(&self, key: &str) -> bool {
        let Some(target) = self.hovered.as_ref() else {
            return false;
        };
        let Some(node_id) = self.layout.key_index.get(key) else {
            return false;
        };
        target_in_subtree(node_id, &target.node_id)
    }
}

/// True when `target_id` names `node_id` itself or any descendant of it
/// in the path-shaped `computed_id` namespace (`root.x.y.z`). Pure
/// string-prefix predicate; doesn't touch the tree, so callers can use
/// it from any context that already holds the two ids.
pub(crate) fn target_in_subtree(node_id: &str, target_id: &str) -> bool {
    if node_id.is_empty() || target_id.is_empty() {
        return false;
    }
    if target_id == node_id {
        return true;
    }
    target_id
        .strip_prefix(node_id)
        .is_some_and(|rest| rest.starts_with('.'))
}

fn find_target_by_key(root: &El, key: &str) -> Option<UiTarget> {
    if root.key.as_deref() == Some(key) {
        return Some(UiTarget {
            key: key.to_string(),
            node_id: root.computed_id.clone(),
            rect: Rect::default(),
            tooltip: root.tooltip.clone(),
            scroll_offset_y: 0.0,
        });
    }
    root.children
        .iter()
        .find_map(|child| find_target_by_key(child, key))
}
