//! Sidebar anatomy — familiar navigation groups and menu rows.
//!
//! `sidebar([...])` is the panel-surface wrapper: it bundles the
//! canonical [`SurfaceRole::Panel`] + `tokens::CARD` fill +
//! `tokens::BORDER` stroke + `tokens::SIDEBAR_WIDTH` width recipe.
//! `sidebar_header`, `sidebar_group`, `sidebar_group_label`,
//! `sidebar_menu`, `sidebar_menu_item`, `sidebar_menu_button`, and
//! `sidebar_menu_button_with_icon` are conveniences for the common
//! flat-nav case.
//!
//! When your sidebar has shapes the helpers don't cover (collapsible
//! sections, count badges on group headers, nested sub-groups, custom
//! row anatomy), **wrap your custom composition in `sidebar([...])`
//! and skip the inner helpers** — that keeps the canonical surface
//! recipe correct without forcing your row data into the helper mold.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

/// Navigation rail surface — a [`tokens::SIDEBAR_WIDTH`]-wide, full-height
/// panel column with the canonical card fill + border recipe. Wrap custom
/// nav compositions in this even when skipping the inner helpers.
#[track_caller]
pub fn sidebar<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Panel)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .width(Size::Fixed(tokens::SIDEBAR_WIDTH))
        .height(Size::Fill(1.0))
        .default_padding(tokens::SPACE_4)
        .default_gap(tokens::SPACE_4)
}

/// Header slot at the top of the rail (app name, workspace switcher).
#[track_caller]
pub fn sidebar_header<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// A titled section of the rail — typically a [`sidebar_group_label`]
/// followed by a [`sidebar_menu`].
#[track_caller]
pub fn sidebar_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// Non-interactive muted caption heading a [`sidebar_group`].
#[track_caller]
pub fn sidebar_group_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .caption()
        .semibold()
        .muted()
        .ellipsis()
        .padding(Sides {
            left: 0.0,
            right: tokens::SPACE_2,
            top: tokens::SPACE_1,
            bottom: 0.0,
        })
        .width(Size::Fill(1.0))
}

/// Vertical list of nav rows ([`sidebar_menu_button`]s or
/// [`sidebar_menu_item`]s) within a group.
#[track_caller]
pub fn sidebar_menu<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

/// Row wrapper for a single menu entry — use for custom row anatomy that
/// [`sidebar_menu_button`] doesn't cover.
#[track_caller]
pub fn sidebar_menu_item(child: impl Into<El>) -> El {
    row([child.into()])
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
}

/// Focusable nav row. `current: true` renders the selected (current-page)
/// treatment; otherwise it renders ghost. Add `.key(...)` so clicks route
/// to the app — the app owns which row is current.
#[track_caller]
pub fn sidebar_menu_button(label: impl Into<String>, current: bool) -> El {
    let button = row([sidebar_menu_label(label)])
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::ListItem)
        .focusable()
        .cursor(Cursor::Pointer)
        .fill(tokens::CARD)
        .default_radius(tokens::RADIUS_SM)
        .default_gap(tokens::SPACE_2)
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .default_height(Size::Fixed(40.0))
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .width(Size::Fill(1.0))
        .align(Align::Center);
    let styled = if current {
        button.current()
    } else {
        button.ghost()
    };
    styled.animate(Timing::SPRING_QUICK)
}

/// Like [`sidebar_menu_button`], with a small muted leading icon.
#[track_caller]
pub fn sidebar_menu_button_with_icon(
    source: impl IntoIconSource,
    label: impl Into<String>,
    current: bool,
) -> El {
    let button = row([
        icon(source)
            .icon_size(tokens::ICON_SM)
            .color(tokens::MUTED_FOREGROUND),
        sidebar_menu_label(label),
    ])
    .at_loc(Location::caller())
    .style_profile(StyleProfile::Solid)
    .metrics_role(MetricsRole::ListItem)
    .focusable()
    .cursor(Cursor::Pointer)
    .fill(tokens::CARD)
    .default_radius(tokens::RADIUS_SM)
    .default_gap(tokens::SPACE_2)
    .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
    .default_height(Size::Fixed(40.0))
    .paint_overflow(Sides::all(tokens::RING_WIDTH))
    .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
    .width(Size::Fill(1.0))
    .align(Align::Center);
    let styled = if current {
        button.current()
    } else {
        button.ghost()
    };
    styled.animate(Timing::SPRING_QUICK)
}

/// Label text inside a menu row — medium-weight, ellipsizing, fills the
/// remaining row width.
#[track_caller]
pub fn sidebar_menu_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Medium)
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_uses_standard_width_and_panel_surface() {
        let s = sidebar([sidebar_header([text("Damascene")])]);

        assert_eq!(s.width, Size::Fixed(tokens::SIDEBAR_WIDTH));
        assert_eq!(s.height, Size::Fill(1.0));
        assert_eq!(s.surface_role, SurfaceRole::Panel);
        assert_eq!(s.fill, Some(tokens::CARD));
    }

    #[test]
    fn sidebar_group_label_reads_as_noninteractive_heading() {
        let label = sidebar_group_label("Foundations");

        assert!(label.key.is_none());
        assert!(!label.focusable);
        assert_eq!(label.cursor, None);
        assert_eq!(label.padding.left, 0.0);
        assert_eq!(label.padding.right, tokens::SPACE_2);
        assert_eq!(label.padding.top, tokens::SPACE_1);
        assert_eq!(label.padding.bottom, 0.0);
    }

    #[test]
    fn sidebar_menu_button_uses_list_density_and_current_treatment() {
        let current = sidebar_menu_button_with_icon("layout-dashboard", "Overview", true);
        let inactive = sidebar_menu_button("Settings", false);

        assert_eq!(current.metrics_role, Some(MetricsRole::ListItem));
        assert_eq!(current.align, Align::Center);
        assert_eq!(current.height, Size::Fixed(40.0));
        assert_eq!(current.surface_role, SurfaceRole::Current);
        assert!(current.focusable);
        assert_eq!(current.paint_overflow, Sides::all(tokens::RING_WIDTH));
        assert_eq!(current.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
        assert!(
            current.animate_timing().is_some(),
            "current changes should ease"
        );
        assert_eq!(inactive.height, Size::Fixed(40.0));
        assert_eq!(inactive.padding, Sides::xy(tokens::SPACE_3, 0.0));
        assert_eq!(inactive.paint_overflow, Sides::all(tokens::RING_WIDTH));
        assert_eq!(inactive.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
        assert!(inactive.fill.is_none());
        assert!(
            inactive.animate_timing().is_some(),
            "inactive changes should ease"
        );
    }
}
