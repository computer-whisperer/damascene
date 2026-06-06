//! Editor tabs — the closeable, addable tab strip familiar from VS
//! Code, Chrome, and Ant Design's `Tabs type="editable-card"`. Each
//! tab carries a label and a close (`×`) affordance; a trailing `+`
//! button asks the app to open a new tab.
//!
//! Distinct from [`crate::widgets::tabs`], which models the shadcn /
//! Radix segmented-control pattern (a muted pill with one active
//! trigger raised inside it). Use `tabs_list` for view-mode toggles
//! and settings-style category pickers; reach for `editor_tabs` when
//! the tabs represent **opened documents** the user can close and
//! create.
//!
//! # Shape
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Workbench {
//!     docs: Vec<String>,
//!     active: String,
//! }
//!
//! impl App for Workbench {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             editor_tabs(
//!                 "docs",
//!                 &self.active,
//!                 self.docs.iter().map(|d| (d.clone(), d.clone())),
//!             ),
//!             // panel for the active document...
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         let mut counter = 0;
//!         editor_tabs::apply_event(
//!             &mut self.docs,
//!             &mut self.active,
//!             &event,
//!             "docs",
//!             |s| Some(s.to_string()),
//!             || {
//!                 counter += 1;
//!                 format!("doc-{counter}")
//!             },
//!         );
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}:tab:{value}` — `Click` on a tab body activates it;
//!   `MiddleClick` closes it. The token format matches
//!   [`crate::widgets::tabs`] so the same per-app conventions apply.
//! - `{key}:close:{value}` — `Click` on a tab's `×`; the app removes
//!   that document and (if it was active) picks a neighbour.
//! - `{key}:add` — `Click` on the trailing `+`; the app appends a
//!   new tab and activates it.
//!
//! # Configuration
//!
//! Default flavor matches VS Code: lifted active tab, close icon at
//! full opacity on the active tab and dimmed on the rest. Override
//! via [`editor_tabs_with`] + [`EditorTabsConfig`] for top-accent
//! (Chrome-like) or always-visible close icons.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom` for
//! the inspector tag, `.focusable()` + `.paint_overflow()` for the
//! focus ring on each tab, `.key()` for hit-test routing, and
//! [`crate::widgets::button::icon_button`] (with `.ghost()`) for the
//! close + add affordances. An app crate can fork this file. See
//! `widget_kit.md`.

use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind};
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::icon_button;
use crate::{IconName, text};

/// Visual treatment for the active tab.
///
/// `Lifted` is the default — it matches VS Code, Sublime, and most
/// modern editor tab strips: the active tab fills with [`tokens::CARD`]
/// so it visually attaches to whatever panel sits below it. `TopAccent`
/// is the Chrome-style treatment (a coloured rule sits above the active
/// tab); `BottomRule` is the Material-style rule under the active tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ActiveTabStyle {
    /// VS Code: active tab fills with `CARD`, inactive tabs are
    /// transparent over the strip's `MUTED` background.
    #[default]
    Lifted,
    /// Chrome-ish: a 2 px [`tokens::PRIMARY`] rule sits above the
    /// active tab; tab fills stay uniform across active and inactive.
    TopAccent,
    /// Material: a 2 px [`tokens::PRIMARY`] rule sits below the active
    /// tab.
    BottomRule,
}

/// When the close (`×`) icon is rendered on each tab.
///
/// All three variants keep the close icon in the tab layout so the
/// tab geometry stays stable across selection. They differ only in
/// the rest-state opacity: `ActiveOrHover` hides it entirely until a
/// hover signal arrives, `Dimmed` keeps a faint hint, and `Always`
/// shows it unconditionally.
///
/// The hover signal cascades from the tab through
/// [`crate::tree::El::hover_alpha`] — when the user mouses over the
/// tab (or directly over the `×`), the icon eases up to full opacity
/// via the runtime's subtree interaction envelope. Keyboard focus on
/// the tab also reveals the icon, so a tabbed-into inactive tab still
/// shows its close affordance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CloseVisibility {
    /// VS Code default: full opacity on the active tab, invisible on
    /// inactive tabs at rest, eased up to full on hover (of either
    /// the tab body or the `×` itself).
    #[default]
    ActiveOrHover,
    /// Always at full opacity. Matches Antd `editable-card` tabs.
    Always,
    /// Always visible but de-emphasized on inactive non-hovered tabs
    /// (rest at 40% opacity), brightening to full on hover. A softer
    /// "always discoverable" variant.
    Dimmed,
}

/// Configuration for [`editor_tabs_with`]. Public-fields struct so
/// callers can spread `..Default::default()` to override one field
/// at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct EditorTabsConfig {
    pub active_style: ActiveTabStyle,
    pub close_visibility: CloseVisibility,
}

/// What a routed [`UiEvent`] means for an editor-tabs strip keyed
/// `key`.
///
/// Returned by [`classify_event`]; [`apply_event`] is the convenience
/// wrapper that applies the action to the app's `(tabs, active)` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditorTabsAction<'a> {
    /// A tab body was clicked. Activate this tab.
    Select(&'a str),
    /// A tab's `×` was clicked, or a tab body was middle-clicked.
    /// Remove this tab from the list and, if it was active, pick a
    /// neighbour.
    Close(&'a str),
    /// The trailing `+` button was clicked. Append a new tab and
    /// activate it.
    Add,
}

/// Format the routed key emitted when a tab body is clicked. Mirrors
/// [`crate::widgets::tabs::tab_option_key`] so apps that already use
/// `tab_option_key` for [`tabs_list`][crate::widgets::tabs::tabs_list]
/// can reuse the same helper.
pub fn editor_tab_select_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:tab:{value}")
}

/// Format the routed key emitted when a tab's `×` is clicked.
pub fn editor_tab_close_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:close:{value}")
}

/// Format the routed key emitted when the trailing `+` is clicked.
pub fn editor_tab_add_key(key: &str) -> String {
    format!("{key}:add")
}

/// Classify a routed [`UiEvent`] against an editor-tabs strip keyed
/// `key`. Returns `None` for events that aren't for this strip.
///
/// `Click` / `Activate` qualify for normal tab-strip actions.
/// `MiddleClick` qualifies only on tab and close routes, mapping to
/// [`EditorTabsAction::Close`] so editor tabs follow the common
/// browser / editor convention. The borrowed string in
/// [`EditorTabsAction::Select`] / [`EditorTabsAction::Close`] points
/// into the event's routed key, so apps that want to keep the value
/// beyond the match arm should `.to_string()` or `.parse()` it inline.
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<EditorTabsAction<'a>> {
    if !matches!(
        event.kind,
        UiEventKind::Click | UiEventKind::Activate | UiEventKind::MiddleClick
    ) {
        return None;
    }
    let routed = event.route()?;
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    if event.kind == UiEventKind::MiddleClick {
        if let Some(value) = rest
            .strip_prefix("tab:")
            .or_else(|| rest.strip_prefix("close:"))
        {
            return Some(EditorTabsAction::Close(value));
        }
        return None;
    }
    if let Some(value) = rest.strip_prefix("tab:") {
        return Some(EditorTabsAction::Select(value));
    }
    if let Some(value) = rest.strip_prefix("close:") {
        return Some(EditorTabsAction::Close(value));
    }
    if rest == "add" {
        return Some(EditorTabsAction::Add);
    }
    None
}

/// Fold a routed [`UiEvent`] into the app's `(tabs, active)` state for
/// an editor-tabs strip keyed `key`. Returns `true` if the event was
/// for this strip (so the caller can short-circuit further dispatch),
/// `false` otherwise.
///
/// `parse` converts the raw value token back to the app's value type.
/// `mint_new` produces a fresh value when the user clicks `+`. The
/// helper handles three cases:
///
/// - **Select** — sets `active` to the parsed value.
/// - **Close** — removes the matching entry from `tabs`. If the
///   closed tab was active, `active` shifts to the neighbour at the
///   same index (or the previous one when closing the last tab); the
///   list is left untouched if the parsed value is no longer present.
///   The last-remaining tab can't be closed via this helper — apps
///   that want to allow that must handle [`EditorTabsAction::Close`]
///   directly so they can decide what `active` becomes.
/// - **Add** — appends `mint_new()` and activates it.
///
/// Apps that need finer control (e.g. confirmation prompts before
/// closing a dirty tab, or closing the last tab) should call
/// [`classify_event`] and handle each action themselves.
pub fn apply_event<V>(
    tabs: &mut Vec<V>,
    active: &mut V,
    event: &UiEvent,
    key: &str,
    parse: impl Fn(&str) -> Option<V>,
    mint_new: impl FnOnce() -> V,
) -> bool
where
    V: Clone + PartialEq,
{
    match classify_event(event, key) {
        Some(EditorTabsAction::Select(raw)) => {
            if let Some(v) = parse(raw) {
                *active = v;
            }
            true
        }
        Some(EditorTabsAction::Close(raw)) => {
            let Some(target) = parse(raw) else {
                return true;
            };
            let Some(index) = tabs.iter().position(|t| *t == target) else {
                return true;
            };
            // Refuse to close the last tab — leaves `active` pointing
            // at a non-existent value otherwise. Apps that want to
            // allow it should handle Close directly.
            if tabs.len() <= 1 {
                return true;
            }
            let was_active = *active == target;
            tabs.remove(index);
            if was_active {
                let next = index.min(tabs.len() - 1);
                *active = tabs[next].clone();
            }
            true
        }
        Some(EditorTabsAction::Add) => {
            let new = mint_new();
            *active = new.clone();
            tabs.push(new);
            true
        }
        None => false,
    }
}

/// The trigger for one tab inside an [`editor_tabs`] strip. Apps
/// usually let `editor_tabs` build these from its options iterator;
/// reach for `editor_tab` directly when composing the strip by hand
/// (e.g. mixing in icons, modified-dot indicators, or per-tab tooltips
/// the wrapper doesn't expose).
///
/// `strip_key` is the parent strip's key — the routed keys on the
/// resulting element are `{strip_key}:tab:{value}` (whole tab) and
/// `{strip_key}:close:{value}` (the `×`). `selected` styles the tab
/// as active.
///
/// `leading` is an optional element placed inside the tab body before
/// the label — typically a small status indicator (CI dot, modified
/// mark, brand glyph) that should sit inside the tab and inherit its
/// hover / focus envelope. Pass `None` for the plain label-only shape.
#[track_caller]
pub fn editor_tab(
    strip_key: &str,
    value: impl std::fmt::Display,
    leading: Option<El>,
    label: impl Into<String>,
    selected: bool,
    config: EditorTabsConfig,
) -> El {
    let select_key = editor_tab_select_key(strip_key, &value);
    let close_key = editor_tab_close_key(strip_key, &value);

    let label_el = text(label).label().ellipsis().text_color(if selected {
        tokens::FOREGROUND
    } else {
        tokens::MUTED_FOREGROUND
    });

    // The close icon is always present in the layout so tab geometry
    // stays stable across selection. The active tab paints it at full
    // opacity; inactive tabs use `hover_alpha(rest, 1.0)` so the icon
    // eases between its rest opacity and full as the tab is hovered,
    // pressed, or keyboard-focused.
    let mut close = icon_button(IconName::X)
        .key(close_key)
        .icon_size(tokens::ICON_XS)
        .ghost()
        .width(Size::Fixed(tokens::SPACE_5))
        .height(Size::Fixed(tokens::SPACE_5));
    if !selected {
        let rest = match config.close_visibility {
            CloseVisibility::ActiveOrHover => 0.0,
            CloseVisibility::Dimmed => 0.4,
            CloseVisibility::Always => 1.0,
        };
        // Only attach the modifier when it would do something (rest <
        // 1.0). At 1.0 the modifier is a no-op; skipping it keeps
        // tree dumps for the `Always` flavor uncluttered.
        if rest < 1.0 {
            close = close.hover_alpha(rest, 1.0);
        }
    }

    let mut body_children: Vec<El> = Vec::with_capacity(3);
    if let Some(leading) = leading {
        body_children.push(leading);
    }
    body_children.push(label_el);
    body_children.push(close);
    let body = row(body_children)
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .padding(Sides::xy(tokens::SPACE_3, 0.0))
        .height(Size::Fill(1.0));

    // The accent rule is a fixed 2 px row above or below the body
    // (depending on `active_style`). Always rendered so the tab keeps
    // a stable height across selection changes; the colour is
    // unset on inactive tabs (no fill draw).
    let rule = || {
        let mut el = El::new(Kind::Custom("editor_tab_accent_rule"))
            .height(Size::Fixed(2.0))
            .width(Size::Fill(1.0));
        if selected {
            el = el.fill(tokens::PRIMARY);
        }
        el
    };

    let stack = match config.active_style {
        ActiveTabStyle::Lifted => column([body]),
        ActiveTabStyle::TopAccent => column([rule(), body]),
        ActiveTabStyle::BottomRule => column([body, rule()]),
    };

    let mut tab = stack
        .at_loc(Location::caller())
        .key(select_key)
        .style_profile(StyleProfile::Solid)
        .focusable()
        .cursor(Cursor::Pointer)
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .axis(Axis::Column)
        .align(Align::Stretch)
        .height(Size::Fixed(tokens::CONTROL_HEIGHT + 2.0))
        .width(Size::Hug);
    if matches!(config.active_style, ActiveTabStyle::Lifted) && selected {
        tab = tab.fill(tokens::CARD).default_radius(tokens::RADIUS_SM);
    }
    tab
}

/// An editor-tab strip with default config (lifted active tab, dimmed
/// close icons on inactive tabs). See [`editor_tabs_with`] for
/// flavor overrides.
#[track_caller]
pub fn editor_tabs<I, V, L>(
    key: impl Into<String>,
    current: &impl std::fmt::Display,
    options: I,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    editor_tabs_with(key, current, options, EditorTabsConfig::default())
}

/// An editor-tab strip with explicit configuration. Like [`editor_tabs`]
/// but lets the caller pick the active-tab treatment and close-icon
/// visibility.
#[track_caller]
pub fn editor_tabs_with<I, V, L>(
    key: impl Into<String>,
    current: &impl std::fmt::Display,
    options: I,
    config: EditorTabsConfig,
) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    let caller = Location::caller();
    let key = key.into();
    let current_str = current.to_string();

    let mut children: Vec<El> = options
        .into_iter()
        .map(|(value, label)| {
            let selected = value.to_string() == current_str;
            editor_tab(&key, value, None, label, selected, config).at_loc(caller)
        })
        .collect();

    // Trailing `+` button — separated from the last tab by a small
    // gap so it reads as a distinct "new tab" affordance rather than
    // another tab. Ghosted (no fill, no stroke) to match the strip's
    // flat aesthetic.
    let add_key = editor_tab_add_key(&key);
    let add_btn = icon_button(IconName::Plus)
        .at_loc(caller)
        .key(add_key)
        .icon_size(tokens::ICON_SM)
        .ghost()
        .width(Size::Fixed(tokens::CONTROL_HEIGHT))
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));
    children.push(add_btn);

    El::new(Kind::Custom("editor_tabs"))
        .at_loc(caller)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_1)
        .align(Align::Center)
        .children(children)
        .fill(tokens::MUTED)
        .default_padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::KeyModifiers;

    fn click(key: &str) -> UiEvent {
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

    fn middle_click(key: &str) -> UiEvent {
        let mut event = click(key);
        event.kind = UiEventKind::MiddleClick;
        event
    }

    #[test]
    fn key_helpers_match_widget_format() {
        assert_eq!(editor_tab_select_key("docs", &"readme"), "docs:tab:readme");
        assert_eq!(editor_tab_close_key("docs", &"readme"), "docs:close:readme");
        assert_eq!(editor_tab_add_key("docs"), "docs:add");
    }

    #[test]
    fn classify_event_recognises_all_three_actions() {
        assert_eq!(
            classify_event(&click("docs:tab:readme"), "docs"),
            Some(EditorTabsAction::Select("readme")),
        );
        assert_eq!(
            classify_event(&click("docs:close:readme"), "docs"),
            Some(EditorTabsAction::Close("readme")),
        );
        assert_eq!(
            classify_event(&click("docs:add"), "docs"),
            Some(EditorTabsAction::Add),
        );
        // Non-matching keys fall through.
        assert_eq!(classify_event(&click("other:tab:x"), "docs"), None);
        assert_eq!(classify_event(&click("docs"), "docs"), None);
    }

    #[test]
    fn classify_event_middle_click_on_tab_closes_it() {
        assert_eq!(
            classify_event(&middle_click("docs:tab:readme"), "docs"),
            Some(EditorTabsAction::Close("readme")),
        );
        assert_eq!(
            classify_event(&middle_click("docs:close:readme"), "docs"),
            Some(EditorTabsAction::Close("readme")),
        );
        assert_eq!(
            classify_event(&middle_click("docs:add"), "docs"),
            None,
            "middle-clicking the add button should not create a tab",
        );
    }

    #[test]
    fn classify_event_ignores_non_activating_kinds() {
        let mut ev = click("docs:close:readme");
        ev.kind = UiEventKind::PointerDown;
        assert_eq!(classify_event(&ev, "docs"), None);
        ev.kind = UiEventKind::Activate;
        assert_eq!(
            classify_event(&ev, "docs"),
            Some(EditorTabsAction::Close("readme")),
            "keyboard activation should fire close like a click",
        );
    }

    #[test]
    fn editor_tab_routes_via_select_key() {
        let tab = editor_tab(
            "docs",
            "readme",
            None,
            "README.md",
            false,
            EditorTabsConfig::default(),
        );
        assert_eq!(tab.key.as_deref(), Some("docs:tab:readme"));
        assert!(tab.focusable);
    }

    #[test]
    fn editor_tab_active_lifted_fills_with_card() {
        let active = editor_tab(
            "docs",
            "readme",
            None,
            "README.md",
            true,
            EditorTabsConfig::default(),
        );
        let inactive = editor_tab(
            "docs",
            "readme",
            None,
            "README.md",
            false,
            EditorTabsConfig::default(),
        );
        assert_eq!(active.fill, Some(tokens::CARD));
        assert_eq!(
            inactive.fill, None,
            "inactive lifted tabs leave fill unset so the strip's MUTED background shows through",
        );
    }

    #[test]
    fn editor_tab_top_accent_renders_a_rule_row_above_the_body() {
        let cfg = EditorTabsConfig {
            active_style: ActiveTabStyle::TopAccent,
            ..Default::default()
        };
        let active = editor_tab("docs", "readme", None, "README.md", true, cfg);
        // Column with [rule, body]; the rule is the first child and
        // carries the PRIMARY fill on the active tab.
        assert!(active.children.len() >= 2);
        assert_eq!(active.children[0].fill, Some(tokens::PRIMARY));
    }

    #[test]
    fn editor_tab_bottom_rule_renders_a_rule_row_below_the_body() {
        let cfg = EditorTabsConfig {
            active_style: ActiveTabStyle::BottomRule,
            ..Default::default()
        };
        let active = editor_tab("docs", "readme", None, "README.md", true, cfg);
        let last = active.children.last().expect("at least one child");
        assert_eq!(last.fill, Some(tokens::PRIMARY));
    }

    #[test]
    fn editor_tab_inactive_under_top_accent_omits_the_rule_fill() {
        let cfg = EditorTabsConfig {
            active_style: ActiveTabStyle::TopAccent,
            ..Default::default()
        };
        let inactive = editor_tab("docs", "readme", None, "README.md", false, cfg);
        // Rule row is still present so the tab's height stays stable
        // across selection changes, but its fill is unset.
        assert_eq!(inactive.children[0].fill, None);
    }

    #[test]
    fn close_visibility_active_or_hover_hides_close_at_rest_on_inactive() {
        let cfg = EditorTabsConfig {
            close_visibility: CloseVisibility::ActiveOrHover,
            ..Default::default()
        };
        // Each tab is `column([body])` (Lifted); body is the first
        // child, which is a row of [label, close]. The close icon is
        // always present in the layout — only its rest opacity changes
        // — so geometry stays stable across selection.
        let active = editor_tab("docs", "readme", None, "README.md", true, cfg);
        let inactive = editor_tab("docs", "readme", None, "README.md", false, cfg);
        let active_body = &active.children[0];
        let inactive_body = &inactive.children[0];
        assert_eq!(active_body.children.len(), 2);
        assert_eq!(inactive_body.children.len(), 2);
        // The active tab's close paints at full opacity (no modifier).
        let active_close = &active_body.children[1];
        assert_eq!(active_close.hover_alpha, None);
        // The inactive tab's close is invisible at rest, fades in on
        // hover / focus / press via the subtree interaction envelope.
        let inactive_close = &inactive_body.children[1];
        let cfg = inactive_close.hover_alpha.expect("hover_alpha attached");
        assert_eq!(cfg.rest, 0.0);
        assert_eq!(cfg.peak, 1.0);
    }

    #[test]
    fn close_visibility_dimmed_uses_partial_rest_opacity() {
        let cfg = EditorTabsConfig {
            close_visibility: CloseVisibility::Dimmed,
            ..Default::default()
        };
        let inactive = editor_tab("docs", "readme", None, "README.md", false, cfg);
        let body = &inactive.children[0];
        let close = &body.children[1];
        // Dimmed sits between hidden and visible — close should rest
        // around 0.4 alpha and ease up on hover.
        match close.hover_alpha {
            Some(cfg) => {
                assert!(
                    cfg.rest > 0.0 && cfg.rest < 1.0,
                    "Dimmed rest should be partial; got {}",
                    cfg.rest,
                );
                assert_eq!(cfg.peak, 1.0);
            }
            None => panic!("Dimmed should attach hover_alpha so interaction composes the alpha"),
        }
    }

    #[test]
    fn close_visibility_always_skips_hover_alpha() {
        let cfg = EditorTabsConfig {
            close_visibility: CloseVisibility::Always,
            ..Default::default()
        };
        let inactive = editor_tab("docs", "readme", None, "README.md", false, cfg);
        let body = &inactive.children[0];
        let close = &body.children[1];
        // `Always` is full opacity unconditionally — the modifier is a
        // no-op at rest=1.0, so we skip attaching it to keep tree
        // dumps for this flavor uncluttered.
        assert_eq!(close.hover_alpha, None);
    }

    #[test]
    fn editor_tab_leading_prepends_inside_the_body_row() {
        let dot = crate::tree::column([crate::widgets::text::text("●")])
            .width(Size::Fixed(8.0))
            .height(Size::Fixed(8.0));
        let tab = editor_tab(
            "docs",
            "readme",
            Some(dot),
            "README.md",
            false,
            EditorTabsConfig::default(),
        );
        // Outer is column([body]); body's children become
        // [leading, label, close] when leading is Some.
        let body = &tab.children[0];
        assert_eq!(body.children.len(), 3);
    }

    #[test]
    fn editor_tabs_appends_an_add_button_with_the_strip_add_key() {
        let strip = editor_tabs(
            "docs",
            &"readme",
            [("readme", "README.md"), ("main", "main.rs")],
        );
        // Two tabs + the trailing + button.
        assert_eq!(strip.children.len(), 3);
        let add = strip.children.last().unwrap();
        assert_eq!(add.key.as_deref(), Some("docs:add"));
    }

    #[test]
    fn editor_tabs_marks_only_the_current_value_active() {
        let strip = editor_tabs(
            "docs",
            &"main",
            [
                ("readme", "README.md"),
                ("main", "main.rs"),
                ("cargo", "Cargo.toml"),
            ],
        );
        assert_eq!(strip.children[0].fill, None);
        assert_eq!(strip.children[1].fill, Some(tokens::CARD));
        assert_eq!(strip.children[2].fill, None);
    }

    #[test]
    fn apply_event_select_swaps_active_without_touching_tabs() {
        let mut tabs = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut active = "a".to_string();
        let next_id = || "fresh".to_string();
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &click("docs:tab:b"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(active, "b");
        assert_eq!(tabs, vec!["a", "b", "c"]);
    }

    #[test]
    fn apply_event_close_removes_tab_and_picks_neighbour_when_active() {
        let mut tabs = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut active = "b".to_string();
        let next_id = || "fresh".to_string();
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &click("docs:close:b"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(tabs, vec!["a", "c"]);
        // The middle tab was active; closing it shifts to the same
        // index, which is now "c".
        assert_eq!(active, "c");
    }

    #[test]
    fn apply_event_close_last_tab_picks_previous_neighbour() {
        let mut tabs = vec!["a".to_string(), "b".to_string()];
        let mut active = "b".to_string();
        let next_id = || "fresh".to_string();
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &click("docs:close:b"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(tabs, vec!["a"]);
        assert_eq!(active, "a");
    }

    #[test]
    fn apply_event_close_inactive_tab_leaves_active_alone() {
        let mut tabs = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut active = "a".to_string();
        let next_id = || "fresh".to_string();
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &click("docs:close:c"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(tabs, vec!["a", "b"]);
        assert_eq!(active, "a");
    }

    #[test]
    fn apply_event_middle_click_on_tab_closes_it() {
        let mut tabs = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut active = "a".to_string();
        let next_id = || "fresh".to_string();
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &middle_click("docs:tab:b"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(tabs, vec!["a", "c"]);
        assert_eq!(active, "a");
    }

    #[test]
    fn apply_event_refuses_to_close_the_last_tab() {
        let mut tabs = vec!["a".to_string()];
        let mut active = "a".to_string();
        let next_id = || "fresh".to_string();
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &click("docs:close:a"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(
            tabs,
            vec!["a"],
            "the last tab can't be closed via the helper"
        );
        assert_eq!(active, "a");
    }

    #[test]
    fn apply_event_add_appends_and_activates_a_minted_tab() {
        let mut tabs = vec!["a".to_string()];
        let mut active = "a".to_string();
        let mut counter = 0;
        let next_id = || {
            counter += 1;
            format!("new-{counter}")
        };
        assert!(apply_event(
            &mut tabs,
            &mut active,
            &click("docs:add"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(tabs, vec!["a", "new-1"]);
        assert_eq!(active, "new-1");
    }

    #[test]
    fn apply_event_returns_false_for_foreign_events() {
        let mut tabs = vec!["a".to_string()];
        let mut active = "a".to_string();
        let next_id = || "fresh".to_string();
        assert!(!apply_event(
            &mut tabs,
            &mut active,
            &click("save"),
            "docs",
            |s| Some(s.to_string()),
            next_id,
        ));
        assert_eq!(tabs, vec!["a"]);
        assert_eq!(active, "a");
    }
}
