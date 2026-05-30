//! Avatar — compact identity chip for sidebars, tables, and activity rows.
//!
//! Start with initials for deterministic app chrome. Use `avatar_image`
//! when the app has raster bytes; it preserves the same size/radius shell
//! and clips the image to a circle.

use std::panic::Location;

use crate::image::{Image, ImageFit};
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

pub const DEFAULT_AVATAR_SIZE: f32 = 32.0;

#[track_caller]
pub fn avatar_initials(initials: impl Into<String>) -> El {
    El::new(Kind::Custom("avatar"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Raised)
        .text(initials)
        .text_align(TextAlign::Center)
        .caption()
        .font_weight(FontWeight::Semibold)
        .fill(tokens::ACCENT)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_PILL)
        .width(Size::Fixed(DEFAULT_AVATAR_SIZE))
        .height(Size::Fixed(DEFAULT_AVATAR_SIZE))
}

#[track_caller]
pub fn avatar_fallback(label: impl Into<String>) -> El {
    let label = label.into();
    avatar_initials(initials_from_label(&label)).at_loc(Location::caller())
}

#[track_caller]
pub fn avatar_image(img: impl Into<Image>) -> El {
    El::new(Kind::Custom("avatar"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Raised)
        .axis(Axis::Overlay)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_PILL)
        .width(Size::Fixed(DEFAULT_AVATAR_SIZE))
        .height(Size::Fixed(DEFAULT_AVATAR_SIZE))
        .clip()
        .child(
            image(img)
                .image_fit(ImageFit::Cover)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .radius(tokens::RADIUS_PILL),
        )
}

fn initials_from_label(label: &str) -> String {
    let mut chars = label
        .split_whitespace()
        .filter_map(|part| part.chars().find(|c| c.is_alphanumeric()));
    let Some(first) = chars.next() else {
        return "?".to_string();
    };
    let second = chars.next();
    match second {
        Some(second) => format!(
            "{}{}",
            first.to_uppercase().collect::<String>(),
            second.to_uppercase().collect::<String>()
        ),
        None => first.to_uppercase().collect::<String>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_initials_uses_fixed_circle_shell() {
        let a = avatar_initials("AK");

        assert_eq!(a.kind, Kind::Custom("avatar"));
        assert_eq!(a.text.as_deref(), Some("AK"));
        assert_eq!(a.text_role, TextRole::Caption);
        assert_eq!(a.font_weight, FontWeight::Semibold);
        assert_eq!(a.width, Size::Fixed(DEFAULT_AVATAR_SIZE));
        assert_eq!(a.height, Size::Fixed(DEFAULT_AVATAR_SIZE));
        assert_eq!(a.radius, crate::tree::Corners::all(tokens::RADIUS_PILL));
        assert_eq!(a.fill, Some(tokens::ACCENT));
    }

    #[test]
    fn avatar_fallback_derives_initials_from_words() {
        assert_eq!(avatar_fallback("Alicia Koch").text.as_deref(), Some("AK"));
        assert_eq!(avatar_fallback("olivia").text.as_deref(), Some("O"));
        assert_eq!(avatar_fallback("   ").text.as_deref(), Some("?"));
    }

    #[test]
    fn avatar_image_clips_cover_image_to_circle() {
        let img = Image::from_rgba8(2, 2, vec![255; 2 * 2 * 4]);
        let a = avatar_image(img);

        assert_eq!(a.kind, Kind::Custom("avatar"));
        assert_eq!(a.axis, Axis::Overlay);
        assert!(a.clip);
        assert_eq!(a.children.len(), 1);
        assert_eq!(a.children[0].kind, Kind::Image);
        assert_eq!(a.children[0].image_fit, ImageFit::Cover);
        assert_eq!(a.children[0].width, Size::Fill(1.0));
        assert_eq!(a.children[0].height, Size::Fill(1.0));
    }
}
