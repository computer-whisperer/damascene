//! Runtime announcement-queue helpers for [`UiState`](super::UiState).

use web_time::Instant;

use super::UiState;

impl UiState {
    /// Queue a screen-reader announcement for the next frame. Stamps a
    /// monotonic `id` (which keys the synthesized live-region node, so
    /// the platform adapter sees a *new* node even for a repeated
    /// message) and computes the retention deadline from `now +`
    /// [`ANNOUNCEMENT_RETENTION`](crate::announce::ANNOUNCEMENT_RETENTION).
    ///
    /// The queue is bounded at
    /// [`MAX_QUEUED_ANNOUNCEMENTS`](crate::announce::MAX_QUEUED_ANNOUNCEMENTS):
    /// pushing onto a full queue drops the oldest entry.
    pub fn push_announcement(&mut self, a: crate::announce::Announcement, now: Instant) {
        if self.announce.queue.len() >= crate::announce::MAX_QUEUED_ANNOUNCEMENTS {
            self.announce.queue.remove(0);
        }
        let id = self.announce.next_id;
        self.announce.next_id = self.announce.next_id.wrapping_add(1);
        self.announce
            .queue
            .push(crate::announce::QueuedAnnouncement {
                id,
                message: a.message,
                politeness: a.politeness,
                expires_at: now + crate::announce::ANNOUNCEMENT_RETENTION,
            });
    }

    /// Read-only view of the pending announcement queue (entries still
    /// inside their retention window). Used by tests and hosts that
    /// mirror announcements into platform-specific channels.
    pub fn announcements(&self) -> &[crate::announce::QueuedAnnouncement] {
        &self.announce.queue
    }
}
