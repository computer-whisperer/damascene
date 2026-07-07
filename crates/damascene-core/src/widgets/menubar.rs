//! Menubar anatomy — shadcn-shaped top-level action menus.
//!
//! Use this when a toolbar wants named application menus such as
//! File / Edit / View. It is the horizontal sibling of
//! [`crate::widgets::dropdown_menu`]: triggers live inline in a
//! [`menubar`] root, and each open menu is rendered at the root of the
//! El tree via [`menubar_menu`].
//!
//! # Shape
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Workbench {
//!     open_menu: Option<String>,
//! }
//!
//! impl App for Workbench {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         let open = self.open_menu.as_deref();
//!         let bar = menubar([
//!             menubar_trigger("main-menu", "file", "File", open == Some("file")),
//!             menubar_trigger("main-menu", "edit", "Edit", open == Some("edit")),
//!         ]);
//!
//!         let mut layers = vec![bar];
//!         if open == Some("file") {
//!             layers.push(menubar_menu("main-menu", "file", [
//!                 menubar_item_with_shortcut("New Window", "Ctrl+N"),
//!                 menubar_separator(),
//!                 menubar_item([menubar_item_label("Close")]).key("close"),
//!             ]));
//!         }
//!         stack(layers)
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         menubar::apply_event(&mut self.open_menu, &event, "main-menu");
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}:menu:{value}` — trigger click / activate toggles that
//!   menu open.
//! - `{key}:menu:{value}:dismiss` — click outside the open popover;
//!   clears the open menu.
//!
//! Menu rows are ordinary focusable keyed elements. Apps key them with
//! the command they should route to, the same as dropdown-menu items.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::popover::{Anchor, popover};
use crate::widgets::separator::separator;
use crate::widgets::text::{mono, text};
use crate::{IntoIconSource, icon};

/// What a routed [`UiEvent`] means for a controlled menubar keyed
/// `key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MenubarAction<'a> {
    /// A top-level trigger was clicked or activated.
    Toggle(&'a str),
    /// The popover dismiss scrim was clicked.
    Dismiss(&'a str),
}

/// Classify a routed [`UiEvent`] against a controlled menubar keyed
/// `key`. Returns `None` for events that aren't for this menubar.
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<MenubarAction<'a>> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let routed = event.route()?;
    let rest = routed.strip_prefix(key)?.strip_prefix(':')?;
    let value = rest.strip_prefix("menu:")?;
    if let Some(value) = value.strip_suffix(":dismiss") {
        return Some(MenubarAction::Dismiss(value));
    }
    Some(MenubarAction::Toggle(value))
}

/// Fold a routed [`UiEvent`] into an app-owned open-menu slot. The
/// open value is the raw `{value}` token from [`menubar_trigger_key`].
pub fn apply_event(open: &mut Option<String>, event: &UiEvent, key: &str) -> bool {
    let Some(action) = classify_event(event, key) else {
        return false;
    };
    match action {
        MenubarAction::Toggle(value) => {
            if open.as_deref() == Some(value) {
                *open = None;
            } else {
                *open = Some(value.to_string());
            }
        }
        MenubarAction::Dismiss(value) => {
            if open.as_deref() == Some(value) {
                *open = None;
            }
        }
    }
    true
}

/// Format the routed key emitted by a top-level menubar trigger.
pub fn menubar_trigger_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:menu:{value}")
}

/// Horizontal menubar root. Put [`menubar_trigger`] children inside.
#[track_caller]
pub fn menubar<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("menubar"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_1)
        .align(Align::Center)
        .children(children)
        .fill(tokens::BACKGROUND)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .default_padding(Sides::all(tokens::SPACE_1))
        .width(Size::Hug)
        .default_height(Size::Fixed(tokens::SPACE_10))
}

/// Top-level trigger inside a [`menubar`]. Clicks route as
/// `{key}:menu:{value}` and should be folded with [`apply_event`].
#[track_caller]
pub fn menubar_trigger(
    key: &str,
    value: impl std::fmt::Display,
    label: impl Into<String>,
    open: bool,
) -> El {
    let routed_key = menubar_trigger_key(key, &value);
    let base = El::new(Kind::Custom("menubar_trigger"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Button)
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
    let styled = if open { base.current() } else { base.ghost() };
    styled.animate(Timing::SPRING_QUICK)
}

/// Anchored popover for one open menubar trigger. Render this as a
/// root-level overlay only while the menu is open.
#[track_caller]
pub fn menubar_menu<I, E>(key: &str, value: impl std::fmt::Display, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let trigger_key = menubar_trigger_key(key, &value);
    popover(
        trigger_key.clone(),
        Anchor::below_key(trigger_key),
        menubar_content(children),
    )
}

/// The floating menu panel body without its overlay/scrim wrapper.
#[track_caller]
pub fn menubar_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("menubar_content"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Popover)
        .arrow_nav_siblings()
        .children(children)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .radius(crate::widgets::popover::MENU_PANEL_RADIUS)
        .enter_transition(crate::anim::EnterTransition::fade().with_slide(0.0, -6.0))
        .shadow(tokens::SHADOW_MD)
        .padding(Sides::all(crate::widgets::popover::MENU_PANEL_PADDING))
        .gap(0.0)
        .width(Size::Hug)
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}

/// Tight column grouping related rows inside a menu panel.
#[track_caller]
pub fn menubar_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(0.0)
}

/// Non-interactive muted caption heading a section of menu rows.
#[track_caller]
pub fn menubar_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .caption()
        .semibold()
        .color(tokens::MUTED_FOREGROUND)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .width(Size::Fill(1.0))
}

/// Horizontal divider between menu sections, with vertical breathing room.
#[track_caller]
pub fn menubar_separator() -> El {
    column([separator()])
        .at_loc(Location::caller())
        .padding(Sides {
            left: 0.0,
            right: 0.0,
            top: tokens::SPACE_1,
            bottom: tokens::SPACE_1,
        })
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Focusable menu row. Compose slots ([`menubar_icon`],
/// [`menubar_item_label`], [`menubar_shortcut`]) and chain `.key(...)`
/// with the command the row should route to.
#[track_caller]
pub fn menubar_item<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("menubar_item"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::MenuItem)
        .focusable()
        .focus_ring_inside()
        .cursor(Cursor::Pointer)
        .children(children)
        .fill(tokens::POPOVER)
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .default_gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(30.0))
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Start)
}

/// Label slot for a [`menubar_item`] — regular-weight, ellipsizing,
/// fills the remaining row width.
#[track_caller]
pub fn menubar_item_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Regular)
        .ellipsis()
        .width(Size::Fill(1.0))
}

/// Small muted leading-icon slot for a [`menubar_item`].
#[track_caller]
pub fn menubar_icon(source: impl IntoIconSource) -> El {
    icon(source)
        .at_loc(Location::caller())
        .icon_size(tokens::ICON_SM)
        .color(tokens::MUTED_FOREGROUND)
}

/// Trailing keyboard-shortcut hint (e.g. `"Ctrl+N"`) in muted monospace.
/// Display only — wiring the hotkey is the app's job.
#[track_caller]
pub fn menubar_shortcut(shortcut: impl Into<String>) -> El {
    mono(shortcut)
        .at_loc(Location::caller())
        .caption()
        .color(tokens::MUTED_FOREGROUND)
        .width(Size::Hug)
}

/// Convenience: a [`menubar_item`] with label + trailing shortcut hint.
#[track_caller]
pub fn menubar_item_with_shortcut(label: impl Into<String>, shortcut: impl Into<String>) -> El {
    menubar_item([menubar_item_label(label), menubar_shortcut(shortcut)]).at_loc(Location::caller())
}

/// Convenience: a [`menubar_item`] with leading icon + label.
#[track_caller]
pub fn menubar_item_with_icon(source: impl IntoIconSource, label: impl Into<String>) -> El {
    menubar_item([menubar_icon(source), menubar_item_label(label)]).at_loc(Location::caller())
}

/// Convenience: a [`menubar_item`] with icon, label, and shortcut hint.
#[track_caller]
pub fn menubar_item_with_icon_and_shortcut(
    source: impl IntoIconSource,
    label: impl Into<String>,
    shortcut: impl Into<String>,
) -> El {
    menubar_item([
        menubar_icon(source),
        menubar_item_label(label),
        menubar_shortcut(shortcut),
    ])
    .at_loc(Location::caller())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn trigger_key_matches_widget_format() {
        assert_eq!(menubar_trigger_key("app", &"file"), "app:menu:file");
        assert_eq!(
            menubar_trigger_key("workspace:7", &42u32),
            "workspace:7:menu:42"
        );
    }

    #[test]
    fn classify_event_routes_toggle_and_dismiss() {
        assert_eq!(
            classify_event(&click_event("app:menu:file"), "app"),
            Some(MenubarAction::Toggle("file")),
        );
        assert_eq!(
            classify_event(&click_event("app:menu:file:dismiss"), "app"),
            Some(MenubarAction::Dismiss("file")),
        );
        assert_eq!(classify_event(&click_event("apple:menu:file"), "app"), None);

        let mut ev = click_event("app:menu:file");
        ev.kind = UiEventKind::PointerDown;
        assert_eq!(classify_event(&ev, "app"), None);
    }

    #[test]
    fn apply_event_toggles_open_slot_and_dismisses_active_menu() {
        let mut open = None;
        assert!(apply_event(&mut open, &click_event("app:menu:file"), "app"));
        assert_eq!(open.as_deref(), Some("file"));

        assert!(apply_event(&mut open, &click_event("app:menu:file"), "app"));
        assert_eq!(open, None);

        assert!(apply_event(&mut open, &click_event("app:menu:edit"), "app"));
        assert_eq!(open.as_deref(), Some("edit"));

        assert!(apply_event(
            &mut open,
            &click_event("app:menu:file:dismiss"),
            "app"
        ));
        assert_eq!(open.as_deref(), Some("edit"));

        assert!(apply_event(
            &mut open,
            &click_event("app:menu:edit:dismiss"),
            "app"
        ));
        assert_eq!(open, None);
    }

    #[test]
    fn menubar_root_and_trigger_have_expected_shape() {
        let root = menubar([
            menubar_trigger("app", "file", "File", true),
            menubar_trigger("app", "edit", "Edit", false),
        ]);
        assert_eq!(root.kind, Kind::Custom("menubar"));
        assert_eq!(root.axis, Axis::Row);
        assert_eq!(root.height, Size::Fixed(tokens::SPACE_10));

        let file = &root.children[0];
        assert_eq!(file.kind, Kind::Custom("menubar_trigger"));
        assert_eq!(file.key.as_deref(), Some("app:menu:file"));
        assert_eq!(file.fill, Some(tokens::ACCENT));
        assert!(file.focusable);

        let edit = &root.children[1];
        assert_eq!(edit.key.as_deref(), Some("app:menu:edit"));
        assert!(edit.fill.is_none());
    }

    #[test]
    fn menubar_menu_uses_trigger_key_for_anchor_and_dismiss_route() {
        let menu = menubar_menu(
            "app",
            "file",
            [menubar_item_with_shortcut("Open", "Ctrl+O")],
        );
        assert_eq!(menu.kind, Kind::Overlay);
        assert_eq!(
            menu.children[0].key.as_deref(),
            Some("app:menu:file:dismiss")
        );

        let layer = &menu.children[1];
        let panel = &layer.children[0];
        assert_eq!(panel.kind, Kind::Custom("menubar_content"));
        assert_eq!(panel.children[0].kind, Kind::Custom("menubar_item"));
    }

    #[test]
    fn menubar_item_uses_menu_density_and_slots() {
        let item = menubar_item_with_icon_and_shortcut("copy", "Copy", "Ctrl+C");
        assert_eq!(item.kind, Kind::Custom("menubar_item"));
        assert_eq!(item.metrics_role, Some(MetricsRole::MenuItem));
        assert_eq!(
            item.focus_ring_placement,
            crate::tree::FocusRingPlacement::Inside
        );
        assert_eq!(item.axis, Axis::Row);
        assert_eq!(item.children.len(), 3);
        assert_eq!(item.children[0].width, Size::Fixed(tokens::ICON_SM));
        assert_eq!(item.children[1].text.as_deref(), Some("Copy"));
        assert_eq!(item.children[1].width, Size::Fill(1.0));
        assert_eq!(item.children[2].text.as_deref(), Some("Ctrl+C"));
    }
}
