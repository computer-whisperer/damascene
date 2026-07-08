//! Runtime toast queue helpers for [`UiState`](super::UiState).

use web_time::Instant;

use super::UiState;

impl UiState {
    /// Queue a toast for the next frame. Stamps an `id` (monotonic)
    /// and computes the `expires_at` deadline from `now + spec.ttl`.
    /// The runtime re-walks the queue each frame and drops expired
    /// entries before synthesizing the toast layer.
    ///
    /// The queue is bounded at [`crate::toast::MAX_QUEUED_TOASTS`]:
    /// pushing onto a full queue drops the oldest entry, so a runaway
    /// producer (a reconnect loop pushing long-TTL toasts, say) can't
    /// grow it without bound. Only the newest
    /// [`crate::toast::MAX_VISIBLE_TOASTS`] render at once.
    pub fn push_toast(&mut self, spec: crate::toast::ToastSpec, now: Instant) {
        if self.toast.queue.len() >= crate::toast::MAX_QUEUED_TOASTS {
            self.toast.queue.remove(0);
        }
        let id = self.toast.next_id;
        self.toast.next_id = self.toast.next_id.wrapping_add(1);
        self.toast.queue.push(crate::toast::Toast {
            id,
            level: spec.level,
            message: spec.message,
            expires_at: now + spec.ttl,
            dismissed: false,
            exit_started: None,
        });
    }

    /// Dismiss the toast with the given id. Used by the runtime when
    /// the user clicks a `toast-dismiss-{id}` button; apps that want
    /// to programmatically cancel a toast can call this directly via
    /// the `Runner::dismiss_toast` host accessor. The toast stays in
    /// the queue through its exit animation
    /// ([`crate::toast::TOAST_EXIT_WINDOW`]) and is removed by the
    /// next synthesis passes.
    pub fn dismiss_toast(&mut self, id: u64) {
        if let Some(t) = self.toast.queue.iter_mut().find(|t| t.id == id) {
            t.dismissed = true;
        }
    }

    /// Read-only view of the current toast queue (post-expiry,
    /// including toasts still playing their exit animation). Used by
    /// hosts that want to drive cursor / accessibility state from the
    /// visible stack.
    pub fn toasts(&self) -> &[crate::toast::Toast] {
        &self.toast.queue
    }
}
