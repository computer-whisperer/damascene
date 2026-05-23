//! HTML — editable HTML / Markdown+HTML playground.
//!
//! Same shape as the Math page: a `text_area` editor on the left and a
//! live preview on the right (stacked on phone), driven through either
//! `aetna_html::html` (Html mode) or `aetna_markdown::md_with_options`
//! with the `html` feature on (Markdown+HTML mode). Presets at the top
//! seed the editor with representative scraps; the mode toggle changes
//! which renderer the preview pipes through without touching the
//! source text.

use aetna_core::prelude::*;
use aetna_html::html as html_render;
use aetna_markdown::{MarkdownOptions, md_with_options};

const SOURCE_KEY: &str = "html-source";
const MODE_KEY: &str = "html-mode";

/// Routing prefix for the preset row. The suffix is the preset's
/// stable `key`, matched against [`PRESETS`].
const PRESET_PREFIX: &str = "html-preset-";

const HTML_BASICS_SOURCE: &str = r##"<h1>HTML basics</h1>
<p>
  Aetna's <strong>HTML transformer</strong> walks parsed HTML and emits
  the same <em>El</em> tree a hand-author would type. Inline runs
  compose: <strong>bold</strong>, <em>italic</em>, <u>underline</u>,
  <s>strikethrough</s>, and <code>inline code</code> all nest the way
  CommonMark expects.
</p>
<p>
  Links route through the same <code>.link()</code> modifier the rest
  of the widget kit uses, so a click on
  <a href="https://aetna.dev">aetna.dev</a> goes through
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
  <a href="https://aetna.dev"><strong>both bold and a link</strong></a>,
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
  <img src="https://aetna.dev/badge.png" alt="Aetna badge" title="brand mark">
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
  <a href="https://aetna.dev">safe link</a>,
  <code>inline code</code>, <mark>marked phrase</mark>.
</p>
"##;

const MD_HTML_MIX_SOURCE: &str = r##"# Markdown + HTML scraps

Plain markdown still works: **bold**, *italic*, `code`, and
[links](https://aetna.dev). Lists too:

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
    <tr><td>Walk</td><td>Aetna widget tree</td></tr>
  </tbody>
</table>

Inline HTML works the same way — a <kbd>Ctrl</kbd>+<kbd>K</kbd> chord
or a <mark>highlighted phrase</mark> sits inside a markdown paragraph
without breaking the flow.

> Blockquotes still belong to markdown; the HTML scraps share the same
> outer block stream.

<details>
  <summary>HTML details block</summary>
  <p>
    Tier-1 flattens <code>&lt;details&gt;</code> to its children in
    document order. Tier-2 will produce a real accordion item.
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
        key: "md-mix",
        label: "Markdown + HTML",
        source: MD_HTML_MIX_SOURCE,
        mode: Mode::MarkdownHtml,
    },
];

/// Which preview transformer the page's editor flows through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// `aetna_html::html(&source)` — standalone HTML rendering.
    #[default]
    Html,
    /// `aetna_markdown::md_with_options(&source, ...)` with the
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
            "Aetna's HTML transformer maps a tier-1 subset of HTML onto \
             the same widget vocabulary `aetna-markdown` produces. Edit \
             the source on the left to exercise headings, lists, tables, \
             inline runs, the sanitizer, and the markdown + HTML bridge.",
        )
        .muted(),
        mode_bar(state, phone),
        preset_bar(state, phone),
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

pub fn drain_scroll_requests(state: &mut State) -> Vec<aetna_core::scroll::ScrollRequest> {
    if std::mem::take(&mut state.scroll_caret_into_view)
        && let Some(req) =
            text_area::caret_scroll_request_for(&state.source, &state.selection, SOURCE_KEY)
    {
        vec![req]
    } else {
        Vec::new()
    }
}

fn preset_bar(state: &State, phone: bool) -> El {
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
    let strip: El = if phone {
        scroll([buttons
            .width(Size::Hug)
            .padding(Sides::xy(0.0, tokens::RING_WIDTH))])
        .axis(Axis::Row)
        .height(Size::Hug)
        .width(Size::Fill(1.0))
    } else {
        buttons.width(Size::Fill(1.0))
    };
    row([text("Presets").label().muted(), strip])
        .gap(tokens::SPACE_3)
        .align(Align::Center)
        .width(Size::Fill(1.0))
}

fn mode_bar(state: &State, phone: bool) -> El {
    let tabs = tabs_list(
        MODE_KEY,
        &state.mode.slug(),
        [
            (Mode::Html.slug(), "HTML"),
            (Mode::MarkdownHtml.slug(), "Markdown + HTML"),
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
        Mode::MarkdownHtml => "Parsed by pulldown-cmark; HTML events folded by aetna-html.",
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
    let body: El = match state.mode {
        Mode::Html => html_render(&state.source),
        Mode::MarkdownHtml => md_with_options(&state.source, MarkdownOptions::default()),
    };
    card([
        card_header([
            card_title("Preview"),
            card_description(match state.mode {
                Mode::Html => "aetna_html::html(&source)",
                Mode::MarkdownHtml => "aetna_markdown::md_with_options(&source, _)",
            }),
        ]),
        card_content([scroll([body])
            .key("html-preview")
            .height(Size::Fixed(330.0))]),
    ])
    .width(Size::Fill(1.0))
}
