//! Library-level text selection model.
//!
//! Selection is a single, application-owned [`Selection`] value that
//! identifies *which* keyed text-bearing element holds the active
//! selection and *where* in that element's text the anchor and head
//! sit. The library enforces the single-selection invariant by
//! emitting `SelectionChanged` events; the app folds them into its
//! `Selection` field the same way it folds `apply_event` results into
//! a [`crate::widgets::text_input::TextSelection`] today.
//!
//! # Model
//!
//! - [`Selection`] — the slot, holds an `Option<SelectionRange>`.
//! - [`SelectionRange`] — anchor + head, both [`SelectionPoint`].
//! - [`SelectionPoint`] — `(key, byte)`. The key references the same
//!   widget-key form that `focus_order` already uses; the byte indexes
//!   into that element's text content.
//!
//! When `anchor.key == head.key` the selection lives entirely inside
//! one leaf — equivalent to a [`crate::widgets::text_input::TextSelection`]
//! over that leaf's text. When they differ, the selection spans
//! multiple leaves in document order.
//!
//! # Key requirement
//!
//! Selectable leaves must carry an explicit `.key(...)` — same
//! convention as focusable widgets. Without a key the leaf is silently
//! excluded from `selection_order` because nothing could survive a
//! tree rebuild as a stable identity. See [`crate::tree::El::selectable`].

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::ops::Range;

use crate::tree::{El, Kind};
use crate::widgets::text_input::TextSelection;

/// The application's single selection slot. `None` means nothing is
/// selected. The library emits `SelectionChanged` events that fold
/// into this; widgets read it back to draw highlight bands and route
/// editing operations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// The active range, or `None` when nothing is selected.
    pub range: Option<SelectionRange>,
}

/// A non-empty selection range. `anchor` is where the user started
/// (pointer-down); `head` is where they ended up (pointer current /
/// last move). The pair may be in tree order or reversed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    /// Where the selection started (pointer-down).
    pub anchor: SelectionPoint,
    /// Where the selection currently ends (pointer position / last
    /// extension).
    pub head: SelectionPoint,
}

/// A point inside a selectable leaf's text content. `key` is the
/// widget key that owns the leaf; `byte` is a byte offset into that
/// leaf's text (clamped to a UTF-8 char boundary by anything that
/// reads or writes it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionPoint {
    /// Widget key of the selectable leaf that owns this point.
    pub key: String,
    /// Byte offset into that leaf's visible text. Readers clamp it to
    /// a UTF-8 char boundary before slicing.
    pub byte: usize,
}

impl SelectionPoint {
    /// A point at `byte` inside the leaf keyed `key`.
    pub fn new(key: impl Into<String>, byte: usize) -> Self {
        Self {
            key: key.into(),
            byte,
        }
    }
}

/// Source-backed copy/hit-test payload for a selectable rich-text
/// node.
///
/// `visible` is the logical text users point at while selecting;
/// `source` is what copy should return. `spans` maps byte ranges in
/// `visible` to byte ranges in `source`. A plain text leaf has one
/// identity span. Markdown can instead map rendered words to their
/// original markdown source, and atomic embeds such as math can map a
/// one-byte object slot to the full `$...$` / `$$...$$` source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionSource {
    /// What copy should return — the underlying source text (e.g. raw
    /// markdown).
    pub source: String,
    /// The rendered text users point at while selecting.
    pub visible: String,
    /// Mappings from `visible` byte ranges to `source` byte ranges, in
    /// visible order.
    pub spans: Vec<SelectionSourceSpan>,
    /// Dedup group for full-leaf selections: adjacent leaves carrying
    /// the same group emit their shared `source_full` text once when a
    /// selection covers all of them (e.g. cells of one markdown table
    /// row). Set via [`Self::full_selection_group()`].
    pub full_selection_group: Option<String>,
}

/// One `visible` → `source` byte-range mapping inside a
/// [`SelectionSource`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSourceSpan {
    /// Byte range in [`SelectionSource::visible`] this span covers.
    pub visible: Range<usize>,
    /// Corresponding byte range in [`SelectionSource::source`], used
    /// for partial slices inside the span.
    pub source: Range<usize>,
    /// Byte range in [`SelectionSource::source`] emitted when the span
    /// is selected in full (or is atomic) — may include surrounding
    /// markup, e.g. `**bold**` for a visible `bold`.
    pub source_full: Range<usize>,
    /// Treat the span as indivisible: any overlap selects the whole
    /// `source_full` range (math embeds, object slots).
    pub atomic: bool,
}

impl SelectionSource {
    /// A payload with the given source and visible text and no spans
    /// yet — add mappings with [`Self::push_span`] /
    /// [`Self::push_span_with_full_source`].
    pub fn new(source: impl Into<String>, visible: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            visible: visible.into(),
            spans: Vec::new(),
            full_selection_group: None,
        }
    }

    /// A plain-text payload: `visible` and `source` are the same
    /// string, mapped by a single identity span.
    pub fn identity(text: impl Into<String>) -> Self {
        let text = text.into();
        let len = text.len();
        Self {
            source: text.clone(),
            visible: text,
            spans: vec![SelectionSourceSpan {
                visible: 0..len,
                source: 0..len,
                source_full: 0..len,
                atomic: false,
            }],
            full_selection_group: None,
        }
    }

    /// Builder-style: tag this payload with a full-selection dedup
    /// group (see the [`full_selection_group`][field@Self::full_selection_group]
    /// field docs).
    pub fn full_selection_group(mut self, group: impl Into<String>) -> Self {
        self.full_selection_group = Some(group.into());
        self
    }

    /// Append a span mapping `visible` to `source`, with
    /// `source_full = source`. Out-of-bounds or inverted ranges are
    /// dropped silently.
    pub fn push_span(&mut self, visible: Range<usize>, source: Range<usize>, atomic: bool) {
        self.push_span_with_full_source(visible, source.clone(), source, atomic);
    }

    /// Append a span whose full-selection copy text (`source_full`)
    /// differs from its partial-slice range (`source`) — e.g. a
    /// visible `bold` mapping to source `bold` inside the full
    /// `**bold**`. Out-of-bounds or inverted ranges are dropped
    /// silently.
    pub fn push_span_with_full_source(
        &mut self,
        visible: Range<usize>,
        source: Range<usize>,
        source_full: Range<usize>,
        atomic: bool,
    ) {
        if visible.start <= visible.end
            && visible.end <= self.visible.len()
            && source.start <= source.end
            && source.end <= self.source.len()
            && source_full.start <= source_full.end
            && source_full.end <= self.source.len()
        {
            self.spans.push(SelectionSourceSpan {
                visible,
                source,
                source_full,
                atomic,
            });
        }
    }

    /// Byte length of the visible text — the range selection offsets
    /// index into.
    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    /// Map the visible byte range `a..b` (order-insensitive, clamped
    /// to char boundaries) to one contiguous slice of `source`.
    /// Selecting the whole visible text returns the whole source.
    /// `None` when the mapped range collapses. Unlike
    /// [`Self::source_text_for_visible`] this cannot stitch
    /// per-span `source_full` pieces; prefer the owned variant for
    /// copy text.
    pub fn source_slice_for_visible(&self, a: usize, b: usize) -> Option<&str> {
        let (a, b) = (a.min(b), a.max(b));
        if a == 0 && b >= self.visible.len() && !self.source.is_empty() {
            return Some(&self.source);
        }
        let a = clamp_to_char_boundary(&self.visible, a.min(self.visible.len()));
        let b = clamp_to_char_boundary(&self.visible, b.min(self.visible.len()));
        let lo = self.source_offset_for_visible(a, Bias::Start)?;
        let hi = self.source_offset_for_visible(b, Bias::End)?;
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        let lo = clamp_to_char_boundary(&self.source, lo.min(self.source.len()));
        let hi = clamp_to_char_boundary(&self.source, hi.min(self.source.len()));
        (lo < hi).then(|| &self.source[lo..hi])
    }

    /// Copy text for the visible byte range `a..b` (order-insensitive,
    /// clamped to char boundaries): walks the spans, emitting
    /// `source_full` for atomic or fully-covered spans and
    /// proportional partial slices otherwise. Selecting the whole
    /// visible text returns the whole source. `None` when nothing
    /// maps. This is what [`selected_text`] uses for source-backed
    /// leaves.
    pub fn source_text_for_visible(&self, a: usize, b: usize) -> Option<String> {
        let (a, b) = (a.min(b), a.max(b));
        if a == 0 && b >= self.visible.len() && !self.source.is_empty() {
            return Some(self.source.clone());
        }
        let a = clamp_to_char_boundary(&self.visible, a.min(self.visible.len()));
        let b = clamp_to_char_boundary(&self.visible, b.min(self.visible.len()));
        if a >= b {
            return None;
        }
        if self.spans.is_empty() {
            return self.source_slice_for_visible(a, b).map(str::to_string);
        }

        let mut out = String::new();
        for span in &self.spans {
            let start = a.max(span.visible.start);
            let end = b.min(span.visible.end);
            if start >= end {
                continue;
            }
            if span.atomic || (start == span.visible.start && end == span.visible.end) {
                out.push_str(&self.source[span.source_full.clone()]);
                continue;
            }
            let lo = source_offset_in_span(span, start, Bias::Start)?;
            let hi = source_offset_in_span(span, end, Bias::End)?;
            let (lo, hi) = (lo.min(hi), lo.max(hi));
            let lo = clamp_to_char_boundary(&self.source, lo.min(self.source.len()));
            let hi = clamp_to_char_boundary(&self.source, hi.min(self.source.len()));
            if lo < hi {
                out.push_str(&self.source[lo..hi]);
            }
        }
        if out.is_empty() { None } else { Some(out) }
    }

    fn full_group_for_visible(&self, start: usize, end: usize) -> Option<&str> {
        (start == 0 && end >= self.visible.len())
            .then_some(self.full_selection_group.as_deref())
            .flatten()
    }

    fn source_offset_for_visible(&self, byte: usize, bias: Bias) -> Option<usize> {
        if self.spans.is_empty() {
            return Some(byte.min(self.source.len()));
        }
        for span in &self.spans {
            if byte < span.visible.start || byte > span.visible.end {
                continue;
            }
            if byte == span.visible.end && byte != span.visible.start && matches!(bias, Bias::Start)
            {
                continue;
            }
            if span.atomic {
                return Some(match bias {
                    Bias::Start => span.source.start,
                    Bias::End => span.source.end,
                });
            }
            let visible_len = span.visible.end.saturating_sub(span.visible.start);
            let source_len = span.source.end.saturating_sub(span.source.start);
            if visible_len == 0 {
                return Some(match bias {
                    Bias::Start => span.source.start,
                    Bias::End => span.source.end,
                });
            }
            let offset = byte.saturating_sub(span.visible.start).min(visible_len);
            let mapped = if source_len == visible_len {
                span.source.start + offset
            } else {
                span.source.start
                    + ((offset as f32 / visible_len as f32) * source_len as f32) as usize
            };
            return Some(mapped.min(span.source.end));
        }
        let first = self.spans.first()?;
        if byte <= first.visible.start {
            return Some(first.source.start);
        }
        let last = self.spans.last()?;
        if byte >= last.visible.end {
            return Some(last.source.end);
        }
        self.spans
            .windows(2)
            .find(|pair| byte > pair[0].visible.end && byte < pair[1].visible.start)
            .map(|pair| match bias {
                Bias::Start => pair[0].source.end,
                Bias::End => pair[1].source.start,
            })
    }
}

fn source_offset_in_span(span: &SelectionSourceSpan, byte: usize, bias: Bias) -> Option<usize> {
    if span.atomic {
        return Some(match bias {
            Bias::Start => span.source_full.start,
            Bias::End => span.source_full.end,
        });
    }
    let visible_len = span.visible.end.saturating_sub(span.visible.start);
    let source_len = span.source.end.saturating_sub(span.source.start);
    if visible_len == 0 {
        return Some(match bias {
            Bias::Start => span.source.start,
            Bias::End => span.source.end,
        });
    }
    let offset = byte.saturating_sub(span.visible.start).min(visible_len);
    let mapped = if source_len == visible_len {
        span.source.start + offset
    } else {
        span.source.start + ((offset as f32 / visible_len as f32) * source_len as f32) as usize
    };
    Some(mapped.min(span.source.end))
}

#[derive(Clone, Copy)]
enum Bias {
    Start,
    End,
}

impl Selection {
    /// A collapsed caret at `(key, byte)`. Convenience for tests and
    /// app-side initialization.
    pub fn caret(key: impl Into<String>, byte: usize) -> Self {
        let pt = SelectionPoint::new(key, byte);
        Self {
            range: Some(SelectionRange {
                anchor: pt.clone(),
                head: pt,
            }),
        }
    }

    /// True when there is no active selection.
    pub fn is_empty(&self) -> bool {
        self.range.is_none()
    }

    /// True when the selection lives entirely inside `key` — both
    /// anchor and head reference it. False for cross-element
    /// selections and for the empty selection.
    pub fn is_within(&self, key: &str) -> bool {
        match &self.range {
            Some(r) => r.anchor.key == key && r.head.key == key,
            None => false,
        }
    }

    /// True when `key` is the anchor's key (the originating leaf).
    pub fn anchored_at(&self, key: &str) -> bool {
        self.range.as_ref().is_some_and(|r| r.anchor.key == key)
    }

    /// View the selection through one leaf's lens: returns
    /// `Some(TextSelection)` only when the selection lives entirely
    /// inside `key`. Cross-element selections return `None` here —
    /// callers that need a per-leaf slice for a spanned leaf should
    /// instead consult the document-order range.
    pub fn within(&self, key: &str) -> Option<TextSelection> {
        let r = self.range.as_ref()?;
        if r.anchor.key == key && r.head.key == key {
            Some(TextSelection {
                anchor: r.anchor.byte,
                head: r.head.byte,
            })
        } else {
            None
        }
    }

    /// Replace this selection's slice for `key` from a freshly
    /// produced [`TextSelection`]. Used by editable widgets after
    /// folding an event: take the slice via [`Self::within`], let the
    /// widget mutate it, and write it back. No-op when the selection
    /// isn't currently within `key`.
    pub fn set_within(&mut self, key: &str, sel: TextSelection) {
        let Some(r) = self.range.as_mut() else { return };
        if r.anchor.key == key && r.head.key == key {
            r.anchor.byte = sel.anchor;
            r.head.byte = sel.head;
        }
    }

    /// Clear the selection.
    pub fn clear(&mut self) {
        self.range = None;
    }
}

/// Compute the byte range within `key`'s text that should be
/// highlighted, given the current `selection` and the document-order
/// list of selectable leaves. Returns `None` when `key` isn't part
/// of the selection range.
///
/// The painter calls this for each selectable leaf to decide whether
/// (and where) to draw a highlight band:
///
/// - Single-leaf selection: returns the `(lo, hi)` byte range when
///   `key` matches both endpoints.
/// - Anchor leaf (in cross-leaf): returns `(anchor.byte, text_len)`
///   for the leaf where the drag started.
/// - Head leaf (in cross-leaf): returns `(0, head.byte)` for the
///   leaf where the drag currently ends.
/// - Middle leaf: returns `(0, text_len)` — fully selected.
///
/// Anchor / head are normalized to document order using
/// `order` (keys in tree order, e.g. `selection_order` from
/// [`crate::state::UiState::selection_order`]).
pub fn slice_for_leaf(
    selection: &Selection,
    order: &[crate::event::UiTarget],
    key: &str,
    text_len: usize,
) -> Option<(usize, usize)> {
    let r = selection.range.as_ref()?;
    if r.anchor.key == r.head.key {
        if r.anchor.key != key {
            return None;
        }
        let (lo, hi) = (
            r.anchor.byte.min(r.head.byte).min(text_len),
            r.anchor.byte.max(r.head.byte).min(text_len),
        );
        return (lo < hi).then_some((lo, hi));
    }
    let pos = |k: &str| order.iter().position(|t| t.key == k);
    let (a_idx, h_idx, key_idx) = (pos(&r.anchor.key)?, pos(&r.head.key)?, pos(key)?);
    let (lo_idx, lo_byte, hi_idx, hi_byte) = if a_idx <= h_idx {
        (a_idx, r.anchor.byte, h_idx, r.head.byte)
    } else {
        (h_idx, r.head.byte, a_idx, r.anchor.byte)
    };
    if key_idx < lo_idx || key_idx > hi_idx {
        return None;
    }
    let lo = if key_idx == lo_idx {
        lo_byte.min(text_len)
    } else {
        0
    };
    let hi = if key_idx == hi_idx {
        hi_byte.min(text_len)
    } else {
        text_len
    };
    (lo < hi).then_some((lo, hi))
}

/// Walk `tree` and return the substring covered by `selection`.
/// Returns `None` for an empty selection or when the selection
/// references a key with no matching keyed text leaf in the tree.
///
/// For single-leaf selections (the only kind P1a renders) the
/// returned string is `value[lo..hi]` for that leaf. Cross-leaf
/// selections walk in tree order: anchor leaf from anchor.byte to
/// end, every leaf strictly between anchor and head fully, head leaf
/// up to head.byte. Joined with `\n` between leaves.
pub fn selected_text(tree: &El, selection: &Selection) -> Option<String> {
    let r = selection.range.as_ref()?;
    if r.anchor.key == r.head.key {
        if let Some(source) = find_keyed_selection_source(tree, &r.anchor.key) {
            let lo = r.anchor.byte.min(r.head.byte);
            let hi = r.anchor.byte.max(r.head.byte);
            return source.source_text_for_visible(lo, hi);
        }
        let value = find_keyed_text(tree, &r.anchor.key)?;
        // Selections are app-owned and can outlive the text they were
        // made against (streaming/edited leaves), so the offsets may
        // land mid-codepoint — snap to a boundary before slicing.
        let lo = clamp_to_char_boundary(&value, r.anchor.byte.min(r.head.byte).min(value.len()));
        let hi = clamp_to_char_boundary(&value, r.anchor.byte.max(r.head.byte).min(value.len()));
        if lo >= hi {
            return None;
        }
        return Some(value[lo..hi].to_string());
    }
    // Cross-leaf walk in tree order.
    let mut leaves: Vec<(String, LeafSelectionText)> = Vec::new();
    collect_keyed_selection_leaves(tree, &mut leaves);
    let anchor_idx = leaves.iter().position(|(k, _)| *k == r.anchor.key)?;
    let head_idx = leaves.iter().position(|(k, _)| *k == r.head.key)?;
    let (lo_idx, lo_byte, hi_idx, hi_byte) = if anchor_idx <= head_idx {
        (anchor_idx, r.anchor.byte, head_idx, r.head.byte)
    } else {
        (head_idx, r.head.byte, anchor_idx, r.anchor.byte)
    };
    let mut out = String::new();
    let mut last_group: Option<String> = None;
    for (i, (_, value)) in leaves
        .iter()
        .enumerate()
        .skip(lo_idx)
        .take(hi_idx - lo_idx + 1)
    {
        let start = if i == lo_idx {
            lo_byte.min(value.visible_len())
        } else {
            0
        };
        let end = if i == hi_idx {
            hi_byte.min(value.visible_len())
        } else {
            value.visible_len()
        };
        if start >= end {
            continue;
        }
        let Some(slice) = value.source_text_for_visible(start, end) else {
            continue;
        };
        let group = value.full_group_for_visible(start, end).map(str::to_string);
        if group.is_some() && group == last_group {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&slice);
        last_group = group;
    }
    if out.is_empty() { None } else { Some(out) }
}

pub(crate) fn find_keyed_text(node: &El, key: &str) -> Option<String> {
    if node.key.as_deref() == Some(key) {
        if let Some(source) = &node.selection_source {
            return Some(source.visible.clone());
        }
        if matches!(node.kind, Kind::Text | Kind::Heading)
            && let Some(t) = &node.text
        {
            return Some(t.clone());
        }
        let mut out = String::new();
        collect_text_content(node, &mut out);
        if !out.is_empty() {
            return Some(out);
        }
    }
    node.children.iter().find_map(|c| find_keyed_text(c, key))
}

pub(crate) fn find_keyed_selection_source(node: &El, key: &str) -> Option<SelectionSource> {
    if node.key.as_deref() == Some(key)
        && let Some(source) = &node.selection_source
    {
        return Some(source.clone());
    }
    node.children
        .iter()
        .find_map(|c| find_keyed_selection_source(c, key))
}

fn collect_text_content(node: &El, out: &mut String) {
    if matches!(node.kind, Kind::Text | Kind::Heading)
        && let Some(t) = &node.text
    {
        out.push_str(t);
    }
    for c in &node.children {
        collect_text_content(c, out);
    }
}

enum LeafSelectionText {
    Source(SelectionSource),
    Text(String),
}

impl LeafSelectionText {
    fn visible_len(&self) -> usize {
        match self {
            LeafSelectionText::Source(source) => source.visible_len(),
            LeafSelectionText::Text(text) => text.len(),
        }
    }

    fn source_text_for_visible(&self, start: usize, end: usize) -> Option<String> {
        match self {
            LeafSelectionText::Source(source) => source.source_text_for_visible(start, end),
            LeafSelectionText::Text(text) => {
                let start = clamp_to_char_boundary(text, start.min(text.len()));
                let end = clamp_to_char_boundary(text, end.min(text.len()));
                (start < end).then(|| text[start..end].to_string())
            }
        }
    }

    fn full_group_for_visible(&self, start: usize, end: usize) -> Option<&str> {
        match self {
            LeafSelectionText::Source(source) => source.full_group_for_visible(start, end),
            LeafSelectionText::Text(_) => None,
        }
    }
}

fn collect_keyed_selection_leaves(node: &El, out: &mut Vec<(String, LeafSelectionText)>) {
    if let (Some(k), Some(source)) = (&node.key, &node.selection_source) {
        out.push((k.clone(), LeafSelectionText::Source(source.clone())));
        return;
    }
    if matches!(node.kind, Kind::Text | Kind::Heading)
        && let (Some(k), Some(t)) = (&node.key, &node.text)
    {
        out.push((k.clone(), LeafSelectionText::Text(t.clone())));
    }
    for c in &node.children {
        collect_keyed_selection_leaves(c, out);
    }
}

/// Word range containing `byte`, returned as `(lo, hi)` byte offsets
/// into `text`. A *word* is a maximal run of `is_word_char` scalars
/// (alphanumeric, `_`, or `'`); when `byte` lands on a non-word
/// character the result is just that single codepoint, matching the
/// browser convention where double-clicking a punctuation mark
/// selects only that mark rather than the surrounding whitespace.
/// Used for double-click word selection.
///
/// `byte` is clamped to a UTF-8 char boundary; positions inside a
/// multi-byte codepoint snap to the previous boundary. An empty
/// `text` returns `(0, 0)`.
pub fn word_range_at(text: &str, byte: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let byte = clamp_to_char_boundary(text, byte.min(text.len()));
    // At the very end of the text, point at the previous codepoint so
    // double-click after the last word still selects that word rather
    // than collapsing to (len, len).
    let probe = if byte == text.len() {
        prev_char_boundary(text, byte)
    } else {
        byte
    };
    let probe_char = text[probe..].chars().next().unwrap_or(' ');
    if !is_word_char(probe_char) {
        // Non-word char → select just this codepoint. Avoids the
        // awkward "comma + space" double-select that grouping would
        // produce.
        return (probe, probe + probe_char.len_utf8());
    }

    // Word char → expand left and right through the run.
    let mut lo = probe;
    while lo > 0 {
        let p = prev_char_boundary(text, lo);
        let ch = text[p..].chars().next().unwrap();
        if !is_word_char(ch) {
            break;
        }
        lo = p;
    }
    let mut hi = probe;
    while hi < text.len() {
        let ch = text[hi..].chars().next().unwrap();
        if !is_word_char(ch) {
            break;
        }
        hi += ch.len_utf8();
    }
    (lo, hi)
}

/// Line range containing `byte`, returned as `(lo, hi)` byte offsets
/// into `text`. The range excludes the trailing `\n` so the matching
/// substring renders the visible line. An empty text returns `(0, 0)`.
/// Used for triple-click line selection.
///
/// `byte` is clamped to a UTF-8 char boundary; positions inside a
/// multi-byte codepoint snap to the previous boundary.
pub fn line_range_at(text: &str, byte: usize) -> (usize, usize) {
    let byte = clamp_to_char_boundary(text, byte.min(text.len()));
    let lo = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let hi = text[byte..]
        .find('\n')
        .map(|i| byte + i)
        .unwrap_or(text.len());
    (lo, hi)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// The byte offset of the next word boundary at or after `byte`, used
/// by `Ctrl+Right` style word-forward navigation in `text_input` and
/// `text_area`. Skips any non-word characters immediately after the
/// caret, then skips the following run of word characters and stops
/// just after it — matching the "jump to end of next word" convention
/// most desktop text editors use.
///
/// `byte` is clamped to a UTF-8 char boundary. At end of text, returns
/// `text.len()`.
pub fn next_word_boundary(text: &str, byte: usize) -> usize {
    let mut i = clamp_to_char_boundary(text, byte.min(text.len()));
    // Skip any non-word characters first (whitespace, punctuation).
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap();
        if is_word_char(ch) {
            break;
        }
        i += ch.len_utf8();
    }
    // Then skip the run of word characters.
    while i < text.len() {
        let ch = text[i..].chars().next().unwrap();
        if !is_word_char(ch) {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

/// The byte offset of the previous word boundary at or before `byte`,
/// used by `Ctrl+Left` style word-backward navigation. Skips any
/// non-word characters immediately before the caret, then skips the
/// preceding run of word characters and stops at its start — matching
/// the "jump to start of previous word" convention.
///
/// `byte` is clamped to a UTF-8 char boundary. At start of text,
/// returns `0`.
pub fn prev_word_boundary(text: &str, byte: usize) -> usize {
    let mut i = clamp_to_char_boundary(text, byte.min(text.len()));
    // Skip any non-word characters going backward.
    while i > 0 {
        let p = prev_char_boundary(text, i);
        let ch = text[p..].chars().next().unwrap();
        if is_word_char(ch) {
            break;
        }
        i = p;
    }
    // Then skip the run of word characters.
    while i > 0 {
        let p = prev_char_boundary(text, i);
        let ch = text[p..].chars().next().unwrap();
        if !is_word_char(ch) {
            break;
        }
        i = p;
    }
    i
}

fn clamp_to_char_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte;
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

fn prev_char_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte.saturating_sub(1);
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_has_no_views() {
        let sel = Selection::default();
        assert!(sel.is_empty());
        assert!(!sel.is_within("name"));
        assert!(sel.within("name").is_none());
    }

    #[test]
    fn caret_constructor_is_within_its_key() {
        let sel = Selection::caret("name", 3);
        assert!(!sel.is_empty());
        assert!(sel.is_within("name"));
        assert!(!sel.is_within("email"));
        let view = sel.within("name").expect("within name");
        assert_eq!(view, TextSelection::caret(3));
    }

    #[test]
    fn within_returns_none_for_cross_element_selection() {
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("para_a", 0),
                head: SelectionPoint::new("para_b", 5),
            }),
        };
        // Cross-element: neither lens reveals the full selection.
        assert!(sel.within("para_a").is_none());
        assert!(sel.within("para_b").is_none());
        // But the originating-leaf check still works.
        assert!(sel.anchored_at("para_a"));
        assert!(!sel.anchored_at("para_b"));
    }

    #[test]
    fn set_within_writes_back_a_modified_slice() {
        let mut sel = Selection::caret("name", 0);
        let mut view = sel.within("name").expect("caret");
        view.head = 5; // simulate widget editing the slice
        sel.set_within("name", view);
        let view_back = sel.within("name").expect("still within name");
        assert_eq!(view_back, TextSelection::range(0, 5));
    }

    #[test]
    fn set_within_is_a_noop_when_selection_is_not_in_key() {
        let mut sel = Selection::caret("name", 0);
        sel.set_within("email", TextSelection::range(0, 9));
        // Selection unchanged.
        assert_eq!(sel.within("name"), Some(TextSelection::caret(0)));
        assert!(sel.within("email").is_none());
    }

    #[test]
    fn selected_text_returns_single_leaf_substring() {
        let tree = crate::widgets::text::text("Hello, world!").key("p");
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("p", 7),
                head: SelectionPoint::new("p", 12),
            }),
        };
        assert_eq!(selected_text(&tree, &sel).as_deref(), Some("world"));
    }

    #[test]
    fn selected_text_snaps_stale_mid_codepoint_offsets_single_leaf() {
        // Selections persist across rebuilds; if the leaf text changed
        // underneath one, its byte offsets can land inside a multi-byte
        // codepoint. Slicing must snap, not panic.
        // "aé€b": a=0, é=1..3, €=3..6, b=6..7.
        let tree = crate::widgets::text::text("aé€b").key("p");
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("p", 2), // inside 'é'
                head: SelectionPoint::new("p", 4),   // inside '€'
            }),
        };
        assert_eq!(selected_text(&tree, &sel).as_deref(), Some("é"));
    }

    #[test]
    fn selected_text_snaps_stale_mid_codepoint_offsets_cross_leaf() {
        let tree = crate::column([
            crate::widgets::text::text("héllo").key("a"),
            crate::widgets::text::text("wörld").key("b"),
        ]);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("a", 2), // inside 'é'
                head: SelectionPoint::new("b", 2),   // inside 'ö'
            }),
        };
        assert_eq!(selected_text(&tree, &sel).as_deref(), Some("éllo\nw"));
    }

    #[test]
    fn line_range_at_snaps_mid_codepoint_byte() {
        // "é\nö": é=0..2, \n=2, ö=3..5. Byte 1 is inside 'é'.
        assert_eq!(line_range_at("é\nö", 1), (0, 2));
        assert_eq!(line_range_at("é\nö", 4), (3, 5));
    }

    #[test]
    fn selected_text_reads_text_inside_keyed_composite_widget() {
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("name", 1),
                head: SelectionPoint::new("name", 4),
            }),
        };
        let tree = crate::widgets::text_input::text_input("name", "hello", &sel);
        assert_eq!(selected_text(&tree, &sel).as_deref(), Some("ell"));
    }

    #[test]
    fn selected_text_walks_tree_order_for_cross_leaf_selection() {
        let tree = crate::column([
            crate::widgets::text::text("alpha").key("a"),
            crate::widgets::text::text("bravo").key("b"),
            crate::widgets::text::text("charlie").key("c"),
        ]);
        // Anchor inside "alpha" at byte 2, head inside "charlie" at
        // byte 4 — should yield "pha\nbravo\nchar" (joined by newline
        // between leaves; full middle leaf included).
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("a", 2),
                head: SelectionPoint::new("c", 4),
            }),
        };
        assert_eq!(
            selected_text(&tree, &sel).as_deref(),
            Some("pha\nbravo\nchar")
        );
    }

    #[test]
    fn selected_text_uses_source_payload_for_single_leaf() {
        let mut source = SelectionSource::new("This is **bold**.", "This is bold.");
        source.push_span(0..8, 0..8, false);
        source.push_span_with_full_source(8..12, 10..14, 8..16, false);
        source.push_span(12..13, 16..17, false);
        let tree = crate::text_runs([crate::text("This is "), crate::text("bold").bold()])
            .key("md:p")
            .selectable()
            .selection_source(source);

        let inner_only = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("md:p", 8),
                head: SelectionPoint::new("md:p", 12),
            }),
        };
        assert_eq!(
            selected_text(&tree, &inner_only).as_deref(),
            Some("**bold**")
        );

        let partial_inner = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("md:p", 9),
                head: SelectionPoint::new("md:p", 11),
            }),
        };
        assert_eq!(selected_text(&tree, &partial_inner).as_deref(), Some("ol"));

        let through_styled_span = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("md:p", 0),
                head: SelectionPoint::new("md:p", 12),
            }),
        };
        assert_eq!(
            selected_text(&tree, &through_styled_span).as_deref(),
            Some("This is **bold**")
        );

        let whole = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("md:p", 0),
                head: SelectionPoint::new("md:p", 13),
            }),
        };
        assert_eq!(
            selected_text(&tree, &whole).as_deref(),
            Some("This is **bold**.")
        );
    }

    #[test]
    fn selected_text_dedupes_adjacent_full_source_group_leaves() {
        let mut first = SelectionSource::new("| **Ada** | dev |", "Ada");
        first.push_span_with_full_source(0..3, 4..7, 0..17, false);
        let first = first.full_selection_group("row:0");

        let mut second = SelectionSource::new("| **Ada** | dev |", "dev");
        second.push_span_with_full_source(0..3, 12..15, 0..17, false);
        let second = second.full_selection_group("row:0");

        let tree = crate::row([
            crate::text("Ada")
                .key("a")
                .selectable()
                .selection_source(first),
            crate::text("dev")
                .key("b")
                .selectable()
                .selection_source(second),
        ]);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("a", 0),
                head: SelectionPoint::new("b", 3),
            }),
        };

        assert_eq!(
            selected_text(&tree, &sel).as_deref(),
            Some("| **Ada** | dev |")
        );
    }

    #[test]
    fn slice_for_leaf_single_leaf() {
        let order = order_for(&["a", "b", "c"]);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("b", 2),
                head: SelectionPoint::new("b", 5),
            }),
        };
        assert_eq!(slice_for_leaf(&sel, &order, "b", 10), Some((2, 5)));
        assert_eq!(slice_for_leaf(&sel, &order, "a", 10), None);
        assert_eq!(slice_for_leaf(&sel, &order, "c", 10), None);
    }

    #[test]
    fn slice_for_leaf_cross_leaf_anchor_to_head_in_doc_order() {
        // anchor = a@2, head = c@4: spans a, b, c.
        let order = order_for(&["a", "b", "c"]);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("a", 2),
                head: SelectionPoint::new("c", 4),
            }),
        };
        assert_eq!(
            slice_for_leaf(&sel, &order, "a", 10),
            Some((2, 10)),
            "anchor leaf: from anchor.byte to text_len"
        );
        assert_eq!(
            slice_for_leaf(&sel, &order, "b", 8),
            Some((0, 8)),
            "middle leaf: fully selected"
        );
        assert_eq!(
            slice_for_leaf(&sel, &order, "c", 10),
            Some((0, 4)),
            "head leaf: from 0 to head.byte"
        );
    }

    #[test]
    fn slice_for_leaf_cross_leaf_reversed_drag() {
        // anchor in c (later), head in a (earlier) — order shouldn't
        // matter; the slice is the same as forward drag.
        let order = order_for(&["a", "b", "c"]);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("c", 3),
                head: SelectionPoint::new("a", 1),
            }),
        };
        // Forward in doc order: a@1..end, b full, c 0..3.
        assert_eq!(slice_for_leaf(&sel, &order, "a", 5), Some((1, 5)));
        assert_eq!(slice_for_leaf(&sel, &order, "b", 6), Some((0, 6)));
        assert_eq!(slice_for_leaf(&sel, &order, "c", 9), Some((0, 3)));
    }

    #[test]
    fn slice_for_leaf_returns_none_for_leaves_outside_range() {
        // 5-leaf order; selection covers only b..d.
        let order = order_for(&["a", "b", "c", "d", "e"]);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("b", 0),
                head: SelectionPoint::new("d", 0),
            }),
        };
        assert_eq!(slice_for_leaf(&sel, &order, "a", 10), None);
        assert_eq!(slice_for_leaf(&sel, &order, "e", 10), None);
        // Boundary leaves with collapsed endpoints: anchor at b@0
        // means b's slice is (0, len). head at d@0 means d's slice is
        // (0, 0) which collapses → None.
        assert_eq!(slice_for_leaf(&sel, &order, "b", 4), Some((0, 4)));
        assert_eq!(slice_for_leaf(&sel, &order, "c", 7), Some((0, 7)));
        assert_eq!(slice_for_leaf(&sel, &order, "d", 5), None);
    }

    fn order_for(keys: &[&str]) -> Vec<crate::event::UiTarget> {
        keys.iter()
            .map(|k| crate::event::UiTarget {
                key: (*k).to_string(),
                node_id: format!("root.{k}"),
                rect: crate::tree::Rect::new(0.0, 0.0, 0.0, 0.0),
                tooltip: None,
                scroll_offset_y: 0.0,
            })
            .collect()
    }

    #[test]
    fn selected_text_returns_none_for_empty_or_unknown_keys() {
        let tree = crate::widgets::text::text("hi").key("p");
        assert!(selected_text(&tree, &Selection::default()).is_none());
        let unknown = Selection::caret("missing", 0);
        assert!(selected_text(&tree, &unknown).is_none());
    }

    #[test]
    fn word_range_at_picks_run_around_byte() {
        let text = "Hello, world!";
        // Byte 0 in "Hello" → whole word.
        assert_eq!(word_range_at(text, 0), (0, 5));
        // Byte 3 (inside "Hello") → whole word.
        assert_eq!(word_range_at(text, 3), (0, 5));
        // Byte 5 (the comma) → run of non-word chars (just ",").
        assert_eq!(word_range_at(text, 5), (5, 6));
        // Byte 6 (the space) → run of non-word chars (just " ").
        assert_eq!(word_range_at(text, 6), (6, 7));
        // Byte 7 (start of "world") → "world".
        assert_eq!(word_range_at(text, 7), (7, 12));
        // Byte 12 ("!") → "!".
        assert_eq!(word_range_at(text, 12), (12, 13));
    }

    #[test]
    fn word_range_at_treats_apostrophe_and_underscore_as_word_chars() {
        // Contractions stay one word.
        assert_eq!(word_range_at("don't stop", 2), (0, 5));
        // Identifier-style.
        assert_eq!(word_range_at("foo_bar baz", 4), (0, 7));
    }

    #[test]
    fn word_range_at_handles_end_of_text_and_empty() {
        let text = "hello";
        // Byte at len → snaps back into the trailing word.
        assert_eq!(word_range_at(text, 5), (0, 5));
        // Empty text → (0, 0).
        assert_eq!(word_range_at("", 0), (0, 0));
    }

    #[test]
    fn word_range_at_clamps_off_utf8_boundary() {
        // 'é' is two bytes; byte=1 sits inside the codepoint and snaps
        // back to byte 0, then expands into the run of non-ASCII word chars.
        let text = "café";
        let (lo, hi) = word_range_at(text, 1);
        assert_eq!((lo, hi), (0, text.len()));
    }

    #[test]
    fn line_range_at_returns_line_around_byte() {
        let text = "first\nsecond line\nthird";
        // First line: bytes 0..5 ("first"), \n at byte 5.
        assert_eq!(line_range_at(text, 0), (0, 5));
        assert_eq!(line_range_at(text, 3), (0, 5));
        assert_eq!(line_range_at(text, 5), (0, 5));
        // Second line: bytes 6..17 ("second line"), \n at byte 17.
        assert_eq!(line_range_at(text, 6), (6, 17));
        assert_eq!(line_range_at(text, 12), (6, 17));
        assert_eq!(line_range_at(text, 17), (6, 17));
        // Third (final, no trailing \n) line: bytes 18..23.
        assert_eq!(line_range_at(text, 18), (18, 23));
        assert_eq!(line_range_at(text, 23), (18, 23));
    }

    #[test]
    fn line_range_at_handles_empty_and_single_line() {
        assert_eq!(line_range_at("", 0), (0, 0));
        assert_eq!(line_range_at("just one line", 4), (0, 13));
    }
}
