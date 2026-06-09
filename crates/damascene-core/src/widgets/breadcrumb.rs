//! Breadcrumb anatomy — shadcn-shaped path navigation.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::cursor::Cursor;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// Breadcrumb root (shadcn's `Breadcrumb`) — a centered inline row;
/// fill it with a [`breadcrumb_list`] or items directly.
#[track_caller]
pub fn breadcrumb<I, E>(children: I) -> El
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

/// Row of [`breadcrumb_item`]s interleaved with
/// [`breadcrumb_separator`]s (shadcn's `BreadcrumbList`).
#[track_caller]
pub fn breadcrumb_list<I, E>(children: I) -> El
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

/// Wrapper around one path segment (shadcn's `BreadcrumbItem`) —
/// typically holds a [`breadcrumb_link`] or [`breadcrumb_page`].
#[track_caller]
pub fn breadcrumb_item(child: impl Into<El>) -> El {
    row([child.into()])
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
        .align(Align::Center)
}

/// A navigable ancestor segment (shadcn's `BreadcrumbLink`) — muted
/// text with a pointer cursor. Chain `.key(...)` to receive clicks.
#[track_caller]
pub fn breadcrumb_link(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .small()
        .muted()
        .ellipsis()
        .cursor(Cursor::Pointer)
        .width(Size::Hug)
}

/// The current, non-clickable segment (shadcn's `BreadcrumbPage`) —
/// semibold foreground text.
#[track_caller]
pub fn breadcrumb_page(label: impl Into<String>) -> El {
    text(label)
        .at_loc(Location::caller())
        .small()
        .semibold()
        .ellipsis()
        .width(Size::Hug)
}

/// Muted `/` between segments (shadcn's `BreadcrumbSeparator`).
#[track_caller]
pub fn breadcrumb_separator() -> El {
    text("/")
        .at_loc(Location::caller())
        .small()
        .muted()
        .width(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breadcrumb_list_centers_inline_items() {
        let crumbs = breadcrumb_list([
            breadcrumb_item(breadcrumb_link("Projects")),
            breadcrumb_separator(),
            breadcrumb_item(breadcrumb_page("Damascene")),
        ]);

        assert_eq!(crumbs.axis, Axis::Row);
        assert_eq!(crumbs.align, Align::Center);
        assert_eq!(crumbs.gap, tokens::SPACE_2);
        assert_eq!(crumbs.children.len(), 3);
    }

    #[test]
    fn breadcrumb_link_and_page_have_distinct_treatments() {
        let link = breadcrumb_link("Projects");
        let page = breadcrumb_page("Damascene");

        assert_eq!(link.text_color, Some(tokens::MUTED_FOREGROUND));
        assert_eq!(link.cursor, Some(Cursor::Pointer));
        assert_eq!(page.font_weight, FontWeight::Semibold);
    }
}
