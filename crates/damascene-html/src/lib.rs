//! HTML → Damascene `El` transformer.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use damascene_html::html;
//!
//! let tree: El = html("<h1>Hi</h1><p>Hello <strong>world</strong>.</p>");
//! ```
//!
//! `damascene-html` is a focused HTML-to-`El` transformer, not a browser
//! engine. The thesis: Damascene's widget kit already echoes most of HTML's
//! semantic vocabulary (`text_runs` ≈ `<p>`, `hard_break` ≈ `<br>`,
//! span modifiers ≈ inline tags, `bullet_list` ≈ `<ul>`,
//! `code_block` ≈ `<pre><code>`, `table` ≈ `<table>`, …), so a parse
//! through `html5ever` plus a tag-by-tag mapping onto the existing
//! widget vocabulary produces a faithful render for the subset of HTML
//! that actually fits.
//!
//! Supported coverage (per `docs/HTML_VISION.md`):
//!
//! - **Tier 1 — direct widget mapping.** Headings `<h1>`…`<h3>`
//!   (h4–h6 clamp to h3), paragraphs with the inline tag set
//!   (`<strong>`/`<b>`, `<em>`/`<i>`, `<u>`, `<s>`/`<strike>`/`<del>`,
//!   `<code>`, `<a href>`, `<br>`, `<kbd>`, `<mark>`), `<ul>` / `<ol>`
//!   / `<li>` with nesting + GFM task-list shape, `<blockquote>`,
//!   `<pre><code class="language-X">`, `<hr>`, full table tag family,
//!   `<img>` placeholders.
//! - **Tier 2A — inline `style="..."`.** Color, background, padding,
//!   border, radius, opacity, width/height + min/max, text-align,
//!   font-size/weight/style, text-decoration.
//! - **Tier 2B — `<style>` blocks + selectors.** Tag / class / id /
//!   compound / comma-grouped selectors with full specificity sort.
//!   At-rules and unsupported selector shapes drop with lint findings.
//! - **Tier 2C — generic containers.** `<div>`, `<section>`, `<details>`,
//!   `<figure>`, `<button>`, `<input type="checkbox">` (all cosmetic).
//! - **Tier 2D — layout reshape + lint surface.** `display: flex` +
//!   `flex-direction`, `align-items`, `justify-content`, `overflow`
//!   (hidden → clip, auto/scroll → wrap in `scroll`), `box-shadow`
//!   (blur extracted), `font-family` monospace detection, and
//!   `margin*` reconciled into parent `gap`. Properties without an
//!   Damascene equivalent (`position`, `float`, `vh`/`vw`/`fr`,
//!   `display: grid`, …) drop with [`Finding`]s exposed through the
//!   `_with_lints` entry points.
//!
//! Tier 3 (security-dropped: `<script>`, `<iframe>`, `<object>`,
//! `<embed>`, `<noscript>`, every `on*` attribute, every
//! `javascript:` / `vbscript:` / `data:text/html` URL) is enforced
//! always. Embedders handling untrusted HTML should still layer a
//! dedicated sanitizer (e.g. `ammonia`) in front.

mod css;
mod lints;
mod options;
mod parser;
mod sanitize;
mod selectors;
mod transform;

pub use lints::{Finding, FindingKind};
pub use options::HtmlOptions;
pub use transform::{
    html, html_blocks, html_blocks_with_lints, html_fragment_inline,
    html_fragment_inline_with_lints, html_with_lints, html_with_options,
};
