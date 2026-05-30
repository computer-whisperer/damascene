//! long_form_content — exercises the long-form prose widgets that the
//! upcoming `damascene-markdown` transformer will target: `bullet_list`,
//! `numbered_list`, `blockquote`, and `code_block`, alongside the
//! existing heading + paragraph + `text_runs` primitives.
//!
//! The fixture is shaped like a typical README block — heading, intro
//! paragraph, a bulleted list, a blockquote, a numbered list, and a
//! fenced code block — so the visual rhythm and indentation of the new
//! widgets can be inspected directly.
//!
//! Run: `cargo run -p damascene-core --example long_form_content`

use damascene_core::prelude::*;

fn fixture() -> El {
    column([
        h2("Long-form content widgets"),
        paragraph(
            "These primitives compose the markdown-shaped vocabulary an \
             upcoming transformer will target. Each widget is plain \
             Damascene — selectable text, themed surfaces, the same layout \
             pass as everything else.",
        ),
        h3("Highlights"),
        bullet_list(vec![
            text_runs([
                text("Bulleted lists with a hanging indent — wrapped lines align under "),
                text("themselves").italic(),
                text(", not under the marker."),
            ]),
            text_runs([
                text("Inline runs work inside list items: "),
                text("bold").bold(),
                text(", "),
                text("code").code(),
                text(", "),
                text("links").link("https://damascene.dev"),
                text("."),
            ]),
            text("Nested blocks live inside an item by composing a column."),
        ]),
        blockquote([
            paragraph(
                "Markdown's shape is HTML's shape. The Damascene widget kit \
                 already mirrors most of that shape, so the transformer \
                 mostly hands events to existing constructors.",
            ),
            paragraph("— Damascene design notes").muted(),
        ]),
        h3("Setup steps"),
        numbered_list(vec![
            text("Add `damascene-markdown` to the workspace."),
            text_runs([
                text("Pull in "),
                text("pulldown-cmark").code(),
                text(" with the GFM features the project actually uses."),
            ]),
            text("Wire the transformer through the existing widget kit — paragraph, list, blockquote, code_block, divider, table."),
        ]),
        h3("Example fenced block"),
        code_block(
            "fn render(md: &str) -> El {\n    \
                 // pulldown-cmark events -> El\n    \
                 todo!(\"phase 2\")\n}",
        ),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
    .width(Size::Fixed(640.0))
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 640.0, 720.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "long_form_content")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
