//! Badge — small status chip (rounded-md, per current shadcn), and
//! [`status_dot`], its textless sibling.
//!
//! Default style is `info` (accent-tinted). Apply status modifiers from
//! [`crate::style`]: `.success()`, `.warning()`, `.destructive()`,
//! `.info()`, `.muted()`.
//!
//! `.outline()` produces shadcn's `Badge` `variant="outline"`:
//! transparent fill, `border`-token stroke, plain foreground label —
//! the quiet chip for metadata (`badge("v2").outline()`) where a
//! status tint would over-claim.
//!
//! # `status_dot` has no oracle — recorded justification
//!
//! `docs/NAMING_ORACLE.md` requires every new public name to cite its
//! oracle, or to record why it has none. [`status_dot`] has none:
//!
//! - **shadcn** (the widget/anatomy oracle) ships no dot component. Its
//!   `Badge` is the nearest relative, which is why this lives here: a
//!   presence dot is the badge genre with the label removed.
//! - **Lucide** (the icon oracle) has `circle` / `dot` glyphs, but they
//!   are 1px-stroked outline icons at 24px viewbox. A presence dot is a
//!   7px *filled* shape, not an icon, and routing it through the icon
//!   pipeline would make it inherit stroke width and icon sizing.
//! - The **web platform** names no such element.
//!
//! What justifies minting it anyway is the roster: the dot is the
//! highest-frequency hand-roll in the validation corpus. Four of the
//! four 2026-07 rounds wrote it, under three names and three sizes —
//! `examples/voice.rs` and `examples/parts_match.rs` at 7px, both named
//! `dot`; `examples/voice_match.rs` at 8px, named `dot`;
//! `examples/voice_v2.rs` (the fresh acceptance round, which had the
//! whole anatomy arc available) at a local `PRESENCE_DOT` const, named
//! `presence_dot`. A production that four independent agents write from
//! scratch is a missing production, and the divergent sizes are exactly
//! the drift a stock recipe removes. The name takes `status` over
//! `presence` because the two validation asks — a member's presence in
//! `voice`, a net-class state in `parts` — are both *status*, and
//! presence is the narrower of the two.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// Badge corner radius — shadcn's `rounded-md` (current shadcn badges
/// moved off the full pill).
pub const BADGE_RADIUS: f32 = 6.0;

/// Small status chip (shadcn's `Badge`) — hugging caption text in a
/// tinted, rounded-md shell. Defaults to the `info` tint; chain a
/// status modifier (`.success()`, `.warning()`, …) for other variants,
/// or `.outline()` for shadcn's `variant="outline"` (transparent fill,
/// `border` stroke, foreground label).
#[track_caller]
pub fn badge(label: impl Into<String>) -> El {
    El::new(Kind::Badge)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Tinted)
        .metrics_role(MetricsRole::Badge)
        .text(label)
        .text_align(TextAlign::Center)
        .caption()
        .font_weight(FontWeight::Medium)
        .text_color(tokens::INFO_TINT_FOREGROUND)
        .fill(tokens::INFO.with_alpha_u8(38))
        .stroke(tokens::INFO.with_alpha_u8(120))
        .default_radius(BADGE_RADIUS)
        .width(Size::Hug)
        .default_height(Size::Fixed(20.0))
        .default_padding(Sides::xy(tokens::SPACE_2, 0.0))
}

/// Diameter of a [`status_dot`], in logical px.
///
/// 7px is the size two of the four validation rounds converged on
/// independently. It is the smallest disc that still reads as a
/// deliberate shape rather than as a rendering artifact next to
/// [`tokens::TEXT_XS`] text, and it clears the cap height of a 12px
/// caption without out-weighing it.
pub const STATUS_DOT_SIZE: f32 = 7.0;

/// The smallest thing in a shell that carries state — a
/// [`STATUS_DOT_SIZE`] filled disc in `color`, with no label.
///
/// ```ignore
/// use damascene_core::prelude::*;
///
/// row([status_dot(tokens::SUCCESS), text("t.vaughan").caption()])
///     .gap(tokens::SPACE_2)
///     .align(Align::Center)
/// ```
///
/// # Anatomy
///
/// ```text
/// status_dot  fill=<color>  radius=pill  7×7
/// ```
///
/// A leaf: no children, no text, not focusable, no cursor. It is
/// decoration for the row it sits in, and the row — not the dot —
/// carries the key, the tooltip, and the accessible name. Pair it with
/// text; a dot alone communicates a state only to someone who already
/// knows the legend.
///
/// `color` is deliberately a parameter rather than a set of status
/// modifiers: the caller's state enum maps onto whatever the app calls
/// online / away / busy, and that mapping is the app's, not the
/// library's. Pass a **token** ([`tokens::SUCCESS`],
/// [`tokens::WARNING`], [`tokens::DESTRUCTIVE`],
/// [`tokens::MUTED_FOREGROUND`] for offline) so the dot follows a
/// palette swap — a raw rgba here trips the `RawColor` lint, on
/// purpose.
///
/// Related and deliberately distinct: [`badge`] is the *labelled*
/// status chip, and `damascene_workbench::chrome::chip` is the solid
/// count chip for application chrome. Reach for a dot only when the
/// label is already in the row beside it.
#[track_caller]
pub fn status_dot(color: Color) -> El {
    // `Kind::Custom` rather than `divider()`: a divider is a rule, and
    // the inspector, the semantics pass and every lint that keys off
    // `Kind` should see a state indicator instead.
    El::new(Kind::Custom("status_dot"))
        .at_loc(Location::caller())
        .fill(color)
        .default_radius(tokens::RADIUS_PILL)
        .width(Size::Fixed(STATUS_DOT_SIZE))
        .height(Size::Fixed(STATUS_DOT_SIZE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_defaults_to_the_info_tint() {
        let b = badge("Online");
        assert_eq!(b.kind, Kind::Badge);
        assert_eq!(b.style_profile, StyleProfile::Tinted);
        assert_eq!(b.text.as_deref(), Some("Online"));
        assert_eq!(b.text_role, TextRole::Caption);
        assert_eq!(b.font_weight, FontWeight::Medium);
        assert_eq!(b.fill, Some(tokens::INFO.with_alpha_u8(38)));
        assert_eq!(b.stroke, Some(tokens::INFO.with_alpha_u8(120)));
        assert_eq!(b.text_color, Some(tokens::INFO_TINT_FOREGROUND));
        assert_eq!(b.width, Size::Hug);
    }

    #[test]
    fn badge_outline_is_transparent_with_a_border_stroke() {
        // shadcn `Badge` variant="outline": `border-border
        // text-foreground`, no background.
        let b = badge("v2").outline();
        assert!(b.fill.is_none(), "outline badge paints no fill");
        assert_eq!(b.stroke, Some(tokens::BORDER));
        assert_eq!(b.stroke_width, 1.0);
        assert_eq!(b.text_color, Some(tokens::FOREGROUND));
        // Geometry is the shared badge recipe — outline changes color
        // only.
        assert_eq!(b.radius, Corners::all(BADGE_RADIUS));
        assert_eq!(b.height, Size::Fixed(20.0));
        assert_eq!(b.font_weight, FontWeight::Medium);
    }

    #[test]
    fn badge_outline_stroke_is_border_not_input() {
        // Regression guard for the profile-aware `.outline()` split:
        // chips take shadcn's `border`, controls take `input`. The two
        // tokens happen to carry the same rgb in the stock dark
        // palette, so only the token name distinguishes them — and it
        // is the token that a palette swap re-resolves.
        let chip = badge("v2").outline();
        assert_eq!(chip.stroke.and_then(|c| c.token), Some("border"));

        let control = crate::button("Outline").outline();
        assert_eq!(control.stroke.and_then(|c| c.token), Some("input"));
    }

    #[test]
    fn badge_tinted_variants_are_unchanged_by_the_outline_split() {
        // The fill/stroke ride the status token; the *text* rides its
        // text-grade sibling (`*_TINT_FOREGROUND`), which is what
        // clears WCAG AA on the tint. `.primary()` is its own text
        // tone — a near-white/near-black neutral is already legible on
        // its own tint.
        for (b, tint, text) in [
            (
                badge("ok").success(),
                tokens::SUCCESS,
                tokens::SUCCESS_TINT_FOREGROUND,
            ),
            (
                badge("warn").warning(),
                tokens::WARNING,
                tokens::WARNING_TINT_FOREGROUND,
            ),
            (
                badge("err").destructive(),
                tokens::DESTRUCTIVE,
                tokens::DESTRUCTIVE_TINT_FOREGROUND,
            ),
            (
                badge("info").info(),
                tokens::INFO,
                tokens::INFO_TINT_FOREGROUND,
            ),
            (badge("brand").primary(), tokens::PRIMARY, tokens::PRIMARY),
        ] {
            assert_eq!(b.fill, Some(tint.with_alpha_u8(38)));
            assert_eq!(b.stroke, Some(tint.with_alpha_u8(120)));
            assert_eq!(b.stroke_width, 1.0);
            assert_eq!(b.text_color, Some(text));
        }

        // `.muted()` still swaps to the neutral surface, not a tint.
        let m = badge("draft").muted();
        assert_eq!(m.fill, Some(tokens::MUTED));
        assert_eq!(m.stroke, Some(tokens::BORDER));
        assert_eq!(m.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn status_dot_has_the_documented_anatomy() {
        let d = status_dot(tokens::SUCCESS);

        assert_eq!(d.kind, Kind::Custom("status_dot"));
        assert_eq!(d.fill, Some(tokens::SUCCESS));
        assert_eq!(d.width, Size::Fixed(STATUS_DOT_SIZE));
        assert_eq!(d.height, Size::Fixed(STATUS_DOT_SIZE));
        assert_eq!(d.radius, Corners::all(tokens::RADIUS_PILL));
    }

    #[test]
    fn status_dot_is_a_textless_unkeyed_leaf() {
        // Decoration for the row it sits in: the row owns the key, the
        // tooltip and the accessible name. If any of this changes, a
        // dot starts competing with its own label for hit-testing.
        let d = status_dot(tokens::DESTRUCTIVE);
        assert!(d.children.is_empty());
        assert!(d.text.is_none());
        assert!(d.icon.is_none());
        assert!(!d.focusable);
        assert!(d.key.is_none());
        assert!(d.cursor.is_none());
        assert!(d.stroke.is_none(), "a dot is a fill, not an outline");
    }

    #[test]
    fn status_dot_stays_round_under_a_radius_scale() {
        // `RADIUS_PILL` is exempt from the theme radius scale, which is
        // what keeps a dot circular under the workbench theme's 0.29×.
        // It goes through `default_radius` so a caller can still square
        // it deliberately.
        assert_eq!(
            status_dot(tokens::INFO).radius_origin,
            RadiusOrigin::ThemeDefault
        );
        let squared = status_dot(tokens::INFO).radius(2.0);
        assert_eq!(squared.radius, Corners::all(2.0));
        assert_eq!(squared.radius_origin, RadiusOrigin::Fixed);
    }

    #[test]
    fn status_dot_takes_a_token_so_it_follows_a_palette_swap() {
        // The parameter is a `Color`, but the intent is a token: the
        // name must survive so `Palette::resolve` can re-point it.
        let d = status_dot(tokens::WARNING);
        assert_eq!(d.fill.and_then(|c| c.token), Some("warning"));
    }

    #[test]
    fn status_dot_is_smaller_than_the_badge_it_is_the_textless_form_of() {
        let dot = status_dot(tokens::SUCCESS);
        let chip = badge("Online").success();
        let fixed = |s: Size| match s {
            Size::Fixed(v) => v,
            other => panic!("expected a fixed size, got {other:?}"),
        };
        assert!(fixed(dot.height) < fixed(chip.height));
        // And it reads beside caption text rather than swamping it.
        const { assert!(STATUS_DOT_SIZE < tokens::TEXT_XS.size) };
    }
}
