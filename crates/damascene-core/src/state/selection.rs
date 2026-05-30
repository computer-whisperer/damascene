//! Selection traversal state helpers for [`UiState`](super::UiState).

use crate::event::UiTarget;
use crate::tree::El;

use super::UiState;

impl UiState {
    /// Walk the laid-out tree and rebuild the selectable-text order.
    /// Same shape as [`Self::sync_focus_order`] but filters for
    /// `selectable` keyed leaves instead of `focusable` ones. Should
    /// run on every frame post-layout, before the selection manager
    /// processes pointer events.
    pub fn sync_selection_order(&mut self, root: &El) {
        let order = crate::focus::selection_order(root, self);
        self.selection.order = order;
    }

    /// Read access to the current document-order list of selectable
    /// leaves. Mainly for tests; the selection manager uses internal
    /// access.
    pub fn selection_order(&self) -> &[UiTarget] {
        &self.selection.order
    }
}
