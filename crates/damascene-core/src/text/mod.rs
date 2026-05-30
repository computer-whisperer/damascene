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
