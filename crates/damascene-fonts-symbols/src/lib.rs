//! NotoSans Symbols2 + Math bundled for Damascene.
//!
//! These fallback faces cover arrows, math operators, dingbats, and
//! box-drawing — the characters LLM-generated UI freely reaches for
//! that the Latin Roboto bundle doesn't carry. `damascene-fonts`
//! re-exports both constants when the `symbols` feature is enabled.
//! Most consumers should depend on `damascene-fonts` (with the default
//! features) rather than this crate directly.

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

pub static NOTO_SANS_SYMBOLS2_REGULAR: &[u8] = font_bytes!("../fonts/NotoSansSymbols2-Regular.ttf");
pub static NOTO_SANS_MATH_REGULAR: &[u8] = font_bytes!("../fonts/NotoSansMath-Regular.ttf");
