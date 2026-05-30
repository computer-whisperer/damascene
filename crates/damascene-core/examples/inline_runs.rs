//! inline_runs — exercises the attributed-text path.
//!
//! One paragraph composed of styled inline runs (regular + bold +
//! italic + colored + monospace) plus a hard break, all flowing through
//! cosmic-text's rich-text shaping. Wrapping decisions cross run
//! boundaries — the same way a `<p>` containing `<strong>`, `<em>`,
//! `<code>`, and `<br>` would behave in HTML.
//!
//! Inspect `out/inline_runs.tree.txt` to see the Inlines container with
//! its child Text/HardBreak runs, and `out/inline_runs.draw_ops.txt`
//! to see the single `Attr ... runs=N` line that draw_ops emits — one
//! attributed text op per paragraph, with all the styling baked in.
//!
//! Run: `cargo run -p damascene-core --example inline_runs`

use damascene_core::prelude::*;

fn fixture() -> El {
    column([
        h2("Inline runs"),
        text_runs([
            text("Damascene's attributed-text path lets you compose runs with "),
            text("bold").bold(),
            text(", "),
            text("italic").italic(),
            text(", "),
            text("colored").color(tokens::DESTRUCTIVE),
            text(", and "),
            text("inline code").code(),
            text(" segments inside one wrapping paragraph."),
            hard_break(),
            text("Hard breaks act like ").muted(),
            text("<br>").code(),
            text(" — they end the current line without breaking out of the run.").muted(),
        ])
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        paragraph(
            "All of the above flows through one cosmic-text rich-text shape — \
             wrapping decisions cross run boundaries the way real prose wraps.",
        )
        .muted(),
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
    let written = write_bundle(&bundle, &out_dir, "inline_runs")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
