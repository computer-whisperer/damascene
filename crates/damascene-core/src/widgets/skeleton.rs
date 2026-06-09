//! Skeleton — pulsing loading placeholder.
//!
//! Mirrors shadcn's `animate-pulse`: a rounded muted block whose
//! alpha breathes 0.5 → 1.0 → 0.5 over 2 s, communicating "content
//! coming, layout reserved." The shader reads `frame.time`; the
//! runtime keeps the host loop ticking automatically while a
//! skeleton is in the tree.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! column([
//!     skeleton().width(Size::Fixed(220.0)),
//!     skeleton().height(Size::Fixed(80.0)),
//! ])
//! ```
//!
//! Use [`skeleton_circle`] for avatar placeholders. Override the
//! base color with [`skeleton_with_color`] when the surface
//! underneath isn't muted-friendly.
//!
//! In Settled-mode fixtures (headless render binaries) the cosine
//! envelope pins to its peak so PNG snapshots show the skeleton at
//! its most readable phase.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::shader::{ShaderBinding, StockShader, UniformValue};
use crate::tokens;
use crate::tree::*;

/// Default radius for rectangular placeholders. Matches the rounded
/// rect the rest of the catalog uses for grouped content blocks.
pub const DEFAULT_RADIUS: f32 = tokens::RADIUS_MD;

/// Default height for a single-line placeholder, sized to the body
/// text rhythm so a row of skeletons reads as a paragraph stub.
pub const DEFAULT_HEIGHT: f32 = 16.0;

/// Pulsing loading placeholder — full-width, one text line tall
/// ([`DEFAULT_HEIGHT`]) in [`tokens::MUTED`]; override with `.width(...)`
/// / `.height(...)`.
#[track_caller]
pub fn skeleton() -> El {
    skeleton_with_color(tokens::MUTED)
}

/// Skeleton with an explicit base color — handy when the muted token
/// doesn't have enough contrast against a tinted card / sheet.
#[track_caller]
pub fn skeleton_with_color(base: Color) -> El {
    skeleton_shape(base, DEFAULT_RADIUS)
        .default_width(Size::Fill(1.0))
        .default_height(Size::Fixed(DEFAULT_HEIGHT))
}

/// Round skeleton (avatar placeholder). `size` sets both the width
/// and height in logical pixels.
#[track_caller]
pub fn skeleton_circle(size: f32) -> El {
    skeleton_shape(tokens::MUTED, size * 0.5)
        .width(Size::Fixed(size))
        .height(Size::Fixed(size))
}

#[track_caller]
fn skeleton_shape(base: Color, radius: f32) -> El {
    let binding = ShaderBinding::stock(StockShader::Skeleton)
        .with("vec_a", UniformValue::Color(base))
        // vec_c.x = radius (0 = default 6px)
        // vec_c.y = pulse period seconds (0 = default 2.0)
        // vec_c.z = min alpha multiplier (0 = default 0.5)
        // vec_c.w = max alpha multiplier (0 = default 1.0)
        .with("vec_c", UniformValue::Vec4([radius, 0.0, 0.0, 0.0]));

    El::new(Kind::Custom("skeleton"))
        .at_loc(Location::caller())
        .shader(binding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::ShaderHandle;

    #[test]
    fn skeleton_binds_stock_skeleton_shader() {
        let s = skeleton();
        let binding = s.shader_override.as_ref().expect("shader binding");
        assert_eq!(
            binding.handle,
            ShaderHandle::Stock(StockShader::Skeleton),
            "skeleton must paint through stock::skeleton",
        );
    }

    #[test]
    fn skeleton_packs_color_into_vec_a() {
        let s = skeleton_with_color(tokens::PRIMARY);
        let binding = s.shader_override.as_ref().unwrap();
        match binding.uniforms.get("vec_a") {
            Some(UniformValue::Color(c)) => assert_eq!(*c, tokens::PRIMARY),
            other => panic!("expected vec_a=PRIMARY color, got {other:?}"),
        }
    }

    #[test]
    fn skeleton_circle_sets_equal_axes_and_pill_radius() {
        let s = skeleton_circle(32.0);
        assert_eq!(s.width, Size::Fixed(32.0));
        assert_eq!(s.height, Size::Fixed(32.0));
        // Half the size is fully rounded — pill on a square = circle.
        let binding = s.shader_override.as_ref().unwrap();
        match binding.uniforms.get("vec_c") {
            Some(UniformValue::Vec4(v)) => assert!(
                (v[0] - 16.0).abs() < f32::EPSILON,
                "skeleton_circle radius should be size/2, got {}",
                v[0]
            ),
            other => panic!("expected vec_c with radius, got {other:?}"),
        }
    }

    #[test]
    fn skeleton_default_size_is_full_width_one_line() {
        let s = skeleton();
        assert_eq!(s.width, Size::Fill(1.0));
        assert_eq!(s.height, Size::Fixed(DEFAULT_HEIGHT));
    }

    #[test]
    fn skeleton_kind_is_custom_skeleton() {
        let s = skeleton();
        assert_eq!(s.kind, Kind::Custom("skeleton"));
    }
}
