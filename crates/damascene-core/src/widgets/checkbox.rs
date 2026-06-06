//! Checkbox — a controlled boolean shaped like the shadcn / Radix
//! Checkbox primitive. A small rounded square that fills with the
//! primary color and shows a check icon when `value` is `true`.
//!
//! The app owns the underlying `bool`. Same controlled pattern as
//! [`crate::widgets::switch`]: clicking emits a `Click` (or
//! `Activate` for keyboard activation) on the trigger key, and
//! [`apply_event`] flips the bool.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Form { agree: bool }
//!
//! impl App for Form {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         row([
//!             checkbox(self.agree).key("agree"),
//!             text("I agree to the terms").label(),
//!         ]).gap(tokens::SPACE_2).align(Align::Center)
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         checkbox::apply_event(&mut self.agree, &event, "agree");
//!     }
//! }
//! ```
//!
//! # Dogfood note
//!
//! Composition over `Kind::Custom`, `.focusable()` + `.paint_overflow()`
//! for the focus ring, and a child `icon("check")` when checked. An
//! app crate can fork this against the public surface.

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::UiEvent;
use crate::icons::icon;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// Outer box edge length in logical pixels.
pub const SIZE: f32 = 16.0;
/// Check icon size when `value` is `true`.
const CHECK_ICON_SIZE: f32 = 12.0;

/// A two-state checkbox. `value == true` paints a primary-filled box
/// with a check glyph; `value == false` paints a hollow rounded square
/// with a strong border.
///
/// State changes ease through [`Timing::SPRING_STANDARD`] — the box's
/// fill and stroke cross-fade between hollow and filled, and the
/// check icon scales/fades in over the centred glyph slot. The check
/// is always present in the tree; an opacity multiplier hides it when
/// `value` is false so the same ease drives both directions.
///
/// Chain `.key(...)` on the returned `El` to receive the click event.
#[track_caller]
pub fn checkbox(value: bool) -> El {
    // Animatable props depending on `value`. Driving the check via
    // opacity + scale rather than child add/remove keeps the
    // animation system in charge of the transition; structural
    // child changes don't ease, but prop changes on a stable child
    // do.
    let (fill, stroke) = if value {
        (tokens::PRIMARY, tokens::PRIMARY)
    } else {
        (tokens::CARD, tokens::INPUT)
    };
    let check_opacity = if value { 1.0 } else { 0.0 };
    let check_scale = if value { 1.0 } else { 0.6 };

    El::new(Kind::Custom("checkbox"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::ChoiceControl)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .axis(Axis::Overlay)
        .align(Align::Center)
        .justify(Justify::Center)
        .default_width(Size::Fixed(SIZE))
        .default_height(Size::Fixed(SIZE))
        .default_radius(tokens::RADIUS_SM)
        .fill(fill)
        .stroke(stroke)
        .animate(Timing::SPRING_STANDARD)
        .child(
            icon("check")
                .icon_size(CHECK_ICON_SIZE)
                .icon_stroke_width(2.5)
                .color(tokens::PRIMARY_FOREGROUND)
                .opacity(check_opacity)
                .scale(check_scale)
                .animate(Timing::SPRING_STANDARD),
        )
}

/// Fold a routed [`UiEvent`] into a `bool` checkbox value. Returns
/// `true` if the event was a `Click` / `Activate` for `key` and the
/// value was flipped.
pub fn apply_event(value: &mut bool, event: &UiEvent, key: &str) -> bool {
    if event.is_click_or_activate(key) {
        *value = !*value;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchecked_paints_hollow_square_with_invisible_check_child() {
        // The check glyph is always present in the tree so its
        // opacity can ease back from 1→0; an unchecked box renders
        // it transparent rather than removing it.
        let c = checkbox(false);
        assert_eq!(c.children.len(), 1, "check glyph stays in the tree");
        assert_eq!(c.children[0].opacity, 0.0);
        assert_eq!(c.fill, Some(tokens::CARD));
        assert_eq!(c.stroke, Some(tokens::INPUT));
    }

    #[test]
    fn checked_paints_primary_with_visible_check_glyph() {
        let c = checkbox(true);
        assert_eq!(c.fill, Some(tokens::PRIMARY));
        assert_eq!(c.stroke, Some(tokens::PRIMARY));
        // Check icon is the only child and visible at full opacity.
        assert_eq!(c.children.len(), 1);
        let glyph = &c.children[0];
        assert_eq!(
            glyph.icon,
            Some(crate::IconSource::Builtin(IconName::Check))
        );
        assert_eq!(glyph.opacity, 1.0);
    }

    #[test]
    fn box_and_check_animate_so_state_changes_ease() {
        let c = checkbox(false);
        assert!(c.animate.is_some(), "outer box eases fill/stroke");
        assert!(c.children[0].animate.is_some(), "check eases opacity/scale");
    }

    #[test]
    fn checkbox_is_focusable_and_paints_focus_ring_outset() {
        let c = checkbox(false);
        assert!(c.focusable);
        assert!(c.paint_overflow.left > 0.0);
        assert_eq!(c.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
    }

    #[test]
    fn checkbox_declares_pointer_cursor() {
        assert_eq!(checkbox(false).cursor, Some(Cursor::Pointer));
    }

    #[test]
    fn apply_event_toggles_on_click() {
        let mut value = false;
        assert!(apply_event(
            &mut value,
            &UiEvent::synthetic_click("agree"),
            "agree"
        ));
        assert!(value);
        assert!(apply_event(
            &mut value,
            &UiEvent::synthetic_click("agree"),
            "agree"
        ));
        assert!(!value);
    }

    #[test]
    fn apply_event_ignores_unrelated_keys() {
        let mut value = false;
        assert!(!apply_event(
            &mut value,
            &UiEvent::synthetic_click("other"),
            "agree",
        ));
        assert!(!value);
    }
}
