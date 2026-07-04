//! Progress — a non-interactive horizontal bar showing how full a
//! `0.0..=1.0` value is. Shaped like the shadcn / Radix Progress
//! primitive, scaled down to a single `progress(value)` builder
//! because Damascene progress bars don't need to advertise their
//! indeterminate or label-bearing state — apps compose those around
//! the bar.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Storage { used_pct: u32 }
//!
//! impl App for Storage {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             row([
//!                 text("Storage").label(),
//!                 spacer(),
//!                 text(format!("{}%", self.used_pct)).muted(),
//!             ]),
//!             progress(self.used_pct as f32 / 100.0),
//!         ])
//!     }
//! }
//! ```
//!
//! Progress paints the same way as the slider track + fill, minus the
//! thumb. There's no `apply_event` because the widget is read-only —
//! apps update the underlying value through whatever channel they
//! own (timer tick, async snapshot, computed metric, ...).

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::layout::LayoutCtx;
use crate::metrics::MetricsRole;
use crate::shader::{ShaderBinding, StockShader, UniformValue};
use crate::tokens;
use crate::tree::*;

/// Default bar height in logical pixels.
pub const DEFAULT_HEIGHT: f32 = 8.0;

/// A horizontal progress bar. `value` is clamped to `0.0..=1.0`; the
/// returned `El` defaults to filling its container's width and a
/// fixed [`DEFAULT_HEIGHT`]. Override with `.height(...)` /
/// `.width(...)` like any El.
///
/// The visible portion fills with [`tokens::PRIMARY`]; use
/// [`progress_with_color`] to vary it (e.g. `tokens::SUCCESS`, or
/// `tokens::DESTRUCTIVE` when the value crosses a "near full"
/// threshold).
#[track_caller]
pub fn progress(value: f32) -> El {
    progress_with_color(value, tokens::PRIMARY)
}

/// A [`progress`] bar whose visible portion uses `fill_color` instead
/// of the default [`tokens::PRIMARY`].
#[track_caller]
pub fn progress_with_color(value: f32, fill_color: Color) -> El {
    let value = value.clamp(0.0, 1.0);
    let layout = move |ctx: LayoutCtx| {
        let r = ctx.container;
        vec![
            // Track spans the full container.
            Rect::new(r.x, r.y, r.w, r.h),
            // Fill spans the portion proportional to value.
            Rect::new(r.x, r.y, r.w * value, r.h),
        ]
    };

    El::new(Kind::Custom("progress"))
        .axis(Axis::Overlay)
        .children([
            El::new(Kind::Custom("progress-track"))
                .fill(tokens::MUTED)
                .radius(tokens::RADIUS_PILL),
            El::new(Kind::Custom("progress-fill"))
                .fill(fill_color)
                .radius(tokens::RADIUS_PILL),
        ])
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::Progress)
        .layout(layout)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(DEFAULT_HEIGHT))
}

/// Indeterminate horizontal loader — same dimensions as
/// [`progress`], but with a small [`tokens::PRIMARY`] bar sliding
/// back and forth across a muted track on a continuous loop. Use this
/// in progress slots where no completion ratio is available
/// (uploading to a server that doesn't report bytes-sent, parsing a
/// stream of unknown length, etc.). The runtime keeps the host loop
/// ticking automatically while one is in the tree. Use
/// [`progress_indeterminate_with_color`] for a different bar color.
///
/// ```ignore
/// use damascene_core::prelude::*;
///
/// row([
///     text("Uploading…").label(),
///     spacer(),
///     progress_indeterminate().width(Size::Fixed(120.0)),
/// ])
/// ```
#[track_caller]
pub fn progress_indeterminate() -> El {
    progress_indeterminate_with_color(tokens::PRIMARY)
}

/// A [`progress_indeterminate`] loader whose sliding bar uses
/// `bar_color` instead of the default [`tokens::PRIMARY`].
#[track_caller]
pub fn progress_indeterminate_with_color(bar_color: Color) -> El {
    let binding = ShaderBinding::stock(StockShader::ProgressIndeterminate)
        .with("vec_a", UniformValue::Color(bar_color))
        .with("vec_b", UniformValue::Color(tokens::MUTED))
        // vec_c.x = radius (0 = default 4px; for a pill at 8px height we want PILL)
        // vec_c.y = period seconds (0 = default 1.6)
        // vec_c.z = bar width as fraction of track (0 = default 0.35)
        // vec_c.w unused
        .with(
            "vec_c",
            UniformValue::Vec4([tokens::RADIUS_PILL, 0.0, 0.0, 0.0]),
        );

    El::new(Kind::Custom("progress-indeterminate"))
        .at_loc(Location::caller())
        .shader(binding)
        .metrics_role(MetricsRole::Progress)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(DEFAULT_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_and_fill_use_expected_tokens() {
        let p = progress(0.5);
        assert_eq!(p.children.len(), 2);
        assert_eq!(p.children[0].fill, Some(tokens::MUTED), "track is muted");
        assert_eq!(
            p.children[1].fill,
            Some(tokens::PRIMARY),
            "fill uses caller's color"
        );
        // Both rounded pills so the bar reads as one piece.
        assert_eq!(
            p.children[0].radius,
            crate::tree::Corners::all(tokens::RADIUS_PILL)
        );
        assert_eq!(
            p.children[1].radius,
            crate::tree::Corners::all(tokens::RADIUS_PILL)
        );
    }

    #[test]
    fn layout_clamps_value_below_zero() {
        // The visible result of a clamped value is the fill rect's
        // width, so verify the layout closure end-to-end.
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = progress(-0.5);
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT);
        layout(&mut tree, &mut state, viewport);
        let fill_rect = tree.children[1].computed_rect;
        assert_eq!(fill_rect.w, 0.0, "negative values clamp to empty fill");
    }

    #[test]
    fn layout_clamps_value_above_one() {
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = progress(1.5);
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT);
        layout(&mut tree, &mut state, viewport);
        let fill_rect = tree.children[1].computed_rect;
        assert_eq!(fill_rect.w, 200.0, "values above 1.0 clamp to full track");
    }

    #[test]
    fn indeterminate_binds_stock_shader() {
        use crate::shader::ShaderHandle;
        let p = progress_indeterminate();
        let binding = p.shader_override.as_ref().expect("shader binding");
        assert_eq!(
            binding.handle,
            ShaderHandle::Stock(StockShader::ProgressIndeterminate),
            "progress_indeterminate must paint through stock::progress_indeterminate",
        );
        match binding.uniforms.get("vec_a") {
            Some(UniformValue::Color(c)) => assert_eq!(*c, tokens::PRIMARY),
            other => panic!("expected vec_a=PRIMARY, got {other:?}"),
        }
    }

    #[test]
    fn indeterminate_inherits_progress_dimensions() {
        let p = progress_indeterminate();
        assert_eq!(p.width, Size::Fill(1.0));
        assert_eq!(p.height, Size::Fixed(DEFAULT_HEIGHT));
    }

    #[test]
    fn layout_fills_proportionally_to_value() {
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = progress(0.25);
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT);
        layout(&mut tree, &mut state, viewport);
        let fill_rect = tree.children[1].computed_rect;
        assert!(
            (fill_rect.w - 50.0).abs() < 1e-3,
            "0.25 * 200 = 50; got {}",
            fill_rect.w
        );
    }
}
