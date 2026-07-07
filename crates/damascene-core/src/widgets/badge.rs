//! Badge — small status chip (rounded-md, per current shadcn).
//!
//! Default style is `info` (accent-tinted). Apply status modifiers from
//! [`crate::style`]: `.success()`, `.warning()`, `.destructive()`,
//! `.info()`, `.muted()`.

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
/// status modifier (`.success()`, `.warning()`, …) for other variants.
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
        .text_color(tokens::INFO)
        .fill(tokens::INFO.with_alpha_u8(38))
        .stroke(tokens::INFO.with_alpha_u8(120))
        .default_radius(BADGE_RADIUS)
        .width(Size::Hug)
        .default_height(Size::Fixed(20.0))
        .default_padding(Sides::xy(tokens::SPACE_2, 0.0))
}
