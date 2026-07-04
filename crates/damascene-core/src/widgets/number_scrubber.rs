//! Number scrubber — drag the value display horizontally to edit it.
//!
//! A compact alternative to [`crate::widgets::numeric_input`] for
//! dense control panels. There are no spinner buttons: the value text
//! is itself the affordance. Pointer-drag across it adjusts the value
//! at a configurable pixels-per-step rate; Arrow keys nudge with the
//! same modifier scaling (`Shift` ×10, `Alt` ×0.1).
//!
//! Common in design and audio tools — Figma's numeric scrubbers,
//! Blender's parameter fields, After Effects' time displays, Linear's
//! sidebar number inputs all share this shape.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use damascene_core::widgets::number_scrubber::{self, ScrubDrag, ScrubberOpts};
//!
//! struct Mixer {
//!     gain_db: String,
//!     gain_drag: ScrubDrag,
//! }
//!
//! impl App for Mixer {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         number_scrubber::number_scrubber("gain", &self.gain_db)
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent, _cx: &EventCx) {
//!         let opts = ScrubberOpts::default()
//!             .min(-60.0).max(12.0).step(0.5).sensitivity(3.0).decimals(1);
//!         number_scrubber::apply_event(
//!             &mut self.gain_db, &mut self.gain_drag, "gain", &opts, &e,
//!         );
//!     }
//! }
//! ```
//!
//! # Routed key
//!
//! - `{key}` — the cell itself. `PointerDown` / `Drag` / `PointerUp`
//!   form the drag lifecycle; `KeyDown` ArrowLeft/Right/Up/Down step
//!   when the cell is focused. The widget assigns its own key; callers
//!   pass it via the `key` argument.
//!
//! # Modifier scaling
//!
//! - **Shift** — 10× step (coarse).
//! - **Alt** — 0.1× step (fine).
//!
//! Scaling is re-evaluated on every drag frame, so pressing `Shift`
//! mid-drag widens the displacement-to-value gain without restarting
//! the drag.
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface. The cell is
//! one focusable `El` with a text leaf — no privileged internals.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{KeyModifiers, NamedKey, UiEvent, UiEventKind};
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// Configuration for [`number_scrubber`] / [`apply_event`].
///
/// Defaults: no min, no max, `step = 1.0`, `sensitivity = 4.0`
/// (4 pixels of drag = one `step`), no fixed precision.
#[derive(Clone, Copy, Debug)]
pub struct ScrubberOpts {
    /// Lower bound. Drags and arrow steps clamp to at least this value.
    pub min: Option<f64>,
    /// Upper bound. Drags and arrow steps clamp to at most this value.
    pub max: Option<f64>,
    /// Increment for one "tick". A drag of `sensitivity` pixels covers
    /// one `step`; an arrow press covers one `step`.
    pub step: f64,
    /// Pixels of horizontal drag per `step`. Smaller = more sensitive
    /// (the value sweeps faster); larger = more deliberate.
    pub sensitivity: f64,
    /// Fixed decimal places for the formatted result. `None` matches
    /// [`crate::widgets::numeric_input::NumericInputOpts::decimals`]:
    /// integral values render as `42`, non-integral via `f64::Display`;
    /// `Some(n)` always formats with `n` decimals.
    pub decimals: Option<u8>,
}

impl Default for ScrubberOpts {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            step: 1.0,
            sensitivity: 4.0,
            decimals: None,
        }
    }
}

impl ScrubberOpts {
    /// Set the lower bound (see [`ScrubberOpts::min`]).
    pub fn min(mut self, v: f64) -> Self {
        self.min = Some(v);
        self
    }
    /// Set the upper bound (see [`ScrubberOpts::max`]).
    pub fn max(mut self, v: f64) -> Self {
        self.max = Some(v);
        self
    }
    /// Set the per-tick increment (see [`ScrubberOpts::step`]).
    pub fn step(mut self, v: f64) -> Self {
        self.step = v;
        self
    }
    /// Set the pixels-per-step drag rate (see [`ScrubberOpts::sensitivity`]).
    pub fn sensitivity(mut self, v: f64) -> Self {
        self.sensitivity = v;
        self
    }
    /// Format results with a fixed number of decimal places (see
    /// [`ScrubberOpts::decimals`]).
    pub fn decimals(mut self, v: u8) -> Self {
        self.decimals = Some(v);
        self
    }
}

/// Drag-anchor state for [`apply_event`]. Lives in the app struct
/// alongside the scrubber's value; default-init it
/// (`ScrubDrag::default()`) and pass `&mut`.
///
/// `anchor_x` is the pointer x captured at `PointerDown`; `initial` is
/// the numeric value at that moment. Each `Drag` event recomputes an
/// absolute target value from `(anchor_x, initial, current_x)` so
/// drags don't accumulate float rounding across many events — same
/// shape as [`crate::widgets::resize_handle::ResizeDrag`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrubDrag {
    /// Pointer x captured at `PointerDown`; `None` when no drag is active.
    pub anchor_x: Option<f32>,
    /// Numeric value at the moment the drag started.
    pub initial: f64,
}

/// Minimum width of the scrubber cell. Wide enough that a 3-digit
/// integer plus a couple of decimal places stays readable without
/// horizontal jitter as the value scrubs.
pub const MIN_WIDTH: f32 = 64.0;

/// A draggable numeric cell. `value` is the string to render (the app
/// owns the formatting between events). Chain `.width(...)` to override
/// the default minimum.
#[track_caller]
pub fn number_scrubber(key: &str, value: &str) -> El {
    El::new(Kind::Custom("number-scrubber"))
        .at_loc(Location::caller())
        .key(key.to_string())
        .style_profile(StyleProfile::Solid)
        .focusable()
        .text(value)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .text_color(tokens::FOREGROUND)
        .fill(tokens::INPUT)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Fixed(MIN_WIDTH))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        // EwResize signals "horizontal drag adjusts" — same idiom used
        // by Figma/Blender. Holding the cursor through the press keeps
        // the affordance visible during the actual drag too.
        .cursor(Cursor::EwResize)
        .cursor_pressed(Cursor::EwResize)
        // Touch drag scrubs the value; opt out of the touch-scroll
        // synthesis so the gesture doesn't get cancelled mid-scrub.
        .consumes_touch_drag()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
}

/// Fold a routed [`UiEvent`] into the scrubber's value. Returns `true`
/// when the event belonged to this scrubber and the value changed.
///
/// Lifecycle:
///
/// - `PointerDown` on `key` captures `drag.anchor_x` and parses
///   `value` (or falls back to `opts.min` / 0) into `drag.initial`.
/// - `Drag` recomputes target value = `initial + (x - anchor) /
///   sensitivity * step * modifier_scale`, clamps to `min`/`max`,
///   formats per `decimals`, and writes back.
/// - `PointerUp` clears the anchor.
/// - `KeyDown` ArrowLeft/Down → `-step`; ArrowRight/Up → `+step`.
///   `Shift` ×10, `Alt` ×0.1.
pub fn apply_event(
    value: &mut String,
    drag: &mut ScrubDrag,
    key: &str,
    opts: &ScrubberOpts,
    event: &UiEvent,
) -> bool {
    if event.route() != Some(key) {
        return false;
    }
    match event.kind {
        UiEventKind::PointerDown => {
            if let Some((px, _)) = event.pointer {
                drag.anchor_x = Some(px);
                drag.initial = parse_or_default(value, opts);
            }
            false
        }
        UiEventKind::Drag => {
            let Some(anchor) = drag.anchor_x else {
                return false;
            };
            let Some((px, _)) = event.pointer else {
                return false;
            };
            // `sensitivity` is pixels-per-step, so the pixel delta
            // converts to step counts by division; multiplying by
            // `step` gives the raw value delta and the modifier scale
            // is layered on top. A non-positive sensitivity would make
            // the drag explode — clamp defensively.
            let sens = opts.sensitivity.max(f32::EPSILON as f64);
            let scale = step_scale(event.modifiers);
            let delta = ((px - anchor) as f64) / sens * opts.step * scale;
            let next = clamp_opt(drag.initial + delta, opts.min, opts.max);
            let formatted = format_numeric(next, opts.decimals);
            if formatted != *value {
                *value = formatted;
                true
            } else {
                false
            }
        }
        UiEventKind::PointerUp => {
            drag.anchor_x = None;
            false
        }
        UiEventKind::KeyDown => {
            let Some(kp) = event.key_press.as_ref() else {
                return false;
            };
            let dir = match kp.logical.named() {
                Some(NamedKey::ArrowRight | NamedKey::ArrowUp) => 1,
                Some(NamedKey::ArrowLeft | NamedKey::ArrowDown) => -1,
                _ => return false,
            };
            let parsed = parse_or_default(value, opts);
            let stepped = parsed + (dir as f64) * opts.step * step_scale(kp.modifiers);
            let next = clamp_opt(stepped, opts.min, opts.max);
            let formatted = format_numeric(next, opts.decimals);
            if formatted != *value {
                *value = formatted;
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn parse_or_default(value: &str, opts: &ScrubberOpts) -> f64 {
    value
        .parse::<f64>()
        .ok()
        .unwrap_or_else(|| opts.min.unwrap_or(0.0))
}

fn step_scale(mods: KeyModifiers) -> f64 {
    if mods.shift {
        10.0
    } else if mods.alt {
        0.1
    } else {
        1.0
    }
}

fn clamp_opt(n: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let n = if let Some(hi) = max { n.min(hi) } else { n };
    if let Some(lo) = min { n.max(lo) } else { n }
}

fn format_numeric(n: f64, decimals: Option<u8>) -> String {
    match decimals {
        Some(d) => format!("{:.*}", d as usize, n),
        None if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e18 => {
            format!("{}", n as i64)
        }
        None => format!("{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, LogicalKey, PhysicalKey, UiTarget};
    use crate::tree::Rect;

    fn pointer_event(key: &str, kind: UiEventKind, x: f32, mods: KeyModifiers) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}").into(),
                rect: Rect::new(0.0, 0.0, MIN_WIDTH, tokens::CONTROL_HEIGHT),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: Some((x, tokens::CONTROL_HEIGHT * 0.5)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: mods,
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind,
        }
    }

    fn key_event(key: &str, ui_key: LogicalKey, mods: KeyModifiers) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}").into(),
                rect: Rect::new(0.0, 0.0, MIN_WIDTH, tokens::CONTROL_HEIGHT),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: Some(KeyPress {
                logical: ui_key,
                physical: PhysicalKey::Unidentified,
                modifiers: mods,
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers: mods,
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        }
    }

    #[test]
    fn pointer_drag_increments_by_pixel_ratio() {
        // sensitivity=4, step=1 → 16px of right-drag = +4.
        let mut value = String::from("10");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().sensitivity(4.0).step(1.0);

        let down = pointer_event(
            "n",
            UiEventKind::PointerDown,
            100.0,
            KeyModifiers::default(),
        );
        assert!(!apply_event(&mut value, &mut drag, "n", &opts, &down));
        assert_eq!(drag.anchor_x, Some(100.0));
        assert_eq!(drag.initial, 10.0);

        let drag_ev = pointer_event("n", UiEventKind::Drag, 116.0, KeyModifiers::default());
        assert!(apply_event(&mut value, &mut drag, "n", &opts, &drag_ev));
        assert_eq!(value, "14");
    }

    #[test]
    fn pointer_drag_left_decrements() {
        let mut value = String::from("10");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().sensitivity(4.0).step(1.0);
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event(
                "n",
                UiEventKind::PointerDown,
                100.0,
                KeyModifiers::default(),
            ),
        );
        assert!(apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, 88.0, KeyModifiers::default()),
        ));
        // -12px / 4 = -3 steps × 1.0 = -3.
        assert_eq!(value, "7");
    }

    #[test]
    fn drag_recomputes_from_anchor_not_previous_event() {
        // Many consecutive Drag events with the same target position
        // should not stack — the helper recomputes from the anchor each
        // time, mirroring resize_handle::apply_event_fixed.
        let mut value = String::from("0");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().sensitivity(4.0).step(1.0);
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerDown, 50.0, KeyModifiers::default()),
        );
        // Drag to 50 + 20 = 70; (20/4)*1 = 5.
        for _ in 0..5 {
            apply_event(
                &mut value,
                &mut drag,
                "n",
                &opts,
                &pointer_event("n", UiEventKind::Drag, 70.0, KeyModifiers::default()),
            );
        }
        assert_eq!(value, "5", "anchor-relative drag must not accumulate");
    }

    #[test]
    fn pointer_up_clears_anchor() {
        let mut value = String::from("3");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default();
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerDown, 10.0, KeyModifiers::default()),
        );
        assert!(drag.anchor_x.is_some());
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerUp, 30.0, KeyModifiers::default()),
        );
        assert!(drag.anchor_x.is_none());
        // A Drag after PointerUp without a fresh PointerDown does nothing.
        assert!(!apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, 60.0, KeyModifiers::default()),
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn shift_drag_scales_step_by_ten() {
        let mut value = String::from("0");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().sensitivity(4.0).step(1.0);
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerDown, 0.0, KeyModifiers::default()),
        );
        let shift = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        // +8 px / 4 = +2 steps × 1.0 × 10 = +20.
        assert!(apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, 8.0, shift),
        ));
        assert_eq!(value, "20");
    }

    #[test]
    fn alt_drag_scales_step_by_one_tenth() {
        let mut value = String::from("0");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default()
            .sensitivity(4.0)
            .step(1.0)
            .decimals(1);
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerDown, 0.0, KeyModifiers::default()),
        );
        let alt = KeyModifiers {
            alt: true,
            ..KeyModifiers::default()
        };
        // +40 px / 4 = +10 steps × 1.0 × 0.1 = +1.0; formatted as "1.0".
        assert!(apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, 40.0, alt),
        ));
        assert_eq!(value, "1.0");
    }

    #[test]
    fn drag_clamps_to_min_and_max() {
        let mut value = String::from("50");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default()
            .sensitivity(1.0)
            .step(1.0)
            .min(0.0)
            .max(100.0);
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerDown, 0.0, KeyModifiers::default()),
        );
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, 9999.0, KeyModifiers::default()),
        );
        assert_eq!(value, "100");
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, -9999.0, KeyModifiers::default()),
        );
        assert_eq!(value, "0");
    }

    #[test]
    fn arrow_keys_step_when_focused() {
        let mut value = String::from("3");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().step(2.0);
        assert!(apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &key_event(
                "n",
                LogicalKey::Named(NamedKey::ArrowRight),
                KeyModifiers::default()
            ),
        ));
        assert_eq!(value, "5");
        assert!(apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &key_event(
                "n",
                LogicalKey::Named(NamedKey::ArrowDown),
                KeyModifiers::default()
            ),
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn arrow_keys_honor_shift_and_alt() {
        let mut value = String::from("0");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().step(1.0);
        let shift = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &key_event("n", LogicalKey::Named(NamedKey::ArrowUp), shift),
        );
        assert_eq!(value, "10");
        // Reset; Alt-step drops fine adjustment.
        value = "0".into();
        let opts = ScrubberOpts::default().step(1.0).decimals(1);
        let alt = KeyModifiers {
            alt: true,
            ..KeyModifiers::default()
        };
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &key_event("n", LogicalKey::Named(NamedKey::ArrowUp), alt),
        );
        assert_eq!(value, "0.1");
    }

    #[test]
    fn events_routed_elsewhere_are_ignored() {
        let mut value = String::from("3");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default();
        assert!(!apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event(
                "other",
                UiEventKind::PointerDown,
                10.0,
                KeyModifiers::default()
            ),
        ));
        assert!(drag.anchor_x.is_none());
        assert!(!apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &key_event(
                "other",
                LogicalKey::Named(NamedKey::ArrowUp),
                KeyModifiers::default()
            ),
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn unparseable_value_starts_drag_at_min_or_zero() {
        // Mirrors numeric_input: a non-numeric current value behaves
        // like an empty field — the drag baseline is `min` if set,
        // else 0.
        let mut value = String::from("abc");
        let mut drag = ScrubDrag::default();
        let opts = ScrubberOpts::default().min(7.0).sensitivity(1.0);
        apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::PointerDown, 0.0, KeyModifiers::default()),
        );
        assert_eq!(drag.initial, 7.0);
        assert!(apply_event(
            &mut value,
            &mut drag,
            "n",
            &opts,
            &pointer_event("n", UiEventKind::Drag, 3.0, KeyModifiers::default()),
        ));
        assert_eq!(value, "10");
    }

    #[test]
    fn build_widget_sets_key_and_is_focusable() {
        let el = number_scrubber("gain", "42");
        assert_eq!(el.key.as_deref(), Some("gain"));
        assert!(el.focusable);
        // Cursor declares horizontal scrubability at rest and during press.
        assert_eq!(el.cursor, Some(Cursor::EwResize));
        assert_eq!(el.cursor_pressed, Some(Cursor::EwResize));
    }
}
