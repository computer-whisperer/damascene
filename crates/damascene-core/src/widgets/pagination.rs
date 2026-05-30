//! Pagination anatomy — compact page-navigation controls.

use std::panic::Location;

use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::{button, button_with_icon};
use crate::widgets::text::text;

#[track_caller]
pub fn pagination<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
        .align(Align::Center)
}

#[track_caller]
pub fn pagination_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
        .align(Align::Center)
}

#[track_caller]
pub fn pagination_item(child: impl Into<El>) -> El {
    row([child.into()])
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
        .align(Align::Center)
}

#[track_caller]
pub fn pagination_link(label: impl Into<String>, current: bool) -> El {
    let link = button(label)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::Button)
        .width(Size::Fixed(tokens::CONTROL_HEIGHT))
        .height(Size::Fixed(tokens::CONTROL_HEIGHT));
    if current {
        link.secondary()
    } else {
        link.ghost()
    }
}

#[track_caller]
pub fn pagination_previous() -> El {
    button_with_icon("chevron-left", "Previous")
        .at_loc(Location::caller())
        .ghost()
}

#[track_caller]
pub fn pagination_next() -> El {
    button_with_icon("chevron-right", "Next")
        .at_loc(Location::caller())
        .ghost()
}

#[track_caller]
pub fn pagination_ellipsis() -> El {
    text("...")
        .at_loc(Location::caller())
        .muted()
        .width(Size::Fixed(tokens::CONTROL_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_content_centers_items() {
        let pages = pagination_content([
            pagination_item(pagination_previous()),
            pagination_item(pagination_link("1", true)),
            pagination_item(pagination_ellipsis()),
            pagination_item(pagination_next()),
        ]);

        assert_eq!(pages.axis, Axis::Row);
        assert_eq!(pages.align, Align::Center);
        assert_eq!(pages.gap, tokens::SPACE_1);
        assert_eq!(pages.children.len(), 4);
    }

    #[test]
    fn pagination_link_has_fixed_square_box() {
        let current = pagination_link("2", true);

        assert_eq!(current.width, Size::Fixed(tokens::CONTROL_HEIGHT));
        assert_eq!(current.height, Size::Fixed(tokens::CONTROL_HEIGHT));
        assert_eq!(current.metrics_role, Some(MetricsRole::Button));
    }
}
