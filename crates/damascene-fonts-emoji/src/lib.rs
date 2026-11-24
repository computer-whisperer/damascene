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

/// Embeds a font file as a named static and evaluates to a reference to
/// it. Plain `include_bytes!` data has no symbol, so a crate that copies
/// the `&[u8]` at compile time (as `damascene_fonts::DEFAULT_FONTS` does)
/// emits its own copy of the whole file; a named static is emitted once
/// and linked to (issue #156).
macro_rules! font_bytes {
    ($path:literal) => {{
        static BYTES: [u8; include_bytes!($path).len()] = *include_bytes!($path);
        &BYTES
    }};
}

pub static NOTO_COLOR_EMOJI: &[u8] = font_bytes!("../fonts/NotoColorEmoji.ttf");
