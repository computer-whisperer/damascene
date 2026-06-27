//! Compile-time-baked default-font MSDF atlas.
//!
//! Present only under the `prebaked-default-fonts` feature. `build.rs`
//! bakes the bundled fonts' printable-ASCII MTSDF into [`DEFAULT_ATLAS`];
//! a backend's warmup imports it via
//! [`crate::text::msdf_atlas::MsdfAtlas::import_snapshot`], mapping the
//! token constants here to runtime `fontdb::ID`s, instead of
//! regenerating the glyphs at startup.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

/// The baked snapshot blob (format defined in
/// [`crate::text::msdf_snapshot`]). Empty of glyph sections if no bundled
/// font feature was enabled at build time; the staleness guard still lets
/// `import_snapshot` reject it cleanly into live generation.
pub const DEFAULT_ATLAS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/default_atlas.bin"));

/// Font token for Inter within [`DEFAULT_ATLAS`].
pub const TOKEN_INTER: u64 = 0;
/// Font token for JetBrains Mono within [`DEFAULT_ATLAS`].
pub const TOKEN_JETBRAINS_MONO: u64 = 1;
