//! Inter Variable bundled for Damascene.
//!
//! Each face exposes a `pub static FOO: &[u8]` constant that
//! `damascene-fonts` re-exports when the `inter` feature is enabled.

#![no_std]

pub static INTER_VARIABLE: &[u8] = include_bytes!("../fonts/InterVariable.ttf");
pub static INTER_VARIABLE_ITALIC: &[u8] = include_bytes!("../fonts/InterVariable-Italic.ttf");
