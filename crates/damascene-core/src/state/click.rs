//! Multi-click sequence tracking.

use web_time::Instant;

use super::UiState;
use super::types::{ClickSequence, MULTI_CLICK_DIST, MULTI_CLICK_TIME};

impl UiState {
    /// Resolve the click count for a fresh primary-button press at
    /// `(x, y)` and update the runtime's last-click record. Increments
    /// the count when this press extends a multi-click sequence (same
    /// target, within `MULTI_CLICK_TIME` and `MULTI_CLICK_DIST` of the
    /// previous press); otherwise resets to 1.
    pub(crate) fn next_click_count(
        &mut self,
        now: Instant,
        pos: (f32, f32),
        target_node_id: Option<&str>,
    ) -> u8 {
        let mut count = 1;
        if let Some(prev) = self.click.last.as_ref() {
            let dt = now.saturating_duration_since(prev.time);
            let dx = pos.0 - prev.pos.0;
            let dy = pos.1 - prev.pos.1;
            let same_target = match (prev.target_node_id.as_deref(), target_node_id) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if same_target
                && dt < MULTI_CLICK_TIME
                && (dx * dx + dy * dy).sqrt() <= MULTI_CLICK_DIST
            {
                count = prev.count.saturating_add(1);
            }
        }
        self.click.last = Some(ClickSequence {
            time: now,
            pos,
            target_node_id: target_node_id.map(str::to_owned),
            count,
        });
        count
    }

    /// Current click count of the most recent primary press, or 1 if
    /// no press has happened yet. Used by `pointer_up` to stamp the
    /// matching `PointerUp` / `Click` events with the same count their
    /// originating `PointerDown` carried.
    pub(crate) fn current_click_count(&self) -> u8 {
        self.click.last.as_ref().map(|c| c.count).unwrap_or(1)
    }
}
