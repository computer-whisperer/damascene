//! Rating — a row of stars for scoring something from one to `max`,
//! and a read-only twin for showing a score.
//!
//! Neither shadcn nor HTML has one; the shape follows the WAI-ARIA
//! *rating radio group*: the row is a `radiogroup`, each star a
//! `radio` named "3 stars", arrows move among them, Enter or Space
//! picks. Every star up to the value is lit, and only the value's own
//! star is the checked radio.
//!
//! The app owns the value as a `u8`, where `0` means unrated — the
//! controlled pattern of [`crate::widgets::radio`] and
//! [`crate::widgets::toggle`].
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Book { stars: u8 }
//!
//! impl App for Book {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         field("Rating", rating("stars", self.stars))
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         rating::apply_event(&mut self.stars, &event, "stars");
//!     }
//! }
//! ```
//!
//! Picking the star that is already the value clears the rating to
//! `0`, which is the only way to take a rating back without a second
//! control. An app that must always hold a rating matches on
//! [`classify_event`] instead and assigns the star it reports.
//!
//! # Size and colour
//!
//! The stars are ghost icon buttons, so the row takes a size rung and
//! a status the way a button does, and both reach every star:
//! `rating(..).xsmall()` for an inspector row, `rating(..).warning()`
//! for amber stars. Unstyled, lit stars are filled in the foreground
//! colour and the rest are muted outlines.
//!
//! [`rating_display`] is the score shown rather than set — in a list
//! row, a card, a table cell. It is one image to assistive technology
//! ("3 of 5 stars"), takes no key and no focus, and colours the same
//! way.
//!
//! # Routed keys
//!
//! - `{key}:rating:{n}` — `Click` or `Activate` on star `n` (1-based).
//!   Use [`rating_option_key`] to format one, e.g. for a test's
//!   `UiEvent::synthetic_click`.
//!
//! # Anatomy
//!
//! ```text
//! rating  row  hug  gap=2  radiogroup
//! └── rating_item × max  icon button, ghost  ICON_MD star
//!
//! rating_display  row  hug  gap=2  img
//! └── icon × max  ICON_SM star
//! ```
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `icon_button`,
//! `.ghost()`, `.filled()`, the ARIA modifiers and
//! [`ArrowNav`]. An app crate can fork this file
//! and produce an equivalent widget against the same public API.

#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::event::{UiEvent, UiEventKind};
use crate::icons::icon;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::button::icon_button;

/// Stars in a rating unless [`RatingOpts::max`] says otherwise.
pub const DEFAULT_MAX: u8 = 5;

/// Space between stars: twice the hit outset each star carries, so
/// neighbouring hit targets meet without overlapping, and exactly the
/// room a focus ring needs.
const STAR_GAP: f32 = 2.0 * tokens::HIT_OVERFLOW;

/// Options for [`rating_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RatingOpts {
    /// How many stars the scale has. Default [`DEFAULT_MAX`].
    pub max: u8,
}

impl Default for RatingOpts {
    fn default() -> Self {
        Self { max: DEFAULT_MAX }
    }
}

impl RatingOpts {
    /// Set the number of stars (at least one).
    pub fn max(mut self, max: u8) -> Self {
        self.max = max.max(1);
        self
    }
}

/// What a routed [`UiEvent`] means for a rating keyed `key`.
///
/// Returned by [`classify_event`]; [`apply_event`] is the convenience
/// wrapper that folds the action into the app's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RatingAction {
    /// Star `n` (1-based) was clicked or activated.
    Rate(u8),
}

/// Classify a routed [`UiEvent`] against a rating keyed `key`.
/// Returns `None` for events that aren't for this rating.
///
/// Only `Click` / `Activate` qualify, so an `on_event` can call this
/// unconditionally.
pub fn classify_event(event: &UiEvent, key: &str) -> Option<RatingAction> {
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return None;
    }
    let rest = event.route()?.strip_prefix(key)?.strip_prefix(":rating:")?;
    rest.parse().ok().map(RatingAction::Rate)
}

/// Fold a routed [`UiEvent`] into the app's rating for a rating keyed
/// `key`. Returns `true` if the event was for this rating.
///
/// Picking star `n` sets the value to `n`; picking the star that is
/// already the value clears it to `0` (unrated).
pub fn apply_event(value: &mut u8, event: &UiEvent, key: &str) -> bool {
    let Some(RatingAction::Rate(n)) = classify_event(event, key) else {
        return false;
    };
    *value = if *value == n { 0 } else { n };
    true
}

/// Format the routed key emitted when star `n` (1-based) is picked.
pub fn rating_option_key(key: &str, n: u8) -> String {
    format!("{key}:rating:{n}")
}

/// A five-star rating input. `value` is the current rating, `0` for
/// unrated. Fold picks back with [`apply_event`].
///
/// Give it a name the way any control gets one — `field("Rating",
/// rating(..))` or `.aria_label(..)`.
#[track_caller]
pub fn rating(key: impl Into<String>, value: u8) -> El {
    build(Location::caller(), key.into(), value, RatingOpts::default())
}

/// [`rating`] with a scale other than five stars.
#[track_caller]
pub fn rating_with(key: impl Into<String>, value: u8, opts: RatingOpts) -> El {
    build(Location::caller(), key.into(), value, opts)
}

fn build(caller: &'static Location<'static>, key: String, value: u8, opts: RatingOpts) -> El {
    let max = opts.max.max(1);
    let value = value.min(max);
    let stars = (1..=max).map(|n| rating_item(&key, n, n <= value, n == value).at_loc(caller));
    // The row is deliberately not keyed (as `radio_group` and
    // `toggle_group` are not): the space between stars is chrome, and
    // a keyed row would route the bare key from a click there.
    El::new(Kind::Custom("rating"))
        .at_loc(caller)
        // Composite: a variant or status named on the row is meant
        // for the stars.
        .style_profile(StyleProfile::Composite)
        .role(Role::RadioGroup)
        .axis(Axis::Row)
        .gap(STAR_GAP)
        .align(Align::Center)
        // ARIA radio-group pattern: Left/Up previous, Right/Down next.
        .arrow_nav(ArrowNav::Both)
        .children(stars)
        .width(Size::Hug)
        .height(Size::Hug)
}

/// One star of a [`rating`]. Apps let [`rating`] build these; reach
/// for it directly to lay a scale out by hand.
///
/// `group_key` is the rating's key — the routed key on the star is
/// `{group_key}:rating:{n}` (see [`rating_option_key`]). `lit` fills
/// the star (every star up to the value); `checked` marks the one
/// star that *is* the value.
#[track_caller]
pub fn rating_item(group_key: &str, n: u8, lit: bool, checked: bool) -> El {
    let star = icon_button(IconName::Star)
        .at_loc(Location::caller())
        .default_icon_size(tokens::ICON_MD)
        .ghost();
    let star = if lit {
        star.filled().text_color(tokens::FOREGROUND)
    } else {
        star
    };
    star.role(Role::Radio)
        .aria_checked(checked)
        .aria_label(stars_label(n))
        .key(rating_option_key(group_key, n))
}

/// A score shown, not set: `value` of `max` stars as one read-only
/// image, for a list row, a card or a table cell. Takes no key and no
/// focus; assistive technology reads it as "3 of 5 stars".
#[track_caller]
pub fn rating_display(value: u8, max: u8) -> El {
    let max = max.max(1);
    let value = value.min(max);
    let stars = (1..=max).map(|n| {
        let star = icon(IconName::Star).default_icon_size(tokens::ICON_SM);
        if n <= value {
            star.filled().color(tokens::FOREGROUND)
        } else {
            star.color(tokens::MUTED_FOREGROUND)
        }
    });
    El::new(Kind::Custom("rating_display"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Composite)
        .role(Role::Img)
        .aria_label(format!("{value} of {}", stars_label(max)))
        .axis(Axis::Row)
        .gap(STAR_GAP)
        .align(Align::Center)
        .children(stars)
        .width(Size::Hug)
        .height(Size::Hug)
}

fn stars_label(n: u8) -> String {
    if n == 1 {
        "1 star".to_string()
    } else {
        format!("{n} stars")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::svg::IconSource;

    fn lit_count(row: &El) -> usize {
        row.children
            .iter()
            .filter(|star| matches!(star.icon, Some(IconSource::Filled(IconName::Star))))
            .count()
    }

    #[test]
    fn every_star_up_to_the_value_is_lit_and_the_value_is_the_checked_radio() {
        let row = rating("stars", 3);
        assert_eq!(row.children.len(), 5);
        assert_eq!(lit_count(&row), 3);
        let checked: Vec<Option<bool>> = row
            .children
            .iter()
            .map(|star| star.a11y.as_deref().and_then(|a| a.checked))
            .collect();
        assert_eq!(
            checked,
            [false, false, true, false, false].map(Some),
            "a radio group has one checked radio, the value's own star"
        );
        assert_eq!(row.children[2].key.as_deref(), Some("stars:rating:3"));
    }

    #[test]
    fn an_unrated_scale_lights_nothing_and_a_value_past_the_scale_lights_all() {
        assert_eq!(lit_count(&rating("stars", 0)), 0);
        let ten = rating_with("stars", 200, RatingOpts::default().max(10));
        assert_eq!(ten.children.len(), 10);
        assert_eq!(lit_count(&ten), 10);
    }

    #[test]
    fn picking_a_star_sets_it_and_picking_it_again_clears() {
        let mut stars = 0u8;
        let pick = |n| UiEvent::synthetic_click(rating_option_key("stars", n));
        assert!(apply_event(&mut stars, &pick(4), "stars"));
        assert_eq!(stars, 4);
        assert!(apply_event(&mut stars, &pick(2), "stars"));
        assert_eq!(stars, 2);
        assert!(apply_event(&mut stars, &pick(2), "stars"));
        assert_eq!(stars, 0, "the value's own star takes the rating back");

        // Keyboard activation is the same pick.
        let enter = UiEvent::synthetic_activate(rating_option_key("stars", 5));
        assert!(apply_event(&mut stars, &enter, "stars"));
        assert_eq!(stars, 5);
    }

    #[test]
    fn events_for_other_keys_and_other_kinds_are_not_this_ratings() {
        let mut stars = 3u8;
        let other = UiEvent::synthetic_click(rating_option_key("other", 1));
        assert!(!apply_event(&mut stars, &other, "stars"));
        // A longer key that merely starts with this one.
        let longer = UiEvent::synthetic_click(rating_option_key("stars2", 1));
        assert!(!apply_event(&mut stars, &longer, "stars"));
        let key_down = UiEvent::synthetic_key_down(
            rating_option_key("stars", 1),
            crate::event::LogicalKey::Character("a".into()),
        );
        assert!(!apply_event(&mut stars, &key_down, "stars"));
        assert_eq!(stars, 3);
    }

    #[test]
    fn a_status_or_a_size_named_on_the_row_reaches_the_stars() {
        let amber = rating("stars", 2).warning();
        for star in &amber.children {
            assert_eq!(
                star.text_color.and_then(|c| c.token),
                Some("warning-tint-foreground"),
                "lit and unlit stars both take the status"
            );
        }
        assert_eq!(lit_count(&amber), 2, "the fill still tells them apart");

        let mut small = column([rating("stars", 2).xsmall()]);
        crate::theme::Theme::default().apply_metrics(&mut small);
        let star = &small.children[0].children[0];
        assert_eq!(star.height, Size::Fixed(28.0));
        assert_eq!(star.width, Size::Fixed(28.0));
    }

    #[test]
    fn the_rating_and_its_display_lint_clean_in_a_form() {
        use crate::widgets::field::field;
        let mut root = column([
            field("Rating", rating("stars", 3)),
            rating_display(4, 5),
            rating("bare", 1).aria_label("Score"),
        ])
        .padding(tokens::SPACE_4)
        .gap(tokens::SPACE_4);
        let bundle = crate::render_bundle(&mut root, Rect::new(0.0, 0.0, 480.0, 320.0));
        assert!(bundle.lint.findings.is_empty(), "{}", bundle.lint.text());
    }

    #[test]
    fn the_display_is_one_named_image_with_no_focus_stop() {
        let shown = rating_display(3, 5);
        assert_eq!(lit_count(&shown), 3);
        assert_eq!(
            shown.a11y.as_deref().and_then(|a| a.label.as_deref()),
            Some("3 of 5 stars")
        );
        assert!(shown.key.is_none());
        assert!(shown.children.iter().all(|star| !star.focusable));
    }
}
