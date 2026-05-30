//! Bundled font assets for Damascene.
//!
//! This crate is a thin re-exporter: each Cargo feature pulls in a
//! sibling crate that bundles the matching font family
//! (`damascene-fonts-inter`, `damascene-fonts-jetbrains-mono`, `damascene-fonts-roboto`,
//! `damascene-fonts-emoji`, `damascene-fonts-symbols`).
//! Splitting the families across crates keeps every published
//! `.crate` artifact under crates.io's per-crate upload size limit,
//! while the `damascene-fonts` API stays the same surface damascene-core (and
//! application code) consumes.
//!
//! The default feature set is generous (Inter + JetBrains Mono + emoji +
//! symbols) — chosen so that LLM output, which freely reaches for arrows,
//! math operators, dingbats, and box-drawing characters, doesn't render as
//! tofu (`◻`) out of the box, and so code samples land on a real
//! programmer's mono instead of the UI sans.
//!
//! # Feature flags
//!
//! | feature          | adds                                                  | size (raw)   |
//! |------------------|-------------------------------------------------------|--------------|
//! | `inter`          | Inter Variable Roman / Italic                         | ~1.8 MB      |
//! | `jetbrains-mono` | JetBrains Mono Variable Roman / Italic (with ligatures) | ~600 KB    |
//! | `roboto`         | Roboto Regular / Medium / Bold / Italic               | ~1.8 MB      |
//! | `emoji`          | NotoColorEmoji (CBDT color bitmaps)                   | ~10 MB       |
//! | `symbols`        | NotoSansSymbols2 + NotoSansMath                       | ~2.2 MB      |
//!
//! `default = ["default_fonts"]` and `default_fonts = ["inter",
//! "jetbrains-mono", "emoji", "symbols"]`. Roboto was previously bundled
//! by default; it is still available behind the `roboto` feature for
//! consumers that want the Material UI sans. To skip the bundled fonts
//! entirely (for example, to ship your own Material Symbols or a brand
//! typeface):
//!
//! ```toml
//! damascene-fonts = { version = "0.3", default-features = false }
//! ```
//!
//! CJK was previously available as an opt-in `cjk` feature shipping
//! NotoSansCJK SC (~16 MB). The bundled font pushed the published
//! `.crate` over crates.io's upload cap, so it has been removed for
//! this release; a bring-your-own-font path will return in a later
//! release. In the meantime, register a CJK face into damascene-core's
//! `fontdb` directly via the public text-atlas APIs.
//!
//! Color emoji (NotoColorEmoji) is rendered through damascene-core's
//! unified RGBA atlas — outline glyphs are stored as
//! `(255, 255, 255, alpha)` and color emoji as native RGBA, so backends
//! sample one texture format and run one shader path.
//!
//! # API
//!
//! Each enabled font family re-exports a `pub static FOO: &[u8]`
//! constant from its sibling crate. The [`DEFAULT_FONTS`] slice
//! collects every byte slice that is enabled in the current build,
//! in priority order (sans-serif text first, then symbol / emoji
//! fallbacks). damascene-core's atlas loads them all into its `fontdb`
//! so cosmic-text's per-codepoint fallback can pick from them.

#![no_std]

#[cfg(feature = "roboto")]
pub use damascene_fonts_roboto::{ROBOTO_BOLD, ROBOTO_ITALIC, ROBOTO_MEDIUM, ROBOTO_REGULAR};

#[cfg(feature = "inter")]
pub use damascene_fonts_inter::{INTER_VARIABLE, INTER_VARIABLE_ITALIC};

#[cfg(feature = "jetbrains-mono")]
pub use damascene_fonts_jetbrains_mono::{JETBRAINS_MONO_VARIABLE, JETBRAINS_MONO_VARIABLE_ITALIC};

#[cfg(feature = "emoji")]
pub use damascene_fonts_emoji::NOTO_COLOR_EMOJI;

#[cfg(feature = "symbols")]
pub use damascene_fonts_symbols::{NOTO_SANS_MATH_REGULAR, NOTO_SANS_SYMBOLS2_REGULAR};

/// Byte slices for every font enabled in the current build, in priority
/// order: text faces first, then symbol / emoji fallbacks.
///
/// damascene-core loads every entry into its `fontdb`. cosmic-text's font
/// matcher then walks the database per codepoint when a primary face
/// lacks a glyph — the order here only documents intent; cosmic-text's
/// fallback is keyed on Unicode coverage, not list position.
pub const DEFAULT_FONTS: &[&[u8]] = &[
    #[cfg(feature = "inter")]
    INTER_VARIABLE,
    #[cfg(feature = "inter")]
    INTER_VARIABLE_ITALIC,
    #[cfg(feature = "jetbrains-mono")]
    JETBRAINS_MONO_VARIABLE,
    #[cfg(feature = "jetbrains-mono")]
    JETBRAINS_MONO_VARIABLE_ITALIC,
    #[cfg(feature = "roboto")]
    ROBOTO_REGULAR,
    #[cfg(feature = "roboto")]
    ROBOTO_MEDIUM,
    #[cfg(feature = "roboto")]
    ROBOTO_BOLD,
    #[cfg(feature = "roboto")]
    ROBOTO_ITALIC,
    #[cfg(feature = "emoji")]
    NOTO_COLOR_EMOJI,
    #[cfg(feature = "symbols")]
    NOTO_SANS_SYMBOLS2_REGULAR,
    #[cfg(feature = "symbols")]
    NOTO_SANS_MATH_REGULAR,
];
