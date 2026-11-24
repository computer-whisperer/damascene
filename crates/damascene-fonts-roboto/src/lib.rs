//! Roboto bundled for Damascene.
//!
//! Each weight exposes a `pub static FOO: &[u8]` constant that
//! `damascene-fonts` re-exports when the `roboto` feature is enabled.
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

pub static ROBOTO_REGULAR: &[u8] = font_bytes!("../fonts/Roboto-Regular.ttf");
pub static ROBOTO_MEDIUM: &[u8] = font_bytes!("../fonts/Roboto-Medium.ttf");
pub static ROBOTO_BOLD: &[u8] = font_bytes!("../fonts/Roboto-Bold.ttf");
pub static ROBOTO_ITALIC: &[u8] = font_bytes!("../fonts/Roboto-Italic.ttf");
