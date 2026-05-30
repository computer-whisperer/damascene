# damascene-fonts-symbols

NotoSans Symbols2 + NotoSans Math bundled for Damascene — the arrows / math / dingbats / box-drawing fallback set.

Most consumers should depend on `damascene-fonts` (with the `symbols` feature, which is on by default) rather than this crate directly. This crate exists so the published `.crate` artifact for each font family stays under crates.io's per-crate upload size limit; `damascene-fonts` re-exports the constants when the matching feature is enabled.
