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

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::{IntoIconSource, icon, text};

#[track_caller]
pub fn button(label: impl Into<String>) -> El {
    El::new(Kind::Custom("button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::Button)
        .surface_role(SurfaceRole::Raised)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Pointer)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .animate(Timing::SPRING_QUICK)
}

#[track_caller]
pub fn icon_button(source: impl IntoIconSource) -> El {
    El::new(Kind::Custom("icon_button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::IconButton)
        .surface_role(SurfaceRole::Raised)
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

#[track_caller]
pub fn button_with_icon(source: impl IntoIconSource, label: impl Into<String>) -> El {
    El::new(Kind::Custom("button_with_icon"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::Button)
        .surface_role(SurfaceRole::Raised)
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
        assert!(button("Save").animate.is_some());
        assert!(button("Save").primary().animate.is_some());
        assert!(icon_button("settings").animate.is_some());
        assert!(button_with_icon("folder", "Open").animate.is_some());
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
