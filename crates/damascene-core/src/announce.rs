//! Runtime-synthesized screen-reader announcements.
//!
//! Apps push [`Announcement`]s via [`crate::App::drain_announcements`];
//! the runtime stamps each with a monotonic id + a retention deadline,
//! queues it in [`UiState`], and synthesizes an invisible
//! `Kind::Custom("announcements")` layer at the El root each frame —
//! the ARIA live-region pattern. Each queued announcement becomes a
//! zero-size node carrying the message as its accessible *name*
//! (`aria_label`), so nothing paints and layout is untouched, while
//! platform adapters see a freshly-added named node inside a live
//! region and speak it (AT-SPI emits `object:announcement`; other
//! platforms have equivalent events). A new keyed node per
//! announcement means repeating the same message announces again —
//! matching how web live regions behave when content is re-inserted.
//!
//! This mirrors [`crate::toast`]: the tree remains the source of truth
//! at frame end, but the queue is runtime-managed because composing
//! transient live-region nodes by hand each frame would be per-app
//! plumbing for a behaviour every UI shares. Toasts themselves need
//! none of this — the toast card is already a `Role::Status` polite
//! live region, so it announces on arrival.
//!
//! Use announcements for state changes with **no visible focus
//! change**: a background save completing, a long-running export
//! failing, "3 results found" after a filter keystroke. Don't announce
//! what focus movement or a toast already announces — double-speaking
//! is noise to a screen-reader user.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::time::Duration;

use web_time::Instant;

use crate::a11y::{LiveRegion, Role};
use crate::state::UiState;
use crate::tree::*;

/// How long a queued announcement stays mounted in the synthesized
/// live-region layer. The platform adapter announces on the frame the
/// node first appears; retention beyond that only has to survive
/// skipped frames and adapter diffing, so the window is short.
pub const ANNOUNCEMENT_RETENTION: Duration = Duration::from_secs(2);

/// Hard cap on the announcement queue. [`UiState::push_announcement`]
/// drops the oldest entry once full — a runaway producer (announcing
/// per frame) bounds itself instead of growing the tree.
pub const MAX_QUEUED_ANNOUNCEMENTS: usize = 16;

/// One screen-reader announcement, produced from
/// [`crate::App::drain_announcements`]. The runtime stamps an id and
/// retention deadline when it queues the announcement into
/// [`UiState`].
#[derive(Clone, Debug)]
pub struct Announcement {
    /// What the screen reader speaks.
    pub message: String,
    /// How urgently to speak it (ARIA `aria-live` politeness).
    pub politeness: LiveRegion,
}

impl Announcement {
    /// A polite announcement — spoken at the next graceful opportunity
    /// (ARIA `aria-live="polite"`, `role="status"`). The right default
    /// for progress and completion messages.
    pub fn polite(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            politeness: LiveRegion::Polite,
        }
    }

    /// An assertive announcement — interrupts current speech (ARIA
    /// `aria-live="assertive"`, `role="alert"`). Reserve for errors
    /// and anything the user must hear now.
    pub fn assertive(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            politeness: LiveRegion::Assertive,
        }
    }
}

/// A queued announcement — id stamped by the runtime on enqueue (keys
/// the synthesized node so each announcement is a *new* node to the
/// platform adapter), plus the retention deadline.
#[derive(Clone, Debug)]
pub struct QueuedAnnouncement {
    /// Monotonic id stamped on enqueue; keys the synthesized node.
    pub id: u64,
    /// What the screen reader speaks.
    pub message: String,
    /// ARIA politeness carried over from the [`Announcement`].
    pub politeness: LiveRegion,
    /// When the entry leaves the queue (enqueue time +
    /// [`ANNOUNCEMENT_RETENTION`]).
    pub expires_at: Instant,
}

/// Runtime synthesis pass: drop expired announcements, then append an
/// invisible live-region layer if any remain. Called from
/// `prepare_layout` after [`crate::toast::synthesize_toasts`]. Returns
/// `true` while any announcement is queued so the host keeps the
/// redraw loop alive long enough to prune the queue.
///
/// **Root precondition:** same as toasts — the layer is appended as a
/// sibling of whatever the app returned from [`crate::App::build`], so
/// the root must be an `Axis::Overlay` container (`overlays(main,
/// [])`); in a flow container the extra child would disturb gap
/// spacing. Debug builds panic on a non-overlay root.
pub fn synthesize_announcements(root: &mut El, ui_state: &mut UiState, now: Instant) -> bool {
    ui_state.announce.queue.retain(|a| a.expires_at > now);
    if ui_state.announce.queue.is_empty() {
        return false;
    }
    debug_assert_eq!(
        root.axis,
        Axis::Overlay,
        "synthesize_announcements: root must be an Axis::Overlay container so the \
         live-region layer overlays the main view. Wrap your `App::build` return \
         value in `overlays(main, [])`. Got axis = {:?}",
        root.axis,
    );
    let nodes: Vec<El> = ui_state
        .announce
        .queue
        .iter()
        .map(|a| {
            // The message rides as the accessible *name* of a
            // zero-size node: platform adapters announce a named node
            // added inside a live region, and with no text/fill there
            // is nothing to paint or lay out. Status/Alert are the
            // ARIA roles whose implicit politeness matches.
            El::new(Kind::Group)
                .key(format!("announcement-{}", a.id))
                .role(match a.politeness {
                    LiveRegion::Polite => Role::Status,
                    LiveRegion::Assertive => Role::Alert,
                })
                .aria_live(a.politeness)
                .aria_label(a.message.clone())
                .width(Size::Fixed(0.0))
                .height(Size::Fixed(0.0))
        })
        .collect();
    root.children.push(
        El::new(Kind::Custom("announcements"))
            .children(nodes)
            .width(Size::Fixed(0.0))
            .height(Size::Fixed(0.0)),
    );
    let i = root.children.len() - 1;
    crate::layout::assign_id_appended(&root.computed_id, &mut root.children[i], i);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::assign_ids;

    #[test]
    fn synthesize_appends_live_region_nodes() {
        let mut tree = crate::stack(std::iter::empty::<El>());
        let mut state = UiState::new();
        let now = Instant::now();
        state.push_announcement(Announcement::polite("Saved"), now);
        state.push_announcement(Announcement::assertive("Connection lost"), now);

        assign_ids(&mut tree);
        let pending = synthesize_announcements(&mut tree, &mut state, now);
        assert!(pending, "queued announcements → keep redrawing to prune");
        let layer = tree.children.last().expect("layer appended to root");
        assert!(matches!(layer.kind, Kind::Custom("announcements")));
        assert_eq!(layer.children.len(), 2);

        let polite = &layer.children[0];
        let p = polite.a11y.as_deref().expect("a11y props");
        assert_eq!(p.role, Some(Role::Status));
        assert_eq!(p.live, Some(LiveRegion::Polite));
        assert_eq!(p.label.as_deref(), Some("Saved"));
        assert_eq!(polite.key.as_deref(), Some("announcement-0"));

        let assertive = &layer.children[1];
        let a = assertive.a11y.as_deref().expect("a11y props");
        assert_eq!(a.role, Some(Role::Alert));
        assert_eq!(a.live, Some(LiveRegion::Assertive));
        assert_eq!(a.label.as_deref(), Some("Connection lost"));
    }

    #[test]
    fn retention_prunes_and_ids_stay_monotonic() {
        let mut state = UiState::new();
        let now = Instant::now();
        state.push_announcement(Announcement::polite("first"), now);

        // Past retention: the entry is pruned and nothing synthesizes.
        let later = now + ANNOUNCEMENT_RETENTION + Duration::from_millis(1);
        let mut tree = crate::stack(std::iter::empty::<El>());
        assign_ids(&mut tree);
        let pending = synthesize_announcements(&mut tree, &mut state, later);
        assert!(!pending);
        assert!(tree.children.is_empty(), "no layer for an empty queue");

        // A later announcement gets a fresh id — the platform adapter
        // must see a *new* node even for a repeated message.
        state.push_announcement(Announcement::polite("first"), later);
        assert_eq!(state.announce.queue[0].id, 1, "monotonic across prunes");
    }

    #[test]
    fn queue_is_bounded() {
        let mut state = UiState::new();
        let now = Instant::now();
        for i in 0..(MAX_QUEUED_ANNOUNCEMENTS + 3) {
            state.push_announcement(Announcement::polite(format!("m{i}")), now);
        }
        assert_eq!(state.announce.queue.len(), MAX_QUEUED_ANNOUNCEMENTS);
        assert_eq!(
            state.announce.queue[0].message, "m3",
            "oldest entries dropped first"
        );
    }
}
