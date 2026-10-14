//! Kbd — keycap chip for keyboard-shortcut hints.
//!
//! Oracle: shadcn's `Kbd` / `KbdGroup`
//! (`references/workbench-validation/shadcn-refs/src/components/ui/kbd.tsx`),
//! per `docs/NAMING_ORACLE.md`.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! row([
//!     text("Open the palette").caption(),
//!     kbd_group([kbd("⌘"), kbd("K")]),
//! ])
//! .gap(tokens::SPACE_2)
//! .align(Align::Center)
//! ```
//!
//! Related, and deliberately distinct:
//!
//! - [`command_shortcut`](super::command::command_shortcut) and
//!   [`menubar_shortcut`](super::menubar::menubar_shortcut) are dim
//!   *un-boxed* trailing text — the right thing inside a menu row,
//!   where a boxed keycap per row would be noise.
//! - [`kbd`] is the boxed keycap, for hint rows and composer footers
//!   where the key itself is the subject ("press ⌘K").
//!
//! Anatomy notes (where this diverges from the vendored tsx, and why):
//!
//! - shadcn's `Kbd` is `font-sans` with `bg-muted` and no border.
//!   Damascene's is **mono with a 1px rim** — the 2026-07 validation
//!   rounds hand-rolled exactly that shape (`examples/voice_match.rs`
//!   `kbd`, fill `#2A2A2A` over a `#4B4B4B` stroke), because against a
//!   dark workbench ground a borderless muted fill does not separate
//!   from the panel behind it. Names come from the oracle; values stay
//!   calibrated to the workbench target.
//! - The rim is [`tokens::INPUT`], not [`tokens::BORDER`]. Those
//!   collapse onto one value in the stock dark palette but split under
//!   the workbench theme (`input.border` `#3C3C3C` vs `panel.border`
//!   `#2B2B2B`), and only the brighter one reads as a keycap edge over
//!   a `list.hoverBackground` fill.
//! - Height is [`KBD_HEIGHT`] (18px) rather than shadcn's `h-5`
//!   (20px): workbench hint rows are 22–24px tall, and a 20px keycap
//!   fills them wall to wall.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// Keycap height — one notch under shadcn's `h-5`, so a keycap reads
/// as instrumentation inside a caption-height hint row. See the module
/// docs for why this diverges from the oracle.
pub const KBD_HEIGHT: f32 = 18.0;

/// Keyboard-shortcut keycap (shadcn's `Kbd`) — a hugging monospace
/// caption in a muted, 1px-rimmed, `rounded-sm` chip.
///
/// Display only: it neither registers nor handles the shortcut. Bind
/// the real chord with [`ChordTrigger`](crate::event::ChordTrigger).
///
/// One key per call — `kbd("⌘")`, `kbd("Shift")`, `kbd("F9")` — and
/// weld a sequence together with [`kbd_group`].
///
/// Style profile is [`StyleProfile::Surface`]: `.muted()` quiets the rim
/// to [`tokens::BORDER`], and a status modifier (`.destructive()`, …)
/// tints the chip when a key is bound to something dangerous.
#[track_caller]
pub fn kbd(label: impl Into<String>) -> El {
    El::new(Kind::Custom("kbd"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .text(label)
        .caption()
        .mono()
        .font_weight(FontWeight::Medium)
        .text_color(tokens::MUTED_FOREGROUND)
        .text_align(TextAlign::Center)
        .fill(tokens::MUTED)
        .stroke(tokens::INPUT)
        .default_radius(tokens::RADIUS_SM)
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Center)
        .default_gap(tokens::SPACE_1)
        .width(Size::Hug)
        // shadcn's `min-w-5` keeps a one-glyph cap from collapsing to
        // a sliver; ours squares off at the keycap height.
        .min_width(KBD_HEIGHT)
        .default_height(Size::Fixed(KBD_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_1, 0.0))
}

/// Tight row of [`kbd`] caps (shadcn's `KbdGroup`) — for a chord or
/// sequence like `⌘` `K`, or `Shift` `+` `Enter` with a literal `+`
/// text leaf between the caps.
///
/// Purely structural: it adds no styling of its own beyond a 4px gap
/// and centered cross-axis alignment, so the caps stay welded while
/// neighbouring hint groups space apart normally.
#[track_caller]
pub fn kbd_group<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("kbd_group"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Row)
        .align(Align::Center)
        .width(Size::Hug)
        .height(Size::Hug)
        .default_gap(tokens::SPACE_1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kbd_is_a_mono_caption_on_a_bordered_muted_chip() {
        let k = kbd("⌘");

        assert_eq!(k.kind, Kind::Custom("kbd"));
        assert_eq!(k.text.as_deref(), Some("⌘"));

        // Mono caption text: TEXT_XS, medium, muted-foreground.
        assert!(k.font_mono, "keycaps read in the monospace face");
        assert!(
            k.explicit_mono,
            "`.mono()` must be sticky so a later role modifier can't reset it",
        );
        assert_eq!(k.text_role, TextRole::Caption);
        assert_eq!(k.font_size, tokens::TEXT_XS.size);
        assert_eq!(k.line_height, tokens::TEXT_XS.line_height);
        assert_eq!(k.font_weight, FontWeight::Medium);
        assert_eq!(k.text_color, Some(tokens::MUTED_FOREGROUND));
        assert_eq!(k.text_align, TextAlign::Center);

        // Chip shell: muted fill, 1px rim, rounded-sm. The rim is the
        // brighter `input` token, not `border` — see the module docs.
        assert_eq!(k.fill, Some(tokens::MUTED));
        assert_eq!(k.stroke, Some(tokens::INPUT));
        assert_eq!(k.stroke.and_then(|c| c.token), Some("input"));
        assert_eq!(k.stroke_width, 1.0);
        assert_eq!(k.radius, Corners::all(tokens::RADIUS_SM));

        // Hugging width, tight fixed height, tight x padding.
        assert_eq!(k.width, Size::Hug);
        assert_eq!(k.height, Size::Fixed(KBD_HEIGHT));
        assert_eq!(k.min_width, Some(KBD_HEIGHT));
        assert_eq!(k.padding, Sides::xy(tokens::SPACE_1, 0.0));
    }

    #[test]
    fn kbd_is_dim_instrumentation_not_a_control() {
        let k = kbd("F9");
        assert!(!k.focusable, "a keycap is display only, never focusable");
        assert!(k.cursor.is_none(), "a keycap must not claim a hand cursor");
        assert!(
            matches!(k.height, Size::Fixed(h) if h < tokens::CONTROL_HEIGHT),
            "a keycap must sit well under control height, got {:?}",
            k.height,
        );
    }

    #[test]
    fn kbd_geometry_is_overridable() {
        // The height/radius/padding recipe goes through the `default_*`
        // setters, so an explicit call at the site still wins.
        let k = kbd("Enter").height(Size::Fixed(24.0)).radius(2.0);
        assert_eq!(k.height, Size::Fixed(24.0));
        assert_eq!(k.radius, Corners::all(2.0));
    }

    #[test]
    fn kbd_takes_surface_status_tints() {
        // Surface profile: `.muted()` keeps the neutral fill and quiets
        // the rim rather than fighting the resting look.
        let m = kbd("Del").muted();
        assert_eq!(m.fill, Some(tokens::MUTED));
        assert_eq!(m.stroke, Some(tokens::BORDER));

        let d = kbd("Del").destructive();
        assert_eq!(d.fill, Some(tokens::DESTRUCTIVE.with_alpha_u8(38)));
        // The tint's text-grade sibling, not the fill token — the
        // status tones split into fill/text pairs so tinted text
        // clears WCAG AA.
        assert_eq!(d.text_color, Some(tokens::DESTRUCTIVE_TINT_FOREGROUND));
    }

    #[test]
    fn kbd_group_is_a_flat_tight_row() {
        let g = kbd_group([kbd("Shift"), crate::text("+").caption(), kbd("Enter")]);

        assert_eq!(g.kind, Kind::Custom("kbd_group"));
        // Flat: the caps are direct children, in order, unwrapped.
        assert_eq!(g.children.len(), 3);
        assert_eq!(g.children[0].kind, Kind::Custom("kbd"));
        assert_eq!(g.children[0].text.as_deref(), Some("Shift"));
        assert_eq!(g.children[1].text.as_deref(), Some("+"));
        assert_eq!(g.children[2].text.as_deref(), Some("Enter"));

        // Structure only — no shell of its own.
        assert_eq!(g.axis, Axis::Row);
        assert_eq!(g.align, Align::Center);
        assert_eq!(g.gap, tokens::SPACE_1);
        assert_eq!(g.width, Size::Hug);
        assert_eq!(g.height, Size::Hug);
        assert!(g.fill.is_none(), "kbd_group paints nothing");
        assert!(g.stroke.is_none(), "kbd_group paints nothing");
        assert_eq!(g.padding, Sides::all(0.0));
    }
}
