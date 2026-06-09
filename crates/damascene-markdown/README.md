# damascene-markdown

Markdown → Damascene `El` transformer.

```rust
use damascene_core::prelude::*;
use damascene_markdown::md;

let tree: El = md("# Hi\n\nHello **world** with [a link](https://damascene.dev).");
```

Markdown is defined as a transformation to HTML, and Damascene's widget kit
already echoes most of HTML's shape (`text_runs` ≈ `<p>`, `hard_break` ≈
`<br>`, span modifiers ≈ inline tags, `bullet_list` ≈ `<ul>`, `code_block`
≈ `<pre><code>`, …). The transformer walks `pulldown-cmark`'s streaming
event API and assembles an `El` tree out of those primitives — a column of
blocks an author would have written by hand. The rendered output behaves
like any other Damascene tree: themed surfaces, selection, hit-test,
layout, lint.

Supported: headings, paragraphs with the full inline set, bulleted /
numbered / GFM task lists (nested), block quotes, fenced + indented code
blocks, horizontal rules, GFM tables, and image alt-text placeholders.
`md_with_options` exposes output-changing parser extensions (smart
punctuation, GFM alert blockquotes).

## Features

- **`highlighting`** (default) — fenced code blocks with a recognised
  language tag are tokenized through `syntect` (pure Rust, no C `onig`)
  and colored with Damascene palette tokens, so a theme swap recolours
  the syntax run automatically. `default-features = false` opts out and
  shrinks the dependency surface.
- **`math`** via `MarkdownOptions::math(true)` — `$…$` / `$$…$$`
  expressions render through `damascene_core::math`'s native box layout
  (no webview, no raster round-trip). The current slice covers a focused
  TeX subset: rows, identifiers / numbers / operators, `\frac`, `\sqrt`,
  superscripts, subscripts.
- **`html`** (default-off) — routes inline/block HTML events through the
  [`damascene-html`](https://crates.io/crates/damascene-html) transformer
  instead of dropping them.

The full behaviour contract lives in the crate rustdoc:
[docs.rs/damascene-markdown](https://docs.rs/damascene-markdown).
