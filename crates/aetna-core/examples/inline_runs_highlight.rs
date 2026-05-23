//! inline_runs_highlight — exercises sub-word inline backgrounds.
//!
//! Three paragraphs:
//!
//! - **A search-result row** with the matched term highlighted mid-word.
//! - **A diff-style line** with added / removed tinted spans flowing
//!   inside one prose paragraph (no per-word `<span>` chrome — the
//!   shaper places the rects at the actual glyph extents).
//! - **A wrapping highlight** that splits across two lines so the rect
//!   per-line emission is visible in the artifact.
//!
//! Inspect `out/inline_runs_highlight.draw_ops.txt` — each Attr line
//! reports `runs=N bg_runs=M`, and the SVG fallback emits one
//! `<rect data-node="…run-bg">` per highlighted run on the first line.
//! The wgpu / vulkano paths shape once and emit one quad per line per
//! styled span.
//!
//! Run: `cargo run -p aetna-core --example inline_runs_highlight`

use aetna_core::prelude::*;

const HIGHLIGHT_YELLOW: Color = Color::srgb_token("inline-mark", 240, 210, 90, 200);
const DIFF_ADD: Color = Color::srgb_token("diff-add", 64, 130, 88, 220);
const DIFF_REMOVE: Color = Color::srgb_token("diff-remove", 180, 70, 80, 220);

fn fixture() -> El {
    column([
        h2("Inline run backgrounds"),
        paragraph(
            "RunStyle.bg paints a per-line solid quad behind the glyphs of \
             a styled span — the shaper computes the rect from the actual \
             glyph extents, so wrapping splits the highlight cleanly.",
        )
        .muted(),
        // Search-result style.
        text_runs([
            text("…the matcher finds "),
            text("aetna").background(HIGHLIGHT_YELLOW).bold(),
            text(" in "),
            text("aetna_core::widgets").mono(),
            text(" — the highlight tracks the glyph extent."),
        ])
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        // Diff-style: add + remove tints inside the same line.
        text_runs([
            text("- "),
            text("error::Custom").mono().background(DIFF_REMOVE),
            text("(\"too narrow\")"),
            hard_break(),
            text("+ "),
            text("error::WrapTooNarrow").mono().background(DIFF_ADD),
            text(" { available }"),
        ])
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        // Wrapping highlight: long span that spans two lines.
        text_runs([
            text("Long highlight: "),
            text("the quick brown fox jumps over the lazy dog and keeps going")
                .background(HIGHLIGHT_YELLOW),
            text(" — the rect is split per line."),
        ])
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
    .width(Size::Fixed(640.0))
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 640.0, 360.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "inline_runs_highlight")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
