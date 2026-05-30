# Damascene HTML Vision

This is the maintainer-facing architecture note for HTML rendering in Damascene.
Public author guidance belongs in crate READMEs and rustdoc once the surface is
stable enough to document as supported API.

## Goal

`damascene-html` is a focused HTML-to-`El` transformer, not a browser engine. Its
purpose is to make scraps of HTML inside markdown documents render alongside
the markdown subset Damascene already supports, and to give Damascene apps a way to
ingest authored HTML fragments without inventing a parallel templating
language.

The shape mirrors `damascene-markdown`: a free function (`html(input)`) and an
options-variant (`html_with_options`) that walk a parsed DOM and produce an
`El` tree built from the same widget kit a hand-author would use.

This is decidedly **not**:

- a CSS layout engine (no margin collapse, no floats, no positioning, no Grid),
- a full cascade implementation (no descendant / sibling / pseudo selectors),
- a scripting host (no `<script>`, no `on*` attributes),
- a media engine (no `<video>`, `<audio>`, `<iframe>`, `<canvas>`).

Damascene's thesis — vocabulary parity with what an LLM author writes, not with
what a browser implements — explicitly cuts against trying to reproduce the
web platform. The crate ships the subset of HTML that maps cleanly onto
Damascene's existing widget vocabulary, and surfaces lint findings for the rest
so authors and downstream tools know what was dropped.

## Architecture

```text
HTML source -> html5ever parse -> sanitizer -> DOM walker -> El tree
                                                    ^
                                                    +-- ComputedStyle stack (tier 2)
```

The DOM walker mirrors `damascene-markdown::Walker` — it carries an `InlineState`
flat style stack (italic / bold / strikethrough depth counters + the current
link href + the current inline color) and a context flag (`Block` vs
`Inline`) so that block tags appearing inside an inline context get coerced
to their inline-equivalent output rather than terminating the paragraph.

Two entry points:

- `html(input) -> El` — block-level walker. Returns a `column([...])` of
  block Els, exactly like `damascene_markdown::md`.
- `html_fragment_inline(input, opts) -> Vec<El>` — inline-only walker.
  Returns the run vector a caller can feed into their own `text_runs([...])`
  or paragraph. This is the entry point `damascene-markdown` uses when folding
  `Event::InlineHtml` into an open paragraph.

## Tag Matrix

The tag set is split into three tiers. Tier 1 is the v1 commit; tier 2 is the
follow-up slice that brings the CSS subset and generic containers online;
tier 3 is the permanent set of tags we drop for security or scope reasons.

### Tier 1 — direct widget mapping

Lossless mapping to existing Damascene primitives. No CSS needed.

| HTML | Damascene primitive |
|---|---|
| `<p>` | `paragraph(text)` (plain) or `text_runs([...])` (rich) |
| `<h1>`, `<h2>`, `<h3>` | `h1`, `h2`, `h3` |
| `<h4>`, `<h5>`, `<h6>` | clamp to `h3` (matches `damascene-markdown`) |
| `<br>` | `hard_break()` |
| `<hr>` | `divider()` |
| `<strong>`, `<b>` | `text(...).bold()` |
| `<em>`, `<i>` | `text(...).italic()` |
| `<u>` | `text(...).underline()` |
| `<s>`, `<strike>`, `<del>` | `text(...).strikethrough()` |
| `<code>` (inline) | `text(...).code()` |
| `<pre><code class="language-X">` | `build_code_block(Some("X"), body)` — shares the highlighter pipeline with markdown |
| `<pre>` (no code child) | `code_block(text)` plain mono |
| `<a href="...">` | inline runs with `.link(href)` |
| `<ul>` / `<li>` | `bullet_list([...])` |
| `<ol>` / `<li>` | `numbered_list_from(start, [...])`, honours `start=""` attr |
| `<ul>` with `<input type="checkbox">` leading children | `task_list([...])` |
| `<blockquote>` | `blockquote([...])` |
| `<img src="" alt="">` | image placeholder, same shape as markdown's Phase 2 placeholder |
| `<table>` / `<thead>` / `<tbody>` / `<tr>` / `<th>` / `<td>` | `table` / `table_header` / `table_body` / `table_row` / `table_head` / `table_cell` |
| `<kbd>` | `text(...).mono()` |
| `<mark>` | `text(...).background(tokens::WARNING.with_alpha(...))` (yellow tint) |
| `<sub>`, `<sup>` | flat text for v1 (no baseline-shift primitive yet) |

### Tier 2 — generic containers (next slice, needs CSS subset)

| HTML | Damascene primitive |
|---|---|
| `<div>` | `column([...])` |
| `<span>` | inline run (Inline ctx) or `row([...])` (Block ctx) |
| `<section>`, `<article>`, `<main>`, `<header>`, `<footer>`, `<nav>`, `<aside>` | `column([...])` |
| `<figure>` / `<figcaption>` | `column([img, text(caption).muted().italic()])` |
| `<details>` / `<summary>` | `accordion_item` (open if `<details open>`) |
| `<button>` | `button(text)` (cosmetic; no event wiring) |
| `<input type="checkbox">` | `checkbox` (cosmetic) |
| `<style>` | parsed into `Stylesheet`, not rendered |
| `<title>`, `<meta>`, `<link>`, `<head>` | stripped |

### Tier 3 — permanently dropped

`<script>`, `<iframe>`, `<object>`, `<embed>`, `<noscript>`, `<form>` (rendered
as group, no submission), `<video>`, `<audio>`, `<canvas>`. Plus every
attribute starting with `on*`.

## CSS Subset

Split into sub-slices ordered by user-visible value. Slice **2A** —
inline `style="..."` parsing — is shipped. Slices **2B–D** are
follow-ups.

### Tier-2A — inline `style="..."` (shipped)

Per-element `style="..."` is parsed into a [`ComputedStyle`][cs]
flat-property bag and applied after the El's stock constructor runs.
Block-level builders apply it directly; inline runs fold the
text-related fields (`color`, `background`, `font-size`,
`font-weight`, `font-style`, `text-decoration`) into the per-leaf
`InlineState`. Generic block containers (`<div>`, `<section>`, …)
without any styling stay flat (no extra nesting); the same containers
with at least one declared property wrap their children in a styled
`column`.

[cs]: ../crates/damascene-html/src/css.rs

| CSS | Damascene |
|---|---|
| `color` | `text_color` |
| `background`, `background-color` | `fill` (block) / inline text-bg (inline) |
| `padding`, `padding-{top,right,bottom,left}` | `padding` (shorthand parse) |
| `width`, `height` | `px` → `Size::Fixed`, `%` → `Size::Fill`, `auto` → `Size::Hug` |
| `min-width`, `max-width`, `min-height`, `max-height` | direct |
| `border` (shorthand) / `border-{width,color}` | `stroke`, `stroke_width` (only `solid` painted) |
| `border-radius` | `radius` |
| `opacity` | `opacity` |
| `text-align` | `text_align` |
| `font-size` | `font_size` (parses `px`, `pt`, `rem`, `em`) |
| `font-weight` | `font_weight` (numeric or named) |
| `font-style: italic` | `text_italic` (bumps `italic_depth` on inline runs) |
| `text-decoration: underline / line-through` | `text_underline` / `text_strikethrough` |

Colors parsed: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`,
`rgba()` (with both numeric and percentage channels), `transparent`,
plus a small named-color subset (red, green, blue, black, white,
gray, …).

### Tier-2B — `<style>` blocks + selectors (shipped)

`<style>` blocks anywhere in the document (including inside `<head>`,
which is otherwise stripped from rendering) are collected at entry,
their CSS tokenised, and rules folded into a flat `Vec<Rule>`. At
each tag dispatch, the cascade picks matching rules, sorts by
`(specificity, source_order)`, flattens into a `ComputedStyle`, then
layers the element's inline `style="..."` on top so inline always
wins.

Selector forms:

- Tag: `p`, `h1`, `*` (case-insensitive against the element).
- Class: `.foo` (case-sensitive against the `class` attribute, which
  splits on whitespace).
- ID: `#bar` (case-sensitive against `id`).
- Compound: any combination — `p.note#main`, `.a.b.c`, `h1.heading`.
- Comma-grouped: `p, h1, .quote { ... }`.

Specificity: `(id_count, class_count, tag_count)` tuple, compared
lexicographically. Ties broken by source order — later rule wins.

At-rules (`@media`, `@import`, `@font-face`, …) are skipped wholesale.
CSS comments (`/* ... */`) are stripped before parsing.

Anything containing a combinator (space, `>`, `+`, `~`), pseudo-class
colon, attribute selector, or namespace prefix is rejected at
selector-parse time and silently dropped. Authors who need those
shapes get the rule ignored rather than a partial match.

The `HtmlOptions::sanitize_styles` flag skips `<style>` block
collection entirely — turn it on for untrusted HTML where embedded
CSS could be a vector (CSS injection / data exfiltration via
`background: url(...)`).

### Tier-2C — generic-container semantics (shipped)

- `<details>` / `<summary>` → cosmetic disclosure. Static — body
  shown only when the `open` attribute is set; no toggle wiring.
  Apps that want interactivity fork `accordion_item` and own the
  state themselves.
- `<figure>` / `<figcaption>` → column with `<figcaption>` children's
  blocks muted + italicised, matching the markdown image-placeholder
  tone.
- `<button>` → cosmetic `button(label)`. Label flattens the
  element's text content; no `on_click` wiring.
- `<input type="checkbox">` → cosmetic `checkbox(checked)`. The
  `checked` attr drives the boolean. Other input types (`text`,
  `radio`, `number`, …) are silently dropped.

`<button>` and `<input>` are inline-classified, so they flow inline
inside paragraphs (`<p>click <button>here</button></p>`) and wrap in
an anonymous paragraph when standalone at block position. Standalone
`<input type="checkbox">` inside `<li>` still triggers the GFM task-
list shape detector — that path takes precedence.

### Tier-2D — layout reconciliation + lint (shipped)

- `margin*` → projected into the parent's `gap` via
  `walk_block_children`'s reconciliation pass. Sibling pairs collapse
  via `max(prev.margin_bottom, next.margin_top)`. Uniform pair values
  set the parent gap losslessly; mixed pair values flatten to the
  largest and emit a `MarginAsymmetryFlattened` finding. First /
  last child margins fold into the parent's `padding-top` /
  `padding-bottom` when the parent has none declared.
- `box-shadow` → best-effort blur radius into `shadow`. Multi-shadow
  lists collapse to the largest blur seen. Offset, spread, and color
  drop silently.
- `font-family` → monospace detection (`monospace`, `mono`, plus a
  fingerprint list of common mono faces — `JetBrains Mono`,
  `Consolas`, `Menlo`, …) flips the El to `.mono()`. Non-mono
  families lint as dropped because Damascene doesn't expose per-element
  family pinning beyond the mono toggle.
- `display: flex` + `flex-direction` → `Axis::Row` / `Column` on the
  styled container. `display: block` / `inline-block` parse without
  effect. `display: grid` / `table` / etc. lint as dropped.
- `align-items` → `Align` (`stretch`, `start`, `center`, `end`;
  `baseline` lints as dropped).
- `justify-content` → `Justify` (`start`, `center`, `end`,
  `space-between`; `space-around` / `space-evenly` lint as dropped).
- `overflow: hidden` / `clip` → `.clip()` on the container. `overflow:
  auto` / `scroll` → wrap the container in `scroll([...])`.
- `position: absolute / fixed / sticky`, `float: left/right`, and the
  unit set `vh`/`vw`/`vmin`/`vmax`/`fr`/`ch`/`ex`/`lh`/`rlh`/`cm`/
  `mm`/`in`/`pc` drop with `DroppedDeclaration` findings. `position:
  static` / `relative` parse without effect (no finding).
- Tags with no Damascene equivalent (`<video>`, `<audio>`, `<canvas>`,
  `<dialog>`, `<menu>`, `<marquee>`, `<applet>`, `<bgsound>`) emit
  an `UnsupportedTag` finding and flatten their children so authored
  text isn't lost.

Lint surface: `Finding` / `FindingKind` (defined in
[`crates/damascene-html/src/lints.rs`](../crates/damascene-html/src/lints.rs))
are public, returned by `html_with_lints`,
`html_blocks_with_lints`, and `html_fragment_inline_with_lints` in
[`crates/damascene-html/src/transform.rs`](../crates/damascene-html/src/transform.rs).
The non-lint entry points (`html`, `html_with_options`,
`html_blocks`, `html_fragment_inline`) collect findings internally
and discard, keeping the v1 signatures intact.

**Inheritance:** the inline `InlineState` stack inherits italic /
bold / strikethrough / link / color / background / font-size /
font-weight / font-mono through nesting. Full CSS inheritance with
computed-value resolution is not in scope.

## Layout Mismatches

Where CSS and Damascene's flex-shaped layout disagree, we lie deliberately:

- **Margin → gap.** Author-set `margin*` declarations on block-level
  siblings reconcile into the parent's `.gap()` via
  `max(prev.margin_bottom, next.margin_top)`. Uniform pair values are
  lossless; mixed pair values flatten to the largest and emit a
  `MarginAsymmetryFlattened` finding. First / last child margins fold
  into the parent's `padding-top` / `padding-bottom` when the parent
  has none declared. Damascene does not synthesise default margins for
  unstyled tags — bare HTML inherits the surrounding column's default
  rhythm.
- **`display: inline-block`** → no-op (parses; no Damascene distinction).
- **`display: block`** → no-op (the default).
- **`display: grid` / `display: table` / ...** → drop with a finding.
- **`position: absolute / fixed / sticky`** → drop with a finding.
  Positioned overlays exist only via `stack` / `overlay`.
- **`float`** → drop with a finding.
- **Percentage widths inside `Hug` parents** → fall back to `Hug`
  (same constraint Damascene's layout engine already has).
- **CSS units.** Support `px`, `pt`, `rem` (= 16px), `em` (= 16px;
  no parent-font-size lookup yet), `%`. Drop `vh`, `vw`, `vmin`,
  `vmax`, `fr`, `ch`, `ex`, `lh`, `rlh`, `cm`, `mm`, `in`, `pc`
  with a `DroppedDeclaration` finding.

The lint findings are the honest feedback channel — a doc with twenty
dropped properties tells the author the renderer can't do their layout
without claiming success.

## Sanitization Policy

Hardcoded in the parser, exposed as a `Sanitizer` trait so apps embedding
wild HTML can swap in a stricter policy (`ammonia`-shaped or otherwise):

- Strip `<script>`, `<iframe>`, `<object>`, `<embed>`, `<noscript>` and their
  contents.
- Strip every attribute starting with `on*`.
- For `href`, `src`, `action`, `formaction`: allow `http://`, `https://`,
  `mailto:`, `tel:`, relative URLs, `data:image/*`. Drop `javascript:`,
  `vbscript:`, `data:text/html`, anything else.
- Strip `<style>` if `HtmlOptions::sanitize_styles = true` (off by default
  for trusted scraps, on for untrusted content).
- No CSS expressions, no `@import`.

## Integration With damascene-markdown

`damascene-markdown` gains an opt-in `html` feature that pulls in `damascene-html`.
With the feature on, the `Event::Html(_)` / `Event::InlineHtml(_)` arms at
`crates/damascene-markdown/src/transformer.rs:350` change from "drop" to:

- `Event::Html(s)` (block) — accumulate consecutive events into a buffer,
  flush by feeding the buffer to `damascene_html::html` and appending the
  resulting block Els to the current frame.
- `Event::InlineHtml(s)` (inline) — accumulate inside an inline-HTML buffer
  on the open paragraph / heading / link / table cell; on the next non-
  inline-HTML event or on frame close, hand the buffer to
  `damascene_html::html_fragment_inline` and append the produced runs.

Default off, so existing `damascene-markdown` users see no behaviour change and
the dependency surface stays minimal.

## What This Is Not

- Not a browser. No DOM API, no event model, no scripting.
- Not a CSS engine. The v2 cascade is a flat property bag with one-level
  selector matching.
- Not a sanitizer library. The default policy is strict enough for scraps in
  trusted markdown; embedders handling untrusted HTML should layer
  `ammonia` (or equivalent) in front.
- Not a layout faithful to CSS. The layout mismatches above are honest
  trade-offs, not bugs.

## Crate Layering

```
damascene-html (publishable)
  -> damascene-core

damascene-markdown (publishable)
  -> damascene-core
  -> damascene-html (optional, behind `html` feature)
```

The crate stays a leaf relative to `damascene-core`, in the same tier as
`damascene-markdown`. No backend dependencies.
