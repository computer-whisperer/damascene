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

#[cfg(all(test, feature = "inter"))]
mod tests {
    use super::*;
    use crate::text::msdf_atlas::{MsdfAtlas, MsdfGlyphKey};
    use cosmic_text::fontdb;

    #[test]
    fn baked_atlas_covers_every_stock_inter_weight() {
        // The bake must cover the same weights the renderer's warm
        // path expects (Inter at 400/500/600/700), keyed exactly as
        // `GlyphAtlas::msdf_raster_weight` will ask for them — a
        // mismatch silently degrades every prebaked launch back to
        // live rasterization of headings and labels.
        let mut db = fontdb::Database::new();
        db.load_font_data(damascene_fonts::INTER_VARIABLE.to_vec());
        let font = db.faces().next().expect("Inter face").id;

        let mut atlas = MsdfAtlas::default();
        let packed = atlas
            .import_snapshot(DEFAULT_ATLAS, |t| (t == TOKEN_INTER).then_some(font))
            .expect("baked blob imports into a default atlas");
        assert!(packed > 0, "blob holds Inter glyphs");

        let face = ttf_parser::Face::parse(damascene_fonts::INTER_VARIABLE, 0).unwrap();
        let glyph_id = face.glyph_index('A').unwrap().0;
        for weight in [400u16, 500, 600, 700] {
            assert!(
                atlas
                    .slot(MsdfGlyphKey {
                        font,
                        glyph_id,
                        weight,
                    })
                    .is_some(),
                "'A' at wght {weight} must be resident after import"
            );
        }
    }
}
