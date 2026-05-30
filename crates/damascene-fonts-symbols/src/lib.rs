//! NotoSans Symbols2 + Math bundled for Damascene.
//!
//! These fallback faces cover arrows, math operators, dingbats, and
//! box-drawing — the characters LLM-generated UI freely reaches for
//! that the Latin Roboto bundle doesn't carry. `damascene-fonts`
//! re-exports both constants when the `symbols` feature is enabled.
//! Most consumers should depend on `damascene-fonts` (with the default
//! features) rather than this crate directly.

#![no_std]

pub static NOTO_SANS_SYMBOLS2_REGULAR: &[u8] =
    include_bytes!("../fonts/NotoSansSymbols2-Regular.ttf");
pub static NOTO_SANS_MATH_REGULAR: &[u8] = include_bytes!("../fonts/NotoSansMath-Regular.ttf");
