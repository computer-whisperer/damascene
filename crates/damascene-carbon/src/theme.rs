//! The Carbon theme — palette, metrics, and fonts as one decision.
//!
//! ## Two vocabularies, one set of pixels
//!
//! An app using this crate paints from two token sets at once:
//!
//! - **Carbon tokens** ([`crate::tokens`]) used by this crate's widgets.
//!   `Palette::lookup` only recognizes shadcn names, so these resolve to
//!   the literal `g100` rgba baked into each constant.
//! - **shadcn tokens** (`damascene_core::tokens`) used by any stock
//!   widget you drop in — `button`, `checkbox`, `text_input`. Those
//!   resolve through the active [`Palette`].
//!
//! [`palette`] exists to keep those two in agreement: it maps Carbon's
//! `g100` values onto the shadcn field names, so a stock `button()`
//! beside a Carbon [`crate::widgets::tag`] reads as one system rather
//! than two. Without it you would get shadcn zinc controls on Carbon
//! gray surfaces.
//!
//! ## Known limitation
//!
//! Because Carbon token names are not palette members, swapping the
//! palette at runtime moves the stock widgets but *not* this crate's
//! widgets. That is acceptable while `g100` is the only theme here, and
//! the fix is a one-line additive core change — an `extra:
//! BTreeMap<&'static str, Color>` on `Palette`, consulted by `lookup`
//! on a miss. Until then, treat this theme as fixed-dark.
//!
//! ## Font substitution
//!
//! Carbon specifies IBM Plex Sans and IBM Plex Mono. Damascene bundles
//! Inter, Roboto, and JetBrains Mono, so Inter and JetBrains Mono stand
//! in. Every size, line height, weight, and letter-spacing value in
//! [`crate::tokens`] is Carbon's; only the faces differ.

#![warn(missing_docs)]

use damascene_core::Theme;
use damascene_core::metrics::{ComponentSize, ThemeMetrics};
use damascene_core::theme::palette::Palette;
use damascene_core::tree::FontFamily;

use crate::tokens as carbon;

/// Carbon `g100` values mapped onto the shadcn palette field names, so
/// stock damascene widgets match this crate's widgets.
///
/// The interesting mappings:
///
/// - `card` / `popover` become [`carbon::LAYER_01`] / [`carbon::LAYER_02`]
///   rather than repeating `background`. In the stock dark palette all
///   three are `#09090b`, which is why a stock `card()` needs a border
///   to be visible at all.
/// - `muted` and `accent` are *split*: `muted` is the `layer-02` band,
///   `accent` is `layer-hover-01`. The stock palette collapses
///   `secondary`/`muted`/`accent`/`border`/`input` onto one value.
/// - `border` is `border-subtle-01`, distinct from `input`'s `field-01`.
pub fn palette() -> Palette {
    Palette {
        background: carbon::BACKGROUND,
        foreground: carbon::TEXT_PRIMARY,

        card: carbon::LAYER_01,
        card_foreground: carbon::TEXT_PRIMARY,

        popover: carbon::LAYER_02,
        popover_foreground: carbon::TEXT_PRIMARY,

        primary: carbon::BUTTON_PRIMARY,
        primary_foreground: carbon::TEXT_ON_COLOR,

        secondary: carbon::BUTTON_SECONDARY,
        secondary_foreground: carbon::TEXT_ON_COLOR,

        muted: carbon::LAYER_02,
        muted_foreground: carbon::TEXT_SECONDARY,

        accent: carbon::LAYER_HOVER_01,
        accent_foreground: carbon::TEXT_PRIMARY,

        destructive: carbon::BUTTON_DANGER,
        destructive_foreground: carbon::TEXT_ON_COLOR,

        border: carbon::BORDER_SUBTLE_01,
        input: carbon::FIELD_01,
        ring: carbon::FOCUS,

        success: carbon::SUPPORT_SUCCESS,
        warning: carbon::SUPPORT_WARNING,
        info: carbon::SUPPORT_INFO,

        overlay_scrim: carbon::OVERLAY,
        link_foreground: carbon::LINK_PRIMARY,

        // Foregrounds-on-solid and the selection/scrollbar extensions
        // have no distinct Carbon counterpart worth overriding; the
        // stock dark values already read correctly against `g100`.
        ..Palette::damascene_dark()
    }
}

/// The Carbon `g100` theme: [`palette`], compact control metrics, Inter
/// + JetBrains Mono.
///
/// `ComponentSize::Xs` (28px controls) is deliberately *tighter* than
/// Carbon's own `sm` field height of 32px. Carbon's density lives in its
/// data display — `size="xs"` tables are 24px rows — while its buttons
/// stay comparatively large. Damascene's `Xs` rung is the closest
/// available match for dense toolbar chrome, and this crate's own
/// widgets use [`carbon::FIELD_HEIGHT_SM`] where a true Carbon field
/// height is wanted.
pub fn theme() -> Theme {
    Theme::default()
        .with_palette(palette())
        .with_font_family(FontFamily::Inter)
        .with_mono_font_family(FontFamily::JetBrainsMono)
        .with_metrics(ThemeMetrics::new().with_default_component_size(ComponentSize::Xs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_are_distinct_from_background() {
        // The whole premise: a surface must be legible without a border.
        let p = palette();
        assert_ne!(
            (p.background.r, p.background.g, p.background.b),
            (p.card.r, p.card.g, p.card.b),
            "card must step above background, unlike the stock dark palette"
        );
        assert_ne!(
            (p.card.r, p.card.g, p.card.b),
            (p.popover.r, p.popover.g, p.popover.b),
            "popover must step above card"
        );
    }

    #[test]
    fn muted_accent_and_border_are_not_collapsed() {
        // The stock dark palette maps secondary/muted/accent/border/input
        // all to #27272a. Carbon separates the roles.
        let p = palette();
        let rgb = |c: damascene_core::tree::Color| (c.r, c.g, c.b);
        assert_ne!(rgb(p.accent), rgb(p.muted));
        assert_ne!(rgb(p.border), rgb(p.input));
    }

    #[test]
    fn carbon_tokens_do_not_resolve_through_the_palette() {
        // Documents the known limitation above: Carbon names are not
        // palette members, so they keep their literal rgba. If a future
        // core change adds an `extra` map, this test should flip.
        let p = palette();
        assert!(
            p.lookup("layer-01").is_none(),
            "layer-01 is not a shadcn palette member"
        );
    }

    #[test]
    fn theme_uses_carbon_palette() {
        let t = theme();
        assert_eq!(t.palette().background.r, carbon::BACKGROUND.r);
    }
}
