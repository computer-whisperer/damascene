//! Card — shadcn-shaped content container anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `card([card_header([...]), card_content([...]), card_footer([...])])`.
//! `titled_card(title, body)` is a convenience wrapper for older/simple
//! examples, built from the same anatomy rather than a separate layout.
//!
//! `card()` is also the canonical "panel surface" — the bundle of
//! [`SurfaceRole::Panel`] + `tokens::CARD` fill + `tokens::BORDER`
//! stroke + `tokens::RADIUS_LG` (= shadcn `rounded-xl`) + shadow that an
//! LLM might otherwise hand-roll. If the helpers (`card_header`,
//! `card_content`, `card_footer`) don't fit your data shape, **wrap
//! your custom composition in `card([...])` instead of replacing it**
//! — that keeps the surface recipe correct everywhere. Same applies to
//! right-rail inspector panes and other "boxed" wrappers that aren't
//! navigation (use `sidebar()` for that).
//!
//! Slot padding is baked into each constructor as `default_padding(...)`
//! — shadcn's stock recipe, visible at the call site:
//!
//! - `card_header` — `SPACE_6` on all sides, plus `default_gap(SPACE_2)`
//!   for the title + description rhythm (≈ `space-y-1.5`).
//! - `card_content` — `SPACE_6` on left / right / bottom, `0` on top
//!   (= `p-6 pt-0`), so the visual gap below the header comes from the
//!   header's bottom padding rather than from doubling.
//! - `card_footer` — same recipe as `card_content`, with
//!   `Align::Center`.
//!
//! Override at the call site, Tailwind-shaped: `.padding(...)` replaces
//! the whole `Sides` struct (= `p-N`); the additive shorthands
//! (`.pt(...)`, `.pb(...)`, `.pl(...)`, `.pr(...)`, `.px(...)`,
//! `.py(...)`) override a single side or axis while preserving the
//! constructor's defaults for the others (= `p-6 pt-0` is
//! `card_content([...]).pt(0.0)` here). The metrics pass does not
//! touch these slots, so any explicit value sticks.
//!
//! A header bar with a tinted strip — common for inspector panes and
//! diff/hunk frames — is `card_header([...]).fill(tokens::MUTED)`; do
//! not hand-roll the strip as a `row(...).fill(MUTED).stroke(BORDER)`
//! sibling of the body.
//!
//! The metrics pass propagates the card's top-corner radii onto a
//! leading `card_header` that has a fill (and symmetric for a trailing
//! `card_footer`) so the strip follows the card's curve instead of
//! poking flat corners through it. Author-set `.radius(...)` on the
//! slot wins; explicit `.padding(...)` on the card itself disables
//! inheritance since the slot is no longer flush with the card edge.

use std::panic::Location;

use super::text::{h3, text};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn card<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Card)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Card)
        .surface_role(SurfaceRole::Panel)
        .children(children)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_LG)
        .shadow(tokens::SHADOW_MD)
        .width(Size::Fill(1.0))
        .default_height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}

#[track_caller]
pub fn titled_card<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    card([
        card_header([card_title(title)]),
        card_content(body.into_iter().map(Into::into).collect::<Vec<_>>()),
    ])
    .at_loc(Location::caller())
}

#[track_caller]
pub fn card_header<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::CardHeader)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(tokens::SPACE_6)
        .default_gap(tokens::SPACE_2)
}

#[track_caller]
pub fn card_title(title: impl Into<String>) -> El {
    h3(title)
        .at_loc(Location::caller())
        .line_height(tokens::TEXT_BASE.size)
}

#[track_caller]
pub fn card_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .fill_width()
}

#[track_caller]
pub fn card_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::CardContent)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(Sides {
            left: tokens::SPACE_6,
            right: tokens::SPACE_6,
            top: 0.0,
            bottom: tokens::SPACE_6,
        })
}

#[track_caller]
pub fn card_footer<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::CardFooter)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
        .default_padding(Sides {
            left: tokens::SPACE_6,
            right: tokens::SPACE_6,
            top: 0.0,
            bottom: tokens::SPACE_6,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_title_uses_tight_heading_rhythm() {
        let title = card_title("Latency");

        assert_eq!(title.text_role, TextRole::Title);
        assert_eq!(title.font_size, tokens::TEXT_BASE.size);
        assert_eq!(title.line_height, tokens::TEXT_BASE.size);
        assert_eq!(title.font_weight, FontWeight::Semibold);
    }
}
