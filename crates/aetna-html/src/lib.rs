//! HTML → Aetna `El` transformer.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//! use aetna_html::html;
//!
//! let tree: El = html("<h1>Hi</h1><p>Hello <strong>world</strong>.</p>");
//! ```
//!
//! `aetna-html` is a focused HTML-to-`El` transformer, not a browser
//! engine. The thesis: Aetna's widget kit already echoes most of HTML's
//! semantic vocabulary (`text_runs` ≈ `<p>`, `hard_break` ≈ `<br>`,
//! span modifiers ≈ inline tags, `bullet_list` ≈ `<ul>`,
//! `code_block` ≈ `<pre><code>`, `table` ≈ `<table>`, …), so a parse
//! through `html5ever` plus a tag-by-tag mapping onto the existing
//! widget vocabulary produces a faithful render for the subset of HTML
//! that actually fits.
//!
//! Supported in this slice (tier 1 — direct widget mapping, no CSS):
//!
//! - Headings `<h1>` … `<h3>` (h4–h6 clamped to h3, matching
//!   `aetna-markdown`).
//! - Paragraphs `<p>` with inline `<strong>`/`<b>`, `<em>`/`<i>`,
//!   `<u>`, `<s>`/`<strike>`/`<del>`, `<code>`, `<a href>`, `<br>`,
//!   `<kbd>`, `<mark>`.
//! - Lists `<ul>` / `<ol>` / `<li>` (ordered lists honour the `start`
//!   attribute); nested lists; GFM-style checkbox-prefixed task lists.
//! - `<blockquote>`.
//! - `<pre><code class="language-X">…</code></pre>` and bare `<pre>`
//!   (plain mono — syntax highlighting handoff is a follow-up).
//! - `<hr>`.
//! - `<table>` / `<thead>` / `<tbody>` / `<tr>` / `<th>` / `<td>`.
//! - `<img alt="">` placeholder (Phase 2 follow-up, mirroring
//!   `aetna-markdown`).
//!
//! Tier 2 (generic containers + CSS subset for inline `style=""` and a
//! flat `<style>` block) and tier 3 (security-dropped tags) are
//! documented in `docs/HTML_VISION.md`.
//!
//! Sanitization is hardcoded in this slice: `<script>`, `<iframe>`,
//! `<object>`, `<embed>`, `<noscript>`, every `on*` attribute, and
//! every `javascript:` / `vbscript:` / `data:text/html` URL are
//! dropped. Embedders handling untrusted HTML should still layer a
//! dedicated sanitizer (e.g. `ammonia`) in front.

mod options;
mod parser;
mod sanitize;
mod transform;

pub use options::HtmlOptions;
pub use transform::{html, html_blocks, html_fragment_inline, html_with_options};
