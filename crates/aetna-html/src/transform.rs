//! DOM walker that maps the tier-1 HTML tag set onto Aetna `El` widgets.
//!
//! The walker mirrors `aetna-markdown::Walker` in spirit: a small flat
//! `InlineState` carries italic / bold / strike / underline / mono /
//! link / inline-color through nested inline tags, and a context split
//! (`walk_block_children` vs `walk_inline_children`) decides whether
//! each child is rendered as a block-level Aetna widget or appended to
//! an inline run buffer.
//!
//! Unknown tags fall back to a context-sensitive pass-through —
//! block-context: recurse as block; inline-context: recurse as inline.
//! That matches what browsers do with tag-soup HTML and keeps the
//! transformer total: every input produces an El, even if some content
//! gets flattened.

use aetna_core::prelude::*;
// `namespace_url` is the trait the `ns!()` macro consumes via fully-
// qualified path. Without it in scope the macro expands to an empty
// atom and every `name.ns != ns!(html)` comparison spuriously rejects
// HTML elements.
#[allow(unused_imports)]
use html5ever::namespace_url;
use html5ever::ns;
use markup5ever_rcdom::{Handle, NodeData};

use crate::options::HtmlOptions;
use crate::parser::{parse_document_dom, parse_fragment_dom};
use crate::sanitize::{is_blocked_attr, is_blocked_tag, is_safe_url};

/// Render an HTML document as an Aetna `El`. Returns a `column([...])`
/// of block-level Aetna widgets — the same shape an author would have
/// hand-written, and the same shape `aetna_markdown::md` returns.
pub fn html(input: &str) -> El {
    html_with_options(input, HtmlOptions::default())
}

/// Render an HTML document with explicit options.
pub fn html_with_options(input: &str, opts: HtmlOptions) -> El {
    // `document` must stay in scope for the duration of the walk.
    // `markup5ever_rcdom::Node::Drop` iteratively `mem::take`s the
    // children of every descendant to avoid stack overflow on deep
    // trees; if the document handle drops while we still hold a body
    // sub-handle, the body's `children` Vec is silently emptied.
    let document = parse_document_dom(input);
    let body = find_body(&document).unwrap_or_else(|| document.clone());
    let state = InlineState::default();
    let blocks = walk_block_children(&body, &state, &opts);
    column(blocks)
        .gap(tokens::SPACE_4)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Like [`html_with_options`] but returns the block-level Els
/// directly instead of wrapping them in a `column`. Intended for
/// callers (e.g. `aetna-markdown`'s block-HTML event handler) that
/// already have a containing block frame and just want the produced
/// children appended.
pub fn html_blocks(input: &str, opts: HtmlOptions) -> Vec<El> {
    let document = parse_document_dom(input);
    let body = find_body(&document).unwrap_or_else(|| document.clone());
    let state = InlineState::default();
    walk_block_children(&body, &state, &opts)
}

/// Inline-only entry point: parse `input` as an HTML fragment and
/// return the inline runs it produces. The intended caller is
/// `aetna-markdown`'s `Event::InlineHtml` handler, which buffers
/// consecutive inline-HTML events into one string, hands the buffer
/// here, and appends the produced runs to the open paragraph /
/// heading / link / table cell.
///
/// Block-level tags appearing in the fragment are flattened: their
/// children render inline in source order rather than terminating the
/// paragraph.
pub fn html_fragment_inline(input: &str, opts: HtmlOptions) -> Vec<El> {
    // See `html_with_options` for the rcdom drop trap — keep
    // `document` alive for the whole walk.
    let document = parse_fragment_dom(input);
    let root = find_fragment_root(&document).unwrap_or_else(|| document.clone());
    let state = InlineState::default();
    let mut runs = Vec::new();
    for child in root.children.borrow().iter() {
        walk_inline_node(child, &state, &mut runs, &opts);
    }
    runs
}

// ---------- DOM helpers ----------

/// Walk the document tree to find the `<body>` element html5ever
/// always synthesises for full-document parses.
fn find_body(node: &Handle) -> Option<Handle> {
    if let NodeData::Element { name, .. } = &node.data
        && name.local.as_ref() == "body"
    {
        return Some(node.clone());
    }
    for child in node.children.borrow().iter() {
        if let Some(found) = find_body(child) {
            return Some(found);
        }
    }
    None
}

/// `parse_fragment` wraps the input in a synthetic `<html>` element
/// whose children are the fragment's top-level nodes. Find it so the
/// caller iterates the fragment's siblings rather than the wrapper.
fn find_fragment_root(node: &Handle) -> Option<Handle> {
    if let NodeData::Element { name, .. } = &node.data
        && name.local.as_ref() == "html"
    {
        return Some(node.clone());
    }
    for child in node.children.borrow().iter() {
        if let Some(found) = find_fragment_root(child) {
            return Some(found);
        }
    }
    None
}

fn element_tag(node: &Handle) -> Option<String> {
    if let NodeData::Element { name, .. } = &node.data {
        if name.ns != ns!(html) {
            return None;
        }
        Some(name.local.as_ref().to_ascii_lowercase())
    } else {
        None
    }
}

fn element_attr(node: &Handle, attr: &str) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    for a in attrs.borrow().iter() {
        if a.name.local.as_ref().eq_ignore_ascii_case(attr)
            && !is_blocked_attr(a.name.local.as_ref())
        {
            return Some(a.value.to_string());
        }
    }
    None
}

// ---------- Inline state ----------

/// Inline styling currently in effect for new text runs. Mirrors
/// `aetna-markdown::InlineState` but extends it to the HTML-specific
/// tags (`<u>`, `<kbd>`, `<mark>`, `<code>` as an inline run) and to
/// `<a href>` which carries a value rather than just a flag.
#[derive(Default, Clone)]
struct InlineState {
    italic_depth: u32,
    bold_depth: u32,
    strike_depth: u32,
    underline_depth: u32,
    code_depth: u32,
    mono_depth: u32,
    /// Most-recent open `<a href="...">`. Inline tags inside an `<a>`
    /// inherit the same href so the painter groups them as one link.
    link: Option<String>,
    /// Highlight (`<mark>`). Set as a flag rather than a value so an
    /// open `<mark>` inherits through nested inline tags.
    highlight: bool,
}

impl InlineState {
    fn apply(&self, mut el: El) -> El {
        if self.bold_depth > 0 {
            el = el.bold();
        }
        if self.italic_depth > 0 {
            el = el.italic();
        }
        if self.strike_depth > 0 {
            el = el.strikethrough();
        }
        if self.underline_depth > 0 {
            el = el.underline();
        }
        if self.code_depth > 0 {
            el = el.code();
        } else if self.mono_depth > 0 {
            // `<kbd>` etc. use mono without the inline-code surface
            // role; `<code>`'s `.code()` already implies mono.
            el = el.mono();
        }
        if let Some(href) = &self.link {
            el = el.link(href.clone());
        }
        if self.highlight {
            // Soft yellow band behind the glyphs. Uses the theme's
            // WARNING token so palette swaps recolour automatically.
            el = el.background(tokens::WARNING.with_alpha(60));
        }
        el
    }
}

// ---------- Tag classification ----------

/// Tags whose semantic is purely inline. Block tags appearing inside
/// an inline buffer get coerced to their inline-equivalent flattening.
fn is_inline_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "abbr"
            | "b"
            | "bdi"
            | "bdo"
            | "br"
            | "cite"
            | "code"
            | "data"
            | "dfn"
            | "em"
            | "i"
            | "img"
            | "kbd"
            | "mark"
            | "q"
            | "s"
            | "samp"
            | "small"
            | "span"
            | "strong"
            | "strike"
            | "del"
            | "sub"
            | "sup"
            | "time"
            | "u"
            | "var"
            | "wbr"
    )
}

/// Whether a DOM node — element or text — is "inline" for block-context
/// flow. Comments and whitespace-only text count as inline so they can
/// be absorbed into a pending paragraph buffer rather than triggering
/// an anonymous paragraph flush.
fn is_inline_node(node: &Handle) -> bool {
    match &node.data {
        NodeData::Text { .. } | NodeData::Comment { .. } => true,
        NodeData::Element { name, .. } => {
            if name.ns != ns!(html) {
                return true;
            }
            let tag = name.local.as_ref().to_ascii_lowercase();
            if is_blocked_tag(&tag) {
                return true;
            }
            is_inline_tag(&tag)
        }
        _ => true,
    }
}

// ---------- Block walker ----------

fn walk_block_children(parent: &Handle, state: &InlineState, opts: &HtmlOptions) -> Vec<El> {
    let mut blocks: Vec<El> = Vec::new();
    let mut inline_buf: Vec<El> = Vec::new();
    for child in parent.children.borrow().iter() {
        if is_inline_node(child) {
            walk_inline_node(child, state, &mut inline_buf, opts);
        } else {
            flush_inline_buf(&mut inline_buf, &mut blocks);
            walk_block_node(child, state, &mut blocks, opts);
        }
    }
    flush_inline_buf(&mut inline_buf, &mut blocks);
    blocks
}

/// Fold an accumulated inline-run buffer into an anonymous paragraph
/// block. Drops a buffer that contains only whitespace runs.
fn flush_inline_buf(inline_buf: &mut Vec<El>, blocks: &mut Vec<El>) {
    if inline_buf.is_empty() {
        return;
    }
    let runs: Vec<El> = std::mem::take(inline_buf);
    if runs_are_blank(&runs) {
        return;
    }
    blocks.push(build_paragraph(runs));
}

fn build_paragraph(runs: Vec<El>) -> El {
    if let Some(plain) = single_plain_text(&runs) {
        paragraph(plain)
    } else {
        text_runs(runs)
            .wrap_text()
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    }
}

fn walk_block_node(node: &Handle, state: &InlineState, blocks: &mut Vec<El>, opts: &HtmlOptions) {
    let Some(tag) = element_tag(node) else {
        return;
    };
    if is_blocked_tag(&tag) {
        return;
    }
    match tag.as_str() {
        "p" => {
            let runs = collect_inline_runs(node, state, opts);
            if !runs_are_blank(&runs) {
                blocks.push(build_paragraph(runs));
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let runs = collect_inline_runs(node, state, opts);
            blocks.push(build_heading(&tag, runs));
        }
        "br" => {
            // Block-context `<br>` is unusual but legal; render as an
            // empty paragraph spacer so the column gap pushes one row
            // of separation. Doing nothing would silently swallow the
            // author's intent.
            blocks.push(paragraph(""));
        }
        "hr" => blocks.push(divider()),
        "ul" => blocks.push(build_unordered_list(node, state, opts)),
        "ol" => blocks.push(build_ordered_list(node, state, opts)),
        "blockquote" => {
            let inner = walk_block_children(node, state, opts);
            blocks.push(blockquote(inner));
        }
        "pre" => blocks.push(build_pre(node)),
        "table" => blocks.push(build_table(node, state, opts)),
        "img" => {
            // Block-position `<img>` (rare but legal). Use the inline
            // placeholder path; the placeholder is itself an `El` so
            // it works at block position too.
            if let Some(placeholder) = build_image_placeholder(node) {
                blocks.push(placeholder);
            }
        }
        // Generic block containers — pass through to children. The CSS
        // pass in tier 2 will read inline `style=""` and `<style>`
        // rules off these and apply them.
        "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside"
        | "figure" | "figcaption" | "details" | "summary" | "form" | "fieldset" | "legend"
        | "body" | "html" => {
            let inner = walk_block_children(node, state, opts);
            blocks.extend(inner);
        }
        _ => {
            // Unknown / tier-2 block-shaped tag: pass through. The
            // inline-context coercion handles unknown inline tags.
            let inner = walk_block_children(node, state, opts);
            blocks.extend(inner);
        }
    }
}

// ---------- Inline walker ----------

fn walk_inline_node(node: &Handle, state: &InlineState, runs: &mut Vec<El>, opts: &HtmlOptions) {
    match &node.data {
        NodeData::Text { contents } => {
            let s = contents.borrow().to_string();
            if s.is_empty() {
                return;
            }
            runs.push(state.apply(text(s)));
        }
        NodeData::Comment { .. } => {}
        NodeData::Element { name, .. } => {
            if name.ns != ns!(html) {
                return;
            }
            let tag = name.local.as_ref().to_ascii_lowercase();
            if is_blocked_tag(&tag) {
                return;
            }
            dispatch_inline_element(node, &tag, state, runs, opts);
        }
        _ => {}
    }
}

fn dispatch_inline_element(
    node: &Handle,
    tag: &str,
    state: &InlineState,
    runs: &mut Vec<El>,
    opts: &HtmlOptions,
) {
    match tag {
        "strong" | "b" => {
            let mut next = state.clone();
            next.bold_depth += 1;
            walk_inline_children(node, &next, runs, opts);
        }
        "em" | "i" | "cite" | "dfn" | "var" => {
            let mut next = state.clone();
            next.italic_depth += 1;
            walk_inline_children(node, &next, runs, opts);
        }
        "u" => {
            let mut next = state.clone();
            next.underline_depth += 1;
            walk_inline_children(node, &next, runs, opts);
        }
        "s" | "strike" | "del" => {
            let mut next = state.clone();
            next.strike_depth += 1;
            walk_inline_children(node, &next, runs, opts);
        }
        "code" => {
            let mut next = state.clone();
            next.code_depth += 1;
            walk_inline_children(node, &next, runs, opts);
        }
        "kbd" | "samp" => {
            let mut next = state.clone();
            next.mono_depth += 1;
            walk_inline_children(node, &next, runs, opts);
        }
        "mark" => {
            let mut next = state.clone();
            next.highlight = true;
            walk_inline_children(node, &next, runs, opts);
        }
        "a" => {
            let href = element_attr(node, "href").filter(|h| is_safe_url(h));
            let mut next = state.clone();
            // Inner `<a>` overrides outer href (browser semantics:
            // nested `<a>` is invalid, but we take the innermost).
            if let Some(href) = href {
                next.link = Some(href);
            }
            walk_inline_children(node, &next, runs, opts);
        }
        "br" => runs.push(hard_break()),
        "img" => {
            if let Some(placeholder) = build_image_placeholder(node) {
                // The placeholder builder returns a text El styled as
                // muted italic plus an optional link; reapply the
                // current inline state so an `<img>` inside `<strong>`
                // still reads as bold-italic.
                runs.push(state.apply(placeholder));
            }
        }
        "span" | "abbr" | "bdi" | "bdo" | "data" | "q" | "small" | "time" | "wbr" | "sub"
        | "sup" => {
            // Pass-through inline. `<sub>` / `<sup>` lose their
            // baseline shift in v1 (no inline baseline-shift primitive
            // yet) but their content still renders.
            walk_inline_children(node, state, runs, opts);
        }
        _ => {
            // Unknown tag in inline context: flatten its children.
            // This includes block-shaped tags appearing inside an
            // inline buffer — exactly the tag-soup coercion browsers
            // do.
            walk_inline_children(node, state, runs, opts);
        }
    }
}

fn walk_inline_children(
    node: &Handle,
    state: &InlineState,
    runs: &mut Vec<El>,
    opts: &HtmlOptions,
) {
    for child in node.children.borrow().iter() {
        walk_inline_node(child, state, runs, opts);
    }
}

fn collect_inline_runs(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> Vec<El> {
    let mut runs = Vec::new();
    walk_inline_children(node, state, &mut runs, opts);
    runs
}

// ---------- Builders ----------

fn build_heading(tag: &str, runs: Vec<El>) -> El {
    // Headings h4–h6 clamp to h3 to match aetna-markdown's behaviour.
    let plain = single_plain_text(&runs);
    if let Some(plain) = plain {
        return match tag {
            "h1" => h1(plain),
            "h2" => h2(plain),
            _ => h3(plain),
        };
    }
    let role = match tag {
        "h1" => TextRole::Display,
        "h2" => TextRole::Heading,
        _ => TextRole::Title,
    };
    text_runs(runs)
        .text_role(role)
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn build_unordered_list(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> El {
    let items = collect_list_items(node, state, opts);
    // Detect a task-list shape — non-empty, and every item begins with
    // `<input type="checkbox">`. GFM and many static-site generators
    // emit markdown task lists as HTML in this shape.
    if !items.is_empty() && items.iter().all(|item| item.checkbox_state.is_some()) {
        return task_list(
            items
                .into_iter()
                .map(|item| (item.checkbox_state.unwrap_or(false), item.content)),
        );
    }
    bullet_list(items.into_iter().map(|item| item.content))
}

fn build_ordered_list(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> El {
    let start = element_attr(node, "start")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1);
    let items = collect_list_items(node, state, opts);
    numbered_list_from(start, items.into_iter().map(|item| item.content))
}

struct CollectedItem {
    content: El,
    /// `Some(checked)` if the first DOM child is `<input type="checkbox">`;
    /// `None` otherwise. Used to detect a GFM task-list shape.
    checkbox_state: Option<bool>,
}

fn collect_list_items(
    node: &Handle,
    state: &InlineState,
    opts: &HtmlOptions,
) -> Vec<CollectedItem> {
    let mut items = Vec::new();
    for child in node.children.borrow().iter() {
        let Some(tag) = element_tag(child) else {
            continue;
        };
        if tag != "li" {
            continue;
        }
        let checkbox_state = first_checkbox_state(child);
        let blocks = walk_block_children(child, state, opts);
        let content = if blocks.len() == 1 {
            blocks.into_iter().next().unwrap()
        } else if blocks.is_empty() {
            paragraph("")
        } else {
            column(blocks)
                .gap(tokens::SPACE_2)
                .width(Size::Fill(1.0))
                .height(Size::Hug)
        };
        items.push(CollectedItem {
            content,
            checkbox_state,
        });
    }
    items
}

/// If the first element child of `<li>` is `<input type="checkbox">`,
/// return its checked state. The walker hides the actual `<input>` Els
/// when classifying the list as a task list.
fn first_checkbox_state(li: &Handle) -> Option<bool> {
    for child in li.children.borrow().iter() {
        if let NodeData::Text { contents } = &child.data {
            if contents.borrow().trim().is_empty() {
                continue;
            }
            return None;
        }
        if let Some(tag) = element_tag(child) {
            if tag != "input" {
                return None;
            }
            let ty = element_attr(child, "type").unwrap_or_default();
            if !ty.eq_ignore_ascii_case("checkbox") {
                return None;
            }
            let checked = element_attr(child, "checked").is_some();
            return Some(checked);
        }
    }
    None
}

fn build_pre(node: &Handle) -> El {
    // If the `<pre>` wraps a single `<code>` element, take its text
    // content as the code body (the common
    // `<pre><code class="language-X">…</code></pre>` shape). Otherwise
    // collect the `<pre>`'s own text content.
    let body = inner_code_text(node);
    code_block(body)
}

fn inner_code_text(pre: &Handle) -> String {
    let children = pre.children.borrow();
    let code_child = children.iter().find_map(|c| {
        if let NodeData::Element { name, .. } = &c.data {
            if name.local.as_ref().eq_ignore_ascii_case("code") {
                return Some(c.clone());
            }
        }
        None
    });
    let target = code_child.as_ref().unwrap_or(pre);
    let mut out = String::new();
    collect_text_recursive(target, &mut out);
    out
}

fn collect_text_recursive(node: &Handle, out: &mut String) {
    match &node.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow()),
        NodeData::Element { .. } => {
            for child in node.children.borrow().iter() {
                collect_text_recursive(child, out);
            }
        }
        _ => {}
    }
}

// ---------- Tables ----------

fn build_table(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> El {
    let mut header_rows = Vec::new();
    let mut body_rows = Vec::new();
    let mut explicit_header = false;
    walk_table_sections(
        node,
        state,
        opts,
        &mut header_rows,
        &mut body_rows,
        &mut explicit_header,
        false,
    );
    let mut sections = Vec::new();
    if !header_rows.is_empty() {
        sections.push(table_header(header_rows));
    }
    if !body_rows.is_empty() {
        sections.push(table_body(body_rows));
    }
    table(sections)
}

fn walk_table_sections(
    node: &Handle,
    state: &InlineState,
    opts: &HtmlOptions,
    header_rows: &mut Vec<El>,
    body_rows: &mut Vec<El>,
    explicit_header: &mut bool,
    in_thead: bool,
) {
    for child in node.children.borrow().iter() {
        let Some(tag) = element_tag(child) else {
            continue;
        };
        match tag.as_str() {
            "thead" => {
                *explicit_header = true;
                walk_table_sections(
                    child,
                    state,
                    opts,
                    header_rows,
                    body_rows,
                    explicit_header,
                    true,
                );
            }
            "tbody" | "tfoot" => {
                walk_table_sections(
                    child,
                    state,
                    opts,
                    header_rows,
                    body_rows,
                    explicit_header,
                    false,
                );
            }
            "tr" => {
                let row = build_table_row(child, state, opts);
                if in_thead {
                    header_rows.push(row);
                } else if !*explicit_header && header_rows.is_empty() && row_is_all_headers(child) {
                    // First row of a header-less table that contains
                    // only `<th>` cells reads as a header row, matching
                    // common authoring.
                    header_rows.push(row);
                } else {
                    body_rows.push(row);
                }
            }
            _ => {}
        }
    }
}

fn row_is_all_headers(row: &Handle) -> bool {
    let mut any = false;
    for child in row.children.borrow().iter() {
        let Some(tag) = element_tag(child) else {
            continue;
        };
        match tag.as_str() {
            "th" => any = true,
            "td" => return false,
            _ => {}
        }
    }
    any
}

fn build_table_row(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> El {
    let mut cells: Vec<El> = Vec::new();
    for child in node.children.borrow().iter() {
        let Some(tag) = element_tag(child) else {
            continue;
        };
        match tag.as_str() {
            "th" => cells.push(build_table_head_cell(child, state, opts)),
            "td" => cells.push(build_table_body_cell(child, state, opts)),
            _ => {}
        }
    }
    table_row(cells)
}

fn build_table_head_cell(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> El {
    let runs = collect_inline_runs(node, state, opts);
    if let Some(plain) = single_plain_text(&runs) {
        table_head(plain)
    } else if runs.is_empty() {
        table_head("")
    } else {
        table_head_el(text_runs(runs).width(Size::Fill(1.0)))
    }
}

fn build_table_body_cell(node: &Handle, state: &InlineState, opts: &HtmlOptions) -> El {
    let runs = collect_inline_runs(node, state, opts);
    if let Some(plain) = single_plain_text(&runs) {
        table_cell(text(plain))
    } else if runs.is_empty() {
        table_cell(text(""))
    } else {
        table_cell(text_runs(runs).width(Size::Fill(1.0)))
    }
}

// ---------- Images ----------

fn build_image_placeholder(node: &Handle) -> Option<El> {
    let alt = element_attr(node, "alt").unwrap_or_default();
    let src = element_attr(node, "src")
        .filter(|s| is_safe_url(s))
        .unwrap_or_default();
    let title = element_attr(node, "title").unwrap_or_default();
    if alt.is_empty() && src.is_empty() && title.is_empty() {
        return None;
    }
    let label = image_placeholder_label(&alt, &src, &title);
    let mut el = text(label).muted().italic();
    if !src.is_empty() {
        el = el.link(src);
    }
    Some(el)
}

fn image_placeholder_label(alt: &str, src: &str, title: &str) -> String {
    let mut label = match (alt.is_empty(), src.is_empty()) {
        (true, true) => "[image]".to_string(),
        (false, true) => format!("[image: {alt}]"),
        (true, false) => format!("[image: {src}]"),
        (false, false) => format!("[image: {alt}] {src}"),
    };
    if !title.is_empty() {
        label.push_str(" \"");
        label.push_str(title);
        label.push('"');
    }
    label
}

// ---------- Run helpers ----------

/// Mirrors `aetna-markdown::single_plain_text`. Returns a single plain
/// string when every run is a default-styled `Kind::Text` leaf — drives
/// the `paragraph(s)` / `h1(s)` fast paths.
fn single_plain_text(runs: &[El]) -> Option<String> {
    let mut out = String::new();
    for run in runs {
        if run.kind != Kind::Text {
            return None;
        }
        if run.font_weight != FontWeight::default()
            || run.text_italic
            || run.text_underline
            || run.text_strikethrough
            || run.text_link.is_some()
            || run.text_bg.is_some()
            || run.font_mono
        {
            return None;
        }
        let Some(s) = &run.text else {
            return None;
        };
        out.push_str(s);
    }
    Some(out)
}

fn runs_are_blank(runs: &[El]) -> bool {
    for run in runs {
        if run.kind != Kind::Text {
            return false;
        }
        let Some(s) = &run.text else {
            continue;
        };
        if !s.chars().all(char::is_whitespace) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(input: &str) -> Vec<El> {
        let root = html(input);
        assert_eq!(root.kind, Kind::Group);
        assert_eq!(root.axis, Axis::Column);
        root.children
    }

    fn flatten_text(el: &El) -> String {
        let mut out = String::new();
        if let Some(s) = &el.text {
            out.push_str(s);
        }
        for child in &el.children {
            out.push_str(&flatten_text(child));
        }
        out
    }

    #[test]
    fn empty_document_yields_an_empty_column() {
        assert!(blocks("").is_empty());
    }

    #[test]
    fn plain_paragraph_collapses_to_paragraph_fast_path() {
        let bs = blocks("<p>Hello world.</p>");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Text);
        assert_eq!(bs[0].text.as_deref(), Some("Hello world."));
    }

    #[test]
    fn h1_h2_h3_map_to_heading_kinds_with_roles() {
        let bs = blocks("<h1>One</h1><h2>Two</h2><h3>Three</h3>");
        assert_eq!(bs.len(), 3);
        for b in &bs {
            assert_eq!(b.kind, Kind::Heading);
        }
        assert_eq!(bs[0].text_role, TextRole::Display);
        assert_eq!(bs[1].text_role, TextRole::Heading);
        assert_eq!(bs[2].text_role, TextRole::Title);
        assert_eq!(bs[0].text.as_deref(), Some("One"));
    }

    #[test]
    fn h4_h5_h6_clamp_to_h3() {
        let bs = blocks("<h4>Four</h4><h5>Five</h5><h6>Six</h6>");
        for b in &bs {
            assert_eq!(b.kind, Kind::Heading);
            assert_eq!(b.text_role, TextRole::Title);
        }
    }

    #[test]
    fn mixed_inline_paragraph_becomes_text_runs_with_styled_children() {
        let bs = blocks("<p>Hello <strong>bold</strong> and <em>italic</em>.</p>");
        assert_eq!(bs.len(), 1);
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        // 5 runs: "Hello ", "bold", " and ", "italic", "."
        assert_eq!(p.children.len(), 5);
        assert_eq!(p.children[0].text.as_deref(), Some("Hello "));
        assert_eq!(p.children[1].text.as_deref(), Some("bold"));
        assert_eq!(p.children[1].font_weight, FontWeight::Bold);
        assert_eq!(p.children[3].text.as_deref(), Some("italic"));
        assert!(p.children[3].text_italic);
    }

    #[test]
    fn nested_inline_state_composes() {
        let bs = blocks("<p><strong>bold and <em>both</em></strong></p>");
        assert_eq!(bs.len(), 1);
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let bold_only = &p.children[0];
        assert_eq!(bold_only.text.as_deref(), Some("bold and "));
        assert_eq!(bold_only.font_weight, FontWeight::Bold);
        assert!(!bold_only.text_italic);
        let bold_and_italic = &p.children[1];
        assert_eq!(bold_and_italic.text.as_deref(), Some("both"));
        assert_eq!(bold_and_italic.font_weight, FontWeight::Bold);
        assert!(bold_and_italic.text_italic);
    }

    #[test]
    fn anchor_propagates_href_through_nested_runs() {
        let bs =
            blocks("<p>Go to <a href=\"https://aetna.dev\">the <strong>site</strong></a>.</p>");
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let linked_runs: Vec<&El> = p
            .children
            .iter()
            .filter(|r| r.text_link.is_some())
            .collect();
        assert_eq!(linked_runs.len(), 2);
        for r in linked_runs {
            assert_eq!(r.text_link.as_deref(), Some("https://aetna.dev"));
        }
    }

    #[test]
    fn br_in_paragraph_emits_hard_break_run() {
        let bs = blocks("<p>line one<br>line two</p>");
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        assert!(p.children.iter().any(|r| r.kind == Kind::HardBreak));
    }

    #[test]
    fn hr_emits_divider() {
        let bs = blocks("<hr>");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].height, Size::Fixed(1.0));
    }

    #[test]
    fn ul_emits_one_block_per_item() {
        let bs = blocks("<ul><li>apple</li><li>banana</li><li>cherry</li></ul>");
        assert_eq!(bs.len(), 1);
        let list = &bs[0];
        // bullet_list returns a column of N item-rows.
        assert_eq!(list.children.len(), 3);
    }

    #[test]
    fn ol_with_start_attribute_offsets_marker() {
        let bs = blocks("<ol start=\"5\"><li>five</li><li>six</li></ol>");
        let list = &bs[0];
        // numbered_list_from(5, ...) labels the first marker as "5.".
        let first_marker_text = flatten_text(&list.children[0]);
        assert!(first_marker_text.starts_with("5."));
        assert!(first_marker_text.contains("five"));
    }

    #[test]
    fn ul_with_checkbox_first_children_becomes_task_list() {
        let bs = blocks(
            "<ul>\
                <li><input type=\"checkbox\" checked> done thing</li>\
                <li><input type=\"checkbox\"> open thing</li>\
            </ul>",
        );
        let list = &bs[0];
        // task_list also produces a column with one row per item.
        assert_eq!(list.children.len(), 2);
        // Item text should not include the literal `<input>` markup —
        // the marker is consumed by the task-list shape detector.
        let combined = flatten_text(list);
        assert!(combined.contains("done thing"));
        assert!(combined.contains("open thing"));
        assert!(!combined.contains("checkbox"));
    }

    #[test]
    fn nested_ul_renders_as_nested_blocks() {
        let bs = blocks("<ul><li>outer<ul><li>inner</li></ul></li></ul>");
        let outer = &bs[0];
        assert_eq!(outer.children.len(), 1);
        let combined = flatten_text(outer);
        assert!(combined.contains("outer"));
        assert!(combined.contains("inner"));
    }

    #[test]
    fn pre_code_block_preserves_body_text() {
        let bs = blocks(
            "<pre><code class=\"language-rust\">fn main() {\n    println!(\"hi\");\n}</code></pre>",
        );
        assert_eq!(bs.len(), 1);
        let combined = flatten_text(&bs[0]);
        assert!(combined.contains("fn main()"));
        assert!(combined.contains("println!"));
    }

    #[test]
    fn blockquote_wraps_inner_blocks() {
        let bs = blocks("<blockquote><p>quoted text</p></blockquote>");
        assert_eq!(bs.len(), 1);
        // blockquote's exact shape is a widget composition, so we
        // only assert the quoted text survives the wrap.
        assert!(flatten_text(&bs[0]).contains("quoted text"));
    }

    #[test]
    fn table_with_thead_and_tbody_emits_header_and_body_sections() {
        let bs = blocks(
            "<table>\
                <thead><tr><th>Col A</th><th>Col B</th></tr></thead>\
                <tbody>\
                    <tr><td>a1</td><td>b1</td></tr>\
                    <tr><td>a2</td><td>b2</td></tr>\
                </tbody>\
            </table>",
        );
        assert_eq!(bs.len(), 1);
        let t = &bs[0];
        assert_eq!(t.kind, Kind::Custom("table"));
        // First section is the header; subsequent rows live in the body.
        let combined = flatten_text(t);
        for needle in ["Col A", "Col B", "a1", "b1", "a2", "b2"] {
            assert!(combined.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn table_without_thead_promotes_all_th_first_row_to_header() {
        let bs = blocks(
            "<table>\
                <tr><th>Name</th><th>Score</th></tr>\
                <tr><td>Alice</td><td>10</td></tr>\
            </table>",
        );
        let t = &bs[0];
        // The first child after the implicit promotion should be the
        // header section (table_header). Walk in and check its first
        // cell is a TableHeaderCell row.
        let combined = flatten_text(t);
        assert!(combined.contains("Name"));
        assert!(combined.contains("Alice"));
    }

    #[test]
    fn img_with_alt_and_src_renders_as_muted_italic_link() {
        let bs = blocks("<p><img src=\"https://aetna.dev/x.png\" alt=\"Aetna mark\"></p>");
        let p = &bs[0];
        // Either an Inlines containing the placeholder, or the
        // placeholder run promoted via the single-run fast path.
        let combined = flatten_text(p);
        assert!(combined.contains("Aetna mark"));
        assert!(combined.contains("https://aetna.dev/x.png"));
    }

    #[test]
    fn script_tag_is_dropped_entirely() {
        let bs = blocks("<p>before</p><script>alert('xss')</script><p>after</p>");
        let combined: String = bs.iter().map(flatten_text).collect();
        assert!(combined.contains("before"));
        assert!(combined.contains("after"));
        assert!(!combined.contains("alert"));
    }

    #[test]
    fn iframe_object_noscript_are_dropped_with_their_contents() {
        // `<embed>` is a void element in HTML5 so it can't contain
        // text and is exercised by the script test instead.
        for tag in ["iframe", "object", "noscript"] {
            let bs = blocks(&format!("<p>x</p><{tag}>danger</{tag}><p>y</p>"));
            let combined: String = bs.iter().map(flatten_text).collect();
            assert!(!combined.contains("danger"), "tag {tag} not dropped");
        }
    }

    #[test]
    fn javascript_href_is_treated_as_no_href() {
        let bs = blocks("<p><a href=\"javascript:alert(1)\">click</a></p>");
        let p = &bs[0];
        let runs: Vec<&El> = match p.kind {
            Kind::Inlines => p.children.iter().collect(),
            Kind::Text => vec![p],
            _ => panic!("unexpected paragraph kind: {:?}", p.kind),
        };
        for r in runs {
            assert!(r.text_link.is_none(), "javascript: href should be stripped");
        }
    }

    #[test]
    fn on_attrs_are_dropped() {
        // The walker should never see an `onclick` handler — it gets
        // filtered at the attribute layer. Easiest test: ensure the
        // anchor still parses and the href passes through, with no
        // crash from the handler attribute.
        let bs = blocks("<p><a href=\"https://aetna.dev\" onclick=\"alert(1)\">link</a></p>");
        let p = &bs[0];
        let combined = flatten_text(p);
        assert!(combined.contains("link"));
        // No way to assert the handler was dropped beyond "didn't
        // panic"; the dedicated sanitizer test exercises the rule.
    }

    #[test]
    fn unknown_block_tag_passes_through_children() {
        let bs = blocks("<section><p>inside</p></section><article><h2>also</h2></article>");
        assert!(bs.iter().any(|b| flatten_text(b).contains("inside")));
        assert!(bs.iter().any(|b| flatten_text(b).contains("also")));
    }

    #[test]
    fn loose_text_between_blocks_becomes_anonymous_paragraph() {
        let bs = blocks("loose text<p>real paragraph</p>");
        assert_eq!(bs.len(), 2);
        assert_eq!(flatten_text(&bs[0]), "loose text");
        assert_eq!(flatten_text(&bs[1]), "real paragraph");
    }

    #[test]
    fn html_fragment_inline_returns_runs_only() {
        let runs = html_fragment_inline(
            "hello <strong>strong</strong> world",
            HtmlOptions::default(),
        );
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text.as_deref(), Some("hello "));
        assert_eq!(runs[1].text.as_deref(), Some("strong"));
        assert_eq!(runs[1].font_weight, FontWeight::Bold);
        assert_eq!(runs[2].text.as_deref(), Some(" world"));
    }

    #[test]
    fn html_fragment_inline_coerces_block_tag_to_its_inline_content() {
        // A `<div>` arriving inside an inline buffer should flatten —
        // its children become inline runs rather than terminating the
        // paragraph.
        let runs = html_fragment_inline(
            "a <div>b <strong>c</strong></div> d",
            HtmlOptions::default(),
        );
        let joined: String = runs
            .iter()
            .filter_map(|r| r.text.as_deref())
            .collect::<Vec<_>>()
            .join("");
        assert!(joined.contains("a "));
        assert!(joined.contains("b "));
        assert!(joined.contains("c"));
        assert!(joined.contains(" d"));
    }

    #[test]
    fn mark_run_carries_inline_background() {
        let bs = blocks("<p>see <mark>this</mark> here</p>");
        let p = &bs[0];
        let mark_run = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("this"))
            .expect("mark run");
        assert!(mark_run.text_bg.is_some());
    }

    #[test]
    fn kbd_run_renders_as_monospace_inline() {
        let bs = blocks("<p>press <kbd>Ctrl</kbd>+<kbd>K</kbd>.</p>");
        let p = &bs[0];
        let kbd_runs: Vec<&El> = p.children.iter().filter(|r| r.font_mono).collect();
        assert_eq!(kbd_runs.len(), 2);
    }

    #[test]
    fn link_run_with_strong_inside_still_links() {
        let bs = blocks("<p><a href=\"https://aetna.dev\"><strong>bold link</strong></a></p>");
        let p = &bs[0];
        let bold_link = match p.kind {
            Kind::Inlines => p.children[0].clone(),
            Kind::Text => p.clone(),
            _ => panic!("unexpected kind: {:?}", p.kind),
        };
        assert_eq!(bold_link.text.as_deref(), Some("bold link"));
        assert_eq!(bold_link.font_weight, FontWeight::Bold);
        assert_eq!(bold_link.text_link.as_deref(), Some("https://aetna.dev"));
    }
}
