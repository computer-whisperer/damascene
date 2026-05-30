//! Cursor resolution for [`UiState`](super::UiState).

use crate::cursor::Cursor;
use crate::tree::El;

use super::UiState;

impl UiState {
    /// Resolved pointer cursor for the current frame.
    ///
    /// Picks the cursor in this order:
    /// 1. If [`Self::pressed`] is set:
    ///    a. If the press target itself declares `.cursor_pressed(...)`,
    ///    use that — drives the slider's Grab → Grabbing transition and
    ///    the more general "press has its own affordance" idiom. Does
    ///    not inherit: an ancestor's `cursor_pressed` doesn't apply to a
    ///    descendant press target.
    ///    b. Otherwise walk from the press target up to `root` for the
    ///    first explicit `.cursor(...)`. Press capture wins so a button
    ///    drag that wanders onto a text region doesn't flicker the
    ///    cursor mid-press.
    /// 2. Else if the pointer is over a text-link run
    ///    (`hovered_link` is `Some`), use [`Cursor::Pointer`].
    ///    Link runs aren't keyed hit-test targets, so this branch sits
    ///    parallel to the keyed-hover lookup. Beats any
    ///    `.cursor(Cursor::Text)` declared on a containing paragraph
    ///    — link affordance is more specific than panel affordance.
    /// 3. Else if [`Self::hovered`] is set, walk from the hovered
    ///    target up to `root` for the first explicit declaration —
    ///    so a panel that sets `.cursor(Move)` once propagates to
    ///    children that don't override.
    /// 4. Else [`Cursor::Default`].
    ///
    /// Disabled state isn't auto-mapped to [`Cursor::NotAllowed`];
    /// widgets that want that affordance branch in their build closure.
    pub fn cursor(&self, root: &El) -> Cursor {
        if let Some(pressed) = &self.pressed {
            let id = pressed.node_id.as_str();
            if let Some(c) = cursor_pressed_at_target(root, id) {
                return c;
            }
            return cursor_for_target(root, id).unwrap_or(Cursor::Default);
        }
        if self.hovered_link.is_some() {
            return Cursor::Pointer;
        }
        if let Some(hovered) = &self.hovered {
            return cursor_for_target(root, hovered.node_id.as_str()).unwrap_or(Cursor::Default);
        }
        Cursor::Default
    }
}

/// Find the node by `target_id` and return its `cursor_pressed`, if
/// any. Unlike [`cursor_for_target`] this does **not** walk up — only
/// the literal press target's `cursor_pressed` matters (an ancestor's
/// pressed-cursor declaration shouldn't override a descendant press).
fn cursor_pressed_at_target(root: &El, target_id: &str) -> Option<Cursor> {
    fn walk(node: &El, target_id: &str) -> Option<Option<Cursor>> {
        if node.computed_id == target_id {
            return Some(node.cursor_pressed);
        }
        for c in &node.children {
            if let Some(found) = walk(c, target_id) {
                return Some(found);
            }
        }
        None
    }
    walk(root, target_id).flatten()
}

/// Resolve the cursor a node by `target_id` should display by walking
/// from `root` down, carrying the closest-ancestor cursor declaration.
/// Returns the target's own cursor if declared, else the nearest
/// ancestor's, else `None` when no ancestor (or the target itself)
/// declared one. Returns `None` when `target_id` isn't in the tree —
/// callers fall back to the default in that case.
fn cursor_for_target(root: &El, target_id: &str) -> Option<Cursor> {
    fn walk(node: &El, target_id: &str, inherited: Option<Cursor>) -> Option<Option<Cursor>> {
        let here = node.cursor.or(inherited);
        if node.computed_id == target_id {
            return Some(here);
        }
        for c in &node.children {
            if let Some(found) = walk(c, target_id, here) {
                return Some(found);
            }
        }
        None
    }
    walk(root, target_id, None).flatten()
}
