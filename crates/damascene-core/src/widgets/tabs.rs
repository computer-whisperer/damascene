//! Tabs — an underlined row of hugging triggers that drives a
//! tab-panel selection. Mirrors the shadcn / Radix Tabs primitive's
//! names and events (which themselves reflect the WAI-ARIA
//! `role="tablist"` / `role="tab"` pattern), so LLM authors trained on
//! web UI find the same shape here. Per that pattern the list is
//! arrow-navigable: Left/Right move focus among the triggers
//! ([`crate::tree::ArrowNav::Horizontal`]).
//!
//! # Look
//!
//! Tabs are **navigation**, and draw as every desktop tool draws
//! navigation: triggers hug their labels, sit left-aligned on one
//! hairline, and the active one carries a 2 px `PRIMARY` underline
//! with the plain foreground label; inactive labels are muted. That
//! is Radix Themes' `Tabs`, VS Code's panel tabs and Linear's — and
//! it was ruled the default on 2026-10-14
//! (`docs/POLISH_REVIEW_2026_10.md`). The previous default was
//! shadcn's muted pill with equal-width `Fill` cells, which at dense
//! sizes was indistinguishable from a value toggle: every app built
//! on it stretched five equal cells across the window and used the
//! same drawing for "which page" and "which option" (Opus review
//! §6.5). The segmented look lives on where it belongs, in
//! [`crate::widgets::toggle::toggle_group`], which is a *value* control.
//!
//! An overfull row — more tabs than the width can hold — overflows
//! rather than squeezing every label to a stub; the bundle lint
//! reports it, and a shell that expects that many tabs should wrap
//! the row in a horizontal `scroll` or move the tail into a menu.
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
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
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

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
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
///     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
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

/// Width of the active tab's underline, in logical px — Radix Themes'
/// and Material's 2 px indicator. Every trigger reserves it (inactive
/// ones paint it transparent) so the row does not shift when the
/// selection moves.
pub const TAB_INDICATOR_WIDTH: f32 = 2.0;

/// The trigger surface for a single tab inside a [`tabs_list`]. Apps
/// usually let `tabs_list` build these from `(value, label)` pairs;
/// reach for `tab_trigger` directly when composing the row by hand
/// (e.g. mixing in icons, dividers, or different per-tab widths).
///
/// `list_key` is the parent tabs row's key — the routed key on the
/// trigger is `{list_key}:tab:{value}` (see [`tab_option_key`]).
/// `selected` styles the trigger as the active tab: foreground label
/// over a [`TAB_INDICATOR_WIDTH`] `PRIMARY` underline. Inactive
/// triggers carry a muted label and a transparent underline of the
/// same width. Neither has a fill or a stroke; hover is the stock
/// ghost wash.
///
/// # Anatomy
///
/// ```text
/// tab_trigger  hug  h=36 (rung)  px=12 (rung)  border-b=2 PRIMARY|transparent
/// └── label    text-sm medium  FOREGROUND | MUTED_FOREGROUND
/// ```
#[track_caller]
pub fn tab_trigger(
    list_key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    selected: bool,
) -> El {
    let routed_key = tab_option_key(list_key, &value);
    tab_trigger_shell(Location::caller(), routed_key, selected)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        // A trigger that is squeezed anyway (a `.width(Fill)` caller,
        // a fixed-width shell) degrades to an ellipsized stub rather
        // than a blank: center-aligned nowrap text clips symmetrically
        // and would erase the label (issue #117).
        .ellipsis()
        .text_color(tab_label_color(selected))
        .animate(Timing::SPRING_QUICK)
}

/// The chrome every trigger shares — key, role, focus, hit and ring
/// overflow, rung-sized box, the reserved underline — with the label
/// (or child row) left to the caller.
fn tab_trigger_shell(caller: &'static Location<'static>, routed_key: String, selected: bool) -> El {
    let indicator = if selected {
        tokens::PRIMARY
    } else {
        // Reserved, not painted: the painter skips a fully transparent
        // border, the layout keeps its inset.
        tokens::PRIMARY.with_alpha_u8(0)
    };
    let mut trigger = El::new(Kind::Custom("tab_trigger"))
        .at_loc(caller)
        .cursor(crate::cursor::Cursor::Pointer)
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::TabTrigger)
        .role(Role::Tab)
        .aria_selected(selected)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        // Triggers abut at gap 0: there is no dead zone between them
        // to rescue, and a sideways outset would only hand each
        // trigger a band of its neighbour. Vertical slop stays.
        .hit_overflow(Sides::xy(0.0, tokens::HIT_OVERFLOW))
        .key(routed_key)
        .width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .border_widths(Sides::bottom(TAB_INDICATOR_WIDTH))
        .border_color(indicator);
    // Top corners only: the hover wash and focus ring keep the
    // theme's small radius on top, and the underline runs straight
    // along a square bottom edge — a per-side border on a rounded
    // corner follows the arc (441fcbb), which turned the indicator's
    // ends up into a bracket. `LibraryShape`: ours to shape, the
    // theme's to scale, and `apply_control` leaves it alone.
    trigger.radius = Corners::top(tokens::RADIUS_SM);
    trigger.radius_origin = RadiusOrigin::LibraryShape;
    trigger
}

fn tab_label_color(selected: bool) -> Color {
    if selected {
        tokens::FOREGROUND
    } else {
        tokens::MUTED_FOREGROUND
    }
}

/// A tab trigger with app-supplied children instead of a plain string
/// label. Use this for tabs whose options need icons, badges, dirty
/// markers, or other compact metadata while keeping the same
/// routed-key and active/inactive treatment as [`tab_trigger`]. The
/// children's own colours stand; the active/inactive label colour is
/// the caller's to set on them.
///
/// Pair these triggers with [`tabs_list_from_triggers`] so the row keeps
/// the stock underlined container:
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
    tab_trigger_shell(Location::caller(), routed_key, selected)
        .axis(Axis::Row)
        .children(children)
        .default_gap(tokens::SPACE_1)
        .align(Align::Center)
        .justify(Justify::Center)
        .text_color(tab_label_color(selected))
        .animate(Timing::SPRING_QUICK)
}

/// The tab row: one [`tab_trigger`] per option, hugging and
/// left-aligned on a shared bottom hairline, the active trigger
/// underlined.
///
/// `current` is the currently-selected value — formatted via
/// [`std::fmt::Display`] and compared (also as a `Display` string)
/// against each option's `value` to pick the active trigger.
/// `options` is an iterable of `(value, label)` pairs.
///
/// `key` is the routing namespace; per-trigger routed keys are
/// `{key}:tab:{value}`. The row itself is deliberately not keyed:
/// the slack after the last trigger is visual chrome, not an
/// interactive target, so mousing through it does not hover the whole
/// row. Apps fold trigger events back into their value field with
/// [`apply_event`].
///
/// # Anatomy
///
/// ```text
/// tabs_list  row  fill-width  gap=0  align=end  border-b=1 BORDER
/// ├── tab_trigger  (active:   FOREGROUND label, 2px PRIMARY underline)
/// ├── tab_trigger  (inactive: MUTED_FOREGROUND label, underline reserved)
/// └── …
/// ```
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

/// The tab row for triggers that have already been built with
/// [`tab_trigger`] or [`tab_trigger_content`]. This is the child-rich
/// variant of [`tabs_list`]: it keeps the stock underlined container
/// while letting each option carry icons, badges, or other compact
/// metadata.
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
    El::new(Kind::Custom("tabs_list"))
        .at_loc(caller)
        .metrics_role(MetricsRole::TabList)
        .role(Role::TabList)
        .axis(Axis::Row)
        .gap(tokens::SPACE_0)
        // Triggers stand on the hairline: their reserved underline
        // sits directly above it, so the active indicator and the rule
        // read as one edge.
        .align(Align::End)
        .justify(Justify::Start)
        // ARIA tablist pattern (horizontal): Left / Right move between
        // triggers (issue #63).
        .arrow_nav(crate::tree::ArrowNav::Horizontal)
        .children(triggers)
        .border_b()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, UiTarget};

    /// The active tab is the one whose underline is painted.
    fn is_active(trigger: &El) -> bool {
        trigger
            .border
            .as_deref()
            .and_then(|b| b.color)
            .is_some_and(|c| c.a > 0.0)
    }

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
        // No surface on either state: the underline is the whole
        // treatment.
        assert!(inactive.fill.is_none());
        assert!(inactive.stroke.is_none());
        assert_eq!(inactive.width, Size::Hug, "tabs hug their labels");
        assert_eq!(inactive.text_color, Some(tokens::MUTED_FOREGROUND));
        let reserved = inactive.border.as_deref().expect("underline reserved");
        assert_eq!(reserved.widths, Sides::bottom(TAB_INDICATOR_WIDTH));
        assert_eq!(
            reserved.color.map(|c| c.a),
            Some(0.0),
            "inactive underline is reserved but transparent"
        );
        assert!(!is_active(&inactive));

        let active = tab_trigger("settings", "account", "Account", true);
        assert!(active.fill.is_none());
        assert!(active.stroke.is_none());
        assert_eq!(active.text_color, Some(tokens::FOREGROUND));
        let painted = active.border.as_deref().expect("underline painted");
        assert_eq!(painted.widths, Sides::bottom(TAB_INDICATOR_WIDTH));
        assert_eq!(painted.color, Some(tokens::PRIMARY));
        assert!(is_active(&active));
        assert_eq!(active.a11y.as_deref().and_then(|p| p.selected), Some(true));
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
        assert!(active.fill.is_none());
        assert!(is_active(&active));
        assert_eq!(active.width, Size::Hug);
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

        // Only the trigger whose value matches `current` is underlined.
        assert!(!is_active(account));
        assert!(is_active(appearance));
        assert!(!is_active(advanced));
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
        assert!(!is_active(zero));
        assert!(is_active(seven));
        assert!(!is_active(fortytwo));
    }

    #[test]
    fn tabs_list_from_triggers_keeps_the_stock_underlined_container() {
        let list = tabs_list_from_triggers([
            tab_trigger_content("worktree", "main", [text("main").label()], true),
            tab_trigger("worktree", "feature", "feature", false),
        ]);

        assert_eq!(list.key, None);
        assert_eq!(list.metrics_role, Some(MetricsRole::TabList));
        assert_eq!(list.axis, Axis::Row);
        assert_eq!(list.children.len(), 2);
        assert!(list.fill.is_none());
        assert!(list.stroke.is_none());
        let rule = list.border.as_deref().expect("bottom hairline");
        assert_eq!(rule.widths, Sides::bottom(1.0));
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
        // Without `.animate()` on the trigger, the label colour (and
        // the hover wash) snaps on every rebuild, which reads as a
        // hard cut between tabs.
        assert!(
            tab_trigger("settings", "account", "Account", true)
                .animate_timing()
                .is_some()
        );
        assert!(
            tab_trigger("settings", "account", "Account", false)
                .animate_timing()
                .is_some()
        );
    }

    #[test]
    fn tabs_list_is_an_underlined_row_of_hugging_triggers() {
        // Ruled 2026-10-14: navigation draws as navigation. No pill,
        // no equal-width cells; a hairline under the row, the active
        // trigger's underline on top of it.
        let mut list = tabs_list(
            "settings",
            &"account",
            [("account", "Account"), ("settings", "Settings")],
        );
        assert!(list.fill.is_none(), "no pill");
        assert!(list.stroke.is_none());
        assert_eq!(list.padding, Sides::zero());
        assert_eq!(list.gap, tokens::SPACE_0);
        assert_eq!(list.align, Align::End, "triggers stand on the rule");
        assert_eq!(list.justify, Justify::Start, "left-aligned");
        assert_eq!(list.axis, Axis::Row);
        assert_eq!(list.width, Size::Fill(1.0));
        let rule = list.border.as_deref().expect("bottom hairline");
        assert_eq!(rule.widths, Sides::bottom(1.0));
        assert!(rule.color.is_none(), "the hairline is the stock BORDER");
        for trigger in &list.children {
            assert_eq!(trigger.width, Size::Hug);
        }
        // The list itself is not focusable or keyed — only its
        // triggers are. Otherwise Tab would land on the row before
        // the first tab, and pointer hover in the slack would brighten
        // the whole row.
        assert!(!list.focusable);
        assert!(list.key.is_none());
        // ARIA tablist: the row is one arrow-navigable group, Left /
        // Right step between triggers (issue #63).
        assert_eq!(list.arrow_nav, Some(crate::tree::ArrowNav::Horizontal));

        // Laid out, the triggers hug and the row's slack is to the
        // right of the last one; the active underline sits directly
        // above the row's hairline.
        let mut state = UiState::new();
        layout(&mut list, &mut state, Rect::new(0.0, 0.0, 400.0, 60.0));
        let first = list.children[0].computed_rect;
        let second = list.children[1].computed_rect;
        assert!(first.w < 200.0, "hugging, not half the row: {first:?}");
        assert!((second.x - (first.x + first.w)).abs() < 0.5, "no gap");
        assert!(second.x + second.w < 400.0 - 40.0, "slack remains");
        let row = list.computed_rect;
        assert!(
            (first.y + first.h - (row.y + row.h - 1.0)).abs() < 0.5,
            "trigger bottom meets the 1px hairline: {first:?} in {row:?}"
        );
    }

    #[test]
    fn tabs_list_slack_is_not_a_hover_target() {
        let mut list = tabs_list(
            "settings",
            &"account",
            [("account", "Account"), ("advanced", "Advanced")],
        );
        let mut state = UiState::new();
        layout(&mut list, &mut state, Rect::new(0.0, 0.0, 400.0, 60.0));

        let first = list.children[0].computed_rect;
        let last = list.children[1].computed_rect;

        let trigger_target = hit_test_target(
            &list,
            &state,
            (first.x + first.w / 2.0, first.y + first.h / 2.0),
        )
        .expect("tab trigger should still be interactive");
        assert_eq!(trigger_target.key, "settings:tab:account");

        let slack_x = (last.x + last.w + 400.0) / 2.0;
        let slack_y = first.y + first.h / 2.0;
        assert_eq!(
            hit_test_target(&list, &state, (slack_x, slack_y)),
            None,
            "the slack after the last trigger should not hover the tab-list shell"
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
                content_inset: Sides::zero(),
                font_size: crate::tokens::TEXT_SM.size,
                line_height: crate::tokens::TEXT_SM.line_height,
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

    /// #117 regression: triggers squeezed far below their label width keep
    /// an ellipsized stub instead of center-clipping to a blank pill.
    #[test]
    fn an_overfull_row_overflows_with_labels_intact_and_the_lint_says_so() {
        use crate::bundle::lint::{FindingKind, lint};
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = tabs_list(
            "view",
            &"board",
            [
                ("board", "Board layout"),
                ("map", "Map overview"),
                ("stats", "Statistics"),
            ],
        );
        let mut state = UiState::new();
        // Far narrower than the three labels need.
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 120.0, 40.0));

        // Hugging tabs do not squeeze every label to a stub (the
        // kiosk's "Telemetry" and "Event Log" nearly touching while
        // "QC" floats in slack); the row overflows with the labels
        // whole …
        let ops = draw_ops(&tree, &state);
        let labels: Vec<&str> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::GlyphRun { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            ["Board layout", "Map overview", "Statistics"],
            "labels are drawn whole"
        );

        // … and the bundle lint reports the overflow, which is the
        // shell's cue to scroll the row or move the tail into a menu.
        let report = lint(&tree, &state, &crate::theme::Theme::default());
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::Overflow),
            "{}",
            report.text()
        );
    }
}
