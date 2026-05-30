//! Toolbar anatomy — compact action rows for page and table controls.

use std::panic::Location;

use crate::tokens;
use crate::tree::*;
use crate::widgets::text::{h3, text};

#[track_caller]
pub fn toolbar<I, E>(children: I) -> El
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
}

#[track_caller]
pub fn toolbar_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
        .gap(tokens::SPACE_2)
        .align(Align::Center)
}

#[track_caller]
pub fn toolbar_title(title: impl Into<String>) -> El {
    h3(title)
        .at_loc(Location::caller())
        .line_height(tokens::TEXT_BASE.size)
}

#[track_caller]
pub fn toolbar_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .ellipsis()
        .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::button;

    #[test]
    fn toolbar_centers_action_rows() {
        let bar = toolbar([toolbar_title("Documents"), spacer(), button("Upload")]);

        assert_eq!(bar.axis, Axis::Row);
        assert_eq!(bar.align, Align::Center);
        assert_eq!(bar.width, Size::Fill(1.0));
        assert_eq!(bar.gap, tokens::SPACE_2);
    }

    #[test]
    fn toolbar_group_hugs_inline_actions() {
        let group = toolbar_group([button("Filter"), button("Export")]);

        assert_eq!(group.width, Size::Hug);
        assert_eq!(group.align, Align::Center);
    }
}
