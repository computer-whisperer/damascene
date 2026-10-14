//! Swatch — the color well an accent picker is made of.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! row(ACCENTS.iter().enumerate().map(|(i, c)| {
//!     color_swatch(&format!("accent:{i}"), *c, i == self.accent)
//! }))
//! .gap(tokens::SPACE_2)
//! .align(Align::Center)
//! ```
//!
//! # No oracle — recorded justification
//!
//! `docs/NAMING_ORACLE.md` requires a cited oracle or a recorded
//! reason. [`color_swatch`] has no oracle:
//!
//! - **shadcn** ships no color-well, color-picker or swatch component
//!   at the pinned registry snapshot. Its nearest relatives are
//!   `ToggleGroup` (wrong anatomy: label slot, control height, style
//!   profile — the choice here *is* the fill) and `RadioGroup` (right
//!   semantics, wrong shape).
//! - **Tailwind** (the modifier oracle) has no production for a ring
//!   drawn at an offset outside an element either; `ring-offset-*` is
//!   the web's, and Damascene's `ring` is the focus treatment, not a
//!   selection treatment. That gap is precisely why this is a recipe
//!   rather than a modifier: see the anatomy below.
//! - The **web platform** names `<input type="color">`, but that is the
//!   OS picker dialog, not the row of pickable wells.
//!
//! The roster justifies minting it: all three settings rounds of the
//! 2026-07 validation corpus hand-rolled it, and all three named the
//! local helper `swatch` — `examples/settings.rs` (22px, stroke swap
//! on the cell itself), `examples/settings_match.rs` (18px in a padded
//! wrapper, with a checkmark), `examples/settings_v2.rs` (20px, 1px
//! stroke on the cell). Three sizes and two different ring techniques
//! for one shape is the drift a stock recipe exists to end.
//!
//! # Why the wrapper
//!
//! The selection ring must sit **outside** the swatch with a gap: a
//! stroke drawn on the well itself eats into the color the user is
//! choosing, and against a swatch of a similar hue it disappears.
//! There is no ring-offset modifier — Damascene's one outset stroke is
//! the focus ring, which the runtime owns — so the offset is built the
//! way `examples/settings_match.rs` built it: a padded parent whose
//! own stroke lands one padding-width clear of the child.
//!
//! That parent, not the well, carries the key and the focus opt-in, so
//! the focus ring lands outside the *selection* ring instead of
//! colliding with it in the same 2px band.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::bundle::lint::FindingKind;
use crate::cursor::Cursor;
use crate::tokens;
use crate::tree::*;

/// Edge length of the color well inside a [`color_swatch`], in logical
/// px.
///
/// 18px is the middle of the three sizes the validation rounds picked
/// (18 / 20 / 22) and the smallest of them: a picker row is a row of
/// samples, not of buttons, and the well should read as smaller than
/// the [`tokens::CONTROL_HEIGHT`] controls stacked above it. The
/// pointer target does not shrink with it — the runtime inflates any
/// focusable node's hit rect to [`tokens::MIN_TOUCH_TARGET`].
pub const COLOR_SWATCH_SIZE: f32 = 18.0;

/// Gap between a selected [`color_swatch`]'s well and its ring.
///
/// Small on purpose: the ring has to read as *this* swatch's ring at a
/// glance across a row of them, so the offset must stay well under the
/// row gap.
pub const COLOR_SWATCH_RING_OFFSET: f32 = 2.0;

/// Ring stroke width for a selected [`color_swatch`].
pub const COLOR_SWATCH_RING_WIDTH: f32 = 2.0;

/// One pickable color well — a keyed, focusable
/// [`COLOR_SWATCH_SIZE`] rounded square filled with `color`, ringed
/// when `selected`.
///
/// # Anatomy
///
/// ```text
/// color_swatch   key  focusable  padding=2  radius=RADIUS_SM+2   (Hug)
///     stroke=foreground ×2 when selected, none otherwise
///   └─ color_swatch_well   fill=<color>  radius=RADIUS_SM  18×18
/// ```
///
/// The wrapper always exists, selected or not, so a row of swatches
/// does not reflow when the selection moves — the same
/// contents-independent-geometry rule the workbench bars follow.
///
/// # The ring color
///
/// [`tokens::FOREGROUND`], not [`tokens::RING`] or
/// [`tokens::PRIMARY`]. The ring's job is to contrast against an
/// *arbitrary* fill — the swatches are the app's accent candidates, and
/// one of them is usually the primary itself, which a primary-colored
/// ring would vanish into. `foreground` is the one token guaranteed to
/// sit at the far end of the ramp from every surface and, in practice,
/// from every accent. It is also what two of the three validation
/// rounds reached for unprompted. [`tokens::RING`] is reserved: it
/// means *keyboard focus*, and focus and selection must stay
/// distinguishable on this widget in particular, where both rings are
/// visible at once.
///
/// # Lint
///
/// The well opts out of `RawColor`. Everywhere else a raw rgba fill is
/// a mistake, but a swatch's whole content is a literal color the app
/// is offering — a palette token would defeat the widget.
#[track_caller]
pub fn color_swatch(key: &str, color: Color, selected: bool) -> El {
    let well = El::new(Kind::Custom("color_swatch_well"))
        .at_loc(Location::caller())
        .allow_lint(FindingKind::RawColor)
        .fill(color)
        .default_radius(tokens::RADIUS_SM)
        .width(Size::Fixed(COLOR_SWATCH_SIZE))
        .height(Size::Fixed(COLOR_SWATCH_SIZE));

    let swatch = El::new(Kind::Custom("color_swatch"))
        .at_loc(Location::caller())
        .children([well])
        .key(key.to_string())
        .focusable()
        .cursor(Cursor::Pointer)
        .padding(Sides::all(COLOR_SWATCH_RING_OFFSET))
        // Concentric with the well: the outer corner has to clear the
        // inner one by the offset or the ring cuts the well's corners.
        .default_radius(tokens::RADIUS_SM + COLOR_SWATCH_RING_OFFSET)
        .width(Size::Hug)
        .height(Size::Hug)
        .align(Align::Center)
        .justify(Justify::Center);

    if selected {
        swatch
            .stroke(tokens::FOREGROUND)
            .stroke_width(COLOR_SWATCH_RING_WIDTH)
    } else {
        swatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAL: Color = Color::srgb_token("teal-sample", 20, 148, 140, 255);

    #[test]
    fn color_swatch_has_the_documented_anatomy() {
        let s = color_swatch("accent:0", TEAL, false);

        assert_eq!(s.kind, Kind::Custom("color_swatch"));
        assert_eq!(s.children.len(), 1, "the ring wraps exactly one well");
        assert_eq!(s.width, Size::Hug);
        assert_eq!(s.height, Size::Hug);
        assert_eq!(s.padding, Sides::all(COLOR_SWATCH_RING_OFFSET));

        let well = &s.children[0];
        assert_eq!(well.kind, Kind::Custom("color_swatch_well"));
        assert_eq!(well.fill, Some(TEAL));
        assert_eq!(well.width, Size::Fixed(COLOR_SWATCH_SIZE));
        assert_eq!(well.height, Size::Fixed(COLOR_SWATCH_SIZE));
        assert!(well.text.is_none(), "a color well carries no label");
    }

    #[test]
    fn the_wrapper_owns_the_key_and_the_focus() {
        // Not the well: the focus ring is drawn outside the node that
        // owns it, and on the well it would land inside the selection
        // ring's band.
        let s = color_swatch("accent:2", TEAL, false);
        assert_eq!(s.key.as_deref(), Some("accent:2"));
        assert!(s.focusable);
        assert_eq!(s.cursor, Some(Cursor::Pointer));

        let well = &s.children[0];
        assert!(well.key.is_none());
        assert!(!well.focusable);
    }

    #[test]
    fn selection_rings_outside_the_well_with_an_offset() {
        let off = color_swatch("accent:0", TEAL, false);
        let on = color_swatch("accent:0", TEAL, true);

        assert!(off.stroke.is_none(), "an unselected swatch has no ring");
        assert_eq!(off.stroke_width, 0.0);

        assert_eq!(on.stroke, Some(tokens::FOREGROUND));
        assert_eq!(on.stroke.and_then(|c| c.token), Some("foreground"));
        assert_eq!(on.stroke_width, COLOR_SWATCH_RING_WIDTH);

        // The ring is on the wrapper, so it stands `COLOR_SWATCH_RING_OFFSET`
        // clear of the well rather than eating into the color.
        assert!(
            on.children[0].stroke.is_none(),
            "the well itself is never stroked — that is the whole point"
        );
        assert_eq!(on.padding, Sides::all(COLOR_SWATCH_RING_OFFSET));
    }

    #[test]
    fn geometry_is_contents_independent_across_the_selection() {
        // A row of swatches must not reflow when the selection moves.
        let off = color_swatch("a", TEAL, false);
        let on = color_swatch("a", TEAL, true);
        assert_eq!(off.width, on.width);
        assert_eq!(off.height, on.height);
        assert_eq!(off.padding, on.padding);
        assert_eq!(off.children.len(), on.children.len());
    }

    #[test]
    fn the_ring_corner_clears_the_well_corner_by_the_offset() {
        let s = color_swatch("a", TEAL, true);
        assert_eq!(
            s.radius.tl,
            s.children[0].radius.tl + COLOR_SWATCH_RING_OFFSET,
            "concentric corners: outer = inner + offset",
        );
    }

    #[test]
    fn the_well_opts_out_of_the_raw_color_lint() {
        // A literal color is this widget's content. Assert the opt-out
        // is narrow — only `RawColor`, and only on the well.
        let s = color_swatch("a", Color::srgb_u8(20, 148, 140), false);
        let allowed = s.children[0]
            .allow_lint
            .as_ref()
            .expect("the well opts out");
        assert_eq!(allowed.as_slice(), &[FindingKind::RawColor]);
        assert!(
            s.allow_lint.is_none(),
            "the wrapper suppresses nothing — only the well holds the raw color"
        );
    }
}
