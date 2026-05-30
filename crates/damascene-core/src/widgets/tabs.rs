//! Tabs — a segmented row of triggers that drives a tab-panel
//! selection. Mirrors the shadcn / Radix Tabs primitive (which itself
//! reflects the WAI-ARIA `role="tablist"` / `role="tab"` pattern), so
//! LLM authors trained on web UI find the same shape here.
//!
//! The app owns the active tab value as a field of its choice
//! (`String`, an enum implementing [`std::fmt::Display`], …); the widget is a
//! pure visual + identity carrier — exactly the same controlled
//! pattern used by [`crate::widgets::select`] and
//! [`crate::widgets::slider`].
//!
//! # Shape
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Settings { tab: String }
//!
//! impl App for Settings {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             tabs_list("settings", &self.tab, [
//!                 ("account", "Account"),
//!                 ("appearance", "Appearance"),
//!                 ("advanced", "Advanced"),
//!             ]),
//!             match self.tab.as_str() {
//!                 "account" => account_panel(),
//!                 "appearance" => appearance_panel(),
//!                 "advanced" => advanced_panel(),
//!                 _ => spacer(),
//!             },
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent) {
//!         tabs::apply_event(&mut self.tab, &event, "settings", |s| {
//!             Some(s.to_string())
//!         });
//!     }
//! }
//! ```
//!
//! There is intentionally no `tab_panel` / `tabs_content` wrapper:
//! Rust's `match` is more idiomatic than a sibling element that hides
//! itself when not active, and shadcn's `<TabsContent>` adds no visual
//! beyond a plain block.
//!
//! # Routed keys
//!
//! - `{key}:tab:{value}` — `Click` on a trigger; the app sets the
//!   active tab. Use [`tab_option_key`] to format and parse.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom` for
//! the inspector tag, `.focusable()` + `.paint_overflow()` for the
//! focus ring, the existing `.current()` / `.ghost()` style modifiers
//! for active-vs-inactive treatment. An app crate can fork this file
//! and produce an equivalent widget against the same public API. See
//! `widget_kit.md`.

use std::panic::Location;

use crate::anim::Timing;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// What a routed [`UiEvent`] means for a controlled tabs row keyed
/// `key`.
///
/// Returned by [`classify_event`]; [`apply_event`] is the convenience
/// wrapper that folds the action straight into the app's value field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TabsAction<'a> {
    /// A trigger was clicked or activated. The string is the raw value
    /// token from the trigger's option key — apps convert it back to
    /// their value type (`&str` → `String`, `s.parse::<u32>()`, an
    /// enum lookup, …).
    Select(&'a str),
}

/// Classify a routed [`UiEvent`] against a controlled tabs row keyed
/// `key`. Returns `None` for events that aren't for this row.
///
/// Only `Click` / `Activate` event kinds qualify — pointer-move,
/// hover, and other non-activating events return `None` even when
/// they target a trigger. That means an app can call
/// [`classify_event`] unconditionally inside its event handler
/// without filtering on `event.kind` first.
///
/// The returned [`TabsAction::Select`] borrows from the event's routed
/// key, so apps that want to keep the value beyond the match arm
/// should `.to_string()` or `.parse()` it inline.
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<TabsAction<'a>> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    let value = rest.strip_prefix("tab:")?;
    Some(TabsAction::Select(value))
}

/// Fold a routed [`UiEvent`] into the app's tab-value field for a
/// controlled tabs row keyed `key`. Returns `true` if the event was a
/// tabs event for this `key` (so the caller can short-circuit further
/// dispatch), `false` otherwise.
///
/// `parse` converts the raw value token back to the app's value type.
/// Returning `None` from `parse` ignores the click silently (useful
/// when the tab list and the value type can drift — e.g. a stale event
/// arriving after the underlying data changed).
///
/// ```ignore
/// use damascene_core::prelude::*;
///
/// struct Settings { tab: String }
///
/// impl App for Settings {
///     fn on_event(&mut self, event: UiEvent) {
///         tabs::apply_event(&mut self.tab, &event, "settings", |s| {
///             Some(s.to_string())
///         });
///     }
///     // ...
/// }
/// ```
pub fn apply_event<V>(
    value: &mut V,
    event: &UiEvent,
    key: &str,
    parse: impl FnOnce(&str) -> Option<V>,
) -> bool {
    let Some(TabsAction::Select(raw)) = classify_event(event, key) else {
        return false;
    };
    if let Some(v) = parse(raw) {
        *value = v;
    }
    true
}

/// Format the routed key emitted when a trigger is clicked. Apps that
/// match against the `:tab:` suffix can use this helper to produce the
/// same string the widget produces, but the convention is also stable
/// enough to format inline.
pub fn tab_option_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:tab:{value}")
}

/// The trigger surface for a single tab inside a [`tabs_list`]. Apps
/// usually let `tabs_list` build these from `(value, label)` pairs;
/// reach for `tab_trigger` directly when composing the row by hand
/// (e.g. mixing in icons, dividers, or different per-tab widths).
///
/// `list_key` is the parent tabs row's key — the routed key on the
/// trigger is `{list_key}:tab:{value}` (see [`tab_option_key`]).
/// `selected` styles the trigger as the active tab (raised surface
/// and semibold text, via the stock `.current()` modifier); inactive
/// triggers render with the `.ghost()` treatment.
#[track_caller]
pub fn tab_trigger(
    list_key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    selected: bool,
) -> El {
    let routed_key = tab_option_key(list_key, &value);
    let base = El::new(Kind::Custom("tab_trigger"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::TabTrigger)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .key(routed_key)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .default_radius(tokens::RADIUS_SM)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0));
    // `.current()` / `.ghost()` set fill, stroke, and text_color —
    // adding `.animate(SPRING_QUICK)` after them eases all three
    // between rebuilds, so switching tabs cross-fades the active
    // surface instead of snapping.
    let styled = if selected {
        base.current()
    } else {
        base.ghost()
    };
    styled.animate(Timing::SPRING_QUICK)
}

/// A tab trigger with app-supplied children instead of a plain string
/// label. Use this for segmented controls whose options need icons,
/// badges, dirty markers, or other compact metadata while keeping the
/// same routed-key and active/inactive treatment as [`tab_trigger`].
///
/// Pair these triggers with [`tabs_list_from_triggers`] so the row keeps
/// the stock muted pill container:
///
/// ```ignore
/// tabs_list_from_triggers([
///     tab_trigger_content("worktree", "main", [text("main").label(), badge("3")], active),
/// ])
/// ```
#[track_caller]
pub fn tab_trigger_content<I, E>(
    list_key: &str,
    value: impl std::fmt::Display,
    children: I,
    selected: bool,
) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let routed_key = tab_option_key(list_key, &value);
    let base = El::new(Kind::Custom("tab_trigger"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::TabTrigger)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .key(routed_key)
        .axis(Axis::Row)
        .children(children)
        .default_gap(tokens::SPACE_1)
        .align(Align::Center)
        .justify(Justify::Center)
        .default_radius(tokens::RADIUS_SM)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0));
    let styled = if selected {
        base.current()
    } else {
        base.ghost()
    };
    styled.animate(Timing::SPRING_QUICK)
}

/// A segmented-control row of tab triggers. Visually a muted pill
/// containing one [`tab_trigger`] per option; the active trigger
/// surfaces above the muted base.
///
/// `current` is the currently-selected value — formatted via
/// [`std::fmt::Display`] and compared (also as a `Display` string)
/// against each option's `value` to pick the active trigger.
/// `options` is an iterable of `(value, label)` pairs.
///
/// `key` is the routing namespace; per-trigger routed keys are
/// `{key}:tab:{value}`. The row itself is deliberately not keyed:
/// the space between triggers is visual chrome, not an interactive
/// target, so mousing through the gaps does not hover the whole pill.
/// Apps fold trigger events back into their value field with
/// [`apply_event`].
#[track_caller]
pub fn tabs_list<I, V, L>(
    key: impl Into<String>,
    current: &impl std::fmt::Display,
    options: I,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    // Capture once so the location applies to children too.
    // `#[track_caller]` doesn't propagate through closures, so naive
    // `tab_trigger(...)` calls inside `.map(...)` would record the
    // closure's source instead of the user's `tabs_list(...)` call —
    // making lint findings on the triggers (e.g. text overflow from
    // labels that don't fit) point inside damascene-core. Forwarding via
    // `.at_loc(caller)` keeps the user's call site as the blame
    // target for both lint findings and tree dumps.
    let caller = Location::caller();
    let key = key.into();
    let current_str = current.to_string();
    let triggers: Vec<El> = options
        .into_iter()
        .map(|(value, label)| {
            let selected = value.to_string() == current_str;
            tab_trigger(&key, value, label, selected).at_loc(caller)
        })
        .collect();
    tabs_list_container(caller, triggers)
}

/// A segmented-control row for triggers that have already been built
/// with [`tab_trigger`] or [`tab_trigger_content`]. This is the
/// child-rich variant of [`tabs_list`]: it keeps the stock tab-list
/// container while letting each option carry icons, badges, or other
/// compact metadata.
#[track_caller]
pub fn tabs_list_from_triggers<I, E>(triggers: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    tabs_list_container(Location::caller(), triggers)
}

fn tabs_list_container<I, E>(caller: &'static Location<'static>, triggers: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut triggers: Vec<El> = triggers.into_iter().map(Into::into).collect();
    apply_edge_radii(&mut triggers);

    El::new(Kind::Custom("tabs_list"))
        .at_loc(caller)
        .metrics_role(MetricsRole::TabList)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_1)
        .align(Align::Stretch)
        .children(triggers)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .default_padding(Sides::all(tokens::SPACE_1))
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn apply_edge_radii(triggers: &mut [El]) {
    match triggers {
        [] => {}
        [only] => {
            set_trigger_radius(only, Corners::all(tokens::RADIUS_MD));
        }
        [first, middle @ .., last] => {
            set_trigger_radius(
                first,
                Corners {
                    tl: tokens::RADIUS_MD,
                    tr: tokens::RADIUS_SM,
                    br: tokens::RADIUS_SM,
                    bl: tokens::RADIUS_MD,
                },
            );
            for trigger in middle {
                set_trigger_radius(trigger, Corners::all(tokens::RADIUS_SM));
            }
            set_trigger_radius(
                last,
                Corners {
                    tl: tokens::RADIUS_SM,
                    tr: tokens::RADIUS_MD,
                    br: tokens::RADIUS_MD,
                    bl: tokens::RADIUS_SM,
                },
            );
        }
    }
}

fn set_trigger_radius(trigger: &mut El, radius: Corners) {
    trigger.radius = radius;
    trigger.explicit_radius = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, UiTarget};
    use crate::hit_test::hit_test_target;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::widgets::text::text;

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
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
        }
    }

    #[test]
    fn tab_option_key_matches_widget_format() {
        // Apps decoding routed events should use the same helper to
        // avoid format drift.
        assert_eq!(
            tab_option_key("settings", &"account"),
            "settings:tab:account"
        );
        assert_eq!(tab_option_key("dashboard:7", &42u32), "dashboard:7:tab:42");
    }

    #[test]
    fn tab_trigger_routes_via_tab_option_key() {
        let inactive = tab_trigger("settings", "account", "Account", false);
        assert_eq!(inactive.key.as_deref(), Some("settings:tab:account"));
        // Trigger opts into focus + ring overhead so keyboard users
        // can tab through the row like any other interactive surface.
        assert!(inactive.focusable);
        // Inactive triggers carry the ghost treatment: no fill, no
        // stroke. The .ghost() modifier wipes both.
        assert!(inactive.fill.is_none());
        assert!(inactive.stroke.is_none());

        let active = tab_trigger("settings", "account", "Account", true);
        // Active triggers carry the .current() treatment: ACCENT
        // fill, BORDER stroke, and Selected/Current surface role for
        // theme dispatch.
        assert_eq!(active.fill, Some(tokens::ACCENT));
        assert_eq!(active.surface_role, SurfaceRole::Current);
    }

    #[test]
    fn tab_trigger_content_keeps_tab_treatment_for_rich_children() {
        let active = tab_trigger_content(
            "worktree",
            "main",
            [text("main").label(), text("3").caption().muted()],
            true,
        );

        assert_eq!(active.key.as_deref(), Some("worktree:tab:main"));
        assert_eq!(active.metrics_role, Some(MetricsRole::TabTrigger));
        assert_eq!(active.axis, Axis::Row);
        assert_eq!(active.align, Align::Center);
        assert_eq!(active.justify, Justify::Center);
        assert_eq!(active.gap, tokens::SPACE_1);
        assert_eq!(active.children.len(), 2);
        assert_eq!(active.fill, Some(tokens::ACCENT));
        assert_eq!(active.surface_role, SurfaceRole::Current);
    }

    #[test]
    fn tabs_list_marks_only_the_current_value_active() {
        let list = tabs_list(
            "settings",
            &"appearance",
            [
                ("account", "Account"),
                ("appearance", "Appearance"),
                ("advanced", "Advanced"),
            ],
        );
        assert_eq!(
            list.key, None,
            "the visual pill is not an interactive target; triggers carry the routed keys"
        );
        assert_eq!(list.children.len(), 3);

        let [account, appearance, advanced] =
            [&list.children[0], &list.children[1], &list.children[2]];
        // Per-trigger routed keys.
        assert_eq!(account.key.as_deref(), Some("settings:tab:account"));
        assert_eq!(appearance.key.as_deref(), Some("settings:tab:appearance"));
        assert_eq!(advanced.key.as_deref(), Some("settings:tab:advanced"));

        // Only the trigger whose value matches `current` carries the
        // active-tab surface role.
        assert_ne!(account.surface_role, SurfaceRole::Current);
        assert_eq!(appearance.surface_role, SurfaceRole::Current);
        assert_ne!(advanced.surface_role, SurfaceRole::Current);
    }

    #[test]
    fn tabs_list_compares_via_display_so_typed_values_work() {
        // Mirrors the select_option_key Display contract: an `enum` or
        // `u32` value can drive the comparison as long as it formats
        // the same way as the option-list values.
        let list = tabs_list(
            "page",
            &7u32,
            [(0u32, "Zero"), (7u32, "Seven"), (42u32, "Forty-two")],
        );
        let [zero, seven, fortytwo] = [&list.children[0], &list.children[1], &list.children[2]];
        assert_ne!(zero.surface_role, SurfaceRole::Current);
        assert_eq!(seven.surface_role, SurfaceRole::Current);
        assert_ne!(fortytwo.surface_role, SurfaceRole::Current);
    }

    #[test]
    fn tabs_list_from_triggers_keeps_stock_pill_container() {
        let list = tabs_list_from_triggers([
            tab_trigger_content("worktree", "main", [text("main").label()], true),
            tab_trigger("worktree", "feature", "feature", false),
        ]);

        assert_eq!(list.key, None);
        assert_eq!(list.metrics_role, Some(MetricsRole::TabList));
        assert_eq!(list.axis, Axis::Row);
        assert_eq!(list.children.len(), 2);
        assert_eq!(list.fill, Some(tokens::MUTED));
        assert_eq!(list.stroke, Some(tokens::BORDER));
    }

    #[test]
    fn classify_event_selects_only_on_matching_route() {
        assert_eq!(
            classify_event(&click_event("settings:tab:account"), "settings"),
            Some(TabsAction::Select("account")),
        );
        // Compound keys (e.g. a per-card tabs row) work the same way
        // — the helper compares against the full row key.
        assert_eq!(
            classify_event(&click_event("dashboard:7:tab:42"), "dashboard:7"),
            Some(TabsAction::Select("42")),
        );

        // Non-matching keys fall through.
        assert_eq!(
            classify_event(&click_event("settings"), "settings"),
            None,
            "the row's own key isn't itself a tab target",
        );
        assert_eq!(
            classify_event(&click_event("other:tab:account"), "settings"),
            None,
        );
        // Even when a key shares a prefix with the tabs key, the
        // separator-after-prefix check rejects events that aren't this
        // row's own children.
        assert_eq!(
            classify_event(&click_event("settings-other:tab:x"), "settings"),
            None,
        );
        // Malformed suffix isn't a Select.
        assert_eq!(
            classify_event(&click_event("settings:option:x"), "settings"),
            None,
        );
    }

    #[test]
    fn classify_event_ignores_non_activating_kinds() {
        // Pointer-down / drag / hotkey events that target the same
        // trigger key shouldn't switch tabs — only Click and Activate
        // qualify.
        let mut ev = click_event("settings:tab:account");
        ev.kind = UiEventKind::PointerDown;
        assert_eq!(classify_event(&ev, "settings"), None);
        ev.kind = UiEventKind::Drag;
        assert_eq!(classify_event(&ev, "settings"), None);
        ev.kind = UiEventKind::Activate;
        assert_eq!(
            classify_event(&ev, "settings"),
            Some(TabsAction::Select("account")),
            "keyboard activation should select like a click",
        );
    }

    #[test]
    fn apply_event_folds_actions_into_value() {
        let mut tab = String::from("account");
        // Click on a trigger replaces the value.
        assert!(apply_event(
            &mut tab,
            &click_event("settings:tab:advanced"),
            "settings",
            |s| Some(s.to_string()),
        ));
        assert_eq!(tab, "advanced");

        // Non-tabs event returns false; state unchanged.
        assert!(!apply_event(
            &mut tab,
            &click_event("save"),
            "settings",
            |s| Some(s.to_string()),
        ));
        assert_eq!(tab, "advanced");
    }

    #[test]
    fn apply_event_silently_ignores_unparseable_values() {
        // A typed-value tabs row (e.g. u32 tab indices) should leave
        // state alone when a stale string can't be parsed back, rather
        // than panic.
        let mut tab: u32 = 1;
        assert!(apply_event(
            &mut tab,
            &click_event("page:tab:not-a-number"),
            "page",
            |s| s.parse::<u32>().ok(),
        ));
        assert_eq!(tab, 1, "value preserved when parse returns None");
    }

    #[test]
    fn tab_trigger_animates_so_selection_changes_ease() {
        // Without `.animate()` on the trigger, `.current()` →
        // `.ghost()` (and back) snaps fill/text_color on every
        // rebuild, which reads as a hard cut between tabs.
        assert!(
            tab_trigger("settings", "account", "Account", true)
                .animate
                .is_some()
        );
        assert!(
            tab_trigger("settings", "account", "Account", false)
                .animate
                .is_some()
        );
    }

    #[test]
    fn tabs_list_paints_a_segmented_pill_around_the_triggers() {
        // The row carries the muted pill background, so the active
        // trigger's raised surface visually nests inside it.
        let list = tabs_list(
            "settings",
            &"account",
            [("account", "Account"), ("settings", "Settings")],
        );
        assert_eq!(list.fill, Some(tokens::MUTED));
        assert_eq!(list.radius, crate::tree::Corners::all(tokens::RADIUS_MD));
        assert_eq!(
            list.children[0].radius,
            crate::tree::Corners {
                tl: tokens::RADIUS_MD,
                tr: tokens::RADIUS_SM,
                br: tokens::RADIUS_SM,
                bl: tokens::RADIUS_MD,
            }
        );
        assert!(
            list.children[0].explicit_radius,
            "edge trigger radius must survive the metrics pass"
        );
        assert_eq!(
            list.children[1].radius,
            crate::tree::Corners {
                tl: tokens::RADIUS_SM,
                tr: tokens::RADIUS_MD,
                br: tokens::RADIUS_MD,
                bl: tokens::RADIUS_SM,
            }
        );
        // Row axis with a small gap so triggers are visually distinct.
        assert_eq!(list.axis, Axis::Row);
        // The list itself is not focusable or keyed — only its
        // triggers are. Otherwise Tab would land on the row before
        // the first tab, and pointer hover in the gaps would brighten
        // the whole pill.
        assert!(!list.focusable);
        assert!(list.key.is_none());
    }

    #[test]
    fn tabs_list_gap_is_not_a_hover_target() {
        let mut list = tabs_list(
            "settings",
            &"account",
            [("account", "Account"), ("advanced", "Advanced")],
        );
        let mut state = UiState::new();
        layout(&mut list, &mut state, Rect::new(0.0, 0.0, 240.0, 60.0));

        let first = state.rect(&list.children[0].computed_id);
        let second = state.rect(&list.children[1].computed_id);
        assert!(
            second.x > first.x + first.w,
            "test requires the tab list's configured gap to be present"
        );

        let trigger_target = hit_test_target(
            &list,
            &state,
            (first.x + first.w / 2.0, first.y + first.h / 2.0),
        )
        .expect("tab trigger should still be interactive");
        assert_eq!(trigger_target.key, "settings:tab:account");

        let gap_x = (first.x + first.w + second.x) / 2.0;
        let gap_y = first.y + first.h / 2.0;
        assert_eq!(
            hit_test_target(&list, &state, (gap_x, gap_y)),
            None,
            "the gap between triggers should not hover the tab-list shell"
        );
    }

    #[test]
    fn target_for_event_can_be_routed_through_apply_event() {
        // Smoke test that a click event whose route comes from a real
        // UiTarget (mirroring what runtime delivers) is matched.
        let ev = UiEvent {
            path: None,
            kind: UiEventKind::Click,
            key: Some("settings:tab:advanced".into()),
            target: Some(UiTarget {
                key: "settings:tab:advanced".into(),
                node_id: "/settings/2".into(),
                rect: Rect::new(0.0, 0.0, 60.0, 28.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            }),
            pointer: None,
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
        };
        let mut tab = String::from("account");
        assert!(apply_event(&mut tab, &ev, "settings", |s| Some(
            s.to_string()
        )));
        assert_eq!(tab, "advanced");
    }
}
