//! JetBrains Mono Variable bundled for Damascene.
//!
//! Each face exposes a `pub static FOO: &[u8]` constant that
//! `damascene-fonts` re-exports when the `jetbrains-mono` feature is enabled.
//! The variable fonts ship with the upstream programming ligature set
//! intact; consumers that want plain monospace can disable `liga` at
//! shaping time rather than picking a different font file.

#![no_std]

pub static JETBRAINS_MONO_VARIABLE: &[u8] = include_bytes!("../fonts/JetBrainsMonoVariable.ttf");
pub static JETBRAINS_MONO_VARIABLE_ITALIC: &[u8] =
    include_bytes!("../fonts/JetBrainsMonoVariable-Italic.ttf");
