//! Item anatomy — shadcn-shaped object rows with media, content, and actions.
//!
//! Use `item(...)` for clickable rows that display an object rather than
//! a command: recent repositories, files, people, projects, notifications,
//! search results, and settings shortcuts. It mirrors shadcn/ui's `Item`
//! vocabulary (`ItemGroup`, `ItemMedia`, `ItemContent`, `ItemTitle`,
//! `ItemDescription`, `ItemActions`) so app authors and LLMs have a
//! familiar name to reach for instead of building raw focusable rows.
//!
//! # Oracle mapping (`docs/NAMING_ORACLE.md`: shadcn/ui `item.tsx`)
//!
//! Every slot shadcn exports has a constructor here under the same name
//! in snake case. The full anatomy round-trips one-for-one:
//!
//! | shadcn | damascene |
//! |---|---|
//! | `ItemGroup` | [`item_group`] |
//! | `Item` | [`item`] |
//! | `ItemMedia` | [`item_media`], plus the [`item_media_icon`] shorthand |
//! | `ItemContent` | [`item_content`] |
//! | `ItemTitle` | [`item_title`] |
//! | `ItemDescription` | [`item_description`] |
//! | `ItemActions` | [`item_actions`] |
//! | `ItemHeader` | [`item_header`] |
//! | `ItemFooter` | [`item_footer`] |
//! | `ItemSeparator` | [`item_separator`] (the stock [`crate::separator`]) |
//!
//! Text roles are translated rather than copied: shadcn's `ItemTitle`
//! (`text-sm font-medium`) becomes damascene's label role at
//! [`FontWeight::Semibold`], and `ItemDescription`
//! (`text-sm text-muted-foreground`) becomes the muted caption role.
//! Both take `.ellipsis()` and fill their column, where shadcn leans on
//! `line-clamp-2` — a deliberate divergence: desktop rows are dense and
//! a wrapping description would make row heights ragged.
//!
//! # Recorded divergences
//!
//! shadcn parameterizes two of these slots with `cva` variants that
//! damascene does not model as variants:
//!
//! - `Item`'s `variant` (`default` / `outline` / `muted`) and `size`
//!   (`default` / `sm`). Damascene routes these through the general El
//!   surface vocabulary instead — `.stroke(tokens::BORDER)` for
//!   `outline`, `.fill(...)` for `muted`, `.padding(...)` /
//!   `.component_size(...)` for density — because those modifiers apply
//!   to every surface, not just items. The `selected` / `current`
//!   treatments (`.selected()` / `.current()`, which reveal the accent
//!   rail) have no shadcn counterpart at all; they come from the
//!   workbench layer.
//! - `ItemMedia`'s `variant` (`default` / `icon` / `image`).
//!   [`item_media`] **is** the `icon` variant: a 32 px (`size-8`)
//!   bordered, muted-fill tile. That is the variant every validation
//!   reference reached for. For the bare `default` variant put the icon
//!   directly in the [`item`] row; for `image`, size the media element
//!   up to shadcn's `size-10`
//!   (`.width(Size::Fixed(40.0)).height(Size::Fixed(40.0))`) and give it
//!   the thumbnail as its child — the image covers the tile fill, and
//!   the tile's border reads as the image's frame.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::separator::separator;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

/// Column of [`item`]s (shadcn's `ItemGroup`).
///
/// Carries `RING_WIDTH` of horizontal padding so the items' outside
/// focus-ring bands have slack on the left/right edges — the canonical
/// `scroll([item_group([...])])` pattern would otherwise clip those
/// bands against the scroll scissor (the `FocusRingObscured` lint). The
/// inset is invisible outside a scroll; override with `.padding(...)`.
#[track_caller]
pub fn item_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("item_group"))
        .at_loc(Location::caller())
        .role(Role::List)
        .children(children)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(tokens::SPACE_1)
        .default_padding(Sides::xy(tokens::RING_WIDTH, 0.0))
}

/// A clickable object row (shadcn's `Item`) — compose from
/// [`item_media`] / [`item_media_icon`], [`item_content`], and
/// [`item_actions`]. Key it with the object it routes to; clicks emit
/// `UiEventKind::Click` to that key. Includes a hidden left accent
/// rail that the runtime can reveal for selection treatments.
#[track_caller]
pub fn item<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let rail = El::new(Kind::Custom("item_rail"))
        .fill(tokens::PRIMARY)
        .default_radius(tokens::RADIUS_PILL)
        .width(Size::Fixed(3.0))
        .height(Size::Fill(1.0))
        .opacity(0.0)
        .animate(Timing::SPRING_QUICK);

    let content = row(children)
        .at_loc(Location::caller())
        .align(Align::Center)
        .justify(Justify::Start)
        .default_gap(tokens::SPACE_3)
        .default_padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .width(Size::Fill(1.0))
        .height(Size::Hug);

    El::new(Kind::Custom("item"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::ListItem)
        .role(Role::ListItem)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .children([rail, content])
        .axis(Axis::Overlay)
        .align(Align::Stretch)
        .justify(Justify::Start)
        .default_radius(tokens::RADIUS_MD)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .animate(Timing::SPRING_QUICK)
}

/// Full-width centered row above a group of items (shadcn's
/// `ItemHeader`).
#[track_caller]
pub fn item_header<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .align(Align::Center)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Full-width centered row below a group of items (shadcn's
/// `ItemFooter`).
#[track_caller]
pub fn item_footer<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .align(Align::Center)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Leading 32px media tile (shadcn's `ItemMedia`) — a bordered,
/// muted-fill square that centers its children (icon, avatar,
/// thumbnail).
#[track_caller]
pub fn item_media<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("item_media"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .children(children)
        .axis(Axis::Overlay)
        .align(Align::Center)
        .justify(Justify::Center)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_SM)
        .width(Size::Fixed(32.0))
        .height(Size::Fixed(32.0))
}

/// Shorthand: [`item_media`] holding a small muted icon.
#[track_caller]
pub fn item_media_icon(source: impl IntoIconSource) -> El {
    item_media([icon(source)
        .icon_size(tokens::ICON_SM)
        .color(tokens::MUTED_FOREGROUND)])
    .at_loc(Location::caller())
}

/// The row's main text column (shadcn's `ItemContent`) — typically
/// [`item_title`] over [`item_description`]; fills the remaining
/// width.
#[track_caller]
pub fn item_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(0.0)
}

/// Semibold single-line title (shadcn's `ItemTitle`), ellipsized.
#[track_caller]
pub fn item_title(title: impl Into<String>) -> El {
    text(title)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Semibold)
        .ellipsis()
        .width(Size::Fill(1.0))
}

/// Muted single-line caption under the title (shadcn's
/// `ItemDescription`), ellipsized.
#[track_caller]
pub fn item_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .caption()
        .muted()
        .ellipsis()
        .width(Size::Fill(1.0))
}

/// Trailing action cluster (shadcn's `ItemActions`) — a right-aligned
/// hugging row, typically of icon buttons or a badge.
#[track_caller]
pub fn item_actions<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .align(Align::Center)
        .justify(Justify::End)
        .default_gap(tokens::SPACE_2)
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Hairline rule between items — the stock [`crate::separator`] under
/// the anatomy's name.
#[track_caller]
pub fn item_separator() -> El {
    separator().at_loc(Location::caller())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_is_a_tactile_object_row() {
        let row = item([
            item_media_icon("folder"),
            item_content([item_title("whisper-git"), item_description("/home/example")]),
        ]);

        assert_eq!(row.kind, Kind::Custom("item"));
        assert_eq!(row.metrics_role, Some(MetricsRole::ListItem));
        assert_eq!(row.axis, Axis::Overlay);
        assert_eq!(row.align, Align::Stretch);
        assert_eq!(row.width, Size::Fill(1.0));
        assert_eq!(row.radius, crate::tree::Corners::all(tokens::RADIUS_MD));
        assert!(row.focusable);
        assert_eq!(row.cursor, Some(Cursor::Pointer));
        assert_eq!(row.fill, None, "default item rests transparent");
        assert_eq!(row.children[0].kind, Kind::Custom("item_rail"));
        assert_eq!(row.children[0].opacity, 0.0);
        assert!(
            row.children[0].animate_timing().is_some(),
            "rail opacity should ease"
        );
        assert_eq!(row.children[1].axis, Axis::Row);
        assert_eq!(
            row.children[1].padding,
            Sides::xy(tokens::SPACE_3, tokens::SPACE_2)
        );
        assert!(
            row.animate_timing().is_some(),
            "item fill/stroke should ease"
        );
    }

    #[test]
    fn item_current_and_selected_enable_accent_rail() {
        let current = item([item_title("Current")]).current();
        assert_eq!(current.surface_role, SurfaceRole::Current);
        assert_eq!(current.fill, Some(tokens::ACCENT.with_alpha_u8(24)));
        assert_eq!(current.children[0].kind, Kind::Custom("item_rail"));
        assert_eq!(current.children[0].opacity, 1.0);
        assert_eq!(current.children[0].fill, Some(tokens::PRIMARY));

        let selected = item([item_title("Selected")]).selected();
        assert_eq!(selected.surface_role, SurfaceRole::Selected);
        assert_eq!(selected.fill, Some(tokens::PRIMARY.with_alpha_u8(18)));
        assert_eq!(selected.children[0].opacity, 1.0);
        assert_eq!(selected.children[0].fill, Some(tokens::PRIMARY));
    }

    #[test]
    fn item_media_icon_uses_stock_icon_slot() {
        let media = item_media_icon("folder");

        assert_eq!(media.kind, Kind::Custom("item_media"));
        assert_eq!(media.axis, Axis::Overlay);
        assert_eq!(media.width, Size::Fixed(32.0));
        assert_eq!(media.height, Size::Fixed(32.0));
        assert_eq!(media.fill, Some(tokens::MUTED));
        assert_eq!(media.stroke, Some(tokens::BORDER));
        assert_eq!(media.children.len(), 1);
    }

    #[test]
    fn item_content_builds_title_description_stack() {
        let content = item_content([item_title("Repository"), item_description("Parent path")]);

        assert_eq!(content.axis, Axis::Column);
        assert_eq!(content.width, Size::Fill(1.0));
        assert_eq!(content.gap, 0.0);
        assert_eq!(content.children.len(), 2);
        assert_eq!(content.children[0].text.as_deref(), Some("Repository"));
        assert_eq!(content.children[0].text_role, TextRole::Label);
        assert_eq!(content.children[1].text.as_deref(), Some("Parent path"));
        assert_eq!(content.children[1].text_role, TextRole::Caption);
        assert_eq!(
            content.children[1].text_color,
            Some(tokens::MUTED_FOREGROUND)
        );
    }

    #[test]
    fn item_title_and_description_translate_tsx_roles() {
        // shadcn `ItemTitle` is `text-sm font-medium`; damascene's label
        // role at Semibold. `ItemDescription` is `text-sm
        // text-muted-foreground`; damascene's muted caption role. Both
        // ellipsize rather than `line-clamp-2` (recorded divergence).
        let title = item_title("whisper-git");
        assert_eq!(title.text.as_deref(), Some("whisper-git"));
        assert_eq!(title.text_role, TextRole::Label);
        assert_eq!(title.font_weight, FontWeight::Semibold);
        assert_eq!(title.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(title.width, Size::Fill(1.0));
        assert_eq!(
            title.text_color,
            Some(tokens::FOREGROUND),
            "the title is the row's primary text; only the description is muted"
        );

        let description = item_description("/home/example");
        assert_eq!(description.text.as_deref(), Some("/home/example"));
        assert_eq!(description.text_role, TextRole::Caption);
        assert_eq!(description.text_color, Some(tokens::MUTED_FOREGROUND));
        assert_eq!(description.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(description.width, Size::Fill(1.0));
    }

    #[test]
    fn item_actions_hugs_at_the_trailing_edge() {
        let actions = item_actions([item_title("A"), item_title("B")]);

        assert_eq!(actions.axis, Axis::Row);
        assert_eq!(actions.align, Align::Center);
        assert_eq!(actions.justify, Justify::End);
        assert_eq!(actions.gap, tokens::SPACE_2);
        assert_eq!(actions.width, Size::Hug, "must not steal content width");
        assert_eq!(actions.height, Size::Hug);
        assert_eq!(actions.children.len(), 2);
    }

    #[test]
    fn item_header_and_footer_are_full_width_rows() {
        for slot in [
            item_header([item_title("Recent")]),
            item_footer([item_title("Show all")]),
        ] {
            assert_eq!(slot.axis, Axis::Row);
            assert_eq!(slot.align, Align::Center);
            assert_eq!(slot.width, Size::Fill(1.0));
            assert_eq!(slot.height, Size::Hug);
        }
    }

    #[test]
    fn item_separator_is_the_stock_separator() {
        assert_eq!(item_separator().kind, separator().kind);
    }

    #[test]
    fn full_slot_anatomy_composes_inside_item() {
        // The slicer round-2 reference's "loaded model" block: media +
        // content(title, description) + actions, all inside one item.
        let row = item([
            item_media_icon("box"),
            item_content([
                item_title("bracket_v3.stl"),
                item_description("41.3 cm³ · scale 100 %"),
            ]),
            item_actions([item_media_icon("eye")]),
        ]);

        // children[0] is the accent rail; children[1] the content row.
        let slots = &row.children[1].children;
        assert_eq!(slots.len(), 3);
        assert_eq!(slots[0].kind, Kind::Custom("item_media"));
        assert_eq!(slots[1].axis, Axis::Column, "item_content");
        assert_eq!(slots[1].children.len(), 2);
        assert_eq!(slots[1].children[0].text_role, TextRole::Label);
        assert_eq!(slots[1].children[1].text_role, TextRole::Caption);
        assert_eq!(slots[2].justify, Justify::End, "item_actions");
        // Media hugs, content fills, actions hug — the three-column
        // reading shadcn gets from `shrink-0` / `flex-1` / default.
        assert_eq!(slots[0].width, Size::Fixed(32.0));
        assert_eq!(slots[1].width, Size::Fill(1.0));
        assert_eq!(slots[2].width, Size::Hug);
    }

    #[test]
    fn item_group_stacks_related_items() {
        let group = item_group([
            item([item_title("One")]),
            item_separator(),
            item([item_title("Two")]),
        ]);

        assert_eq!(group.kind, Kind::Custom("item_group"));
        assert_eq!(group.axis, Axis::Column);
        assert_eq!(group.width, Size::Fill(1.0));
        assert_eq!(group.gap, tokens::SPACE_1);
        assert_eq!(group.children.len(), 3);
    }
}
