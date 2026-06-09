//! Process-global registry of host-supplied fonts.
//!
//! Damascene shapes text in two places, with two `FontSystem`s: the
//! per-thread layout/measurement system in [`super::metrics`] and the
//! paint-side system owned by each backend's
//! [`GlyphAtlas`](super::atlas::GlyphAtlas). A font registered here is
//! visible to both — [`register_font`] is the single public entry
//! point, and each `FontSystem` syncs lazily before shaping. Without
//! this split-brain guard, a host-registered CJK or brand face would
//! paint correctly while wrap points, carets, and selection rects were
//! measured against a database that lacks it (issue #56).
//!
//! Bytes are stored once in an `Arc` and shared into every fontdb via
//! `Source::Binary`, so a large face costs its size once — not once
//! per `FontSystem` per layout thread.
//!
//! The registry is append-only; fonts cannot be unregistered. That
//! keeps `fontdb::ID`s stable for the lifetime of every `FontSystem`
//! that has already handed them out (the glyph atlases key
//! rasterizations by those ids).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cosmic_text::{FontSystem, fontdb};

static EXTRA_FONTS: Mutex<Vec<Arc<Vec<u8>>>> = Mutex::new(Vec::new());
/// Mirrors `EXTRA_FONTS.len()` so the per-shape fast path can check
/// "anything new?" with one atomic load instead of taking the lock.
static REGISTERED: AtomicUsize = AtomicUsize::new(0);

/// Register an additional font (TTF/OTF bytes) with every Damascene
/// `FontSystem` in the process — both text measurement (wrapping,
/// carets, hit-testing) and glyph rasterization. The font's family,
/// weight, and style are read from its metadata, so registering
/// `Roboto-Bold.ttf` joins the existing `"Roboto"` family at
/// weight 700.
///
/// cosmic-text walks the font database for per-codepoint fallback, so
/// a registered emoji, CJK, or symbol face automatically participates
/// in fallback for any glyph the primary family lacks. This is the
/// supported way to extend script coverage past the default bundle —
/// and, for hosts built with `default-features = false` on
/// `damascene-core`, the way to supply fonts at all.
///
/// Already-shaped text reshapes against the extended database on the
/// next layout pass (the shape caches invalidate themselves), so
/// registration at startup or at runtime both work.
pub fn register_font(bytes: Vec<u8>) {
    let mut fonts = EXTRA_FONTS.lock().expect("font registry poisoned");
    fonts.push(Arc::new(bytes));
    REGISTERED.store(fonts.len(), Ordering::Release);
}

/// Number of fonts registered so far. Sync callers compare this
/// against their own loaded count before touching the lock.
pub(crate) fn registered_count() -> usize {
    REGISTERED.load(Ordering::Acquire)
}

/// Load registry entries `[*loaded..]` into `font_system`'s database
/// and advance `*loaded`. Returns `true` when new faces were added —
/// the caller must invalidate any shape cache, because fallback
/// resolution (and therefore line metrics) may differ against the
/// extended database.
pub(crate) fn sync_font_system(font_system: &mut FontSystem, loaded: &mut usize) -> bool {
    if *loaded >= registered_count() {
        return false;
    }
    let fonts = EXTRA_FONTS.lock().expect("font registry poisoned");
    let mut added = false;
    for font in fonts.iter().skip(*loaded) {
        let source: Arc<dyn AsRef<[u8]> + Send + Sync> = font.clone();
        font_system
            .db_mut()
            .load_font_source(fontdb::Source::Binary(source));
        added = true;
    }
    *loaded = fonts.len();
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "jetbrains-mono")]
    fn sync_loads_only_new_entries_and_reports_change() {
        // Drive sync_font_system through the real global registry.
        // Registering a duplicate of a bundled face is benign for
        // other tests sharing the process: family/weight resolution
        // lands on identical font data either way.
        let mut fs = FontSystem::new_with_locale_and_db(
            "en-US".to_string(),
            fontdb::Database::new(),
        );
        let mut loaded = registered_count();
        assert!(!sync_font_system(&mut fs, &mut loaded), "nothing new yet");

        let before = fs.db().len();
        register_font(damascene_fonts::JETBRAINS_MONO_VARIABLE.to_vec());
        assert!(sync_font_system(&mut fs, &mut loaded));
        assert!(fs.db().len() > before, "registered face should be loaded");
        assert_eq!(loaded, registered_count());

        // Second sync with nothing new is a cheap no-op.
        assert!(!sync_font_system(&mut fs, &mut loaded));
    }

    #[test]
    #[cfg(feature = "jetbrains-mono")]
    fn atlas_registration_reaches_the_measurement_side() {
        // The issue-#56 contract: registering through a GlyphAtlas
        // must extend the measurement-side FontSystem too, so wrap
        // points and carets agree with painted glyphs. Face counts are
        // compared with `>` because other tests in the process may
        // also register fonts concurrently.
        let before = crate::text::metrics::font_system_face_count_for_tests();
        let mut atlas = crate::text::atlas::GlyphAtlas::new();
        atlas.register_font(damascene_fonts::JETBRAINS_MONO_VARIABLE.to_vec());
        let after = crate::text::metrics::font_system_face_count_for_tests();
        assert!(
            after > before,
            "atlas-registered font must reach measurement ({before} -> {after})"
        );
    }
}
