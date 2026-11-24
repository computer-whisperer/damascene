//! JetBrains Mono Variable bundled for Damascene.
//!
//! Each face exposes a `pub static FOO: &[u8]` constant that
//! `damascene-fonts` re-exports when the `jetbrains-mono` feature is enabled.
//! The variable fonts ship with the upstream programming ligature set
//! intact; consumers that want plain monospace can disable `liga` at
//! shaping time rather than picking a different font file.

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

pub static JETBRAINS_MONO_VARIABLE: &[u8] = font_bytes!("../fonts/JetBrainsMonoVariable.ttf");
pub static JETBRAINS_MONO_VARIABLE_ITALIC: &[u8] =
    font_bytes!("../fonts/JetBrainsMonoVariable-Italic.ttf");
