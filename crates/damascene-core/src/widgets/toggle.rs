//! Toggle — pressed/unpressed two-state buttons, used either standalone
//! or grouped. Mirrors the shadcn / Radix Toggle + ToggleGroup primitives
//! (which themselves reflect the WAI-ARIA `role="button"` with
//! `aria-pressed` and `role="group"` patterns), so LLM authors trained
//! on web UI find the same shape here. Grouped rows are
//! arrow-navigable: Left/Right move focus among the items
//! ([`crate::tree::ArrowNav::Horizontal`]).
//!
//! Three flavors, three state shapes:
//!
//! - [`toggle`] — a single binary on/off button. State is a `bool`.
//! - [`toggle_group`] — a row of mutually-exclusive options. State is
//!   the value of the currently-pressed item. Looks like a panel-less
//!   [`crate::widgets::tabs`] row; reach for that instead when each
//!   option has associated content.
//! - [`toggle_group_multi`] — a row of independent on/off options.
//!   State is a set of pressed values. Use for filter chips, format
//!   toggles, anything where multiple options can be on at once.
//!
//! The app owns the state; the widget is a pure visual + identity
//! carrier — same controlled pattern used by [`crate::widgets::radio`]
//! and [`crate::widgets::tabs`].
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use std::collections::HashSet;
//!
//! struct App {
//!     wrap: bool,                 // standalone toggle
//!     view: String,               // single-select group
//!     filters: HashSet<String>,   // multi-select group
//! }
//!
//! impl damascene_core::App for App {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             toggle("wrap", self.wrap, "Wrap lines"),
//!             toggle_group("view", &self.view, [
//!                 ("list", "List"),
//!                 ("grid", "Grid"),
//!                 ("kanban", "Kanban"),
//!             ]),
//!             toggle_group_multi("filters", &self.filters, [
//!                 ("open", "Open"),
//!                 ("draft", "Draft"),
//!                 ("merged", "Merged"),
//!             ]),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         toggle::apply_event_pressed(&mut self.wrap, &event, "wrap");
//!         toggle::apply_event_single(&mut self.view, &event, "view", |s| {
//!             Some(s.to_string())
//!         });
//!         toggle::apply_event_multi(&mut self.filters, &event, "filters");
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - Standalone toggle: `{key}` — `Click` flips the bool.
//! - Group items: `{group_key}:toggle:{value}` — `Click` selects (single)
//!   or flips (multi) that value. Use [`toggle_option_key`] to format
//!   and parse.
//!
//! Chosen to parallel [`crate::widgets::tabs`]'s `{key}:tab:{value}` and
//! [`crate::widgets::radio`]'s `{key}:radio:{value}` so the controlled-
//! widget vocabulary stays consistent.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom`,
//! `.focusable()` + `.paint_overflow()` for the focus ring, and
//! `.current()` / `.ghost()` for pressed-vs-unpressed. An app crate can
//! fork this file and produce an equivalent widget against the same
//! public API.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::collections::HashSet;
use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// What a routed [`UiEvent`] means for a controlled toggle keyed `key`.
///
/// Returned by [`classify_event`]; the per-flavor `apply_event_*`
/// helpers are the convenience wrappers that fold the action straight
/// into the app's value field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToggleAction<'a> {
    /// A standalone toggle was clicked. The app flips its bool.
    Pressed,
    /// A toggle inside a group was clicked. The string is the raw
    /// value token from the option key. Whether this means "set" or
    /// "flip" is the app's call (single vs multi mode).
    Selected(&'a str),
}

/// Classify a routed [`UiEvent`] against a controlled toggle keyed
/// `key`. Returns `None` for events that aren't for this toggle.
///
/// Only `Click` / `Activate` event kinds qualify. Apps can call this
/// unconditionally inside their event handler without filtering on
/// `event.kind` first.
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<ToggleAction<'a>> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    if routed == key {
        return Some(ToggleAction::Pressed);
    }
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    let value = rest.strip_prefix("toggle:")?;
    Some(ToggleAction::Selected(value))
}

/// Fold a routed [`UiEvent`] into a standalone toggle's `bool` field.
/// Returns `true` if the event was a press for this `key`.
pub fn apply_event_pressed(pressed: &mut bool, event: &UiEvent, key: &str) -> bool {
    let Some(ToggleAction::Pressed) = classify_event(event, key) else {
        return false;
    };
    *pressed = !*pressed;
    true
}

/// Fold a routed [`UiEvent`] into a single-select toggle group's value
/// field. Returns `true` if the event was a toggle event for this
/// `key`.
///
/// `parse` converts the raw value token back to the app's value type;
/// returning `None` from `parse` ignores the click silently. Re-clicking
/// the already-pressed item is a no-op (the value stays set), matching
/// shadcn's `<ToggleGroup type="single">` semantics — for "click to
/// clear" use [`apply_event_multi`].
pub fn apply_event_single<V>(
    value: &mut V,
    event: &UiEvent,
    key: &str,
    parse: impl FnOnce(&str) -> Option<V>,
) -> bool {
    let Some(ToggleAction::Selected(raw)) = classify_event(event, key) else {
        return false;
    };
    if let Some(v) = parse(raw) {
        *value = v;
    }
    true
}

/// Fold a routed [`UiEvent`] into a multi-select toggle group's
/// `HashSet<String>` field, flipping the clicked value's membership.
/// Returns `true` if the event was a toggle event for this `key`.
pub fn apply_event_multi(set: &mut HashSet<String>, event: &UiEvent, key: &str) -> bool {
    let Some(ToggleAction::Selected(raw)) = classify_event(event, key) else {
        return false;
    };
    if !set.remove(raw) {
        set.insert(raw.to_string());
    }
    true
}

/// Format the routed key emitted when a toggle group item is clicked.
pub fn toggle_option_key(group_key: &str, value: &impl std::fmt::Display) -> String {
    format!("{group_key}:toggle:{value}")
}

/// A standalone two-state button. `pressed` paints the active surface
/// (accent fill + accent foreground + semibold), unpressed renders as
/// ghost. Click on the routed key `key` flips the bool — fold the
/// event back with [`apply_event_pressed`].
#[track_caller]
pub fn toggle(key: impl Into<String>, pressed: bool, label: impl Into<String>) -> El {
    toggle_button(Location::caller(), key.into(), pressed, label)
}

/// A single item inside a toggle group. Apps usually let
/// [`toggle_group`] / [`toggle_group_multi`] build these from
/// `(value, label)` pairs; reach for `toggle_item` directly when
/// composing the row by hand (e.g. mixing in icons or badges per
/// option).
///
/// `group_key` is the parent group's key — the routed key on the item
/// is `{group_key}:toggle:{value}` (see [`toggle_option_key`]).
/// `selected` paints the pressed surface.
#[track_caller]
pub fn toggle_item(
    group_key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    selected: bool,
) -> El {
    let routed_key = toggle_option_key(group_key, &value);
    toggle_button(Location::caller(), routed_key, selected, label)
}

/// A row of mutually-exclusive toggle items — pick one. `current` is
/// the currently-pressed value, formatted via [`std::fmt::Display`]
/// and compared against each option's `value`. `options` is an
/// iterable of `(value, label)` pairs.
///
/// Per-item routed keys are `{key}:toggle:{value}`. Apps fold those
/// back into their value field with [`apply_event_single`].
///
/// Use this for view-mode pickers (list / grid / kanban), text
/// alignment (left / center / right), and similar one-of-N choices
/// without panel content. When each option owns a panel, reach for
/// [`crate::widgets::tabs`] instead.
#[track_caller]
pub fn toggle_group<I, V, L>(
    key: impl Into<String>,
    current: &impl std::fmt::Display,
    options: I,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    let caller = Location::caller();
    let key = key.into();
    let current_str = current.to_string();
    let items: Vec<El> = options
        .into_iter()
        .map(|(value, label)| {
            let selected = value.to_string() == current_str;
            toggle_item(&key, value, label, selected).at_loc(caller)
        })
        .collect();
    toggle_group_row(caller, items)
}

/// A row of independent on/off toggle items — flip each
/// independently. `selected` is the set of currently-pressed values
/// (compared as strings, formatted from each option's `value`).
/// `options` is an iterable of `(value, label)` pairs.
///
/// Per-item routed keys are `{key}:toggle:{value}`. Apps fold those
/// back into their set with [`apply_event_multi`].
///
/// Use this for filter chips, formatting toolbars (B / I / U), and
/// anything where multiple options coexist.
#[track_caller]
pub fn toggle_group_multi<I, V, L>(
    key: impl Into<String>,
    selected: &HashSet<String>,
    options: I,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    let caller = Location::caller();
    let key = key.into();
    let items: Vec<El> = options
        .into_iter()
        .map(|(value, label)| {
            let value_str = value.to_string();
            let pressed = selected.contains(&value_str);
            toggle_item(&key, value, label, pressed).at_loc(caller)
        })
        .collect();
    toggle_group_row(caller, items)
}

fn toggle_button(
    caller: &'static Location<'static>,
    routed_key: String,
    pressed: bool,
    label: impl Into<String>,
) -> El {
    let base = El::new(Kind::Custom("toggle"))
        .at_loc(caller)
        // Surface profile so `.current()` paints the accent fill
        // instead of taking the text-only branch (matches the
        // tab_trigger setup that also flips between `.current()` and
        // `.ghost()` per state).
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Button)
        // ARIA toggle-button pattern: role="button" + aria-pressed.
        .role(Role::Button)
        .aria_pressed(pressed)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .key(routed_key)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0));
    let styled = if pressed {
        base.current()
    } else {
        base.ghost()
    };
    styled.animate(Timing::SPRING_QUICK)
}

fn toggle_group_row(caller: &'static Location<'static>, items: Vec<El>) -> El {
    // The row itself is deliberately not keyed (same rationale as
    // `tabs_list`): the space between items is visual chrome, not an
    // interactive target. A keyed row would also make gap clicks route
    // the bare group key, which `classify_event` reads as a standalone
    // toggle's `Pressed` — a phantom state flip from dead space.
    El::new(Kind::Custom("toggle_group"))
        .at_loc(caller)
        .role(Role::Group)
        .axis(Axis::Row)
        .gap(tokens::SPACE_1)
        .align(Align::Center)
        // ToggleGroup pattern: the row is one arrow-navigable group —
        // Left / Right move between items (issue #63).
        .arrow_nav(crate::tree::ArrowNav::Horizontal)
        .children(items)
        .width(Size::Hug)
        .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_test::hit_test_target;
    use crate::layout::layout;
    use crate::state::UiState;

    fn click(key: &str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn toggle_group_gap_click_is_not_a_pressed_action() {
        // Regression for #62: a keyed group container made clicks in
        // the gap between items route the bare group key, which
        // classify_event reads as a standalone toggle's `Pressed`.
        let mut group = toggle_group("view", &"list", [("list", "List"), ("grid", "Grid")]);
        let mut state = UiState::new();
        layout(&mut group, &mut state, Rect::new(0.0, 0.0, 240.0, 60.0));

        let first = group.children[0].computed_rect;
        let second = group.children[1].computed_rect;
        assert!(
            second.x > first.x + first.w,
            "test requires the group's configured gap to be present"
        );

        let item_target = hit_test_target(
            &group,
            &state,
            (first.x + first.w / 2.0, first.y + first.h / 2.0),
        )
        .expect("toggle item should still be interactive");
        assert_eq!(item_target.key, "view:toggle:list");

        let gap_x = (first.x + first.w + second.x) / 2.0;
        let gap_y = first.y + first.h / 2.0;
        assert_eq!(
            hit_test_target(&group, &state, (gap_x, gap_y)),
            None,
            "the gap between toggles must not route the bare group key"
        );
    }

    #[test]
    fn classify_standalone_returns_pressed() {
        let event = click("wrap");
        assert_eq!(classify_event(&event, "wrap"), Some(ToggleAction::Pressed),);
    }

    #[test]
    fn classify_group_returns_selected_with_value() {
        let event = click("view:toggle:grid");
        assert_eq!(
            classify_event(&event, "view"),
            Some(ToggleAction::Selected("grid")),
        );
    }

    #[test]
    fn classify_unrelated_event_is_none() {
        let event = click("other");
        assert!(classify_event(&event, "view").is_none());
    }

    #[test]
    fn apply_pressed_flips_bool() {
        let mut wrap = false;
        let event = click("wrap");
        assert!(apply_event_pressed(&mut wrap, &event, "wrap"));
        assert!(wrap);
        assert!(apply_event_pressed(&mut wrap, &event, "wrap"));
        assert!(!wrap);
    }

    #[test]
    fn apply_pressed_ignores_other_keys() {
        let mut wrap = false;
        let event = click("other");
        assert!(!apply_event_pressed(&mut wrap, &event, "wrap"));
        assert!(!wrap);
    }

    #[test]
    fn apply_single_sets_value_via_parser() {
        let mut view = String::from("list");
        let event = click("view:toggle:grid");
        assert!(apply_event_single(&mut view, &event, "view", |s| {
            Some(s.to_string())
        }));
        assert_eq!(view, "grid");
    }

    #[test]
    fn apply_single_ignores_unparseable_value() {
        let mut view = String::from("list");
        let event = click("view:toggle:grid");
        // Parser rejects everything → value stays "list" but the
        // event is still consumed (returns true).
        assert!(apply_event_single(&mut view, &event, "view", |_| {
            None::<String>
        }));
        assert_eq!(view, "list");
    }

    #[test]
    fn apply_multi_flips_membership() {
        let mut set: HashSet<String> = HashSet::new();
        let event = click("filters:toggle:open");
        assert!(apply_event_multi(&mut set, &event, "filters"));
        assert!(set.contains("open"));
        // Second click removes it.
        assert!(apply_event_multi(&mut set, &event, "filters"));
        assert!(!set.contains("open"));
    }

    #[test]
    fn standalone_toggle_routes_via_its_key() {
        let t = toggle("wrap", false, "Wrap");
        assert_eq!(t.key.as_deref(), Some("wrap"));
        assert!(t.focusable);
        assert_eq!(t.cursor, Some(Cursor::Pointer));
    }

    #[test]
    fn toggle_option_key_matches_widget_format() {
        assert_eq!(toggle_option_key("view", &"grid"), "view:toggle:grid");
        assert_eq!(toggle_option_key("page:7", &42u32), "page:7:toggle:42");
    }

    #[test]
    fn standalone_toggle_pressed_renders_current_surface() {
        let pressed = toggle("wrap", true, "Wrap");
        // `.current()` paints with ACCENT fill on Custom surface kinds.
        assert_eq!(pressed.fill, Some(tokens::ACCENT));
    }

    #[test]
    fn standalone_toggle_unpressed_is_ghost() {
        let unpressed = toggle("wrap", false, "Wrap");
        // `.ghost()` clears fill and stroke.
        assert!(unpressed.fill.is_none());
        assert!(unpressed.stroke.is_none());
    }

    #[test]
    fn group_marks_only_current_value_as_pressed() {
        let group = toggle_group("view", &"grid", [("list", "List"), ("grid", "Grid")]);
        let [list_item, grid_item] = [&group.children[0], &group.children[1]];
        assert!(list_item.fill.is_none(), "non-current item is ghost");
        assert_eq!(
            grid_item.fill,
            Some(tokens::ACCENT),
            "current item paints accent",
        );
        assert_eq!(list_item.key.as_deref(), Some("view:toggle:list"));
        assert_eq!(grid_item.key.as_deref(), Some("view:toggle:grid"));
    }

    #[test]
    fn group_multi_marks_each_pressed_value() {
        let mut selected = HashSet::new();
        selected.insert("open".to_string());
        selected.insert("draft".to_string());
        let group = toggle_group_multi(
            "filters",
            &selected,
            [("open", "Open"), ("draft", "Draft"), ("merged", "Merged")],
        );
        let [open, draft, merged] = [&group.children[0], &group.children[1], &group.children[2]];
        assert_eq!(open.fill, Some(tokens::ACCENT));
        assert_eq!(draft.fill, Some(tokens::ACCENT));
        assert!(merged.fill.is_none(), "unpressed multi item is ghost");
    }
}
