//! Runtime toast queue helpers for [`UiState`](super::UiState).

use web_time::Instant;

use super::UiState;

impl UiState {
    /// Queue a toast for the next frame. Stamps an `id` (monotonic)
    /// and computes the `expires_at` deadline from `now + spec.ttl`.
    /// The runtime re-walks the queue each frame and drops expired
    /// entries before synthesizing the toast layer.
    pub fn push_toast(&mut self, spec: crate::toast::ToastSpec, now: Instant) {
        let id = self.toast.next_id;
        self.toast.next_id = self.toast.next_id.wrapping_add(1);
        self.toast.queue.push(crate::toast::Toast {
            id,
            level: spec.level,
            message: spec.message,
            expires_at: now + spec.ttl,
        });
    }

    /// Remove the toast with the given id. Used by the runtime when
    /// the user clicks a `toast-dismiss-{id}` button; apps that want
    /// to programmatically cancel a toast can call this directly via
    /// the `Runner::dismiss_toast` host accessor.
    pub fn dismiss_toast(&mut self, id: u64) {
        self.toast.queue.retain(|t| t.id != id);
    }

    /// Read-only view of the current toast queue (post-expiry).
    /// Used by hosts that want to drive cursor / accessibility state
    /// from the visible stack.
    pub fn toasts(&self) -> &[crate::toast::Toast] {
        &self.toast.queue
    }
}
