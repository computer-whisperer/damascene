//! Command/menu anatomy — familiar rows for command palettes and menus.
//!
//! This does not add new interaction state. It packages the row
//! conventions that shadcn examples repeat constantly: icon slot, label,
//! trailing shortcut, menu-item density, and centered inline content.

use std::panic::Location;

use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::{mono, text};
use crate::{IntoIconSource, icon};

#[track_caller]
pub fn command_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("command_group"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(0.0)
}

#[track_caller]
pub fn command_item<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("command_item"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::MenuItem)
        .surface_role(SurfaceRole::Raised)
        .focusable()
        .cursor(Cursor::Pointer)
        .children(children)
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Start)
        .fill(tokens::CARD)
        .default_radius(tokens::RADIUS_SM)
        .default_padding(Sides::xy(tokens::SPACE_2, 0.0))
        .default_gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

#[track_caller]
pub fn command_icon(source: impl IntoIconSource) -> El {
    El::new(Kind::Custom("command_icon"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .child(
            icon(source)
                .icon_size(tokens::ICON_XS)
                .color(tokens::FOREGROUND),
        )
        .align(Align::Center)
        .justify(Justify::Center)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_SM)
        .width(Size::Fixed(24.0))
        .height(Size::Fixed(24.0))
}

#[track_caller]
pub fn command_label(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Regular)
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[track_caller]
pub fn command_shortcut(shortcut: impl Into<String>) -> El {
    mono(shortcut)
        .at_loc(Location::caller())
        .caption()
        .color(tokens::MUTED_FOREGROUND)
        .width(Size::Hug)
}

#[track_caller]
pub fn command_row(
    source: impl IntoIconSource,
    label: impl Into<String>,
    shortcut: impl Into<String>,
) -> El {
    command_item([
        command_icon(source),
        command_label(label),
        command_shortcut(shortcut),
    ])
    .at_loc(Location::caller())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_item_uses_menu_density_and_centered_row() {
        let item = command_item([command_label("New branch"), command_shortcut("Ctrl+B")]);

        assert_eq!(item.kind, Kind::Custom("command_item"));
        assert_eq!(item.metrics_role, Some(MetricsRole::MenuItem));
        assert_eq!(item.axis, Axis::Row);
        assert_eq!(item.align, Align::Center);
        assert_eq!(item.gap, tokens::SPACE_2);
        assert_eq!(item.width, Size::Fill(1.0));
        assert!(item.focusable);
    }

    #[test]
    fn command_row_builds_icon_label_and_shortcut() {
        let row = command_row("git-branch", "New branch", "Ctrl+B");

        assert_eq!(row.children.len(), 3);
        assert_eq!(row.children[0].kind, Kind::Custom("command_icon"));
        assert_eq!(row.children[0].width, Size::Fixed(24.0));
        assert_eq!(row.children[1].text.as_deref(), Some("New branch"));
        assert_eq!(row.children[1].width, Size::Fill(1.0));
        assert_eq!(row.children[2].text.as_deref(), Some("Ctrl+B"));
        assert_eq!(row.children[2].text_role, TextRole::Caption);
    }
}
