//! Typography + long-form content.
//!
//! Demonstrates the heading / paragraph / list / quote / code-block
//! vocabulary by hand, then renders the same vocabulary through
//! `damascene_markdown::md` so the markdown transformer's output is
//! visible alongside the hand-authored variant.

use damascene_core::prelude::*;
use damascene_core::selection::SelectionSource;
use damascene_markdown::{MarkdownOptions, md_with_options};

#[derive(Default)]
pub struct State {
    pub selection: Selection,
}

const MARKDOWN_SOURCE: &str = "\
## Markdown

`damascene_markdown::md` walks `pulldown-cmark` events into Damascene widgets — \
the same widget kit the hand-authored examples above compose. Inline runs \
cover **bold**, *italic*, `inline code`, ~~strike~~, and \
[links](https://damascene.dev), all flowing through one wrapping paragraph.

### Lists

- Bullet items wrap under themselves with a hanging indent.
- Inline runs work inside items: **bold**, *italic*, `code`, \
  [links](https://damascene.dev).
- Nested lists live inside a composite item.

42. Numbered lists preserve custom starts.
43. The next marker continues from the source.

- [x] Completed task items use static checkbox markers.
- [ ] Pending task items use the same hanging indent.

### Quote

> Markdown's shape is HTML's shape. Damascene's widget kit already \
> mirrors most of that shape, so the transformer mostly hands events \
> to existing constructors.

### Fenced code

```rust
// damascene-markdown highlights fenced code with a recognised lang tag.
fn render(md: &str) -> El {
    damascene_markdown::md(md)
}
```

### Tables

| Construct  | Maps to            |
|------------|--------------------|
| Heading    | `h1` / `h2` / `h3` |
| List       | `bullet_list` / `numbered_list` |
| Blockquote | `blockquote`       |
| Code block | `code_block`       |
| Table      | `table`            |

### Native math

Inline math shares a prose baseline: $e^{i\\pi}+1=0$, \
$x_1+x_2$, and $\\sqrt{x_1+x_2}$.

$$
\\frac{a^2+b^2}{\\sqrt{x_1+x_2}} + \\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}
$$
";

pub fn view(_state: &State) -> El {
    scroll([column([
        selectable(h1("Typography"), "typography-title"),
        paragraph(
            "Long-form-content widgets compose the same markdown-shaped \
             vocabulary the `damascene_markdown::md` transformer targets — \
             headings, paragraphs, lists, quotes, code blocks, links. \
             Each is plain Damascene with selectable text and themed surfaces.",
        )
        .key("typography-intro")
        .selectable()
        .muted(),
        selectable(h2("Headings"), "typography-headings-title"),
        selectable(h3("Subheading"), "typography-subheading"),
        selectable_runs(
            "typography-heading-copy",
            "Headings stack at h1 / h2 / h3 - the display, heading, and title text roles, respectively.",
            [
                text("Headings stack at "),
                text("h1").code(),
                text(" / "),
                text("h2").code(),
                text(" / "),
                text("h3").code(),
                text(" - the "),
                text("display").italic(),
                text(", "),
                text("heading").italic(),
                text(", and "),
                text("title").italic(),
                text(" text roles, respectively."),
            ],
        ),
        selectable(h2("Bulleted list"), "typography-bullets-title"),
        bullet_list(vec![
            selectable(
                text("Plain string items wrap inside the content column so a long item flows under itself rather than under the bullet."),
                "typography-bullet-plain",
            ),
            selectable_runs(
                "typography-bullet-runs",
                "Inline runs work in items: bold, italic, code, links.",
                [
                    text("Inline runs work in items: "),
                    text("bold").bold(),
                    text(", "),
                    text("italic").italic(),
                    text(", "),
                    text("code").code(),
                    text(", "),
                    text("links").link("https://damascene.dev"),
                    text("."),
                ],
            ),
            column([
                selectable(
                    paragraph("Composite items host nested blocks - a paragraph, then a sub-list:"),
                    "typography-bullet-composite",
                ),
                bullet_list(["nested one", "nested two"]),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0)),
        ]),
        selectable(h2("Numbered list"), "typography-numbered-title"),
        numbered_list([
            "Markers right-align so the period sits flush across items.",
            "Marker-slot width grows with the item count — `9.` and `99.` lay out without crowding the content.",
            "Plain-text items wrap inside the content column, same convention as the bullet list.",
        ]),
        selectable(h2("Blockquote"), "typography-blockquote-title"),
        blockquote([
            paragraph(
                "Markdown's shape is HTML's shape. Damascene's widget kit \
                 already mirrors most of that shape, so the transformer \
                 mostly hands events to existing constructors.",
            ),
            paragraph("— Damascene design notes").muted(),
        ]),
        selectable(h2("Code block"), "typography-code-title"),
        code_block(
            "fn render(md: &str) -> El {\n    \
                 damascene_markdown::md(md)\n}",
        ),
        selectable(h2("Inline runs"), "typography-inline-title"),
        selectable_runs(
            "typography-inline-copy",
            "Inline runs carry underline, strikethrough, and links via per-run flags. The decoration bar tracks each run's color automatically.",
            [
                text("Inline runs carry "),
                text("underline").underline(),
                text(", "),
                text("strikethrough").strikethrough(),
                text(", and "),
                text("links").link("https://damascene.dev"),
                text(" via per-run flags. The decoration bar tracks each run's "),
                text("color").italic(),
                text(" automatically."),
            ],
        ),
        separator(),
        paragraph(
            "Below: the same vocabulary rendered from a markdown source string \
             through `damascene_markdown::md`, so the transformer's output sits \
             next to the hand-authored variant.",
        )
        .key("typography-markdown-intro")
        .selectable()
        .muted()
        .small(),
        md_with_options(MARKDOWN_SOURCE, MarkdownOptions::default().math(true)),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Start)
    .width(Size::Fill(1.0))])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, event: UiEvent) {
    if event.kind == UiEventKind::SelectionChanged
        && let Some(sel) = event.selection.as_ref()
    {
        state.selection = sel.clone();
    }
}

fn selectable(el: El, key: &'static str) -> El {
    el.key(key).selectable()
}

fn selectable_runs<const N: usize>(key: &'static str, visible: &'static str, runs: [El; N]) -> El {
    text_runs(runs)
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .key(key)
        .selectable()
        .selection_source(SelectionSource::identity(visible))
}
