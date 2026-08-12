//! Resize handle — a thin draggable bar between siblings that adjusts
//! their sizes. Use it to build a movable divider between a sidebar and
//! a main pane, or between two weighted panes in a split view.
//!
//! The handle is a sibling primitive, not a container wrapper. Drop it
//! between two siblings inside any `row()` / `column()`; the app owns
//! the size state and folds drag events back through
//! [`apply_event_fixed`] / [`apply_event_weights`] (or a custom handler
//! built on [`delta_from_event`]).
//!
//! For the common pinned-sidebar case, consider
//! [`El::user_resizable`](crate::tree::El::user_resizable) first: it
//! needs no divider widget and no app state — the runtime hit-tests an
//! invisible band on the pane's seam edge and keeps the dragged width
//! like a scroll offset. Reach for `resize_handle` when you want a
//! *visible* divider, keyboard accessibility (the handle is focusable
//! and arrow-key driven; `.user_resizable()` is pointer-only like CSS
//! `resize`), a weighted split ([`apply_event_weights`]), or custom
//! redistribution logic.
//!
//! # Pinned sidebar (one fixed-pixel pane + one filling pane)
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use damascene_core::widgets::resize_handle::{self, ResizeDrag, Side};
//!
//! struct Editor {
//!     sidebar_w: f32,
//!     sidebar_drag: ResizeDrag,
//! }
//!
//! impl Default for Editor {
//!     fn default() -> Self {
//!         Self {
//!             sidebar_w: tokens::SIDEBAR_WIDTH,
//!             sidebar_drag: ResizeDrag::default(),
//!         }
//!     }
//! }
//!
//! impl App for Editor {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         row([
//!             file_tree().width(Size::Fixed(self.sidebar_w)),
//!             resize_handle("sidebar:resize", Axis::Row),
//!             editor_pane().width(Size::Fill(1.0)),
//!         ])
//!         .height(Size::Fill(1.0))
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         resize_handle::apply_event_fixed(
//!             &mut self.sidebar_w,
//!             &mut self.sidebar_drag,
//!             &event,
//!             "sidebar:resize",
//!             Axis::Row,
//!             Side::Start,
//!             tokens::SIDEBAR_WIDTH_MIN,
//!             tokens::SIDEBAR_WIDTH_MAX,
//!         );
//!     }
//! }
//! ```
//!
//! Pass `Side::End` instead when the controlled value lives on the
//! right (Row) or bottom (Column) of the handle — e.g. an inspector
//! pane pinned to the right edge of a row, with a filling editor on
//! its left. Drag direction and the Arrow / Home / End keys flip
//! accordingly so drag-left grows a right-anchored pane.
//!
//! # Weighted split (two `Fill` siblings sharing a parent)
//!
//! ```ignore
//! struct Diff {
//!     weights: [f32; 2],
//!     drag: ResizeWeightsDrag,
//!     row_width: f32, // captured from the previous frame's layout
//! }
//!
//! impl App for Diff {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         row([
//!             left().width(Size::Fill(self.weights[0])),
//!             resize_handle("diff:split", Axis::Row),
//!             right().width(Size::Fill(self.weights[1])),
//!         ])
//!         .key("diff:row")
//!         .height(Size::Fill(1.0))
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         resize_handle::apply_event_weights(
//!             &mut self.weights,
//!             &mut self.drag,
//!             &event,
//!             "diff:split",
//!             Axis::Row,
//!             self.row_width,
//!             0.15, // each pane keeps at least 15% of the row
//!         );
//!     }
//! }
//! ```
//!
//! Apps that need finer control (multi-pane redistribution, snapping,
//! collapse-on-min) should fold [`delta_from_event`] into their own
//! handler.
//!
//! # Focus ring
//!
//! The handle uses an inside focus ring ([`El::focus_ring_inside`]),
//! like menu rows: it sits flush in the seam between panes by design,
//! so an outside ring's bleed would be occluded by whichever flush
//! neighbor paints later (issue #48). No `.focus_ring_inside()` is
//! needed at call sites.
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface (`Kind::Custom`,
//! `.focusable()`, `.focus_ring_inside()`). No privileged internals.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::cursor::Cursor;
use crate::event::{NamedKey, UiEvent, UiEventKind};
use crate::tokens;
use crate::tree::*;

/// Thickness of the handle's interactive bar in logical pixels. The
/// hit area is intentionally wider than the painted hairline so the
/// pointer doesn't have to land pixel-perfect — comparable to
/// VS Code's ~6–8px and Slack's ~10px native handles, and roughly
/// double shadcn's ~5px effective hit area (`w-px` + `after:w-1`).
pub const HANDLE_THICKNESS: f32 = 8.0;

/// Visual width of the painted hairline inside the wider hit area.
const HAIRLINE_THICKNESS: f32 = 2.0;

/// Pixels the value moves per Arrow key press when the handle is
/// keyboard-focused. Small enough for fine alignment, large enough
/// that a held-down arrow makes visible progress.
pub const KEYBOARD_STEP_PX: f32 = 8.0;

/// Pixels the value moves per PageUp / PageDown press.
pub const KEYBOARD_PAGE_STEP_PX: f32 = 40.0;

/// A thin draggable bar that lives between two siblings. `key` routes
/// the pointer / drag / key events — fold them through one of the
/// [`apply_event_fixed`] or [`apply_event_weights`] helpers. `axis` is
/// the container axis the handle resizes along — `Axis::Row` for a
/// vertical bar in a row of panes (drags left/right), `Axis::Column`
/// for a horizontal bar in a column (drags up/down).
#[track_caller]
pub fn resize_handle(key: impl Into<String>, axis: Axis) -> El {
    let (width, height) = match axis {
        Axis::Row => (Size::Fixed(HANDLE_THICKNESS), Size::Fill(1.0)),
        Axis::Column => (Size::Fill(1.0), Size::Fixed(HANDLE_THICKNESS)),
        Axis::Overlay => (Size::Fixed(HANDLE_THICKNESS), Size::Fixed(HANDLE_THICKNESS)),
    };
    let hairline = match axis {
        Axis::Row => El::new(Kind::Custom("resize-handle-hairline"))
            .width(Size::Fixed(HAIRLINE_THICKNESS))
            .height(Size::Fill(1.0))
            .fill(tokens::BORDER)
            // Hit-test lands on the focusable outer wrapper; without
            // the cascade the hairline would never lighten on hover or
            // darken under a drag.
            .state_follows_interactive_ancestor(),
        Axis::Column | Axis::Overlay => El::new(Kind::Custom("resize-handle-hairline"))
            .width(Size::Fill(1.0))
            .height(Size::Fixed(HAIRLINE_THICKNESS))
            .fill(tokens::BORDER)
            .state_follows_interactive_ancestor(),
    };
    // The cursor matches the drag axis: a Row-axis handle is a
    // vertical bar that slides left/right (EwResize), a Column-axis
    // handle is a horizontal bar that slides up/down (NsResize).
    // Overlay isn't a real layout case — just fall back to EwResize.
    let cursor = match axis {
        Axis::Row => Cursor::EwResize,
        Axis::Column => Cursor::NsResize,
        Axis::Overlay => Cursor::EwResize,
    };
    // No `capture_keys()` — Tab must keep traversing past the handle.
    // Arrow / PageUp / PageDown / Home / End still arrive as `KeyDown`
    // events on the focused handle by default, which `apply_event_*`
    // folds into the size value.
    //
    // Center the hairline within the wider hit-area so the focus ring
    // wraps the visible hairline symmetrically. Without this the
    // overlay default (`Align::Stretch`) pins the Fixed-thickness
    // hairline to one edge and the ring looks visibly offset from the
    // line it surrounds.
    //
    // The ring is placed *inside* the hit-area, like menu rows: a
    // resize handle sits flush in the seam between panes, so there is
    // structurally never room for an outside ring's bleed — a
    // later-painted flush sibling would occlude it (caught by the
    // `FocusRingObscured` lint). The 3px margin between hit-area edge
    // and hairline gives the inside bands room without covering the
    // hairline itself.
    El::new(Kind::Custom("resize-handle"))
        .axis(Axis::Overlay)
        .child(hairline)
        .at_loc(Location::caller())
        .key(key)
        // ARIA focusable-separator pattern; the handle paints only a
        // hairline, so give it an accessible name too.
        .role(Role::Separator)
        .aria_label("Resize")
        .align(Align::Center)
        .justify(Justify::Center)
        .focusable()
        .focus_ring_inside()
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(cursor)
        // Touch drag is the whole point of a resize handle. Without
        // this opt-in the runner would cancel the press and route
        // the motion to scroll, leaving the handle inert on touch.
        .consumes_touch_drag()
        .width(width)
        .height(height)
}

/// Drag-anchor state for [`apply_event_fixed`]. Lives in the app
/// struct alongside the size value the handle controls; default-init
/// it (`ResizeDrag::default()`) and pass `&mut`.
///
/// `anchor` is the pointer position captured at PointerDown along the
/// handle's resize axis; `initial` is the size value at that moment.
/// Combining the two with the current pointer position gives an
/// absolute target size each frame, so drags don't accumulate float
/// rounding error across many `Drag` events.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResizeDrag {
    /// Pointer position along the resize axis at PointerDown; `None`
    /// when no drag is in progress.
    pub anchor: Option<f32>,
    /// The controlled size value at the moment the drag began.
    pub initial: f32,
}

/// Which side of the handle the controlled value lives on.
///
/// For `Side::Start` (left of a Row handle, top of a Column handle),
/// drag-right or drag-down grows the value: a left-anchored sidebar
/// gets wider as the handle moves right. For `Side::End` (right of a
/// Row handle, bottom of a Column handle), the relationship flips:
/// drag-left or drag-up grows the value, since the right-anchored
/// pane gains pixels as the handle's position recedes from the
/// right edge. Arrow / PageUp / PageDown / Home / End all mirror the
/// same flip so keyboard nudges feel symmetric to the drag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// Left of a Row handle, top of a Column handle.
    #[default]
    Start,
    /// Right of a Row handle, bottom of a Column handle.
    End,
}

impl Side {
    /// Sign multiplier for "pointer / arrow movement → value delta."
    /// `+1.0` for `Start` (drag-right grows), `-1.0` for `End`
    /// (drag-left grows).
    fn sign(self) -> f32 {
        match self {
            Side::Start => 1.0,
            Side::End => -1.0,
        }
    }
}

/// Drag-anchor state for [`apply_event_weights`]. Captures the pointer
/// position and the pair of weights at the moment the drag began so
/// the helper can recompute absolute target weights each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResizeWeightsDrag {
    /// Pointer position along the resize axis at PointerDown; `None`
    /// when no drag is in progress.
    pub anchor: Option<f32>,
    /// The `[left, right]` weights at the moment the drag began.
    pub initial: [f32; 2],
}

/// Project a 2D pointer position onto the resize axis.
fn project(pos: (f32, f32), axis: Axis) -> f32 {
    match axis {
        Axis::Row | Axis::Overlay => pos.0,
        Axis::Column => pos.1,
    }
}

/// Pixel delta from drag anchor to the event's current pointer
/// position along `axis`. Returns `None` if the drag hasn't started
/// (PointerDown not seen) or the event carries no pointer.
///
/// Useful for apps that want to roll their own redistribution logic
/// (multi-pane, snapping, collapse-to-zero). Most apps should use
/// [`apply_event_fixed`] or [`apply_event_weights`] directly.
pub fn delta_from_event(drag: &ResizeDrag, event: &UiEvent, axis: Axis) -> Option<f32> {
    let anchor = drag.anchor?;
    let pos = event.pointer?;
    Some(project(pos, axis) - anchor)
}

/// Fold a routed event into a fixed-pixel size value (e.g. a sidebar
/// width). Returns `true` when the value changed.
///
/// Handles the full drag lifecycle: PointerDown captures the anchor,
/// Drag updates the value, PointerUp clears the anchor. Arrow keys on
/// the focused handle nudge by [`KEYBOARD_STEP_PX`]; PageUp /
/// PageDown by [`KEYBOARD_PAGE_STEP_PX`]; Home / End jump to one
/// extreme.
///
/// `axis` must match the axis the handle was constructed with —
/// `Axis::Row` for a sidebar in a row, `Axis::Column` for a top pane
/// in a column. `side` says which side of the handle the value
/// lives on; `Side::Start` for the common left/top-anchored case,
/// `Side::End` to flip drag and keyboard direction for a right- or
/// bottom-anchored pane.
#[allow(clippy::too_many_arguments)]
pub fn apply_event_fixed(
    value: &mut f32,
    drag: &mut ResizeDrag,
    event: &UiEvent,
    key: &str,
    axis: Axis,
    side: Side,
    min: f32,
    max: f32,
) -> bool {
    if event.route() != Some(key) {
        return false;
    }
    match event.kind {
        UiEventKind::PointerDown => {
            if let Some(pos) = event.pointer {
                drag.anchor = Some(project(pos, axis));
                drag.initial = *value;
            }
            false
        }
        UiEventKind::Drag => {
            let Some(anchor) = drag.anchor else {
                return false;
            };
            let Some(pos) = event.pointer else {
                return false;
            };
            let pixel_delta = (project(pos, axis) - anchor) * side.sign();
            let next = (drag.initial + pixel_delta).clamp(min, max);
            let changed = (next - *value).abs() > f32::EPSILON;
            *value = next;
            changed
        }
        UiEventKind::PointerUp => {
            drag.anchor = None;
            false
        }
        UiEventKind::KeyDown => apply_key(value, event, side, min, max),
        _ => false,
    }
}

/// Fold a routed event into a `[left_weight, right_weight]` pair that
/// share a parent's main-axis extent. Returns `true` when the
/// weights changed.
///
/// `parent_main_extent` is the parent's width (Row) or height
/// (Column) in logical pixels — the helper needs it to convert the
/// pointer's pixel delta into a weight delta. Capture it after each
/// frame's layout via `UiState::rect_of_key("parent:key")` and feed
/// it back here.
///
/// `min_weight` clamps each side so a pane can't be dragged below it.
/// Pass a small fraction (e.g. `0.15`) to leave room for the pane's
/// content.
pub fn apply_event_weights(
    weights: &mut [f32; 2],
    drag: &mut ResizeWeightsDrag,
    event: &UiEvent,
    key: &str,
    axis: Axis,
    parent_main_extent: f32,
    min_weight: f32,
) -> bool {
    if event.route() != Some(key) {
        return false;
    }
    match event.kind {
        UiEventKind::PointerDown => {
            if let Some(pos) = event.pointer {
                drag.anchor = Some(project(pos, axis));
                drag.initial = *weights;
            }
            false
        }
        UiEventKind::Drag => {
            let Some(anchor) = drag.anchor else {
                return false;
            };
            let Some(pos) = event.pointer else {
                return false;
            };
            if parent_main_extent <= 0.0 {
                return false;
            }
            let total = drag.initial[0] + drag.initial[1];
            if total <= 0.0 {
                return false;
            }
            // Pixel delta → weight delta, scaled by the parent's
            // weight density (total weight per pixel of shared extent).
            let pixel_delta = project(pos, axis) - anchor;
            let weight_delta = pixel_delta * (total / parent_main_extent);
            let lo = min_weight.max(0.0);
            let hi = (total - lo).max(lo);
            let next_left = (drag.initial[0] + weight_delta).clamp(lo, hi);
            let next_right = total - next_left;
            let changed = (next_left - weights[0]).abs() > f32::EPSILON
                || (next_right - weights[1]).abs() > f32::EPSILON;
            weights[0] = next_left;
            weights[1] = next_right;
            changed
        }
        UiEventKind::PointerUp => {
            drag.anchor = None;
            false
        }
        _ => false,
    }
}

fn apply_key(value: &mut f32, event: &UiEvent, side: Side, min: f32, max: f32) -> bool {
    let Some(press) = event.key_press.as_ref() else {
        return false;
    };
    let prev = *value;
    // Same sign trick as the pointer drag: ArrowRight/Down "moves the
    // handle in the +axis direction" — that grows a Start-side value
    // and shrinks an End-side one. Home/End follow the same rule by
    // mapping to whichever extreme the handle's leftmost/topmost
    // position represents.
    let step = KEYBOARD_STEP_PX * side.sign();
    let page_step = KEYBOARD_PAGE_STEP_PX * side.sign();
    let (home_target, end_target) = match side {
        Side::Start => (min, max),
        Side::End => (max, min),
    };
    let next = match press.logical.named() {
        Some(NamedKey::ArrowRight | NamedKey::ArrowDown) => *value + step,
        Some(NamedKey::ArrowLeft | NamedKey::ArrowUp) => *value - step,
        Some(NamedKey::PageUp) => *value + page_step,
        Some(NamedKey::PageDown) => *value - page_step,
        Some(NamedKey::Home) => home_target,
        Some(NamedKey::End) => end_target,
        _ => return false,
    };
    *value = next.clamp(min, max);
    (*value - prev).abs() > f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, LogicalKey, PhysicalKey, UiTarget};

    fn pointer_event(kind: UiEventKind, key: &str, x: f32) -> UiEvent {
        let click_count = match kind {
            UiEventKind::PointerDown | UiEventKind::PointerUp | UiEventKind::Click => 1,
            _ => 0,
        };
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}").into(),
                rect: Rect::new(0.0, 0.0, 6.0, 400.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: Some((x, 100.0)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count,
            pointer_kind: None,
            wheel_delta: None,
            kind,
        }
    }

    fn key_event(key: &str, ui_key: LogicalKey) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}").into(),
                rect: Rect::new(0.0, 0.0, 6.0, 400.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: Some(KeyPress {
                logical: ui_key,
                physical: PhysicalKey::Unidentified,
                modifiers: KeyModifiers::default(),
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        }
    }

    #[test]
    fn handle_is_focusable_and_thin_in_its_resize_axis() {
        let row_handle = resize_handle("row-handle", Axis::Row);
        assert!(row_handle.focusable);
        assert_eq!(row_handle.width, Size::Fixed(HANDLE_THICKNESS));
        assert_eq!(row_handle.height, Size::Fill(1.0));

        let col_handle = resize_handle("col-handle", Axis::Column);
        assert_eq!(col_handle.width, Size::Fill(1.0));
        assert_eq!(col_handle.height, Size::Fixed(HANDLE_THICKNESS));
    }

    #[test]
    fn handle_cursor_matches_drag_axis() {
        // Row axis → vertical bar that slides ←→ → EwResize.
        // Column axis → horizontal bar that slides ↑↓ → NsResize.
        assert_eq!(
            resize_handle("row-cursor", Axis::Row).cursor,
            Some(crate::cursor::Cursor::EwResize),
        );
        assert_eq!(
            resize_handle("col-cursor", Axis::Column).cursor,
            Some(crate::cursor::Cursor::NsResize),
        );
    }

    #[test]
    fn handle_defaults_to_inside_focus_ring() {
        // A resize handle is flush-by-design — it sits in the seam
        // between panes, so an outside ring's cross-axis bands would
        // be occluded by whichever flush neighbor paints later
        // (issue #48, caught by the `FocusRingObscured` lint). The
        // widget defaults to an inside ring so call sites don't each
        // have to chain `.focus_ring_inside()`.
        for axis in [Axis::Row, Axis::Column] {
            let handle = resize_handle("handle", axis);
            assert_eq!(
                handle.focus_ring_placement,
                crate::tree::FocusRingPlacement::Inside,
            );
            // With the ring inside, no paint-overflow band is needed
            // for it either.
            assert_eq!(handle.paint_overflow, Sides::zero());
        }
    }

    #[test]
    fn handle_does_not_capture_keys() {
        // Regression guard: an earlier sketch added `capture_keys()`
        // which silently swallowed Tab and trapped focus on the
        // handle. The handle must let the runtime's default Tab
        // traversal move focus past it; arrow keys still arrive as
        // `KeyDown` without opting in.
        assert!(!resize_handle("row-keys", Axis::Row).capture_keys);
        assert!(!resize_handle("col-keys", Axis::Column).capture_keys);
    }

    #[test]
    fn fixed_drag_uses_absolute_anchor_so_no_drift() {
        // Down at x=300, drag to x=350 → +50px. Drag to x=380 → +80px
        // (absolute from anchor, not accumulated from previous Drag).
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "h", 300.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert_eq!(drag.anchor, Some(300.0));
        assert_eq!(drag.initial, 256.0);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 350.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert!((value - 306.0).abs() < 1e-3);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 380.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert!((value - 336.0).abs() < 1e-3);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerUp, "h", 380.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert_eq!(drag.anchor, None, "anchor cleared on PointerUp");
    }

    #[test]
    fn fixed_drag_clamps_to_min_max() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "h", 300.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        // Way beyond max.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 1000.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert_eq!(value, 480.0);
        // Way below min.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 0.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert_eq!(value, 180.0);
    }

    #[test]
    fn fixed_ignores_unrouted_events() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();
        let changed = apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "other", 300.0),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert!(!changed);
        assert_eq!(drag.anchor, None);
        assert_eq!(value, 256.0);
    }

    #[test]
    fn fixed_arrow_keys_nudge_within_bounds() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();
        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::ArrowRight)),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert!((value - (256.0 + KEYBOARD_STEP_PX)).abs() < 1e-3);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::Home)),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert_eq!(value, 180.0);

        // ArrowLeft at min is a no-op.
        let unchanged = apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::ArrowLeft)),
            "h",
            Axis::Row,
            Side::Start,
            180.0,
            480.0,
        );
        assert!(!unchanged);
        assert_eq!(value, 180.0);
    }

    #[test]
    fn weights_drag_redistributes_proportionally_to_parent_extent() {
        // Parent row 800px wide, weights [1.0, 1.0] → each pane is
        // 400px. Drag the handle 100px right → +0.25 weight to left,
        // -0.25 from right. (100 px / 800 px * 2.0 total = 0.25.)
        let mut weights = [1.0, 1.0];
        let mut drag = ResizeWeightsDrag::default();
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "split", 400.0),
            "split",
            Axis::Row,
            800.0,
            0.15,
        );
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "split", 500.0),
            "split",
            Axis::Row,
            800.0,
            0.15,
        );
        assert!((weights[0] - 1.25).abs() < 1e-3, "left = {}", weights[0]);
        assert!((weights[1] - 0.75).abs() < 1e-3, "right = {}", weights[1]);
        assert!(
            (weights[0] + weights[1] - 2.0).abs() < 1e-3,
            "total weight is conserved"
        );
    }

    #[test]
    fn weights_drag_clamps_each_side_to_min_weight() {
        let mut weights = [1.0, 1.0];
        let mut drag = ResizeWeightsDrag::default();
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "split", 400.0),
            "split",
            Axis::Row,
            800.0,
            0.5, // each side floors at 0.5 of weight
        );
        // Way past either end — should clamp.
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "split", 10_000.0),
            "split",
            Axis::Row,
            800.0,
            0.5,
        );
        assert!((weights[0] - 1.5).abs() < 1e-3);
        assert!((weights[1] - 0.5).abs() < 1e-3);

        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "split", -10_000.0),
            "split",
            Axis::Row,
            800.0,
            0.5,
        );
        assert!((weights[0] - 0.5).abs() < 1e-3);
        assert!((weights[1] - 1.5).abs() < 1e-3);
    }

    #[test]
    fn delta_from_event_returns_none_until_pointerdown() {
        let drag = ResizeDrag::default();
        let drag_event = pointer_event(UiEventKind::Drag, "h", 350.0);
        assert!(delta_from_event(&drag, &drag_event, Axis::Row).is_none());
    }

    #[test]
    fn fixed_drag_with_end_side_inverts_direction() {
        // Right-anchored pane: drag-LEFT should grow the value, since
        // the handle moving leftward expands the pane on its right.
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "h", 800.0),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );

        // Drag the handle 50px LEFT — pointer goes 800 → 750, pixel
        // delta is -50, but a right-anchored value should grow by 50.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 750.0),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );
        assert!(
            (value - 306.0).abs() < 1e-3,
            "drag-left on End side should grow value, got {value}",
        );

        // Drag the handle 30px RIGHT past the original anchor — should
        // shrink back below the initial.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 830.0),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );
        assert!(
            (value - 226.0).abs() < 1e-3,
            "drag-right on End side should shrink value, got {value}",
        );
    }

    #[test]
    fn fixed_arrow_keys_with_end_side_invert_direction() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();

        // ArrowLeft on End side grows (matches drag-left).
        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::ArrowLeft)),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );
        assert!((value - (256.0 + KEYBOARD_STEP_PX)).abs() < 1e-3);

        // ArrowRight on End side shrinks.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::ArrowRight)),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );
        assert!((value - 256.0).abs() < 1e-3);

        // Home on End side jumps to MAX (handle leftmost = pane
        // largest). End on End side jumps to MIN.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::Home)),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );
        assert_eq!(value, 480.0);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", LogicalKey::Named(NamedKey::End)),
            "h",
            Axis::Row,
            Side::End,
            180.0,
            480.0,
        );
        assert_eq!(value, 180.0);
    }
}
