//! Text shaping + atlas infrastructure. The unified RGBA glyph atlas
//! (color emoji + outline glyphs) lives in [`atlas`]; line measurement,
//! wrapping, and the `TextLayout` value backends consume live in
//! [`metrics`].
//!
//! The widget helpers (`h1`, `paragraph`, `text(...)`, …) live in
//! [`crate::widgets::text`] and compose against this module.

pub mod atlas;
pub(crate) mod inline_mixed;
pub mod metrics;
pub mod msdf;
pub mod msdf_atlas;
pub mod msdf_snapshot;
pub mod registry;

pub use registry::register_font;

/// Snap a logical-px coordinate to the nearest whole *physical* pixel.
///
/// Backends snap text **baselines** (and decoration-bar tops) with this
/// before emitting glyph quads: an unhinted MSDF glyph whose baseline
/// falls between device rows renders its horizontal features (baseline
/// stems, crossbars, the underline) as two half-covered rows, which
/// reads as per-line blur. Browsers snap baselines to device pixels for
/// the same reason. Horizontal placement stays fractional — X-snapping
/// would visibly un-even letter spacing, and vertical crispness doesn't
/// require it.
///
/// Non-positive `scale_factor` (defensive) returns `v` unchanged.
pub fn snap_to_physical(v: f32, scale_factor: f32) -> f32 {
    if scale_factor > 0.0 {
        (v * scale_factor).round() / scale_factor
    } else {
        v
    }
}
