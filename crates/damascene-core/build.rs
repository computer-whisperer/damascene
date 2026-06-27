//! Compile-time bake of the default-font MSDF atlas.
//!
//! When the `prebaked-default-fonts` feature is on, this bakes the
//! printable-ASCII MTSDF for the bundled fonts and writes a snapshot blob
//! to `$OUT_DIR/default_atlas.bin`, which the library embeds via
//! `include_bytes!` (see `crate::prebaked`). The backend's warmup then
//! loads it instead of regenerating ~190 glyphs at startup.
//!
//! The bake reuses the library's own MSDF generator and snapshot codec by
//! `#[path]`-including the two leaf modules, so there is no second copy of
//! the algorithm to drift. It reruns whenever those sources, the fonts, or
//! this script change — the blob is always fresh.

use std::path::PathBuf;

// Share the leaf MSDF + snapshot modules with the library so the bake
// uses the identical generator + codec (no drift). Declared flat at the
// build script's crate root: `#[path]` resolves relative to this file's
// directory, and `msdf_snapshot`'s `use super::msdf` resolves because
// both modules are siblings at the root here, as in the library.
// The library exercises every item in these modules; the build script
// uses only the encode/bake half, so silence dead-code in this context.
#[path = "src/text/msdf.rs"]
#[allow(dead_code)]
mod msdf;
#[path = "src/text/msdf_snapshot.rs"]
#[allow(dead_code)]
mod msdf_snapshot;

use msdf_snapshot::{SnapshotHeader, SnapshotSection, bake_font_section, encode_snapshot};

// Must match DEFAULT_BASE_EM / DEFAULT_SPREAD in src/text/msdf_atlas.rs
// and the warm path's error-correction setting. A mismatch is *safe* —
// the runtime import rejects a snapshot whose params differ and falls
// back to live generation — so this is a soft contract (you lose the
// speedup, never correctness) rather than something that can go wrong
// silently.
const BASE_EM: u32 = 48;
const SPREAD: f64 = 6.0;
const ERROR_CORRECTION: bool = false;

// Font tokens — must match the constants in `crate::prebaked`.
const TOKEN_INTER: u64 = 0;
const TOKEN_JETBRAINS_MONO: u64 = 1;

fn main() {
    println!("cargo:rerun-if-changed=src/text/msdf.rs");
    println!("cargo:rerun-if-changed=src/text/msdf_snapshot.rs");
    println!("cargo:rerun-if-changed=build.rs");

    // Nothing to embed unless the feature is on (the library only
    // `include_bytes!`s the blob under the same feature).
    if std::env::var_os("CARGO_FEATURE_PREBAKED_DEFAULT_FONTS").is_none() {
        return;
    }

    let out =
        PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("default_atlas.bin");
    let chars: Vec<char> = (0x20u32..=0x7Eu32).filter_map(char::from_u32).collect();
    let mut sections: Vec<SnapshotSection> = Vec::new();

    // Bake only the fonts whose features the consumer enabled, so the
    // blob never carries glyphs the runtime fontdb won't have.
    if std::env::var_os("CARGO_FEATURE_INTER").is_some() {
        let face = ttf_parser::Face::parse(damascene_fonts::INTER_VARIABLE, 0)
            .expect("parse bundled Inter");
        sections.push(bake_font_section(
            &face,
            TOKEN_INTER,
            &chars,
            BASE_EM,
            SPREAD,
            ERROR_CORRECTION,
        ));
    }
    if std::env::var_os("CARGO_FEATURE_JETBRAINS_MONO").is_some() {
        let face = ttf_parser::Face::parse(damascene_fonts::JETBRAINS_MONO_VARIABLE, 0)
            .expect("parse bundled JetBrains Mono");
        sections.push(bake_font_section(
            &face,
            TOKEN_JETBRAINS_MONO,
            &chars,
            BASE_EM,
            SPREAD,
            ERROR_CORRECTION,
        ));
    }

    let header = SnapshotHeader {
        base_em: BASE_EM,
        spread: SPREAD,
        error_correction: ERROR_CORRECTION,
    };
    let bytes = encode_snapshot(header, &sections);
    std::fs::write(&out, &bytes).expect("write default_atlas.bin");
}
