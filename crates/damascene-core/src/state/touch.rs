//! Live touch-contact registry and primary-contact routing.
//!
//! The interaction pipeline (`pointer_down` / `pointer_moved` /
//! `pointer_up` in [`crate::runtime`]) is single-pointer: one `pressed`
//! target, one gesture state machine, one capture slot per surface.
//! Multi-touch hosts forward *every* finger through those entry points,
//! so without routing a second finger re-runs the whole press cascade
//! and corrupts whatever gesture the first finger is mid-way through.
//!
//! The registry resolves this with DOM `isPrimary` semantics: contacts
//! are tracked in arrival order, the first-arrived live contact is the
//! *primary* and flows through the pipeline unchanged, and every other
//! contact only keeps its registry entry fresh — the input the pinch
//! recognizer reads. A custom host that passes
//! [`PointerId::PRIMARY`](crate::event::PointerId::PRIMARY) for every
//! finger degrades gracefully: all events map to one registry entry,
//! which is always primary, reproducing the pre-registry behavior.

use web_time::Instant;

use super::UiState;
use super::types::TouchContact;
use crate::event::PointerId;

impl UiState {
    /// Register a touch contact at `pointer_down`. Returns `true` when
    /// the contact is the *primary* contact (first-arrived live), i.e.
    /// when the down should run the normal press cascade. A down for
    /// an id that is already live refreshes it in place and keeps its
    /// arrival rank — defensive against host echo, and the degradation
    /// path for single-id hosts.
    pub(crate) fn touch_contact_down(
        &mut self,
        id: PointerId,
        pos: (f32, f32),
        now: Instant,
    ) -> bool {
        if let Some(i) = self.touch_contacts.iter().position(|c| c.id == id) {
            self.touch_contacts[i].pos = pos;
            return i == 0;
        }
        self.touch_contacts.push(TouchContact {
            id,
            pos,
            down_at: now,
        });
        self.touch_contacts.len() == 1
    }

    /// Refresh a touch contact's position at `pointer_moved`. Returns
    /// `true` when the move belongs on the single-pointer pipeline:
    /// the primary contact's moves, or — when no contact is live at
    /// all — a contactless touch move (synthetic events, hosts that
    /// move without a down), which keeps its historical behavior of
    /// flowing through with the touch-without-press hover gating.
    /// A move for an unknown id while other contacts are live is
    /// dropped: it can only be a finger the registry never saw (e.g.
    /// one that went down before a global cancel), and routing it
    /// would jerk the primary's gesture.
    pub(crate) fn touch_contact_moved(&mut self, id: PointerId, pos: (f32, f32)) -> bool {
        if self.touch_contacts.is_empty() {
            return true;
        }
        match self.touch_contacts.iter().position(|c| c.id == id) {
            Some(i) => {
                self.touch_contacts[i].pos = pos;
                i == 0
            }
            None => false,
        }
    }

    /// Remove a touch contact at `pointer_up`. Same routing contract
    /// as [`Self::touch_contact_moved`]: only the primary contact's
    /// release runs release semantics (click confirmation, box-zoom
    /// apply, gesture-machine teardown). A secondary finger lifting
    /// mid-gesture must not end the primary's capture. When the
    /// primary lifts while other fingers remain, the next-oldest
    /// contact becomes primary for subsequent events — its moves flow
    /// through the pipeline as contactless-press moves (no `pressed`,
    /// hover gated), and its own release runs the (empty) release
    /// path.
    pub(crate) fn touch_contact_up(&mut self, id: PointerId) -> bool {
        if self.touch_contacts.is_empty() {
            return true;
        }
        match self.touch_contacts.iter().position(|c| c.id == id) {
            Some(i) => {
                self.touch_contacts.remove(i);
                i == 0
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use web_time::Instant;

    use crate::event::PointerId;
    use crate::state::UiState;

    #[test]
    fn arrival_order_decides_primary_and_promotes_on_lift() {
        let mut ui = UiState::default();
        let now = Instant::now();
        assert!(ui.touch_contact_down(PointerId(7), (10.0, 10.0), now));
        assert!(!ui.touch_contact_down(PointerId(9), (50.0, 50.0), now));
        // Only the first-arrived contact routes.
        assert!(ui.touch_contact_moved(PointerId(7), (12.0, 12.0)));
        assert!(!ui.touch_contact_moved(PointerId(9), (60.0, 60.0)));
        // Registry keeps both positions fresh regardless of routing.
        assert_eq!(ui.touch_contacts[0].pos, (12.0, 12.0));
        assert_eq!(ui.touch_contacts[1].pos, (60.0, 60.0));
        // Secondary lifting doesn't route; primary lifting does.
        assert!(!ui.touch_contact_up(PointerId(9)));
        assert!(ui.touch_contact_up(PointerId(7)));
        assert!(ui.touch_contacts.is_empty());
    }

    #[test]
    fn primary_lift_promotes_next_oldest() {
        let mut ui = UiState::default();
        let now = Instant::now();
        assert!(ui.touch_contact_down(PointerId(1), (0.0, 0.0), now));
        assert!(!ui.touch_contact_down(PointerId(2), (5.0, 5.0), now));
        assert!(ui.touch_contact_up(PointerId(1)));
        // The survivor is now primary: its events route.
        assert!(ui.touch_contact_moved(PointerId(2), (6.0, 6.0)));
        assert!(ui.touch_contact_up(PointerId(2)));
    }

    #[test]
    fn single_id_hosts_degrade_to_primary_for_everything() {
        // A custom host that passes PointerId::PRIMARY for every finger
        // collapses to one registry entry that is always primary —
        // exactly the pre-registry behavior.
        let mut ui = UiState::default();
        let now = Instant::now();
        assert!(ui.touch_contact_down(PointerId::PRIMARY, (0.0, 0.0), now));
        assert!(ui.touch_contact_down(PointerId::PRIMARY, (9.0, 9.0), now));
        assert_eq!(ui.touch_contacts.len(), 1);
        assert!(ui.touch_contact_moved(PointerId::PRIMARY, (11.0, 11.0)));
        assert!(ui.touch_contact_up(PointerId::PRIMARY));
    }

    #[test]
    fn contactless_touch_moves_route_while_unknown_ids_drop() {
        let mut ui = UiState::default();
        // No live contact: a stray touch move keeps flowing (historical
        // behavior for synthetic events / hosts without downs).
        assert!(ui.touch_contact_moved(PointerId(3), (1.0, 1.0)));
        assert!(ui.touch_contacts.is_empty());
        // With a live contact, an id the registry never saw is dropped.
        let now = Instant::now();
        assert!(ui.touch_contact_down(PointerId(1), (0.0, 0.0), now));
        assert!(!ui.touch_contact_moved(PointerId(3), (2.0, 2.0)));
        // Same for a stray up: dropped, primary keeps its gesture.
        assert!(!ui.touch_contact_up(PointerId(3)));
        assert_eq!(ui.touch_contacts.len(), 1);
    }
}
