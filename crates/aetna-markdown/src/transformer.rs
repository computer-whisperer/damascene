//! Walk pulldown-cmark events into an Aetna `El` tree.

use std::ops::Range;

use aetna_core::prelude::*;
use aetna_core::selection::SelectionSource;
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options as CmarkOptions, Parser,
    Tag, TagEnd,
};

/// Optional markdown extensions that can change rendered output.
///
/// [`md`] uses `Default`, which keeps output conservative while still
/// enabling the GFM features Aetna renders directly today: tables,
/// strikethrough, and task lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Replace ASCII punctuation with typographic punctuation during
    /// parsing (`--`, `---`, `...`, straight quotes).
    pub smart_punctuation: bool,
    /// Render GFM alert blockquotes (`[!NOTE]`, `[!WARNING]`, ...)
    /// through Aetna's `alert` widget instead of a plain blockquote.
    pub gfm_alerts: bool,
    /// Parse `$...$` and `$$...$$` as native Aetna math instead of
    /// leaving pulldown-cmark's math extension disabled.
    pub math: bool,
}

impl MarkdownOptions {
    pub fn smart_punctuation(mut self, enabled: bool) -> Self {
        self.smart_punctuation = enabled;
        self
    }

    pub fn gfm_alerts(mut self, enabled: bool) -> Self {
        self.gfm_alerts = enabled;
        self
    }

    pub fn math(mut self, enabled: bool) -> Self {
        self.math = enabled;
        self
    }
}

/// Render a markdown document as an Aetna `El`.
///
/// The result is a `column([...])` of block-level Aetna widgets — the
/// same shape an author would have hand-written. See the crate-level
/// docs for the supported subset and the deferred features.
pub fn md(input: &str) -> El {
    md_with_options(input, MarkdownOptions::default())
}

/// Render a markdown document with explicit extension options.
pub fn md_with_options(input: &str, options: MarkdownOptions) -> El {
    // GFM tables, task lists, and `~~strike~~` all have direct widget-kit
    // / inline-modifier analogs. Footnotes and math stay off until the
    // markdown surface grows first-class support for references and TeX.
    let mut parser_options = CmarkOptions::ENABLE_TABLES
        | CmarkOptions::ENABLE_STRIKETHROUGH
        | CmarkOptions::ENABLE_TASKLISTS;
    if options.smart_punctuation {
        parser_options |= CmarkOptions::ENABLE_SMART_PUNCTUATION;
    }
    if options.gfm_alerts {
        parser_options |= CmarkOptions::ENABLE_GFM;
    }
    if options.math {
        parser_options |= CmarkOptions::ENABLE_MATH;
    }

    let parser = Parser::new_ext(input, parser_options);
    let mut walker = Walker::new(input, options);
    for (event, range) in parser.into_offset_iter() {
        walker.handle(event, range);
    }
    walker.finish()
}

/// Block-level frame on the parser's open-container stack. `Walker`
/// pops these on the matching `End` event and folds the collected
/// child content into a single Aetna widget.
enum Frame {
    /// Open `<p>` — accumulates inline runs.
    Paragraph(InlineBuffer),
    /// Open `<h1..h6>` — accumulates inline runs.
    Heading(HeadingLevel, InlineBuffer),
    /// Open `<blockquote>` — accumulates block children.
    BlockQuote {
        kind: Option<BlockQuoteKind>,
        blocks: Vec<El>,
    },
    /// Open `<ul>` / `<ol>` — collects items as nested block lists.
    List {
        /// `None` ↔ bullet list, `Some(start)` ↔ ordered list starting
        /// at `start` (CommonMark allows non-1 starts).
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    /// Open `<li>` — accumulates one item's block children plus an
    /// optional GFM task marker.
    Item {
        blocks: Vec<El>,
        task_checked: Option<bool>,
    },
    /// Open `<pre><code>` — accumulating verbatim text. The optional
    /// `lang` is the fenced info string (`` ```rust `` → `Some("rust")`,
    /// indented blocks and bare `` ``` `` fences → `None`); when
    /// highlighting is enabled and the lang resolves to a known syntax,
    /// the close handler tokenises the body, otherwise it emits the
    /// existing plain-mono `code_block(...)`.
    CodeBlock {
        lang: Option<String>,
        text: String,
        text_source: Option<Range<usize>>,
        indented: bool,
    },
    /// Open `<a>` — accumulates inline children that share its URL.
    /// The URL is applied to each text run on close (not via inline
    /// style flags) so a link spanning multiple text events groups
    /// correctly under one href in the painter.
    Link(String, InlineBuffer),
    /// Open `<img>` — accumulates alt text and keeps the destination
    /// for placeholder rendering. Image content loading is deferred
    /// (see crate docs).
    Image {
        alt: String,
        dest_url: String,
        title: String,
    },
    /// Open `<table>` — collects the header and body rows the matching
    /// `End(Table)` folds into a `widgets::table` block.
    Table {
        /// Per-column alignments (`:---`, `:---:`, `---:`), applied to
        /// header and body cell text.
        alignments: Vec<Alignment>,
        /// Header row, populated on `TagEnd::TableHead`. `None` if the
        /// document somehow ends a table without a header (CommonMark
        /// + GFM always emits one but this stays defensive).
        head: Option<Vec<El>>,
        /// Body rows, accumulated on each `TagEnd::TableRow`.
        body: Vec<Vec<El>>,
    },
    /// Open `<thead>` — accumulates the header rows.
    TableHead(Vec<El>),
    /// Open `<tr>` — accumulates the row's cells.
    TableRow(Vec<El>),
    /// Open `<th>` / `<td>`. `in_header` toggles the header-styled
    /// `table_head(...)` builder on close vs. the body-styled
    /// `table_cell(...)`.
    TableCell {
        runs: InlineBuffer,
        in_header: bool,
        alignment: Alignment,
    },
}

#[derive(Clone, Debug, Default)]
struct InlineBuffer {
    runs: Vec<El>,
    visible: String,
    spans: Vec<InlineSourceSpan>,
}

#[derive(Clone, Debug)]
struct InlineSourceSpan {
    visible: Range<usize>,
    source: Range<usize>,
    source_full: Range<usize>,
    atomic: bool,
}

impl InlineBuffer {
    fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    fn visible_len(&self) -> usize {
        self.visible.len()
    }

    fn push(
        &mut self,
        el: El,
        visible: &str,
        source: Range<usize>,
        source_full: Range<usize>,
        atomic: bool,
    ) {
        let start = self.visible.len();
        self.visible.push_str(visible);
        let end = self.visible.len();
        if start < end {
            self.spans.push(InlineSourceSpan {
                visible: start..end,
                source,
                source_full,
                atomic,
            });
        }
        self.runs.push(el);
    }

    fn append(&mut self, mut other: InlineBuffer) {
        let offset = self.visible.len();
        self.visible.push_str(&other.visible);
        for span in other.spans.drain(..) {
            self.spans.push(InlineSourceSpan {
                visible: (span.visible.start + offset)..(span.visible.end + offset),
                source: span.source,
                source_full: span.source_full,
                atomic: span.atomic,
            });
        }
        self.runs.append(&mut other.runs);
    }

    fn mark_full_source(&mut self, visible: Range<usize>, source_full: Range<usize>) {
        for span in &mut self.spans {
            if span.visible.start >= visible.start && span.visible.end <= visible.end {
                span.source_full = source_full.clone();
            }
        }
    }

    fn mark_all_full_source(&mut self, source_full: Range<usize>) {
        self.mark_full_source(0..self.visible_len(), source_full);
    }

    fn into_runs(self) -> Vec<El> {
        self.runs
    }

    fn selection_source(
        &self,
        input: &str,
        source_range: Option<Range<usize>>,
    ) -> Option<SelectionSource> {
        let source_range = source_range?;
        let source_text = input.get(source_range.clone())?.to_string();
        let mut source = SelectionSource::new(source_text, self.visible.clone());
        for span in &self.spans {
            let start = span.source.start.saturating_sub(source_range.start);
            let end = span.source.end.saturating_sub(source_range.start);
            let full_start = span.source_full.start.saturating_sub(source_range.start);
            let full_end = span.source_full.end.saturating_sub(source_range.start);
            source.push_span_with_full_source(
                span.visible.clone(),
                start..end,
                full_start..full_end,
                span.atomic,
            );
        }
        Some(source)
    }
}

struct ListItem {
    content: El,
    task_checked: Option<bool>,
}

/// Inline styling currently in effect for new text runs.
///
/// Markdown inline tags (`*em*`, `**strong**`, `~~strike~~`) can nest;
/// each pair pushes / pops a depth counter. The Code and Link cases
/// are scoped through their own frames in `Walker::stack` rather than
/// as flags here, since they carry data (the run's `code_role` shape
/// and the link URL respectively).
#[derive(Default)]
struct InlineState {
    italic_depth: u32,
    bold_depth: u32,
    strike_depth: u32,
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
        el
    }
}

struct InlineSourceMarker {
    visible_start: usize,
    source_start: usize,
}

struct Walker {
    input: String,
    options: MarkdownOptions,
    /// Open block-level frames + open `<a>` / `<img>` containers,
    /// innermost last. `<a>` and `<img>` are stack-tracked rather than
    /// stored as inline-state flags because they own the text events
    /// between Start/End and need to fold them into their own El on
    /// close.
    stack: Vec<Frame>,
    source_stack: Vec<Option<Range<usize>>>,
    /// Inline-style flags for upcoming text events.
    inline: InlineState,
    inline_source_stack: Vec<InlineSourceMarker>,
    /// Top-level blocks collected outside any open frame.
    root: Vec<El>,
}

impl Walker {
    fn new(input: &str, options: MarkdownOptions) -> Self {
        Self {
            input: input.to_string(),
            options,
            stack: Vec::new(),
            source_stack: Vec::new(),
            inline: InlineState::default(),
            inline_source_stack: Vec::new(),
            root: Vec::new(),
        }
    }

    fn handle(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => {
                self.extend_top_source(range.clone());
                self.start(tag, range);
            }
            Event::End(end) => self.end(end, range),
            Event::Text(text) => self.text(text.into_string(), range),
            Event::Code(text) => self.code_span(text.into_string(), range),
            Event::SoftBreak => self.text(" ".to_string(), range),
            Event::HardBreak => {
                self.ensure_inline_frame(range.clone());
                self.extend_top_source(range.clone());
                self.push_inline_mapped(hard_break(), "\n", range.clone(), range, false);
            }
            Event::Rule => {
                self.extend_top_source(range);
                self.push_block(divider());
            }
            Event::InlineMath(text) => self.inline_math(text.into_string(), range),
            Event::DisplayMath(text) => self.display_math(text.into_string(), range),
            #[cfg(feature = "html")]
            Event::Html(s) => self.block_html(s.into_string(), range),
            #[cfg(feature = "html")]
            Event::InlineHtml(s) => self.inline_html(s.into_string(), range),
            #[cfg(not(feature = "html"))]
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(checked) => {
                self.extend_top_source(range);
                self.task_list_marker(checked);
            }
        }
    }

    fn push_frame(&mut self, frame: Frame, range: Range<usize>) {
        self.stack.push(frame);
        self.source_stack.push(Some(range));
    }

    fn pop_frame(&mut self) -> Option<(Frame, Option<Range<usize>>)> {
        let frame = self.stack.pop()?;
        let range = self.source_stack.pop().flatten();
        Some((frame, range))
    }

    fn parent_item_source_range(&self) -> Option<Range<usize>> {
        let frame_index = self
            .stack
            .iter()
            .rposition(|frame| matches!(frame, Frame::Item { .. }))?;
        self.source_stack.get(frame_index).cloned().flatten()
    }

    fn parent_table_line_source_range(&self) -> Option<Range<usize>> {
        let frame_index = self
            .stack
            .iter()
            .rposition(|frame| matches!(frame, Frame::TableHead(_) | Frame::TableRow(_)))?;
        self.source_stack.get(frame_index).cloned().flatten()
    }

    fn extend_top_source(&mut self, range: Range<usize>) {
        if range.start >= range.end {
            return;
        }
        if let Some(slot) = self.source_stack.last_mut() {
            match slot {
                Some(existing) => {
                    existing.start = existing.start.min(range.start);
                    existing.end = existing.end.max(range.end);
                }
                None => *slot = Some(range),
            }
        }
    }

    fn open_inline_source(&mut self, range: Range<usize>) {
        self.ensure_inline_frame(range.clone());
        self.extend_top_source(range.clone());
        let visible_start = self.current_inline_visible_len().unwrap_or(0);
        self.inline_source_stack.push(InlineSourceMarker {
            visible_start,
            source_start: range.start,
        });
    }

    fn close_inline_source(&mut self, range: Range<usize>) {
        let Some(marker) = self.inline_source_stack.pop() else {
            return;
        };
        let Some(visible_end) = self.current_inline_visible_len() else {
            return;
        };
        if visible_end <= marker.visible_start {
            return;
        }
        if let Some(buffer) = self.current_inline_buffer_mut() {
            buffer.mark_full_source(
                marker.visible_start..visible_end,
                marker.source_start..range.end,
            );
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Paragraph => self.push_frame(Frame::Paragraph(InlineBuffer::default()), range),
            Tag::Heading { level, .. } => {
                self.push_frame(Frame::Heading(level, InlineBuffer::default()), range)
            }
            Tag::BlockQuote(kind) => self.push_frame(
                Frame::BlockQuote {
                    kind: kind.filter(|_| self.options.gfm_alerts),
                    blocks: Vec::new(),
                },
                range,
            ),
            Tag::List(start) => self.push_frame(
                Frame::List {
                    start,
                    items: Vec::new(),
                },
                range,
            ),
            Tag::Item => self.push_frame(
                Frame::Item {
                    blocks: Vec::new(),
                    task_checked: None,
                },
                range,
            ),
            Tag::CodeBlock(kind) => {
                let (lang, indented) = match kind {
                    CodeBlockKind::Fenced(info) => {
                        // The info string can carry attributes after a
                        // space (`rust ignore`); first token is the
                        // language tag, anything else we don't speak.
                        let token = info.split_whitespace().next().unwrap_or("");
                        if token.is_empty() {
                            (None, false)
                        } else {
                            (Some(token.to_string()), false)
                        }
                    }
                    CodeBlockKind::Indented => (None, true),
                };
                self.push_frame(
                    Frame::CodeBlock {
                        lang,
                        text: String::new(),
                        text_source: None,
                        indented,
                    },
                    range,
                );
            }
            Tag::Emphasis => {
                self.inline.italic_depth += 1;
                self.open_inline_source(range);
            }
            Tag::Strong => {
                self.inline.bold_depth += 1;
                self.open_inline_source(range);
            }
            Tag::Strikethrough => {
                self.inline.strike_depth += 1;
                self.open_inline_source(range);
            }
            Tag::Link { dest_url, .. } => {
                self.push_frame(
                    Frame::Link(dest_url.into_string(), InlineBuffer::default()),
                    range,
                );
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                // Alt text accumulates through inline events while the
                // image frame is open; on End we fold into a placeholder.
                self.push_frame(
                    Frame::Image {
                        alt: String::new(),
                        dest_url: dest_url.into_string(),
                        title: title.into_string(),
                    },
                    range,
                );
            }
            Tag::Table(alignments) => {
                self.push_frame(
                    Frame::Table {
                        alignments,
                        head: None,
                        body: Vec::new(),
                    },
                    range,
                );
            }
            Tag::TableHead => self.push_frame(Frame::TableHead(Vec::new()), range),
            Tag::TableRow => self.push_frame(Frame::TableRow(Vec::new()), range),
            Tag::TableCell => {
                // Header vs body is decided by what the current table
                // section is at cell-start time. pulldown-cmark emits
                // header cells directly under `TableHead` today, but
                // this also handles a nested `TableRow` shape.
                let in_header = self
                    .stack
                    .iter()
                    .rev()
                    .any(|frame| matches!(frame, Frame::TableHead(_)));
                let alignment = self.next_table_cell_alignment();
                self.push_frame(
                    Frame::TableCell {
                        runs: InlineBuffer::default(),
                        in_header,
                        alignment,
                    },
                    range,
                );
            }
            // Footnote definitions, definition lists, and subscript /
            // superscript are deferred — open a paragraph frame so any
            // inline text between Start and End ends up captured (and
            // dropped when we pop).
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::Subscript
            | Tag::Superscript => {
                self.push_frame(Frame::Paragraph(InlineBuffer::default()), range);
            }
        }
    }

    fn end(&mut self, end: TagEnd, range: Range<usize>) {
        match end {
            TagEnd::Paragraph => {
                let inside_blockquote = self.inside_blockquote();
                if let Some((Frame::Paragraph(inlines), source_range)) = self.pop_frame() {
                    // Empty paragraph: pulldown-cmark wraps inline
                    // images in their own paragraph, so once the image
                    // pops out as a block the wrapping paragraph is
                    // empty. Skip emission for that case (and for any
                    // other zero-run paragraph) so the document
                    // doesn't carry phantom empty blocks.
                    if inlines.is_empty() {
                        return;
                    }
                    let source_range = if inside_blockquote {
                        expand_source_range_start_to_line_start(&self.input, source_range)
                            .map(|range| trim_source_range_end(&self.input, range))
                    } else {
                        source_range
                    };
                    let block = build_paragraph(inlines, &self.input, source_range);
                    self.push_block(block);
                }
            }
            TagEnd::Heading(_) => {
                let inside_blockquote = self.inside_blockquote();
                if let Some((Frame::Heading(level, inlines), source_range)) = self.pop_frame() {
                    let source_range = if inside_blockquote {
                        expand_source_range_start_to_line_start(&self.input, source_range)
                            .map(|range| trim_source_range_end(&self.input, range))
                    } else {
                        source_range
                    };
                    let block = build_heading(level, inlines, &self.input, source_range);
                    self.push_block(block);
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some((Frame::BlockQuote { kind, blocks }, _)) = self.pop_frame() {
                    self.push_block(build_blockquote(kind, blocks));
                }
            }
            TagEnd::List(_) => {
                if let Some((Frame::List { start, items }, _)) = self.pop_frame() {
                    let block = build_list(start, items);
                    self.push_block(block);
                }
            }
            TagEnd::Item => {
                // Tight-list items in pulldown-cmark omit the wrapping
                // `Paragraph` events — text events arrive directly
                // under `Item`. We lazily push a `Paragraph` frame on
                // the first inline event under such an item (see
                // `ensure_inline_frame`). Drain any such open
                // paragraphs into the item's blocks before closing.
                while matches!(self.stack.last(), Some(Frame::Paragraph(_))) {
                    let item_source_range = self.parent_item_source_range();
                    if let Some((Frame::Paragraph(mut inlines), source_range)) = self.pop_frame()
                        && !inlines.is_empty()
                    {
                        if let Some(item_source_range) = item_source_range {
                            let item_source_range =
                                trim_source_range_end(&self.input, item_source_range);
                            let item_source_range = if self.inside_blockquote() {
                                expand_source_range_start_to_line_start(
                                    &self.input,
                                    Some(item_source_range.clone()),
                                )
                                .unwrap_or(item_source_range)
                            } else {
                                item_source_range
                            };
                            let source_range =
                                union_source_ranges(source_range, Some(item_source_range.clone()));
                            inlines.mark_all_full_source(item_source_range);
                            let block = build_paragraph(inlines, &self.input, source_range);
                            self.push_block(block);
                            continue;
                        }
                        let block = build_paragraph(inlines, &self.input, source_range);
                        self.push_block(block);
                    }
                }
                if let Some((
                    Frame::Item {
                        blocks,
                        task_checked,
                    },
                    _,
                )) = self.pop_frame()
                {
                    let item_el = build_list_item(blocks);
                    if let Some(Frame::List { items, .. }) = self.stack.last_mut() {
                        items.push(ListItem {
                            content: item_el,
                            task_checked,
                        });
                    }
                }
            }
            TagEnd::CodeBlock => {
                let inside_blockquote = self.inside_blockquote();
                if let Some((
                    Frame::CodeBlock {
                        lang,
                        text,
                        text_source,
                        indented,
                    },
                    source_range,
                )) = self.pop_frame()
                {
                    let source_range = if indented || inside_blockquote {
                        expand_source_range_start_to_line_start(&self.input, source_range)
                    } else {
                        source_range
                    };
                    self.push_block(build_code_block(
                        lang.as_deref(),
                        text,
                        &self.input,
                        source_range,
                        text_source,
                    ));
                }
            }
            TagEnd::Emphasis => {
                self.close_inline_source(range);
                self.inline.italic_depth = self.inline.italic_depth.saturating_sub(1)
            }
            TagEnd::Strong => {
                self.close_inline_source(range);
                self.inline.bold_depth = self.inline.bold_depth.saturating_sub(1);
            }
            TagEnd::Strikethrough => {
                self.close_inline_source(range);
                self.inline.strike_depth = self.inline.strike_depth.saturating_sub(1);
            }
            TagEnd::Link => {
                if let Some((Frame::Link(url, mut inlines), source_range)) = self.pop_frame() {
                    if let Some(source_range) = source_range {
                        inlines.mark_full_source(0..inlines.visible_len(), source_range);
                    }
                    for run in &mut inlines.runs {
                        // Each text leaf inside the `<a>` adopts the
                        // same href so the renderer groups them into
                        // one link for hit-testing.
                        *run = std::mem::take(run).link(url.clone());
                    }
                    self.push_inline_buffer(inlines);
                }
            }
            TagEnd::Image => {
                if let Some((
                    Frame::Image {
                        alt,
                        dest_url,
                        title,
                    },
                    source_range,
                )) = self.pop_frame()
                {
                    let placeholder = build_image_placeholder(&alt, &dest_url, &title);
                    let visible = image_placeholder_label(&alt, &dest_url, &title);
                    if self.in_inline_container() {
                        if let Some(source_range) = source_range {
                            self.push_inline_mapped(
                                placeholder,
                                &visible,
                                source_range.clone(),
                                source_range,
                                true,
                            );
                        } else {
                            self.push_inline_mapped(placeholder, &visible, 0..0, 0..0, false);
                        }
                    } else {
                        let block = with_atomic_source_selection(
                            placeholder,
                            "img",
                            &self.input,
                            source_range,
                            visible,
                        );
                        self.push_block(block);
                    }
                }
            }
            TagEnd::Table => {
                if let Some((Frame::Table { head, body, .. }, _)) = self.pop_frame() {
                    self.push_block(build_table(head, body));
                }
            }
            TagEnd::TableHead => {
                if let Some((Frame::TableHead(items), _)) = self.pop_frame() {
                    let rows = normalize_table_head_rows(items);
                    if let Some(Frame::Table { head, .. }) = self.stack.last_mut() {
                        *head = Some(rows);
                    }
                }
            }
            TagEnd::TableRow => {
                if let Some((Frame::TableRow(cells), _)) = self.pop_frame() {
                    let row = table_row(cells);
                    match self.stack.last_mut() {
                        Some(Frame::TableHead(rows)) => rows.push(row),
                        Some(Frame::Table { body, .. }) => body.push(vec![row]),
                        _ => {}
                    }
                }
            }
            TagEnd::TableCell => {
                let raw_row_source_range = self
                    .parent_table_line_source_range()
                    .map(|range| trim_source_range_end(&self.input, range));
                if let Some((
                    Frame::TableCell {
                        runs,
                        in_header,
                        alignment,
                    },
                    source_range,
                )) = self.pop_frame()
                {
                    let row_source_range = raw_row_source_range.map(|range| {
                        if in_header {
                            expand_table_header_source_to_delimiter(&self.input, range)
                        } else {
                            range
                        }
                    });
                    let cell = build_table_cell(
                        runs,
                        in_header,
                        alignment,
                        &self.input,
                        source_range,
                        row_source_range,
                    );
                    match self.stack.last_mut() {
                        Some(Frame::TableHead(cells)) | Some(Frame::TableRow(cells)) => {
                            cells.push(cell);
                        }
                        _ => {}
                    }
                }
            }
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::Subscript
            | TagEnd::Superscript => {
                // Drain the matching ignored frame from `start`.
                self.pop_frame();
            }
        }
    }

    fn text(&mut self, s: String, range: Range<usize>) {
        // CodeBlock receives raw text; everything else flows through
        // an inline buffer with the active style applied.
        if let Some(Frame::CodeBlock {
            text: buf,
            text_source,
            ..
        }) = self.stack.last_mut()
        {
            buf.push_str(&s);
            *text_source = union_source_ranges(text_source.take(), Some(range));
            return;
        }
        if let Some(Frame::Image { alt, .. }) = self.stack.last_mut() {
            alt.push_str(&s);
            return;
        }
        self.ensure_inline_frame(range.clone());
        self.extend_top_source(range.clone());
        let run = self.inline.apply(text(s.clone()));
        let source = self.source_range_for_visible(range.clone(), &s);
        self.push_inline_mapped(run, &s, source, range, false);
    }

    fn code_span(&mut self, s: String, range: Range<usize>) {
        // Inline code: `text(...).code()` carries the code role, which
        // theme application maps to mono + foreground. Strikethrough
        // / italic / bold can wrap a code span in CommonMark, so the
        // current InlineState still applies on top of `.code()`.
        if matches!(self.stack.last(), Some(Frame::CodeBlock { .. })) {
            // Inside a fenced code block, `Event::Code` shouldn't
            // arrive — but if it does, treat as raw text.
            if let Some(Frame::CodeBlock {
                text: buf,
                text_source,
                ..
            }) = self.stack.last_mut()
            {
                buf.push_str(&s);
                *text_source = union_source_ranges(text_source.take(), Some(range));
            }
            return;
        }
        if let Some(Frame::Image { alt, .. }) = self.stack.last_mut() {
            alt.push_str(&s);
            return;
        }
        self.ensure_inline_frame(range.clone());
        self.extend_top_source(range.clone());
        let run = self.inline.apply(text(s.clone()).code());
        let source = self.source_range_for_visible(range.clone(), &s);
        self.push_inline_mapped(run, &s, source, range, false);
    }

    fn inline_math(&mut self, source: String, range: Range<usize>) {
        let expr = parse_tex_or_error(&source);
        self.ensure_inline_frame(range.clone());
        self.extend_top_source(range.clone());
        self.push_inline_mapped(math_inline(expr), "\u{fffc}", range.clone(), range, true);
    }

    /// Handle a block-level `Event::Html`. Parses the HTML through
    /// [`aetna_html::html_blocks`] and pushes each produced block via
    /// the existing block-push path so blockquote / list / root
    /// nesting all work without special-casing.
    ///
    /// CommonMark treats block-level HTML as opaque, so each event
    /// arrives as a complete block (or a nearly-complete block; we
    /// hand whatever pulldown-cmark produced to html5ever, which is
    /// permissive about unclosed tags). No buffering is required for
    /// this v1 cut.
    #[cfg(feature = "html")]
    fn block_html(&mut self, s: String, range: Range<usize>) {
        let _ = range;
        if matches!(self.stack.last(), Some(Frame::CodeBlock { .. })) {
            // Defensive: pulldown-cmark shouldn't emit Html inside a
            // code block, but if it does, treat as raw text.
            if let Some(Frame::CodeBlock {
                text: buf,
                text_source,
                ..
            }) = self.stack.last_mut()
            {
                buf.push_str(&s);
                *text_source = union_source_ranges(text_source.take(), None);
            }
            return;
        }
        let blocks = aetna_html::html_blocks(&s, aetna_html::HtmlOptions::default());
        for block in blocks {
            self.push_block(block);
        }
    }

    /// Handle an inline `Event::InlineHtml`. Parses the HTML fragment
    /// through [`aetna_html::html_fragment_inline`], applies the
    /// current markdown [`InlineState`] (`*em*` / `**strong**` etc.
    /// nested around the HTML scrap stay honoured), and pushes each
    /// produced run into the open inline-accepting frame.
    ///
    /// V1 limitation: pulldown-cmark fragments long-running inline
    /// HTML (`<span class="x">`, text, `</span>`) across separate
    /// events; this handler parses each event independently, so the
    /// wrapping span's styling is lost — html5ever auto-closes the
    /// dangling tag and the text run lands in the paragraph with
    /// only the surrounding markdown state applied. The CSS pass
    /// (tier 2) will buffer the open/close pair so attributes carry
    /// through.
    #[cfg(feature = "html")]
    fn inline_html(&mut self, s: String, range: Range<usize>) {
        if matches!(self.stack.last(), Some(Frame::CodeBlock { .. })) {
            if let Some(Frame::CodeBlock {
                text: buf,
                text_source,
                ..
            }) = self.stack.last_mut()
            {
                buf.push_str(&s);
                *text_source = union_source_ranges(text_source.take(), Some(range));
            }
            return;
        }
        if let Some(Frame::Image { alt, .. }) = self.stack.last_mut() {
            alt.push_str(&s);
            return;
        }
        self.ensure_inline_frame(range.clone());
        self.extend_top_source(range.clone());
        let runs = aetna_html::html_fragment_inline(&s, aetna_html::HtmlOptions::default());
        for run in runs {
            let styled = self.inline.apply(run);
            // Source mapping uses the whole InlineHtml event range —
            // no per-character mapping into the HTML is meaningful.
            let visible = styled.text.clone().unwrap_or_default();
            self.push_inline_mapped(styled, &visible, range.clone(), range.clone(), false);
        }
    }

    fn display_math(&mut self, source: String, range: Range<usize>) {
        let expr = parse_tex_or_error(&source);
        let source_text = self.input.get(range.clone()).unwrap_or(&source).to_string();
        let visible = "\u{fffc}".to_string();
        let mut selection_source = SelectionSource::new(source_text.clone(), visible);
        selection_source.push_span(0.."\u{fffc}".len(), 0..source_text.len(), true);
        self.push_block(
            math_block(expr)
                .key(markdown_key("math", &range))
                .selectable()
                .selection_source(selection_source),
        );
    }

    /// Lazily open a `Paragraph` frame so an inline event arriving
    /// directly under an `Item` (CommonMark's tight-list shape — no
    /// wrapping `<p>`) has somewhere to land. The matching
    /// `TagEnd::Item` drains any such open paragraph back into the
    /// item before closing it. Table cells already accept inlines
    /// directly so they don't need the lazy paragraph.
    fn ensure_inline_frame(&mut self, source_range: Range<usize>) {
        match self.stack.last() {
            Some(
                Frame::Paragraph(_)
                | Frame::Heading(_, _)
                | Frame::Link(_, _)
                | Frame::TableCell { .. },
            ) => {}
            Some(Frame::Item { .. }) => {
                self.push_frame(Frame::Paragraph(InlineBuffer::default()), source_range)
            }
            _ => {}
        }
    }

    /// Append a block-level El to the innermost block container, or
    /// the root if none is open.
    fn push_block(&mut self, el: El) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::BlockQuote { blocks, .. } | Frame::Item { blocks, .. } => {
                    blocks.push(el);
                    return;
                }
                _ => {}
            }
        }
        self.root.push(el);
    }

    /// Append an inline-level El to the innermost inline-accepting
    /// container (paragraph, heading, link, table cell). Drops if
    /// none is open — stray text outside a paragraph should not be
    /// reachable from a well-formed pulldown-cmark stream.
    fn current_inline_visible_len(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|frame| match frame {
            Frame::Paragraph(runs)
            | Frame::Heading(_, runs)
            | Frame::Link(_, runs)
            | Frame::TableCell { runs, .. } => Some(runs.visible_len()),
            _ => None,
        })
    }

    fn current_inline_buffer_mut(&mut self) -> Option<&mut InlineBuffer> {
        self.stack.iter_mut().rev().find_map(|frame| match frame {
            Frame::Paragraph(runs)
            | Frame::Heading(_, runs)
            | Frame::Link(_, runs)
            | Frame::TableCell { runs, .. } => Some(runs),
            _ => None,
        })
    }

    fn push_inline_mapped(
        &mut self,
        el: El,
        visible: &str,
        source: Range<usize>,
        source_full: Range<usize>,
        atomic: bool,
    ) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Paragraph(runs)
                | Frame::Heading(_, runs)
                | Frame::Link(_, runs)
                | Frame::TableCell { runs, .. } => {
                    runs.push(el, visible, source, source_full, atomic);
                    return;
                }
                _ => {}
            }
        }
    }

    fn push_inline_buffer(&mut self, buffer: InlineBuffer) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Paragraph(runs)
                | Frame::Heading(_, runs)
                | Frame::Link(_, runs)
                | Frame::TableCell { runs, .. } => {
                    runs.append(buffer);
                    return;
                }
                _ => {}
            }
        }
    }

    fn source_range_for_visible(&self, range: Range<usize>, visible: &str) -> Range<usize> {
        let Some(fragment) = self.input.get(range.clone()) else {
            return range;
        };
        let Some(start) = fragment.find(visible) else {
            return range;
        };
        (range.start + start)..(range.start + start + visible.len())
    }

    fn finish(mut self) -> El {
        // Defensive: a malformed input could leave open frames. Drain
        // anything still on the stack into root order so we still
        // produce a valid El rather than panicking.
        while let Some((frame, source_range)) = self.pop_frame() {
            match frame {
                Frame::Paragraph(runs) => {
                    self.root
                        .push(build_paragraph(runs, &self.input, source_range))
                }
                Frame::Heading(level, runs) => {
                    self.root
                        .push(build_heading(level, runs, &self.input, source_range))
                }
                Frame::BlockQuote { kind, blocks } => {
                    self.root.push(build_blockquote(kind, blocks))
                }
                Frame::List { start, items } => self.root.push(build_list(start, items)),
                Frame::Item { blocks, .. } => self.root.push(build_list_item(blocks)),
                Frame::CodeBlock {
                    lang,
                    text,
                    text_source,
                    indented,
                } => {
                    let source_range = if indented {
                        expand_source_range_start_to_line_start(&self.input, source_range)
                    } else {
                        source_range
                    };
                    self.root.push(build_code_block(
                        lang.as_deref(),
                        text,
                        &self.input,
                        source_range,
                        text_source,
                    ))
                }
                Frame::Link(_, runs) => {
                    for run in runs.into_runs() {
                        self.root.push(run);
                    }
                }
                Frame::Image {
                    alt,
                    dest_url,
                    title,
                } => self
                    .root
                    .push(build_image_placeholder(&alt, &dest_url, &title)),
                Frame::Table { head, body, .. } => self.root.push(build_table(head, body)),
                Frame::TableHead(_) | Frame::TableRow(_) | Frame::TableCell { .. } => {
                    // Cells / rows whose enclosing table never closed
                    // can't usefully be rendered standalone — drop.
                }
            }
        }
        column(self.root)
            .gap(tokens::SPACE_4)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    }

    fn task_list_marker(&mut self, checked: bool) {
        for frame in self.stack.iter_mut().rev() {
            if let Frame::Item { task_checked, .. } = frame {
                *task_checked = Some(checked);
                return;
            }
        }
    }

    fn in_inline_container(&self) -> bool {
        matches!(
            self.stack.last(),
            Some(
                Frame::Paragraph(_)
                    | Frame::Heading(_, _)
                    | Frame::Link(_, _)
                    | Frame::TableCell { .. }
            )
        )
    }

    fn inside_blockquote(&self) -> bool {
        self.stack
            .iter()
            .rev()
            .skip(1)
            .any(|frame| matches!(frame, Frame::BlockQuote { .. }))
    }

    fn next_table_cell_alignment(&self) -> Alignment {
        let index = match self.stack.last() {
            Some(Frame::TableHead(cells)) | Some(Frame::TableRow(cells)) => cells.len(),
            _ => 0,
        };
        self.stack
            .iter()
            .rev()
            .find_map(|frame| {
                if let Frame::Table { alignments, .. } = frame {
                    alignments.get(index).copied()
                } else {
                    None
                }
            })
            .unwrap_or(Alignment::None)
    }
}

/// Build a fenced/indented code block. With the `highlighting` feature
/// and a recognised `lang`, tokenise the body through `syntect` and
/// wrap the styled `text_runs([...])` paragraph in the standard
/// [`code_block_chrome`] surface — palette tokens flow through to
/// paint, so `Theme::aetna_light()` recolours the result without
/// re-rendering the markdown. Otherwise (feature off, no lang,
/// unknown lang) fall through to the plain-mono [`code_block`] path.
fn build_code_block(
    lang: Option<&str>,
    raw_text: String,
    input: &str,
    source_range: Option<Range<usize>>,
    text_source: Option<Range<usize>>,
) -> El {
    let body = strip_trailing_newline(raw_text);
    let body_selection =
        code_block_selection_source(input, source_range.clone(), text_source, &body);
    let body_key_range = source_range.clone();
    #[cfg(feature = "highlighting")]
    if let Some(lang) = lang
        && let Some(syntax) = crate::highlight::find_syntax(lang)
    {
        let runs = crate::highlight::highlight_to_runs(&body, syntax);
        if !runs.is_empty() {
            let mut body = text_runs(runs)
                .mono()
                .nowrap_text()
                .font_size(tokens::TEXT_SM.size)
                .width(Size::Hug)
                .height(Size::Hug);
            if let Some(source) = body_selection.clone() {
                body = body
                    .key(markdown_key(
                        "code",
                        &body_key_range.clone().unwrap_or(0..source.source.len()),
                    ))
                    .selectable()
                    .selection_source(source);
            }
            return code_block_chrome(body);
        }
    }
    #[cfg(not(feature = "highlighting"))]
    let _ = lang;
    let mut body_el = text(body.clone())
        .mono()
        .font_size(tokens::TEXT_SM.size)
        .nowrap_text()
        .width(Size::Hug)
        .height(Size::Hug);
    if let Some(source) = body_selection {
        body_el = body_el
            .key(markdown_key(
                "code",
                &body_key_range.unwrap_or(0..source.source.len()),
            ))
            .selectable()
            .selection_source(source);
    }
    code_block_chrome(body_el)
}

fn code_block_selection_source(
    input: &str,
    source_range: Option<Range<usize>>,
    text_source: Option<Range<usize>>,
    visible: &str,
) -> Option<SelectionSource> {
    let source_range = source_range?;
    let source_text = input.get(source_range.clone())?.to_string();
    let mut source = SelectionSource::new(source_text.clone(), visible.to_string());
    if visible.is_empty() {
        return Some(source);
    }

    let search_range = text_source
        .map(|range| trim_source_range_end(input, range))
        .and_then(|range| {
            (range.start >= source_range.start && range.end <= source_range.end)
                .then_some((range.start - source_range.start)..(range.end - source_range.start))
        })
        .unwrap_or(0..source_text.len());

    let mut visible_start = 0;
    let mut source_cursor = search_range.start;
    for segment in visible.split_inclusive('\n') {
        if segment.is_empty() {
            continue;
        }
        let search = &source.source[source_cursor..search_range.end];
        let found = search.find(segment)?;
        let source_start = source_cursor + found;
        let source_end = source_start + segment.len();
        let visible_end = visible_start + segment.len();
        source.push_span_with_full_source(
            visible_start..visible_end,
            source_start..source_end,
            source_start..source_end,
            false,
        );
        visible_start = visible_end;
        source_cursor = source_end;
    }
    Some(source)
}

fn union_source_ranges(a: Option<Range<usize>>, b: Option<Range<usize>>) -> Option<Range<usize>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.start.min(b.start)..a.end.max(b.end)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn trim_source_range_end(input: &str, mut range: Range<usize>) -> Range<usize> {
    while range.end > range.start {
        let Some((idx, ch)) = input[..range.end].char_indices().next_back() else {
            break;
        };
        if matches!(ch, '\n' | '\r') {
            range.end = idx;
        } else {
            break;
        }
    }
    range
}

fn expand_table_header_source_to_delimiter(input: &str, mut range: Range<usize>) -> Range<usize> {
    let mut cursor = range.end;
    if input.as_bytes().get(cursor) == Some(&b'\n') {
        cursor += 1;
    }

    let delimiter_start = cursor;
    let delimiter_end = input[delimiter_start..]
        .find('\n')
        .map(|end| delimiter_start + end)
        .unwrap_or(input.len());
    let delimiter = input[delimiter_start..delimiter_end].trim();
    if is_table_delimiter_row(delimiter) {
        range.end = delimiter_end;
    }
    range
}

fn is_table_delimiter_row(line: &str) -> bool {
    let mut saw_dash = false;
    for ch in line.chars() {
        match ch {
            '|' | ':' | '-' | ' ' | '\t' => {
                saw_dash |= ch == '-';
            }
            _ => return false,
        }
    }
    saw_dash
}

fn expand_source_range_start_to_line_start(
    input: &str,
    range: Option<Range<usize>>,
) -> Option<Range<usize>> {
    let mut range = range?;
    while range.start > 0 && input.as_bytes().get(range.start - 1) != Some(&b'\n') {
        range.start -= 1;
    }
    Some(range)
}

fn parse_tex_or_error(source: &str) -> MathExpr {
    match parse_tex(source) {
        Ok(expr) => expr,
        Err(err) => MathExpr::Error(format!("math parse error at {}: {}", err.byte, err.message)),
    }
}

/// Build a paragraph block. A single plain `text(...)` run can become
/// a `paragraph(...)` (one wrapped string); anything richer collapses
/// to `text_runs([...])`.
fn build_paragraph(inlines: InlineBuffer, input: &str, source_range: Option<Range<usize>>) -> El {
    let key_range = source_range.clone();
    let selection_source = inlines.selection_source(input, source_range);
    let runs = inlines.into_runs();
    let mut el = if let Some(plain) = single_plain_text(&runs) {
        paragraph(plain)
    } else {
        text_runs(runs)
            .wrap_text()
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    };
    if let Some(source) = selection_source {
        el = el
            .key(markdown_key(
                "p",
                &key_range.unwrap_or(0..source.source.len()),
            ))
            .selectable()
            .selection_source(source);
    }
    el
}

fn with_source_selection(
    mut el: El,
    kind: &str,
    source: Option<SelectionSource>,
    key_range: Option<Range<usize>>,
) -> El {
    if let Some(source) = source {
        el = el
            .key(markdown_key(
                kind,
                &key_range.unwrap_or(0..source.source.len()),
            ))
            .selectable()
            .selection_source(source);
    }
    el
}

fn with_atomic_source_selection(
    mut el: El,
    kind: &str,
    input: &str,
    source_range: Option<Range<usize>>,
    visible: String,
) -> El {
    let Some(source_range) = source_range else {
        return el;
    };
    let Some(source_text) = input.get(source_range.clone()) else {
        return el;
    };
    let mut source = SelectionSource::new(source_text.to_string(), visible.clone());
    source.push_span(0..visible.len(), 0..source_text.len(), true);
    el = el
        .key(markdown_key(kind, &source_range))
        .selectable()
        .selection_source(source);
    el
}

fn markdown_key(kind: &str, range: &Range<usize>) -> String {
    format!("md:{kind}:{}..{}", range.start, range.end)
}

/// Build a heading block. For headings whose only content is plain
/// text, use `h1` / `h2` / `h3` so the result carries the semantic
/// `Kind::Heading` (visible in tree dumps and inspect output). For
/// styled headings we fall back to a heading-roled `text_runs`.
fn build_heading(
    level: HeadingLevel,
    inlines: InlineBuffer,
    input: &str,
    source_range: Option<Range<usize>>,
) -> El {
    let key_range = source_range.clone();
    let selection_source = inlines.selection_source(input, source_range);
    let runs = inlines.into_runs();
    if let Some(plain) = single_plain_text(&runs) {
        let el = match level {
            HeadingLevel::H1 => h1(plain),
            HeadingLevel::H2 => h2(plain),
            // h4–h6 are rare and Aetna's heading vocabulary stops at
            // h3 — clamp the rest so deep nesting still renders.
            _ => h3(plain),
        };
        return with_source_selection(el, "h", selection_source, key_range);
    }
    let role = match level {
        HeadingLevel::H1 => TextRole::Display,
        HeadingLevel::H2 => TextRole::Heading,
        _ => TextRole::Title,
    };
    let el = text_runs(runs)
        .text_role(role)
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug);
    with_source_selection(el, "h", selection_source, key_range)
}

fn build_blockquote(kind: Option<BlockQuoteKind>, blocks: Vec<El>) -> El {
    let Some(kind) = kind else {
        return blockquote(blocks);
    };

    let title = match kind {
        BlockQuoteKind::Note => "Note",
        BlockQuoteKind::Tip => "Tip",
        BlockQuoteKind::Important => "Important",
        BlockQuoteKind::Warning => "Warning",
        BlockQuoteKind::Caution => "Caution",
    };
    let body = match blocks.len() {
        0 => column(Vec::<El>::new()),
        1 => blocks.into_iter().next().unwrap(),
        _ => column(blocks)
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
    };
    let alert = alert([alert_title(title), body]);
    match kind {
        BlockQuoteKind::Note | BlockQuoteKind::Important => alert.info(),
        BlockQuoteKind::Tip => alert.success(),
        BlockQuoteKind::Warning => alert.warning(),
        BlockQuoteKind::Caution => alert.destructive(),
    }
}

fn build_list(start: Option<u64>, items: Vec<ListItem>) -> El {
    match start {
        None if !items.is_empty() && items.iter().all(|item| item.task_checked.is_some()) => {
            task_list(
                items
                    .into_iter()
                    .map(|item| (item.task_checked.unwrap_or(false), item.content)),
            )
        }
        None => bullet_list(items.into_iter().map(|item| item.content)),
        Some(start) => numbered_list_from(start, items.into_iter().map(|item| item.content)),
    }
}

/// Collapse one item's accumulated blocks into a single El. Single
/// block → that block; multiple blocks → wrap in `column`.
fn build_list_item(mut blocks: Vec<El>) -> El {
    if blocks.len() == 1 {
        blocks.pop().unwrap()
    } else {
        column(blocks)
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
    }
}

/// Build a `widgets::table` block from the parsed header and body
/// rows. Falls back to body-only if no header was emitted.
fn build_table(head: Option<Vec<El>>, body: Vec<Vec<El>>) -> El {
    // `body` came in as `Vec<Vec<El>>` (one outer per row) but each
    // inner Vec is single-element since `TagEnd::TableRow` already
    // built one `table_row(...)` per row. Re-flatten into row-Els.
    let body_rows: Vec<El> = body.into_iter().flatten().collect();
    match head {
        Some(header_rows) => table([table_header(header_rows), table_body(body_rows)]),
        None => table([table_body(body_rows)]),
    }
}

fn normalize_table_head_rows(items: Vec<El>) -> Vec<El> {
    if items.is_empty()
        || items
            .iter()
            .all(|item| item.metrics_role == Some(MetricsRole::TableRow))
    {
        items
    } else {
        vec![table_row(items)]
    }
}

/// Wrap accumulated inline runs into a header-styled or body-styled
/// table cell. Plain-text-only cells flow through `table_head` /
/// `text(...)` so the cell carries the right typography defaults; mixed
/// inline content uses `text_runs([...])` and keeps per-run styling.
fn build_table_cell(
    inlines: InlineBuffer,
    in_header: bool,
    alignment: Alignment,
    input: &str,
    source_range: Option<Range<usize>>,
    row_source_range: Option<Range<usize>>,
) -> El {
    let key_range = source_range.clone().or_else(|| row_source_range.clone());
    let row_group = row_source_range
        .as_ref()
        .map(|range| format!("md:table-row:{}..{}", range.start, range.end));
    let selection_source_range = row_source_range.or(source_range);
    let selection_source = inlines
        .selection_source(input, selection_source_range)
        .map(|source| {
            if let Some(group) = row_group {
                source.full_selection_group(group)
            } else {
                source
            }
        });
    let runs = inlines.into_runs();
    if in_header {
        let cell = if let Some(plain) = single_plain_text(&runs) {
            table_head(plain)
        } else if runs.is_empty() {
            table_head("")
        } else {
            table_head_el(text_runs(runs).width(Size::Fill(1.0)))
        };
        let cell = with_source_selection(cell, "th", selection_source, key_range);
        return apply_table_alignment(cell, alignment);
    }
    let cell = if let Some(plain) = single_plain_text(&runs) {
        table_cell(text(plain))
    } else if runs.is_empty() {
        table_cell(text(""))
    } else {
        table_cell(text_runs(runs).width(Size::Fill(1.0)))
    };
    let cell = with_source_selection(cell, "td", selection_source, key_range);
    apply_table_alignment(cell, alignment)
}

fn apply_table_alignment(mut el: El, alignment: Alignment) -> El {
    let text_align = match alignment {
        Alignment::None | Alignment::Left => TextAlign::Start,
        Alignment::Center => TextAlign::Center,
        Alignment::Right => TextAlign::End,
    };
    apply_text_align(&mut el, text_align);
    el
}

fn apply_text_align(el: &mut El, text_align: TextAlign) {
    el.text_align = text_align;
    for child in &mut el.children {
        apply_text_align(child, text_align);
    }
}

fn build_image_placeholder(alt: &str, dest_url: &str, title: &str) -> El {
    // Phase 2 doesn't wire image loading. Surface the alt text plus
    // source metadata so the page reads sensibly until image resolution
    // lands; muted + italic so it doesn't look like first-class content.
    let label = image_placeholder_label(alt, dest_url, title);
    let mut el = text(label).muted().italic();
    if !dest_url.is_empty() {
        el = el.link(dest_url.to_string());
    }
    el
}

fn image_placeholder_label(alt: &str, dest_url: &str, title: &str) -> String {
    let mut label = match (alt.is_empty(), dest_url.is_empty()) {
        (true, true) => "[image]".to_string(),
        (false, true) => format!("[image: {alt}]"),
        (true, false) => format!("[image: {dest_url}]"),
        (false, false) => format!("[image: {alt}] {dest_url}"),
    };
    if !title.is_empty() {
        label.push_str(" \"");
        label.push_str(title);
        label.push('"');
    }
    label
}

/// Inspect a run vector and return a single plain string if every run
/// is a default-styled `Kind::Text` leaf (no bold, italic, strike,
/// link, code, custom color). Drives the heading + paragraph builder
/// fast paths. The `Body` role's auto-applied `FOREGROUND` text color
/// counts as "default"; a run that explicitly sets a different color
/// disqualifies the fast path.
fn single_plain_text(runs: &[El]) -> Option<String> {
    let mut out = String::new();
    for run in runs {
        if !matches!(run.kind, Kind::Text) {
            return None;
        }
        if run.font_weight != FontWeight::Regular
            || run.text_italic
            || run.text_strikethrough
            || run.text_underline
            || run.text_link.is_some()
            || run.text_role != TextRole::Body
        {
            return None;
        }
        // Body role auto-sets `text_color = Some(FOREGROUND)`. Treat
        // `None` and `Some(FOREGROUND)` both as "default"; anything
        // else (a `.color(...)` override) is styled.
        if let Some(c) = run.text_color
            && c != tokens::FOREGROUND
        {
            return None;
        }
        let s = run.text.as_deref()?;
        out.push_str(s);
    }
    Some(out)
}

/// Trim a single trailing `\n` (pulldown-cmark always emits one at
/// the end of a fenced or indented code block).
fn strip_trailing_newline(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use aetna_core::draw_ops::draw_ops;
    use aetna_core::ir::DrawOp;
    use aetna_core::layout::layout;
    use aetna_core::selection::{Selection, SelectionPoint, SelectionRange, selected_text};
    use aetna_core::state::UiState;

    /// The transformer always wraps blocks in a `column`. Reach into
    /// it for the test assertions.
    fn blocks(input: &str) -> Vec<El> {
        match md(input) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        }
    }

    fn blocks_with_options(input: &str, options: MarkdownOptions) -> Vec<El> {
        match md_with_options(input, options) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        }
    }

    fn first_source_backed(el: &El) -> Option<&El> {
        if el.selection_source.is_some() {
            return Some(el);
        }
        el.children.iter().find_map(first_source_backed)
    }

    fn collect_source_backed<'a>(el: &'a El, out: &mut Vec<&'a El>) {
        if el.selection_source.is_some() {
            out.push(el);
        }
        for child in &el.children {
            collect_source_backed(child, out);
        }
    }

    fn selection(key: &str, start: usize, end: usize) -> Selection {
        Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new(key, start),
                head: SelectionPoint::new(key, end),
            }),
        }
    }

    #[test]
    fn empty_document_yields_an_empty_column() {
        let bs = blocks("");
        assert!(bs.is_empty());
    }

    #[test]
    fn h1_h2_h3_map_to_heading_constructors() {
        let bs = blocks("# Title\n\n## Subtitle\n\n### Section");
        assert_eq!(bs.len(), 3);
        assert_eq!(bs[0].kind, Kind::Heading);
        assert_eq!(bs[0].text.as_deref(), Some("Title"));
        assert_eq!(bs[0].text_role, TextRole::Display);
        assert_eq!(bs[1].text_role, TextRole::Heading);
        assert_eq!(bs[2].text_role, TextRole::Title);
    }

    #[test]
    fn h4_h5_h6_clamp_to_h3() {
        let bs = blocks("#### Four\n\n##### Five\n\n###### Six");
        for b in &bs {
            assert_eq!(b.kind, Kind::Heading);
            assert_eq!(b.text_role, TextRole::Title);
        }
    }

    #[test]
    fn markdown_heading_selection_copies_heading_marker_when_whole_heading_selected() {
        let input = "## Subtitle";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed heading");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 1, source.visible_len() - 1)).as_deref(),
            Some("ubtitl")
        );
    }

    #[test]
    fn markdown_blockquote_selection_copies_quote_marker_when_whole_line_selected() {
        let input = "> Quoted text";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed quote paragraph");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 1, source.visible_len() - 1)).as_deref(),
            Some("uoted tex")
        );
    }

    #[test]
    fn markdown_blockquote_selection_preserves_markers_for_heading_and_list_items() {
        let input = "> ## Quoted heading\n>\n> - quoted item";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);

        let selected_whole = |visible: &str| {
            let node = nodes
                .iter()
                .find(|node| {
                    node.selection_source
                        .as_ref()
                        .is_some_and(|source| source.visible == visible)
                })
                .expect("source-backed quoted node");
            let source = node.selection_source.as_ref().unwrap();
            let key = node.key.as_deref().unwrap();
            selected_text(&doc, &selection(key, 0, source.visible_len()))
        };

        assert_eq!(
            selected_whole("Quoted heading").as_deref(),
            Some("> ## Quoted heading")
        );
        assert_eq!(
            selected_whole("quoted item").as_deref(),
            Some("> - quoted item")
        );
    }

    #[test]
    fn plain_paragraph_collapses_to_paragraph_widget() {
        let bs = blocks("Just some prose.");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Text);
        assert_eq!(bs[0].text.as_deref(), Some("Just some prose."));
        assert_eq!(bs[0].text_wrap, TextWrap::Wrap);
    }

    #[test]
    fn paragraph_with_inline_styling_uses_text_runs() {
        let bs = blocks("Hello **world** and *italic* and `code`.");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Inlines);
        let runs: Vec<&El> = bs[0].children.iter().collect();
        // Plain "Hello " + bold "world" + " and " + italic "italic" +
        // " and " + code "code" + ".".
        assert!(
            runs.iter()
                .any(|r| r.font_weight == FontWeight::Bold && r.text.as_deref() == Some("world"))
        );
        assert!(
            runs.iter()
                .any(|r| r.text_italic && r.text.as_deref() == Some("italic"))
        );
        assert!(
            runs.iter()
                .any(|r| r.text_role == TextRole::Code && r.text.as_deref() == Some("code"))
        );
    }

    #[test]
    fn markdown_selection_copies_paragraph_source() {
        let input = "This is **bold**.";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed paragraph");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();

        let bold_start = source.visible.find("bold").unwrap();
        let bold_end = bold_start + "bold".len();
        assert_eq!(
            selected_text(&doc, &selection(key, bold_start, bold_end)).as_deref(),
            Some("**bold**")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, bold_start + 1, bold_end - 1)).as_deref(),
            Some("ol")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 0, bold_end)).as_deref(),
            Some("This is **bold**")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, bold_start, source.visible_len())).as_deref(),
            Some("**bold**.")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, bold_start + 1, source.visible_len())).as_deref(),
            Some("old.")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
    }

    #[test]
    fn markdown_selection_copies_all_delimiters_when_styled_text_fills_paragraph() {
        let input = "**bold**";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed paragraph");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();

        assert_eq!(source.visible, "bold");
        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
    }

    #[test]
    fn markdown_selection_copies_full_inline_construct_source() {
        let input = "Use `code` and [site](https://aetna.dev).";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed paragraph");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();

        let code_start = source.visible.find("code").unwrap();
        let code_end = code_start + "code".len();
        assert_eq!(
            selected_text(&doc, &selection(key, code_start, code_end)).as_deref(),
            Some("`code`")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, code_start + 1, code_end - 1)).as_deref(),
            Some("od")
        );

        let site_start = source.visible.find("site").unwrap();
        let site_end = site_start + "site".len();
        assert_eq!(
            selected_text(&doc, &selection(key, site_start, site_end)).as_deref(),
            Some("[site](https://aetna.dev)")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, site_start + 1, site_end - 1)).as_deref(),
            Some("it")
        );
    }

    #[test]
    fn markdown_list_selection_does_not_pull_in_previous_document_source() {
        let input = "Intro paragraph.\n\n- first item\n- second item\n- third item";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);
        let list_nodes: Vec<&El> = nodes
            .into_iter()
            .filter(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible.contains("item"))
            })
            .collect();
        assert_eq!(list_nodes.len(), 3);

        let first = list_nodes[0];
        let third = list_nodes[2];
        let first_key = first.key.as_deref().unwrap();
        let third_key = third.key.as_deref().unwrap();
        let third_len = third.selection_source.as_ref().unwrap().visible_len();

        let selected = selected_text(
            &doc,
            &Selection {
                range: Some(SelectionRange {
                    anchor: SelectionPoint::new(first_key, 0),
                    head: SelectionPoint::new(third_key, third_len),
                }),
            },
        );
        assert_eq!(
            selected.as_deref(),
            Some("- first item\n- second item\n- third item")
        );
    }

    #[test]
    fn markdown_list_whole_item_selection_copies_marker_source() {
        let input = "Intro paragraph.\n\n- first item\n- second item";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);
        let first_item = nodes
            .into_iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "first item")
            })
            .expect("first list item");
        let source = first_item.selection_source.as_ref().unwrap();
        let key = first_item.key.as_deref().unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some("- first item")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 1, source.visible_len() - 1)).as_deref(),
            Some("irst ite")
        );
    }

    #[test]
    fn markdown_list_selection_preserves_ordered_and_task_markers() {
        let input = "3. third item\n4. fourth item\n\n- [x] done item\n- [ ] todo item";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);

        let selected_whole_item = |visible: &str| {
            let item = nodes
                .iter()
                .find(|node| {
                    node.selection_source
                        .as_ref()
                        .is_some_and(|source| source.visible == visible)
                })
                .expect("list item");
            let source = item.selection_source.as_ref().unwrap();
            let key = item.key.as_deref().unwrap();
            selected_text(&doc, &selection(key, 0, source.visible_len()))
        };

        assert_eq!(
            selected_whole_item("third item").as_deref(),
            Some("3. third item")
        );
        assert_eq!(
            selected_whole_item("done item").as_deref(),
            Some("- [x] done item")
        );
        assert_eq!(
            selected_whole_item("todo item").as_deref(),
            Some("- [ ] todo item")
        );
    }

    #[test]
    fn math_option_routes_inline_and_display_math_to_math_nodes() {
        let bs = blocks_with_options(
            "Euler $e^{i\\pi}+1=0$\n\n$$\\frac{a}{b}$$",
            MarkdownOptions::default().math(true),
        );
        assert_eq!(bs.len(), 2);
        assert_eq!(bs[0].kind, Kind::Inlines);
        assert!(
            bs[0]
                .children
                .iter()
                .any(|child| matches!(child.kind, Kind::Math) && child.math.is_some())
        );
        assert_eq!(bs[1].kind, Kind::Math);
        assert_eq!(bs[1].math_display, MathDisplay::Block);
    }

    #[test]
    fn inline_math_selection_copies_original_tex_source_atomically() {
        let input = "Inline $x_1^2$ math.";
        let doc = md_with_options(input, MarkdownOptions::default().math(true));
        let node = first_source_backed(&doc).expect("source-backed paragraph");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();
        let math_start = source.visible.find('\u{fffc}').unwrap();
        let math_end = math_start + "\u{fffc}".len();

        assert_eq!(
            selected_text(&doc, &selection(key, math_start, math_end)).as_deref(),
            Some("$x_1^2$")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
    }

    #[test]
    fn display_math_selection_copies_original_tex_source_atomically() {
        let input = "$$\\frac{a}{b}$$";
        let doc = md_with_options(input, MarkdownOptions::default().math(true));
        let node = first_source_backed(&doc).expect("source-backed display math");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
    }

    #[test]
    fn link_groups_runs_under_the_same_url() {
        let bs = blocks("Check [the **bold** site](https://aetna.dev) for info.");
        assert_eq!(bs[0].kind, Kind::Inlines);
        let linked: Vec<&El> = bs[0]
            .children
            .iter()
            .filter(|r| r.text_link.as_deref() == Some("https://aetna.dev"))
            .collect();
        assert!(!linked.is_empty(), "expected at least one linked run");
        // The bold word inside the link keeps its bold flag plus the
        // shared href.
        assert!(linked.iter().any(|r| r.font_weight == FontWeight::Bold));
    }

    #[test]
    fn bullet_list_emits_bullet_list_widget() {
        let bs = blocks("- one\n- two\n- three");
        assert_eq!(bs.len(), 1);
        // bullet_list returns a column of overlay-stack items — the
        // children count matches the item count.
        assert_eq!(bs[0].kind, Kind::Group);
        assert_eq!(bs[0].axis, Axis::Column);
        assert_eq!(bs[0].children.len(), 3);
    }

    #[test]
    fn ordered_list_emits_numbered_list_widget() {
        let bs = blocks("1. alpha\n2. beta\n3. gamma");
        assert_eq!(bs[0].kind, Kind::Group);
        assert_eq!(bs[0].axis, Axis::Column);
        assert_eq!(bs[0].children.len(), 3);
        // The first item's marker slot should carry the "1." label.
        let first_marker_slot = &bs[0].children[0].children[0];
        let first_marker = &first_marker_slot.children[0];
        assert_eq!(first_marker.text.as_deref(), Some("1."));
    }

    #[test]
    fn ordered_list_preserves_non_one_start_number() {
        let bs = blocks("42. alpha\n43. beta");
        assert_eq!(bs[0].children.len(), 2);
        let first_marker_slot = &bs[0].children[0].children[0];
        let second_marker_slot = &bs[0].children[1].children[0];
        assert_eq!(first_marker_slot.children[0].text.as_deref(), Some("42."));
        assert_eq!(second_marker_slot.children[0].text.as_deref(), Some("43."));
    }

    #[test]
    fn task_list_emits_static_task_markers() {
        let bs = blocks("- [x] done\n- [ ] todo");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].children.len(), 2);

        let checked = &bs[0].children[0].children[0].children[0];
        let unchecked = &bs[0].children[1].children[0].children[0];
        assert_eq!(checked.kind, Kind::Custom("task_marker"));
        assert_eq!(unchecked.kind, Kind::Custom("task_marker"));
        assert_eq!(checked.fill, Some(tokens::PRIMARY));
        assert_eq!(unchecked.fill, Some(tokens::CARD));
        assert!(!checked.focusable);
        assert!(!unchecked.focusable);
    }

    #[test]
    fn nested_list_lives_inside_the_outer_item() {
        let input = "- outer one\n  - inner a\n  - inner b\n- outer two";
        let bs = blocks(input);
        assert_eq!(bs.len(), 1);
        let outer = &bs[0];
        assert_eq!(outer.children.len(), 2);
        // First outer item collapses to a multi-block column (paragraph
        // + nested list). The transformer wraps multi-block items in
        // `column`; reach into it.
        let first_item_body = &outer.children[0].children[1];
        // first_item_body is the body slot in the overlay-stack item
        // shape. It contains a single child: the list-item content.
        let inner_content = &first_item_body.children[0];
        // For nested-list items, the content is a column of [paragraph,
        // nested bullet_list]. The second child is the nested list.
        assert_eq!(inner_content.kind, Kind::Group);
        assert!(inner_content.children.len() >= 2);
    }

    #[test]
    fn blockquote_wraps_inner_paragraphs() {
        let bs = blocks("> First line.\n>\n> Second line.");
        assert_eq!(bs.len(), 1);
        // blockquote is a stack of [rule, body_column].
        assert_eq!(bs[0].kind, Kind::Group);
        assert_eq!(bs[0].axis, Axis::Overlay);
        assert_eq!(bs[0].children.len(), 2);
        let body = &bs[0].children[1];
        assert_eq!(body.children.len(), 2);
    }

    #[test]
    fn fenced_code_block_keeps_verbatim_text() {
        let bs = blocks("```\nfn main() {}\n```");
        assert_eq!(bs.len(), 1);
        // code_block surface contains a single mono text leaf that
        // resolves to the JBM monospace face via the El default
        // (themes can override with `with_mono_font_family`).
        let surface = &bs[0];
        assert_eq!(surface.surface_role, SurfaceRole::Sunken);
        let body = &surface.children[0];
        assert_eq!(body.text.as_deref(), Some("fn main() {}"));
        assert!(body.font_mono);
        assert_eq!(
            body.mono_font_family,
            aetna_core::tree::FontFamily::JetBrainsMono
        );
    }

    #[test]
    fn indented_code_block_keeps_verbatim_text() {
        let bs = blocks("    let x = 1;\n    let y = 2;");
        assert_eq!(bs.len(), 1);
        let body = &bs[0].children[0];
        assert_eq!(body.text.as_deref(), Some("let x = 1;\nlet y = 2;"));
    }

    #[test]
    fn markdown_code_block_selection_copies_fence_when_whole_body_selected() {
        let input = "```rust\nfn main() {}\nlet x = 1;\n```";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);
        let body = nodes
            .into_iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "fn main() {}\nlet x = 1;")
            })
            .expect("source-backed code body");
        let source = body.selection_source.as_ref().unwrap();
        let key = body.key.as_deref().unwrap();
        let main_start = source.visible.find("main").unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
        assert_eq!(
            selected_text(&doc, &selection(key, main_start, main_start + "main".len())).as_deref(),
            Some("main")
        );
    }

    #[test]
    fn markdown_indented_code_partial_selection_copies_visible_code() {
        let input = "    let x = 1;\n    let y = 2;";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed code body");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();
        let y_start = source.visible.find("let y").unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
        assert_eq!(
            selected_text(&doc, &selection(key, y_start, source.visible_len())).as_deref(),
            Some("let y = 2;")
        );
    }

    /// Fenced block with an unrecognised language falls back to the
    /// plain-mono `code_block(...)` path (single text leaf, no inline
    /// runs) — same behaviour as a fence with no info string.
    #[test]
    fn fenced_code_block_unknown_language_falls_back_to_plain_mono() {
        let bs = blocks("```nothinglikethis\nfn x() {}\n```");
        assert_eq!(bs.len(), 1);
        let body = &bs[0].children[0];
        assert_eq!(body.kind, Kind::Text);
        assert_eq!(body.text.as_deref(), Some("fn x() {}"));
        assert!(body.font_mono);
    }

    /// With the `highlighting` feature enabled (default), a fenced
    /// block tagged with a recognised language tokenises into a
    /// `text_runs` paragraph wrapped in the same code-block chrome.
    /// Tokens carry palette colors (here we just confirm there's more
    /// than one Text run and at least one carries a non-default color);
    /// finer mapping assertions live in `highlight::tests`.
    #[cfg(feature = "highlighting")]
    #[test]
    fn fenced_rust_code_block_emits_highlighted_runs() {
        let bs = blocks("```rust\n// hi\nfn main() {}\n```");
        assert_eq!(bs.len(), 1);
        let surface = &bs[0];
        assert_eq!(surface.surface_role, SurfaceRole::Sunken);
        let body = &surface.children[0];
        assert_eq!(body.kind, Kind::Inlines);
        assert!(body.font_mono);
        let text_runs: Vec<&El> = body
            .children
            .iter()
            .filter(|c| c.kind == Kind::Text)
            .collect();
        assert!(
            text_runs.len() > 2,
            "expected multiple highlighted runs, got {}",
            text_runs.len()
        );
        assert!(
            text_runs.iter().all(|r| r.font_mono),
            "every highlighted run should ride the mono path"
        );
        assert!(
            text_runs.iter().any(|r| r.text_color.is_some()),
            "expected at least one run to carry a syntax color"
        );
    }

    #[test]
    fn horizontal_rule_emits_a_divider() {
        let bs = blocks("Above.\n\n---\n\nBelow.");
        let kinds: Vec<&Kind> = bs.iter().map(|b| &b.kind).collect();
        assert!(kinds.iter().any(|k| matches!(k, Kind::Divider)));
    }

    #[test]
    fn hard_break_inside_paragraph_emits_hard_break_node() {
        // CommonMark hard break = trailing two spaces + newline.
        let bs = blocks("line one  \nline two");
        assert_eq!(bs[0].kind, Kind::Inlines);
        assert!(
            bs[0]
                .children
                .iter()
                .any(|c| matches!(c.kind, Kind::HardBreak))
        );
    }

    #[test]
    fn soft_break_renders_as_a_space() {
        let bs = blocks("line one\nline two");
        assert_eq!(bs[0].kind, Kind::Text);
        // Plain paragraph fast path; soft break became a single space.
        let s = bs[0].text.as_deref().unwrap();
        assert!(s.contains("line one line two"), "got {s:?}");
    }

    #[test]
    fn image_renders_as_alt_placeholder() {
        let bs = blocks("![diagram of pipeline](pipeline.png \"Pipeline\")");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].kind, Kind::Inlines);
        let run = &bs[0].children[0];
        let s = run.text.as_deref().unwrap_or("");
        assert!(s.contains("diagram of pipeline"), "got {s:?}");
        assert!(s.contains("pipeline.png"), "got {s:?}");
        assert!(s.contains("Pipeline"), "got {s:?}");
        assert_eq!(run.text_link.as_deref(), Some("pipeline.png"));
    }

    #[test]
    fn inline_image_placeholder_preserves_order() {
        let bs = blocks("Before ![alt](img.png) after");
        assert_eq!(bs[0].kind, Kind::Inlines);
        let text: String = bs[0]
            .children
            .iter()
            .filter_map(|run| run.text.as_deref())
            .collect();
        assert!(
            text.contains("Before [image: alt] img.png after"),
            "got {text:?}"
        );
    }

    #[test]
    fn markdown_image_selection_copies_image_source_atomically() {
        let input = "Before ![alt text](img.png \"Title\") after";
        let doc = md(input);
        let node = first_source_backed(&doc).expect("source-backed paragraph");
        let source = node.selection_source.as_ref().unwrap();
        let key = node.key.as_deref().unwrap();
        let label = "[image: alt text] img.png \"Title\"";
        let image_start = source.visible.find(label).unwrap();
        let image_end = image_start + label.len();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some(input)
        );
        assert_eq!(
            selected_text(&doc, &selection(key, image_start, image_end)).as_deref(),
            Some("![alt text](img.png \"Title\")")
        );
    }

    #[test]
    fn document_outer_column_carries_block_gap() {
        let el = md("# A\n\nb");
        assert_eq!(el.kind, Kind::Group);
        assert_eq!(el.gap, tokens::SPACE_4);
    }

    #[test]
    fn table_emits_header_plus_body_widget() {
        let bs = blocks(
            "\
| Name  | Role |\n\
|-------|------|\n\
| Ada   | dev  |\n\
| Grace | ops  |\n",
        );
        assert_eq!(bs.len(), 1);
        let t = &bs[0];
        // `widgets::table` is `Kind::Custom("table")` with a header
        // child and a body child.
        assert_eq!(t.kind, Kind::Custom("table"));
        assert_eq!(t.children.len(), 2);
        let header = &t.children[0];
        let body = &t.children[1];
        assert_eq!(header.kind, Kind::Custom("table_header"));
        assert_eq!(body.kind, Kind::Custom("table_body"));
        // Header has one row of two cells.
        assert_eq!(header.children.len(), 1);
        assert_eq!(header.children[0].children.len(), 2);
        assert_eq!(header.children[0].children[0].text.as_deref(), Some("Name"));
        assert_eq!(header.children[0].children[1].text.as_deref(), Some("Role"));
        // Body has two rows, each with two cells.
        assert_eq!(body.children.len(), 2);
        assert_eq!(body.children[0].children.len(), 2);
        assert_eq!(body.children[0].children[0].text.as_deref(), Some("Ada"));
        assert_eq!(body.children[0].children[1].text.as_deref(), Some("dev"));
    }

    #[test]
    fn table_header_cells_carry_caption_styling() {
        let bs = blocks(
            "\
| Header |\n\
|--------|\n\
| body   |\n",
        );
        let t = &bs[0];
        let header_cell = &t.children[0].children[0].children[0];
        assert_eq!(header_cell.text.as_deref(), Some("Header"));
        // `table_head(...)` applies the caption role.
        assert_eq!(header_cell.text_role, TextRole::Caption);
    }

    #[test]
    fn table_body_cells_with_inline_styling_use_text_runs() {
        let bs = blocks(
            "\
| Col |\n\
|-----|\n\
| **bold** word |\n",
        );
        let t = &bs[0];
        let body_cell = &t.children[1].children[0].children[0];
        // Body cell wraps the styled content in an Inlines paragraph.
        assert_eq!(body_cell.kind, Kind::Inlines);
        assert!(
            body_cell
                .children
                .iter()
                .any(|r| r.font_weight == FontWeight::Bold && r.text.as_deref() == Some("bold"))
        );
    }

    #[test]
    fn table_alignment_applies_to_header_and_body_cells() {
        let bs = blocks(
            "\
| Left | Center | Right |\n\
|:-----|:------:|------:|\n\
| a    | b      | c     |\n",
        );
        let t = &bs[0];
        let header_row = &t.children[0].children[0];
        let body_row = &t.children[1].children[0];

        assert_eq!(header_row.children[0].text_align, TextAlign::Start);
        assert_eq!(header_row.children[1].text_align, TextAlign::Center);
        assert_eq!(header_row.children[2].text_align, TextAlign::End);
        assert_eq!(body_row.children[0].text_align, TextAlign::Start);
        assert_eq!(body_row.children[1].text_align, TextAlign::Center);
        assert_eq!(body_row.children[2].text_align, TextAlign::End);
    }

    #[test]
    fn table_header_cells_preserve_inline_styling() {
        let bs = blocks(
            "\
| **Header** |\n\
|------------|\n\
| body       |\n",
        );
        let header_cell = &bs[0].children[0].children[0].children[0];
        assert_eq!(header_cell.kind, Kind::Inlines);
        assert!(
            header_cell
                .children
                .iter()
                .any(|r| r.font_weight == FontWeight::Bold
                    && r.text_role == TextRole::Caption
                    && r.text.as_deref() == Some("Header"))
        );
    }

    #[test]
    fn markdown_table_cell_selection_copies_row_source_when_whole_cell_selected() {
        let input = "\
| Name | Role |\n\
|------|------|\n\
| **Ada** | dev |\n";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);
        let ada_cell = nodes
            .into_iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "Ada")
            })
            .expect("source-backed Ada table cell");
        let source = ada_cell.selection_source.as_ref().unwrap();
        let key = ada_cell.key.as_deref().unwrap();

        assert_eq!(
            selected_text(&doc, &selection(key, 0, source.visible_len())).as_deref(),
            Some("| **Ada** | dev |")
        );
        assert_eq!(
            selected_text(&doc, &selection(key, 1, source.visible_len() - 1)).as_deref(),
            Some("d")
        );
    }

    #[test]
    fn markdown_table_row_selection_copies_pipe_row_source_once() {
        let input = "\
| Name | Role |\n\
|------|------|\n\
| **Ada** | dev |\n";
        let doc = md(input);
        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);
        let ada_cell = nodes
            .iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "Ada")
            })
            .expect("source-backed Ada table cell");
        let role_cell = nodes
            .iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "dev")
            })
            .expect("source-backed role table cell");
        let ada_key = ada_cell.key.as_deref().unwrap();
        let role_key = role_cell.key.as_deref().unwrap();
        let role_len = role_cell.selection_source.as_ref().unwrap().visible_len();

        let selected = selected_text(
            &doc,
            &Selection {
                range: Some(SelectionRange {
                    anchor: SelectionPoint::new(ada_key, 0),
                    head: SelectionPoint::new(role_key, role_len),
                }),
            },
        );
        assert_eq!(selected.as_deref(), Some("| **Ada** | dev |"));
    }

    #[test]
    fn markdown_table_header_draws_and_copy_preserves_separator() {
        let input = "\
| Construct  | Maps to            |\n\
|------------|--------------------|\n\
| Heading    | `h1` / `h2` / `h3` |\n\
| List       | `bullet_list` / `numbered_list` |\n\
| Blockquote | `blockquote`       |\n\
| Code block | `code_block`       |\n\
| Table      | `table`            |\n";
        let mut doc = scroll([column([md(input)])
            .gap(tokens::SPACE_4)
            .align(Align::Start)
            .width(Size::Fill(1.0))])
        .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut doc, &mut state, Rect::new(0.0, 0.0, 640.0, 240.0));
        let ops = draw_ops(&doc, &state);

        let text_y = |needle: &str| {
            ops.iter().find_map(|op| match op {
                DrawOp::GlyphRun { text, rect, .. } if text == needle => Some(rect.y),
                _ => None,
            })
        };
        let construct_y = text_y("Construct").expect("Construct header should draw");
        let heading_y = text_y("Heading").expect("Heading body cell should draw");
        assert!(
            construct_y < heading_y,
            "expected header above body, got Construct y={construct_y}, Heading y={heading_y}"
        );

        let mut nodes = Vec::new();
        collect_source_backed(&doc, &mut nodes);
        let construct_cell = nodes
            .iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "Construct")
            })
            .expect("source-backed Construct table cell");
        let heading_cell = nodes
            .iter()
            .find(|node| {
                node.selection_source
                    .as_ref()
                    .is_some_and(|source| source.visible == "Heading")
            })
            .expect("source-backed Heading table cell");
        let construct_key = construct_cell.key.as_deref().unwrap();
        let heading_key = heading_cell.key.as_deref().unwrap();
        let heading_len = heading_cell
            .selection_source
            .as_ref()
            .unwrap()
            .visible_len();
        let selected = selected_text(
            &doc,
            &Selection {
                range: Some(SelectionRange {
                    anchor: SelectionPoint::new(construct_key, 0),
                    head: SelectionPoint::new(heading_key, heading_len),
                }),
            },
        );
        assert_eq!(
            selected.as_deref(),
            Some(
                "| Construct  | Maps to            |\n|------------|--------------------|\n| Heading    | `h1` / `h2` / `h3` |"
            )
        );
    }

    #[test]
    fn strikethrough_inline_run_marks_text_strikethrough() {
        // GFM strikethrough is gated behind ENABLE_STRIKETHROUGH; the
        // walker already had the Tag matcher, but the parser only
        // emits the events when the option is on.
        let bs = blocks("Some ~~obsolete~~ text.");
        assert_eq!(bs[0].kind, Kind::Inlines);
        let strike: Vec<&El> = bs[0]
            .children
            .iter()
            .filter(|r| r.text_strikethrough)
            .collect();
        assert!(!strike.is_empty(), "expected a strikethrough run");
        assert_eq!(strike[0].text.as_deref(), Some("obsolete"));
    }

    #[test]
    fn smart_punctuation_is_opt_in() {
        let plain = blocks("Wait...");
        assert_eq!(plain[0].text.as_deref(), Some("Wait..."));

        let smart = match md_with_options(
            "Wait...",
            MarkdownOptions::default().smart_punctuation(true),
        ) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        };
        assert_eq!(smart[0].text.as_deref(), Some("Wait\u{2026}"));
    }

    #[test]
    fn gfm_alerts_are_opt_in() {
        let plain = blocks("> [!WARNING]\n> Careful.");
        assert_eq!(plain[0].axis, Axis::Overlay);

        let alert_blocks = match md_with_options(
            "> [!WARNING]\n> Careful.",
            MarkdownOptions::default().gfm_alerts(true),
        ) {
            el if matches!(el.kind, Kind::Group) && el.axis == Axis::Column => el.children,
            other => panic!("expected outer column, got {:?}", other.kind),
        };
        assert_eq!(alert_blocks[0].kind, Kind::Custom("alert"));
        assert_eq!(alert_blocks[0].children[0].text.as_deref(), Some("Warning"));
        assert_eq!(
            alert_blocks[0].fill,
            Some(tokens::WARNING.with_alpha_u8(38))
        );
    }

    #[cfg(feature = "html")]
    #[test]
    fn html_block_event_lands_as_block_when_feature_enabled() {
        // A block-level HTML scrap in the middle of markdown gets
        // parsed by aetna-html and appended in source order.
        let bs = blocks("First paragraph.\n\n<div><h2>From HTML</h2></div>\n\nLast paragraph.");
        assert_eq!(bs.len(), 3);
        assert_eq!(bs[0].text.as_deref(), Some("First paragraph."));
        // The middle block is the heading parsed out of the div.
        assert_eq!(bs[1].kind, Kind::Heading);
        assert_eq!(bs[1].text.as_deref(), Some("From HTML"));
        assert_eq!(bs[2].text.as_deref(), Some("Last paragraph."));
    }

    #[cfg(feature = "html")]
    #[test]
    fn html_block_script_is_dropped() {
        // Sanitization carries through the markdown → html bridge.
        let bs = blocks("Before.\n\n<script>alert('xss')</script>\n\nAfter.");
        let combined: String = bs
            .iter()
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(combined.contains("Before."));
        assert!(combined.contains("After."));
        assert!(!combined.contains("alert"));
    }

    #[cfg(not(feature = "html"))]
    #[test]
    fn html_block_event_is_dropped_when_feature_disabled() {
        // Without the `html` feature, raw HTML blocks render as nothing.
        let bs = blocks("First.\n\n<div><h2>Hidden</h2></div>\n\nLast.");
        let combined: String = bs
            .iter()
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!combined.contains("Hidden"));
    }
}
