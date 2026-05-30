# damascene-fonts-emoji

NotoColorEmoji (CBDT color bitmaps) bundled for Damascene.

Most consumers should depend on `damascene-fonts` (with the `emoji` feature, which is on by default) rather than this crate directly. This crate exists so the published `.crate` artifact for each font family stays under crates.io's per-crate upload size limit; `damascene-fonts` re-exports the byte slice when the matching feature is enabled.

Color rendering requires damascene-core's RGBA atlas path. Loading this directly into a non-damascene `fontdb` will render color glyphs as B&W silhouettes.
