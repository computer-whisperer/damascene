//! Input OTP — a row of `N` single-character cells for typing a
//! one-time password / verification code.
//!
//! Mirrors shadcn / Radix `<InputOTP>`: each character of the value
//! lives in its own bordered cell, the next-to-fill cell gets the
//! "active" border treatment, and `Backspace` pops the most recent
//! character. The whole row is one focusable target — click anywhere
//! to focus, then type.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct LoginForm {
//!     code: String,
//! }
//!
//! impl App for LoginForm {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         input_otp("code", &self.code, 6)
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent, _cx: &EventCx) {
//!         input_otp::apply_event(&mut self.code, "code", 6, &e);
//!     }
//! }
//! ```
//!
//! The app owns the value as a `String`; callers can mix in their own
//! validation (digits-only, alphanumeric, etc.) by post-filtering
//! `value` after each `apply_event` returns `true`. The widget itself
//! is character-class agnostic — whatever `TextInput` events deliver
//! lands in the value, capped at `length`.
//!
//! # Routed keys
//!
//! - `{key}` — the focusable row. `TextInput` events append, `KeyDown`
//!   with [`LogicalKey::Named(NamedKey::Backspace)`] pops.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface: a focusable `row` of
//! `Kind::Custom("input_otp_cell")` boxes. An app crate can fork this
//! file to add per-cell separators (e.g. a hyphen between groups of
//! three) or paste-multiple-chars handling without touching library
//! internals.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{NamedKey, UiEvent, UiEventKind};
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

const CELL_WIDTH: f32 = 36.0;
const CELL_HEIGHT: f32 = 40.0;

/// A row of `length` single-character cells driven by `value`.
/// Filled cells render the corresponding character; the cell at the
/// next-to-fill position carries the active border. When
/// `value.chars().count() >= length` no cell is active and any further
/// `TextInput` events are dropped.
#[track_caller]
pub fn input_otp(key: &str, value: &str, length: usize) -> El {
    let caller = Location::caller();
    let filled = value.chars().count().min(length);
    let mut cells: Vec<El> = Vec::with_capacity(length);
    for (i, ch) in cells_iter(value, length) {
        let active = i == filled && filled < length;
        cells.push(otp_cell(caller, ch, active));
    }

    row(cells)
        .at_loc(caller)
        .style_profile(StyleProfile::Surface)
        .focusable()
        .always_show_focus_ring()
        .capture_keys()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Text)
        .key(key.to_string())
        .gap(tokens::SPACE_1)
        .align(Align::Center)
        .height(Size::Fixed(CELL_HEIGHT))
}

/// Fold a routed [`UiEvent`] into `value`. Returns `true` if the event
/// belonged to this widget and changed the value.
///
/// Handles:
/// - [`UiEventKind::TextInput`] — append each char of `event.text` to
///   `value`, capped so the post-edit `chars().count()` does not
///   exceed `length`.
/// - [`UiEventKind::KeyDown`] with [`LogicalKey::Named(NamedKey::Backspace)`] — pop the most
///   recent character.
///
/// Routes by `event.target_key()`: the key events flow naturally to
/// the focused row, the `TextInput` events too.
pub fn apply_event(value: &mut String, key: &str, length: usize, event: &UiEvent) -> bool {
    if event.target_key() != Some(key) {
        return false;
    }
    match event.kind {
        UiEventKind::TextInput => {
            let Some(text) = event.text.as_deref() else {
                return false;
            };
            // winit emits TextInput alongside named-key / shortcut
            // KeyDowns: `"\u{8}"` for Backspace, `"\u{1b}"` for Escape,
            // `"\r"`/`"\n"` for Enter, `"\t"` for Tab. The KeyDown arm
            // already handles Backspace; without these filters that
            // control character lands in the value as an unprintable
            // box, which (a) looks like an empty cell on most fonts and
            // (b) blocks further deletion because `value.chars().count()`
            // never decreases. Same shape `text_input::apply_event` uses.
            if (event.modifiers.ctrl && !event.modifiers.alt) || event.modifiers.logo {
                return false;
            }
            let mut changed = false;
            for ch in text.chars() {
                if ch.is_control() {
                    continue;
                }
                if value.chars().count() >= length {
                    break;
                }
                value.push(ch);
                changed = true;
            }
            changed
        }
        UiEventKind::KeyDown => {
            let Some(kp) = event.key_press.as_ref() else {
                return false;
            };
            if kp.logical.named() == Some(NamedKey::Backspace) {
                value.pop().is_some()
            } else {
                false
            }
        }
        _ => false,
    }
}

fn cells_iter(value: &str, length: usize) -> impl Iterator<Item = (usize, Option<char>)> + '_ {
    let mut chars = value.chars();
    (0..length).map(move |i| (i, chars.next()))
}

fn otp_cell(caller: &'static Location<'static>, ch: Option<char>, active: bool) -> El {
    let stroke = if active {
        tokens::PRIMARY
    } else {
        tokens::INPUT
    };
    let body: El = match ch {
        Some(c) => text(c.to_string()).label(),
        None => El::new(Kind::Spacer).width(Size::Fixed(0.0)),
    };
    El::new(Kind::Custom("input_otp_cell"))
        .at_loc(caller)
        .style_profile(StyleProfile::Surface)
        .axis(Axis::Overlay)
        .align(Align::Center)
        .justify(Justify::Center)
        .width(Size::Fixed(CELL_WIDTH))
        .height(Size::Fixed(CELL_HEIGHT))
        .fill(tokens::BACKGROUND)
        .stroke(stroke)
        .stroke_width(1.0)
        .radius(tokens::RADIUS_MD)
        .child(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, LogicalKey, PhysicalKey, UiTarget};
    use crate::tree::Rect;

    fn text_input_event(key: &str, txt: &str) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}"),
                rect: Rect::new(0.0, 0.0, 100.0, 40.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: None,
            text: Some(txt.to_string()),
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::TextInput,
        }
    }

    fn backspace_event(key: &str) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}"),
                rect: Rect::new(0.0, 0.0, 100.0, 40.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: Some(KeyPress {
                logical: LogicalKey::Named(NamedKey::Backspace),
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
    fn text_input_appends_chars() {
        let mut value = String::new();
        assert!(apply_event(
            &mut value,
            "code",
            6,
            &text_input_event("code", "1")
        ));
        assert_eq!(value, "1");
        assert!(apply_event(
            &mut value,
            "code",
            6,
            &text_input_event("code", "2")
        ));
        assert_eq!(value, "12");
    }

    #[test]
    fn text_input_caps_at_length() {
        let mut value = String::from("12345");
        // Two more chars but only one slot left.
        assert!(apply_event(
            &mut value,
            "code",
            6,
            &text_input_event("code", "67")
        ));
        assert_eq!(value, "123456");
    }

    #[test]
    fn text_input_dropped_when_full() {
        let mut value = String::from("123456");
        // Already at length; the event matches the widget but doesn't
        // change the value, so apply_event returns false.
        assert!(!apply_event(
            &mut value,
            "code",
            6,
            &text_input_event("code", "7")
        ));
        assert_eq!(value, "123456");
    }

    #[test]
    fn backspace_pops_last_char() {
        let mut value = String::from("123");
        assert!(apply_event(&mut value, "code", 6, &backspace_event("code")));
        assert_eq!(value, "12");
    }

    #[test]
    fn backspace_on_empty_is_noop() {
        let mut value = String::new();
        assert!(!apply_event(
            &mut value,
            "code",
            6,
            &backspace_event("code")
        ));
        assert!(value.is_empty());
    }

    #[test]
    fn ignores_events_for_other_keys() {
        let mut value = String::new();
        assert!(!apply_event(
            &mut value,
            "code",
            6,
            &text_input_event("other", "x"),
        ));
        assert_eq!(value, "");
    }

    #[test]
    fn text_input_drops_control_chars_so_backspace_doesnt_self_insert() {
        // Regression: winit fires TextInput("\u{8}") *alongside* the
        // KeyDown(Backspace) event. Without the control-char filter,
        // the OTP's apply_event would (a) pop on KeyDown and then
        // (b) re-append `\u{8}` on TextInput, leaving an unprintable
        // box in the last cell that further Backspaces can't clear.
        let mut value = String::from("123");
        // Step 1: KeyDown(Backspace) pops the last char.
        assert!(apply_event(&mut value, "code", 6, &backspace_event("code")));
        assert_eq!(value, "12");
        // Step 2: TextInput("\u{8}") that winit also emits should be
        // dropped, not appended.
        let mut bs_text = text_input_event("code", "\u{8}");
        assert!(!apply_event(&mut value, "code", 6, &bs_text));
        assert_eq!(value, "12", "control char should not be inserted");
        // Other control chars (Enter, Tab, Escape) are also filtered.
        for ctl in ["\r", "\n", "\t", "\u{1b}", "\u{7f}"] {
            bs_text = text_input_event("code", ctl);
            assert!(
                !apply_event(&mut value, "code", 6, &bs_text),
                "control char {ctl:?} should be dropped",
            );
            assert_eq!(value, "12");
        }
    }

    #[test]
    fn text_input_drops_ctrl_modified_chars() {
        // winit fires TextInput("c") for Ctrl+C on some platforms.
        // The clipboard side already handled the KeyDown — we don't
        // want the literal letter to land in the OTP value.
        let mut value = String::new();
        let mut ev = text_input_event("code", "c");
        ev.modifiers = KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        };
        assert!(!apply_event(&mut value, "code", 6, &ev));
        assert_eq!(value, "");
    }

    #[test]
    fn text_input_keeps_alt_gr_chars() {
        // AltGr is reported as Ctrl+Alt; that combination must still
        // produce text (`@`, `€`, `[`, `]`, `{`, `}`, …). Mirrors the
        // exemption in text_input.
        let mut value = String::new();
        let mut ev = text_input_event("code", "@");
        ev.modifiers = KeyModifiers {
            ctrl: true,
            alt: true,
            ..KeyModifiers::default()
        };
        assert!(apply_event(&mut value, "code", 6, &ev));
        assert_eq!(value, "@");
    }

    #[test]
    fn paste_of_multiple_chars_fills_remaining_cells() {
        let mut value = String::new();
        assert!(apply_event(
            &mut value,
            "code",
            6,
            &text_input_event("code", "abcdef"),
        ));
        assert_eq!(value, "abcdef");
    }

    #[test]
    fn build_widget_has_one_cell_per_length_with_correct_active_marker() {
        let el = input_otp("code", "12", 6);
        assert_eq!(el.key.as_deref(), Some("code"));
        assert_eq!(el.children.len(), 6);
        // Cell index == 2 (next-to-fill) should carry the PRIMARY
        // stroke; siblings get the muted INPUT stroke.
        assert_eq!(el.children[0].stroke, Some(tokens::INPUT));
        assert_eq!(el.children[1].stroke, Some(tokens::INPUT));
        assert_eq!(el.children[2].stroke, Some(tokens::PRIMARY));
        assert_eq!(el.children[3].stroke, Some(tokens::INPUT));
    }

    #[test]
    fn full_value_renders_no_active_cell() {
        let el = input_otp("code", "123456", 6);
        for cell in &el.children {
            assert_eq!(
                cell.stroke,
                Some(tokens::INPUT),
                "no cell should be active when value is full",
            );
        }
    }
}
