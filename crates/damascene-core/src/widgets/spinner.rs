//! Spinner — indeterminate loading indicator.
//!
//! Mirrors egui's spinner motion: a `start` anchor rotates
//! continuously while an `end` anchor swings around it on a cosine
//! envelope, so the visible arc grows, shrinks, reverses, and
//! rotates all at once. Communicates "work in progress, no
//! determinate ratio." For a determinate bar, use [`progress`]
//! instead — apps choose the right primitive at the call site.
//!
//! By default there is no track ring — the off region of the circle
//! is fully transparent. Reach for [`spinner_with_track`] when a
//! contrasting low-alpha rest ring helps the spinner read against
//! its surface.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! row([
//!     spinner(),
//!     text("Loading…").muted(),
//! ])
//! .gap(tokens::SPACE_2)
//! .align(Align::Center)
//! ```
//!
//! Size with the standard `ComponentSize` modifiers (`.small()`,
//! `.large()`) or by setting an explicit `width`/`height`. Color
//! comes from a `fill` token — pass `tokens::PRIMARY` for an action
//! spinner, `tokens::DESTRUCTIVE` for a retrying error path. A
//! contrasting track is derived automatically; override it with
//! [`spinner_with_track`] when the default doesn't read.
//!
//! Unlike the rest of the catalog, spinner paints through a stock
//! WGSL shader (`stock::spinner`) that reads `frame.time` to drive
//! its phase. The runtime detects spinner draws in the resolved op
//! list and forces `needs_redraw=true` so the host idle loop keeps
//! ticking — apps don't manage a timer for it.
//!
//! [`progress`]: super::progress::progress

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::shader::{ShaderBinding, StockShader, UniformValue};
use crate::tokens;
use crate::tree::*;

/// Default outer diameter, matching `tokens::ICON_SM` so a spinner
/// drops into icon slots (button leading, sidebar items) without
/// extra sizing.
pub const DEFAULT_SIZE: f32 = 16.0;

/// Indeterminate loading spinner, sized to drop into icon slots.
/// Paints a 270° arc rotating around a dim track.
#[track_caller]
pub fn spinner() -> El {
    spinner_with_color(tokens::FOREGROUND)
}

/// Like [`spinner`], but with an explicit arc color. No track ring —
/// the off region of the circle stays fully transparent. Pair with
/// [`spinner_with_track`] when a rest ring is wanted.
#[track_caller]
pub fn spinner_with_color(arc: Color) -> El {
    spinner_with_track(arc, arc.with_alpha_u8(0))
}

/// Like [`spinner`], but with explicit arc and track colors. Reach
/// for this when an opaque-ish rest ring helps the spinner read
/// against its surface (e.g. a low-contrast tinted backdrop).
#[track_caller]
pub fn spinner_with_track(arc: Color, track: Color) -> El {
    let binding = ShaderBinding::stock(StockShader::Spinner)
        .with("vec_a", UniformValue::Color(arc))
        .with("vec_b", UniformValue::Color(track))
        // vec_c.x = thickness override (0 = derive from diameter)
        // vec_c.y = max sweep in radians (0 = default 240°)
        // vec_c.z = head angular speed in rad/s (0 = default 4.19)
        // vec_c.w = pulse angular speed in rad/s (0 = default 1.0)
        .with("vec_c", UniformValue::Vec4([0.0, 0.0, 0.0, 0.0]));

    El::new(Kind::Custom("spinner"))
        .at_loc(Location::caller())
        .shader(binding)
        .default_width(Size::Fixed(DEFAULT_SIZE))
        .default_height(Size::Fixed(DEFAULT_SIZE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::{ShaderHandle, UniformValue};

    #[test]
    fn spinner_binds_stock_spinner_shader() {
        let s = spinner();
        let binding = s.shader_override.as_ref().expect("shader binding");
        assert_eq!(
            binding.handle,
            ShaderHandle::Stock(StockShader::Spinner),
            "spinner widget must paint through stock::spinner"
        );
    }

    #[test]
    fn spinner_packs_color_into_vec_a() {
        let s = spinner_with_color(tokens::PRIMARY);
        let binding = s.shader_override.as_ref().unwrap();
        match binding.uniforms.get("vec_a") {
            Some(UniformValue::Color(c)) => assert_eq!(*c, tokens::PRIMARY),
            other => panic!("expected vec_a=PRIMARY color, got {other:?}"),
        }
    }

    #[test]
    fn spinner_default_size_matches_icon_sm() {
        let s = spinner();
        assert_eq!(s.width, Size::Fixed(DEFAULT_SIZE));
        assert_eq!(s.height, Size::Fixed(DEFAULT_SIZE));
        assert_eq!(
            DEFAULT_SIZE,
            tokens::ICON_SM,
            "spinner default size should match icon-sm so it drops into icon slots"
        );
    }

    #[test]
    fn spinner_kind_is_custom_spinner() {
        let s = spinner();
        assert_eq!(s.kind, Kind::Custom("spinner"));
    }

    #[test]
    fn spinner_default_track_is_fully_transparent() {
        // Egui-style: the off region of the spinner is invisible. A
        // rest ring is opt-in via spinner_with_track.
        let s = spinner_with_color(tokens::PRIMARY);
        let binding = s.shader_override.as_ref().unwrap();
        match binding.uniforms.get("vec_b") {
            Some(UniformValue::Color(c)) => {
                assert_eq!(c.a, 0.0, "default track must be fully transparent");
            }
            other => panic!("expected vec_b track color, got {other:?}"),
        }
    }

    #[test]
    fn spinner_with_track_uses_explicit_track() {
        let s = spinner_with_track(tokens::SUCCESS, tokens::MUTED);
        let binding = s.shader_override.as_ref().unwrap();
        assert_eq!(
            binding.uniforms.get("vec_b"),
            Some(&UniformValue::Color(tokens::MUTED))
        );
    }
}
