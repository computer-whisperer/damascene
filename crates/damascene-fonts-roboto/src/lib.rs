//! Roboto bundled for Damascene.
//!
//! Each weight exposes a `pub static FOO: &[u8]` constant that
//! `damascene-fonts` re-exports when the `roboto` feature is enabled.
//! Most consumers should depend on `damascene-fonts` (with the default
//! features) rather than this crate directly.

#![no_std]

pub static ROBOTO_REGULAR: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");
pub static ROBOTO_MEDIUM: &[u8] = include_bytes!("../fonts/Roboto-Medium.ttf");
pub static ROBOTO_BOLD: &[u8] = include_bytes!("../fonts/Roboto-Bold.ttf");
pub static ROBOTO_ITALIC: &[u8] = include_bytes!("../fonts/Roboto-Italic.ttf");
