//! Numeric input — text input with stepper buttons.
//!
//! Two visual variants share one event surface:
//!
//! - **Flanked** (default) — `[−] [text] [+]`. Hit area-friendly,
//!   matches the existing widget shape.
//! - **Stacked** — `[text │ ⌃/⌄]`. The conventional `<input type="number">`
//!   look (Tailwind UI, browser native). Opt in via [`NumericInputOpts::stacked`].
//!
//! shadcn doesn't ship a dedicated component (web apps lean on
//! `<input type="number">` and let the browser draw spinners); for a
//! renderer-agnostic UI kit we render the spinners explicitly so the
//! affordance is consistent across backends.
//!
//! The app owns the value as a `String` (matching [`crate::widgets::text_input`]) so
//! mid-edit states like `"1."` aren't clobbered by a parse-and-reformat
//! round-trip on every keystroke. Parse to a number with
//! `s.parse::<f64>()` (or `i64`, …) when you actually need the value.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Form {
//!     count: String,
//!     selection: Selection,
//! }
//!
//! impl App for Form {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         let opts = NumericInputOpts::default()
//!             .min(0.0)
//!             .max(100.0)
//!             .step(1.0);
//!         numeric_input("count", &self.count, &self.selection, opts)
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent, _cx: &EventCx) {
//!         let opts = NumericInputOpts::default()
//!             .min(0.0)
//!             .max(100.0)
//!             .step(1.0);
//!         numeric_input::apply_event(
//!             &mut self.count, &mut self.selection, "count", &opts, &e,
//!         );
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}:dec` — `Click` on the down/`−` button. Steps the value down.
//! - `{key}:inc` — `Click` on the up/`+` button. Steps the value up.
//! - `{key}:field` — the inner [`crate::widgets::text_input`]; routed text edits / IME
//!   commits / pointer caret moves all flow through this key.
//!   `ArrowUp` / `ArrowDown` `KeyDown` events routed to this key are
//!   intercepted as step actions (the keyboard counterpart to the
//!   spinner buttons).
//!
//! Spinner clicks parse the current `value`, add or subtract
//! `opts.step`, clamp to `opts.min`/`opts.max` if set, and write the
//! formatted result back. If the value can't be parsed (empty or
//! garbage), the spinner treats it as `min` when set, otherwise as
//! `0.0`.
//!
//! # Modifier-scaled steps
//!
//! Spinner clicks and arrow-key steps both honor modifier keys to
//! produce coarse / fine adjustments without changing `opts.step`:
//!
//! - **Shift** — multiplies the step by 10 (coarse).
//! - **Alt** — multiplies the step by 0.1 (fine; rounded to
//!   `opts.decimals` when set).
//!
//! Holding both at once falls back to `Shift` since coarse is the more
//! common power-user gesture.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface: a `row` of ghost
//! [`button`]s / [`icon_button`]s and an inner [`text_input_with`].
//! An app crate can fork this file to add a different spinner shape
//! (wheel-on-scroll, named units, …) without touching library
//! internals.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::event::{KeyModifiers, UiEvent, UiEventKind, UiKey};
use crate::selection::Selection;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::{button, icon_button};
use crate::widgets::text_input::{
    TextInputOpts, apply_event_with as text_input_apply, text_input_with,
};

/// Configuration for [`numeric_input`] / [`apply_event`].
///
/// Defaults: no min, no max, `step = 1.0`, no fixed precision, no
/// placeholder. The same value is expected to be available both at
/// build-time (for the placeholder) and at event-time (so spinner
/// clicks know how much to step and where to clamp), so this is a
/// struct the app holds onto rather than chained modifiers on the
/// returned `El` — the same pattern [`TextInputOpts`] uses.
#[derive(Clone, Copy, Debug)]
pub struct NumericInputOpts<'a> {
    /// Lower bound. Spinner clicks clamp to at least this value.
    /// `None` means unbounded below.
    pub min: Option<f64>,
    /// Upper bound. Spinner clicks clamp to at most this value.
    /// `None` means unbounded above.
    pub max: Option<f64>,
    /// Increment for one spinner click. Default `1.0`.
    pub step: f64,
    /// Fixed decimal places for the formatted result.
    /// `None` means: integral values render as `42`, non-integral via
    /// `f64::Display`. `Some(n)` always formats with `n` decimals
    /// (e.g. `Some(2)` produces `"3.50"`).
    pub decimals: Option<u8>,
    /// Muted hint shown only while `value` is empty.
    pub placeholder: Option<&'a str>,
    /// Render the steppers as a stacked `⌃` / `⌄` column on the right
    /// edge of the field — the conventional `<input type="number">`
    /// shape — instead of `−` / `+` buttons flanking the field.
    ///
    /// Routed keys (`{key}:inc`, `{key}:dec`, `{key}:field`) are the
    /// same in both layouts, so [`apply_event`] doesn't branch.
    pub stacked: bool,
}

impl Default for NumericInputOpts<'_> {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            step: 1.0,
            decimals: None,
            placeholder: None,
            stacked: false,
        }
    }
}

impl<'a> NumericInputOpts<'a> {
    /// Set the lower bound (see [`NumericInputOpts::min`]).
    pub fn min(mut self, v: f64) -> Self {
        self.min = Some(v);
        self
    }
    /// Set the upper bound (see [`NumericInputOpts::max`]).
    pub fn max(mut self, v: f64) -> Self {
        self.max = Some(v);
        self
    }
    /// Set the per-click increment (see [`NumericInputOpts::step`]).
    pub fn step(mut self, v: f64) -> Self {
        self.step = v;
        self
    }
    /// Format results with a fixed number of decimal places (see
    /// [`NumericInputOpts::decimals`]).
    pub fn decimals(mut self, v: u8) -> Self {
        self.decimals = Some(v);
        self
    }
    /// Set the muted hint shown while the value is empty.
    pub fn placeholder(mut self, p: &'a str) -> Self {
        self.placeholder = Some(p);
        self
    }
    /// Opt into the stacked-chevron variant. Equivalent to
    /// `NumericInputOpts { stacked: true, ..self }`.
    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }
}

/// A numeric input field. Defaults to the flanked layout
/// `[−] [text_input] [+]`; opt into the stacked-chevron variant with
/// [`NumericInputOpts::stacked`].
///
/// The two spinner buttons are routed `{key}:dec` and `{key}:inc` in
/// both layouts; the inner text input is keyed `{key}:field`. The
/// wrapping `row` is keyed `{key}` itself so layout/test code can find
/// the whole composite by the same name the app uses.
#[track_caller]
pub fn numeric_input(
    key: &str,
    value: &str,
    selection: &Selection,
    opts: NumericInputOpts<'_>,
) -> El {
    let caller = Location::caller();

    let mut text_opts = TextInputOpts::default();
    if let Some(p) = opts.placeholder {
        text_opts = text_opts.placeholder(p);
    }
    let field_key = format!("{key}:field");
    let field = text_input_with(&field_key, value, selection, text_opts).width(Size::Fill(1.0));

    // RING_WIDTH gap: each focusable child needs a sliver of space so
    // its focus-ring band isn't painted over by the next sibling.
    //
    // The wrapping row defaults to a fixed width ([`DEFAULT_WIDTH`])
    // and the inner field stays `Fill(1.0)` to claim whatever's left
    // after the spinner buttons / chevron column. This avoids two
    // failure modes: a `Hug` row would collapse the inner `Fill(1.0)`
    // field to zero, and a `Fill(1.0)` row would stretch a 3-digit
    // value across the entire form — see [`DEFAULT_WIDTH`] for the
    // design rationale.
    let children: Vec<El> = if opts.stacked {
        vec![field, stacked_chevron_column(key, caller)]
    } else {
        let dec = button("−")
            .at_loc(caller)
            .key(format!("{key}:dec"))
            .ghost()
            .width(Size::Fixed(tokens::CONTROL_HEIGHT))
            .height(Size::Fixed(tokens::CONTROL_HEIGHT));
        let inc = button("+")
            .at_loc(caller)
            .key(format!("{key}:inc"))
            .ghost()
            .width(Size::Fixed(tokens::CONTROL_HEIGHT))
            .height(Size::Fixed(tokens::CONTROL_HEIGHT));
        vec![dec, field, inc]
    };

    row(children)
        .at_loc(caller)
        .key(key.to_string())
        .gap(tokens::RING_WIDTH)
        .align(Align::Center)
        .default_width(Size::Fixed(DEFAULT_WIDTH))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

/// Width of the stacked-chevron column. Narrow enough to feel like an
/// edge affordance, wide enough for a 14px chevron to sit centered
/// with a touch of horizontal breathing room.
const STACKED_CHEVRON_WIDTH: f32 = 22.0;

/// Default width of the wrapping row. Comfortable for 3–4 digit values
/// in either layout — equivalent to Tailwind's `w-36`.
///
/// Numeric inputs intrinsically display short values, so the default
/// is a fixed width rather than filling the parent — apps that want
/// the wider, text-input-style fill explicitly chain
/// `.width(Size::Fill(1.0))` on the returned `El`. This mirrors the
/// design-system consensus for numeric inputs (Material UI's
/// `<TextField type="number">` defaults `fullWidth=false`; Chakra's
/// `<NumberInput>` is content-width; Tailwind UI's examples use
/// `w-24` / `w-32`) rather than shadcn's generic-`<Input>` `w-full`
/// default that lumps numeric in with free-text fields.
pub const DEFAULT_WIDTH: f32 = 144.0;

/// Build the `⌃` over `⌄` chevron stack used by the stacked variant.
/// Each chevron is its own focusable [`icon_button`] so the inc/dec
/// hit areas remain distinct (and stay reachable by Tab focus).
///
/// Focus rings render inside each button's rect (via
/// [`El::focus_ring_inside`]) rather than the default outward bleed.
/// Without this, the up chevron's bottom focus-ring band would be
/// occluded by the dec button painted immediately below — the same
/// idiom dropdown-menu rows and calendar days use for densely packed
/// focusables that should stay visually flush. Each chevron is exactly
/// `CONTROL_HEIGHT / 2` so the two split the column with no gap.
fn stacked_chevron_column(key: &str, caller: &'static Location<'static>) -> El {
    let half_h = (tokens::CONTROL_HEIGHT * 0.5).floor();
    let inc = icon_button("chevron-up")
        .at_loc(caller)
        .key(format!("{key}:inc"))
        .ghost()
        .icon_size(tokens::ICON_XS)
        .focus_ring_inside()
        .width(Size::Fixed(STACKED_CHEVRON_WIDTH))
        .height(Size::Fixed(half_h));
    let dec = icon_button("chevron-down")
        .at_loc(caller)
        .key(format!("{key}:dec"))
        .ghost()
        .icon_size(tokens::ICON_XS)
        .focus_ring_inside()
        .width(Size::Fixed(STACKED_CHEVRON_WIDTH))
        .height(Size::Fixed(half_h));
    column([inc, dec])
        .at_loc(caller)
        .gap(0.0)
        .width(Size::Fixed(STACKED_CHEVRON_WIDTH))
        .height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

/// Fold a routed [`UiEvent`] into the numeric input's value, handling
/// spinner clicks, arrow-key steps on the focused field, and text
/// edits. Returns `true` if the event belonged to this widget
/// (regardless of whether the value changed).
///
/// Spinner clicks and arrow-key steps parse the current `value`, step
/// by `opts.step` (scaled by `Shift`/`Alt` modifiers), clamp to
/// `opts.min`/`opts.max`, and rewrite `value` formatted per
/// `opts.decimals`. Text edits are forwarded verbatim to
/// [`crate::widgets::text_input::apply_event`] — no parse / reformat cycle, so a
/// half-typed `"1."` keeps its cursor position.
pub fn apply_event(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    opts: &NumericInputOpts<'_>,
    event: &UiEvent,
) -> bool {
    if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        let inc_key = format!("{key}:inc");
        let dec_key = format!("{key}:dec");
        if event.route() == Some(inc_key.as_str()) {
            step_value(value, opts, 1, event.modifiers);
            return true;
        }
        if event.route() == Some(dec_key.as_str()) {
            step_value(value, opts, -1, event.modifiers);
            return true;
        }
    }

    let field_key = format!("{key}:field");

    // Arrow up / down on the focused field step the value — the
    // keyboard counterpart to the spinner buttons. text_input's own
    // KeyDown handler ignores ArrowUp/Down (it only consumes
    // ArrowLeft/Right/Home/End), so intercepting here doesn't steal
    // caret moves.
    if event.kind == UiEventKind::KeyDown
        && event.is_route(&field_key)
        && let Some(kp) = event.key_press.as_ref()
    {
        let dir = match kp.key {
            UiKey::ArrowUp => Some(1),
            UiKey::ArrowDown => Some(-1),
            _ => None,
        };
        if let Some(d) = dir {
            step_value(value, opts, d, kp.modifiers);
            return true;
        }
    }

    // Only consume text events that actually target the inner field.
    // text_input::apply_event itself doesn't gate on target_key
    // (callers do, see the per-input dispatch in the Inputs section);
    // forwarding every event would steal keystrokes meant for sibling
    // widgets and dump them into our value.
    if event.target_key() != Some(field_key.as_str()) {
        return false;
    }

    let text_opts = match opts.placeholder {
        Some(p) => TextInputOpts::default().placeholder(p),
        None => TextInputOpts::default(),
    };

    // Run the text_input edit, then revert if the post-edit value
    // contains non-numeric characters. The filter is permissive: any
    // char in `[0-9.eE+\-]` is allowed so mid-edit states like `"-"`,
    // `"1."`, or `"1.5e+"` keep the cursor where the user expects
    // while the value isn't yet a complete f64.
    let prev_value = value.clone();
    let prev_selection = selection.clone();
    let changed = text_input_apply(value, selection, &field_key, event, &text_opts);
    if changed && !is_acceptable_numeric_progress(value) {
        *value = prev_value;
        *selection = prev_selection;
        // The event targeted this field even though the edit was
        // rejected — `true` per the return contract above.
        return true;
    }
    changed
}

fn is_acceptable_numeric_progress(s: &str) -> bool {
    s.is_empty()
        || s.chars()
            .all(|c| matches!(c, '0'..='9' | '.' | 'e' | 'E' | '+' | '-'))
}

fn step_value(value: &mut String, opts: &NumericInputOpts<'_>, dir: i32, mods: KeyModifiers) {
    // Treat unparseable input as `min` if set, else 0 — same shape as
    // browsers' default for `<input type="number">` arrow clicks
    // against an empty field.
    let parsed = value
        .parse::<f64>()
        .ok()
        .unwrap_or_else(|| opts.min.unwrap_or(0.0));
    let stepped = parsed + (dir as f64) * opts.step * step_scale(mods);
    let clamped = clamp_opt(stepped, opts.min, opts.max);
    *value = format_numeric(clamped, opts.decimals);
}

/// Modifier-key step multiplier. `Shift` → 10× (coarse), `Alt` → 0.1×
/// (fine). When both are held, prefer `Shift` since coarse is the
/// dominant power-user gesture and the simultaneous combo is rarely
/// pressed intentionally.
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
            // Integral: render without trailing ".0" so the canonical
            // round-trip of `numeric_input("0", ...) → click + → "1"`
            // doesn't drift to "1.0".
            format!("{}", n as i64)
        }
        None => format!("{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, UiTarget};
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::tree::Rect;

    fn click(key: &str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn default_is_fixed_width_with_inner_field_filling() {
        // Two regressions, one test: a numeric input dropped into a
        // wide Fill parent must (a) take its declared fixed width
        // ([`DEFAULT_WIDTH`]) rather than stretching across the row,
        // and (b) the inner `Fill(1.0)` text field must still claim
        // the leftover space inside that fixed wrapper — earlier
        // iterations either filled the whole parent or collapsed the
        // field to zero.
        let value = String::from("42");
        let sel = Selection::default();
        let widget = numeric_input("n", &value, &sel, NumericInputOpts::default());
        let mut tree = crate::widgets::form::form([crate::widgets::form::form_item([
            crate::widgets::form::form_control(widget),
        ])]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 320.0, 200.0));

        let row_rect = state.rect_of_key("n").expect("row rect");
        let field_rect = state.rect_of_key("n:field").expect("field rect");
        assert_eq!(
            row_rect.w, DEFAULT_WIDTH,
            "row should keep its fixed default width inside a wide form parent"
        );
        // Inner field fills the leftover space after the two spinner
        // buttons plus inter-child ring gaps.
        let expected_field_w =
            DEFAULT_WIDTH - 2.0 * tokens::CONTROL_HEIGHT - 2.0 * tokens::RING_WIDTH;
        assert!(
            (field_rect.w - expected_field_w).abs() < 0.5,
            "field should take leftover space inside wrapper, got {} expected ~{}",
            field_rect.w,
            expected_field_w,
        );
    }

    #[test]
    fn explicit_width_fill_still_works() {
        // The fixed default is a hint, not a hard cap — apps that want
        // the wider text-input-style behavior chain `.width(...)` and
        // get it. `default_width` is preempted by an explicit `width`.
        let value = String::from("42");
        let sel = Selection::default();
        let widget =
            numeric_input("n", &value, &sel, NumericInputOpts::default()).width(Size::Fill(1.0));
        let mut tree = crate::widgets::form::form([crate::widgets::form::form_item([
            crate::widgets::form::form_control(widget),
        ])]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 320.0, 200.0));
        let row_rect = state.rect_of_key("n").expect("row rect");
        assert!(
            row_rect.w > DEFAULT_WIDTH,
            "explicit `.width(Fill)` should override the fixed default, got {}",
            row_rect.w,
        );
    }

    /// Build a TextInput event targeting `target_key` with `text` as
    /// the composed payload. Used to drive both the routing-gate and
    /// the numeric-character-filter tests.
    fn text_event(target_key: &str, text: &str) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(target_key.to_string()),
            target: Some(UiTarget {
                key: target_key.to_string(),
                node_id: format!("/{target_key}"),
                rect: Rect::new(0.0, 0.0, 100.0, 32.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: None,
            text: Some(text.to_string()),
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::TextInput,
        }
    }

    #[test]
    fn inc_steps_value_up_by_step() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(2.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "5");
    }

    #[test]
    fn dec_steps_value_down_by_step() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(0.5).decimals(1);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:dec")
        ));
        assert_eq!(value, "2.5");
    }

    #[test]
    fn inc_clamps_to_max() {
        let mut value = String::from("99");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().min(0.0).max(100.0);
        // 99 + 1*5 = 104, clamped to 100.
        let opts = opts.step(5.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "100");
    }

    #[test]
    fn dec_clamps_to_min() {
        let mut value = String::from("1");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().min(0.0).max(100.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:dec")
        ));
        assert_eq!(value, "0");
        // Already at min — another dec stays at 0.
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:dec")
        ));
        assert_eq!(value, "0");
    }

    #[test]
    fn empty_value_treated_as_min_when_set() {
        let mut value = String::new();
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().min(10.0).max(100.0);
        // Empty → starts at min (10), then +1 → 11.
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "11");
    }

    #[test]
    fn empty_value_treated_as_zero_when_no_min() {
        let mut value = String::new();
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "1");
    }

    #[test]
    fn unparseable_value_treated_as_zero_when_no_min() {
        let mut value = String::from("abc");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "1");
    }

    #[test]
    fn ignores_unrelated_keys() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        // Different key family — should not match this widget.
        assert!(!apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("other:inc")
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn decimals_format_pads_zeros() {
        let mut value = String::from("0");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(0.10).decimals(2);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        assert_eq!(value, "0.10");
    }

    #[test]
    fn no_decimals_strips_trailing_zero() {
        let mut value = String::from("0");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(1.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc")
        ));
        // 1.0 → "1", not "1.0" (we only fall through to `f64::Display`
        // when the result has a fractional component).
        assert_eq!(value, "1");
    }

    #[test]
    fn text_event_for_other_widget_is_ignored() {
        // Regression: previously `apply_event` forwarded every
        // non-spinner event into `text_input::apply_event`, which
        // doesn't gate on target_key — so typing into a sibling
        // text input would also write into the numeric input.
        let mut value = String::from("42");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        // A TextInput event targeted at a sibling widget should not
        // touch our value at all.
        assert!(!apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &text_event("other-input", "x"),
        ));
        assert_eq!(value, "42");
    }

    #[test]
    fn text_event_filter_rejects_non_numeric_chars() {
        // A TextInput event targeting our inner field whose payload
        // isn't numeric is rolled back so the value never absorbs
        // letters / punctuation. The event still targeted this field,
        // so `apply_event` reports it as consumed (`true`) per the
        // return contract.
        let mut value = String::from("12");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &text_event("n:field", "abc"),
        ));
        assert_eq!(value, "12");
    }

    #[test]
    fn text_event_filter_accepts_partial_numeric_states() {
        // Mid-edit values are kept: bare `-`, trailing `.`, exponent
        // prefix, etc. should all pass the filter even though they
        // aren't yet a complete f64.
        for partial in ["-", "1.", "1.5e", "1.5e+", ".5", "+"] {
            let mut value = String::new();
            let mut sel = Selection::default();
            let opts = NumericInputOpts::default();
            assert!(
                apply_event(
                    &mut value,
                    &mut sel,
                    "n",
                    &opts,
                    &text_event("n:field", partial),
                ),
                "filter should accept partial value {partial:?}",
            );
            assert_eq!(value, partial, "value should equal {partial:?}");
        }
    }

    #[test]
    fn text_event_filter_accepts_full_numeric_paste() {
        let mut value = String::new();
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &text_event("n:field", "42.5"),
        ));
        assert_eq!(value, "42.5");
    }

    #[test]
    fn build_widget_has_three_children_and_correct_keys() {
        let value = String::from("0");
        let sel = Selection::default();
        let opts = NumericInputOpts::default();
        let el = numeric_input("n", &value, &sel, opts);
        assert_eq!(el.key.as_deref(), Some("n"));
        assert_eq!(el.children.len(), 3, "decrement, field, increment");
        assert_eq!(el.children[0].key.as_deref(), Some("n:dec"));
        assert_eq!(el.children[1].key.as_deref(), Some("n:field"));
        assert_eq!(el.children[2].key.as_deref(), Some("n:inc"));
    }

    /// Build a `KeyDown` event routed to `key` for the given physical
    /// key + modifier mask. Used by the arrow-step and Shift/Alt
    /// scaling tests.
    fn key_event(key: &str, ui_key: UiKey, modifiers: KeyModifiers) -> UiEvent {
        use crate::event::KeyPress;
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}"),
                rect: Rect::new(0.0, 0.0, 100.0, 32.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: Some(KeyPress {
                key: ui_key,
                modifiers,
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        }
    }

    #[test]
    fn arrow_up_on_field_steps_up() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(1.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::ArrowUp, KeyModifiers::default()),
        ));
        assert_eq!(value, "4");
    }

    #[test]
    fn arrow_down_on_field_steps_down() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(1.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::ArrowDown, KeyModifiers::default()),
        ));
        assert_eq!(value, "2");
    }

    #[test]
    fn shift_arrow_steps_by_ten_times() {
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(1.0);
        let shift = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::ArrowUp, shift),
        ));
        assert_eq!(value, "13");
    }

    #[test]
    fn alt_arrow_steps_by_one_tenth() {
        // 0.1 step × 0.1 modifier = 0.01; with `.decimals(2)` the
        // formatter pads to "0.01" instead of f64::Display's "0.01".
        let mut value = String::from("0");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(0.1).decimals(2);
        let alt = KeyModifiers {
            alt: true,
            ..KeyModifiers::default()
        };
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::ArrowUp, alt),
        ));
        assert_eq!(value, "0.01");
    }

    #[test]
    fn shift_click_on_inc_button_scales_step() {
        // Click events also honor the modifier mask, so Shift-clicking
        // the `+` button is the pointer counterpart of Shift+ArrowUp.
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(1.0);
        let mut ev = click("n:inc");
        ev.modifiers = KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        };
        assert!(apply_event(&mut value, &mut sel, "n", &opts, &ev));
        assert_eq!(value, "13");
    }

    #[test]
    fn arrow_key_on_field_clamps_to_max() {
        let mut value = String::from("99");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().step(5.0).max(100.0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::ArrowUp, KeyModifiers::default()),
        ));
        assert_eq!(value, "100");
    }

    #[test]
    fn arrow_key_routed_elsewhere_is_ignored() {
        // Arrow keys routed to a different widget mustn't move this
        // numeric input's value — the keyboard handler is strictly
        // gated on `{key}:field` route.
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(!apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("other:field", UiKey::ArrowUp, KeyModifiers::default()),
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn non_arrow_keydown_on_field_falls_through() {
        // Letters, digits, Enter etc. arrive as TextInput events; an
        // unrelated KeyDown (e.g. Tab) is not consumed by the numeric
        // input so focus traversal still works.
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default();
        assert!(!apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::Tab, KeyModifiers::default()),
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn stacked_variant_has_field_and_chevron_column() {
        let value = String::from("0");
        let sel = Selection::default();
        let opts = NumericInputOpts::default().stacked();
        let el = numeric_input("n", &value, &sel, opts);
        assert_eq!(el.key.as_deref(), Some("n"));
        // Two children in the stacked layout: the text field and the
        // chevron column. The inc/dec keys live one level deeper, on
        // the column's children.
        assert_eq!(el.children.len(), 2, "field + chevron column");
        assert_eq!(el.children[0].key.as_deref(), Some("n:field"));
        let column_children = &el.children[1].children;
        assert_eq!(column_children.len(), 2, "chevron-up over chevron-down");
        assert_eq!(column_children[0].key.as_deref(), Some("n:inc"));
        assert_eq!(column_children[1].key.as_deref(), Some("n:dec"));
    }

    #[test]
    fn stacked_variant_keeps_apply_event_contract() {
        // The stacked layout reuses the same routed key vocabulary, so
        // apply_event is layout-agnostic.
        let mut value = String::from("3");
        let mut sel = Selection::default();
        let opts = NumericInputOpts::default().stacked();
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &click("n:inc"),
        ));
        assert_eq!(value, "4");
        assert!(apply_event(
            &mut value,
            &mut sel,
            "n",
            &opts,
            &key_event("n:field", UiKey::ArrowDown, KeyModifiers::default()),
        ));
        assert_eq!(value, "3");
    }
}
