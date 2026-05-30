//! Sheet anatomy — edge-attached dialog surfaces.
//!
//! This mirrors shadcn's Sheet shape without adding a new runtime
//! primitive: a sheet is an overlay, a dismiss scrim, and a panel aligned
//! to one viewport edge.

use std::panic::Location;

use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::overlay::{overlay, scrim};
use crate::widgets::text::{h3, text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// A blocking edge-attached sheet with a keyed dismiss scrim.
///
/// Keys:
/// - `{key}:dismiss` — emitted when the user clicks outside the sheet.
#[track_caller]
pub fn sheet<I, E>(key: impl Into<String>, side: SheetSide, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let key = key.into();
    let layer = overlay([
        scrim(format!("{key}:dismiss")),
        sheet_content(side, body).block_pointer(),
    ]);

    match side {
        SheetSide::Left => layer.align(Align::Start).justify(Justify::Center),
        SheetSide::Right => layer.align(Align::End).justify(Justify::Center),
        SheetSide::Top => layer.align(Align::Center).justify(Justify::Start),
        SheetSide::Bottom => layer.align(Align::Center).justify(Justify::End),
    }
}

#[track_caller]
pub fn sheet_content<I, E>(side: SheetSide, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut content = El::new(Kind::Custom("sheet_content"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Popover)
        .children(children)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .default_radius(0.0)
        .shadow(tokens::SHADOW_LG)
        .default_padding(tokens::SPACE_4)
        .default_gap(tokens::SPACE_4)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .clip();

    match side {
        SheetSide::Left | SheetSide::Right => {
            content.width = Size::Fixed(360.0);
            content.height = Size::Fill(1.0);
        }
        SheetSide::Top | SheetSide::Bottom => {
            content.width = Size::Fill(1.0);
            content.height = Size::Hug;
        }
    }

    content
}

#[track_caller]
pub fn sheet_header<I, E>(children: I) -> El
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
pub fn sheet_footer<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .justify(Justify::End)
}

#[track_caller]
pub fn sheet_title(title: impl Into<String>) -> El {
    h3(title)
        .at_loc(Location::caller())
        .line_height(tokens::TEXT_BASE.size)
}

#[track_caller]
pub fn sheet_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .fill_width()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_aligns_layer_by_side() {
        let right = sheet("settings", SheetSide::Right, [sheet_title("Settings")]);
        assert_eq!(right.align, Align::End);
        assert_eq!(right.justify, Justify::Center);
        assert_eq!(right.children[0].key.as_deref(), Some("settings:dismiss"));
        assert!(right.children[1].block_pointer);

        let bottom = sheet("activity", SheetSide::Bottom, [sheet_title("Activity")]);
        assert_eq!(bottom.align, Align::Center);
        assert_eq!(bottom.justify, Justify::End);
    }

    #[test]
    fn vertical_sheets_fill_height_and_horizontal_sheets_fill_width() {
        let side = sheet_content(SheetSide::Right, [sheet_title("Settings")]);
        assert_eq!(side.width, Size::Fixed(360.0));
        assert_eq!(side.height, Size::Fill(1.0));
        assert_eq!(side.radius, crate::tree::Corners::ZERO);

        let bottom = sheet_content(SheetSide::Bottom, [sheet_title("Activity")]);
        assert_eq!(bottom.width, Size::Fill(1.0));
        assert_eq!(bottom.height, Size::Hug);
    }
}
