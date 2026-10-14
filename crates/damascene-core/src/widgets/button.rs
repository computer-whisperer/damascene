//! Button component.
//!
//! Default `button("Save")` is the secondary style. Apply variants from
//! [`crate::style`] to opt into others:
//!
//! - `.primary()` — filled accent color, semibold text.
//! - `.secondary()` — secondary surface (the default look).
//! - `.ghost()` — no fill, no border, muted text.
//! - `.outline()` — outline-only.
//! - `.destructive()` — solid red, contrasting text.
//!
//! Buttons hug their text width and default to [`tokens::CONTROL_HEIGHT`]
//! — the same height used by `select`, `text_input`, and tab triggers,
//! so they line up in form rows. Override `.width(Size::Fill(1.0))` to
//! stretch; the label stays horizontally centered.
//!
//! # Dogfood note
//!
//! This builder uses only the public widget-author surface — `Kind::Custom`
//! for the inspector tag, `.focusable()` to opt into the focus ring,
//! `.paint_overflow()` to give the ring somewhere to render, and
//! `.text_align(TextAlign::Center)` to center the label. An app crate
//! can write an equivalent button against the same API; nothing here
//! reaches into library internals. See `widget_kit.md`.

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
use crate::{IntoIconSource, icon, text};

/// Text button (HTML `<button>`) in the secondary style — chain
/// `.primary()`, `.ghost()`, `.outline()`, or `.destructive()` for
/// other variants, and `.key(...)` to receive `Click` events. Hugs its
/// label at the shared control height so it lines up in form rows.
#[track_caller]
pub fn button(label: impl Into<String>) -> El {
    El::new(Kind::Custom("button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::Button)
        .surface_role(SurfaceRole::Raised)
        .role(Role::Button)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        // Center-aligned nowrap text clips symmetrically when squeezed
        // (e.g. a `Fill`-width button in an overfull row) — degrade to an
        // ellipsized stub instead of a blank surface (issue #117).
        .ellipsis()
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .animate(Timing::SPRING_QUICK)
}

/// Square icon-only button at the shared control height — same
/// variants and event contract as [`button`].
///
/// # Labelling an icon-only control
///
/// The glyph is the whole control, so nothing here carries the
/// button's meaning as text. Give it both halves:
///
/// ```ignore
/// icon_button("terminal").key("run").aria_label("Run").tooltip("Run (F5)")
/// ```
///
/// - [`El::aria_label`][method@crate::El::aria_label] — the accessible
///   name. Unconditional, no layout or timing behavior, no requirement
///   on your root, read by the AccessKit bridge, and printed in the
///   tree dump so headless review can tell the buttons apart.
/// - `.tooltip(...)` — the hover affordance. Requires a `.key(...)`
///   on this node *and* an overlay root (`overlays(root, [])`); see
///   [`El::tooltip`][method@crate::El::tooltip] for the full contract. If a
///   shell has no overlay root yet, add one — dropping the label is
///   the worse trade.
///
/// For a compact strip — a title bar, status bar, pane header, or
/// toolbar 22–30 px tall — chain
/// `.size(ComponentSize::Xxs)`
/// ([`ComponentSize::Xxs`][crate::metrics::ComponentSize::Xxs]) rather
/// than hardcoding a height. That is the ladder's chrome rung: it
/// stamps a 22 px square that still clears a 30 px strip with the 2 px
/// focus ring on both sides. The default `Md` (36 px) and even `Xs`
/// (28 px) do not fit one.
///
/// ```
/// use damascene_core::prelude::*;
///
/// // A pane-header action strip: 22 px buttons in a 22 px header.
/// let actions = row([
///     icon_button("eye").ghost().size(ComponentSize::Xxs),
///     icon_button("more-horizontal").ghost().size(ComponentSize::Xxs),
/// ])
/// .gap(tokens::SPACE_1);
/// ```
///
/// The 22 px measurement is pinned by
/// `metrics::tests::an_xxs_icon_button_stamps_a_22px_square`.
#[track_caller]
pub fn icon_button(source: impl IntoIconSource) -> El {
    El::new(Kind::Custom("icon_button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::IconButton)
        .surface_role(SurfaceRole::Raised)
        .role(Role::Button)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .icon_source(source)
        .icon_size(tokens::ICON_SM)
        .icon_stroke_width(2.0)
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .animate(Timing::SPRING_QUICK)
}

/// [`button`] with a leading icon before the label — same variants and
/// event contract.
#[track_caller]
pub fn button_with_icon(source: impl IntoIconSource, label: impl Into<String>) -> El {
    El::new(Kind::Custom("button_with_icon"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::Button)
        .surface_role(SurfaceRole::Raised)
        .role(Role::Button)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_2)
        .align(Align::Center)
        .justify(Justify::Center)
        .child(
            icon(source)
                .icon_size(tokens::ICON_SM)
                .color(tokens::SECONDARY_FOREGROUND),
        )
        .child(text(label).label())
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .animate(Timing::SPRING_QUICK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_ease_variant_changes() {
        assert!(button("Save").animate_timing().is_some());
        assert!(button("Save").primary().animate_timing().is_some());
        assert!(icon_button("settings").animate_timing().is_some());
        assert!(
            button_with_icon("folder", "Open")
                .animate_timing()
                .is_some()
        );
    }

    #[test]
    fn buttons_have_conservative_default_hit_overflow() {
        assert_eq!(
            button("Save").hit_overflow,
            Sides::all(tokens::HIT_OVERFLOW)
        );
        assert_eq!(
            icon_button("settings").hit_overflow,
            Sides::all(tokens::HIT_OVERFLOW)
        );
        assert_eq!(
            button_with_icon("folder", "Open").hit_overflow,
            Sides::all(tokens::HIT_OVERFLOW)
        );
    }

    #[test]
    fn button_with_icon_icon_size_does_not_collapse_outer() {
        // Regression for GH #29: `.icon_size(...)` on a `button_with_icon`
        // used to overwrite the outer width/height to `Fixed(icon_size)`,
        // collapsing the chip to a tiny square.
        let el = button_with_icon("folder", "Open").icon_size(tokens::ICON_XS);
        assert!(
            !matches!(el.width, Size::Fixed(s) if (s - tokens::ICON_XS).abs() < f32::EPSILON),
            "outer width should not collapse to ICON_XS, got {:?}",
            el.width,
        );
        assert!(
            !matches!(el.height, Size::Fixed(s) if (s - tokens::ICON_XS).abs() < f32::EPSILON),
            "outer height should not collapse to ICON_XS, got {:?}",
            el.height,
        );
    }

    #[test]
    fn button_with_icon_icon_size_propagates_to_icon_child() {
        let el = button_with_icon("folder", "Open").icon_size(tokens::ICON_XS);
        let icon_child = el
            .children
            .iter()
            .find(|c| matches!(&c.kind, Kind::Custom(n) if *n == "icon"))
            .expect("button_with_icon has an icon child");
        assert!((icon_child.font_size - tokens::ICON_XS).abs() < f32::EPSILON);
        assert!((icon_child.line_height - tokens::ICON_XS).abs() < f32::EPSILON);
        assert_eq!(icon_child.width, Size::Fixed(tokens::ICON_XS));
        assert_eq!(icon_child.height, Size::Fixed(tokens::ICON_XS));
    }
}
