//! Runtime-synthesized toast notifications.
//!
//! Apps push toasts via [`crate::App::drain_toasts`]; the runtime stamps each
//! with a monotonic id + an expiry, queues it in [`UiState`],
//! and synthesizes a `Kind::Custom("toast_stack")` floating layer at
//! the El root each frame. The layer is bottom-right anchored, hit-test
//! transparent except for the per-toast dismiss button (which the
//! runtime intercepts in `pointer_up` and removes the toast on).
//!
//! This mirrors [`crate::tooltip`]: tree is the source of truth at
//! frame end, but the *triggers* (hover for tooltips, fire-and-forget
//! for toasts) are runtime-managed because composing them by hand each
//! frame would be a lot of per-app plumbing for a behaviour every UI
//! shares.

use std::time::Duration;

use web_time::Instant;

use crate::state::UiState;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::button;

/// Default time a toast stays on screen before auto-dismissing.
/// Matches the shadcn / Sonner default. Apps override per-toast via
/// [`ToastSpec::with_ttl`].
pub const DEFAULT_TOAST_TTL: Duration = Duration::from_secs(4);

/// Severity / variant for a toast. Drives the leading icon and the
/// surface accent colour. Mirrors the shadcn `<Toast variant="...">`
/// vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Default,
    Success,
    Warning,
    Error,
    Info,
}

/// What the app produces from [`crate::App::drain_toasts`]. The
/// runtime stamps an `id` + computes `expires_at` when it queues
/// the toast into [`UiState`]'s runtime queue.
#[derive(Clone, Debug)]
pub struct ToastSpec {
    pub level: ToastLevel,
    pub message: String,
    pub ttl: Duration,
}

impl ToastSpec {
    pub fn new(level: ToastLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            ttl: DEFAULT_TOAST_TTL,
        }
    }
    pub fn default(message: impl Into<String>) -> Self {
        Self::new(ToastLevel::Default, message)
    }
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(ToastLevel::Success, message)
    }
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(ToastLevel::Warning, message)
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(ToastLevel::Error, message)
    }
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(ToastLevel::Info, message)
    }
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }
}

/// A queued toast — id stamped by the runtime on enqueue, used both
/// as the dismiss-button suffix and to drop the right entry when
/// the X is clicked or the TTL elapses.
#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub level: ToastLevel,
    pub message: String,
    pub expires_at: Instant,
}

/// Runtime synthesis pass: drop expired toasts, then append a
/// floating `toast_stack` layer if any remain. Called from
/// `prepare_layout` after [`crate::tooltip::synthesize_tooltip`].
/// Returns `true` while any toast is pending so the host keeps the
/// redraw loop alive long enough to drop the next-to-expire toast.
///
/// **Root precondition:** the synthesized layer is appended as a
/// sibling of whatever the app returned from [`crate::App::build`].
/// For it to overlay (rather than compete for flex space) the root
/// must be an `Axis::Overlay` container — typically `overlays(main,
/// [])`, which is the same convention apps use for user-composed
/// popovers and modals. Debug builds panic on a non-overlay root.
pub fn synthesize_toasts(root: &mut El, ui_state: &mut UiState, now: Instant) -> bool {
    ui_state.toast.queue.retain(|t| t.expires_at > now);
    if ui_state.toast.queue.is_empty() {
        return false;
    }
    debug_assert_eq!(
        root.axis,
        Axis::Overlay,
        "synthesize_toasts: root must be an Axis::Overlay container so the toast \
         stack overlays the main view. Wrap your `App::build` return value in \
         `overlays(main, [])`. Got axis = {:?}",
        root.axis,
    );
    let cards: Vec<El> = ui_state.toast.queue.iter().map(toast_card).collect();
    root.children.push(toast_stack(cards));
    // Assign computed_ids to the pushed layer in-place so the
    // subsequent `layout_post_assign` doesn't have to re-walk the
    // whole tree. Pairs with `RunnerCore::prepare_layout`'s
    // skip-the-second-id-walk flow.
    let i = root.children.len() - 1;
    crate::layout::assign_id_appended(&root.computed_id, &mut root.children[i], i);
    true
}

/// Bottom-right anchored stack. Uses a custom layout function that
/// pulls the *root* (viewport) rect via `rect_of_id("root")` and
/// places each card at the bottom-right corner, stacking newest at
/// the bottom. This makes the layer immune to whatever flow
/// (`column` / `row` / overlay) the user picked at root — it always
/// floats over the entire viewport, like a real toast notification.
fn toast_stack(cards: Vec<El>) -> El {
    El::new(Kind::Custom("toast_stack"))
        .children(cards)
        .fill_size()
        .layout(|ctx| {
            let viewport = (ctx.rect_of_id)("root").unwrap_or(ctx.container);
            let pad = tokens::SPACE_4;
            let gap = tokens::SPACE_2;
            let mut rects = Vec::with_capacity(ctx.children.len());
            // Newest toast (last in `children`) renders at the bottom;
            // earlier toasts pile upward above it.
            let mut bottom = viewport.bottom() - pad;
            for c in ctx.children.iter().rev() {
                let (w, h) = (ctx.measure)(c);
                let x = viewport.right() - w - pad;
                rects.push(Rect::new(x, bottom - h, w, h));
                bottom -= h + gap;
            }
            rects.reverse();
            rects
        })
}

/// One toast card — surface with level-coloured leading bar, message
/// text, and a dismiss button keyed `toast-dismiss-{id}` so the
/// runtime can recognize and remove it on click. The leading bar is
/// `Align::Stretch` so it fills the card's vertical extent.
fn toast_card(t: &Toast) -> El {
    let accent = level_accent(t.level);
    let lead = El::new(Kind::Group)
        .width(Size::Fixed(3.0))
        .height(Size::Fill(1.0))
        .fill(accent)
        .radius(tokens::RADIUS_SM);
    let body = El::new(Kind::Text)
        .text(t.message.clone())
        .text_role(TextRole::Body)
        .text_color(tokens::FOREGROUND)
        .text_wrap(TextWrap::Wrap)
        .width(Size::Fill(1.0));
    let dismiss = button("×")
        .key(format!("toast-dismiss-{}", t.id))
        .secondary();

    El::new(Kind::Custom("toast_card"))
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Popover)
        .axis(Axis::Row)
        .align(Align::Stretch)
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_3)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .shadow(tokens::SHADOW_MD)
        .width(Size::Fixed(360.0))
        .height(Size::Hug)
        .children([lead, body, dismiss])
}

fn level_accent(level: ToastLevel) -> Color {
    match level {
        ToastLevel::Default => tokens::INPUT,
        ToastLevel::Success => tokens::SUCCESS,
        ToastLevel::Warning => tokens::WARNING,
        ToastLevel::Error => tokens::DESTRUCTIVE,
        ToastLevel::Info => tokens::INFO,
    }
}

/// Parse the toast id out of a `toast-dismiss-{id}` button key.
/// Returns `None` for keys that don't match the toast-dismiss
/// convention. Used by the runtime to intercept dismiss clicks.
pub fn parse_dismiss_key(key: &str) -> Option<u64> {
    key.strip_prefix("toast-dismiss-")
        .and_then(|rest| rest.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{assign_ids, layout};

    #[test]
    fn synthesize_appends_layer_per_active_toast() {
        let mut tree = crate::stack(std::iter::empty::<El>());
        let mut state = UiState::new();
        let now = Instant::now();
        state.push_toast(ToastSpec::success("Saved"), now);
        state.push_toast(ToastSpec::error("Failed"), now);

        assign_ids(&mut tree);
        let pending = synthesize_toasts(&mut tree, &mut state, now);
        assert!(pending, "active toasts → caller should request redraw");
        let stack = tree.children.last().expect("toast_stack appended to root");
        assert!(matches!(stack.kind, Kind::Custom("toast_stack")));
        assert_eq!(stack.children.len(), 2);
    }

    #[test]
    fn synthesize_drops_expired_toasts() {
        let mut tree = crate::stack(std::iter::empty::<El>());
        let mut state = UiState::new();
        let t0 = Instant::now();
        // Old TTL: already gone. New TTL: still fresh.
        state.push_toast(
            ToastSpec::info("old").with_ttl(Duration::from_millis(10)),
            t0,
        );
        state.push_toast(ToastSpec::info("new").with_ttl(Duration::from_secs(60)), t0);
        let later = t0 + Duration::from_secs(1);
        let pending = synthesize_toasts(&mut tree, &mut state, later);
        assert!(pending);
        assert_eq!(state.toast.queue.len(), 1, "expired toast dropped");
        assert_eq!(state.toast.queue[0].message, "new");
    }

    #[test]
    fn synthesize_returns_false_when_no_toasts() {
        let mut tree = crate::stack(std::iter::empty::<El>());
        let mut state = UiState::new();
        let pending = synthesize_toasts(&mut tree, &mut state, Instant::now());
        assert!(!pending);
        assert!(tree.children.is_empty());
    }

    #[test]
    fn parse_dismiss_key_round_trip() {
        assert_eq!(parse_dismiss_key("toast-dismiss-7"), Some(7));
        assert_eq!(parse_dismiss_key("toast-dismiss-0"), Some(0));
        assert_eq!(parse_dismiss_key("save"), None);
        assert_eq!(parse_dismiss_key("toast-dismiss-abc"), None);
    }

    #[test]
    fn toast_stack_layer_lays_out_at_root() {
        let mut tree = crate::stack(std::iter::empty::<El>()).fill_size();
        let mut state = UiState::new();
        let now = Instant::now();
        state.push_toast(ToastSpec::default("hello"), now);
        synthesize_toasts(&mut tree, &mut state, now);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));
        // The toast_stack layer occupies the full viewport so its
        // children can be bottom-right anchored.
        let stack = tree.children.last().unwrap();
        let r = state.rect(&stack.computed_id);
        assert!((r.w - 800.0).abs() < 0.01);
        assert!((r.h - 600.0).abs() < 0.01);
    }
}
