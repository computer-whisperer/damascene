# damascene-html

HTML → Damascene `El` transformer.

```rust
use damascene_core::prelude::*;
use damascene_html::html;

let tree: El = html("<h1>Hi</h1><p>Hello <strong>world</strong>.</p>");
```

`damascene-html` is a focused HTML-to-`El` transformer, not a browser
engine. Damascene's widget kit already echoes most of HTML's semantic
vocabulary (`text_runs` ≈ `<p>`, span modifiers ≈ inline tags,
`bullet_list` ≈ `<ul>`, `code_block` ≈ `<pre><code>`, `table` ≈
`<table>`, …), so a parse through `html5ever` plus a tag-by-tag mapping
onto the existing widget vocabulary produces a faithful render for the
subset of HTML that actually fits.

Coverage, in tiers:

- **Direct widget mapping** — headings, paragraphs with the inline tag
  set, nested lists (including the task-list shape), block quotes,
  `<pre><code class="language-X">`, `<hr>`, the full table tag family,
  `<img>` placeholders.
- **Inline `style="…"`** — color, background, padding, border, radius,
  opacity, sizing, text-align, font properties, text-decoration.
- **`<style>` blocks** — tag / class / id / compound / comma-grouped
  selectors with full specificity sort.
- **Generic containers** — `<div>`, `<section>`, `<details>`,
  `<figure>`, `<button>`, `<input type="checkbox">` (cosmetic).
- **Layout reshape** — `display: flex` with direction/alignment,
  `overflow` (clip / scroll wrap), `box-shadow`, monospace
  `font-family` detection, margins reconciled into parent `gap`.

Properties and selectors without a Damascene equivalent drop with lint
`Finding`s exposed through the `_with_lints` entry points — silent
fidelity loss is treated as a bug in the contract.

Script-capable content (`<script>`, `<iframe>`, `on*` attributes,
`javascript:` URLs, …) is always dropped. Embedders handling untrusted
HTML should still layer a dedicated sanitizer (e.g. `ammonia`) in front.

Used standalone, or via `damascene-markdown`'s `html` feature to render
inline/block HTML inside markdown. Full contract in the crate rustdoc:
[docs.rs/damascene-html](https://docs.rs/damascene-html).
