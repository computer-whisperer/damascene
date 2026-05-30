//! NotoColorEmoji bundled for Damascene.
//!
//! `NOTO_COLOR_EMOJI` exposes the CBDT color-bitmap font that
//! `damascene-fonts` re-exports when the `emoji` feature is enabled.
//! Most consumers should depend on `damascene-fonts` (with the default
//! features) rather than this crate directly.
//!
//! Color rendering needs damascene-core's RGBA atlas path; if you load
//! this directly into a non-damascene `fontdb`, color glyphs render as
//! B&W silhouettes.

#![no_std]

pub static NOTO_COLOR_EMOJI: &[u8] = include_bytes!("../fonts/NotoColorEmoji.ttf");
