//! Dropdown-menu anatomy — richer shadcn-shaped menu content.
//!
//! The older [`crate::menu_item`] helper stays as a one-label shortcut.
//! This module exposes the longer names agents see in shadcn examples:
//! content, group, label, separator, item, icon, and shortcut.
//!
//! This is the **action menu** family — items fire side-effects when
//! activated (open a file, copy, delete). For a value-bound picker
//! (model, timezone, enum field) reach for [`crate::widgets::select`]
//! instead: it owns a `(value, open)` state shape with
//! [`crate::widgets::select::apply_event`], same convention as `tabs`,
//! `text_input`, and `switch`.

use std::panic::Location;

use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::popover::{Anchor, MenuDensity, apply_menu_density, popover};
use crate::widgets::separator::separator;
use crate::widgets::text::{mono, text};
use crate::{IntoIconSource, icon};

#[track_caller]
pub fn dropdown_menu(
    key: impl Into<String>,
    trigger_key: impl Into<String>,
    children: impl IntoIterator<Item = impl Into<El>>,
) -> El {
    popover(
        key,
        Anchor::below_key(trigger_key),
        dropdown_menu_content(children),
    )
}

#[track_caller]
pub fn dropdown_menu_with_density(
    key: impl Into<String>,
    trigger_key: impl Into<String>,
    density: MenuDensity,
    children: impl IntoIterator<Item = impl Into<El>>,
) -> El {
    popover(
        key,
        Anchor::below_key(trigger_key),
        dropdown_menu_content_with_density(children, density),
    )
}

#[track_caller]
pub fn dropdown_menu_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("dropdown_menu_content"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Popover)
        .arrow_nav_siblings()
        .children(children)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .radius(0.0)
        .shadow(tokens::SHADOW_MD)
        .padding(Sides::zero())
        .gap(0.0)
        .width(Size::Hug)
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}

#[track_caller]
pub fn dropdown_menu_content_with_density<I, E>(children: I, density: MenuDensity) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let children = children
        .into_iter()
        .map(Into::into)
        .map(|item| apply_menu_density(item, density));
    dropdown_menu_content(children).at_loc(Location::caller())
}

#[track_caller]
pub fn dropdown_menu_group<I, E>(children: I) -> El
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

#[track_caller]
pub fn dropdown_menu_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .caption()
        .semibold()
        .color(tokens::MUTED_FOREGROUND)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .width(Size::Fill(1.0))
}

#[track_caller]
pub fn dropdown_menu_separator() -> El {
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

#[track_caller]
pub fn dropdown_menu_item<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("dropdown_menu_item"))
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

#[track_caller]
pub fn dropdown_menu_item_with_density<I, E>(children: I, density: MenuDensity) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    apply_menu_density(dropdown_menu_item(children), density).at_loc(Location::caller())
}

#[track_caller]
pub fn dropdown_menu_item_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Regular)
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[track_caller]
pub fn dropdown_menu_icon(source: impl IntoIconSource) -> El {
    icon(source)
        .at_loc(Location::caller())
        .icon_size(tokens::ICON_SM)
        .color(tokens::MUTED_FOREGROUND)
}

#[track_caller]
pub fn dropdown_menu_shortcut(shortcut: impl Into<String>) -> El {
    mono(shortcut)
        .at_loc(Location::caller())
        .caption()
        .color(tokens::MUTED_FOREGROUND)
        .width(Size::Hug)
}

#[track_caller]
pub fn dropdown_menu_item_with_shortcut(
    label: impl Into<String>,
    shortcut: impl Into<String>,
) -> El {
    dropdown_menu_item([
        dropdown_menu_item_label(label),
        dropdown_menu_shortcut(shortcut),
    ])
    .at_loc(Location::caller())
}

#[track_caller]
pub fn dropdown_menu_item_with_icon(source: impl IntoIconSource, label: impl Into<String>) -> El {
    dropdown_menu_item([dropdown_menu_icon(source), dropdown_menu_item_label(label)])
        .at_loc(Location::caller())
}

#[track_caller]
pub fn dropdown_menu_item_with_icon_and_shortcut(
    source: impl IntoIconSource,
    label: impl Into<String>,
    shortcut: impl Into<String>,
) -> El {
    dropdown_menu_item([
        dropdown_menu_icon(source),
        dropdown_menu_item_label(label),
        dropdown_menu_shortcut(shortcut),
    ])
    .at_loc(Location::caller())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_menu_wraps_content_in_popover() {
        let menu = dropdown_menu(
            "actions",
            "actions-trigger",
            [dropdown_menu_item_with_shortcut("Copy", "Ctrl+C")],
        );

        assert_eq!(menu.kind, Kind::Overlay);
        assert_eq!(menu.children[0].key.as_deref(), Some("actions:dismiss"));
        assert_eq!(
            menu.children[1].children[0].kind,
            Kind::Custom("dropdown_menu_content")
        );
    }

    #[test]
    fn dropdown_menu_item_uses_menu_density_and_row_alignment() {
        let item = dropdown_menu_item_with_icon_and_shortcut("copy", "Copy", "Ctrl+C");

        assert_eq!(item.kind, Kind::Custom("dropdown_menu_item"));
        assert_eq!(item.metrics_role, Some(MetricsRole::MenuItem));
        assert_eq!(
            item.focus_ring_placement,
            crate::tree::FocusRingPlacement::Inside
        );
        assert_eq!(item.axis, Axis::Row);
        assert_eq!(item.align, Align::Center);
        assert_eq!(item.children.len(), 3);
        assert_eq!(item.children[0].width, Size::Fixed(tokens::ICON_SM));
        assert_eq!(item.children[1].text.as_deref(), Some("Copy"));
        assert_eq!(item.children[1].width, Size::Fill(1.0));
        assert_eq!(item.children[2].text.as_deref(), Some("Ctrl+C"));
    }

    #[test]
    fn dropdown_menu_with_touch_density_expands_action_rows() {
        let menu = dropdown_menu_with_density(
            "actions",
            "actions-trigger",
            MenuDensity::Touch,
            [
                dropdown_menu_label("Workspace"),
                dropdown_menu_item_with_shortcut("Copy", "Ctrl+C"),
            ],
        );
        let item = &menu.children[1].children[0].children[1];

        assert_eq!(item.kind, Kind::Custom("dropdown_menu_item"));
        assert_eq!(
            item.height,
            Size::Fixed(crate::widgets::popover::TOUCH_MENU_ITEM_HEIGHT)
        );
        assert_eq!(item.padding.left, tokens::SPACE_4);
        assert_eq!(item.padding.right, tokens::SPACE_4);
    }

    #[test]
    fn dropdown_menu_item_with_density_expands_single_row() {
        let item = dropdown_menu_item_with_density(
            [dropdown_menu_icon("copy"), dropdown_menu_item_label("Copy")],
            MenuDensity::Touch,
        );

        assert_eq!(
            item.height,
            Size::Fixed(crate::widgets::popover::TOUCH_MENU_ITEM_HEIGHT)
        );
        assert_eq!(item.padding.left, tokens::SPACE_4);
    }

    #[test]
    fn dropdown_menu_label_and_separator_are_structural_items() {
        let label = dropdown_menu_label("Actions");
        let sep = dropdown_menu_separator();

        assert_eq!(label.text.as_deref(), Some("Actions"));
        assert_eq!(label.text_role, TextRole::Caption);
        assert_eq!(sep.kind, Kind::Group);
        assert_eq!(sep.children[0].kind, Kind::Divider);
        assert_eq!(sep.padding.top, tokens::SPACE_1);
    }
}
