//! Radio group — a column of single-select choices, shaped like the
//! shadcn / Radix RadioGroup primitive (which itself reflects the
//! WAI-ARIA `role="radiogroup"` / `role="radio"` pattern). Per that
//! pattern the group is arrow-navigable: Up/Left and Down/Right move
//! focus among the options ([`crate::tree::ArrowNav::Both`]).
//!
//! The app owns the active value as a field of its choice (`String`,
//! an enum implementing [`std::fmt::Display`], …); the widget is a
//! pure visual + identity carrier — same controlled pattern as
//! [`crate::widgets::tabs`] and [`crate::widgets::select`].
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Prefs { theme: String }
//!
//! impl App for Prefs {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         radio_group("theme", &self.theme, [
//!             ("system", "Match system"),
//!             ("light", "Light"),
//!             ("dark", "Dark"),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         radio::apply_event(&mut self.theme, &event, "theme", |s| {
//!             Some(s.to_string())
//!         });
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}:radio:{value}` — `Click` on a radio item; the app sets
//!   the active value. Use [`radio_option_key`] to format and parse.
//!
//! Chosen to parallel [`crate::widgets::tabs`]'s `{key}:tab:{value}`
//! convention so the controlled-widget vocabulary stays consistent.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom`,
//! `.focusable()` + `.paint_overflow()` for the focus ring, and
//! standard tokens. An app crate can fork this file and produce an
//! equivalent widget against the same public API.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// Outer indicator diameter in logical pixels.
const INDICATOR_OUTER: f32 = 16.0;
/// Inner dot diameter when `selected`.
const INDICATOR_DOT: f32 = 8.0;

/// What a routed [`UiEvent`] means for a controlled radio group keyed
/// `key`.
///
/// Returned by [`classify_event`]; [`apply_event`] is the convenience
/// wrapper that folds the action straight into the app's value field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RadioAction<'a> {
    /// A radio item was clicked or activated. The string is the raw
    /// value token from the option key.
    Select(&'a str),
}

/// Classify a routed [`UiEvent`] against a controlled radio group
/// keyed `key`. Returns `None` for events that aren't for this group.
///
/// Only `Click` / `Activate` event kinds qualify. Apps can call this
/// unconditionally inside their event handler without filtering on
/// `event.kind` first.
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<RadioAction<'a>> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    let value = rest.strip_prefix("radio:")?;
    Some(RadioAction::Select(value))
}

/// Fold a routed [`UiEvent`] into the app's radio-value field for a
/// controlled radio group keyed `key`. Returns `true` if the event
/// was a radio event for this `key`.
///
/// `parse` converts the raw value token back to the app's value type;
/// returning `None` from `parse` ignores the click silently.
pub fn apply_event<V>(
    value: &mut V,
    event: &UiEvent,
    key: &str,
    parse: impl FnOnce(&str) -> Option<V>,
) -> bool {
    let Some(RadioAction::Select(raw)) = classify_event(event, key) else {
        return false;
    };
    if let Some(v) = parse(raw) {
        *value = v;
    }
    true
}

/// Format the routed key emitted when a radio item is clicked.
pub fn radio_option_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:radio:{value}")
}

/// A single radio item — a row of [round indicator, label]. Apps
/// usually let [`radio_group`] build these from `(value, label)`
/// pairs; reach for `radio_item` directly when composing the column
/// by hand (e.g. mixing in helper text or icons per row).
///
/// `group_key` is the parent radio group's key — the routed key on
/// the item is `{group_key}:radio:{value}` (see [`radio_option_key`]).
/// `selected` paints the inner dot.
#[track_caller]
pub fn radio_item(
    group_key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    selected: bool,
) -> El {
    let routed_key = radio_option_key(group_key, &value);

    // Animatable props depending on `selected`. The dot is always in
    // the tree so its opacity + scale can ease both directions; the
    // outer ring's stroke eases between the unselected/selected
    // tokens.
    let stroke = if selected {
        tokens::PRIMARY
    } else {
        tokens::INPUT
    };
    let dot_opacity = if selected { 1.0 } else { 0.0 };
    let dot_scale = if selected { 1.0 } else { 0.4 };

    let indicator = El::new(Kind::Custom("radio-indicator"))
        .metrics_role(MetricsRole::ChoiceControl)
        .axis(Axis::Overlay)
        .align(Align::Center)
        .justify(Justify::Center)
        .default_width(Size::Fixed(INDICATOR_OUTER))
        .default_height(Size::Fixed(INDICATOR_OUTER))
        .radius(tokens::RADIUS_PILL)
        .fill(tokens::CARD)
        .stroke(stroke)
        .animate(Timing::SPRING_STANDARD)
        .child(
            El::new(Kind::Custom("radio-dot"))
                .width(Size::Fixed(INDICATOR_DOT))
                .height(Size::Fixed(INDICATOR_DOT))
                .radius(tokens::RADIUS_PILL)
                .fill(tokens::PRIMARY)
                .opacity(dot_opacity)
                .scale(dot_scale)
                .animate(Timing::SPRING_STANDARD),
        );

    El::new(Kind::Custom("radio_item"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::ChoiceItem)
        .role(Role::Radio)
        .aria_checked(selected)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .key(routed_key)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_2)
        .align(Align::Center)
        .child(indicator)
        .child(text(label).label())
        .default_padding(Sides::xy(0.0, tokens::SPACE_1))
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_radius(tokens::RADIUS_SM)
}

/// A vertical column of [`radio_item`]s for selecting one value from
/// a list. `current` is the currently-selected value, formatted via
/// [`std::fmt::Display`] and compared against each option's `value`.
/// `options` is an iterable of `(value, label)` pairs.
///
/// Per-item routed keys are `{key}:radio:{value}`. Apps fold those
/// back into their value field with [`apply_event`].
#[track_caller]
pub fn radio_group<I, V, L>(
    key: impl Into<String>,
    current: &impl std::fmt::Display,
    options: I,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    // Capture once so the location flows through to each item; see
    // `tabs_list` for the closure / `#[track_caller]` rationale.
    let caller = Location::caller();
    let key = key.into();
    let current_str = current.to_string();
    let items: Vec<El> = options
        .into_iter()
        .map(|(value, label)| {
            let selected = value.to_string() == current_str;
            radio_item(&key, value, label, selected).at_loc(caller)
        })
        .collect();
    // The group container is deliberately not keyed (same rationale
    // as `tabs_list` / `toggle_group`): the space between items is
    // visual chrome, and a keyed container would swallow gap clicks
    // and enroll the whole column as a hover target.
    El::new(Kind::Custom("radio_group"))
        .at_loc(caller)
        .role(Role::RadioGroup)
        .axis(Axis::Column)
        .gap(tokens::SPACE_1)
        .align(Align::Stretch)
        // ARIA radio-group pattern: arrows move within the group
        // regardless of its visual axis — Up/Left previous, Down/Right
        // next (issue #63).
        .arrow_nav(crate::tree::ArrowNav::Both)
        .children(items)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn radio_option_key_matches_widget_format() {
        assert_eq!(radio_option_key("theme", &"dark"), "theme:radio:dark");
        assert_eq!(radio_option_key("page:7", &42u32), "page:7:radio:42");
    }

    #[test]
    fn radio_item_routes_via_radio_option_key() {
        let item = radio_item("theme", "dark", "Dark", false);
        assert_eq!(item.key.as_deref(), Some("theme:radio:dark"));
        assert!(item.focusable);
    }

    #[test]
    fn radio_item_declares_pointer_cursor() {
        let item = radio_item("theme", "dark", "Dark", false);
        assert_eq!(item.cursor, Some(Cursor::Pointer));
    }

    #[test]
    fn unselected_indicator_has_invisible_dot() {
        // The dot stays in the tree so its opacity + scale can ease
        // both directions on selection change.
        let item = radio_item("theme", "dark", "Dark", false);
        let indicator = &item.children[0];
        assert_eq!(indicator.children.len(), 1, "dot stays in the tree");
        assert_eq!(indicator.children[0].opacity, 0.0);
        // Border strong on unselected items so the unselected radio is
        // visually distinguishable from a checkbox.
        assert_eq!(indicator.stroke, Some(tokens::INPUT));
    }

    #[test]
    fn selected_indicator_has_visible_dot_and_primary_stroke() {
        let item = radio_item("theme", "dark", "Dark", true);
        let indicator = &item.children[0];
        assert_eq!(indicator.children.len(), 1);
        let dot = &indicator.children[0];
        assert_eq!(dot.fill, Some(tokens::PRIMARY));
        assert_eq!(dot.opacity, 1.0);
        assert_eq!(indicator.stroke, Some(tokens::PRIMARY));
    }

    #[test]
    fn indicator_and_dot_animate_so_selection_changes_ease() {
        let item = radio_item("theme", "dark", "Dark", false);
        let indicator = &item.children[0];
        assert!(indicator.animate_timing().is_some(), "ring eases stroke");
        assert!(
            indicator.children[0].animate_timing().is_some(),
            "dot eases opacity/scale"
        );
    }

    #[test]
    fn radio_group_marks_only_current_value_visibly_selected() {
        let g = radio_group(
            "theme",
            &"dark",
            [
                ("system", "Match system"),
                ("light", "Light"),
                ("dark", "Dark"),
            ],
        );
        // The container is deliberately unkeyed — gaps between items
        // must not be a hit/hover target (see the constructor comment).
        assert_eq!(g.key, None);
        assert_eq!(g.children.len(), 3);
        let [system, light, dark] = [&g.children[0], &g.children[1], &g.children[2]];
        // Every indicator carries a dot child; only the selected
        // one's dot is visible.
        assert_eq!(system.children[0].children[0].opacity, 0.0);
        assert_eq!(light.children[0].children[0].opacity, 0.0);
        assert_eq!(dark.children[0].children[0].opacity, 1.0);
    }

    #[test]
    fn radio_group_compares_via_display_so_typed_values_work() {
        let g = radio_group(
            "page",
            &7u32,
            [(0u32, "Zero"), (7u32, "Seven"), (42u32, "Forty-two")],
        );
        let [zero, seven, fortytwo] = [&g.children[0], &g.children[1], &g.children[2]];
        assert_eq!(zero.children[0].children[0].opacity, 0.0);
        assert_eq!(seven.children[0].children[0].opacity, 1.0);
        assert_eq!(fortytwo.children[0].children[0].opacity, 0.0);
    }

    #[test]
    fn classify_event_selects_only_on_matching_route() {
        assert_eq!(
            classify_event(&click("theme:radio:dark"), "theme"),
            Some(RadioAction::Select("dark")),
        );
        // Compound parent keys.
        assert_eq!(
            classify_event(&click("page:7:radio:42"), "page:7"),
            Some(RadioAction::Select("42")),
        );
        // The group's own key alone isn't a select target.
        assert_eq!(classify_event(&click("theme"), "theme"), None);
        // Different group prefix.
        assert_eq!(classify_event(&click("other:radio:x"), "theme"), None);
        // Adjacent prefix.
        assert_eq!(classify_event(&click("theme-other:radio:x"), "theme"), None);
        // Wrong suffix shape (no `radio:` prefix on the rest).
        assert_eq!(classify_event(&click("theme:option:x"), "theme"), None);
    }

    #[test]
    fn classify_event_ignores_non_activating_kinds() {
        let mut ev = click("theme:radio:dark");
        ev.kind = UiEventKind::PointerDown;
        assert_eq!(classify_event(&ev, "theme"), None);
        ev.kind = UiEventKind::Activate;
        assert_eq!(
            classify_event(&ev, "theme"),
            Some(RadioAction::Select("dark")),
        );
    }

    #[test]
    fn apply_event_folds_actions_into_value() {
        let mut theme = String::from("system");
        assert!(apply_event(
            &mut theme,
            &click("theme:radio:dark"),
            "theme",
            |s| Some(s.to_string()),
        ));
        assert_eq!(theme, "dark");

        // Unrelated event leaves state alone.
        assert!(!apply_event(&mut theme, &click("save"), "theme", |s| Some(
            s.to_string()
        ),));
        assert_eq!(theme, "dark");
    }

    #[test]
    fn apply_event_silently_ignores_unparseable_values() {
        let mut page: u32 = 1;
        assert!(apply_event(
            &mut page,
            &click("page:radio:not-a-number"),
            "page",
            |s| s.parse::<u32>().ok(),
        ));
        assert_eq!(page, 1);
    }
}
