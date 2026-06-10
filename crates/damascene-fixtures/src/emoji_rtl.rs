//! Emoji + RTL fixture section shared by the text-quality renders and
//! the showcase typography page (issue #75).
//!
//! Exercises the two text paths that previously had zero fixture
//! coverage anywhere:
//!
//! - **Color emoji** through the unified-RGBA glyph atlas: plain
//!   pictographs across the size ramp (each size is a distinct atlas
//!   key — relevant to page recycling, #58), plus the cluster-shaping
//!   cases where a regression turns one glyph into several: ZWJ
//!   sequences, skin-tone modifiers, flag pairs, and VS16
//!   text-vs-emoji presentation.
//! - **RTL and bidi**: Hebrew (including nikud mark attachment),
//!   Arabic (joining — isolated letters mean a broken GSUB path),
//!   and mixed-direction lines in both base directions.
//!
//! No bundled font covers Hebrew or Arabic (Inter stops at Cyrillic;
//! Noto Sans Math carries only an isolated-forms math subset), so the
//! samples ship with fixture-local Noto faces registered through
//! [`damascene_core::text::register_font`] — the same process-global
//! registry an app with RTL users would call. `damascene-fixtures` is
//! `publish = false`, so the embedded fonts cost nothing downstream.

use std::sync::Once;

use damascene_core::prelude::*;

/// Emoji size ramp. Mirrors the text-quality matrix's intent: each
/// size rasterizes into its own atlas entries, so the ramp populates
/// (and on regression, tells on) the size-keyed RGBA atlas pages.
pub const EMOJI_SIZES: &[f32] = &[12.0, 16.0, 24.0, 32.0, 48.0];

/// Single-codepoint pictographs — the plain color-emoji rasterization
/// path with no cluster composition involved.
pub const EMOJI_SAMPLE: &str = "😀 🚀 🌍 🎂 🔥 🐢 🍕 ⚽";

/// Cluster-shaping stress: ZWJ sequences (family, profession, flag),
/// skin-tone modifiers, regional-indicator pairs, and VS16 emoji
/// presentation. Each must render as ONE glyph — a shaping regression
/// decomposes them into several.
pub const EMOJI_ZWJ_SAMPLE: &str = "👨\u{200d}👩\u{200d}👧\u{200d}👦 👩\u{200d}💻 🏳\u{fe0f}\u{200d}🌈 👍🏽 🇺🇳 🇯🇵 ❤\u{fe0f}";

/// Hebrew, right-to-left. Plain letters — no marks.
pub const HEBREW_SAMPLE: &str = "שלום עולם — טקסט עברי נכתב מימין לשמאל";

/// Pointed Hebrew (nikud): combining marks must attach to their base
/// letters instead of advancing the pen.
pub const HEBREW_POINTED_SAMPLE: &str = "שָׁלוֹם עוֹלָם — עִם נִקּוּד";

/// Arabic, right-to-left with mandatory joining: letters take
/// initial/medial/final forms. Isolated forms rendering here means
/// the GSUB shaping path broke (or fallback landed on the math face).
pub const ARABIC_SAMPLE: &str = "مرحبا بالعالم — النص العربي يُكتب من اليمين إلى اليسار";

/// Mixed direction, LTR base: embedded Hebrew and Arabic runs must
/// reorder visually while the paragraph stays left-aligned LTR.
pub const MIXED_LTR_SAMPLE: &str =
    "The Hebrew word שלום and the Arabic مرحبا sit inside an English sentence.";

/// Mixed direction, RTL base: Latin brand name, digits, and an emoji
/// embedded in a Hebrew sentence. Digits stay LTR inside the RTL flow.
pub const MIXED_RTL_SAMPLE: &str = "ממשק Damascene מסדר 123 מספרים ואימוג'י 🎉 בשורה אחת";

/// Register the fixture-local RTL faces (Noto Sans Hebrew + Noto Sans
/// Arabic) with the process-global font registry. Idempotent — the
/// registry itself is append-only, so the `Once` guard keeps repeated
/// fixture builds from loading duplicate faces.
pub fn register_rtl_fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        damascene_core::text::register_font(
            include_bytes!("../fonts/NotoSansHebrew-Regular.ttf").to_vec(),
        );
        damascene_core::text::register_font(
            include_bytes!("../fonts/NotoSansArabic-Regular.ttf").to_vec(),
        );
    });
}

fn labeled(label: &str, content: El) -> El {
    row([
        text(label).font_size(11.0).muted().width(Size::Fixed(72.0)),
        content,
    ])
    .gap(tokens::SPACE_2)
    .align(Align::End)
    .width(Size::Fill(1.0))
}

/// The emoji + RTL section, sized for the text-quality page. Calls
/// [`register_rtl_fonts`] itself so every consumer (render bins on
/// all three backends, the showcase) gets real glyphs.
pub fn section() -> El {
    register_rtl_fonts();
    let emoji_rows = EMOJI_SIZES.iter().map(|&size| {
        labeled(
            &format!("emoji {}px", size as u32),
            text(EMOJI_SAMPLE).font_size(size),
        )
    });
    column(
        [
            vec![h2("Emoji & RTL")],
            emoji_rows.collect::<Vec<_>>(),
            vec![
                labeled("zwj 16px", text(EMOJI_ZWJ_SAMPLE).font_size(16.0)),
                labeled("zwj 32px", text(EMOJI_ZWJ_SAMPLE).font_size(32.0)),
                labeled("hebrew", text(HEBREW_SAMPLE).font_size(16.0)),
                labeled("nikud", text(HEBREW_POINTED_SAMPLE).font_size(16.0)),
                labeled(
                    "bold",
                    text(HEBREW_SAMPLE)
                        .font_size(16.0)
                        .font_weight(FontWeight::Bold),
                ),
                labeled("arabic", text(ARABIC_SAMPLE).font_size(16.0)),
                labeled("arabic 24", text(ARABIC_SAMPLE).font_size(24.0)),
                labeled("ltr base", text(MIXED_LTR_SAMPLE).font_size(16.0)),
                labeled("rtl base", text(MIXED_RTL_SAMPLE).font_size(16.0)),
            ],
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
    )
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use damascene_core::text::metrics;
    use damascene_core::tree::{FontFamily, FontWeight, TextWrap};

    fn line_width(s: &str) -> f32 {
        metrics::layout_text_with_line_height_and_family(
            s,
            16.0,
            24.0,
            FontFamily::Inter,
            FontWeight::Regular,
            false,
            TextWrap::NoWrap,
            None,
        )
        .width
    }

    fn first_line_rtl(s: &str) -> bool {
        metrics::layout_text_with_line_height_and_family(
            s,
            16.0,
            24.0,
            FontFamily::Inter,
            FontWeight::Regular,
            false,
            TextWrap::NoWrap,
            None,
        )
        .lines[0]
            .rtl
    }

    #[test]
    fn rtl_samples_resolve_rtl_and_mixed_samples_keep_their_base_direction() {
        register_rtl_fonts();
        assert!(first_line_rtl(HEBREW_SAMPLE));
        assert!(first_line_rtl(HEBREW_POINTED_SAMPLE));
        assert!(first_line_rtl(ARABIC_SAMPLE));
        assert!(first_line_rtl(MIXED_RTL_SAMPLE), "RTL base direction");
        assert!(!first_line_rtl(MIXED_LTR_SAMPLE), "LTR base direction");
    }

    #[test]
    fn arabic_shapes_with_joining_not_isolated_forms() {
        // Three beh letters in a row must take initial/medial/final
        // forms; if the registered face (or its GSUB pass) drops out,
        // each letter renders isolated and the width degenerates to
        // 3× the isolated form.
        register_rtl_fonts();
        let joined = line_width("ببب");
        let isolated = line_width("ب");
        assert!(joined > 0.0 && isolated > 0.0);
        assert!(
            (joined - 3.0 * isolated).abs() > 0.5,
            "joined run ({joined}px) should not measure as 3 isolated letters ({isolated}px each)"
        );
    }

    #[test]
    fn nikud_marks_attach_instead_of_advancing() {
        // Pointed and unpointed spellings of the same word must shape
        // to (nearly) the same width — combining marks attach to their
        // bases rather than taking their own advance.
        register_rtl_fonts();
        let plain = line_width("שלום");
        let pointed = line_width("שָׁלוֹם");
        assert!(plain > 0.0);
        assert!(
            (pointed - plain).abs() < 0.2 * plain,
            "pointed שָׁלוֹם ({pointed}px) should measure like plain שלום ({plain}px)"
        );
    }

    #[test]
    fn zwj_sequences_shape_as_single_clusters() {
        // The family ZWJ sequence is one glyph; if shaping decomposes
        // it the width balloons toward four people + joiners.
        let family = line_width("👨\u{200d}👩\u{200d}👧\u{200d}👦");
        let single = line_width("👨");
        assert!(single > 0.0);
        assert!(
            family < 2.0 * single,
            "family ZWJ sequence ({family}px) should shape as one cluster (single emoji: {single}px)"
        );
        // Flag pair: two regional indicators fuse into one flag.
        let flag = line_width("🇺🇳");
        assert!(
            flag < 2.0 * single,
            "regional-indicator pair ({flag}px) should fuse into one flag"
        );
    }

    #[test]
    fn section_builds_and_lays_out() {
        use damascene_core::layout::layout;
        use damascene_core::state::UiState;
        use damascene_core::tree::Rect;

        let mut tree = section();
        let mut state = UiState::new();
        layout(
            &mut tree,
            &mut state,
            Rect::new(0.0, 0.0, 980.0, 2000.0),
        );
        let h = state.rect(&tree.computed_id).h;
        assert!(h > 100.0, "section should have real extent, got {h}");
    }
}
