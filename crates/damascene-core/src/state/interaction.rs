//! Hover/press/focus interaction-state resolution.

use web_time::Instant;

use crate::event::UiTarget;
use crate::tree::InteractionState;

use super::UiState;

impl UiState {
    /// Resolved interaction state for `id`. Returns
    /// [`InteractionState::Default`] when no tracker matches.
    pub fn node_state(&self, id: &str) -> InteractionState {
        self.node_states.nodes.get(id).copied().unwrap_or_default()
    }

    /// Rebuild the resolved per-node interaction-state side map from
    /// the current focused/pressed/hovered trackers. Press wins over
    /// Hover on a same-node match; Hover wins over Focus on a
    /// same-node match (so a keyboard-auto-focused menu item still
    /// gets its hover-lighten when the cursor is over it). Focus
    /// applies on its own when the node isn't pressed or hovered.
    ///
    /// Press is gated on the pointer being currently over the
    /// originally-pressed target — drag the cursor off and the press
    /// visual decays, drag back on and it returns. Mirrors the HTML /
    /// Tailwind `:active` behaviour: the visual reflects "would
    /// release-here activate?", not "was pointer_down captured?".
    /// Drag events still route to `pressed` regardless of pointer
    /// position (see `runtime::pointer_moved`); this gating only
    /// affects the visual envelope.
    pub fn apply_to_state(&mut self) {
        self.node_states.nodes.clear();
        if let Some(target) = &self.focused {
            self.node_states
                .nodes
                .insert(target.node_id.clone(), InteractionState::Focus);
        }
        let press_target = match (&self.pressed, &self.hovered) {
            (Some(pressed), Some(hovered)) if pressed.node_id == hovered.node_id => Some(pressed),
            _ => None,
        };
        if let Some(target) = &self.hovered {
            let pressed_same = press_target
                .map(|p| p.node_id == target.node_id)
                .unwrap_or(false);
            if !pressed_same {
                self.node_states
                    .nodes
                    .insert(target.node_id.clone(), InteractionState::Hover);
            }
        }
        if let Some(target) = press_target {
            self.node_states
                .nodes
                .insert(target.node_id.clone(), InteractionState::Press);
        }
    }

    /// Update the hovered target. Maintains the hover-stable timer
    /// the tooltip pass reads — resets to `now` whenever the hovered
    /// node changes (or hover is gained), clears when it goes away.
    /// Also clears the per-session "tooltip dismissed by press" flag
    /// so the next hover starts fresh.
    ///
    /// Returns `true` when the hovered identity actually changed —
    /// used by [`crate::runtime::RunnerCore::pointer_moved`] to decide
    /// whether the host should redraw (cursor moves *within* the
    /// same hovered node are visual no-ops).
    pub(crate) fn set_hovered(&mut self, new: Option<UiTarget>, now: Instant) -> bool {
        let same = match (&self.hovered, &new) {
            (Some(a), Some(b)) => a.node_id == b.node_id,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.tooltip.hover_started_at = new.as_ref().map(|_| now);
            self.tooltip.dismissed_for_hover = false;
        }
        self.hovered = new;
        !same
    }
}
