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

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

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

#[track_caller]
pub fn sidebar_menu_item(child: impl Into<El>) -> El {
    row([child.into()])
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
}

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
        assert!(current.animate.is_some(), "current changes should ease");
        assert_eq!(inactive.height, Size::Fixed(40.0));
        assert_eq!(inactive.padding, Sides::xy(tokens::SPACE_3, 0.0));
        assert_eq!(inactive.paint_overflow, Sides::all(tokens::RING_WIDTH));
        assert_eq!(inactive.hit_overflow, Sides::all(tokens::HIT_OVERFLOW));
        assert!(inactive.fill.is_none());
        assert!(inactive.animate.is_some(), "inactive changes should ease");
    }
}
