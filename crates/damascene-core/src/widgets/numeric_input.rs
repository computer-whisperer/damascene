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
//! # Units and other adornments
//!
//! A single trailing unit — the overwhelmingly common case for a
//! numeric field — is [`NumericInputOpts::suffix`], which paints a
//! muted label inside the field's own trough between the value and the
//! steppers. "A number with a unit and a spinner" is then one widget:
//!
//! ```ignore
//! numeric_input("layer_height", &self.layer_height, &self.selection,
//!     NumericInputOpts::default().step(0.05).decimals(2).suffix("mm"))
//! ```
//!
//! Anything richer — a leading icon or currency prefix, a trailing
//! button, a prefix *and* a suffix — is the
//! [`crate::widgets::input_group`] anatomy, which makes the group
//! itself the trough and renders the inputs inside it bare:
//!
//! ```ignore
//! input_group([
//!     input_group_addon(icon("ruler")),
//!     numeric_input("w", &self.w, &self.selection, NumericInputOpts::default()),
//!     input_group_text("mm"),
//! ])
//! ```
//!
//! The trade is the focus ring: inside an `input_group` the ring
//! stays on the inner input's rect rather than the group border (the
//! group's documented v1 behaviour), whereas `suffix` keeps the ring
//! around the whole trough. Prefer `suffix` when it suffices.
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

use crate::a11y::Role;
use crate::event::{KeyModifiers, NamedKey, UiEvent, UiEventKind};
use crate::selection::Selection;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::{button, icon_button};
use crate::widgets::text_input::{
    self, TextInputOpts, apply_event_with as text_input_apply, text_input_with,
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
    /// Muted unit label — `"mm"`, `"°C"`, `"%"` — rendered *inside*
    /// the field's trough, between the value and the trailing stepper
    /// affordance. "A number with a unit and a spinner" is then one
    /// widget rather than a nested composition.
    ///
    /// Forwarded verbatim to [`TextInputOpts::suffix`], so the value's
    /// viewport stops before the label (long values clip rather than
    /// sliding under it), the label takes no pointer events, and the
    /// focus ring still wraps the whole trough. Because the reserved
    /// band is derived from the opts on both the build and the event
    /// path, pass the *same* `NumericInputOpts` to [`numeric_input`]
    /// and [`apply_event`] — the contract `min` / `max` / `step`
    /// already impose.
    ///
    /// For richer adornments — a leading icon, a trailing button, a
    /// prefix and a suffix at once — compose the field with
    /// [`crate::widgets::input_group::input_group`] instead; see this
    /// module's docs.
    pub suffix: Option<&'a str>,
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
            suffix: None,
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
    /// Set the muted unit label rendered inside the trough (see
    /// [`NumericInputOpts::suffix`]).
    pub fn suffix(mut self, s: &'a str) -> Self {
        self.suffix = Some(s);
        self
    }
}

/// A numeric input field. Defaults to the flanked layout
/// `[−] [text_input] [+]`; opt into the stacked-chevron variant with
/// [`NumericInputOpts::stacked`] and a trailing unit label inside the
/// trough with [`NumericInputOpts::suffix`] (for adorned shapes beyond
/// a plain suffix, see this module's "Units and other adornments").
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

    let text_opts = field_opts(&opts);
    let field_key = format!("{key}:field");
    // The field is the focusable surface; announce it as a spinbutton
    // (overriding the inner text_input's textbox role) with the
    // formatted text it already displays. A numeric range is only
    // stamped when the options carry real bounds — never invented.
    let mut field = text_input_with(&field_key, value, selection, text_opts)
        .width(Size::Fill(1.0))
        .role(Role::SpinButton)
        .aria_value_text(value);
    if let (Some(min), Some(max), Ok(now)) = (opts.min, opts.max, value.parse::<f64>()) {
        field = field.aria_value(now, min, max);
    }

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
            // Explicit name: "−" name-from-content would announce as
            // the bare glyph, which not every screen reader verbalizes.
            .aria_label("Decrease")
            .ghost()
            .width(Size::Fixed(tokens::CONTROL_HEIGHT))
            .height(Size::Fixed(tokens::CONTROL_HEIGHT));
        let inc = button("+")
            .at_loc(caller)
            .key(format!("{key}:inc"))
            .aria_label("Increase")
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
        // A unit label eats trough width, so the fixed default grows by
        // exactly the band it reserves — otherwise adding `.suffix("mm")`
        // would silently cost the digits ~29px of room. An explicit
        // `.width(...)` still preempts this (it's a `default_`).
        .default_width(Size::Fixed(
            DEFAULT_WIDTH + text_input::suffix_reserve(&text_opts),
        ))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

/// The inner field's [`TextInputOpts`], derived from the numeric opts.
///
/// Built by one function because the build path and [`apply_event`]
/// must agree exactly: tabular numerals change the advances the caret
/// is measured against, and a `suffix` narrows the scrolling text
/// viewport. A divergence between the two shows up as clicks landing
/// on the wrong byte in a scrolled field.
fn field_opts<'a>(opts: &NumericInputOpts<'a>) -> TextInputOpts<'a> {
    // Tabular numerals so digits don't shift as the value spins;
    // text_input threads the same flag through caret geometry.
    let mut text_opts = TextInputOpts::default().tabular_numerals();
    if let Some(p) = opts.placeholder {
        text_opts = text_opts.placeholder(p);
    }
    if let Some(s) = opts.suffix {
        text_opts = text_opts.suffix(s);
    }
    text_opts
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
///
/// The seam edges also drop their `hit_overflow`. `icon_button` inflates
/// its hit rect by [`tokens::HIT_OVERFLOW`] on all four sides, and with
/// the two chevrons flush that band overlapped by the full column width
/// × `2 * HIT_OVERFLOW` — an invisible strip on the seam where a click
/// aimed at `⌃` steps the value *down*, which
/// [`FindingKind::HitOverflowCollision`][crate::bundle::lint::FindingKind::HitOverflowCollision]
/// flagged on a bare `numeric_input(...).stacked()`. Same trade
/// [`join_row`][crate::widgets::button_group] makes for joined groups:
/// flush neighbours have no whitespace for an expanded target to live
/// in. Only the shared edge is zeroed — the column's outer edges keep
/// their band, since nothing is flush against them (the field sits a
/// `RING_WIDTH` gap away).
fn stacked_chevron_column(key: &str, caller: &'static Location<'static>) -> El {
    let half_h = (tokens::CONTROL_HEIGHT * 0.5).floor();
    let seam = tokens::HIT_OVERFLOW;
    let inc = icon_button("chevron-up")
        .at_loc(caller)
        .key(format!("{key}:inc"))
        .aria_label("Increase")
        .ghost()
        .icon_size(tokens::ICON_XS)
        .focus_ring_inside()
        .hit_overflow(Sides {
            bottom: 0.0,
            ..Sides::all(seam)
        })
        .width(Size::Fixed(STACKED_CHEVRON_WIDTH))
        .height(Size::Fixed(half_h));
    let dec = icon_button("chevron-down")
        .at_loc(caller)
        .key(format!("{key}:dec"))
        .aria_label("Decrease")
        .ghost()
        .icon_size(tokens::ICON_XS)
        .focus_ring_inside()
        .hit_overflow(Sides {
            top: 0.0,
            ..Sides::all(seam)
        })
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
        let dir = match kp.logical.named() {
            Some(NamedKey::ArrowUp) => Some(1),
            Some(NamedKey::ArrowDown) => Some(-1),
            _ => None,
        };
        if let Some(d) = dir {
            step_value(value, opts, d, kp.modifiers);
            return true;
        }
    }

    // Only consume events that actually target the inner field. text_input
    // route-gates its *pointer* arms, but key/text events are focus-routed and
    // ungated there; forwarding every key event would steal keystrokes meant
    // for sibling widgets and dump them into our value. Gate here on the field
    // key (this also drops pointer events not aimed at the field).
    if event.target_key() != Some(field_key.as_str()) {
        return false;
    }

    // Same opts as the build path — tabular numerals and the suffix
    // band included, so event-time pointer→byte mapping uses the
    // rendered advances and the rendered viewport width.
    let text_opts = field_opts(opts);

    // Run the text_input edit, then revert if the post-edit value
    // contains non-numeric characters. The filter is permissive: any
    // char in `[0-9.eE+\-]` is allowed so mid-edit states like `"-"`,
    // `"1."`, or `"1.5e+"` keep the cursor where the user expects
    // while the value isn't yet a complete f64.
    let prev_value = value.clone();
    let prev_selection = selection.clone();
    let changed = text_input_apply(value, selection, event, &field_key, &text_opts);
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
    use crate::event::{KeyModifiers, LogicalKey, PhysicalKey, UiTarget};
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
                node_id: format!("/{target_key}").into(),
                rect: Rect::new(0.0, 0.0, 100.0, 32.0),
                tooltip: None,
                scroll_offset_y: 0.0,
                content_inset: Sides::zero(),
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
    fn key_event(key: &str, ui_key: LogicalKey, modifiers: KeyModifiers) -> UiEvent {
        use crate::event::KeyPress;
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}").into(),
                rect: Rect::new(0.0, 0.0, 100.0, 32.0),
                tooltip: None,
                scroll_offset_y: 0.0,
                content_inset: Sides::zero(),
            }),
            pointer: None,
            key_press: Some(KeyPress {
                logical: ui_key,
                physical: PhysicalKey::Unidentified,
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
            &key_event(
                "n:field",
                LogicalKey::Named(NamedKey::ArrowUp),
                KeyModifiers::default()
            ),
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
            &key_event(
                "n:field",
                LogicalKey::Named(NamedKey::ArrowDown),
                KeyModifiers::default()
            ),
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
            &key_event("n:field", LogicalKey::Named(NamedKey::ArrowUp), shift),
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
            &key_event("n:field", LogicalKey::Named(NamedKey::ArrowUp), alt),
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
            &key_event(
                "n:field",
                LogicalKey::Named(NamedKey::ArrowUp),
                KeyModifiers::default()
            ),
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
            &key_event(
                "other:field",
                LogicalKey::Named(NamedKey::ArrowUp),
                KeyModifiers::default()
            ),
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
            &key_event(
                "n:field",
                LogicalKey::Named(NamedKey::Tab),
                KeyModifiers::default()
            ),
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
    fn stacked_chevrons_drop_hit_overflow_on_the_shared_seam() {
        let value = String::from("0");
        let sel = Selection::default();
        let el = numeric_input("n", &value, &sel, NumericInputOpts::default().stacked());
        let chevrons = &el.children[1].children;
        let (inc, dec) = (&chevrons[0], &chevrons[1]);
        // The seam: `⌃`'s bottom band would sit inside `⌄`'s rect and
        // vice versa — invisible, and resolved by paint order, so a
        // click on the bottom edge of `⌃` would step the value *down*.
        assert_eq!(inc.hit_overflow.bottom, 0.0, "up chevron's seam edge");
        assert_eq!(dec.hit_overflow.top, 0.0, "down chevron's seam edge");
        // The outer edges are unaffected — nothing is flush there, so
        // the affordance keeps its band.
        assert_eq!(inc.hit_overflow.top, tokens::HIT_OVERFLOW);
        assert_eq!(inc.hit_overflow.left, tokens::HIT_OVERFLOW);
        assert_eq!(dec.hit_overflow.bottom, tokens::HIT_OVERFLOW);
        assert_eq!(dec.hit_overflow.right, tokens::HIT_OVERFLOW);
    }

    #[test]
    fn stacked_variant_is_lint_clean() {
        use crate::bundle::lint::FindingKind;
        // Regression: a bare `numeric_input(...).stacked()` used to trip
        // `HitOverflowCollision` — `icon_button`'s all-sides
        // `HIT_OVERFLOW` band on two flush chevrons overlapped by the
        // column's full width x `2 * HIT_OVERFLOW`.
        let value = String::from("42");
        let sel = Selection::default();
        let mut root = crate::tree::column([numeric_input(
            "n",
            &value,
            &sel,
            NumericInputOpts::default().stacked(),
        )])
        .padding(Sides::all(tokens::SPACE_4));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 120.0));
        let report = crate::bundle::lint::lint(&root, &state, &crate::theme::Theme::default());
        let relevant: Vec<_> = report
            .findings
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FindingKind::HitOverflowCollision | FindingKind::FocusRingObscured
                )
            })
            .collect();
        assert!(
            relevant.is_empty(),
            "a bare stacked numeric_input must not trip flush-neighbour lints:\n{}",
            report.text(),
        );
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
            &key_event(
                "n:field",
                LogicalKey::Named(NamedKey::ArrowDown),
                KeyModifiers::default()
            ),
        ));
        assert_eq!(value, "3");
    }

    #[test]
    fn suffix_renders_muted_inside_the_trough_and_keeps_the_steppers() {
        let value = String::from("42");
        let sel = Selection::default();
        let el = numeric_input("n", &value, &sel, NumericInputOpts::default().suffix("mm"));

        // Steppers are untouched — the unit lives inside the field,
        // not in place of an affordance.
        assert_eq!(el.children.len(), 3, "decrement, field, increment");
        assert_eq!(el.children[0].key.as_deref(), Some("n:dec"));
        assert_eq!(el.children[2].key.as_deref(), Some("n:inc"));

        let field = &el.children[1];
        assert_eq!(field.key.as_deref(), Some("n:field"));
        assert_eq!(
            field.surface_role,
            SurfaceRole::Input,
            "the field is still the trough, so the focus ring wraps the unit too"
        );
        assert_eq!(field.children.len(), 2, "text viewport + unit cell");

        let unit = &field.children[1];
        assert!(
            unit.key.is_none() && !unit.focusable,
            "the unit label must not capture pointer events"
        );
        let label = &unit.children[0];
        assert_eq!(label.text.as_deref(), Some("mm"));
        assert_eq!(label.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn stacked_suffix_sits_between_the_value_and_the_chevrons() {
        let value = String::from("42");
        let sel = Selection::default();
        let mut tree = numeric_input(
            "n",
            &value,
            &sel,
            NumericInputOpts::default().stacked().suffix("°C"),
        );
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 320.0, 48.0));

        let field = &tree.children[0];
        assert_eq!(field.key.as_deref(), Some("n:field"));
        let viewport = field.children[0].computed_rect;
        let unit = field.children[1].computed_rect;
        let chevrons = tree.children[1].computed_rect;
        assert!(
            viewport.x + viewport.w <= unit.x + 0.01,
            "unit follows the value"
        );
        assert!(
            unit.x + unit.w <= field.computed_rect.x + field.computed_rect.w + 0.01,
            "unit stays inside the trough"
        );
        assert!(
            field.computed_rect.x + field.computed_rect.w <= chevrons.x + 0.01,
            "the stepper column still trails the trough"
        );
    }

    #[test]
    fn suffix_widens_the_fixed_default_by_the_band_it_reserves() {
        // Adding a unit must not silently cost the digits their room.
        let sel = Selection::default();
        let plain = numeric_input("n", "42", &sel, NumericInputOpts::default());
        let suffixed = numeric_input("n", "42", &sel, NumericInputOpts::default().suffix("mm"));
        let reserve = text_input::suffix_reserve(&TextInputOpts::default().suffix("mm"));

        let Size::Fixed(plain_w) = plain.width else {
            panic!("plain default width")
        };
        let Size::Fixed(suffixed_w) = suffixed.width else {
            panic!("suffixed default width")
        };
        assert_eq!(plain_w, DEFAULT_WIDTH);
        assert!((suffixed_w - (DEFAULT_WIDTH + reserve)).abs() < 0.01);
        assert!(
            !suffixed.explicit_width,
            "still a default — `.width(...)` must preempt it"
        );
    }

    /// Content inset of the inner `{key}:field` after the metrics pass
    /// has stamped the Input rung — the origin `text_input`'s pointer
    /// math anchors on, and 2px off `tokens::SPACE_3` at the stock
    /// `ComponentSize::Sm`.
    fn field_inset() -> Sides {
        let sel = Selection::default();
        let mut el = numeric_input("n", "0", &sel, NumericInputOpts::default());
        crate::Theme::default().apply_metrics(&mut el);
        el.children
            .iter()
            .find(|c| c.key.as_deref() == Some("n:field"))
            .expect("field child")
            .content_inset()
    }

    #[test]
    fn suffix_does_not_shift_the_caret_math() {
        // `apply_event` rebuilds the field opts on the event path; the
        // suffix must ride along, and a click in the value area must
        // land on the same byte with or without it.
        let target = UiTarget {
            key: "n:field".to_string(),
            node_id: "/n:field".into(),
            rect: Rect::new(20.0, 0.0, 160.0, 32.0),
            tooltip: None,
            scroll_offset_y: 0.0,
            // The stamped inset of the inner field at the stock rung,
            // i.e. what hit_test would snapshot — not `SPACE_3`.
            content_inset: field_inset(),
        };
        let click_at = |x: f32| UiEvent {
            path: None,
            key: Some("n:field".to_string()),
            target: Some(target.clone()),
            pointer: Some((x, 16.0)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::PointerDown,
        };

        let x = target.rect.x + field_inset().left + 14.0;
        let mut plain_value = String::from("12345");
        let mut plain_sel = Selection::default();
        apply_event(
            &mut plain_value,
            &mut plain_sel,
            "n",
            &NumericInputOpts::default(),
            &click_at(x),
        );

        let mut unit_value = String::from("12345");
        let mut unit_sel = Selection::default();
        apply_event(
            &mut unit_value,
            &mut unit_sel,
            "n",
            &NumericInputOpts::default().suffix("mm"),
            &click_at(x),
        );

        assert_eq!(
            plain_sel.within("n:field"),
            unit_sel.within("n:field"),
            "the caret anchors on the field's left padding, which the unit doesn't move"
        );
        assert!(plain_sel.within("n:field").is_some(), "the click landed");
    }

    /// Post-#117 regression guard: the fixed-width spinner buttons' −/+
    /// glyphs spill into the padding band by design — the ellipsis budget
    /// must be the border box, not the content box, or they degrade to
    /// "…".
    #[test]
    fn spinner_glyphs_render_whole_not_ellipsized() {
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::layout::layout;
        use crate::state::UiState;
        use crate::tree::Rect;

        let sel = Selection::default();
        let mut tree = numeric_input("n", "3", &sel, NumericInputOpts::default());
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 240.0, 48.0));
        let ops = draw_ops(&tree, &state);
        let labels: Vec<&str> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::GlyphRun { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"−"), "decrement glyph intact: {labels:?}");
        assert!(labels.contains(&"+"), "increment glyph intact: {labels:?}");
        assert!(
            !labels.iter().any(|l| l.contains('\u{2026}')),
            "nothing ellipsized at natural size: {labels:?}"
        );
    }
}
