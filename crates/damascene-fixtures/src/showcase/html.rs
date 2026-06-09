//! HTML — editable HTML / Markdown+HTML playground.
//!
//! Same shape as the Math page: a `text_area` editor on the left and a
//! live preview on the right (stacked on phone), driven through either
//! `damascene_html::html` (Html mode) or `damascene_markdown::md_with_options`
//! with the `html` feature on (Markdown+HTML mode). Presets at the top
//! seed the editor with representative scraps; the mode toggle changes
//! which renderer the preview pipes through without touching the
//! source text.

use damascene_core::prelude::*;
use damascene_html::{Finding, FindingKind, HtmlOptions, html_with_lints};
use damascene_markdown::{MarkdownOptions, md_with_options};

const SOURCE_KEY: &str = "html-source";
const MODE_KEY: &str = "html-mode";

/// Routing prefix for the preset row. The suffix is the preset's
/// stable `key`, matched against [`PRESETS`].
const PRESET_PREFIX: &str = "html-preset-";

const HTML_BASICS_SOURCE: &str = r##"<h1>HTML basics</h1>
<p>
  Damascene's <strong>HTML transformer</strong> walks parsed HTML and emits
  the same <em>El</em> tree a hand-author would type. Inline runs
  compose: <strong>bold</strong>, <em>italic</em>, <u>underline</u>,
  <s>strikethrough</s>, and <code>inline code</code> all nest the way
  CommonMark expects.
</p>
<p>
  Links route through the same <code>.link()</code> modifier the rest
  of the widget kit uses, so a click on
  <a href="https://damascene.dev">damascene.dev</a> goes through
  <code>UiEventKind::LinkActivated</code> like every other anchor.
</p>
"##;

const HTML_LISTS_QUOTES_SOURCE: &str = r##"<h2>Lists</h2>
<ul>
  <li>Plain unordered item</li>
  <li>
    <strong>Bold</strong> item with <a href="#nested">a link</a>
  </li>
  <li>
    Nested list:
    <ul>
      <li>second level</li>
      <li>still flat-mapped onto bullet_list</li>
    </ul>
  </li>
</ul>

<h2>Ordered, starting at 7</h2>
<ol start="7">
  <li>Seven</li>
  <li>Eight</li>
  <li>Nine</li>
</ol>

<h2>Task list (GFM checkbox shape)</h2>
<ul>
  <li><input type="checkbox" checked> shipped</li>
  <li><input type="checkbox"> still WIP</li>
  <li><input type="checkbox" checked> verified end-to-end</li>
</ul>

<h2>Blockquote</h2>
<blockquote>
  <p>
    Blockquote contents flow through <code>walk_block_children</code>
    just like the document body, so nested headings and lists work.
  </p>
</blockquote>
"##;

const HTML_TABLES_SOURCE: &str = r##"<h2>Explicit thead / tbody</h2>
<table>
  <thead>
    <tr>
      <th>Capability</th>
      <th>Tier</th>
      <th>Status</th>
    </tr>
  </thead>
  <tbody>
    <tr><td>Direct widget mapping</td><td>1</td><td>shipped</td></tr>
    <tr><td>Generic containers + CSS subset</td><td>2</td><td>planned</td></tr>
    <tr><td>Security-dropped tags</td><td>3</td><td>shipped</td></tr>
  </tbody>
</table>

<h2>Header-less table (first all-th row promotes)</h2>
<table>
  <tr><th>Name</th><th>Score</th><th>Status</th></tr>
  <tr><td>Alice</td><td>92</td><td><strong>pass</strong></td></tr>
  <tr><td>Bob</td><td>71</td><td><em>review</em></td></tr>
</table>
"##;

const HTML_INLINE_MIX_SOURCE: &str = r##"<h2>Inline coverage</h2>
<p>
  Press <kbd>Ctrl</kbd>+<kbd>K</kbd> to focus the command bar.
  <mark>Highlighted phrases</mark> sit under the glyphs without
  reflowing the line. A run can be
  <a href="https://damascene.dev"><strong>both bold and a link</strong></a>,
  and the painter groups them into one hit-test target.
</p>
<p>
  Footnote tags and unfamiliar HTML elements
  (<abbr title="Web Accessibility Initiative">WAI</abbr>,
  <cite>Designing Type</cite>, <q>quoted phrase</q>,
  <time datetime="2026-05-23">today</time>) flatten to plain text — no
  baseline shift for <sub>sub</sub> / <sup>sup</sup> yet but their
  contents survive.
</p>
"##;

const HTML_CODE_AND_IMAGE_SOURCE: &str = r##"<h2>Code block</h2>
<pre><code class="language-rust">fn render(input: &str) -&gt; El {
    let dom = parse_document_dom(input);
    let body = find_body(&dom).unwrap();
    column(walk_block_children(&body, &Default::default(), &Default::default()))
}</code></pre>

<h2>Image placeholder</h2>
<p>
  Inline images render as muted-italic placeholders carrying the alt
  text and the linked source:
  <img src="https://damascene.dev/badge.png" alt="Damascene badge" title="brand mark">
</p>

<h2>Horizontal rule</h2>
<hr>
<p>Above and below the rule are separate paragraphs.</p>
"##;

const HTML_SANITIZATION_SOURCE: &str = r##"<h2>Sanitization (what the parser drops silently)</h2>
<p>
  <strong>Before</strong>:
  <script>alert('xss')</script>
  <strong>After</strong>.
</p>

<p>
  An anchor with a <code>javascript:</code> href renders as text, the
  href is stripped:
  <a href="javascript:alert(1)" onclick="alert(2)">click me</a>.
</p>

<iframe src="https://evil.example.com">embedded content</iframe>
<noscript>noscript body</noscript>
<object>object body</object>

<p>
  Inert <em>html</em> survives untouched —
  <a href="https://damascene.dev">safe link</a>,
  <code>inline code</code>, <mark>marked phrase</mark>.
</p>
"##;

const CSS_STYLES_SOURCE: &str = r##"<h2>Inline CSS subset</h2>
<p>
  Inline <code>style="..."</code> declarations now flow through to the
  Damascene El: colors, padding, border, radius, opacity, sizing, text
  alignment, font size and weight.
</p>

<div style="background: #1e293b; padding: 16px 20px; border-radius: 12px; border: 1px solid #334155">
  <h3 style="color: #38bdf8; font-size: 18px; text-align: center">Styled card</h3>
  <p style="color: #cbd5e1; text-align: center">
    A <code>&lt;div&gt;</code> with background, padding, border,
    radius, and a centered child heading — all from one
    <code>style</code> attribute per element.
  </p>
</div>

<p>
  Inline runs pick up text colour and font weight from
  <code>&lt;span style&gt;</code>:
  <span style="color: #ef4444; font-weight: bold">danger</span>,
  <span style="color: #22c55e">success</span>,
  <span style="background: #fef08a; color: #713f12">marked-via-style</span>,
  <span style="font-size: 22px">larger text</span>.
</p>

<p style="text-align: right; opacity: 0.6">
  Right-aligned, half-opacity paragraph via <code>text-align</code> +
  <code>opacity</code>.
</p>

<div style="width: 100%; padding: 12px; background: rgba(99, 102, 241, 0.15); border-radius: 6px">
  <p>Width 100% with a translucent fill via <code>rgba()</code>.</p>
</div>
"##;

const HTML_INTERACTIVE_SOURCE: &str = r##"<h2>Disclosure (cosmetic)</h2>
<details open>
  <summary>Why details is rendered open here</summary>
  <p>
    The <code>open</code> attribute on <code>&lt;details&gt;</code>
    drives the visible body. Tier-2C renders the chevron + summary
    row inline; clicking is not wired (apps that want a real toggle
    fork the <code>accordion_item</code> widget and own the state).
  </p>
</details>

<details>
  <summary>This one starts collapsed</summary>
  <p>No content shown — body is hidden when <code>open</code> is absent.</p>
</details>

<h2>Figure with caption</h2>
<figure>
  <img src="https://damascene.dev/badge.png" alt="Damascene badge">
  <figcaption>
    Figcaption text renders muted + italic, the same treatment the
    markdown image-placeholder uses.
  </figcaption>
</figure>

<h2>Buttons (cosmetic — no on-click handler)</h2>
<p>
  Standalone block-level buttons:
</p>
<p>
  <button>Primary action</button>
  <button>Secondary</button>
</p>
<p>
  Buttons inline with text — click <button>here</button> to do
  nothing, since the HTML form has no event wiring.
</p>

<h2>Input checkboxes</h2>
<p>
  Outside of a list, <code>&lt;input type="checkbox"&gt;</code>
  renders as the static checkbox widget:
  <input type="checkbox" checked> consent given
  <input type="checkbox"> not yet
</p>

<p>
  Other input types (<code>text</code>, <code>radio</code>,
  <code>number</code>) are silently dropped in this slice — only
  checkbox has a cosmetic mapping today.
</p>
"##;

const CSS_CASCADE_SOURCE: &str = r##"<style>
  /* Tag selector — every <p> picks up the muted body color. */
  p { color: #94a3b8 }

  /* Class selector — beats the tag rule by specificity. */
  .lede { color: #f8fafc; font-size: 18px; font-weight: bold }

  /* Comma-grouped selectors — share one declaration block. */
  h2, h3 { color: #38bdf8 }

  /* Compound: tag + class. */
  p.note {
    background: #1e293b;
    padding: 12px 16px;
    border-radius: 8px;
    border: 1px solid #334155;
    color: #cbd5e1;
  }

  /* ID selector — beats class selectors by specificity. */
  #hero {
    color: #fbbf24;
    font-size: 22px;
    text-align: center;
  }

  /* Inline style on the element always beats <style> rules. */
  .force-red { color: #22c55e }
</style>

<h2>Tier-2B: &lt;style&gt; block + selectors</h2>

<p class="lede">
  This lead paragraph carries a class selector that beats the plain
  <code>p</code> rule by specificity.
</p>

<p>
  Plain paragraph — falls through to the <code>p</code> tag rule.
</p>

<p id="hero">Hero paragraph — id beats class beats tag.</p>

<p class="note">
  Class-styled callout: <code>p.note</code> uses a compound selector
  that combines tag + class, and applies background, padding, border,
  and radius all from one rule.
</p>

<h3>Inline overrides</h3>

<p>
  <span class="force-red">The class says green</span>, but
  <span class="force-red" style="color: #ef4444">inline says red</span>
  — inline always wins over the cascade.
</p>

<p>
  Comma-grouped selectors share declarations: this paragraph's
  parent <code>&lt;h3&gt;</code> heading above is themed via
  <code>h2, h3 { color: #38bdf8 }</code>.
</p>
"##;

const TIER_2D_SOURCE: &str = r##"<style>
  /* Tier-2D: properties that change layout shape or report lint findings. */
  .toolbar {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: #0f172a;
    border-radius: 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
  }
  .toolbar > * { color: #e2e8f0 }
  .doc p { margin: 12px 0 }
  .doc h2 { margin-top: 24px; margin-bottom: 8px }
  .mono-bit { font-family: 'JetBrains Mono', monospace }
  .clip-box {
    overflow: hidden;
    width: 200px;
    height: 60px;
    padding: 8px;
    border: 1px solid #475569;
    border-radius: 6px;
  }
  .scroll-box {
    overflow: auto;
    width: 200px;
    height: 60px;
    padding: 8px;
    border: 1px solid #475569;
    border-radius: 6px;
  }
  /* These deliberately lint as dropped — visit the findings panel below. */
  .lints-bait { position: absolute; float: left; font-size: 4vw }
  .also-bait  { font-family: 'Helvetica Neue', sans-serif }
</style>

<h2>Tier-2D: layout reshape</h2>

<div class="toolbar">
  <strong>Toolbar (display: flex + space-between)</strong>
  <span>shipped &amp; cosmetic</span>
</div>

<h2>Margin reconciliation</h2>

<div class="doc">
  <p>
    The <code>.doc p</code> rule sets <code>margin: 12px 0</code> on every
    paragraph, and <code>.doc h2</code> overrides the heading margins. The
    walker collapses adjacent sibling margins via
    <code>max(prev.bottom, next.top)</code> and folds the result into the
    parent's <code>gap</code> — uniform pairs are lossless, mixed pairs
    flatten to the largest value with a finding.
  </p>
  <p>Each paragraph sits the same distance from its neighbour.</p>
  <h2>A heading interrupts the rhythm</h2>
  <p>
    Because <code>h2.margin-top</code> is bigger than <code>p.margin-bottom</code>,
    the pair (paragraph, heading) reconciles asymmetrically — check the
    <strong>Lint findings</strong> panel below for the asymmetry note.
  </p>
</div>

<h2>Overflow → clip vs scroll</h2>

<div class="clip-box">
  <p>Clipped content. The <code>overflow: hidden</code> declaration sets
  <code>.clip()</code> on the styled container — text beyond the 60px
  height is invisible.</p>
</div>

<div class="scroll-box">
  <p>Scrollable content. <code>overflow: auto</code> wraps the styled
  container in a <code>scroll([...])</code> viewport — drag the
  scrollbar to read past the cutoff.</p>
</div>

<h2>Font family mono detection</h2>

<p>
  The <code>.mono-bit</code> class picks up <code>font-family: 'JetBrains
  Mono', monospace</code> — the parser walks the fallback list, sees a
  monospace family, and flips <code>font_mono</code> on the run:
  <span class="mono-bit">def render(input: str) -&gt; El: …</span>
</p>

<p>
  A <span class="also-bait">non-monospace family</span> can't pin a face
  in Damascene, so the declaration drops with a finding (see below).
</p>

<h2>Drop-with-lint properties</h2>

<p>
  The following paragraph carries
  <code>position: absolute; float: left; font-size: 4vw</code>. None map
  onto Damascene layout primitives, so each one emits a finding while the
  text still renders:
</p>

<p class="lints-bait">Still legible despite the dropped declarations.</p>
"##;

const MD_HTML_MIX_SOURCE: &str = r##"# Markdown + HTML scraps

Plain markdown still works: **bold**, *italic*, `code`, and
[links](https://damascene.dev). Lists too:

- one
- two
- three

But now you can drop in a block of HTML the markdown grammar can't
express:

<table>
  <thead><tr><th>Step</th><th>Outcome</th></tr></thead>
  <tbody>
    <tr><td>Parse</td><td>html5ever DOM</td></tr>
    <tr><td>Sanitize</td><td>script / iframe stripped</td></tr>
    <tr><td>Walk</td><td>Damascene widget tree</td></tr>
  </tbody>
</table>

Inline HTML works the same way — a <kbd>Ctrl</kbd>+<kbd>K</kbd> chord
or a <mark>highlighted phrase</mark> sits inside a markdown paragraph
without breaking the flow.

> Blockquotes still belong to markdown; the HTML scraps share the same
> outer block stream.

<details open>
  <summary>HTML details block</summary>
  <p>
    Tier-2C renders <code>&lt;details&gt;</code> as a cosmetic
    disclosure — the summary row shows a chevron, and the body
    appears only when the <code>open</code> attribute is set.
  </p>
</details>
"##;

#[derive(Clone, Copy)]
struct Preset {
    key: &'static str,
    label: &'static str,
    source: &'static str,
    /// Which preview pipeline this preset is authored for. Selecting a
    /// preset also flips the mode so the source renders through the
    /// right transformer without the user having to toggle by hand.
    mode: Mode,
}

const PRESETS: &[Preset] = &[
    Preset {
        key: "basics",
        label: "Basics",
        source: HTML_BASICS_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "lists",
        label: "Lists & quotes",
        source: HTML_LISTS_QUOTES_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "tables",
        label: "Tables",
        source: HTML_TABLES_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "inline",
        label: "Inline mix",
        source: HTML_INLINE_MIX_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "code",
        label: "Code & image",
        source: HTML_CODE_AND_IMAGE_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "sanitize",
        label: "Sanitization",
        source: HTML_SANITIZATION_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "css",
        label: "Inline CSS",
        source: CSS_STYLES_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "interactive",
        label: "Interactive bits",
        source: HTML_INTERACTIVE_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "cascade",
        label: "<style> + selectors",
        source: CSS_CASCADE_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "tier2d",
        label: "Layout + lints",
        source: TIER_2D_SOURCE,
        mode: Mode::Html,
    },
    Preset {
        key: "md-mix",
        label: "Markdown + HTML",
        source: MD_HTML_MIX_SOURCE,
        mode: Mode::MarkdownHtml,
    },
];

/// Which preview transformer the page's editor flows through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// `damascene_html::html(&source)` — standalone HTML rendering.
    #[default]
    Html,
    /// `damascene_markdown::md_with_options(&source, ...)` with the
    /// `html` feature on. Demonstrates the bridge that lets markdown
    /// documents embed raw HTML scraps and have them render through
    /// the same widget vocabulary as the surrounding markdown.
    MarkdownHtml,
}

impl Mode {
    fn slug(self) -> &'static str {
        match self {
            Mode::Html => "html",
            Mode::MarkdownHtml => "md-html",
        }
    }

    fn from_slug(slug: &str) -> Option<Mode> {
        match slug {
            "html" => Some(Mode::Html),
            "md-html" => Some(Mode::MarkdownHtml),
            _ => None,
        }
    }
}

pub struct State {
    pub source: String,
    pub selection: Selection,
    pub mode: Mode,
    pub scroll_caret_into_view: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            source: HTML_BASICS_SOURCE.into(),
            selection: Selection::default(),
            mode: Mode::Html,
            scroll_caret_into_view: false,
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    let editor_preview: El = if phone {
        column([editor_card(state), preview_card(state)])
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
    } else {
        row([editor_card(state), preview_card(state)])
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .width(Size::Fill(1.0))
    };
    scroll([column([
        h1("HTML"),
        paragraph(
            "Damascene's HTML transformer maps a tier-1 subset of HTML onto \
             the same widget vocabulary `damascene-markdown` produces. Edit \
             the source on the left to exercise headings, lists, tables, \
             inline runs, the sanitizer, and the markdown + HTML bridge.",
        )
        .muted(),
        mode_bar(state, phone),
        preset_bar(state),
        editor_preview,
    ])
    .gap(tokens::SPACE_4)
    .width(Size::Fill(1.0))
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if e.kind == UiEventKind::SelectionChanged
        && let Some(sel) = e.selection.as_ref()
    {
        state.selection = sel.clone();
        return;
    }

    // Preset row — clicking seeds the editor and switches mode so the
    // preview matches the preset's intent without an extra toggle.
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate)
        && let Some(route) = e.route()
        && let Some(suffix) = route.strip_prefix(PRESET_PREFIX)
        && let Some(preset) = PRESETS.iter().find(|p| p.key == suffix)
    {
        state.source = preset.source.into();
        state.mode = preset.mode;
        state.selection = Selection::default();
        return;
    }

    // Mode tabs.
    let mut slug = state.mode.slug().to_string();
    if tabs::apply_event(&mut slug, &e, MODE_KEY, |s| {
        Mode::from_slug(s).map(|m| m.slug().to_string())
    }) {
        if let Some(mode) = Mode::from_slug(&slug) {
            state.mode = mode;
        }
        return;
    }

    if e.target_key() == Some(SOURCE_KEY)
        && text_area::apply_event(&mut state.source, &mut state.selection, SOURCE_KEY, &e)
    {
        state.scroll_caret_into_view = true;
    }
}

pub fn drain_scroll_requests(state: &mut State) -> Vec<damascene_core::scroll::ScrollRequest> {
    if std::mem::take(&mut state.scroll_caret_into_view)
        && let Some(req) =
            text_area::caret_scroll_request_for(&state.source, &state.selection, SOURCE_KEY)
    {
        vec![req]
    } else {
        Vec::new()
    }
}

fn preset_bar(state: &State) -> El {
    let buttons = row(PRESETS.iter().map(|preset| {
        let active = state.source == preset.source;
        let button = button(preset.label)
            .xsmall()
            .key(format!("{PRESET_PREFIX}{}", preset.key));
        if active {
            button.primary()
        } else {
            button.secondary()
        }
    }))
    .gap(tokens::SPACE_2);
    // 11 presets don't fit on a 360px phone viewport, and they don't
    // fit on a desktop preview pane either once the lint, sanitize,
    // and tier-2D entries are added. Wrap the strip in horizontal
    // scroll regardless of form factor so the buttons stay reachable
    // and their focus rings sit inside the scroll's clip rect instead
    // of overflowing the surrounding page scrollbar gutter.
    let strip = scroll([buttons
        .width(Size::Hug)
        .padding(Sides::xy(0.0, tokens::RING_WIDTH))])
    .axis(Axis::Row)
    .height(Size::Hug)
    .width(Size::Fill(1.0));
    row([text("Presets").label().muted(), strip])
        .gap(tokens::SPACE_3)
        .align(Align::Center)
        .width(Size::Fill(1.0))
}

fn mode_bar(state: &State, phone: bool) -> El {
    // The second tab's full label ("Markdown + HTML") doesn't fit the
    // phone viewport once it's split evenly across the tabs row, so
    // shorten it to "MD + HTML" on phone. Desktop has the fixed 360px
    // tabs box and the long label fits.
    let md_label = if phone {
        "MD + HTML"
    } else {
        "Markdown + HTML"
    };
    let tabs = tabs_list(
        MODE_KEY,
        &state.mode.slug(),
        [
            (Mode::Html.slug(), "HTML"),
            (Mode::MarkdownHtml.slug(), md_label),
        ],
    );
    let tabs = if phone {
        tabs.width(Size::Fill(1.0))
    } else {
        tabs.width(Size::Fixed(360.0))
    };
    row([text("Renderer").label().muted(), tabs])
        .gap(tokens::SPACE_3)
        .align(Align::Center)
        .width(Size::Fill(1.0))
}

fn editor_card(state: &State) -> El {
    let title = match state.mode {
        Mode::Html => "HTML source",
        Mode::MarkdownHtml => "Markdown + HTML source",
    };
    let desc = match state.mode {
        Mode::Html => "Parsed by html5ever, walked into an El tree.",
        Mode::MarkdownHtml => "Parsed by pulldown-cmark; HTML events folded by damascene-html.",
    };
    card([
        card_header([card_title(title), card_description(desc)]),
        card_content([
            text_area(&state.source, &state.selection, SOURCE_KEY).height(Size::Fixed(330.0))
        ]),
    ])
    .width(Size::Fill(1.0))
}

fn preview_card(state: &State) -> El {
    let (body, findings): (El, Vec<Finding>) = match state.mode {
        Mode::Html => html_with_lints(&state.source, HtmlOptions::default()),
        Mode::MarkdownHtml => (
            md_with_options(&state.source, MarkdownOptions::default()),
            Vec::new(),
        ),
    };
    let mut content: Vec<El> = vec![
        scroll([body])
            .key("html-preview")
            .height(Size::Fixed(330.0)),
    ];
    if !findings.is_empty() {
        content.push(lint_panel(&findings));
    }
    card([
        card_header([
            card_title("Preview"),
            card_description(match state.mode {
                Mode::Html => "damascene_html::html_with_lints(&source, _)",
                Mode::MarkdownHtml => "damascene_markdown::md_with_options(&source, _)",
            }),
        ]),
        card_content(content),
    ])
    .width(Size::Fill(1.0))
}

/// Render the lint findings as a compact panel below the preview. One
/// row per finding, prefixed with the kind tag.
fn lint_panel(findings: &[Finding]) -> El {
    let rows: Vec<El> = findings
        .iter()
        .map(|f| {
            let kind = match f.kind {
                FindingKind::DroppedDeclaration => "decl",
                FindingKind::UnsupportedSelector => "selector",
                FindingKind::MarginAsymmetryFlattened => "margin",
                FindingKind::UnsupportedTag => "tag",
                FindingKind::SanitizedStyle => "sanitized",
            };
            row([
                text(kind).label().mono().text_color(tokens::WARNING),
                text(f.detail.clone()).wrap_text().width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Start)
            .width(Size::Fill(1.0))
        })
        .collect();
    column([
        text(format!("Lint findings ({})", findings.len()))
            .label()
            .text_color(tokens::MUTED_FOREGROUND),
        column(rows).gap(tokens::SPACE_1).width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides::all(tokens::SPACE_3))
    .width(Size::Fill(1.0))
}
