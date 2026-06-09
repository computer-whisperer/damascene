//! DOM walker that maps the tier-1 HTML tag set onto Damascene `El` widgets.
//!
//! The walker mirrors `damascene-markdown::Walker` in spirit: a small flat
//! `InlineState` carries italic / bold / strike / underline / mono /
//! link / inline-color through nested inline tags, and a context split
//! (`walk_block_children` vs `walk_inline_children`) decides whether
//! each child is rendered as a block-level Damascene widget or appended to
//! an inline run buffer.
//!
//! Unknown tags fall back to a context-sensitive pass-through —
//! block-context: recurse as block; inline-context: recurse as inline.
//! That matches what browsers do with tag-soup HTML and keeps the
//! transformer total: every input produces an El, even if some content
//! gets flattened.

use damascene_core::prelude::*;
// `namespace_url` is the trait the `ns!()` macro consumes via fully-
// qualified path. Without it in scope the macro expands to an empty
// atom and every `name.ns != ns!(html)` comparison spuriously rejects
// HTML elements.
#[allow(unused_imports)]
use html5ever::namespace_url;
use html5ever::ns;
use markup5ever_rcdom::{Handle, NodeData};

use crate::css::{ComputedStyle, read_inline_style};
use crate::lints::{Finding, FindingKind, Lints};
use crate::options::HtmlOptions;
use crate::parser::{parse_document_dom, parse_fragment_dom};
use crate::sanitize::{is_blocked_attr, is_blocked_tag, is_safe_url};
use crate::selectors::Stylesheet;

/// Walker-wide context — read-only options bag, the cascaded
/// stylesheet collected from `<style>` blocks at entry, and the
/// lint collector that the per-element apply / margin-reconciliation
/// passes push findings into. Threaded through every walker function
/// so the tag dispatchers can read all three without separate
/// parameter plumbing.
struct WalkCx<'a> {
    opts: &'a HtmlOptions,
    stylesheet: &'a Stylesheet,
    lints: &'a Lints,
}

/// Render an HTML document as an Damascene `El`. Returns a `column([...])`
/// of block-level Damascene widgets — the same shape an author would have
/// hand-written, and the same shape `damascene_markdown::md` returns.
pub fn html(input: &str) -> El {
    html_with_options(input, HtmlOptions::default())
}

/// Render an HTML document with explicit options.
pub fn html_with_options(input: &str, opts: HtmlOptions) -> El {
    html_with_lints(input, opts).0
}

/// Like [`html_with_options`] but also returns the lint findings
/// gathered during the walk. Use this when you need to surface
/// dropped declarations, unsupported selectors, or margin-asymmetry
/// reconciliations to the author or downstream tools.
pub fn html_with_lints(input: &str, opts: HtmlOptions) -> (El, Vec<Finding>) {
    // `document` must stay in scope for the duration of the walk.
    // `markup5ever_rcdom::Node::Drop` iteratively `mem::take`s the
    // children of every descendant to avoid stack overflow on deep
    // trees; if the document handle drops while we still hold a body
    // sub-handle, the body's `children` Vec is silently emptied.
    let document = parse_document_dom(input);
    let lints = Lints::default();
    let stylesheet = collect_stylesheets(&document, &opts, &lints);
    let body = find_body(&document).unwrap_or_else(|| document.clone());
    let cx = WalkCx {
        opts: &opts,
        stylesheet: &stylesheet,
        lints: &lints,
    };
    let state = InlineState::default();
    let seq = walk_block_children(&body, &state, &cx);
    let gap = seq.gap.unwrap_or(tokens::SPACE_4);
    let leading_pad = seq.leading_pad;
    let trailing_pad = seq.trailing_pad;
    let mut el = column(seq.blocks)
        .gap(gap)
        .width(Size::Fill(1.0))
        .height(Size::Hug);
    if let Some(top) = leading_pad {
        el = el.pt(top);
    }
    if let Some(bottom) = trailing_pad {
        el = el.pb(bottom);
    }
    (el, lints.into_vec())
}

/// Like [`html_with_options`] but returns the block-level Els
/// directly instead of wrapping them in a `column`. Intended for
/// callers (e.g. `damascene-markdown`'s block-HTML event handler) that
/// already have a containing block frame and just want the produced
/// children appended.
pub fn html_blocks(input: &str, opts: HtmlOptions) -> Vec<El> {
    html_blocks_with_lints(input, opts).0
}

/// Lints-returning sibling of [`html_blocks`]. The reconciled `gap`
/// is not surfaced — the calling block frame already owns its rhythm
/// — but margin-asymmetry findings still land in the result.
pub fn html_blocks_with_lints(input: &str, opts: HtmlOptions) -> (Vec<El>, Vec<Finding>) {
    let document = parse_document_dom(input);
    let lints = Lints::default();
    let stylesheet = collect_stylesheets(&document, &opts, &lints);
    let body = find_body(&document).unwrap_or_else(|| document.clone());
    let cx = WalkCx {
        opts: &opts,
        stylesheet: &stylesheet,
        lints: &lints,
    };
    let state = InlineState::default();
    let seq = walk_block_children(&body, &state, &cx);
    (seq.blocks, lints.into_vec())
}

/// Inline-only entry point: parse `input` as an HTML fragment and
/// return the inline runs it produces. The intended caller is
/// `damascene-markdown`'s `Event::InlineHtml` handler, which buffers
/// consecutive inline-HTML events into one string, hands the buffer
/// here, and appends the produced runs to the open paragraph /
/// heading / link / table cell.
///
/// Block-level tags appearing in the fragment are flattened: their
/// children render inline in source order rather than terminating the
/// paragraph.
pub fn html_fragment_inline(input: &str, opts: HtmlOptions) -> Vec<El> {
    html_fragment_inline_with_lints(input, opts).0
}

/// Lints-returning sibling of [`html_fragment_inline`].
pub fn html_fragment_inline_with_lints(input: &str, opts: HtmlOptions) -> (Vec<El>, Vec<Finding>) {
    // See `html_with_options` for the rcdom drop trap — keep
    // `document` alive for the whole walk.
    let document = parse_fragment_dom(input);
    let lints = Lints::default();
    let stylesheet = collect_stylesheets(&document, &opts, &lints);
    let root = find_fragment_root(&document).unwrap_or_else(|| document.clone());
    let cx = WalkCx {
        opts: &opts,
        stylesheet: &stylesheet,
        lints: &lints,
    };
    let state = InlineState::default();
    let mut runs = Vec::new();
    for child in root.children.borrow().iter() {
        walk_inline_node(child, &state, &mut runs, &cx);
    }
    (runs, lints.into_vec())
}

/// Walk the DOM collecting the text contents of every `<style>`
/// element and parse them into a single [`Stylesheet`]. Source order
/// is preserved across blocks. `<head>` and other sanitize-blocked
/// ancestors don't gate this walk — the cascade is the only way to
/// reach `<style>` inside `<head>`, so we descend everywhere except
/// `<script>` / `<iframe>` / friends.
fn collect_stylesheets(root: &Handle, opts: &HtmlOptions, lints: &Lints) -> Stylesheet {
    let mut bodies: Vec<String> = Vec::new();
    walk_for_style_blocks(root, &mut bodies);
    if opts.sanitize_styles {
        if !bodies.is_empty() {
            lints.push(
                FindingKind::SanitizedStyle,
                format!(
                    "{} <style> block(s) dropped by sanitize_styles",
                    bodies.len()
                ),
            );
        }
        return Stylesheet::default();
    }
    Stylesheet::from_blocks(bodies.iter().map(|s| s.as_str()), lints)
}

fn walk_for_style_blocks(node: &Handle, out: &mut Vec<String>) {
    if let NodeData::Element { name, .. } = &node.data {
        let local = name.local.as_ref().to_ascii_lowercase();
        // Always recurse into structural elements (html/head/body)
        // even though `is_blocked_tag` blocks them from rendering.
        // Bail out of execution-context tags so we don't walk into
        // script/style payloads we shouldn't.
        if matches!(local.as_str(), "script" | "iframe" | "noscript") {
            return;
        }
        if local == "style" {
            let mut body = String::new();
            collect_text_recursive(node, &mut body);
            if !body.trim().is_empty() {
                out.push(body);
            }
            // `<style>` contents aren't recursed for nested style
            // elements (none in valid HTML).
            return;
        }
    }
    for child in node.children.borrow().iter() {
        walk_for_style_blocks(child, out);
    }
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

/// Split an element's `class` attribute on whitespace.
fn element_classes(node: &Handle) -> Vec<String> {
    element_attr(node, "class")
        .map(|s| {
            s.split_ascii_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Read the cascaded style for `node`: matching `<style>` block rules
/// (sorted by specificity then source order) flattened, then the
/// element's inline `style="..."` declarations layered on top so
/// inline always wins over the cascade — matches CSS's
/// "author-stylesheet inline beats non-inline" rule and is the
/// expected mental model for embedded scrap authors.
fn cascade_style(node: &Handle, cx: &WalkCx<'_>) -> ComputedStyle {
    let mut style = if cx.stylesheet.is_empty() {
        ComputedStyle::default()
    } else {
        let tag = element_tag(node).unwrap_or_default();
        let classes = element_classes(node);
        let class_refs: Vec<&str> = classes.iter().map(String::as_str).collect();
        let id = element_attr(node, "id");
        cx.stylesheet.cascade(&tag, &class_refs, id.as_deref())
    };
    let inline = read_inline_style(node, cx.lints, cx.opts.sanitize_styles);
    style.merge(&inline);
    style
}

// ---------- Inline state ----------

/// Inline styling currently in effect for new text runs. Mirrors
/// `damascene-markdown::InlineState` but extends it to the HTML-specific
/// tags (`<u>`, `<kbd>`, `<mark>`, `<code>` as an inline run), to
/// `<a href>` which carries a value rather than just a flag, and to
/// per-element `style="..."` overrides.
#[derive(Default, Clone)]
struct InlineState {
    // Tag-derived depth counters. Bumped on entry into a styled tag
    // (`<strong>`, `<em>`, …) and consulted by `apply` to decide which
    // boolean / role modifier to layer onto each text leaf.
    italic_depth: u32,
    bold_depth: u32,
    strike_depth: u32,
    underline_depth: u32,
    code_depth: u32,
    mono_depth: u32,

    // Value overrides (innermost wins). Sourced from semantic tags
    // (`<mark>` sets `text_bg`, `<a>` sets `link`) and from per-
    // element `style="..."`. CSS declarations beat tag defaults
    // because the dispatcher applies style overrides after tag updates.
    text_color: Option<Color>,
    text_bg: Option<Color>,
    font_size: Option<f32>,
    font_weight: Option<FontWeight>,
    /// Most-recent open `<a href="...">`. Inline tags inside an `<a>`
    /// inherit the same href so the painter groups them as one link.
    link: Option<String>,
}

impl InlineState {
    fn apply(&self, mut el: El) -> El {
        // Explicit weight override wins over `<strong>`-derived bold.
        if let Some(w) = self.font_weight {
            el = el.font_weight(w);
        } else if self.bold_depth > 0 {
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
        if let Some(c) = self.text_color {
            el = el.text_color(c);
        }
        if let Some(c) = self.text_bg {
            el = el.background(c);
        }
        if let Some(s) = self.font_size {
            el = el.font_size(s);
        }
        if let Some(href) = &self.link {
            el = el.link(href.clone());
        }
        el
    }

    /// Fold CSS-shaped value overrides into the state in place. Called
    /// after a tag update so explicit `style="..."` declarations beat
    /// the tag's default (e.g. `<mark style="background: blue">` uses
    /// blue, not the WARNING-yellow `<mark>` default).
    fn merge_style_overrides(&mut self, style: &ComputedStyle) {
        if let Some(c) = style.text_color {
            self.text_color = Some(c);
        }
        if let Some(c) = style.background {
            self.text_bg = Some(c);
        }
        if let Some(s) = style.font_size {
            self.font_size = Some(s);
        }
        if let Some(w) = style.font_weight {
            self.font_weight = Some(w);
        }
        if let Some(true) = style.italic {
            self.italic_depth += 1;
        }
        if let Some(true) = style.underline {
            self.underline_depth += 1;
        }
        if let Some(true) = style.strikethrough {
            self.strike_depth += 1;
        }
        if let Some(true) = style.font_mono {
            // `font-family: monospace` (and friends) bumps the mono
            // counter the same way `<kbd>` does, so the apply path
            // already handles it — no new branch needed.
            self.mono_depth += 1;
        }
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
            | "button"
            | "cite"
            | "code"
            | "data"
            | "dfn"
            | "em"
            | "i"
            | "img"
            | "input"
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

/// Reconciled output of [`walk_block_children`]: the produced block
/// Els plus the parent-side rhythm derived from sibling margins.
///
/// `gap` is `Some(px)` when at least one block child declared a
/// margin and the per-sibling-pair `max(prev.margin_bottom,
/// next.margin_top)` values agreed. Asymmetric pairs collapse to the
/// largest value seen and emit a [`FindingKind::MarginAsymmetryFlattened`].
///
/// `leading_pad` / `trailing_pad` are the first child's `margin-top`
/// / last child's `margin-bottom` lifted as caller-facing hints —
/// `html_with_lints` folds them into the outer column's padding when
/// no padding is otherwise declared. Container builders that own
/// their own padding (`blockquote`, `figure`) ignore these fields.
pub(crate) struct BlockSequence {
    pub blocks: Vec<El>,
    pub gap: Option<f32>,
    pub leading_pad: Option<f32>,
    pub trailing_pad: Option<f32>,
}

fn walk_block_children(parent: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> BlockSequence {
    let mut produced: Vec<(El, Option<Sides>)> = Vec::new();
    let mut inline_buf: Vec<El> = Vec::new();
    for child in parent.children.borrow().iter() {
        if is_inline_node(child) {
            walk_inline_node(child, state, &mut inline_buf, cx);
        } else {
            flush_inline_buf(&mut inline_buf, &mut produced);
            walk_block_node(child, state, &mut produced, cx);
        }
    }
    flush_inline_buf(&mut inline_buf, &mut produced);
    reconcile_margins(produced, cx)
}

/// Fold an accumulated inline-run buffer into an anonymous paragraph
/// block. Drops a buffer that contains only whitespace runs.
/// Anonymous paragraphs contribute no margin to the parent's
/// reconciliation — they're a synthetic construct, not an authored
/// element.
fn flush_inline_buf(inline_buf: &mut Vec<El>, blocks: &mut Vec<(El, Option<Sides>)>) {
    if inline_buf.is_empty() {
        return;
    }
    let runs = normalize_inline_runs(std::mem::take(inline_buf));
    if runs.is_empty() || runs_are_blank(&runs) {
        return;
    }
    blocks.push((build_paragraph(runs), None));
}

/// Collapse adjacent sibling margins into a single parent `gap`,
/// matching CSS's `max(prev.margin_bottom, next.margin_top)` rule.
/// When pair values agree, the gap is exact and lossless. When they
/// disagree we pick the largest and emit a lint so the author knows
/// the rhythm collapsed.
fn reconcile_margins(produced: Vec<(El, Option<Sides>)>, cx: &WalkCx<'_>) -> BlockSequence {
    let leading_pad = produced
        .first()
        .and_then(|(_, m)| m.map(|s| s.top))
        .filter(|v| *v > 0.0);
    let trailing_pad = produced
        .last()
        .and_then(|(_, m)| m.map(|s| s.bottom))
        .filter(|v| *v > 0.0);

    let mut pair_gaps: Vec<f32> = Vec::with_capacity(produced.len().saturating_sub(1));
    for pair in produced.windows(2) {
        let prev_bottom = pair[0].1.map(|s| s.bottom).unwrap_or(0.0);
        let next_top = pair[1].1.map(|s| s.top).unwrap_or(0.0);
        pair_gaps.push(prev_bottom.max(next_top));
    }

    let gap = if pair_gaps.is_empty() {
        None
    } else if pair_gaps.iter().all(|&g| g == pair_gaps[0]) {
        if pair_gaps[0] > 0.0 {
            Some(pair_gaps[0])
        } else {
            None
        }
    } else {
        let max = pair_gaps.iter().cloned().fold(0.0_f32, f32::max);
        let min = pair_gaps.iter().cloned().fold(f32::INFINITY, f32::min);
        cx.lints.push(
            FindingKind::MarginAsymmetryFlattened,
            format!(
                "sibling pair margins ranged {min}..{max}px; flattened to {max}px gap on parent"
            ),
        );
        Some(max)
    };

    let blocks: Vec<El> = produced.into_iter().map(|(el, _)| el).collect();
    BlockSequence {
        blocks,
        gap,
        leading_pad,
        trailing_pad,
    }
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

fn walk_block_node(
    node: &Handle,
    state: &InlineState,
    blocks: &mut Vec<(El, Option<Sides>)>,
    cx: &WalkCx<'_>,
) {
    let Some(tag) = element_tag(node) else {
        return;
    };
    if is_blocked_tag(&tag) {
        return;
    }
    if is_unsupported_block_tag(&tag) {
        cx.lints.push(
            FindingKind::UnsupportedTag,
            format!("<{tag}> has no Damascene equivalent; contents flattened"),
        );
        // Fall through to the unknown-tag arm below — flatten content
        // so authored text isn't lost.
    }
    let style = cascade_style(node, cx);
    let margin = style.margin;
    match tag.as_str() {
        "p" => {
            let runs = collect_inline_runs(node, state, cx);
            if !runs_are_blank(&runs) {
                blocks.push((style.apply_to_block(build_paragraph(runs)), margin));
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let runs = collect_inline_runs(node, state, cx);
            blocks.push((style.apply_to_block(build_heading(&tag, runs)), margin));
        }
        "br" => {
            blocks.push((style.apply_to_block(paragraph("")), margin));
        }
        "hr" => blocks.push((style.apply_to_block(divider()), margin)),
        "ul" => blocks.push((
            style.apply_to_block(build_unordered_list(node, state, cx)),
            margin,
        )),
        "ol" => blocks.push((
            style.apply_to_block(build_ordered_list(node, state, cx)),
            margin,
        )),
        "blockquote" => {
            let inner = walk_block_children(node, state, cx);
            blocks.push((style.apply_to_block(blockquote(inner.blocks)), margin));
        }
        "pre" => blocks.push((style.apply_to_block(build_pre(node)), margin)),
        "table" => blocks.push((style.apply_to_block(build_table(node, state, cx)), margin)),
        "img" => {
            if let Some(placeholder) = build_image_placeholder(node) {
                blocks.push((style.apply_to_block(placeholder), margin));
            }
        }
        "details" => blocks.push((style.apply_to_block(build_details(node, state, cx)), margin)),
        "figure" => blocks.push((style.apply_to_block(build_figure(node, state, cx)), margin)),
        // Generic block containers — pass through to children unless
        // the element carries CSS, in which case wrap the children in
        // a styled column so the layout / visual properties have a
        // surface to land on. Untouched containers stay flat so a
        // `<section>` wrapping a single `<h2>` doesn't gain a useless
        // extra level of nesting.
        "div" | "section" | "article" | "main" | "header" | "footer" | "nav" | "aside"
        | "summary" | "figcaption" | "form" | "fieldset" | "legend" | "body" | "html" => {
            push_generic_container(node, state, &style, margin, blocks, cx);
        }
        _ => {
            push_generic_container(node, state, &style, margin, blocks, cx);
        }
    }
}

/// Tags we don't render but want to call out so authors see what's
/// dropped. These slip past `is_blocked_tag` (which is the security
/// filter) and reach the walker as unknown content. We still recurse
/// into their children to preserve any text, but emit a finding so
/// the author knows the wrapping element was lost.
fn is_unsupported_block_tag(tag: &str) -> bool {
    matches!(
        tag,
        "video" | "audio" | "canvas" | "dialog" | "menu" | "marquee" | "applet" | "bgsound"
    )
}

/// Wrap a generic-container element's children in a styled `column`
/// (when CSS calls for it) or pass them through flat. Honors the
/// child-margin reconciliation by folding the inner sequence's `gap`
/// onto the wrapper, and applies any container-layout overrides
/// (display:flex / align-items / justify-content / overflow) before
/// the wrap-in-scroll step.
fn push_generic_container(
    node: &Handle,
    state: &InlineState,
    style: &ComputedStyle,
    margin: Option<Sides>,
    blocks: &mut Vec<(El, Option<Sides>)>,
    cx: &WalkCx<'_>,
) {
    let inner = walk_block_children(node, state, cx);
    let needs_wrap = !style.is_empty();
    if !needs_wrap {
        // Flat pass-through. The child sequence's reconciliation
        // results are dropped here — the surrounding parent's
        // reconciliation already collapses across all flattened
        // descendants in source order via the same pass.
        blocks.extend(inner.blocks.into_iter().map(|el| (el, None)));
        return;
    }
    let gap = inner.gap.unwrap_or(0.0);
    let mut wrapper = column(inner.blocks)
        .gap(gap)
        .width(Size::Fill(1.0))
        .height(Size::Hug);
    // Fold the inner sequence's leading / trailing margin pads onto
    // the wrapper's padding when CSS hasn't declared one explicitly.
    if style.padding.is_none() {
        if let Some(top) = inner.leading_pad {
            wrapper = wrapper.pt(top);
        }
        if let Some(bottom) = inner.trailing_pad {
            wrapper = wrapper.pb(bottom);
        }
    }
    wrapper = style.apply_container_layout(wrapper);
    wrapper = style.apply_to_block(wrapper);
    wrapper = style.wrap_with_overflow(wrapper);
    blocks.push((wrapper, margin));
}

// ---------- Inline walker ----------

fn walk_inline_node(node: &Handle, state: &InlineState, runs: &mut Vec<El>, cx: &WalkCx<'_>) {
    match &node.data {
        NodeData::Text { contents } => {
            let s = contents.borrow().to_string();
            if s.is_empty() {
                return;
            }
            // CSS `white-space: normal`: runs of document whitespace
            // (including the newlines + indentation of pretty-printed
            // source) collapse to a single space. Damascene's text
            // pipeline treats a literal `\n` as a hard line break, so
            // skipping this would turn source formatting into visible
            // breaks. Block-edge trimming happens later, in
            // `normalize_inline_runs`, once the full run sequence is
            // known.
            runs.push(state.apply(text(collapse_whitespace(&s))));
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
            dispatch_inline_element(node, &tag, state, runs, cx);
        }
        _ => {}
    }
}

fn dispatch_inline_element(
    node: &Handle,
    tag: &str,
    state: &InlineState,
    runs: &mut Vec<El>,
    cx: &WalkCx<'_>,
) {
    match tag {
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
        "button" => {
            let label = inline_text_only(node);
            runs.push(button(label));
        }
        "input" => {
            if let Some(el) = build_html_input(node) {
                runs.push(el);
            }
        }
        _ => {
            let next = child_inline_state(node, tag, state, cx);
            walk_inline_children(node, &next, runs, cx);
        }
    }
}

/// Collect plain text content from an element's subtree, ignoring
/// inline markup. Used by `<button>` to extract a flat label string.
fn inline_text_only(node: &Handle) -> String {
    let mut out = String::new();
    collect_text_recursive(node, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Build the cosmetic Damascene widget for an HTML `<input>` element. v1
/// only honours `type="checkbox"`; other input types return `None` so
/// the walker silently skips them.
fn build_html_input(node: &Handle) -> Option<El> {
    let ty = element_attr(node, "type").unwrap_or_else(|| "text".to_string());
    if !ty.eq_ignore_ascii_case("checkbox") {
        return None;
    }
    let checked = element_attr(node, "checked").is_some();
    Some(checkbox(checked))
}

/// Build the inline state that should govern an element's children.
/// Folds the tag's semantic effect (`<strong>` bumps bold depth,
/// `<mark>` sets the highlight background, `<a>` adopts the href)
/// then layers the element's `style="..."` declarations on top so
/// explicit CSS always wins over the tag default.
fn child_inline_state(
    node: &Handle,
    tag: &str,
    state: &InlineState,
    cx: &WalkCx<'_>,
) -> InlineState {
    let mut next = state.clone();
    match tag {
        "strong" | "b" => next.bold_depth += 1,
        "em" | "i" | "cite" | "dfn" | "var" => next.italic_depth += 1,
        "u" => next.underline_depth += 1,
        "s" | "strike" | "del" => next.strike_depth += 1,
        "code" => next.code_depth += 1,
        "kbd" | "samp" => next.mono_depth += 1,
        "mark" => {
            // Soft yellow band behind the glyphs. Snapshot of the
            // theme's WARNING token at build time — explicit
            // `style="background: ..."` on the `<mark>` overrides
            // because the style merge runs after this assignment.
            next.text_bg = Some(tokens::WARNING.with_alpha_u8(60));
        }
        "a" => {
            // Inner `<a>` overrides outer href (browser semantics:
            // nested `<a>` is invalid, but we take the innermost).
            if let Some(href) = element_attr(node, "href").filter(|h| is_safe_url(h)) {
                next.link = Some(href);
            }
        }
        // Pass-through inline tags — no state mutation, but style
        // overrides on a `<span>` still take effect via the merge
        // below. `<sub>` / `<sup>` lose their baseline shift in v1
        // (no inline baseline-shift primitive yet) but their content
        // still renders.
        "span" | "abbr" | "bdi" | "bdo" | "data" | "q" | "small" | "time" | "wbr" | "sub"
        | "sup" => {}
        // Unknown tag in inline context: flatten its children. This
        // includes block-shaped tags appearing inside an inline
        // buffer — exactly the tag-soup coercion browsers do.
        _ => {}
    }
    let style = cascade_style(node, cx);
    next.merge_style_overrides(&style);
    next
}

fn walk_inline_children(node: &Handle, state: &InlineState, runs: &mut Vec<El>, cx: &WalkCx<'_>) {
    for child in node.children.borrow().iter() {
        walk_inline_node(child, state, runs, cx);
    }
}

fn collect_inline_runs(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> Vec<El> {
    let mut runs = Vec::new();
    walk_inline_children(node, state, &mut runs, cx);
    normalize_inline_runs(runs)
}

/// Collapse runs of HTML document whitespace (space, tab, CR, LF, FF)
/// into a single space, per CSS `white-space: normal`. Leading/trailing
/// runs become a single edge space here; whether that space survives is
/// decided by [`normalize_inline_runs`] once neighbouring runs are
/// known. U+00A0 and other non-ASCII spaces are deliberately preserved —
/// `&nbsp;` exists to defeat collapsing.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// The cross-run half of CSS whitespace processing, applied to a
/// block's fully assembled inline runs (in-node collapsing is
/// [`collapse_whitespace`]'s job). Removes the spaces that only become
/// removable once neighbours are known: leading space at the block
/// start or after a `<br>`, a space whose preceding text run already
/// ends in one (`foo <b> bar</b>`), and trailing space at the block end
/// or before a `<br>`. Text runs reduced to nothing are dropped.
/// Non-text atoms (buttons, checkboxes, images) act like words —
/// spaces around them survive.
fn normalize_inline_runs(runs: Vec<El>) -> Vec<El> {
    let mut out: Vec<El> = Vec::with_capacity(runs.len());
    // True at the block start and just after a hard break — positions
    // where leading whitespace is dropped entirely.
    let mut at_boundary = true;
    for mut run in runs {
        match run.kind {
            Kind::HardBreak => {
                trim_trailing_edge(&mut out);
                at_boundary = true;
                out.push(run);
            }
            Kind::Text => {
                let prev_ends_in_space = out
                    .last()
                    .is_some_and(|p| matches!(&p.text, Some(t) if t.ends_with(' ')));
                if let Some(t) = run.text.take() {
                    let t = if at_boundary || prev_ends_in_space {
                        t.trim_start_matches(' ').to_string()
                    } else {
                        t
                    };
                    if t.is_empty() {
                        // A whitespace-only run absorbed by the
                        // boundary; the boundary state carries over.
                        continue;
                    }
                    at_boundary = false;
                    run.text = Some(t);
                } else {
                    at_boundary = false;
                }
                out.push(run);
            }
            _ => {
                at_boundary = false;
                out.push(run);
            }
        }
    }
    trim_trailing_edge(&mut out);
    out
}

/// Strip trailing spaces from the text runs at the tail of `out`,
/// dropping runs that become empty. Stops at the first non-text run
/// (a space before a trailing button is interior, not edge).
fn trim_trailing_edge(out: &mut Vec<El>) {
    while let Some(last) = out.last_mut() {
        if last.kind != Kind::Text {
            return;
        }
        let Some(t) = &last.text else {
            return;
        };
        let trimmed = t.trim_end_matches(' ');
        if trimmed.is_empty() {
            out.pop();
            continue;
        }
        if trimmed.len() != t.len() {
            last.text = Some(trimmed.to_string());
        }
        return;
    }
}

// ---------- Builders ----------

fn build_heading(tag: &str, runs: Vec<El>) -> El {
    // Headings h4–h6 clamp to h3 to match damascene-markdown's behaviour.
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

fn build_unordered_list(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let items = collect_list_items(node, state, cx);
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

fn build_ordered_list(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let start = element_attr(node, "start")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1);
    let items = collect_list_items(node, state, cx);
    numbered_list_from(start, items.into_iter().map(|item| item.content))
}

struct CollectedItem {
    content: El,
    /// `Some(checked)` if the first DOM child is `<input type="checkbox">`;
    /// `None` otherwise. Used to detect a GFM task-list shape.
    checkbox_state: Option<bool>,
}

fn collect_list_items(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> Vec<CollectedItem> {
    let mut items = Vec::new();
    for child in node.children.borrow().iter() {
        let Some(tag) = element_tag(child) else {
            continue;
        };
        if tag != "li" {
            continue;
        }
        let checkbox_state = first_checkbox_state(child);
        let seq = walk_block_children(child, state, cx);
        let content = if seq.blocks.len() == 1 {
            seq.blocks.into_iter().next().unwrap()
        } else if seq.blocks.is_empty() {
            paragraph("")
        } else {
            column(seq.blocks)
                .gap(seq.gap.unwrap_or(tokens::SPACE_2))
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

/// Render a `<details>` element as a static cosmetic disclosure: a
/// summary row with a leading chevron (▼ when `open`, ▶ when not),
/// followed by the rest of the children when `open`. No toggle wiring
/// — apps that want interactive behaviour can fork the tier-1 widget
/// or compose `accordion_item` directly with their own state.
fn build_details(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let open = element_attr(node, "open").is_some();
    let chevron = if open { "\u{25BE}" } else { "\u{25B8}" };
    let mut summary_runs: Vec<El> = Vec::new();
    let mut body_blocks: Vec<(El, Option<Sides>)> = Vec::new();
    for child in node.children.borrow().iter() {
        match element_tag(child).as_deref() {
            Some("summary") => {
                summary_runs = collect_inline_runs(child, state, cx);
            }
            _ => {
                if open {
                    let mut buf = Vec::new();
                    let was_inline = is_inline_node(child);
                    if was_inline {
                        walk_inline_node(child, state, &mut buf, cx);
                        if !runs_are_blank(&buf) {
                            body_blocks.push((build_paragraph(buf), None));
                        }
                    } else {
                        walk_block_node(child, state, &mut body_blocks, cx);
                    }
                }
            }
        }
    }
    let body_blocks: Vec<El> = body_blocks.into_iter().map(|(el, _)| el).collect();
    let summary_label: El = if summary_runs.is_empty() {
        text("Details").label()
    } else if let Some(plain) = single_plain_text(&summary_runs) {
        text(plain).label().font_weight(FontWeight::Medium)
    } else {
        text_runs(summary_runs).width(Size::Fill(1.0))
    };
    let summary_row = row([
        text(chevron).text_color(tokens::MUTED_FOREGROUND),
        summary_label,
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .width(Size::Fill(1.0));
    let mut parts: Vec<El> = vec![summary_row];
    if open && !body_blocks.is_empty() {
        parts.push(
            column(body_blocks)
                .gap(tokens::SPACE_2)
                .width(Size::Fill(1.0))
                .height(Size::Hug)
                .padding(Sides::left(tokens::SPACE_4)),
        );
    }
    column(parts)
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

/// Render a `<figure>` as a column where `<figcaption>` children get
/// their inner blocks muted + italicised. Mirrors the markdown image-
/// placeholder visual treatment so figures sit next to images in the
/// same tone.
fn build_figure(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let mut parts: Vec<(El, Option<Sides>)> = Vec::new();
    for child in node.children.borrow().iter() {
        match element_tag(child).as_deref() {
            Some("figcaption") => {
                let seq = walk_block_children(child, state, cx);
                for el in seq.blocks {
                    parts.push((el.muted().italic(), None));
                }
            }
            Some(_) => walk_block_node(child, state, &mut parts, cx),
            None => {
                if is_inline_node(child) {
                    let mut buf = Vec::new();
                    walk_inline_node(child, state, &mut buf, cx);
                    if !runs_are_blank(&buf) {
                        parts.push((build_paragraph(buf), None));
                    }
                }
            }
        }
    }
    column(parts.into_iter().map(|(el, _)| el).collect::<Vec<_>>())
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
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

fn build_table(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let mut header_rows = Vec::new();
    let mut body_rows = Vec::new();
    let mut explicit_header = false;
    walk_table_sections(
        node,
        state,
        cx,
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
    cx: &WalkCx<'_>,
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
                    cx,
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
                    cx,
                    header_rows,
                    body_rows,
                    explicit_header,
                    false,
                );
            }
            "tr" => {
                let row = build_table_row(child, state, cx);
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

fn build_table_row(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let mut cells: Vec<El> = Vec::new();
    for child in node.children.borrow().iter() {
        let Some(tag) = element_tag(child) else {
            continue;
        };
        match tag.as_str() {
            "th" => cells.push(build_table_head_cell(child, state, cx)),
            "td" => cells.push(build_table_body_cell(child, state, cx)),
            _ => {}
        }
    }
    table_row(cells)
}

fn build_table_head_cell(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let runs = collect_inline_runs(node, state, cx);
    if let Some(plain) = single_plain_text(&runs) {
        table_head(plain)
    } else if runs.is_empty() {
        table_head("")
    } else {
        table_head_el(text_runs(runs).width(Size::Fill(1.0)))
    }
}

fn build_table_body_cell(node: &Handle, state: &InlineState, cx: &WalkCx<'_>) -> El {
    let runs = collect_inline_runs(node, state, cx);
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

/// Mirrors `damascene-markdown::single_plain_text`. Returns a single plain
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
        // The Body role's auto-applied FOREGROUND token counts as
        // "default"; any other explicit color (from `style="color:..."`)
        // forces the rich-runs path so the per-run colour survives.
        if let Some(c) = run.text_color
            && c != tokens::FOREGROUND
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
    fn pretty_printed_source_whitespace_collapses_to_single_spaces() {
        // Newlines + indentation in the source are formatting, not
        // line breaks (CSS `white-space: normal`). Damascene treats a
        // literal \n as a hard break, so without collapsing this
        // paragraph would render as three short lines.
        let bs = blocks("<p>\n  This paragraph wraps across\n  indented source lines.\n</p>");
        assert_eq!(bs.len(), 1);
        assert_eq!(
            bs[0].text.as_deref(),
            Some("This paragraph wraps across indented source lines.")
        );
    }

    #[test]
    fn whitespace_collapses_across_inline_element_boundaries() {
        // "foo " + " bar" (bold) must not yield a double space, and
        // block-edge whitespace must trim.
        let bs = blocks("<p>\n  foo <b>\n bar</b>\n</p>");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Inlines);
        let texts: Vec<&str> = bs[0]
            .children
            .iter()
            .filter_map(|r| r.text.as_deref())
            .collect();
        assert_eq!(texts, vec!["foo ", "bar"]);
    }

    #[test]
    fn whitespace_only_text_between_inline_elements_survives_as_separator() {
        let bs = blocks("<p><b>a</b> <b>b</b></p>");
        assert_eq!(bs.len(), 1);
        let texts: Vec<&str> = bs[0]
            .children
            .iter()
            .filter_map(|r| r.text.as_deref())
            .collect();
        assert_eq!(texts, vec!["a", " ", "b"]);
    }

    #[test]
    fn br_still_breaks_and_swallows_adjacent_source_whitespace() {
        let bs = blocks("<p>alpha\n  <br>\n  beta</p>");
        assert_eq!(bs.len(), 1);
        let kinds: Vec<Kind> = bs[0].children.iter().map(|r| r.kind.clone()).collect();
        assert_eq!(kinds, vec![Kind::Text, Kind::HardBreak, Kind::Text]);
        assert_eq!(bs[0].children[0].text.as_deref(), Some("alpha"));
        assert_eq!(bs[0].children[2].text.as_deref(), Some("beta"));
    }

    #[test]
    fn nbsp_is_not_collapsed() {
        let bs = blocks("<p>a&nbsp;&nbsp;b</p>");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].text.as_deref(), Some("a\u{a0}\u{a0}b"));
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
            blocks("<p>Go to <a href=\"https://damascene.dev\">the <strong>site</strong></a>.</p>");
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let linked_runs: Vec<&El> = p
            .children
            .iter()
            .filter(|r| r.text_link.is_some())
            .collect();
        assert_eq!(linked_runs.len(), 2);
        for r in linked_runs {
            assert_eq!(r.text_link.as_deref(), Some("https://damascene.dev"));
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
        let bs = blocks("<p><img src=\"https://damascene.dev/x.png\" alt=\"Damascene mark\"></p>");
        let p = &bs[0];
        // Either an Inlines containing the placeholder, or the
        // placeholder run promoted via the single-run fast path.
        let combined = flatten_text(p);
        assert!(combined.contains("Damascene mark"));
        assert!(combined.contains("https://damascene.dev/x.png"));
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
        let bs = blocks("<p><a href=\"https://damascene.dev\" onclick=\"alert(1)\">link</a></p>");
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
        let bs = blocks("<p><a href=\"https://damascene.dev\"><strong>bold link</strong></a></p>");
        let p = &bs[0];
        let bold_link = match p.kind {
            Kind::Inlines => p.children[0].clone(),
            Kind::Text => p.clone(),
            _ => panic!("unexpected kind: {:?}", p.kind),
        };
        assert_eq!(bold_link.text.as_deref(), Some("bold link"));
        assert_eq!(bold_link.font_weight, FontWeight::Bold);
        assert_eq!(
            bold_link.text_link.as_deref(),
            Some("https://damascene.dev")
        );
    }

    // ---------- CSS tier-2A integration ----------

    #[test]
    fn block_style_attr_applies_background_padding_and_radius() {
        let bs = blocks(
            "<div style=\"background: #ff0000; padding: 12px; border-radius: 4px\">\
                <p>inside</p>\
            </div>",
        );
        // The styled <div> wraps its children in a column with the
        // style applied.
        assert_eq!(bs.len(), 1);
        let wrap = &bs[0];
        assert_eq!(wrap.fill, Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(wrap.padding, Sides::all(12.0));
        assert_eq!(wrap.radius.tl, 4.0);
    }

    #[test]
    fn unstyled_div_stays_flat_no_extra_nesting() {
        // Existing behaviour: <div> with no style passes children through.
        let bs = blocks("<div><p>inside</p></div>");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Text);
        assert_eq!(bs[0].text.as_deref(), Some("inside"));
    }

    #[test]
    fn paragraph_style_applies_to_paragraph_el() {
        let bs = blocks(r#"<p style="text-align: center; color: blue">hi</p>"#);
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Text);
        assert_eq!(p.text.as_deref(), Some("hi"));
        assert_eq!(p.text_align, TextAlign::Center);
        assert_eq!(p.text_color, Some(Color::srgb_u8(0, 0, 255)));
    }

    #[test]
    fn block_style_width_height_resolve_to_damascene_size() {
        let bs = blocks(r#"<div style="width: 240px; height: 50%"><p>x</p></div>"#);
        let wrap = &bs[0];
        assert_eq!(wrap.width, Size::Fixed(240.0));
        assert_eq!(wrap.height, Size::Fill(0.5));
    }

    #[test]
    fn span_style_color_applies_to_inline_run() {
        let bs = blocks(r#"<p>hello <span style="color: #00ff00">green</span> world</p>"#);
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let green = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("green"))
            .expect("green run");
        assert_eq!(green.text_color, Some(Color::srgb_u8(0, 255, 0)));
    }

    #[test]
    fn span_style_overrides_outer_mark_background() {
        let bs =
            blocks(r#"<p><mark>outer <span style="background: #0000ff">inner</span></mark></p>"#);
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let outer = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("outer "))
            .expect("outer run");
        let inner = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("inner"))
            .expect("inner run");
        // Outer keeps the mark's yellow.
        assert_eq!(outer.text_bg, Some(tokens::WARNING.with_alpha_u8(60)));
        // Inner's style attr wins.
        assert_eq!(inner.text_bg, Some(Color::srgb_u8(0, 0, 255)));
    }

    #[test]
    fn span_style_font_weight_and_font_style_compose_with_tag_state() {
        let bs = blocks(
            r#"<p><strong>bold <span style="font-style: italic; font-size: 24px">and italic</span></strong></p>"#,
        );
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let bold_only = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("bold "))
            .expect("bold-only run");
        let bold_italic = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("and italic"))
            .expect("bold + italic run");
        assert_eq!(bold_only.font_weight, FontWeight::Bold);
        assert!(!bold_only.text_italic);
        assert_eq!(bold_italic.font_weight, FontWeight::Bold);
        assert!(bold_italic.text_italic);
        assert_eq!(bold_italic.font_size, 24.0);
    }

    #[test]
    fn style_attr_with_invalid_value_silently_drops_that_decl() {
        // padding is malformed; color and font-weight still apply.
        let bs = blocks(r#"<p style="color: red; padding: bogus; font-weight: 700">hello</p>"#);
        let p = &bs[0];
        assert_eq!(p.text_color, Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(p.font_weight, FontWeight::Bold);
        assert_eq!(p.padding, Sides::zero());
    }

    #[test]
    fn ul_style_applies_to_outer_list_container() {
        let bs = blocks(r#"<ul style="padding: 16px; background: #eee"><li>a</li><li>b</li></ul>"#);
        let list = &bs[0];
        assert_eq!(list.padding, Sides::all(16.0));
        assert_eq!(list.fill, Some(Color::srgb_u8(238, 238, 238)));
    }

    // ---------- tier-2C: details / figure / button / input ----------

    #[test]
    fn details_without_open_shows_only_summary() {
        let bs = blocks("<details><summary>more</summary><p>body</p></details>");
        assert_eq!(bs.len(), 1);
        let combined = flatten_text(&bs[0]);
        assert!(combined.contains("more"));
        assert!(!combined.contains("body"));
    }

    #[test]
    fn details_with_open_attr_shows_summary_and_body() {
        let bs = blocks("<details open><summary>more</summary><p>body</p></details>");
        let combined = flatten_text(&bs[0]);
        assert!(combined.contains("more"));
        assert!(combined.contains("body"));
    }

    #[test]
    fn details_without_summary_renders_placeholder_label() {
        let bs = blocks("<details open><p>orphan body</p></details>");
        let combined = flatten_text(&bs[0]);
        assert!(combined.contains("Details"));
        assert!(combined.contains("orphan body"));
    }

    #[test]
    fn figure_with_figcaption_applies_muted_italic_to_caption() {
        let bs = blocks(
            "<figure><img src=\"https://damascene.dev/x.png\" alt=\"img\"><figcaption>caption text</figcaption></figure>",
        );
        assert_eq!(bs.len(), 1);
        let fig = &bs[0];
        // The figcaption's block is the last child, with muted+italic.
        let caption = fig
            .children
            .iter()
            .find(|c| c.text.as_deref() == Some("caption text"))
            .expect("caption block");
        assert!(caption.text_italic);
        // .muted() on a text leaf swaps text_color to MUTED_FOREGROUND.
        assert_eq!(caption.text_color, Some(tokens::MUTED_FOREGROUND));
    }

    #[test]
    fn standalone_button_renders_as_button_widget() {
        // <button> is inline-classified; a standalone block-level
        // button arrives as an anonymous paragraph wrapping the
        // button run.
        let bs = blocks("<button>Save</button>");
        assert_eq!(bs.len(), 1);
        // Find a Custom("button") leaf anywhere in the produced tree.
        let mut found = false;
        fn search(el: &El, found: &mut bool) {
            if el.kind == Kind::Custom("button") {
                *found = true;
            }
            for c in &el.children {
                search(c, found);
            }
        }
        search(&bs[0], &mut found);
        assert!(found, "expected a button widget in the tree");
    }

    #[test]
    fn button_inside_paragraph_flows_inline_with_text() {
        let bs = blocks("<p>click <button>here</button> please</p>");
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        // 3 runs: "click ", <button>here</button>, " please"
        assert_eq!(p.children.len(), 3);
        assert_eq!(p.children[0].text.as_deref(), Some("click "));
        assert_eq!(p.children[1].kind, Kind::Custom("button"));
        assert_eq!(p.children[2].text.as_deref(), Some(" please"));
    }

    #[test]
    fn standalone_input_checkbox_renders_with_checked_state() {
        let bs = blocks(r#"<input type="checkbox" checked>"#);
        // The checkbox widget is a styled bool. Tag is Kind::Custom.
        let mut found_kind: Option<Kind> = None;
        fn search(el: &El, found: &mut Option<Kind>) {
            if matches!(el.kind, Kind::Custom(_)) && found.is_none() {
                *found = Some(el.kind.clone());
            }
            for c in &el.children {
                search(c, found);
            }
        }
        search(&bs[0], &mut found_kind);
        assert!(found_kind.is_some(), "expected a custom widget kind");
    }

    #[test]
    fn input_non_checkbox_is_silently_dropped() {
        let bs = blocks(r#"<p>before <input type="text" value="ignored"> after</p>"#);
        let p = &bs[0];
        // Should be one paragraph with "before  after" — input dropped.
        let combined = flatten_text(p);
        assert!(combined.contains("before"));
        assert!(combined.contains("after"));
        assert!(!combined.contains("ignored"));
    }

    // ---------- tier-2B: <style> block + selector cascade ----------

    #[test]
    fn style_block_tag_selector_applies_to_matching_elements() {
        let bs =
            blocks(r#"<style>p { color: red }</style><p>red text</p><h1>untouched heading</h1>"#);
        // The <style> tag itself is blocked from rendering.
        let combined: String = bs.iter().map(flatten_text).collect();
        assert!(!combined.contains("color: red"));
        // Find the paragraph and confirm the rule landed.
        let p = bs
            .iter()
            .find(|b| b.text.as_deref() == Some("red text"))
            .expect("matching paragraph");
        assert_eq!(p.text_color, Some(Color::srgb_u8(255, 0, 0)));
        // The h1 has its own role default — the rule shouldn't touch it.
        let h = bs
            .iter()
            .find(|b| b.text.as_deref() == Some("untouched heading"))
            .expect("heading");
        assert_eq!(h.text_color, Some(tokens::FOREGROUND));
    }

    #[test]
    fn style_block_class_selector_matches_by_class_attr() {
        let bs = blocks(
            r#"<style>.callout { background: #ff0000; padding: 8px }</style>
               <div class="callout"><p>inside</p></div>
               <div><p>outside</p></div>"#,
        );
        // Find the styled div (one wraps "inside", the other "outside" stays flat).
        let styled_div = bs
            .iter()
            .find(|b| b.fill == Some(Color::srgb_u8(255, 0, 0)))
            .expect("styled callout div");
        assert_eq!(styled_div.padding, Sides::all(8.0));
        assert!(flatten_text(styled_div).contains("inside"));
        // The plain div passes through without a wrap.
        assert!(
            bs.iter()
                .any(|b| { b.text.as_deref() == Some("outside") && b.fill.is_none() })
        );
    }

    #[test]
    fn style_block_id_selector_matches_by_id_attr() {
        let bs = blocks(
            r#"<style>#hero { color: #00ff00 }</style>
               <p id="hero">hello</p>"#,
        );
        let p = &bs[0];
        assert_eq!(p.text_color, Some(Color::srgb_u8(0, 255, 0)));
    }

    #[test]
    fn inline_style_attr_beats_style_block_rule() {
        let bs = blocks(
            r#"<style>p { color: red }</style>
               <p style="color: blue">overridden</p>"#,
        );
        let p = &bs[0];
        // Inline always wins, even against a more-specific rule.
        assert_eq!(p.text_color, Some(Color::srgb_u8(0, 0, 255)));
    }

    #[test]
    fn higher_specificity_rule_wins_over_lower() {
        let bs = blocks(
            r#"<style>
                 p { color: red }
                 p.note { color: blue }
                 #hero { color: green }
               </style>
               <p>plain → red</p>
               <p class="note">class → blue</p>
               <p id="hero">id → green</p>
               <p class="note" id="hero">id beats class</p>"#,
        );
        let plain = &bs[0];
        let class_match = &bs[1];
        let id_match = &bs[2];
        let id_and_class = &bs[3];
        assert_eq!(plain.text_color, Some(Color::srgb_u8(255, 0, 0)));
        assert_eq!(class_match.text_color, Some(Color::srgb_u8(0, 0, 255)));
        assert_eq!(id_match.text_color, Some(Color::srgb_u8(0, 128, 0)));
        assert_eq!(id_and_class.text_color, Some(Color::srgb_u8(0, 128, 0)));
    }

    #[test]
    fn later_rule_wins_at_equal_specificity() {
        let bs = blocks(
            r#"<style>p { color: red } p { color: blue }</style>
               <p>later wins</p>"#,
        );
        let p = &bs[0];
        assert_eq!(p.text_color, Some(Color::srgb_u8(0, 0, 255)));
    }

    #[test]
    fn style_block_inside_head_still_applies() {
        // pulldown-cmark-style scraps may include a <head><style>...
        // wrapper. collect_stylesheets must descend through <head>
        // even though <head> is blocked from rendering.
        let bs = blocks(
            r#"<html>
                 <head><style>p { color: red }</style></head>
                 <body><p>red</p></body>
               </html>"#,
        );
        let p = &bs[0];
        assert_eq!(p.text_color, Some(Color::srgb_u8(255, 0, 0)));
    }

    #[test]
    fn sanitize_styles_option_drops_style_blocks() {
        let opts = HtmlOptions::default().sanitize_styles(true);
        let (root, findings) = html_with_lints("<style>p { color: red }</style><p>plain</p>", opts);
        let p = &root.children[0];
        // Style block was dropped; the paragraph keeps its role default.
        assert_eq!(p.text_color, Some(tokens::FOREGROUND));
        assert!(
            findings
                .iter()
                .any(|f| matches!(f.kind, FindingKind::SanitizedStyle))
        );
    }

    #[test]
    fn sanitize_styles_option_drops_inline_style_attributes() {
        // The untrusted-input knob: attacker-authored inline CSS
        // (invisible text, size games) must not be honoured.
        let opts = HtmlOptions::default().sanitize_styles(true);
        let (root, findings) = html_with_lints(
            "<p style=\"color: #112233; font-size: 1px\">styled</p>",
            opts,
        );
        let p = &root.children[0];
        assert_eq!(p.text_color, Some(tokens::FOREGROUND));
        assert!(
            findings
                .iter()
                .any(|f| matches!(f.kind, FindingKind::SanitizedStyle)
                    && f.detail.contains("color: #112233"))
        );
        // Default (trusted) mode still honours the attribute.
        let root = html("<p style=\"color: #112233\">styled</p>");
        assert_eq!(
            root.children[0].text_color,
            Some(Color::srgb_u8(0x11, 0x22, 0x33))
        );
    }

    #[test]
    fn comma_grouped_selectors_apply_to_each_listed_tag() {
        let bs = blocks(
            r#"<style>h1, h2, h3 { color: #ff0000 }</style>
               <h1>one</h1><h2>two</h2><h3>three</h3>"#,
        );
        for h in &bs {
            assert_eq!(h.text_color, Some(Color::srgb_u8(255, 0, 0)));
        }
    }

    #[test]
    fn class_rule_applies_to_inline_span_runs() {
        let bs = blocks(
            r#"<style>.hl { color: #ff8800 }</style>
               <p>before <span class="hl">marked</span> after</p>"#,
        );
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let hl = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("marked"))
            .expect("highlighted run");
        assert_eq!(hl.text_color, Some(Color::srgb_u8(255, 136, 0)));
    }

    // ---------- Tier-2D — layout reconciliation + lint surface ----------

    #[test]
    fn uniform_sibling_margins_become_outer_column_gap() {
        let (el, findings) = html_with_lints(
            "<p style=\"margin: 12px 0\">a</p>\
             <p style=\"margin: 12px 0\">b</p>\
             <p style=\"margin: 12px 0\">c</p>",
            HtmlOptions::default(),
        );
        // No asymmetry → no lint.
        assert!(
            !findings
                .iter()
                .any(|f| matches!(f.kind, FindingKind::MarginAsymmetryFlattened))
        );
        // Outer column gap should be the collapsed pair max (12).
        assert_eq!(el.gap, 12.0);
    }

    #[test]
    fn asymmetric_sibling_margins_lint_and_flatten_to_max() {
        let (el, findings) = html_with_lints(
            "<p style=\"margin-bottom: 20px\">a</p>\
             <p style=\"margin: 4px 0\">b</p>\
             <p style=\"margin: 4px 0\">c</p>",
            HtmlOptions::default(),
        );
        // Pair (a, b): max(20, 4) = 20. Pair (b, c): max(4, 4) = 4.
        // Pairs disagree → flatten to 20 and lint.
        assert!(
            findings
                .iter()
                .any(|f| matches!(f.kind, FindingKind::MarginAsymmetryFlattened))
        );
        assert_eq!(el.gap, 20.0);
    }

    #[test]
    fn first_child_margin_top_folds_into_outer_padding_top() {
        let (el, _findings) = html_with_lints(
            "<p style=\"margin-top: 32px\">a</p><p>b</p>",
            HtmlOptions::default(),
        );
        assert_eq!(el.padding.top, 32.0);
    }

    #[test]
    fn display_flex_with_row_direction_sets_axis_on_styled_div() {
        let bs = blocks(
            "<div style=\"display: flex; flex-direction: row; \
             align-items: center; justify-content: space-between\">\
                <p>left</p><p>right</p>\
             </div>",
        );
        assert_eq!(bs.len(), 1);
        let wrapper = &bs[0];
        assert_eq!(wrapper.axis, Axis::Row);
        assert_eq!(wrapper.align, Align::Center);
        assert_eq!(wrapper.justify, Justify::SpaceBetween);
    }

    #[test]
    fn overflow_hidden_sets_clip_on_styled_container() {
        let bs = blocks("<div style=\"overflow: hidden; padding: 8px\"><p>x</p></div>");
        assert!(bs[0].clip);
    }

    #[test]
    fn overflow_auto_wraps_container_in_scroll() {
        let bs = blocks("<div style=\"overflow: auto; padding: 8px\"><p>x</p></div>");
        assert_eq!(bs[0].kind, Kind::Scroll);
    }

    #[test]
    fn box_shadow_blur_lands_on_shadow_modifier() {
        let bs = blocks("<div style=\"padding: 4px; box-shadow: 0 2px 12px black\"><p>x</p></div>");
        assert!((bs[0].shadow - 12.0).abs() < 0.001);
    }

    #[test]
    fn font_family_monospace_flips_mono_on_inline_run() {
        let bs = blocks("<p>plain <span style=\"font-family: monospace\">mono</span> tail</p>");
        let p = &bs[0];
        assert_eq!(p.kind, Kind::Inlines);
        let mono = p
            .children
            .iter()
            .find(|r| r.text.as_deref() == Some("mono"))
            .expect("mono run");
        assert!(mono.font_mono, "expected font_mono on the styled span");
    }

    #[test]
    fn unsupported_unit_in_inline_style_emits_finding() {
        let (_el, findings) =
            html_with_lints("<p style=\"font-size: 4vw\">a</p>", HtmlOptions::default());
        assert!(findings.iter().any(|f| {
            matches!(f.kind, FindingKind::DroppedDeclaration) && f.detail.contains("4vw")
        }));
    }

    #[test]
    fn position_absolute_emits_finding_but_keeps_content() {
        let (el, findings) = html_with_lints(
            "<p style=\"position: absolute\">still rendered</p>",
            HtmlOptions::default(),
        );
        assert!(findings.iter().any(|f| {
            matches!(f.kind, FindingKind::DroppedDeclaration) && f.detail.contains("position")
        }));
        // Content still renders.
        assert_eq!(flatten_text(&el), "still rendered");
    }

    #[test]
    fn float_left_emits_finding() {
        let (_el, findings) = html_with_lints(
            "<div style=\"float: left\"><p>x</p></div>",
            HtmlOptions::default(),
        );
        assert!(findings.iter().any(|f| {
            matches!(f.kind, FindingKind::DroppedDeclaration) && f.detail.contains("float")
        }));
    }

    #[test]
    fn unsupported_video_tag_emits_finding_and_flattens_text() {
        let (el, findings) = html_with_lints(
            "<p>before</p><video><p>video body</p></video><p>after</p>",
            HtmlOptions::default(),
        );
        assert!(findings.iter().any(|f| {
            matches!(f.kind, FindingKind::UnsupportedTag) && f.detail.contains("video")
        }));
        // The inner <p> still renders so author text isn't lost.
        let flat = flatten_text(&el);
        assert!(flat.contains("video body"));
    }

    #[test]
    fn unsupported_style_selector_emits_finding_other_rules_still_apply() {
        let (el, findings) = html_with_lints(
            "<style>p > span { color: red } .note { color: blue }</style>\
             <p class=\"note\">styled</p>",
            HtmlOptions::default(),
        );
        assert!(findings.iter().any(|f| {
            matches!(f.kind, FindingKind::UnsupportedSelector) && f.detail.contains("p > span")
        }));
        // The .note rule still applied.
        assert_eq!(el.children[0].text_color, Some(Color::srgb_u8(0, 0, 255)));
    }
}
