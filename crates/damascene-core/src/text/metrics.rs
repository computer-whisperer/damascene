//! Font-backed text measurement and simple word wrapping.
//!
//! The production wgpu path uses [`crate::text::atlas::GlyphAtlas`] for
//! shaping + rasterization; layout, lint, SVG artifacts, and draw-op IR
//! all share this core layout artifact for measurement. Proportional
//! text is shaped through `cosmic-text` using bundled UI fonts; the older
//! TTF-advance path remains as a fallback and for monospace until Damascene
//! has a bundled mono font.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::tokens;
use crate::tree::{FontFamily, FontWeight, TextWrap};
use cosmic_text::{
    Attrs, Buffer, Cursor, Family, FeatureTag, FontFeatures, FontSystem, Metrics, Shaping, Weight,
    Wrap, fontdb,
};
use lru::LruCache;
use std::cell::RefCell;
use std::num::NonZeroUsize;

const MONO_CHAR_WIDTH_FACTOR: f32 = 0.62;

const BASELINE_MULTIPLIER: f32 = 0.93;

/// One measured visual line of a [`TextLayout`].
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// The line's text, trailing whitespace trimmed.
    pub text: String,
    /// Measured line width in logical pixels.
    pub width: f32,
    /// Top offset from the text layout origin, in logical pixels.
    pub y: f32,
    /// Baseline offset from the text layout origin, in logical pixels.
    pub baseline: f32,
    /// Paragraph direction as resolved by the shaping engine.
    pub rtl: bool,
}

/// Measured multi-line text layout. Coordinates are in logical pixels
/// (y-down) relative to the layout origin — the top-left of the rect
/// the layout would be drawn into.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    /// Visual lines in top-to-bottom order.
    pub lines: Vec<TextLine>,
    /// Widest line's width in logical pixels.
    pub width: f32,
    /// Total height in logical pixels — at least one `line_height`
    /// even for empty text.
    pub height: f32,
    /// Line height used for this layout, in logical pixels.
    pub line_height: f32,
}

impl TextLayout {
    /// Number of visual lines, counting empty text as one line.
    pub fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    /// Collapse to the size-only [`MeasuredText`] summary.
    pub fn measured(&self) -> MeasuredText {
        MeasuredText {
            width: self.width,
            height: self.height,
            line_count: self.line_count(),
        }
    }
}

/// Size-only summary of a [`TextLayout`], the artifact layout sizing
/// consumes.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredText {
    /// Widest line's width in logical pixels.
    pub width: f32,
    /// Total height in logical pixels.
    pub height: f32,
    /// Number of visual lines (at least 1).
    pub line_count: usize,
}

/// Per-frame counters for the layout-side text cache.
///
/// This tracks [`layout_text_with_line_height_and_family`], the cache
/// used by layout / draw-op measurement. It does not include backend
/// paint-side glyph atlas shaping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextLayoutCacheStats {
    /// Lookups answered from the cache.
    pub hits: u64,
    /// Lookups that had to reshape via cosmic-text.
    pub misses: u64,
    /// Entries pushed out of the bounded LRU by inserts.
    pub evictions: u64,
    /// Total UTF-8 bytes of text shaped on cache misses.
    pub shaped_bytes: u64,
}

/// Shared text geometry context for measurement, hit-testing, caret
/// positioning, and selection rectangles.
///
/// This is intentionally a thin value over the existing cosmic-text-backed
/// helpers: callers spell the text/style/wrap inputs once, then ask the
/// same context for the geometry operation they need. Keeping these calls
/// together matters for widgets like `text_input`, `text_area`, and
/// selectable text, where measurement, hit-testing, caret placement, and
/// selection bands must agree on font, wrap width, and line metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct TextGeometry<'a> {
    text: &'a str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
    layout: TextLayout,
}

impl<'a> TextGeometry<'a> {
    /// Build a geometry context with the default proportional family,
    /// laying out `text` once up front. `available_width` is the wrap
    /// width in logical px, used when `wrap == TextWrap::Wrap`.
    pub fn new(
        text: &'a str,
        size: f32,
        weight: FontWeight,
        mono: bool,
        wrap: TextWrap,
        available_width: Option<f32>,
    ) -> Self {
        Self::new_with_family(
            text,
            size,
            FontFamily::default(),
            weight,
            mono,
            wrap,
            available_width,
        )
    }

    /// [`Self::new`] with an explicit proportional font family.
    pub fn new_with_family(
        text: &'a str,
        size: f32,
        family: FontFamily,
        weight: FontWeight,
        mono: bool,
        wrap: TextWrap,
        available_width: Option<f32>,
    ) -> Self {
        let layout = layout_text_with_family(
            text,
            size,
            family,
            weight,
            mono,
            false,
            wrap,
            available_width,
        );
        Self {
            text,
            size,
            family,
            weight,
            mono,
            wrap,
            available_width,
            layout,
        }
    }

    /// The source text this context was built over.
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// The layout computed at construction.
    pub fn layout(&self) -> &TextLayout {
        &self.layout
    }

    /// Size-only summary of the layout.
    pub fn measured(&self) -> MeasuredText {
        self.layout.measured()
    }

    /// Line height of the layout, in logical pixels.
    pub fn line_height(&self) -> f32 {
        self.layout.line_height
    }

    /// Widest line's width in logical pixels.
    pub fn width(&self) -> f32 {
        self.layout.width
    }

    /// Total layout height in logical pixels.
    pub fn height(&self) -> f32 {
        self.layout.height
    }

    /// Hit-test a point in layout-origin coordinates (logical px);
    /// see [`hit_text`].
    pub fn hit(&self, x: f32, y: f32) -> Option<TextHit> {
        hit_text_with_family(
            self.text,
            self.size,
            self.family,
            self.weight,
            self.wrap,
            self.available_width,
            x,
            y,
        )
    }

    /// Hit-test and convert the result to a global byte offset in
    /// `self.text`. This is the shape most widgets want; cosmic-text's
    /// cursor reports `(line, byte-in-line)` and hard line breaks need to
    /// be folded back into the original string.
    pub fn hit_byte(&self, x: f32, y: f32) -> Option<usize> {
        let hit = self.hit(x, y)?;
        Some(self.byte_from_line_position(hit.line, hit.byte_index))
    }

    /// Caret position for a global byte offset, in layout-origin
    /// logical px; see [`caret_xy`].
    pub fn caret_xy(&self, byte_index: usize) -> (f32, f32) {
        caret_xy_with_family(
            self.text,
            byte_index,
            self.size,
            self.family,
            self.weight,
            self.wrap,
            self.available_width,
        )
    }

    /// X position of the caret at `byte_index`. For single-line text this
    /// replaces ad-hoc substring measurement and preserves shaping/kerning
    /// decisions made by the text engine.
    pub fn prefix_width(&self, byte_index: usize) -> f32 {
        self.caret_xy(byte_index).0
    }

    /// Per-visual-line selection rects for the global byte range
    /// `lo..hi`; see [`selection_rects`].
    pub fn selection_rects(&self, lo: usize, hi: usize) -> Vec<(f32, f32, f32, f32)> {
        selection_rects_with_family(
            self.text,
            lo,
            hi,
            self.size,
            self.family,
            self.weight,
            self.wrap,
            self.available_width,
        )
    }

    fn byte_from_line_position(&self, line: usize, byte_in_line: usize) -> usize {
        line_position_to_byte(self.text, line, byte_in_line)
    }
}

/// Measure text in logical pixels. `available_width` is only used when
/// `wrap == TextWrap::Wrap`; `None` means measure explicit newlines only.
pub fn measure_text(
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> MeasuredText {
    layout_text(text, size, weight, mono, wrap, available_width).measured()
}

/// Lay out text into measured lines. Coordinates in [`TextLine`] are
/// relative to the layout origin; callers place the layout inside a
/// rectangle and apply alignment/vertical centering as needed.
pub fn layout_text(
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> TextLayout {
    layout_text_with_family(
        text,
        size,
        FontFamily::default(),
        weight,
        mono,
        false,
        wrap,
        available_width,
    )
}

/// Lay out text with an explicit proportional UI font family.
#[allow(clippy::too_many_arguments)]
pub fn layout_text_with_family(
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    tabular: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> TextLayout {
    layout_text_with_line_height_and_family(
        text,
        size,
        line_height(size),
        family,
        weight,
        mono,
        tabular,
        0.0,
        wrap,
        available_width,
    )
}

/// Lay out text with an explicit line-height token. This is the
/// preferred path for styled elements; [`layout_text`] remains the
/// fallback for arbitrary measurement callers.
#[allow(clippy::too_many_arguments)]
pub fn layout_text_with_line_height(
    text: &str,
    size: f32,
    line_height: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> TextLayout {
    layout_text_with_line_height_and_family(
        text,
        size,
        line_height,
        FontFamily::default(),
        weight,
        mono,
        false,
        0.0,
        wrap,
        available_width,
    )
}

/// [`layout_text_with_line_height`] with an explicit proportional
/// font family. This is the fully-parameterized entry point all other
/// layout/measure helpers funnel into, and the one fronted by the
/// bounded LRU layout cache (see [`take_shape_cache_stats`]).
#[allow(clippy::too_many_arguments)]
pub fn layout_text_with_line_height_and_family(
    text: &str,
    size: f32,
    line_height: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    tabular: bool,
    letter_spacing: f32,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> TextLayout {
    // Cache by full shaping inputs. The intrinsic measurement pass
    // walks subtrees recursively at every flex level, so the same text
    // node easily gets shaped 2 × tree-depth times per frame.
    // `ellipsize_text_with_family`'s binary search adds further
    // repetition by reshaping each candidate prefix. The cache
    // amortizes all of that to a single hash lookup once the layout
    // has been computed for the first time. `TextLayout` is fully
    // owned (no borrows back into `FontSystem`), so the cached values
    // are safe to clone out across frames. Bounded LRU keeps memory
    // predictable; eviction is benign — the next call recomputes.
    // Probe with a reused thread-local key: the text is copied into
    // the scratch key's existing capacity, so cache hits perform zero
    // heap allocation. Only a miss materializes an owned key for the
    // insert (same cost as shaping's era anyway).
    let cached = SHAPE_KEY_SCRATCH.with_borrow_mut(|key| {
        key.text.clear();
        key.text.push_str(text);
        key.size_bits = size.to_bits();
        key.line_height_bits = line_height.to_bits();
        key.family = family;
        key.weight = weight;
        key.mono = mono;
        key.tabular = tabular;
        key.letter_spacing_bits = letter_spacing.to_bits();
        key.wrap = wrap;
        key.available_width_bits = available_width.map(f32::to_bits);
        SHAPE_CACHE.with_borrow_mut(|c| c.get(key).cloned())
    });
    if let Some(cached) = cached {
        SHAPE_CACHE_STATS.with_borrow_mut(|stats| {
            stats.hits += 1;
        });
        return cached;
    }
    SHAPE_CACHE_STATS.with_borrow_mut(|stats| {
        stats.misses += 1;
        stats.shaped_bytes += text.len() as u64;
    });
    let layout = layout_text_uncached(
        text,
        size,
        line_height,
        family,
        weight,
        mono,
        tabular,
        letter_spacing,
        wrap,
        available_width,
    );
    let key = ShapeKey {
        text: text.to_owned(),
        size_bits: size.to_bits(),
        line_height_bits: line_height.to_bits(),
        family,
        weight,
        mono,
        tabular,
        letter_spacing_bits: letter_spacing.to_bits(),
        wrap,
        available_width_bits: available_width.map(f32::to_bits),
    };
    SHAPE_CACHE.with_borrow_mut(|c| {
        if c.len() == SHAPE_CACHE_CAPACITY {
            SHAPE_CACHE_STATS.with_borrow_mut(|stats| {
                stats.evictions += 1;
            });
        }
        c.put(key, layout.clone());
    });
    layout
}

#[allow(clippy::too_many_arguments)]
fn layout_text_uncached(
    text: &str,
    size: f32,
    line_height: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    tabular: bool,
    letter_spacing: f32,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> TextLayout {
    // The mono fallback path below ignores `tabular` (fixed-advance
    // faces are tabular by construction) and `letter_spacing` (the
    // heuristic advance can't model it; code/mono roles never track).
    if !mono
        && let Some(layout) = layout_text_cosmic(
            text,
            size,
            line_height,
            family,
            weight,
            tabular,
            letter_spacing,
            wrap,
            available_width,
        )
    {
        return layout;
    }

    let raw_lines = match (wrap, available_width) {
        (TextWrap::Wrap, Some(width)) => {
            wrap_lines_by_width(text, width, size, family, weight, mono)
        }
        _ => text.split('\n').map(str::to_string).collect(),
    };
    build_layout(raw_lines, size, line_height, family, weight, mono)
}

/// Return a single-line string that fits within `available_width`,
/// appending an ellipsis when truncation is needed.
pub fn ellipsize_text(
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    available_width: f32,
) -> String {
    ellipsize_text_with_family(
        text,
        size,
        FontFamily::default(),
        weight,
        mono,
        false,
        available_width,
    )
}

/// [`ellipsize_text`] with an explicit proportional font family.
#[allow(clippy::too_many_arguments)]
pub fn ellipsize_text_with_family(
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    tabular: bool,
    available_width: f32,
) -> String {
    if available_width <= 0.0 || text.is_empty() {
        return String::new();
    }
    let full = layout_text_with_family(
        text,
        size,
        family,
        weight,
        mono,
        tabular,
        TextWrap::NoWrap,
        None,
    );
    if full.width <= available_width + 0.5 {
        return text.to_string();
    }

    let ellipsis = "…";
    let ellipsis_w = layout_text_with_family(
        ellipsis,
        size,
        family,
        weight,
        mono,
        tabular,
        TextWrap::NoWrap,
        None,
    )
    .width;
    if ellipsis_w > available_width + 0.5 {
        return ellipsis.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect();
        let candidate = format!("{candidate}{ellipsis}");
        let width = layout_text_with_family(
            &candidate,
            size,
            family,
            weight,
            mono,
            tabular,
            TextWrap::NoWrap,
            None,
        )
        .width;
        if width <= available_width + 0.5 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }

    let prefix: String = chars[..lo].iter().collect();
    format!("{prefix}{ellipsis}")
}

/// Return wrapped text capped to `max_lines`, ellipsizing the final
/// visible line when truncation is needed.
pub fn clamp_text_to_lines(
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    available_width: f32,
    max_lines: usize,
) -> String {
    clamp_text_to_lines_with_family(
        text,
        size,
        FontFamily::default(),
        weight,
        mono,
        available_width,
        max_lines,
    )
}

/// [`clamp_text_to_lines`] with an explicit proportional font family.
pub fn clamp_text_to_lines_with_family(
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    available_width: f32,
    max_lines: usize,
) -> String {
    if text.is_empty() || available_width <= 0.0 || max_lines == 0 {
        return String::new();
    }

    let layout = layout_text_with_family(
        text,
        size,
        family,
        weight,
        mono,
        false,
        TextWrap::Wrap,
        Some(available_width),
    );
    if layout.lines.len() <= max_lines {
        return text.to_string();
    }

    let mut lines: Vec<String> = layout
        .lines
        .iter()
        .take(max_lines)
        .map(|line| line.text.clone())
        .collect();
    if let Some(last) = lines.last_mut() {
        let marked = format!("{last}…");
        *last =
            ellipsize_text_with_family(&marked, size, family, weight, mono, false, available_width);
    }
    lines.join("\n")
}

/// Result of a click-to-caret hit-test against a laid-out text run.
/// Coordinates are in byte units within the source text — convertible
/// to character indices via `text.char_indices()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextHit {
    /// Logical line within the source text (zero-based). For a
    /// single-line input always 0; for a wrapped paragraph this is
    /// the visual line index (line breaks introduced by `\n` or by
    /// soft wrapping both bump it).
    pub line: usize,
    /// Byte offset within that logical line's text. Snaps to the
    /// nearest grapheme boundary cosmic-text reports.
    pub byte_index: usize,
}

/// Hit-test a pixel `(x, y)` against the laid-out form of `text` and
/// return the cursor position the click would land at. Coordinates
/// are relative to the layout origin (top-left of the rect that the
/// layout pass would draw the text into). Returns `None` when the
/// point is above/left of the first glyph; cosmic-text's clamping
/// behavior places clicks below the last line at end-of-text.
///
/// Used by text-input widgets: clicking inside the rect produces a
/// caret position by routing the local pointer (pointer minus rect
/// origin) through this function.
pub fn hit_text(
    text: &str,
    size: f32,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
    x: f32,
    y: f32,
) -> Option<TextHit> {
    hit_text_with_family(
        text,
        size,
        FontFamily::default(),
        weight,
        wrap,
        available_width,
        x,
        y,
    )
}

/// [`hit_text`] with an explicit proportional font family.
#[allow(clippy::too_many_arguments)]
pub fn hit_text_with_family(
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
    x: f32,
    y: f32,
) -> Option<TextHit> {
    with_font_system(|font_system| {
        let line_height = line_height(size);
        let mut buffer = Buffer::new(font_system, Metrics::new(size, line_height));
        buffer.set_wrap(match wrap {
            TextWrap::NoWrap => Wrap::None,
            TextWrap::Wrap => Wrap::WordOrGlyph,
        });
        buffer.set_size(
            match wrap {
                TextWrap::NoWrap => None,
                TextWrap::Wrap => available_width,
            },
            None,
        );
        let attrs = Attrs::new()
            .family(Family::Name(family.family_name()))
            .weight(cosmic_weight(weight));
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(font_system, false);
        let cursor = buffer.hit(x, y)?;
        Some(TextHit {
            line: cursor.line,
            byte_index: cursor.index,
        })
    })
}

/// Pixel position of the caret at byte offset `byte_index` in the
/// laid-out form of `text`. Coordinates are relative to the layout
/// origin (top-left of the rect that the layout pass would draw the
/// text into); `(0.0, 0.0)` is the start of the first line.
///
/// Used by multi-line text widgets: the caret bar's `translate()` is
/// the result of this call. See [`hit_text`] for the inverse.
///
/// `byte_index` is interpreted as a byte offset into the source string
/// where `\n` separates buffer lines. Out-of-range or non-boundary
/// indices are clamped to the nearest UTF-8 char boundary.
pub fn caret_xy(
    text: &str,
    byte_index: usize,
    size: f32,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> (f32, f32) {
    caret_xy_with_family(
        text,
        byte_index,
        size,
        FontFamily::default(),
        weight,
        wrap,
        available_width,
    )
}

/// [`caret_xy`] with an explicit proportional font family.
pub fn caret_xy_with_family(
    text: &str,
    byte_index: usize,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> (f32, f32) {
    let (target_line, byte_in_line) = byte_to_line_position(text, byte_index);
    with_font_system(|font_system| {
        let line_h = line_height(size);
        let buffer = build_buffer(
            font_system,
            text,
            size,
            family,
            weight,
            wrap,
            available_width,
        );
        let cursor = Cursor::new(target_line, byte_in_line);
        // cosmic-text's Buffer::cursor_position handles the past-end case
        // (caret after the last glyph on a line) which highlight() omits
        // because zero-width segments are filtered out.
        if let Some((x, y)) = buffer.cursor_position(&cursor) {
            return (x, y);
        }
        // Phantom line beyond the last visible run (e.g. caret right
        // after a trailing `\n`). Position by line index alone.
        (0.0, target_line as f32 * line_h)
    })
}

/// Per-visual-line highlight rectangles for the byte range `lo..hi`.
/// Each rect is `(x, y, width, height)` in layout-origin coordinates;
/// the list is empty when `lo >= hi`.
///
/// Used by multi-line text widgets to paint the selection band: a
/// selection that spans three visual lines yields three rectangles
/// (partial on the first, full on the middle, partial on the last).
pub fn selection_rects(
    text: &str,
    lo: usize,
    hi: usize,
    size: f32,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> Vec<(f32, f32, f32, f32)> {
    selection_rects_with_family(
        text,
        lo,
        hi,
        size,
        FontFamily::default(),
        weight,
        wrap,
        available_width,
    )
}

/// [`selection_rects`] with an explicit proportional font family.
#[allow(clippy::too_many_arguments)]
pub fn selection_rects_with_family(
    text: &str,
    lo: usize,
    hi: usize,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> Vec<(f32, f32, f32, f32)> {
    if lo >= hi {
        return Vec::new();
    }
    let (lo_line, lo_in_line) = byte_to_line_position(text, lo);
    let (hi_line, hi_in_line) = byte_to_line_position(text, hi);
    with_font_system(|font_system| {
        let buffer = build_buffer(
            font_system,
            text,
            size,
            family,
            weight,
            wrap,
            available_width,
        );
        let c_lo = Cursor::new(lo_line, lo_in_line);
        let c_hi = Cursor::new(hi_line, hi_in_line);
        let mut rects = Vec::new();
        for run in buffer.layout_runs() {
            if run.line_i < lo_line || run.line_i > hi_line {
                continue;
            }
            for (x, w) in run.highlight(c_lo, c_hi) {
                rects.push((x, run.line_top, w, run.line_height));
            }
        }
        rects
    })
}

/// Global byte range (`start..end` offsets into `text`) of the visual
/// line containing `byte_index`. With `wrap == TextWrap::Wrap` the
/// range respects soft wrap points, so it can be narrower than the
/// hard (`\n`-delimited) line; used by Home/End and line-wise cursor
/// movement in text widgets.
pub fn visual_line_byte_range(
    text: &str,
    byte_index: usize,
    size: f32,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> (usize, usize) {
    visual_line_byte_range_with_family(
        text,
        byte_index,
        size,
        FontFamily::default(),
        weight,
        wrap,
        available_width,
    )
}

/// [`visual_line_byte_range`] with an explicit proportional font
/// family.
pub fn visual_line_byte_range_with_family(
    text: &str,
    byte_index: usize,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> (usize, usize) {
    let byte_index = clamp_to_char_boundary(text, byte_index.min(text.len()));
    let (target_line, byte_in_line) = byte_to_line_position(text, byte_index);
    let hard_line_start = line_position_to_byte(text, target_line, 0);
    let hard_line_end = line_end_byte(text, hard_line_start);
    with_font_system(|font_system| {
        let buffer = build_buffer(
            font_system,
            text,
            size,
            family,
            weight,
            wrap,
            available_width,
        );
        let mut last_range = None;
        for run in buffer.layout_runs() {
            if run.line_i != target_line {
                continue;
            }
            let Some((start, end)) = layout_run_byte_range(&run) else {
                continue;
            };
            last_range = Some((start, end));
            if start <= byte_in_line && byte_in_line < end {
                return (
                    line_position_to_byte(text, target_line, start),
                    line_position_to_byte(text, target_line, end),
                );
            }
        }
        if let Some((start, end)) = last_range
            && byte_index >= line_position_to_byte(text, target_line, start)
        {
            return (
                line_position_to_byte(text, target_line, start),
                line_position_to_byte(text, target_line, end),
            );
        }
        (hard_line_start, hard_line_end)
    })
}

/// Convert a global byte offset in `text` to the (BufferLine index,
/// byte-in-line) pair that cosmic-text uses for cursors. `\n`
/// characters are *not* part of any line — they just bump the line
/// counter.
fn byte_to_line_position(text: &str, byte_index: usize) -> (usize, usize) {
    let byte_index = byte_index.min(text.len());
    let mut line = 0;
    let mut line_start = 0;
    for (i, ch) in text.char_indices() {
        if i >= byte_index {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + ch.len_utf8();
        }
    }
    (line, byte_index - line_start)
}

fn line_position_to_byte(text: &str, line: usize, byte_in_line: usize) -> usize {
    let mut current_line = 0;
    let mut line_start = 0;
    for (i, ch) in text.char_indices() {
        if current_line == line {
            let candidate = line_start + byte_in_line;
            return clamp_to_char_boundary(text, candidate.min(text.len()));
        }
        if ch == '\n' {
            current_line += 1;
            line_start = i + ch.len_utf8();
        }
    }
    if current_line == line {
        clamp_to_char_boundary(text, (line_start + byte_in_line).min(text.len()))
    } else {
        text.len()
    }
}

fn line_end_byte(text: &str, line_start: usize) -> usize {
    text[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(text.len())
}

fn layout_run_byte_range(run: &cosmic_text::LayoutRun<'_>) -> Option<(usize, usize)> {
    let start = run.glyphs.iter().map(|glyph| glyph.start).min()?;
    let end = run
        .glyphs
        .iter()
        .map(|glyph| glyph.end)
        .max()
        .unwrap_or(start);
    Some((start, end))
}

fn clamp_to_char_boundary(text: &str, mut byte: usize) -> usize {
    byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

fn build_buffer(
    font_system: &mut FontSystem,
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> Buffer {
    let line_h = line_height(size);
    let mut buffer = Buffer::new(font_system, Metrics::new(size, line_h));
    buffer.set_wrap(match wrap {
        TextWrap::NoWrap => Wrap::None,
        TextWrap::Wrap => Wrap::WordOrGlyph,
    });
    buffer.set_size(
        match wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => available_width,
        },
        None,
    );
    let attrs = Attrs::new()
        .family(Family::Name(family.family_name()))
        .weight(cosmic_weight(weight));
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

/// Word-wrap text into lines whose measured width stays within
/// `max_width` whenever possible. Explicit newlines always split
/// paragraphs. Oversized words are split by character.
pub fn wrap_lines(
    text: &str,
    max_width: f32,
    size: f32,
    weight: FontWeight,
    mono: bool,
) -> Vec<String> {
    wrap_lines_with_family(text, max_width, size, FontFamily::default(), weight, mono)
}

/// [`wrap_lines`] with an explicit proportional font family.
pub fn wrap_lines_with_family(
    text: &str,
    max_width: f32,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
) -> Vec<String> {
    if !mono
        && let Some(layout) = layout_text_cosmic(
            text,
            size,
            line_height(size),
            family,
            weight,
            false,
            0.0,
            TextWrap::Wrap,
            Some(max_width),
        )
    {
        return layout.lines.into_iter().map(|line| line.text).collect();
    }
    wrap_lines_by_width(text, max_width, size, family, weight, mono)
}

fn wrap_lines_by_width(
    text: &str,
    max_width: f32,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
) -> Vec<String> {
    if max_width <= 0.0 {
        return vec![String::new()];
    }

    let ctx = WrapMeasure {
        max_width,
        size,
        family,
        weight,
        mono,
    };
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }

        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                push_word_wrapped(&mut out, &mut line, word, ctx);
                continue;
            }

            let candidate = format!("{line} {word}");
            if line_width_with_family(&candidate, size, family, weight, mono) <= max_width {
                line = candidate;
            } else {
                out.push(std::mem::take(&mut line));
                push_word_wrapped(&mut out, &mut line, word, ctx);
            }
        }

        if !line.is_empty() {
            out.push(line);
        }
    }

    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Measure one single-line string. Newline characters are ignored; use
/// [`measure_text`] for multi-line text.
pub fn line_width(text: &str, size: f32, weight: FontWeight, mono: bool) -> f32 {
    line_width_with_family(text, size, FontFamily::default(), weight, mono)
}

/// [`line_width`] with an explicit proportional font family.
pub fn line_width_with_family(
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
) -> f32 {
    if !mono
        && let Some(layout) = layout_text_cosmic(
            text,
            size,
            line_height(size),
            family,
            weight,
            false,
            0.0,
            TextWrap::NoWrap,
            None,
        )
    {
        return layout.width;
    }
    line_width_by_ttf(text, size, family, weight, mono)
}

fn line_width_by_ttf(
    text: &str,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
) -> f32 {
    if mono {
        return text
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .map(|c| if c == '\t' { 4.0 } else { 1.0 })
            .sum::<f32>()
            * size
            * MONO_CHAR_WIDTH_FACTOR;
    }

    let Ok(face) = ttf_parser::Face::parse(font_bytes(family, weight), 0) else {
        return fallback_line_width(text, size, mono);
    };
    let scale = size / face.units_per_em() as f32;
    let fallback_advance = face.units_per_em() as f32 * 0.5;
    let mut width = 0.0;
    let mut prev = None;

    for c in text.chars() {
        if c == '\n' || c == '\r' {
            continue;
        }
        if c == '\t' {
            width += line_width_with_family("    ", size, family, weight, mono);
            prev = None;
            continue;
        }

        let Some(glyph) = glyph_for(&face, c) else {
            continue;
        };
        if let Some(left) = prev {
            width += kern(&face, left, glyph) * scale;
        }
        width += face
            .glyph_hor_advance(glyph)
            .map(|advance| advance as f32)
            .unwrap_or(fallback_advance)
            * scale;
        prev = Some(glyph);
    }
    width
}

/// Default line height (logical px) for a font size (logical px).
pub fn line_height(size: f32) -> f32 {
    // Styled elements carry an explicit `line_height`; this fallback is
    // for raw measurement callers and custom `.font_size(...)` values.
    // Known design-token sizes return their paired Tailwind/shadcn line
    // height, while arbitrary sizes keep a snapped multiplier.
    tokens::line_height_for_size(size)
}

fn build_layout(
    lines: Vec<String>,
    size: f32,
    line_height: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
) -> TextLayout {
    let raw_lines = if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    };
    let lines: Vec<TextLine> = raw_lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let y = i as f32 * line_height;
            TextLine {
                width: line_width_with_family(&text, size, family, weight, mono),
                text,
                y,
                baseline: y + size * BASELINE_MULTIPLIER,
                rtl: false,
            }
        })
        .collect();
    let width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    TextLayout {
        width,
        height: lines.len().max(1) as f32 * line_height,
        line_height,
        lines,
    }
}

/// The OpenType `tnum` (tabular figures) feature set used when a run
/// requests tabular numerals. Honoured by fonts carrying the feature
/// (the bundled Inter does); a no-op otherwise.
pub(crate) fn tabular_features() -> FontFeatures {
    let mut features = FontFeatures::new();
    features.set(FeatureTag::new(b"tnum"), 1);
    features
}

#[allow(clippy::too_many_arguments)]
fn layout_text_cosmic(
    text: &str,
    size: f32,
    line_height: f32,
    family: FontFamily,
    weight: FontWeight,
    tabular: bool,
    letter_spacing: f32,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> Option<TextLayout> {
    let options = CosmicLayoutOptions {
        size,
        line_height,
        family,
        weight,
        tabular,
        letter_spacing,
        wrap,
        available_width,
    };
    with_font_system(|font_system| layout_text_cosmic_with(font_system, text, options))
}

#[derive(Copy, Clone)]
struct CosmicLayoutOptions {
    size: f32,
    line_height: f32,
    family: FontFamily,
    weight: FontWeight,
    tabular: bool,
    letter_spacing: f32,
    wrap: TextWrap,
    available_width: Option<f32>,
}

fn layout_text_cosmic_with(
    font_system: &mut FontSystem,
    text: &str,
    options: CosmicLayoutOptions,
) -> Option<TextLayout> {
    let CosmicLayoutOptions {
        size,
        line_height,
        family,
        weight,
        tabular,
        letter_spacing,
        wrap,
        available_width,
    } = options;
    let mut buffer = Buffer::new(font_system, Metrics::new(size, line_height));
    buffer.set_wrap(match wrap {
        TextWrap::NoWrap => Wrap::None,
        TextWrap::Wrap => Wrap::WordOrGlyph,
    });
    buffer.set_size(
        match wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => available_width,
        },
        None,
    );
    let mut attrs = Attrs::new()
        .family(Family::Name(family.family_name()))
        .weight(cosmic_weight(weight));
    if tabular {
        attrs = attrs.font_features(tabular_features());
    }
    if letter_spacing != 0.0 {
        // cosmic-text's LetterSpacing is in EM units (added to the
        // em-normalized advance before the font-size multiply); our
        // param is CSS-like px.
        attrs = attrs.letter_spacing(letter_spacing / size);
    }
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let mut lines = Vec::new();
    let mut height: f32 = 0.0;
    for run in buffer.layout_runs() {
        height = height.max(run.line_top + run.line_height);
        lines.push(TextLine {
            text: layout_run_text(&run),
            width: run.line_w,
            y: run.line_top,
            baseline: run.line_y,
            rtl: run.rtl,
        });
    }

    if lines.is_empty() {
        return None;
    }

    let width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    Some(TextLayout {
        lines,
        width,
        height: height.max(line_height),
        line_height,
    })
}

// `FontSystem` construction loads the bundled UI faces and builds a
// fontdb. Doing it per text-shape call burned ~22ms in the layout pass
// on the wasm showcase — basically all of it. Cache once per thread;
// cosmic-text's internal shape cache also accumulates across calls now,
// which is the side benefit.
thread_local! {
    static FONT_SYSTEM: RefCell<FontSystem> = RefCell::new(bundled_font_system());
    /// How many [`crate::text::registry`] fonts this thread's
    /// `FontSystem` has loaded; compared against the registry's count
    /// on every shape so host-registered fonts reach measurement too.
    static FONT_SYSTEM_REGISTRY_LOADED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// All measurement-side `FontSystem` access funnels through here: sync
/// any newly host-registered fonts into this thread's database first
/// (issue #56 — paint and measurement must agree on fallback
/// coverage), invalidating the shape cache when coverage changed.
fn with_font_system<R>(f: impl FnOnce(&mut FontSystem) -> R) -> R {
    FONT_SYSTEM.with_borrow_mut(|font_system| {
        FONT_SYSTEM_REGISTRY_LOADED.with(|loaded| {
            let mut n = loaded.get();
            if crate::text::registry::sync_font_system(font_system, &mut n) {
                SHAPE_CACHE.with_borrow_mut(|cache| cache.clear());
            }
            loaded.set(n);
        });
        f(font_system)
    })
}

/// Number of faces in this thread's measurement `FontSystem`, after a
/// registry sync. Test-only observability for the issue-#56 contract
/// that host-registered fonts reach measurement.
#[cfg(test)]
pub(crate) fn font_system_face_count_for_tests() -> usize {
    with_font_system(|font_system| font_system.db().len())
}

/// Cache key for [`layout_text_with_line_height_and_family`]. Captures
/// every input that influences the produced [`TextLayout`]; floats are
/// stored as `to_bits` so they participate in `Hash` / `Eq` directly.
/// Same shape on every entry — no enum variant hidden behind a `Box`,
/// since the lookup happens once per text node per frame and we want
/// the fast path to be a single hash.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeKey {
    /// `String` (not `Box<str>`) so the thread-local scratch key can
    /// reuse its capacity across lookups — hits allocate nothing.
    text: String,
    size_bits: u32,
    line_height_bits: u32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
    tabular: bool,
    letter_spacing_bits: u32,
    wrap: TextWrap,
    available_width_bits: Option<u32>,
}

/// Bounded thread-local LRU of shaped layouts. UI chrome only needs a
/// few dozen entries, but markdown-heavy conversation views can lower
/// one frame into several thousand distinct text/style/width keys.
/// Keep enough room for those views to stay warm across repeated
/// layout probes; eviction is benign but expensive because the next
/// miss reshapes via cosmic.
const SHAPE_CACHE_CAPACITY: usize = 16_384;
thread_local! {
    static SHAPE_CACHE: RefCell<LruCache<ShapeKey, TextLayout>> =
        RefCell::new(LruCache::new(NonZeroUsize::new(SHAPE_CACHE_CAPACITY).unwrap()));
    static SHAPE_CACHE_STATS: RefCell<TextLayoutCacheStats> =
        RefCell::new(TextLayoutCacheStats::default());
    /// Reused probe key for [`SHAPE_CACHE`] lookups — see
    /// `layout_text_with_line_height_and_family`.
    static SHAPE_KEY_SCRATCH: RefCell<ShapeKey> = RefCell::new(ShapeKey {
        text: String::new(),
        size_bits: 0,
        line_height_bits: 0,
        family: FontFamily::default(),
        weight: FontWeight::Regular,
        mono: false,
        tabular: false,
        letter_spacing_bits: 0,
        wrap: TextWrap::NoWrap,
        available_width_bits: None,
    });
}

/// Drain layout-side text cache counters accumulated since the previous
/// call. Runners call this once per full prepare so diagnostic overlays
/// can distinguish cache churn from paint-side work.
pub fn take_shape_cache_stats() -> TextLayoutCacheStats {
    SHAPE_CACHE_STATS.with_borrow_mut(std::mem::take)
}

fn bundled_font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.set_sans_serif_family(FontFamily::default().family_name());
    for bytes in damascene_fonts::DEFAULT_FONTS {
        db.load_font_data(bytes.to_vec());
    }
    FontSystem::new_with_locale_and_db("en-US".to_string(), db)
}

fn cosmic_weight(weight: FontWeight) -> Weight {
    match weight {
        FontWeight::Regular => Weight::NORMAL,
        FontWeight::Medium => Weight::MEDIUM,
        FontWeight::Semibold => Weight::SEMIBOLD,
        FontWeight::Bold => Weight::BOLD,
    }
}

fn layout_run_text(run: &cosmic_text::LayoutRun<'_>) -> String {
    let Some(start) = run.glyphs.iter().map(|glyph| glyph.start).min() else {
        return String::new();
    };
    let end = run
        .glyphs
        .iter()
        .map(|glyph| glyph.end)
        .max()
        .unwrap_or(start);
    run.text
        .get(start..end)
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

#[derive(Copy, Clone)]
struct WrapMeasure {
    max_width: f32,
    size: f32,
    family: FontFamily,
    weight: FontWeight,
    mono: bool,
}

fn push_word_wrapped(out: &mut Vec<String>, line: &mut String, word: &str, ctx: WrapMeasure) {
    let WrapMeasure {
        max_width,
        size,
        family,
        weight,
        mono,
    } = ctx;
    if line_width_with_family(word, size, family, weight, mono) <= max_width {
        line.push_str(word);
        return;
    }

    for ch in word.chars() {
        let candidate = format!("{line}{ch}");
        if !line.is_empty()
            && line_width_with_family(&candidate, size, family, weight, mono) > max_width
        {
            out.push(std::mem::take(line));
        }
        line.push(ch);
    }
}

fn glyph_for(face: &ttf_parser::Face<'_>, c: char) -> Option<ttf_parser::GlyphId> {
    face.glyph_index(c)
        .or_else(|| face.glyph_index('\u{FFFD}'))
        .or_else(|| face.glyph_index('?'))
        .or_else(|| face.glyph_index(' '))
}

fn kern(face: &ttf_parser::Face<'_>, left: ttf_parser::GlyphId, right: ttf_parser::GlyphId) -> f32 {
    let Some(kern) = &face.tables().kern else {
        return 0.0;
    };
    kern.subtables
        .into_iter()
        .filter(|subtable| subtable.horizontal && !subtable.has_cross_stream)
        .find_map(|subtable| subtable.glyphs_kerning(left, right))
        .map(|value| value as f32)
        .unwrap_or(0.0)
}

fn font_bytes(family: FontFamily, weight: FontWeight) -> &'static [u8] {
    // ttf-parser fallback path (used only when cosmic-text is bypassed
    // for monospace measurement, etc.). Sourced from damascene-fonts so we
    // share one bundle with the cosmic-text path.
    match family {
        FontFamily::Inter => {
            #[cfg(feature = "inter")]
            {
                let _ = weight;
                damascene_fonts::INTER_VARIABLE
            }
            #[cfg(not(feature = "inter"))]
            {
                let _ = weight;
                &[]
            }
        }
        FontFamily::Roboto => {
            #[cfg(feature = "roboto")]
            {
                match weight {
                    FontWeight::Regular => damascene_fonts::ROBOTO_REGULAR,
                    FontWeight::Medium => damascene_fonts::ROBOTO_MEDIUM,
                    FontWeight::Semibold | FontWeight::Bold => damascene_fonts::ROBOTO_BOLD,
                }
            }
            #[cfg(not(feature = "roboto"))]
            {
                let _ = weight;
                &[]
            }
        }
        FontFamily::JetBrainsMono => {
            #[cfg(feature = "jetbrains-mono")]
            {
                let _ = weight;
                damascene_fonts::JETBRAINS_MONO_VARIABLE
            }
            #[cfg(not(feature = "jetbrains-mono"))]
            {
                let _ = weight;
                &[]
            }
        }
    }
}

fn fallback_line_width(text: &str, size: f32, mono: bool) -> f32 {
    let char_w = size * if mono { MONO_CHAR_WIDTH_FACTOR } else { 0.60 };
    text.chars().count() as f32 * char_w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_measurement_distinguishes_narrow_and_wide_glyphs() {
        let narrow = line_width("iiiiii", 16.0, FontWeight::Regular, false);
        let wide = line_width("WWWWWW", 16.0, FontWeight::Regular, false);

        assert!(wide > narrow * 2.0, "wide={wide} narrow={narrow}");
    }

    #[test]
    fn letter_spacing_changes_shaped_advances() {
        // Negative tracking (shadcn tracking-tight on headings) must
        // reach the shaper: the tightened line measures narrower, and
        // the delta scales with the glyph count.
        let natural = layout_text_with_line_height_and_family(
            "Grumpy wizards make toxic brew",
            24.0,
            32.0,
            FontFamily::default(),
            FontWeight::Semibold,
            false,
            false,
            0.0,
            TextWrap::NoWrap,
            None,
        );
        let tight = layout_text_with_line_height_and_family(
            "Grumpy wizards make toxic brew",
            24.0,
            32.0,
            FontFamily::default(),
            FontWeight::Semibold,
            false,
            false,
            crate::tokens::TRACKING_TIGHT_EM * 24.0,
            TextWrap::NoWrap,
            None,
        );
        // 30 glyphs × 0.6 px ≈ 18 px tighter. A unit mix-up (cosmic's
        // LetterSpacing is em-scaled, not px) blows far past this band
        // — the original bug crushed the line by hundreds of px.
        let delta = natural.width - tight.width;
        assert!(
            (8.0..30.0).contains(&delta),
            "tracking-tight must tighten by ~0.6px/glyph: natural {} vs tight {} (delta {delta})",
            natural.width,
            tight.width
        );
    }

    #[test]
    fn tabular_numerals_equalize_digit_advances() {
        let width = |text: &str, tabular: bool| {
            layout_text_with_family(
                text,
                16.0,
                FontFamily::Inter,
                FontWeight::Regular,
                false,
                tabular,
                TextWrap::NoWrap,
                None,
            )
            .width
        };
        // With tnum, every digit takes the same advance — the
        // narrowest and widest digit strings measure identically.
        let one = width("11111111", true);
        let eight = width("88888888", true);
        assert!(
            (one - eight).abs() < 0.01,
            "tabular digits must align: 1s={one} 8s={eight}"
        );
        // The flag participates in the shape cache key: the same
        // string measures differently with and without it (Inter's
        // default figures are proportional — '1' is narrower).
        let one_prop = width("11111111", false);
        assert!(
            one > one_prop + 0.5,
            "tabular '1' should be wider than proportional: tnum={one} default={one_prop}"
        );
    }

    #[cfg(feature = "roboto")]
    #[test]
    fn font_family_changes_proportional_measurement() {
        let roboto = line_width_with_family(
            "Save changes",
            14.0,
            FontFamily::Roboto,
            FontWeight::Semibold,
            false,
        );
        let inter = line_width_with_family(
            "Save changes",
            14.0,
            FontFamily::Inter,
            FontWeight::Semibold,
            false,
        );

        assert!(
            (inter - roboto).abs() > 1.0,
            "inter={inter} roboto={roboto}"
        );
    }

    #[test]
    fn wrap_lines_respects_measured_widths() {
        let lines = wrap_lines(
            "wide WWW words stay measured",
            120.0,
            16.0,
            FontWeight::Regular,
            false,
        );

        assert!(lines.len() > 1);
        for line in lines {
            assert!(
                line_width(&line, 16.0, FontWeight::Regular, false) <= 121.0,
                "{line:?} overflowed"
            );
        }
    }

    #[test]
    fn layout_text_carries_line_positions_and_measurement() {
        let layout = layout_text(
            "alpha beta gamma",
            16.0,
            FontWeight::Regular,
            false,
            TextWrap::Wrap,
            Some(80.0),
        );

        assert!(layout.lines.len() > 1);
        assert_eq!(layout.measured().line_count, layout.lines.len());
        assert_eq!(layout.lines[0].y, 0.0);
        assert_eq!(layout.lines[1].y, layout.line_height);
        assert!(layout.lines[0].baseline > layout.lines[0].y);
        assert!(layout.height >= layout.line_height * 2.0);
    }

    #[test]
    fn tokenized_line_heights_match_shadcn_scale() {
        assert_eq!(line_height(12.0), 16.0);
        assert_eq!(line_height(14.0), 20.0);
        assert_eq!(line_height(16.0), 24.0);
        assert_eq!(line_height(24.0), 32.0);
        assert_eq!(line_height(30.0), 36.0);
    }

    #[test]
    fn hit_text_at_origin_lands_on_first_byte() {
        let hit = hit_text(
            "hello world",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
            0.0,
            8.0,
        )
        .expect("hit at origin");
        assert_eq!(hit.line, 0);
        assert_eq!(hit.byte_index, 0);
    }

    #[test]
    fn hit_text_past_last_glyph_clamps_to_end() {
        let text = "hello";
        // y=8 lands inside the line; a huge x clamps to end-of-line.
        let hit = hit_text(
            text,
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
            1000.0,
            8.0,
        )
        .expect("hit past end");
        assert_eq!(hit.line, 0);
        assert_eq!(hit.byte_index, text.len());
    }

    #[test]
    fn hit_text_walks_columns_left_to_right() {
        // Successive x positions inside the same line should produce
        // monotonically non-decreasing byte indices — the basic contract
        // a text input relies on for click-to-caret.
        let text = "abcdefghij";
        let mut prev = 0usize;
        for x in [4.0, 16.0, 32.0, 64.0, 96.0] {
            let hit = hit_text(
                text,
                16.0,
                FontWeight::Regular,
                TextWrap::NoWrap,
                None,
                x,
                8.0,
            );
            let Some(hit) = hit else { continue };
            assert!(
                hit.byte_index >= prev,
                "byte_index regressed at x={x}: {} < {prev}",
                hit.byte_index
            );
            prev = hit.byte_index;
        }
    }

    #[test]
    fn text_geometry_hit_byte_maps_hard_line_offsets_to_source_bytes() {
        let text = "alpha\nbeta";
        let geometry = TextGeometry::new(
            text,
            16.0,
            FontWeight::Regular,
            false,
            TextWrap::NoWrap,
            None,
        );
        let y = geometry.line_height() * 1.5;
        let byte = geometry.hit_byte(1000.0, y).expect("hit on second line");
        assert_eq!(byte, text.len());
    }

    #[test]
    fn text_geometry_prefix_width_matches_caret_x() {
        let text = "hello world";
        let geometry = TextGeometry::new(
            text,
            16.0,
            FontWeight::Regular,
            false,
            TextWrap::NoWrap,
            None,
        );
        let (x, _y) = geometry.caret_xy(5);
        assert!((geometry.prefix_width(5) - x).abs() < 0.01);
    }

    #[test]
    fn caret_xy_at_origin_is_zero_zero() {
        let (x, y) = caret_xy(
            "hello",
            0,
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
        );
        assert!(x.abs() < 0.01, "x={x}");
        assert_eq!(y, 0.0);
    }

    #[test]
    fn caret_xy_at_end_of_line_is_at_line_width() {
        let text = "hello";
        let width = line_width(text, 16.0, FontWeight::Regular, false);
        let (x, y) = caret_xy(
            text,
            text.len(),
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
        );
        assert!((x - width).abs() < 1.0, "x={x} expected~{width}");
        assert_eq!(y, 0.0);
    }

    #[test]
    fn caret_xy_drops_to_next_line_after_newline() {
        let text = "foo\nbar";
        let line_h = line_height(16.0);
        // Right after the \n: should land at start of line 1.
        let (x, y) = caret_xy(text, 4, 16.0, FontWeight::Regular, TextWrap::NoWrap, None);
        assert!(x.abs() < 0.01, "x={x}");
        assert!((y - line_h).abs() < 0.01, "y={y} expected~{line_h}");
    }

    #[test]
    fn caret_xy_on_phantom_trailing_line_falls_below_text() {
        let text = "foo\n";
        let line_h = line_height(16.0);
        let (x, y) = caret_xy(
            text,
            text.len(),
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
        );
        assert!(x.abs() < 0.01, "x={x}");
        assert!(y >= line_h - 0.01, "y={y} expected ≥ line_h={line_h}");
    }

    #[test]
    fn selection_rects_returns_one_per_visual_line() {
        let text = "alpha\nbeta\ngamma";
        let rects = selection_rects(
            text,
            0,
            text.len(),
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
        );
        assert_eq!(
            rects.len(),
            3,
            "expected one rect per BufferLine, got {rects:?}"
        );
        // Rects are ordered top-down.
        assert!(rects[0].1 < rects[1].1);
        assert!(rects[1].1 < rects[2].1);
        for (_x, _y, w, _h) in &rects {
            assert!(*w > 0.0, "empty width: {rects:?}");
        }
    }

    #[test]
    fn selection_rects_for_single_line_range_do_not_highlight_other_lines() {
        let text = "alpha\nbeta\ngamma";
        let lo = text.find("et").unwrap();
        let hi = lo + "et".len();
        let rects = selection_rects(
            text,
            lo,
            hi,
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
        );
        assert_eq!(
            rects.len(),
            1,
            "single-line range should only highlight that line: {rects:?}"
        );
        let line_h = line_height(16.0);
        let y = rects[0].1;
        assert!(
            (y - line_h).abs() < 0.01,
            "expected second line y={line_h}, got {y}; rects={rects:?}"
        );
    }

    #[test]
    fn visual_line_byte_range_respects_soft_wraps() {
        let text = "alpha beta gamma";
        let beta = text.find("beta").unwrap();
        let width = line_width("alpha", 16.0, FontWeight::Regular, false) + 2.0;
        let (lo, hi) = visual_line_byte_range(
            text,
            beta,
            16.0,
            FontWeight::Regular,
            TextWrap::Wrap,
            Some(width),
        );
        assert!(
            lo > 0 && hi < text.len(),
            "soft-wrapped visual line should be narrower than the hard line: {lo}..{hi}"
        );
        assert!(
            (lo..hi).contains(&beta),
            "range {lo}..{hi} should contain beta byte {beta}"
        );
    }

    #[test]
    fn selection_rects_empty_for_collapsed_range() {
        let rects = selection_rects(
            "alpha",
            2,
            2,
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            None,
        );
        assert!(rects.is_empty());
    }

    #[test]
    fn proportional_layout_uses_cosmic_shaping_widths() {
        let layout = layout_text(
            "Roboto shaping",
            18.0,
            FontWeight::Medium,
            false,
            TextWrap::NoWrap,
            None,
        );

        assert_eq!(layout.lines.len(), 1);
        assert!((layout.lines[0].width - layout.width).abs() < 0.01);
        assert!(layout.lines[0].baseline > layout.lines[0].y);
    }

    #[test]
    fn ellipsize_text_shortens_to_available_width() {
        let source = "this is a long branch name";
        let available = line_width("this is a…", 14.0, FontWeight::Regular, false);
        let clipped = ellipsize_text(source, 14.0, FontWeight::Regular, false, available);
        let width = line_width(&clipped, 14.0, FontWeight::Regular, false);

        assert!(clipped.ends_with('…'), "clipped={clipped}");
        assert!(clipped.len() < source.len());
        assert!(
            width <= available + 0.5,
            "width={width} available={available}"
        );
    }

    #[test]
    fn ellipsize_text_keeps_fitting_text_unchanged() {
        let source = "short";
        let available = line_width(source, 14.0, FontWeight::Regular, false) + 4.0;
        assert_eq!(
            ellipsize_text(source, 14.0, FontWeight::Regular, false, available),
            source
        );
    }

    #[test]
    fn clamp_text_to_lines_caps_wrapped_text_with_final_ellipsis() {
        let source = "alpha beta gamma delta epsilon zeta";
        let available = line_width("alpha beta", 14.0, FontWeight::Regular, false);
        let clamped = clamp_text_to_lines(source, 14.0, FontWeight::Regular, false, available, 2);
        let layout = layout_text(
            &clamped,
            14.0,
            FontWeight::Regular,
            false,
            TextWrap::Wrap,
            Some(available),
        );

        assert!(clamped.ends_with('…'), "clamped={clamped}");
        assert!(layout.lines.len() <= 2, "layout={layout:?}");
    }
}
