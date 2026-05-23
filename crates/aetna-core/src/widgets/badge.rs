//! Badge — small status pill.
//!
//! Default style is `info` (accent-tinted). Apply status modifiers from
//! [`crate::style`]: `.success()`, `.warning()`, `.destructive()`,
//! `.info()`, `.muted()`.

use std::panic::Location;

use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

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
        .default_radius(tokens::RADIUS_PILL)
        .width(Size::Hug)
        .default_height(Size::Fixed(20.0))
        .default_padding(Sides::xy(tokens::SPACE_2, 0.0))
}
