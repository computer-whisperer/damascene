//! Select / dropdown menu — a trigger surface that displays the
//! currently chosen value paired with a dropdown popover of options.
//! Authored as two compositional pieces (trigger + menu) so apps place
//! the trigger inline in their layout and compose the menu at the root
//! of the El tree (the popover paradigm — see `widgets/popover.rs`).
//!
//! This is the **value picker** sibling of
//! [`crate::widgets::dropdown_menu`]: items here carry a value the app
//! binds via [`apply_event`] (`(value, open)` state shape, same as
//! `tabs` / `text_input` / `switch`). Reach for `dropdown_menu` when
//! items perform side-effects instead of selecting a value.
//!
//! # Shape
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Picker {
//!     color: String,
//!     color_open: bool,
//! }
//!
//! impl App for Picker {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         let trigger = select_trigger("color", &self.color);
//!         let main = column([row([text("Color"), trigger])]);
//!
//!         let mut layers: Vec<El> = vec![main];
//!         if self.color_open {
//!             layers.push(select_menu("color", [
//!                 ("red", "Red"),
//!                 ("blue", "Blue"),
//!                 ("green", "Green"),
//!             ]));
//!         }
//!         stack(layers)
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         if event.is_click_or_activate("color") {
//!             self.color_open = !self.color_open;
//!         } else if event.is_click_or_activate("color:dismiss") {
//!             self.color_open = false;
//!         } else if let Some(value) = event.route().and_then(|r| r.strip_prefix("color:option:")) {
//!             self.color = value.to_string();
//!             self.color_open = false;
//!         }
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}` — `Click` on the trigger; the app toggles its open flag.
//! - `{key}:dismiss` — `Click` outside the menu (the popover scrim);
//!   the app clears its open flag.
//! - `{key}:option:{value}` — `Click` on an option; the app sets the
//!   selected value and clears its open flag.
//!
//! Apps that share one open slot across several selects can match the
//! `:option:` and `:dismiss` suffixes back to the active select's key.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom` for
//! the inspector tag, `.focusable()` + `.paint_overflow()` for the
//! focus ring, `.key()` for hit-test routing, and the existing
//! [`crate::widgets::popover`] composition for the dropdown body. An
//! app crate can write an equivalent select against the same public
//! API. See `widget_kit.md`.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::popover::{
    Anchor, MenuDensity, apply_menu_density, menu_item, popover, popover_panel,
};
use crate::{icon, text};

/// What a routed [`UiEvent`] means for a controlled select keyed `key`.
///
/// Returned by [`classify_event`]; [`apply_event`] is the convenience
/// wrapper that folds the action straight into `(value, open)` state.
///
/// The action variants cover the three routed keys [`select_trigger`]
/// + [`select_menu`] emit:
///
/// - `{key}` — toggle (trigger click / activate).
/// - `{key}:dismiss` — dismiss (scrim click).
/// - `{key}:option:{value}` — pick an option; the carried `String` is
///   the same `{value}` token passed to [`select_option_key`]. Apps
///   move it into their value type (identity for `String`, `s.parse()`
///   for numbers, a lookup for enums, …).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SelectAction {
    /// The trigger was clicked or activated. Toggle the open flag.
    Toggle,
    /// The dismiss scrim was clicked. Close the menu.
    Dismiss,
    /// An option was picked. The string is the raw value token from
    /// the option key.
    Pick(String),
}

/// Classify a routed [`UiEvent`] against a controlled select keyed
/// `key`. Returns `None` for events that aren't for this select.
///
/// Only `Click` / `Activate` event kinds qualify — pointer-move,
/// hover, and other non-activating events return `None` even when
/// they target a select sub-key. That means an app can call
/// [`classify_event`] unconditionally inside its event handler
/// without filtering on `event.kind` first.
pub fn classify_event(event: &UiEvent, key: &str) -> Option<SelectAction> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    if routed == key {
        return Some(SelectAction::Toggle);
    }
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    if rest == "dismiss" {
        return Some(SelectAction::Dismiss);
    }
    if let Some(value) = rest.strip_prefix("option:") {
        return Some(SelectAction::Pick(value.to_string()));
    }
    None
}

/// Fold a routed [`UiEvent`] into `(value, open)` state for a
/// controlled select keyed `key`. Returns `true` if the event was a
/// select event for this `key` (so the caller can short-circuit
/// further dispatch), `false` otherwise.
///
/// `parse` converts the raw option-value token back to the app's
/// value type, taking ownership of the picked `String`. Returning
/// `None` ignores the option pick silently (useful when the option
/// list and the value type can drift — e.g. a stale event arriving
/// after the underlying data changed).
///
/// For a `String` value field, pass `Some` directly — the picked
/// string moves straight into the destination. For typed values use
/// `s.parse().ok()` or a lookup closure.
///
/// ```ignore
/// use damascene_core::prelude::*;
///
/// // App owns (value, open) per select.
/// struct Picker { color: String, color_open: bool }
///
/// impl App for Picker {
///     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
///         widgets::select::apply_event(
///             &mut self.color,
///             &mut self.color_open,
///             &event,
///             "color",
///             Some,
///         );
///     }
///     // ...
/// }
/// ```
pub fn apply_event<V>(
    value: &mut V,
    open: &mut bool,
    event: &UiEvent,
    key: &str,
    parse: impl FnOnce(String) -> Option<V>,
) -> bool {
    let Some(action) = classify_event(event, key) else {
        return false;
    };
    match action {
        SelectAction::Toggle => *open = !*open,
        SelectAction::Dismiss => *open = false,
        SelectAction::Pick(s) => {
            if let Some(v) = parse(s) {
                *value = v;
                *open = false;
            }
        }
    }
    true
}

/// Format the routed key emitted when an option is clicked. Apps that
/// match against the `:option:` suffix can use this helper to produce
/// the same string the widget produces, but the convention is also
/// stable enough to format inline.
pub fn select_option_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:option:{value}")
}

/// The trigger surface for a `select`. Visually a button-shaped row
/// of `[ current_label ▼ ]` keyed by `key`. Click emits `Click` on
/// `key`; the app toggles its open flag in `on_event`.
///
/// Default height is [`tokens::CONTROL_HEIGHT`] — use that constant
/// when sizing a parent row that has to fit the trigger.
///
/// The trigger is also the anchor key for [`select_menu`] — keep them
/// identical so the menu drops below the trigger.
#[track_caller]
pub fn select_trigger(key: impl Into<String>, current_label: impl Into<String>) -> El {
    let label = text(current_label)
        .label()
        .ellipsis()
        .width(Size::Fill(1.0));
    let chevron = icon("chevron-down")
        .icon_size(tokens::ICON_SM)
        .text_color(tokens::MUTED_FOREGROUND);
    El::new(Kind::Custom("select_trigger"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Input)
        .surface_role(SurfaceRole::Input)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .key(key)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_2)
        .align(Align::Center)
        .child(label)
        .child(chevron)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .text_color(tokens::FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Fill(1.0))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
}

/// The dropdown popover for a `select`. Render this only while the
/// menu is open; place it at the root of the El tree (e.g. inside a
/// `stack`) so it paints over content and intercepts clicks above
/// siblings.
///
/// `options` is an iterable of `(value, label)` pairs. Each becomes a
/// [`menu_item`] keyed `{key}:option:{value}`. The dismiss scrim
/// emits `{key}:dismiss` (per the popover convention) on click
/// outside.
///
/// The menu anchors below the trigger keyed `key`; if that placement
/// would clip the viewport bottom the popover flips above
/// automatically (see [`crate::anchor_rect`]).
#[track_caller]
pub fn select_menu<I, V, L>(key: impl Into<String>, options: I) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    select_menu_with_density(key, options, MenuDensity::Compact).at_loc(Location::caller())
}

/// Density-aware variant of [`select_menu`].
///
/// Use [`MenuDensity::from_event`] with the event that opened the
/// trigger when a touch-originated select should use larger option
/// rows.
#[track_caller]
pub fn select_menu_with_density<I, V, L>(
    key: impl Into<String>,
    options: I,
    density: MenuDensity,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    // Capture once so the user's call site flows through to each
    // `menu_item`. `#[track_caller]` doesn't propagate through
    // `.map(...)` closures, so the items would otherwise record the
    // closure's source — see `tabs_list` for the same pattern and
    // motivation.
    let caller = Location::caller();
    let key = key.into();
    let items: Vec<El> = options
        .into_iter()
        .map(|(value, label)| {
            menu_item(label)
                .at_loc(caller)
                .key(select_option_key(&key, &value))
        })
        .map(|item| apply_menu_density(item, density))
        .collect();
    popover(key.clone(), Anchor::below_key(key), popover_panel(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_trigger_keys_root_and_carries_chevron() {
        let t = select_trigger("color", "Red");
        assert_eq!(t.key.as_deref(), Some("color"));
        // Trigger is a row of [label, chevron]. The chevron is the
        // last child and carries the chevron-down icon name so visual
        // affordance is unambiguous.
        let chevron = t.children.last().expect("trigger has chevron child");
        assert_eq!(
            chevron.icon,
            Some(crate::IconSource::Builtin(IconName::ChevronDown))
        );
        // Trigger opts into focus + ring overhead so keyboard users
        // can tab through selects like any other interactive surface.
        assert!(t.focusable, "select_trigger must be focusable");
    }

    #[test]
    fn select_menu_routes_dismiss_and_option_keys() {
        let menu = select_menu("color", [("red", "Red"), ("blue", "Blue")]);
        // Dismiss scrim follows the popover convention: `{key}:dismiss`.
        let scrim = &menu.children[0];
        assert_eq!(scrim.kind, Kind::Scrim);
        assert_eq!(scrim.key.as_deref(), Some("color:dismiss"));
        // Layer wraps the panel; panel children are the menu_items
        // keyed `{key}:option:{value}`.
        let layer = &menu.children[1];
        let panel = &layer.children[0];
        assert_eq!(panel.children.len(), 2);
        assert_eq!(panel.children[0].key.as_deref(), Some("color:option:red"));
        assert_eq!(panel.children[1].key.as_deref(), Some("color:option:blue"));
    }

    #[test]
    fn select_menu_with_touch_density_expands_options() {
        let menu = select_menu_with_density(
            "color",
            [("red", "Red"), ("blue", "Blue")],
            MenuDensity::Touch,
        );
        let panel = &menu.children[1].children[0];

        assert_eq!(
            panel.children[0].height,
            Size::Fixed(crate::widgets::popover::TOUCH_MENU_ITEM_HEIGHT)
        );
        assert_eq!(
            panel.children[1].height,
            Size::Fixed(crate::widgets::popover::TOUCH_MENU_ITEM_HEIGHT)
        );
    }

    #[test]
    fn select_option_key_matches_widget_format() {
        // Apps decoding routed events should use the same helper to
        // avoid format drift.
        assert_eq!(select_option_key("color", &"red"), "color:option:red");
        assert_eq!(
            select_option_key("profile:7", &42u32),
            "profile:7:option:42"
        );
    }

    fn click_event(key: &str) -> UiEvent {
        UiEvent {
            path: None,
            kind: UiEventKind::Click,
            key: Some(key.to_string()),
            target: None,
            pointer: None,
            key_press: None,
            text: None,
            selection: None,
            modifiers: Default::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
        }
    }

    #[test]
    fn classify_event_routes_trigger_dismiss_and_option() {
        // The same three keys `parse_profile_event` used to decode in
        // the volume app. classify_event collapses that boilerplate.
        assert_eq!(
            classify_event(&click_event("color"), "color"),
            Some(SelectAction::Toggle),
        );
        assert_eq!(
            classify_event(&click_event("color:dismiss"), "color"),
            Some(SelectAction::Dismiss),
        );
        assert_eq!(
            classify_event(&click_event("color:option:red"), "color"),
            Some(SelectAction::Pick("red".to_string())),
        );

        // Compound keys (the volume app uses `profile:{card_id}` as the
        // select key) work the same way — the helper compares against
        // the full select key, not just a prefix.
        assert_eq!(
            classify_event(&click_event("profile:7"), "profile:7"),
            Some(SelectAction::Toggle),
        );
        assert_eq!(
            classify_event(&click_event("profile:7:dismiss"), "profile:7"),
            Some(SelectAction::Dismiss),
        );
        assert_eq!(
            classify_event(&click_event("profile:7:option:42"), "profile:7"),
            Some(SelectAction::Pick("42".to_string())),
        );

        // Non-matching keys fall through.
        assert_eq!(classify_event(&click_event("mute:7"), "profile:7"), None);
        // Even when a key shares a prefix with the select key, the
        // separator-after-prefix check rejects events that aren't this
        // select's own children.
        assert_eq!(
            classify_event(&click_event("profile:7-other"), "profile:7"),
            None,
        );
        // Malformed option suffix isn't a Pick.
        assert_eq!(
            classify_event(&click_event("profile:7:option"), "profile:7"),
            None,
        );
    }

    #[test]
    fn classify_event_ignores_non_activating_kinds() {
        // Pointer-down / drag / hotkey events that target the same key
        // shouldn't toggle the menu — only Click and Activate qualify.
        let mut ev = click_event("color");
        ev.kind = UiEventKind::PointerDown;
        assert_eq!(classify_event(&ev, "color"), None);
        ev.kind = UiEventKind::Drag;
        assert_eq!(classify_event(&ev, "color"), None);
        ev.kind = UiEventKind::Activate;
        assert_eq!(
            classify_event(&ev, "color"),
            Some(SelectAction::Toggle),
            "keyboard activation should toggle like a click",
        );
    }

    #[test]
    fn apply_event_folds_actions_into_value_and_open() {
        let mut value = String::from("red");
        let mut open = false;

        // Trigger click flips open.
        assert!(apply_event(
            &mut value,
            &mut open,
            &click_event("color"),
            "color",
            Some,
        ));
        assert!(open);
        assert_eq!(value, "red");

        // Pick replaces value and closes the menu.
        assert!(apply_event(
            &mut value,
            &mut open,
            &click_event("color:option:blue"),
            "color",
            Some,
        ));
        assert_eq!(value, "blue");
        assert!(!open);

        // Reopen, then dismiss.
        apply_event(&mut value, &mut open, &click_event("color"), "color", Some);
        assert!(open);
        assert!(apply_event(
            &mut value,
            &mut open,
            &click_event("color:dismiss"),
            "color",
            Some,
        ));
        assert!(!open);
        assert_eq!(value, "blue", "dismiss must not alter the value");

        // Non-select event returns false; state unchanged.
        let mut value = String::from("v");
        let mut open = true;
        assert!(!apply_event(
            &mut value,
            &mut open,
            &click_event("unrelated"),
            "color",
            Some,
        ));
        assert_eq!((value.as_str(), open), ("v", true));
    }

    #[test]
    fn apply_event_silently_ignores_unparseable_picks() {
        // The volume app uses u32 profile indices; a stale option key
        // that doesn't parse should leave state untouched rather than
        // panic.
        let mut value: u32 = 3;
        let mut open = true;
        assert!(apply_event(
            &mut value,
            &mut open,
            &click_event("profile:7:option:not-a-number"),
            "profile:7",
            |s| s.parse::<u32>().ok(),
        ));
        assert_eq!(value, 3, "value preserved when parse returns None");
        assert!(open, "open preserved when parse returns None");
    }

    #[test]
    fn select_menu_anchors_below_trigger_key() {
        // End-to-end layout regression: the menu must look up the
        // trigger's rect via `rect_of_key(key)`, so when the trigger
        // is laid out at (x, y, w, h), the panel lands directly below.
        use crate::layout::layout;
        use crate::state::UiState;
        use crate::tree::stack;
        let trigger = select_trigger("sel", "A");
        let menu = select_menu("sel", [("a", "A"), ("b", "B")]);
        let mut tree = stack([trigger, menu]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        // Trigger laid out by stack at parent origin, height 36.
        let trig_rect = state.rect_of_key("sel").expect("trigger key resolves");
        // The popover panel sits below the trigger with the standard
        // anchor gap. It's the popover layer's first child.
        let layer = &tree.children[1].children[1];
        let panel = &layer.children[0];
        let panel_rect = state.rect(&panel.computed_id);
        assert!(
            panel_rect.y >= trig_rect.bottom(),
            "panel should sit below trigger; trig.bottom={}, panel.y={}",
            trig_rect.bottom(),
            panel_rect.y,
        );
    }
}
