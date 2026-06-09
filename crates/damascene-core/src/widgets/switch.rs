//! Switch — a controlled boolean toggle, shaped like the shadcn /
//! Radix Switch primitive (track + thumb).
//!
//! The app owns the underlying `bool` and projects it into the widget
//! on every `build()`. Clicking the switch emits `Click` (or
//! `Activate` for keyboard space/enter) on the trigger key; the app
//! flips its bool field — typically through [`apply_event`].
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Prefs { auto_save: bool }
//!
//! impl App for Prefs {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         row([
//!             text("Auto-save").label(),
//!             spacer(),
//!             switch(self.auto_save).key("auto_save"),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         switch::apply_event(&mut self.auto_save, &event, "auto_save");
//!     }
//! }
//! ```
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom`,
//! `.focusable()` + `.paint_overflow()` for the focus ring, and a
//! `.layout(...)` closure that places the thumb inside the track.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::UiEvent;
use crate::layout::LayoutCtx;
use crate::tokens;
use crate::tree::*;

/// Track width in logical pixels.
pub const TRACK_WIDTH: f32 = 36.0;
/// Track height in logical pixels.
pub const TRACK_HEIGHT: f32 = 20.0;
/// Thumb diameter in logical pixels.
pub const THUMB_SIZE: f32 = 14.0;
/// Inset of the thumb from the track's edges (each side).
const PAD: f32 = (TRACK_HEIGHT - THUMB_SIZE) / 2.0;

/// Total horizontal travel of the thumb between the off and on
/// positions, in logical pixels. Made public so apps that want a
/// matching label transition can drive the same distance.
pub const THUMB_SLIDE: f32 = TRACK_WIDTH - THUMB_SIZE - 2.0 * PAD;

/// A two-state toggle. `value` controls the visual state (`true`
/// shifts the thumb to the right and fills the track with the primary
/// color); the app flips its underlying bool on `Click` / `Activate`
/// via [`apply_event`].
///
/// State changes are animated. The thumb's position is laid out at
/// the off side and shifted via an animatable [`El::translate`] when
/// `value == true`; the track's fill animates between
/// [`tokens::INPUT`] (off) and [`tokens::PRIMARY`] (on). The thumb
/// uses [`tokens::FOREGROUND`] when off and
/// [`tokens::PRIMARY_FOREGROUND`] when on, matching shadcn's
/// `primary` / `primary-foreground` checked-state pairing so the knob
/// remains visible when the dark theme's active track is white. The
/// underlying timing is [`Timing::SPRING_QUICK`] — calibrated to read
/// as a snappy switch with no overshoot.
///
/// The widget hugs its fixed track size — chain `.key(...)` on the
/// returned `El` to receive the toggle event.
#[track_caller]
pub fn switch(value: bool) -> El {
    let layout = |ctx: LayoutCtx| {
        // Lay out the thumb at the OFF position regardless of `value`;
        // the visual ON position is reached by an animatable translate
        // applied in the builder below. That keeps the slide easeable
        // through `.animate()` — animatable props ease across
        // rebuilds, but the rect a layout closure returns does not.
        let r = ctx.container;
        let track_x = r.x + (r.w - TRACK_WIDTH) * 0.5;
        let track_y = r.y + (r.h - TRACK_HEIGHT) * 0.5;
        let thumb_x = track_x + PAD;
        let thumb_y = track_y + PAD;
        vec![
            Rect::new(track_x, track_y, TRACK_WIDTH, TRACK_HEIGHT),
            Rect::new(thumb_x, thumb_y, THUMB_SIZE, THUMB_SIZE),
        ]
    };

    let track_fill = if value {
        tokens::PRIMARY
    } else {
        tokens::INPUT
    };
    let thumb_fill = if value {
        tokens::PRIMARY_FOREGROUND
    } else {
        tokens::FOREGROUND
    };
    let thumb_translate_x = if value { THUMB_SLIDE } else { 0.0 };

    stack([
        El::new(Kind::Custom("switch-track"))
            .fill(track_fill)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_PILL)
            .animate(Timing::SPRING_QUICK)
            // Hit-test resolves to the focusable outer; without the
            // cascade, the track and thumb would never react to hover
            // / press on the switch.
            .state_follows_interactive_ancestor(),
        El::new(Kind::Custom("switch-thumb"))
            .fill(thumb_fill)
            .radius(tokens::RADIUS_PILL)
            .translate(thumb_translate_x, 0.0)
            .animate(Timing::SPRING_QUICK)
            .state_follows_interactive_ancestor(),
    ])
    .at_loc(Location::caller())
    .focusable()
    .paint_overflow(Sides::all(tokens::RING_WIDTH))
    .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
    .cursor(Cursor::Pointer)
    .layout(layout)
    .width(Size::Fixed(TRACK_WIDTH))
    .height(Size::Fixed(TRACK_HEIGHT))
}

/// Fold a routed [`UiEvent`] into a `bool` switch value. Returns
/// `true` if the event was a `Click` / `Activate` for `key` and the
/// value was flipped.
///
/// ```ignore
/// switch::apply_event(&mut self.auto_save, &event, "auto_save");
/// ```
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
    use crate::event::UiEvent;

    #[test]
    fn off_switch_paints_input_track_and_foreground_thumb() {
        // The track's fill is the visual signal of state, so an off
        // switch uses the shadcn unchecked input token rather than
        // PRIMARY.
        let s = switch(false);
        let track = &s.children[0];
        let thumb = &s.children[1];
        assert_eq!(track.fill, Some(tokens::INPUT));
        assert_eq!(thumb.fill, Some(tokens::FOREGROUND));
        // Track stays a pill regardless of state.
        assert_eq!(track.radius, crate::tree::Corners::all(tokens::RADIUS_PILL));
    }

    #[test]
    fn on_switch_paints_primary_track_and_primary_foreground_thumb() {
        // In shadcn's dark checked state, `primary` is a light track
        // and `primary-foreground` is the dark contrasting thumb.
        let s = switch(true);
        let track = &s.children[0];
        let thumb = &s.children[1];
        assert_eq!(track.fill, Some(tokens::PRIMARY));
        assert_eq!(thumb.fill, Some(tokens::PRIMARY_FOREGROUND));
    }

    #[test]
    fn switch_is_focusable_and_paints_focus_ring_outset() {
        // Tab traversal lands on the switch like any other interactive
        // surface; the ring needs `paint_overflow` to render outside
        // the layout rect.
        let s = switch(false);
        assert!(s.focusable);
        assert!(s.paint_overflow.left > 0.0);
        assert_eq!(s.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
    }

    #[test]
    fn switch_declares_pointer_cursor() {
        assert_eq!(switch(false).cursor, Some(Cursor::Pointer));
    }

    #[test]
    fn apply_event_toggles_on_click() {
        let mut value = false;
        assert!(apply_event(
            &mut value,
            &UiEvent::synthetic_click("save"),
            "save"
        ));
        assert!(value);
        assert!(apply_event(
            &mut value,
            &UiEvent::synthetic_click("save"),
            "save"
        ));
        assert!(!value);
    }

    #[test]
    fn apply_event_ignores_unrelated_keys() {
        let mut value = true;
        assert!(!apply_event(
            &mut value,
            &UiEvent::synthetic_click("other"),
            "save",
        ));
        assert!(value, "value preserved when key doesn't match");
    }

    #[test]
    fn layout_pins_thumb_to_off_position_regardless_of_value() {
        // The animated thumb lays out at the OFF position; the visual
        // ON position is reached through an animatable translate. So
        // the laid-out rect should match for both states — the
        // difference shows up in `translate`, not `rect`.
        use crate::layout::layout;
        use crate::state::UiState;

        for value in [false, true] {
            let mut tree = switch(value);
            let mut state = UiState::new();
            let viewport = Rect::new(0.0, 0.0, TRACK_WIDTH, TRACK_HEIGHT);
            layout(&mut tree, &mut state, viewport);
            let thumb_rect = state.rect(&tree.children[1].computed_id);
            assert!(
                (thumb_rect.x - PAD).abs() < 1e-3,
                "value={value}: layout-rect thumb.x={}, expected={PAD}",
                thumb_rect.x,
            );
        }
    }

    #[test]
    fn translate_carries_the_thumb_slide_when_on() {
        // The on→off motion is the translate going from THUMB_SLIDE
        // to 0. Verify the build-time translate field, since that's
        // what the animation system eases across rebuilds.
        let off = switch(false);
        let on = switch(true);
        assert_eq!(off.children[1].translate, (0.0, 0.0));
        assert!(
            (on.children[1].translate.0 - THUMB_SLIDE).abs() < 1e-3,
            "thumb translate.x = {}, expected {THUMB_SLIDE}",
            on.children[1].translate.0,
        );
    }

    #[test]
    fn track_and_thumb_animate_so_state_changes_ease() {
        // Both children opt into prop interpolation. Without these,
        // the track-fill swap and the thumb slide would jump on
        // toggle.
        let s = switch(false);
        assert!(s.children[0].animate.is_some(), "track must animate");
        assert!(s.children[1].animate.is_some(), "thumb must animate");
    }
}
