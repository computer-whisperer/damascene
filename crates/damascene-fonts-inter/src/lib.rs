//! Inter Variable bundled for Damascene.
//!
//! Each face exposes a `pub static FOO: &[u8]` constant that
//! `damascene-fonts` re-exports when the `inter` feature is enabled.

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

pub static INTER_VARIABLE: &[u8] = font_bytes!("../fonts/InterVariable.ttf");
pub static INTER_VARIABLE_ITALIC: &[u8] = font_bytes!("../fonts/InterVariable-Italic.ttf");
