//! Navigation bar — the bottom bar of a phone app: three to five
//! top-level destinations in thumb reach, each a glyph over a short
//! caption. Material 3's `NavigationBar`, iOS's tab bar, the "bottom
//! navigation" of older Material.
//!
//! It is a tab list in every way but its drawing, so it speaks
//! [`crate::widgets::tabs`]' language: the app owns the current
//! destination, items route `{key}:tab:{value}`, and
//! [`apply_event`] is `tabs::apply_event` under another path. A screen
//! can trade a [`tabs_list`](crate::widgets::tabs::tabs_list) for the
//! bar without touching its event handler.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Shell { screen: String }
//!
//! impl App for Shell {
//!     fn build(&self, cx: &BuildCx) -> El {
//!         overlays(
//!             column([
//!                 screen_body(&self.screen).height(Size::Fill(1.0)),
//!                 navigation_bar("screen", &self.screen, [
//!                     ("home", IconName::House, "Home"),
//!                     ("presets", IconName::SlidersHorizontal, "Presets"),
//!                     ("settings", IconName::Settings, "Settings"),
//!                 ])
//!                 // The fill runs under the home indicator.
//!                 .pb(cx.safe_area_bottom()),
//!             ]),
//!             [],
//!         )
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         navigation_bar::apply_event(&mut self.screen, &event, "screen", |s| {
//!             Some(s.to_string())
//!         });
//!     }
//! }
//! ```
//!
//! # Where it goes
//!
//! Full-bleed at the bottom of the root column — outside
//! [`page`](crate::widgets::page::page), whose padding would float it,
//! and inside [`overlays`](crate::overlays), so a sheet slides
//! over it. The bar hugs its height: bottom padding from
//! [`BuildCx::safe_area_bottom`](crate::event::BuildCx::safe_area_bottom)
//! lengthens it and its fill through the inset band. An app that
//! already insets its root needs none.
//!
//! # Look
//!
//! Equal-width items on a card-toned strip under one hairline. The
//! current item has a `PRIMARY` glyph and a semibold foreground
//! caption; the rest are muted. Only the glyph takes the accent: a
//! 12 px caption in a brand colour fails text contrast on most light
//! themes, where a glyph has the 3:1 floor of a graphic. A caption
//! longer than its item ellipsizes instead of spilling into the next.
//!
//! # Routed keys
//!
//! - `{key}:tab:{value}` — `Click` or `Activate` on an item
//!   ([`tab_option_key`]).
//!
//! # Anatomy
//!
//! ```text
//! navigation_bar  row  fill-width  hug  CARD  border-t  tablist
//! └── navigation_bar_item × n  column  fill-width  h=56  tab
//!     ├── row  justify=center
//!     │   └── icon  ICON_LG   PRIMARY | MUTED_FOREGROUND
//!     └── text  caption, centred, ellipsized
//!                         FOREGROUND semibold | MUTED_FOREGROUND
//! ```
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface; the first version was
//! written in an app crate against it (issue #152).

#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::icons::icon;
use crate::icons::svg::IntoIconSource;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::tabs::tab_option_key;
use crate::widgets::text::text;

pub use crate::widgets::tabs::{TabsAction, apply_event, classify_event};

/// Height of a navigation bar's items in logical px — Material 2's
/// bottom navigation, and a comfortable touch target on both
/// platforms.
pub const NAVIGATION_BAR_HEIGHT: f32 = 56.0;

/// The bottom navigation bar: one [`navigation_bar_item`] per
/// destination, equal widths, the current one accented.
///
/// `current` is the current destination — formatted via
/// [`std::fmt::Display`] and compared against each item's `value`.
/// `items` is an iterable of `(value, icon, label)`; the icon is
/// anything [`icon`] takes (an [`IconName`], a name string, an app
/// SVG).
///
/// `key` is the routing namespace; items route `{key}:tab:{value}`,
/// folded with [`apply_event`]. The bar itself is not keyed.
#[track_caller]
pub fn navigation_bar<I, V, S, L>(
    key: impl Into<String>,
    current: &impl std::fmt::Display,
    items: I,
) -> El
where
    I: IntoIterator<Item = (V, S, L)>,
    V: std::fmt::Display,
    S: IntoIconSource,
    L: Into<String>,
{
    // Capture once so lint findings on the items blame the caller;
    // see `tabs_list`.
    let caller = Location::caller();
    let key = key.into();
    let current = current.to_string();
    let items: Vec<El> = items
        .into_iter()
        .map(|(value, source, label)| {
            let selected = value.to_string() == current;
            navigation_bar_item(&key, value, source, label, selected).at_loc(caller)
        })
        .collect();
    navigation_bar_container(caller, items)
}

/// The bar for items already built with [`navigation_bar_item`] — for
/// a bar whose items carry something the triple cannot (a badge over
/// one glyph, a per-item tooltip).
#[track_caller]
pub fn navigation_bar_from_items<I, E>(items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    navigation_bar_container(Location::caller(), items)
}

fn navigation_bar_container<I, E>(caller: &'static Location<'static>, items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("navigation_bar"))
        .at_loc(caller)
        .role(Role::TabList)
        .axis(Axis::Row)
        .gap(tokens::SPACE_0)
        .align(Align::Start)
        // ARIA tablist pattern (horizontal): Left / Right move between
        // items.
        .arrow_nav(ArrowNav::Horizontal)
        .children(items)
        .fill(tokens::CARD)
        .border_t()
        .width(Size::Fill(1.0))
}

/// One destination in a [`navigation_bar`]: a glyph over a caption.
/// Apps let [`navigation_bar`] build these; reach for it with
/// [`navigation_bar_from_items`] to decorate one.
///
/// `list_key` is the bar's key — the routed key on the item is
/// `{list_key}:tab:{value}`. `selected` marks the current
/// destination.
#[track_caller]
pub fn navigation_bar_item(
    list_key: &str,
    value: impl std::fmt::Display,
    source: impl IntoIconSource,
    label: impl Into<String>,
    selected: bool,
) -> El {
    let (glyph_color, label_color) = if selected {
        (tokens::PRIMARY, tokens::FOREGROUND)
    } else {
        (tokens::MUTED_FOREGROUND, tokens::MUTED_FOREGROUND)
    };
    let caption = text(label)
        .caption()
        .ellipsis()
        .text_align(TextAlign::Center)
        .text_color(label_color);
    let caption = if selected {
        caption.semibold()
    } else {
        caption
    };
    El::new(Kind::Custom("navigation_bar_item"))
        .at_loc(Location::caller())
        .cursor(Cursor::Pointer)
        .style_profile(StyleProfile::Surface)
        .role(Role::Tab)
        .aria_selected(selected)
        .focusable()
        // Items abut each other and the window's edges, so there is
        // no room outside for a ring.
        .focus_ring_inside()
        .key(tab_option_key(list_key, &value))
        .axis(Axis::Column)
        // Stretch, not Center: a centred caption is not held to the
        // item's width and a long one would spill over its neighbour.
        // The glyph gets its own centring row instead.
        .align(Align::Stretch)
        .justify(Justify::Center)
        .gap(tokens::SPACE_1 * 0.5)
        .padding(Sides::xy(tokens::SPACE_1, 0.0))
        .child(
            row([icon(source)
                .default_icon_size(tokens::ICON_LG)
                .color(glyph_color)])
            .justify(Justify::Center),
        )
        .child(caption)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(NAVIGATION_BAR_HEIGHT))
        .animate(Timing::SPRING_QUICK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::UiEvent;

    const SCREENS: [(&str, IconName, &str); 4] = [
        ("home", IconName::House, "Home"),
        ("presets", IconName::SlidersHorizontal, "Presets"),
        ("history", IconName::Clock, "History"),
        ("settings", IconName::Settings, "Settings"),
    ];

    fn phone_shell(bar: El) -> El {
        crate::overlays(
            column([
                column([text("body")])
                    .padding(tokens::SPACE_4)
                    .height(Size::Fill(1.0)),
                bar,
            ]),
            Vec::<Option<El>>::new(),
        )
    }

    fn find<'a>(el: &'a El, key: &str) -> &'a El {
        fn walk<'a>(el: &'a El, key: &str) -> Option<&'a El> {
            if el.key.as_deref() == Some(key) {
                return Some(el);
            }
            el.children.iter().find_map(|c| walk(c, key))
        }
        walk(el, key).unwrap_or_else(|| panic!("no node keyed {key}"))
    }

    #[test]
    fn items_share_the_width_and_the_bar_sits_flush_at_the_bottom() {
        let mut root = phone_shell(navigation_bar("screen", &"presets", SCREENS));
        let viewport = Rect::new(0.0, 0.0, 360.0, 640.0);
        let bundle = crate::render_bundle(&mut root, viewport);
        assert!(bundle.lint.findings.is_empty(), "{}", bundle.lint.text());

        let rects: Vec<Rect> = SCREENS
            .iter()
            .map(|(value, _, _)| find(&root, &format!("screen:tab:{value}")).computed_rect)
            .collect();
        for rect in &rects {
            assert_eq!(rect.w, 90.0, "four items split 360 px evenly");
            assert_eq!(rect.h, NAVIGATION_BAR_HEIGHT);
            assert_eq!(rect.bottom(), viewport.h, "flush with the window's bottom");
        }
        assert_eq!(rects[0].x, 0.0);
        assert_eq!(rects[3].right(), viewport.w);
    }

    #[test]
    fn only_the_current_item_is_selected_and_accented() {
        let bar = navigation_bar("screen", &"presets", SCREENS);
        let selected: Vec<bool> = bar
            .children
            .iter()
            .map(|item| item.a11y.as_deref().and_then(|a| a.selected) == Some(true))
            .collect();
        assert_eq!(selected, [false, true, false, false]);

        let glyph = |item: &El| item.children[0].children[0].text_color;
        let caption = |item: &El| item.children[1].text_color;
        assert_eq!(glyph(&bar.children[1]), Some(tokens::PRIMARY));
        assert_eq!(
            caption(&bar.children[1]),
            Some(tokens::FOREGROUND),
            "the accent stays off the caption, where it would fail text contrast"
        );
        assert_eq!(glyph(&bar.children[0]), Some(tokens::MUTED_FOREGROUND));
        assert_eq!(caption(&bar.children[0]), Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn a_long_caption_ellipsizes_inside_its_item() {
        // The app-side version centred its column, which let a long
        // translation run past the item and under the next one
        // (issue #152, found in Danish).
        let mut root = phone_shell(navigation_bar(
            "screen",
            &"home",
            [
                ("home", IconName::House, "Hjem"),
                (
                    "presets",
                    IconName::SlidersHorizontal,
                    "Forudindstillinger og mere",
                ),
                ("history", IconName::Clock, "Historik"),
                ("settings", IconName::Settings, "Indstillinger"),
                ("help", IconName::CircleQuestionMark, "Hjælp"),
            ],
        ));
        let bundle = crate::render_bundle(&mut root, Rect::new(0.0, 0.0, 360.0, 640.0));
        assert!(bundle.lint.findings.is_empty(), "{}", bundle.lint.text());
        let item = find(&root, "screen:tab:presets");
        let caption = &item.children[1];
        assert!(caption.computed_rect.x >= item.computed_rect.x);
        assert!(caption.computed_rect.right() <= item.computed_rect.right());
    }

    #[test]
    fn safe_area_padding_lengthens_the_bar_under_the_items() {
        let inset = 34.0;
        let mut root = phone_shell(navigation_bar("screen", &"home", SCREENS).pb(inset));
        let viewport = Rect::new(0.0, 0.0, 360.0, 640.0);
        let mut state = crate::state::UiState::new();
        crate::layout::layout(&mut root, &mut state, viewport);
        let item = find(&root, "screen:tab:home").computed_rect;
        assert_eq!(item.h, NAVIGATION_BAR_HEIGHT);
        assert_eq!(
            item.bottom(),
            viewport.h - inset,
            "items stay above the home indicator"
        );
    }

    #[test]
    fn a_tabs_handler_folds_the_bar_unchanged() {
        let mut screen = String::from("home");
        let tap = UiEvent::synthetic_click("screen:tab:settings");
        assert!(crate::widgets::tabs::apply_event(
            &mut screen,
            &tap,
            "screen",
            |s| Some(s.to_string())
        ));
        assert_eq!(screen, "settings");
        // And under the bar's own path.
        let tap = UiEvent::synthetic_click("screen:tab:history");
        assert!(apply_event(&mut screen, &tap, "screen", |s| Some(
            s.to_string()
        )));
        assert_eq!(screen, "history");
    }
}
