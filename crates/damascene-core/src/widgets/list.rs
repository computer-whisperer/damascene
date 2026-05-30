//! Lists — bulleted and numbered.
//!
//! Each item lays out as an overlay (`stack`) of a fixed-width marker
//! slot and a content column whose left padding clears that slot — a
//! hanging indent where wrapped content aligns under itself rather than
//! under the marker. The overlay axis propagates available width into
//! intrinsic measurement, so a wrapping paragraph inside a list item
//! sizes correctly without the row-axis chicken-and-egg between Hug
//! cross-extent and child wrap-width.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! bullet_list([
//!     "Simple item",
//!     text_runs([text("Rich "), text("content").bold()]),
//!     column([
//!         paragraph("Item with a nested list"),
//!         bullet_list(["nested one", "nested two"]),
//!     ]),
//! ])
//!
//! numbered_list_from(42, ["Custom start", "Keeps counting"]);
//! task_list([(true, "Done"), (false, "Todo")]);
//! ```
//!
//! Plain `text(...)` items (including bare `&str` items via the
//! `From<&str>` impl) are normalized to `wrap_text() + Size::Fill(1.0)`
//! so the typical markdown-shaped item — a flowing line of prose —
//! wraps within the item's content column. Composite items
//! (`text_runs`, `column`, …) are passed through unchanged.

use std::panic::Location;

use crate::icons::icon;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::text::metrics::line_width;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// Marker glyph used for bulleted lists. Bullet point (U+2022).
const BULLET_GLYPH: &str = "\u{2022}";

/// Gap between the marker slot and the content column. Tuned to match
/// shadcn's typography list rhythm — small enough that markers feel
/// attached, wide enough that they don't crowd the content.
const MARKER_GAP: f32 = tokens::SPACE_2;

/// Vertical gap between successive items. Authors override at the call
/// site with `.gap(...)` for tighter or looser lists.
const ITEM_GAP: f32 = tokens::SPACE_1;

#[track_caller]
pub fn bullet_list<I, E>(items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let loc = Location::caller();
    let marker_slot_width = bullet_marker_width();

    let item_els: Vec<El> = items
        .into_iter()
        .map(|item| {
            let marker = text(BULLET_GLYPH)
                .at_loc(loc)
                .text_color(tokens::MUTED_FOREGROUND)
                .center_text()
                .width(Size::Fixed(marker_slot_width));
            list_item(marker, marker_slot_width, item.into(), loc)
        })
        .collect();

    column(item_els)
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(ITEM_GAP)
}

#[track_caller]
pub fn numbered_list<I, E>(items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    numbered_list_from(1, items)
}

#[track_caller]
pub fn numbered_list_from<I, E>(start: u64, items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let loc = Location::caller();
    let items_vec: Vec<El> = items.into_iter().map(Into::into).collect();
    let marker_slot_width = numbered_marker_width(start, items_vec.len());

    let item_els: Vec<El> = items_vec
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let n = start.saturating_add(i as u64);
            let marker = text(format!("{n}."))
                .at_loc(loc)
                .text_color(tokens::MUTED_FOREGROUND)
                .end_text()
                .width(Size::Fixed(marker_slot_width));
            list_item(marker, marker_slot_width, item, loc)
        })
        .collect();

    column(item_els)
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(ITEM_GAP)
}

#[track_caller]
pub fn task_list<I, E>(items: I) -> El
where
    I: IntoIterator<Item = (bool, E)>,
    E: Into<El>,
{
    let loc = Location::caller();
    let marker_slot_width = checkbox_marker_width();

    let item_els: Vec<El> = items
        .into_iter()
        .map(|(checked, item)| {
            let marker = task_marker(checked).at_loc(loc);
            list_item(marker, marker_slot_width, item.into(), loc)
        })
        .collect();

    column(item_els)
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(ITEM_GAP)
}

fn list_item(
    marker: El,
    marker_slot_width: f32,
    content: El,
    loc: &'static std::panic::Location<'static>,
) -> El {
    let content_indent = marker_slot_width + MARKER_GAP;
    let body = column([normalize_item_content(content)])
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(Sides {
            left: content_indent,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        });
    let marker_slot = column([marker])
        .at_loc(loc)
        .width(Size::Fixed(marker_slot_width))
        .height(Size::Hug);
    stack([marker_slot, body])
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Plain `Kind::Text` items (typically from a bare `&str` via the
/// `From<&str>` impl, or from the user calling `text(...)`) come in
/// hugged + nowrap. Within a list item that overflows the row width,
/// so we flip them to wrap inside the content column.
fn normalize_item_content(content: El) -> El {
    if matches!(content.kind, Kind::Text) {
        return content.wrap_text().fill_width();
    }
    content
}

fn bullet_marker_width() -> f32 {
    let glyph_w = line_width(
        BULLET_GLYPH,
        tokens::TEXT_BASE.size,
        FontWeight::Regular,
        false,
    );
    // Round up so wrapped content always lines up to a stable column.
    (glyph_w + 4.0).ceil()
}

fn numbered_marker_width(start: u64, count: usize) -> f32 {
    // The widest marker is the largest number plus a period — e.g.
    // `12.` for a 12-item list, `100.` for a 100-item list.
    let widest_num = if count == 0 {
        start
    } else {
        start.saturating_add(count.saturating_sub(1) as u64)
    };
    let sample = format!("{}.", widest_num);
    let w = line_width(&sample, tokens::TEXT_BASE.size, FontWeight::Regular, false);
    (w + 2.0).ceil()
}

fn checkbox_marker_width() -> f32 {
    crate::widgets::checkbox::SIZE
}

fn task_marker(checked: bool) -> El {
    let (fill, stroke) = if checked {
        (tokens::PRIMARY, tokens::PRIMARY)
    } else {
        (tokens::CARD, tokens::INPUT)
    };
    let check_opacity = if checked { 1.0 } else { 0.0 };

    El::new(Kind::Custom("task_marker"))
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::ChoiceControl)
        .axis(Axis::Overlay)
        .align(Align::Center)
        .justify(Justify::Center)
        .default_width(Size::Fixed(crate::widgets::checkbox::SIZE))
        .default_height(Size::Fixed(crate::widgets::checkbox::SIZE))
        .default_radius(tokens::RADIUS_SM)
        .fill(fill)
        .stroke(stroke)
        .child(
            icon("check")
                .icon_size(12.0)
                .icon_stroke_width(2.5)
                .color(tokens::PRIMARY_FOREGROUND)
                .opacity(check_opacity),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullet_list_overlays_marker_slot_and_content_per_item() {
        let l = bullet_list(["one", "two", "three"]);

        assert_eq!(l.kind, Kind::Group);
        assert_eq!(l.axis, Axis::Column);
        assert_eq!(l.width, Size::Fill(1.0));
        assert_eq!(l.children.len(), 3);

        for item in &l.children {
            assert_eq!(item.kind, Kind::Group);
            assert_eq!(item.axis, Axis::Overlay);
            assert_eq!(item.children.len(), 2);

            let marker_slot = &item.children[0];
            assert!(matches!(marker_slot.width, Size::Fixed(_)));
            let marker = &marker_slot.children[0];
            assert_eq!(marker.text.as_deref(), Some(BULLET_GLYPH));
            assert_eq!(marker.text_color, Some(tokens::MUTED_FOREGROUND));

            let body = &item.children[1];
            assert_eq!(body.kind, Kind::Group);
            assert_eq!(body.axis, Axis::Column);
            assert_eq!(body.width, Size::Fill(1.0));
            // Padding-left clears the marker slot plus the gap.
            assert!(body.padding.left > MARKER_GAP);
            assert_eq!(body.children.len(), 1);
        }
    }

    #[test]
    fn numbered_list_markers_count_from_one_and_right_align() {
        let l = numbered_list(["alpha", "beta", "gamma"]);

        let labels: Vec<&str> = l
            .children
            .iter()
            .map(|item| {
                let marker_slot = &item.children[0];
                marker_slot.children[0].text.as_deref().unwrap_or("")
            })
            .collect();
        assert_eq!(labels, vec!["1.", "2.", "3."]);

        for item in &l.children {
            let marker_slot = &item.children[0];
            let marker = &marker_slot.children[0];
            assert_eq!(marker.text_align, TextAlign::End);
        }
    }

    #[test]
    fn numbered_list_from_uses_custom_start() {
        let l = numbered_list_from(42, ["alpha", "beta"]);

        let labels: Vec<&str> = l
            .children
            .iter()
            .map(|item| {
                let marker_slot = &item.children[0];
                marker_slot.children[0].text.as_deref().unwrap_or("")
            })
            .collect();
        assert_eq!(labels, vec!["42.", "43."]);
    }

    #[test]
    fn numbered_marker_width_grows_with_count() {
        let small = numbered_marker_width(1, 9);
        let large = numbered_marker_width(1, 99);
        let huge = numbered_marker_width(1, 999);
        assert!(large > small, "{large} <= {small}");
        assert!(huge > large, "{huge} <= {large}");
    }

    #[test]
    fn task_list_uses_static_checkbox_markers() {
        let l = task_list([(true, "done"), (false, "todo")]);
        assert_eq!(l.children.len(), 2);

        let checked = &l.children[0].children[0].children[0];
        let unchecked = &l.children[1].children[0].children[0];
        assert_eq!(checked.kind, Kind::Custom("task_marker"));
        assert_eq!(unchecked.kind, Kind::Custom("task_marker"));
        assert_eq!(checked.fill, Some(tokens::PRIMARY));
        assert_eq!(unchecked.fill, Some(tokens::CARD));
        assert!(!checked.focusable);
        assert!(!unchecked.focusable);
    }

    #[test]
    fn plain_text_items_are_wrapped_inside_the_content_column() {
        let l = bullet_list(["This item is plain text and should wrap to fit."]);
        let body = &l.children[0].children[1];
        let inner = &body.children[0];
        assert_eq!(inner.kind, Kind::Text);
        assert_eq!(inner.text_wrap, TextWrap::Wrap);
        assert_eq!(inner.width, Size::Fill(1.0));
    }

    #[test]
    fn composite_items_pass_through_unchanged() {
        let l = bullet_list(vec![text_runs([text("rich"), text(" runs")])]);
        let body = &l.children[0].children[1];
        let inner = &body.children[0];
        assert_eq!(inner.kind, Kind::Inlines);
    }
}
