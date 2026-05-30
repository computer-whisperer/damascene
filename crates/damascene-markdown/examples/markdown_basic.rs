//! markdown_basic — round-trip a representative markdown document
//! through `damascene_markdown::md` and write the bundle.
//!
//! The fixture is shaped like a typical README: heading, paragraph
//! with inline emphasis / code / link, bulleted list, numbered list,
//! blockquote, fenced code block, horizontal rule. Each maps through
//! the streaming pulldown-cmark events into Damascene primitives —
//! `h1` / `paragraph` / `text_runs` / `bullet_list` / `numbered_list`
//! / `blockquote` / `code_block` / `divider` — and the rendered
//! output is a normal Damascene tree (selection, theming, lint, layout
//! all behave exactly as if the column were hand-authored).
//!
//! Run: `cargo run -p damascene-markdown --example markdown_basic`

use damascene_core::prelude::*;
use damascene_markdown::md;

const SOURCE: &str = "\
# damascene-markdown

A small **markdown** document rendered by `damascene_markdown::md` into a column \
of Damascene widgets. Inline runs cover *italic*, **bold**, `inline code`, and \
[links](https://damascene.dev) — all flowing through one wrapping paragraph.

## Highlights

- Bullet items wrap to fit, with a hanging indent.
- Inline runs work inside items: **bold**, *italic*, `code`, and \
  [links](https://damascene.dev).
- Nested lists live inside an item.

### Setup

1. Add `damascene-markdown` to the workspace.
2. Pull in `pulldown-cmark` with the GFM features the project uses.
3. Wire the transformer through the existing widget kit.

42. Preserve source start numbers in ordered lists.
43. Keep marker spacing stable as the list continues.

- [x] Render completed task markers.
- [ ] Render pending task markers.

> Markdown's shape is HTML's shape. Damascene's widget kit already mirrors \
> most of that shape, so the transformer mostly hands events to existing \
> constructors.

```
fn render(md: &str) -> El {
    damascene_markdown::md(md)
}
```

GFM strikethrough renders as a ~~struck-through~~ inline run, and a \
GFM table maps to the existing `widgets::table` anatomy:

| Construct | Maps to            |
|-----------|--------------------|
| Heading   | `h1` / `h2` / `h3` |
| List      | `bullet_list` / `numbered_list` |
| Blockquote| `blockquote`       |
| Code block| `code_block`       |
| Table     | `table`            |

---

The horizontal rule above closes the document.
";

fn fixture() -> El {
    column([md(SOURCE)])
        .padding(tokens::SPACE_7)
        .width(Size::Fixed(640.0))
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 640.0, 1000.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "markdown_basic")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
