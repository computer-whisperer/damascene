//! Markdown → Damascene `El` transformer.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use damascene_markdown::md;
//!
//! let tree: El = md("# Hi\n\nHello **world** with [a link](https://damascene.dev).");
//! ```
//!
//! Markdown is defined as a transformation to HTML, and Damascene's widget
//! kit already echoes most of HTML's shape (`text_runs` ≈ `<p>`,
//! `hard_break` ≈ `<br>`, span modifiers ≈ inline tags, `bullet_list`
//! ≈ `<ul>`, `code_block` ≈ `<pre><code>`, …). The transformer walks
//! `pulldown-cmark`'s streaming `Event` API and assembles an `El` tree
//! out of those primitives — a column of blocks an author would have
//! written by hand. The rendered output behaves like any other Damascene
//! tree: themed surfaces, selection, hit-test, layout, lint.
//!
//! Supported today:
//!
//! - Headings `#` … `###` (and h4–h6 clamped to h3).
//! - Paragraphs with inline emphasis, strong, code, link, hard / soft
//!   breaks. Soft breaks render as a space (CommonMark default).
//! - Bulleted (`-` / `*`), numbered (`1.`), and GFM task lists,
//!   including nested lists and non-1 ordered starts.
//! - Block quotes.
//! - Fenced and indented code blocks.
//! - Horizontal rules.
//! - GFM tables.
//! - Optional native math (`$…$` / `$$…$$`) via
//!   `MarkdownOptions::math(true)`. The first renderer slice supports
//!   a focused TeX subset: rows, identifiers / numbers / operators,
//!   `\frac`, `\sqrt`, superscripts, and subscripts.
//! - Inline + block images render as block-level alt-text placeholders
//!   today. Real image resolution and inline images are Phase 2 follow-ups.
//!
//! Syntax highlighting (default-on `highlighting` feature): fenced code
//! blocks with a recognised language tag (`` ```rust ``, `` ```python ``,
//! …) are tokenized through `syntect` (regex-fancy, no `onig` C
//! dependency) and emitted as a styled `text_runs([...])` paragraph
//! inside the same sunken `code_block` chrome. Each token's colour is
//! an Damascene palette token (`tokens::SUCCESS` for strings,
//! `tokens::INFO` for keywords / numbers, `tokens::MUTED_FOREGROUND`
//! for comments, …) so swapping `Theme::damascene_dark()` for
//! `Theme::damascene_light()` recolours the syntax run automatically.
//! `default-features = false` opts out of the highlighter and shrinks
//! the dependency surface.
//!
//! [`md_with_options`] exposes output-changing parser extensions. Today
//! that includes smart punctuation and GFM alert blockquotes; [`md`]
//! keeps both off by default.
//!
//! Deferred:
//!
//! - Footnotes, raw HTML, full TeX / MathML import, definition lists,
//!   heading attributes, metadata blocks, superscript/subscript, and
//!   wikilinks.

#[cfg(feature = "highlighting")]
mod highlight;

mod transformer;

pub use transformer::{MarkdownOptions, md, md_with_options};
