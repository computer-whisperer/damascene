//! Pointer hit-testing and scroll routing on a laid-out tree.
//!
//! All entry points walk children in reverse paint order (top-most
//! visual first), respecting the inherited clip stack so a button outside
//! its scroll viewport can't be clicked. Only nodes with `key.is_some()`
//! are hit-test targets — author intent is "I tagged it with a key, it's
//! interactive."
//!
//! This keyed-only rule is also what gates
//! [`crate::tree::El::tooltip`]: an unkeyed leaf with `.tooltip()` is
//! silently dead because hover never lands on it. The bundle lint
//! flags this case as
//! [`crate::bundle::lint::FindingKind::DeadTooltip`].
//!
//! Reads computed rects from `UiState`'s layout side map (populated by
//! the layout pass) — the tree carries identity (`computed_id`) but not
//! geometry. Paint-time transforms (`translate`, `scale`) are then
//! applied in the same way `draw_ops::push_node` applies them, so
//! hit-testing matches what the user sees. Parent rects are *not*
//! barriers: a child can paint outside its parent (a swatch lifting on
//! `.scale(1.15)`) and still be hittable. Only `clip()` (an explicit
//! author-declared boundary) gates descent into descendants.

use std::ops::Range;

use crate::event::UiTarget;
use crate::selection::SelectionPoint;
use crate::state::UiState;
use crate::text::metrics;
use crate::theme::tokens;
use crate::tree::{El, FontWeight, Kind, Rect, Sides, TextWrap};

/// Find the topmost keyed node whose effective hit rect contains
/// `point` (logical pixels). Returns `None` if the point hits no keyed
/// node. By default the effective hit rect is the transformed laid-out
/// rect; nodes can opt into extra input area with `.hit_overflow(...)`.
pub fn hit_test(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<String> {
    hit_test_target(root, ui_state, point).map(|target| target.key)
}

/// Find the topmost keyed node and return full target metadata.
pub fn hit_test_target(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<UiTarget> {
    match hit_test_rec(root, ui_state, point, None, (0.0, 0.0)) {
        Hit::Target(c) => Some(c.target),
        Hit::Blocked | Hit::Miss => None,
    }
}

/// A candidate hit: the node's `UiTarget` plus the squared distance
/// from `point` to the node's *painted* rect. `distance_sq == 0` means
/// the point falls inside the painted rect; positive values mean the
/// point only reached this node via `hit_overflow` or the min-touch
/// auto-inflation. Squared so picking the best candidate stays
/// allocation-free and `sqrt`-free.
struct Candidate {
    target: UiTarget,
    distance_sq: f32,
}

enum Hit {
    Target(Candidate),
    /// A descendant declared `block_pointer` and no keyed target above
    /// it claimed the hit. Stops the click from leaking up to ancestors
    /// or earlier siblings.
    Blocked,
    Miss,
}

fn hit_test_rec(
    node: &El,
    ui_state: &UiState,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
) -> Hit {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return Hit::Miss;
    }
    // Mirror `draw_ops::push_node`: translate accumulates through the
    // subtree; scale applies to this node only and doesn't propagate.
    // Hit-testing must use the same painted rect that the user sees, or
    // clicks on a translated card land on whatever sibling occupies the
    // un-translated layout slot.
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = node.computed_rect;
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    // We do NOT early-return on `!painted_rect.contains(point)`.
    // A child can paint outside its parent's rect (the palette
    // swatches `.scale(1.15).translate(0, -8)` lift over the row's
    // computed bounds) and the only hard boundary is `inherited_clip`.
    // The painted-rect containment is checked below for self-as-target.
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    // Walk children in reverse paint order (top-most first) and pick
    // the *closest* candidate rather than the first one encountered.
    // Two neighboring buttons with overlapping `hit_overflow` (or
    // overlapping min-touch-target auto-inflation) used to give the
    // hit to whichever was later in the children list — the user
    // would tap "closer to A" and have B fire. With distance-aware
    // resolution a candidate whose *painted* rect actually contains
    // the point (distance 0) always beats one only reached via
    // inflation (distance > 0), and among equal-distance ties the
    // higher-z (first-visited in reverse) wins — preserving the
    // existing z-stacking semantic for overlays, scrims, and lifted
    // children. `Hit::Blocked` short-circuits only when no target has
    // been recorded yet; once a higher-z child has claimed a target,
    // a lower-z descendant's `block_pointer` is irrelevant.
    let mut best: Option<Candidate> = None;
    for child in node.children.iter().rev() {
        match hit_test_rec(child, ui_state, point, child_clip, total_translate) {
            Hit::Target(c) => {
                best = Some(better(best, c));
            }
            Hit::Blocked => {
                if best.is_none() {
                    return Hit::Blocked;
                }
                // A descendant blocked the hit but a higher-z sibling
                // already claimed the click — that target stands. The
                // block region applies to ancestors/lower-z siblings,
                // not the already-chosen winner.
                break;
            }
            Hit::Miss => {}
        }
    }
    // Self counts only if its effective hit rect contains the point
    // AND it's keyed (author tagged it interactive). The returned
    // target rect remains the transformed visual/layout rect: hit
    // overflow is an input affordance, not geometry apps should use
    // for pointer-to-value math.
    //
    // Min-touch-target floor: focusable / selectable nodes whose
    // painted size is below `tokens::MIN_TOUCH_TARGET` get an extra
    // symmetric inflation on top of any explicit `hit_overflow`.
    // Cards, scroll regions, and other keyed-but-non-interactive
    // surfaces are not auto-inflated — they shouldn't intercept taps
    // from neighboring controls just because the author wanted a
    // stable identity for state persistence.
    let auto_inflate = if node.focusable || node.selectable {
        min_touch_inflation(painted_rect)
    } else {
        Sides::default()
    };
    let hit_rect = painted_rect.outset(node.hit_overflow).outset(auto_inflate);
    if !hit_rect.contains(point.0, point.1) {
        return match best {
            Some(c) => Hit::Target(c),
            None => Hit::Miss,
        };
    }
    if let Some(key) = &node.key {
        let self_candidate = Candidate {
            target: UiTarget {
                key: key.clone(),
                node_id: node.computed_id.clone(),
                rect: painted_rect,
                tooltip: node.tooltip.clone(),
                scroll_offset_y: nearest_descendant_scroll_offset_y(node, ui_state),
            },
            distance_sq: point_distance_sq_from_rect(point, painted_rect),
        };
        return Hit::Target(promote_under_block(
            better(best, self_candidate),
            node.block_pointer,
        ));
    }
    if let Some(c) = best {
        return Hit::Target(promote_under_block(c, node.block_pointer));
    }
    if node.block_pointer {
        return Hit::Blocked;
    }
    Hit::Miss
}

/// `block_pointer` claims every click in the node's painted rect for
/// this subtree. When a child claimed the hit only via `hit_overflow`
/// or `min_touch_inflation` its `distance_sq > 0`, and a lower-z
/// sibling whose painted rect trivially contains the point (e.g. a
/// full-screen dismiss scrim behind a modal) would otherwise beat it
/// on `better()`. Promote the distance to zero so the blocking
/// subtree's hit wins on its own intent. Regression for #33: clicks
/// in the gap between sidebar buttons inside a modal dismissed the
/// modal because the buttons' inflated hit landed at `dsq>0` and the
/// scrim siblings claimed `dsq=0`.
fn promote_under_block(c: Candidate, blocking: bool) -> Candidate {
    if blocking {
        Candidate {
            target: c.target,
            distance_sq: 0.0,
        }
    } else {
        c
    }
}

/// Pick the better of two candidates: smaller squared distance wins;
/// on exact ties, the existing (first-recorded) one wins. Because the
/// caller walks children in reverse paint order, "first-recorded"
/// means "higher z", which preserves the z-stacking semantic for
/// overlays and lifted children.
fn better(existing: Option<Candidate>, incoming: Candidate) -> Candidate {
    match existing {
        Some(prev) if prev.distance_sq <= incoming.distance_sq => prev,
        _ => incoming,
    }
}

/// Squared euclidean distance from `point` to the nearest edge of
/// `rect`. Returns `0.0` when the point is inside (or on the edge of)
/// the rect. Squared because the hit-test only needs ordering — no
/// `sqrt` needed.
fn point_distance_sq_from_rect(point: (f32, f32), rect: Rect) -> f32 {
    let dx = (rect.x - point.0).max(point.0 - rect.right()).max(0.0);
    let dy = (rect.y - point.1).max(point.1 - rect.bottom()).max(0.0);
    dx * dx + dy * dy
}

/// Find the topmost selectable + keyed text leaf containing `point`
/// and return a [`SelectionPoint`] resolved against the leaf's text
/// content (one byte offset per Unicode scalar boundary).
///
/// Returns `None` when the point misses every selectable leaf, or
/// when the hit leaf has no text. Walks the same tree the focus
/// hit-test walks, with the same clip / translate rules — so a
/// selectable paragraph that's been scrolled out of view is correctly
/// excluded.
pub fn selection_point_at(root: &El, point: (f32, f32)) -> Option<SelectionPoint> {
    let mut hit: Option<SelectableHit<'_>> = None;
    selectable_rec(root, point, None, (0.0, 0.0), &mut hit);
    let SelectableHit { node, painted } = hit?;
    let key = node.key.clone()?;
    let value = node
        .selection_source
        .as_ref()
        .map(|source| source.visible.as_str())
        .or(node.text.as_deref())?;
    if matches!(node.kind, Kind::Inlines)
        && node.children.iter().any(|c| matches!(c.kind, Kind::Math))
        && let Some(byte) = mixed_inline_hit_byte(node, painted, point)
    {
        return Some(SelectionPoint { key, byte });
    }
    let local_x = (point.0 - painted.x).max(0.0);
    let local_y = (point.1 - painted.y).clamp(0.0, painted.h.max(1.0) - 1.0);
    let geometry = metrics::TextGeometry::new_with_family(
        value,
        node.font_size,
        effective_text_family(node),
        node.font_weight,
        node.font_mono,
        node.text_wrap,
        Some(painted.w),
    );
    let byte = match geometry.hit_byte(local_x, local_y) {
        Some(byte) => byte.min(value.len()),
        None => {
            if local_x <= 0.0 {
                0
            } else {
                value.len()
            }
        }
    };
    Some(SelectionPoint { key, byte })
}

fn mixed_inline_hit_byte(node: &El, painted_rect: Rect, point: (f32, f32)) -> Option<usize> {
    let glyph_rect = painted_rect.inset(node.padding);
    let items = mixed_inline_hit_items(node, glyph_rect);
    if items.is_empty() {
        return Some(0);
    }
    let line = mixed_hit_line(&items, point.1);
    let line_items: Vec<&MixedHitItem> = items.iter().filter(|item| item.line == line).collect();
    let first = line_items.first()?;
    let last = line_items.last()?;
    if point.0 <= first.rect.x {
        return Some(first.visible.start);
    }
    for item in &line_items {
        if point.0 <= item.rect.right() {
            return Some(match &item.kind {
                MixedHitKind::Text {
                    text,
                    font_size,
                    font_family,
                    font_weight,
                    font_mono,
                } => {
                    let geometry = metrics::TextGeometry::new_with_family(
                        text,
                        *font_size,
                        *font_family,
                        *font_weight,
                        *font_mono,
                        TextWrap::NoWrap,
                        None,
                    );
                    let local_x = (point.0 - item.rect.x).max(0.0);
                    let local_y = (point.1 - item.rect.y).clamp(0.0, item.rect.h.max(1.0) - 1.0);
                    let byte = geometry
                        .hit_byte(local_x, local_y)
                        .unwrap_or(if local_x <= 0.0 { 0 } else { text.len() });
                    item.visible.start + byte.min(text.len())
                }
                MixedHitKind::Atomic => {
                    if point.0 < item.rect.center_x() {
                        item.visible.start
                    } else {
                        item.visible.end
                    }
                }
            });
        }
    }
    Some(last.visible.end)
}

#[derive(Clone)]
struct PendingMixedHitItem {
    kind: PendingMixedHitKind,
    x: f32,
    visible: Range<usize>,
}

#[derive(Clone)]
enum PendingMixedHitKind {
    Text { child: Box<El>, text: String },
    Math { layout: crate::math::MathLayout },
}

struct MixedHitItem {
    rect: Rect,
    line_top: f32,
    line_bottom: f32,
    line: usize,
    visible: Range<usize>,
    kind: MixedHitKind,
}

enum MixedHitKind {
    Text {
        text: String,
        font_size: f32,
        font_family: crate::tree::FontFamily,
        font_weight: FontWeight,
        font_mono: bool,
    },
    Atomic,
}

fn mixed_inline_hit_items(node: &El, rect: Rect) -> Vec<MixedHitItem> {
    // `rect` is the painted (scale-transformed) rect, and
    // `flush_mixed_hit_line` measures glyphs at
    // `child.font_size * node.scale` — so the wrap pass must run in
    // the same scaled space or line breaks and item rects drift apart
    // on scaled selectable paragraphs. Mirrors the paint side
    // (`push_inline_mixed_ops`).
    let scale = node.scale;
    let mut breaker = crate::text::inline_mixed::MixedInlineBreaker::new(
        node.text_wrap,
        Some(rect.w),
        node.font_size * scale * 0.82,
        node.font_size * scale * 0.22,
        node.line_height * scale,
    );
    let mut pending = Vec::new();
    let mut out = Vec::new();
    let mut visible_cursor = 0usize;
    let mut line_index = 0usize;

    for child in &node.children {
        match child.kind {
            Kind::HardBreak => {
                flush_mixed_hit_line(node, rect, &mut breaker, &mut pending, &mut out, line_index);
                line_index += 1;
                visible_cursor += "\n".len();
            }
            Kind::Text => {
                if let Some(text) = &child.text {
                    for chunk in inline_text_chunks(text) {
                        let visible = visible_cursor..(visible_cursor + chunk.len());
                        visible_cursor += chunk.len();
                        let is_space = chunk.chars().all(char::is_whitespace);
                        if breaker.skips_leading_space(is_space) {
                            continue;
                        }
                        let (w, ascent, descent) = inline_text_chunk_metrics(child, chunk, scale);
                        if breaker.wraps_before(is_space, w) {
                            flush_mixed_hit_line(
                                node,
                                rect,
                                &mut breaker,
                                &mut pending,
                                &mut out,
                                line_index,
                            );
                            line_index += 1;
                        }
                        if breaker.skips_overflowing_space(is_space, w) {
                            continue;
                        }
                        if is_space
                            && !matches!(
                                pending.last(),
                                Some(PendingMixedHitItem {
                                    kind: PendingMixedHitKind::Text { .. },
                                    ..
                                })
                            )
                        {
                            breaker.push(w, ascent, descent);
                            continue;
                        }
                        pending.push(PendingMixedHitItem {
                            kind: PendingMixedHitKind::Text {
                                child: Box::new(child.clone()),
                                text: chunk.to_string(),
                            },
                            x: breaker.x(),
                            visible,
                        });
                        breaker.push(w, ascent, descent);
                    }
                }
            }
            Kind::Math => {
                if let Some(expr) = &child.math {
                    let layout =
                        crate::math::layout_math(expr, child.font_size * scale, child.math_display);
                    if breaker.wraps_before(false, layout.width) {
                        flush_mixed_hit_line(
                            node,
                            rect,
                            &mut breaker,
                            &mut pending,
                            &mut out,
                            line_index,
                        );
                        line_index += 1;
                    }
                    let visible_len = "\u{fffc}".len();
                    let visible = visible_cursor..(visible_cursor + visible_len);
                    visible_cursor += visible_len;
                    let width = layout.width;
                    let ascent = layout.ascent;
                    let descent = layout.descent;
                    pending.push(PendingMixedHitItem {
                        kind: PendingMixedHitKind::Math { layout },
                        x: breaker.x(),
                        visible,
                    });
                    breaker.push(width, ascent, descent);
                }
            }
            _ => {}
        }
    }
    flush_mixed_hit_line(node, rect, &mut breaker, &mut pending, &mut out, line_index);
    out
}

fn flush_mixed_hit_line(
    parent: &El,
    rect: Rect,
    breaker: &mut crate::text::inline_mixed::MixedInlineBreaker,
    pending: &mut Vec<PendingMixedHitItem>,
    out: &mut Vec<MixedHitItem>,
    line_index: usize,
) {
    let line = breaker.finish_line();
    let line_top = rect.y + line.top;
    let line_bottom =
        line_top + (line.ascent + line.descent).max(parent.line_height * parent.scale);
    let baseline_y = rect.y + line.top + line.ascent;
    for item in pending.drain(..) {
        match item.kind {
            PendingMixedHitKind::Text { child, text } => {
                let size = child.font_size * parent.scale;
                let glyph_layout = metrics::layout_text_with_line_height_and_family(
                    &text,
                    size,
                    child.line_height * parent.scale,
                    child.font_family,
                    child.font_weight,
                    child.font_mono,
                    child.text_tabular_numerals,
                    child.text_letter_spacing * parent.scale,
                    TextWrap::NoWrap,
                    None,
                );
                let glyph_baseline = glyph_layout
                    .lines
                    .first()
                    .map(|line| line.baseline)
                    .unwrap_or_else(|| metrics::line_height(size) * 0.75);
                out.push(MixedHitItem {
                    rect: Rect::new(
                        rect.x + item.x,
                        baseline_y - glyph_baseline,
                        glyph_layout.width,
                        glyph_layout.height,
                    ),
                    line_top,
                    line_bottom,
                    line: line_index,
                    visible: item.visible,
                    kind: MixedHitKind::Text {
                        text,
                        font_size: size,
                        font_family: child.font_family,
                        font_weight: child.font_weight,
                        font_mono: child.font_mono,
                    },
                });
            }
            PendingMixedHitKind::Math { layout } => {
                out.push(MixedHitItem {
                    rect: Rect::new(
                        rect.x + item.x,
                        baseline_y - layout.ascent,
                        layout.width,
                        layout.height(),
                    ),
                    line_top,
                    line_bottom,
                    line: line_index,
                    visible: item.visible,
                    kind: MixedHitKind::Atomic,
                });
            }
        }
    }
}

fn mixed_hit_line(items: &[MixedHitItem], y: f32) -> usize {
    for item in items {
        if y >= item.line_top && y <= item.line_bottom {
            return item.line;
        }
    }
    items
        .iter()
        .min_by(|a, b| {
            let ac = (a.line_top + a.line_bottom) * 0.5;
            let bc = (b.line_top + b.line_bottom) * 0.5;
            (y - ac).abs().total_cmp(&(y - bc).abs())
        })
        .map(|item| item.line)
        .unwrap_or(0)
}

fn inline_text_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut last_space = None;
    for (i, ch) in text.char_indices() {
        let is_space = ch.is_whitespace();
        match last_space {
            None => last_space = Some(is_space),
            Some(prev) if prev != is_space => {
                chunks.push(&text[start..i]);
                start = i;
                last_space = Some(is_space);
            }
            _ => {}
        }
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

fn inline_text_chunk_metrics(child: &El, text: &str, scale: f32) -> (f32, f32, f32) {
    let size = child.font_size * scale;
    let layout = metrics::layout_text_with_line_height_and_family(
        text,
        size,
        child.line_height * scale,
        child.font_family,
        child.font_weight,
        child.font_mono,
        child.text_tabular_numerals,
        child.text_letter_spacing * scale,
        TextWrap::NoWrap,
        None,
    );
    (layout.width, size * 0.82, size * 0.22)
}

/// Find the link URL of the topmost text-link run containing `point`.
///
/// Walks the tree for `Kind::Inlines` paragraphs whose painted rect
/// contains the pointer, re-runs the same shaping pipeline the paint
/// pass uses to find the byte under the pointer, then walks the
/// paragraph's child runs to identify which one owns that byte.
/// Returns the run's [`crate::tree::El::text_link`] URL, or `None`
/// when the click missed every link run.
///
/// This is the read side of the link-click contract: the runtime calls
/// it from `pointer_down` / `pointer_up` and the app sees the result as
/// a [`crate::event::UiEventKind::LinkActivated`] event with the URL in
/// [`crate::event::UiEvent::key`]. Damascene does not act on the URL — see
/// [`crate::event::App::drain_link_opens`] for the app-decided handoff
/// to a host opener.
pub fn link_at(root: &El, point: (f32, f32)) -> Option<String> {
    link_at_rec(root, point, None, (0.0, 0.0))
}

fn link_at_rec(
    node: &El,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
) -> Option<String> {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return None;
    }
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = node.computed_rect;
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    // Children paint last → are on top → check first. A link nested
    // inside an overlay should win over a link in the page beneath.
    for child in node.children.iter().rev() {
        if let Some(url) = link_at_rec(child, point, child_clip, total_translate) {
            return Some(url);
        }
    }
    if !matches!(node.kind, Kind::Inlines) {
        return None;
    }
    if !painted_rect.contains(point.0, point.1) {
        return None;
    }
    link_in_inlines_at(node, painted_rect, point)
}

fn link_in_inlines_at(node: &El, painted_rect: Rect, point: (f32, f32)) -> Option<String> {
    // Mirror `draw_ops`'s inline paragraph: glyphs paint inside the
    // node's padding rect, with the same per-paragraph font size /
    // line height aggregated from the child Text runs.
    let glyph_rect = painted_rect.inset(node.padding);
    if !glyph_rect.contains(point.0, point.1) {
        return None;
    }
    let runs = collect_link_runs(node);
    if runs.iter().all(|(_, link)| link.is_none()) {
        return None;
    }
    let concat: String = runs.iter().map(|(t, _)| t.as_str()).collect();
    if concat.is_empty() {
        return None;
    }
    let inline_size = inline_paragraph_font_size(node) * node.scale;
    let geometry = metrics::TextGeometry::new_with_family(
        &concat,
        inline_size,
        node.font_family,
        FontWeight::Regular,
        false,
        node.text_wrap,
        match node.text_wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => Some(glyph_rect.w),
        },
    );
    let local_x = (point.0 - glyph_rect.x).max(0.0);
    let local_y = (point.1 - glyph_rect.y).max(0.0);
    let byte = geometry.hit_byte(local_x, local_y)?;
    // Map the global byte offset back to the run that owns it. A byte
    // past the last grapheme means the click landed beyond the rendered
    // text (paragraphs hugged narrower than their layout slot leave
    // empty space at the end of each line) — treat as no link rather
    // than guessing the nearest run.
    let mut offset = 0usize;
    for (text, link) in &runs {
        let len = text.len();
        if byte < offset + len {
            return link.clone();
        }
        offset += len;
    }
    None
}

/// Mirror `draw_ops::collect_inline_runs` but keep just the run text
/// and link URL — that's all the link hit-test needs. Hard breaks
/// inject a `\n` so byte offsets line up with the shaped string.
fn collect_link_runs(node: &El) -> Vec<(String, Option<String>)> {
    let mut runs = Vec::new();
    for c in &node.children {
        match c.kind {
            Kind::Text => {
                if let Some(text) = &c.text {
                    runs.push((text.clone(), c.text_link.clone()));
                }
            }
            Kind::HardBreak => runs.push(("\n".to_string(), None)),
            _ => {}
        }
    }
    runs
}

/// Mirror `draw_ops::inline_paragraph_font_size` so the shaping
/// here matches the paint pass exactly.
fn inline_paragraph_font_size(node: &El) -> f32 {
    let mut size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) {
            size = size.max(c.font_size);
        }
    }
    size
}

/// Inner state carried while walking for a selectable target. We
/// keep a borrow of the El so the caller can read `text` / font
/// params after the walk completes — saving a second tree walk.
struct SelectableHit<'a> {
    node: &'a El,
    painted: Rect,
}

fn effective_text_family(node: &El) -> crate::tree::FontFamily {
    if node.font_mono {
        node.mono_font_family
    } else {
        node.font_family
    }
}

fn selectable_rec<'a>(
    node: &'a El,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
    out: &mut Option<SelectableHit<'a>>,
) {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return;
    }
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = node.computed_rect;
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    // Children paint on top → check first.
    for child in node.children.iter().rev() {
        selectable_rec(child, point, child_clip, total_translate, out);
        if out.is_some() {
            return;
        }
    }
    // Self counts only if it's a selectable + keyed text-bearing leaf and the
    // point lands inside its painted rect.
    if node.selectable
        && node.key.is_some()
        && (matches!(node.kind, Kind::Text | Kind::Heading) || node.selection_source.is_some())
        && painted_rect.contains(point.0, point.1)
    {
        *out = Some(SelectableHit {
            node,
            painted: painted_rect,
        });
    }
}

/// Return scrollable containers under `point`, ordered from outermost
/// to innermost. Wheel routing can then try the deepest target first
/// and bubble outward when that target cannot scroll in the requested
/// direction.
///
/// `block_pointer` subtrees that contain the point isolate scroll
/// routing — scrollables outside such a subtree are dropped, so a
/// dialog's scroll doesn't bubble out to the page underneath when it
/// hits an edge.
pub(crate) fn scroll_targets_at(root: &El, point: (f32, f32)) -> Vec<String> {
    let mut hits = Vec::new();
    scroll_target_rec(root, point, None, (0.0, 0.0), &mut hits);
    hits
}

fn scroll_target_rec(
    node: &El,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
    out: &mut Vec<String>,
) {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return;
    }
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = node.computed_rect;
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    let contains_point = painted_rect.contains(point.0, point.1);
    // A `block_pointer` node containing the point is a scroll-routing
    // barrier: anything collected from outside this subtree (pre-order,
    // so anything already in `out`) must not consume wheel events that
    // bubble past the inner scrollables here.
    if node.block_pointer && contains_point {
        out.clear();
    }
    // Self counts as a scroll target only if its painted rect contains
    // the point — but we still recurse into children regardless, since
    // a child can paint outside its parent (translate/scale).
    if node.scrollable && contains_point {
        out.push(node.computed_id.to_string());
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    for c in &node.children {
        scroll_target_rec(c, point, child_clip, total_translate, out);
    }
}

/// Is the node keyed `target_id` occluded at `point` by a `block_pointer`
/// overlay painted in front of it?
///
/// The wheel-zoom gates (`viewport`/`Scene3D`/`plot`) resolve their
/// target purely geometrically — rect containment, no z-order — so a
/// modal floated over a pan/zoom canvas still leaves the canvas rect
/// under the pointer and the wheel is taken as zoom instead of scrolling
/// the modal. This predicate lets those gates yield to scroll routing
/// (which is already `block_pointer`-aware, see [`scroll_targets_at`])
/// exactly when an overlay sits in front.
///
/// Occlusion is paint-order aware, mirroring `scroll_target_rec`'s
/// barrier rule (pre-order == paint order; later == in front):
/// - a `block_pointer` node nested *inside* the target subtree paints
///   over the target at that point → occludes;
/// - a `block_pointer` node in a *disjoint* subtree painted *after* the
///   target → in front → occludes;
/// - a `block_pointer` node painted *before* the target, or one that is
///   an *ancestor* of the target (the target lives inside the overlay,
///   e.g. a viewport in a modal) → behind or around it → does not.
pub(crate) fn occluded_by_overlay(root: &El, point: (f32, f32), target_id: &str) -> bool {
    let mut state = OcclusionScan {
        point,
        target_id,
        target_seen: false,
        occluded: false,
    };
    occlusion_rec(root, None, (0.0, 0.0), false, &mut state);
    state.occluded
}

struct OcclusionScan<'a> {
    point: (f32, f32),
    target_id: &'a str,
    /// Set once we visit the target node in pre-order. A disjoint
    /// `block_pointer` barrier counts as in-front only after this is set.
    target_seen: bool,
    occluded: bool,
}

fn occlusion_rec(
    node: &El,
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
    in_target: bool,
    state: &mut OcclusionScan<'_>,
) {
    if state.occluded {
        return;
    }
    if let Some(clip) = inherited_clip
        && !clip.contains(state.point.0, state.point.1)
    {
        return;
    }
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = node.computed_rect;
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    let contains_point = painted_rect.contains(state.point.0, state.point.1);
    let is_target = &*node.computed_id == state.target_id;
    let now_in_target = in_target || is_target;
    if is_target {
        state.target_seen = true;
    }
    if node.block_pointer && contains_point {
        if now_in_target {
            // The barrier is nested inside the target subtree (the target
            // itself is never `block_pointer`), so it paints over the
            // target at this point.
            if !is_target {
                state.occluded = true;
                return;
            }
        } else if state.target_seen {
            // Disjoint from the target and visited later → painted in front.
            state.occluded = true;
            return;
        }
        // Otherwise the barrier is behind the target (painted earlier) or
        // an ancestor of it — not an occluder.
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    for c in &node.children {
        occlusion_rec(c, child_clip, total_translate, now_in_target, state);
        if state.occluded {
            return;
        }
    }
}

/// Find the nearest `Kind::Scroll` descendant of `node` (or `node`
/// itself) and return its stored scroll offset y. Returns `0.0` when
/// no scroll lives in this subtree.
///
/// Walks pre-order, first-match-wins — widgets that compose a single
/// inner scroll (e.g. `text_area`) get the right offset in O(depth).
/// Widgets that nest multiple scrolls would need a different
/// convention; none exist today.
fn nearest_descendant_scroll_offset_y(node: &El, ui_state: &UiState) -> f32 {
    if matches!(node.kind, Kind::Scroll) {
        return ui_state
            .scroll
            .offsets
            .get(&*node.computed_id)
            .copied()
            .unwrap_or(0.0);
    }
    for c in &node.children {
        if let Some(off) = find_scroll_offset_y(c, ui_state) {
            return off;
        }
    }
    0.0
}

fn find_scroll_offset_y(node: &El, ui_state: &UiState) -> Option<f32> {
    if matches!(node.kind, Kind::Scroll) {
        return Some(
            ui_state
                .scroll
                .offsets
                .get(&*node.computed_id)
                .copied()
                .unwrap_or(0.0),
        );
    }
    node.children
        .iter()
        .find_map(|c| find_scroll_offset_y(c, ui_state))
}

fn translated(r: Rect, offset: (f32, f32)) -> Rect {
    if offset.0 == 0.0 && offset.1 == 0.0 {
        return r;
    }
    Rect::new(r.x + offset.0, r.y + offset.1, r.w, r.h)
}

/// Compute symmetric padding to bring `painted_rect` up to
/// [`tokens::MIN_TOUCH_TARGET`] on each axis. Returns
/// [`Sides::default`] when both dimensions already meet or exceed
/// the floor — small and stateless so the hit-test path can call
/// it per node without bookkeeping.
fn min_touch_inflation(painted_rect: Rect) -> Sides {
    let dx = ((tokens::MIN_TOUCH_TARGET - painted_rect.w).max(0.0)) * 0.5;
    let dy = ((tokens::MIN_TOUCH_TARGET - painted_rect.h).max(0.0)) * 0.5;
    Sides {
        left: dx,
        right: dx,
        top: dy,
        bottom: dy,
    }
}

fn scaled_around_center(r: Rect, s: f32) -> Rect {
    if (s - 1.0).abs() < f32::EPSILON {
        return r;
    }
    let cx = r.center_x();
    let cy = r.center_y();
    let w = r.w * s;
    let h = r.h * s;
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::tree::*;
    use crate::{button, column, row};

    fn lay_out_counter() -> (El, UiState) {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        (tree, state)
    }

    fn find_rect(node: &El, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(node.computed_rect);
        }
        node.children.iter().find_map(|c| find_rect(c, key))
    }

    fn find_text_rect(node: &El) -> Option<Rect> {
        if matches!(node.kind, Kind::Text) {
            return Some(node.computed_rect);
        }
        node.children.iter().find_map(find_text_rect)
    }

    /// Find the rect of the topmost `Kind::Inlines` paragraph. Inline
    /// children (Text leaves) have zero-size rects in layout so callers
    /// that want the painted box reach for the parent's instead.
    fn find_inlines_rect(node: &El) -> Option<Rect> {
        if matches!(node.kind, Kind::Inlines) {
            return Some(node.computed_rect);
        }
        node.children.iter().find_map(find_inlines_rect)
    }

    #[test]
    fn link_at_resolves_per_run_inside_inline_paragraph() {
        // Layout a paragraph that mixes plain text and a single linked
        // run. Clicks on the plain prefix should miss the link; clicks
        // on the linked run should resolve to its URL. This locks the
        // per-run hit test against regressing back to whole-paragraph
        // detection.
        const PREFIX: &str = "Visit ";
        const LINKED: &str = "github.com/computer-whisperer/damascene";
        const URL: &str = "https://github.com/computer-whisperer/damascene";
        let mut tree = column([crate::text_runs([
            crate::text(PREFIX),
            crate::text(LINKED).link(URL),
            crate::text("."),
        ])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 600.0, 200.0));
        let para = find_inlines_rect(&tree).expect("inlines rect");
        // Sanity: the layout fits on one line, so vertical center is
        // safe everywhere.
        let cy = para.y + para.h * 0.5;
        // Click 1px in from the left edge of the paragraph — squarely
        // inside the "Visit " prefix (no link).
        let prefix_x = para.x + 1.0;
        assert_eq!(
            link_at(&tree, (prefix_x, cy)),
            None,
            "clicking the unlinked prefix should not surface the link URL",
        );
        // Click well past the prefix — the linked run is much wider
        // than "Visit " and its trailing ".", so a probe halfway across
        // the paragraph is inside the linked region for any plausible
        // proportional font.
        let linked_x = para.x + para.w * 0.5;
        assert_eq!(
            link_at(&tree, (linked_x, cy)).as_deref(),
            Some(URL),
            "clicking inside the linked run should surface its URL",
        );
    }

    #[test]
    fn selection_point_for_mixed_inline_respects_math_widths() {
        use crate::selection::SelectionSource;

        const TEXT_A: &str = "alpha ";
        const TEXT_B: &str = " middle ";
        let object = "\u{fffc}";
        let visible = format!("{TEXT_A}{object}{TEXT_B}{object}");
        let expr_a = crate::math::parse_tex(r"\frac{a+b}{c+d}").expect("fixture TeX parses");
        let expr_b = crate::math::parse_tex(r"\sqrt{x_1+x_2}").expect("fixture TeX parses");
        let mut tree = column([crate::text_runs([
            crate::text(TEXT_A),
            crate::math_inline(expr_a.clone()),
            crate::text(TEXT_B),
            crate::math_inline(expr_b.clone()),
        ])
        .key("mixed")
        .selectable()
        .selection_source(SelectionSource::identity(visible))])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 700.0, 200.0));
        let para = find_inlines_rect(&tree).expect("inlines rect");

        let text_a_w = metrics::line_width_with_family(
            TEXT_A,
            16.0,
            FontFamily::default(),
            FontWeight::Regular,
            false,
        );
        let math_a_w =
            crate::math::layout_math(&expr_a, 16.0, crate::math::MathDisplay::Inline).width;
        let text_b_w = metrics::line_width_with_family(
            TEXT_B,
            16.0,
            FontFamily::default(),
            FontWeight::Regular,
            false,
        );
        let probe_x = para.x + text_a_w + math_a_w + text_b_w * 0.5;
        let probe_y = para.center_y();
        let point = selection_point_at(&tree, (probe_x, probe_y)).expect("selection point");

        let text_b_start = TEXT_A.len() + object.len();
        let math_b_start = text_b_start + TEXT_B.len();
        assert_eq!(point.key, "mixed");
        assert!(
            point.byte >= text_b_start && point.byte < math_b_start,
            "probe inside second text run must not jump to second math atom; got byte {}, expected {}..{}",
            point.byte,
            text_b_start,
            math_b_start,
        );
    }

    #[test]
    fn hit_test_finds_keyed_button() {
        let (tree, state) = lay_out_counter();
        for key in &["dec", "inc"] {
            let r = find_rect(&tree, key).expect("button rect");
            let center = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            let hit = hit_test(&tree, &state, center);
            assert_eq!(hit.as_deref(), Some(*key));
        }
    }

    #[test]
    fn hit_overflow_expands_pointer_target_but_not_target_rect() {
        let mut tree = column([button("x")
            .key("x")
            .hit_overflow(Sides::all(8.0))
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(24.0))])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

        let rect = find_rect(&tree, "x").expect("button rect");
        let target = hit_test_target(&tree, &state, (rect.x - 4.0, rect.center_y()))
            .expect("left hit overflow should route to the button");
        assert_eq!(target.key, "x");
        assert_eq!(
            target.rect, rect,
            "hit overflow should not change UiTarget::rect used by widgets for pointer math"
        );
    }

    #[test]
    fn paint_overflow_does_not_expand_pointer_target() {
        let mut tree = column([button("x")
            .key("x")
            .paint_overflow(Sides::all(8.0))
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(24.0))])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

        let rect = find_rect(&tree, "x").expect("button rect");
        assert_eq!(
            hit_test(&tree, &state, (rect.x - 4.0, rect.center_y())),
            None,
            "paint overflow is visual only; hit-test requires explicit hit_overflow"
        );
    }

    #[test]
    fn hit_overflow_respects_clipping_ancestor() {
        let mut tree = column([button("x")
            .key("x")
            .hit_overflow(Sides::left(16.0))
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(24.0))])
        .clip()
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(20.0, 0.0, 120.0, 80.0));

        let rect = find_rect(&tree, "x").expect("button rect");
        assert_eq!(
            hit_test(&tree, &state, (rect.x - 8.0, rect.center_y())).as_deref(),
            Some("x"),
            "overflow inside the ancestor clip should remain hittable"
        );
        assert_eq!(
            hit_test(&tree, &state, (19.0, rect.center_y())),
            None,
            "ancestor clip should bound hit overflow"
        );
    }

    #[test]
    fn hit_test_misses_unkeyed_text() {
        let (tree, state) = lay_out_counter();
        let r = find_text_rect(&tree).expect("text rect");
        let center = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        assert!(hit_test(&tree, &state, center).is_none());
    }

    #[test]
    fn hit_test_outside_returns_none() {
        let (tree, state) = lay_out_counter();
        assert!(hit_test(&tree, &state, (-10.0, -10.0)).is_none());
        assert!(hit_test(&tree, &state, (9999.0, 9999.0)).is_none());
    }

    #[test]
    fn hit_test_respects_clipping_ancestor() {
        let mut tree = column([row([
            button("-").key("visible"),
            button("+").key("clipped").width(Size::Fixed(240.0)),
        ])
        .clip()
        .width(Size::Fixed(80.0))]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));

        let clipped = find_rect(&tree, "clipped").expect("clipped button rect");
        assert!(hit_test(&tree, &state, (clipped.center_x(), clipped.center_y())).is_none());
    }

    #[test]
    fn hit_test_follows_ancestor_translate() {
        // A keyed button inside a column that is translated horizontally
        // by 120 px must be hit-testable at its translated location, and
        // the un-translated layout slot should miss. This guards against
        // a regression where `.translate()` (paint-time) shifts visuals
        // but hit-testing still uses layout rects, causing clicks on the
        // visually-shifted widget to land on whatever sibling occupies
        // the original layout slot.
        let mut tree = row([
            column([button("A").key("a")]).translate(120.0, 0.0),
            button("B").key("b"),
        ]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));

        let untranslated = find_rect(&tree, "a").expect("a layout rect");
        let translated_center = (untranslated.center_x() + 120.0, untranslated.center_y());
        let untranslated_center = (untranslated.center_x(), untranslated.center_y());

        assert_eq!(
            hit_test(&tree, &state, translated_center).as_deref(),
            Some("a"),
            "click at translated location should hit the translated button"
        );
        // The original layout slot may still belong to an ancestor row,
        // but it must not return "a" — that would be the bug.
        assert_ne!(
            hit_test(&tree, &state, untranslated_center).as_deref(),
            Some("a"),
            "click at the un-translated layout slot must not hit the translated button"
        );
    }

    #[test]
    fn hit_test_child_lifted_above_parent_still_hits() {
        // Reproduces the palette swatch bug: a child uses
        // `.scale(1.15).translate(0, -8)` so its painted rect lifts
        // above the parent row's layout rect. A click on the lifted
        // top edge must still find the child — the parent row's bounds
        // should not be a hit-test boundary, since only `clip()` is.
        let mut tree = row([crate::titled_card("c", [crate::text("body")])
            .key("swatch")
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(120.0))
            .scale(1.15)
            .translate(0.0, -20.0)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let layout_rect = find_rect(&tree, "swatch").expect("swatch rect");
        // Painted top is roughly: layout.y - 20 (translate) - layout.h * 0.075 (scale lift).
        let painted_top_y = layout_rect.y - 20.0 - layout_rect.h * 0.075 + 1.0;
        let painted_top_x = layout_rect.center_x();
        assert_eq!(
            hit_test(&tree, &state, (painted_top_x, painted_top_y)).as_deref(),
            Some("swatch"),
            "click on lifted top of scaled+translated child should hit"
        );
    }

    #[test]
    fn hit_test_translate_inherits_to_descendants() {
        // Ancestor translate should propagate through a chain of children.
        let mut tree = column([row([button("X").key("x")]).translate(0.0, 50.0)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let pre = find_rect(&tree, "x").expect("x layout rect");
        let translated = (pre.center_x(), pre.center_y() + 50.0);
        assert_eq!(
            hit_test(&tree, &state, translated).as_deref(),
            Some("x"),
            "ancestor translate must accumulate to descendants"
        );
    }

    #[test]
    fn min_touch_target_inflates_small_focusable_hit_rect() {
        // A 24×24 focusable widget surrounded by enough empty space.
        // The painted rect is below the 44px floor, so the runtime
        // should inflate the hit area by 10px on each side
        // ((44-24)/2 = 10) — clicks 10px outside the visual bounds
        // still land on the widget.
        let mut tree = column([crate::widgets::button::button("X")
            .key("tiny")
            .width(Size::Fixed(24.0))
            .height(Size::Fixed(24.0))])
        .padding(40.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r = find_rect(&tree, "tiny").expect("tiny rect");
        // Just outside the painted edge but inside the floor: ~9px
        // beyond the right edge — under the 10px deficit-half, so
        // still a hit.
        let just_outside = (r.x + r.w + 9.0, r.center_y());
        assert_eq!(
            hit_test(&tree, &state, just_outside).as_deref(),
            Some("tiny"),
            "tap within the min-touch floor should land on the button",
        );
        // Past the floor (15px outside, exceeds the inflated hit
        // area) → miss.
        let well_past = (r.x + r.w + 15.0, r.center_y());
        assert!(
            hit_test(&tree, &state, well_past).is_none(),
            "tap beyond the min-touch floor should miss",
        );
    }

    #[test]
    fn min_touch_target_does_not_inflate_non_interactive_keyed_node() {
        // A small `stack(...)` keyed for state persistence is NOT
        // focusable or selectable, so it should NOT auto-inflate.
        // Otherwise dense card layouts with stable keys would start
        // intercepting taps from neighboring controls.
        // A small keyed Group node — keyed for state persistence
        // but with no focusable / selectable flag.
        let mut tree = El::new(Kind::Group)
            .key("card")
            .width(Size::Fixed(20.0))
            .height(Size::Fixed(20.0));
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r = find_rect(&tree, "card").expect("card rect");
        // Just outside the painted edge — for a focusable, this
        // would be a hit; for our non-interactive keyed node, miss.
        let just_outside = (r.x + r.w + 5.0, r.center_y());
        assert!(
            hit_test(&tree, &state, just_outside).is_none(),
            "non-interactive keyed nodes must not auto-grow their hit rect",
        );
    }

    #[test]
    fn min_touch_target_stacks_additively_with_explicit_hit_overflow() {
        // A 24×24 focusable widget with an explicit
        // `hit_overflow(20.0)`. The min-touch deficit-half is 10;
        // total effective overflow is therefore 10 + 20 = 30px on
        // each side. A click 25px outside the painted edge — past
        // either floor alone but inside the sum — still hits.
        let mut tree = column([crate::widgets::button::button("X")
            .key("padded")
            .width(Size::Fixed(24.0))
            .height(Size::Fixed(24.0))
            .hit_overflow(Sides::all(20.0))])
        .padding(60.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 300.0));
        let r = find_rect(&tree, "padded").expect("padded rect");
        let stacked = (r.x + r.w + 25.0, r.center_y());
        assert_eq!(
            hit_test(&tree, &state, stacked).as_deref(),
            Some("padded"),
            "explicit hit_overflow should stack on top of the min-touch inflation",
        );
    }

    #[test]
    fn overlapping_inflated_siblings_go_to_closest_painted_rect() {
        // Regression: two small focusable siblings 4px apart each
        // auto-inflate to the 44px min-touch floor, so their inflated
        // hit rects overlap by ~24px in the gap. With first-hit-wins
        // in reverse paint order the later sibling always claimed
        // the overlap, even when the cursor was clearly closer to
        // the earlier sibling's painted rect. Distance-aware
        // resolution lets each tap go to whichever button is closer.
        use crate::widgets::button::button;
        let mut tree = row([
            button("A")
                .key("a")
                .width(Size::Fixed(16.0))
                .height(Size::Fixed(16.0)),
            crate::spacer().width(Size::Fixed(4.0)),
            button("B")
                .key("b")
                .width(Size::Fixed(16.0))
                .height(Size::Fixed(16.0)),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let a = find_rect(&tree, "a").expect("a rect");
        let b = find_rect(&tree, "b").expect("b rect");
        // Sanity: there's a real 4px gap and both inflations cover it.
        let mid_y = a.center_y();
        let just_past_a = (a.right() + 1.0, mid_y);
        let just_before_b = (b.x - 1.0, mid_y);
        assert_eq!(
            hit_test(&tree, &state, just_past_a).as_deref(),
            Some("a"),
            "tap one pixel past A's painted edge is closer to A than B",
        );
        assert_eq!(
            hit_test(&tree, &state, just_before_b).as_deref(),
            Some("b"),
            "tap one pixel before B's painted edge is closer to B than A",
        );
        // Right on the midline between the two — distance is exactly
        // equal, z-tie-breaks to the higher-z sibling. In a row
        // `[a, ..., b]`, b paints last → higher z → wins ties.
        let midpoint = ((a.right() + b.x) * 0.5, mid_y);
        assert_eq!(
            hit_test(&tree, &state, midpoint).as_deref(),
            Some("b"),
            "exact-midpoint ties resolve to the higher-z sibling",
        );
    }

    #[test]
    fn painted_rect_containment_beats_sibling_inflation_overlap() {
        // A small focusable widget (A) with min-touch auto-inflation
        // sits next to a much larger non-overlapping widget (B). When
        // the click falls *inside* A's painted rect — distance 0 —
        // A wins even though B's inflated rect technically also
        // covers the point. Previously the later sibling (B) would
        // win simply because it was checked first in reverse paint
        // order.
        use crate::widgets::button::button;
        let mut tree = row([
            button("A")
                .key("a")
                .width(Size::Fixed(16.0))
                .height(Size::Fixed(16.0)),
            crate::spacer().width(Size::Fixed(4.0)),
            // B has an aggressive hit_overflow that reaches back over
            // A's painted rect.
            button("B")
                .key("b")
                .width(Size::Fixed(16.0))
                .height(Size::Fixed(16.0))
                .hit_overflow(Sides::left(40.0)),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let a = find_rect(&tree, "a").expect("a rect");
        // Click inside A's painted rect.
        let inside_a = (a.center_x(), a.center_y());
        assert_eq!(
            hit_test(&tree, &state, inside_a).as_deref(),
            Some("a"),
            "tap inside A's painted rect must hit A, not a sibling whose \
             inflation reaches over A",
        );
    }

    #[test]
    fn unkeyed_blocking_node_stops_fallthrough() {
        use crate::tree::stack;
        let mut tree = stack([
            El::new(Kind::Scrim)
                .key("dismiss")
                .fill(crate::tokens::OVERLAY_SCRIM)
                .fill_size(),
            El::new(Kind::Modal)
                .block_pointer()
                .width(Size::Fixed(100.0))
                .height(Size::Fixed(100.0)),
        ])
        .align(Align::Center)
        .justify(Justify::Center)
        .fill_size();
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 300.0));

        assert!(hit_test(&tree, &state, (150.0, 150.0)).is_none());
        assert_eq!(
            hit_test(&tree, &state, (10.0, 10.0)).as_deref(),
            Some("dismiss")
        );
    }

    /// Wrap pass and measure pass of the mixed-inline hit items must
    /// share one scale convention: a paragraph at `scale(2.0)` hit-
    /// tested against a 2×-wide painted rect should break lines at the
    /// same chunks and place every item at 2× the unscaled position.
    /// Before the fix the wrap pass ran unscaled against the scaled
    /// rect, so line assignment and item rects drifted apart.
    #[test]
    fn scaled_mixed_inline_hit_items_match_unscaled_geometry() {
        let make = |scale: f32| {
            crate::text_runs([
                crate::text("alpha beta gamma delta epsilon zeta eta theta"),
                crate::math_inline(crate::math::parse_tex("x^2").unwrap()),
                crate::text("iota kappa lambda mu nu xi omicron pi rho sigma"),
            ])
            .text_wrap(TextWrap::Wrap)
            .scale(scale)
        };
        let base_rect = Rect::new(0.0, 0.0, 180.0, 400.0);
        let scaled_rect = Rect::new(0.0, 0.0, 360.0, 400.0);
        let base = mixed_inline_hit_items(&make(1.0), base_rect);
        let scaled = mixed_inline_hit_items(&make(2.0), scaled_rect);

        assert!(!base.is_empty());
        assert_eq!(base.len(), scaled.len(), "same chunks survive wrapping");
        assert!(
            base.last().unwrap().line > 0,
            "fixture must actually wrap to exercise line assignment"
        );
        for (b, s) in base.iter().zip(&scaled) {
            assert_eq!(b.line, s.line, "line breaks must not drift with scale");
            assert!(
                (s.rect.x - 2.0 * b.rect.x).abs() < 1.0,
                "item x: scaled={} unscaled={}",
                s.rect.x,
                b.rect.x
            );
            assert!(
                (s.rect.w - 2.0 * b.rect.w).abs() < 1.0,
                "item w: scaled={} unscaled={}",
                s.rect.w,
                b.rect.w
            );
        }
    }
}
