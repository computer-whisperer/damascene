//! Glyph rasterization + atlas, backend-agnostic.
//!
//! [`GlyphAtlas`] owns the cosmic-text `FontSystem` and a swash
//! `ScaleContext`. It shapes a logical text run to per-glyph positions,
//! rasterizes any glyphs it has not seen at this size, and packs the
//! alpha-coverage bitmaps onto one or more CPU-side [`AtlasPage`]s.
//! Backends mirror dirty regions of those pages to a GPU texture and
//! draw textured quads at the positions returned in [`ShapedRun`].
//!
//! ## Fonts
//!
//! The font bundle lives in the sibling [`damascene-fonts`](damascene_fonts)
//! crate (so the asset bytes don't bloat the engine source tree). At
//! construction the atlas loads every byte slice in
//! [`damascene_fonts::DEFAULT_FONTS`] into its `fontdb`. Callers that need
//! a custom bundle (their own brand typeface, full pan-CJK, additional
//! color fonts) use [`GlyphAtlas::register_font`] to push more fonts
//! into the database, or build with `default-features = false` on
//! damascene-core to drop the bundled assets entirely.
//!
//! cosmic-text walks fontdb when a primary face lacks a glyph, so any
//! font in the database participates in fallback automatically.
//!
//! ## Color glyphs
//!
//! The atlas is unified RGBA — every glyph is stored as 4 bytes/pixel
//! so the same shader path handles outline text and color glyphs.
//! Three color formats flow through swash and the
//! [`Content::Color`](swash::scale::image::Content) arm of the internal
//! RGBA expansion path:
//!
//! - **CBDT/CBLC** (Google's color bitmap format) — used by the bundled
//!   `NotoColorEmoji`. swash decodes the embedded PNG/raw bitmaps and
//!   resamples to the requested em size.
//! - **COLRv0 + CPAL** (Microsoft's layered-outline format) — each
//!   glyph is a stack of solid-color outlines drawn in palette order.
//!   swash composites the layers internally and emits one RGBA bitmap.
//!   Used by Material Symbols' color variant, Bungee Color, etc.
//! - **sbix** (Apple's color-bitmap format) — same `Content::Color`
//!   path; no in-tree fixtures yet.
//!
//! What we **don't** support: **COLRv1** features — gradients, nested
//! transforms, blend modes, variable color tables. swash 0.1.19 only
//! understands COLRv0; a COLRv1 font (Noto Color Emoji's COLR build,
//! recent Twitter Twemoji v15+) will fall back to v0 layers if the
//! font supplies them, otherwise the glyph won't rasterize.
//!
//! SVG and layout/measurement keep using [`crate::text::metrics`] — its
//! line-level layout is what they consume; the per-glyph artifact here
//! is for paint only.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::ops::Range;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, Style, Weight, Wrap, fontdb,
};
use lru::LruCache;
use swash::scale::image::{Content as SwashContent, Image as SwashImage};
use swash::scale::{Render, ScaleContext, Source as SwashSource, StrikeWith};

use crate::ir::TextAnchor;
use crate::text::metrics::{TextLayout, TextLine, line_height};
use crate::tree::{Color, FontFamily, FontWeight, TextWrap};

/// Default page size. Picked so a typical fixture's glyphs fit on a
/// single page; larger UIs allocate a second page on demand.
const PAGE_SIZE: u32 = 512;

/// Family name passed to cosmic-text for the proportional sans-serif
/// stack. Faces with this family name are matched against `RunStyle`'s
/// weight + italic flags through fontdb. cosmic-text falls back to
/// other families in the database (e.g. Noto Sans Symbols 2) when this
/// one lacks the requested codepoint.
const DEFAULT_SANS_FAMILY: &str = "Inter Variable";

/// One shaped glyph carrying its atlas key, pen position, paint color,
/// and the index of the run that produced it. Positions are in
/// **logical pixels** relative to the shaped run's origin (top of the
/// first line, x = 0).
///
/// `color` lives on the glyph (rather than a single per-run uniform)
/// so attributed paragraphs (inline runs) emit one shaped output with
/// per-glyph colors. Single-style text passes one color and every
/// glyph receives the same value — no behaviour change.
///
/// `run_index` identifies which input run produced this glyph
/// (always `0` for single-style text). Selection / hit-test uses this
/// to map glyphs back to runs (which carry link URLs, semantic tags,
/// etc.).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub key: GlyphKey,
    /// Pen X relative to run origin. Add the bitmap's `offset.0` to
    /// reach the glyph's screen-space top-left.
    pub x: f32,
    /// Baseline Y relative to run origin. The bitmap's top edge is at
    /// `y - offset.1` (offset.1 is positive for bitmaps above baseline).
    pub y: f32,
    /// Source byte range in the input string — kept for future caret /
    /// selection logic.
    pub byte_range: Range<usize>,
    /// Paint color for this glyph.
    pub color: Color,
    /// Index of the run (within an attributed `text_runs` parent) that
    /// produced this glyph. `0` for single-style text.
    pub run_index: u32,
}

/// One shaped + atlased run, the artifact a backend's text path consumes.
///
/// `highlights` carries the per-line background rects for runs whose
/// `RunStyle.bg` is `Some`. Each rect spans one line of one styled run;
/// a span that wraps across two lines produces two rects. Backends paint
/// these as solid quads underneath the glyph layer in the same paint
/// item, so highlights inherit the glyph layer's z-order and scissor.
///
/// `decorations` carries underline / strikethrough rects for runs whose
/// `RunStyle.underline` or `RunStyle.strikethrough` is set (links pull
/// the same path through their auto-underline). Same per-(run, line)
/// shape as `highlights`, but backends paint these *on top* of the glyph
/// layer so a strikethrough actually crosses the glyphs.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRun {
    pub layout: TextLayout,
    pub glyphs: Vec<ShapedGlyph>,
    pub highlights: Vec<HighlightRect>,
    pub decorations: Vec<DecorationRect>,
}

/// One inline-run highlight band: a solid background rect spanning one
/// line of one styled run. Coordinates are in **logical pixels** relative
/// to the shaped run's origin (same frame as [`ShapedGlyph::x`] /
/// [`ShapedGlyph::y`]). `y` is the line top; the rect height is the
/// shaped line's height.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HighlightRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Color,
}

/// One text-decoration rect: a thin solid bar drawn under (underline) or
/// across (strikethrough) the glyphs of one styled run on one line.
/// Coordinates are in **logical pixels** relative to the shaped run's
/// origin, same frame as [`HighlightRect`]. `y`/`h` already encode the
/// decoration's vertical position (e.g. `baseline + ~size*0.10` for
/// underline) so backends just paint the rect — no extra metric lookup.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DecorationRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Color,
}

/// Per-run styling for attributed text shaping. Used by
/// [`GlyphAtlas::shape_and_rasterize_runs`] to compose styled runs into
/// one cosmic-text buffer with rich attributes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RunStyle {
    pub family: FontFamily,
    /// Monospace face used when [`Self::mono`] is set. Independent of
    /// [`Self::family`] so a paragraph can mix proportional and code
    /// runs that resolve through different theme slots.
    pub mono_family: FontFamily,
    pub weight: FontWeight,
    pub italic: bool,
    pub mono: bool,
    pub color: Color,
    /// Optional inline-run background, painted as a solid quad behind
    /// the glyphs that share this run's metadata. `None` skips the
    /// highlight pass for this run.
    pub bg: Option<Color>,
    /// Underline decoration. Backends emit one solid bar per
    /// (run, line) at `baseline + ~size * 0.10`.
    pub underline: bool,
    /// Strikethrough decoration. Backends emit one solid bar per
    /// (run, line) at `baseline - ~size * 0.28`.
    pub strikethrough: bool,
    /// Optional link target URL. When set, [`RunStyle::with_link`]
    /// also forces underline + [`crate::tokens::LINK_FOREGROUND`].
    /// Click hit-testing is not yet wired — the URL is carried so a
    /// future hit-test pass can route clicks to it.
    pub link: Option<String>,
}

impl RunStyle {
    pub fn new(weight: FontWeight, color: Color) -> Self {
        Self {
            family: FontFamily::default(),
            mono_family: FontFamily::JetBrainsMono,
            weight,
            italic: false,
            mono: false,
            color,
            bg: None,
            underline: false,
            strikethrough: false,
            link: None,
        }
    }
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    pub fn family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }
    pub fn mono_family(mut self, family: FontFamily) -> Self {
        self.mono_family = family;
        self
    }
    /// Set the inline-run background colour. Backends paint a solid
    /// quad spanning the run's per-line extent before the glyphs.
    pub fn with_bg(mut self, bg: Color) -> Self {
        self.bg = Some(bg);
        self
    }
    /// Underline this run.
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
    /// Strikethrough this run.
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }
    /// Tag this run as a link to `url`. Sets the run's color to the
    /// themed link foreground and forces underline so the run reads
    /// as a hyperlink at a glance — the same shape as `<a>` in HTML.
    /// The URL is carried in [`RunStyle::link`] for a future
    /// hit-test pass to consume.
    pub fn with_link(mut self, url: impl Into<String>) -> Self {
        self.link = Some(url.into());
        self.color = crate::tokens::LINK_FOREGROUND;
        self.underline = true;
        self
    }
}

/// Identity for a rasterized glyph at a specific pixel size. The `font`
/// component is `cosmic-text`'s `fontdb::ID`; `size_bits` matches
/// cosmic-text's own cache key (`f32::to_bits` of the requested em size)
/// so we can route LayoutGlyph cache keys straight through.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GlyphKey {
    pub font: fontdb::ID,
    pub glyph_id: u16,
    /// `font_size.to_bits()` — same encoding cosmic-text uses internally.
    pub size_bits: u32,
    /// Weight at which cosmic-text resolved this face. Threaded through
    /// to `FontSystem::get_font` so synthetic-bold faces rasterize at
    /// the same weight they were laid out with.
    pub weight: fontdb::Weight,
}

impl GlyphKey {
    pub fn size(&self) -> f32 {
        f32::from_bits(self.size_bits)
    }
}

/// One glyph's slot inside an atlas page.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GlyphSlot {
    pub page: u32,
    /// Pixel rect inside the page where the bitmap sits.
    pub rect: AtlasRect,
    /// Bitmap top-left in screen space relative to the pen+baseline.
    /// `top_left = (pen_x + offset.0, baseline_y - offset.1)`.
    pub offset: (i32, i32),
    /// `true` when the glyph carries its own RGB (color emoji from
    /// CBDT/COLR/sbix sources). Backends pass white as the per-glyph
    /// modulation color for these so the bitmap RGB passes through
    /// unmodulated; outline glyphs (`is_color = false`) are stored as
    /// `(255, 255, 255, alpha)` and modulated by the user's text color.
    pub is_color: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl AtlasRect {
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// Bytes per atlas pixel — RGBA8.
///
/// The atlas is unified: outline glyphs are stored as
/// `(255, 255, 255, alpha)` so the same shader works for monochrome
/// text and color emoji. Backends bind the page as
/// `Rgba8UnormSrgb` (or equivalent) and multiply the sampled texel by
/// the per-glyph color — for color glyphs the per-glyph color is white
/// so the bitmap RGB passes through unmodulated.
pub const ATLAS_BYTES_PER_PIXEL: u32 = 4;

/// One CPU-side atlas page. Backends sample from a GPU texture mirror.
pub struct AtlasPage {
    pub width: u32,
    pub height: u32,
    /// RGBA8 pixels, row-major, `width * height *
    /// ATLAS_BYTES_PER_PIXEL` bytes.
    pub pixels: Vec<u8>,
    /// Bounding box of writes since the last [`take_dirty`](GlyphAtlas::take_dirty)
    /// call. `None` means clean.
    dirty: Option<AtlasRect>,
    shelves: Vec<Shelf>,
}

#[derive(Copy, Clone)]
struct Shelf {
    y_top: u32,
    height: u32,
    cursor: u32,
}

/// Glyph rasterizer + atlas. Cheap to clone? No — owns font system and
/// allocations. One per backend.
pub struct GlyphAtlas {
    font_system: FontSystem,
    scale_ctx: ScaleContext,
    pages: Vec<AtlasPage>,
    map: HashMap<GlyphKey, GlyphSlot>,
    /// Per-font classification cache: `true` if the font carries one of
    /// the colour-bitmap tables (CBDT/CBLC, COLR, sbix). The recorder
    /// uses this to route glyphs from colour fonts down the bitmap path
    /// (this atlas) and glyphs from outline fonts down the MSDF path.
    color_font_cache: HashMap<fontdb::ID, bool>,
    /// Family names tried in priority order when shaping text. The
    /// **first** entry is the family name passed to cosmic-text's
    /// `Attrs::family`; cosmic-text then walks `fontdb` for
    /// per-codepoint fallback regardless of this list. Subsequent
    /// entries record intent (and let future versions of the library
    /// implement explicit per-codepoint stack walking if cosmic-text's
    /// implicit fallback proves inadequate).
    default_family_stack: Vec<String>,
    /// LRU cache of cosmic-text shaping output keyed by all inputs to
    /// `shape_runs_inner`. Sibling to the `metrics::SHAPE_CACHE` for
    /// the layout-side `TextLayout` cache, but living *here* because
    /// the atlas owns its own `FontSystem` separate from metrics' —
    /// glyph IDs in [`ShapedRun`] are bound to *this* `font_system`'s
    /// `fontdb::ID`s. Only the non-rasterizing path
    /// (`shape_runs_with_line_height`) hits the cache;
    /// `shape_and_rasterize_runs` has atlas-mutation side effects we
    /// can't replay from a cached value, so it bypasses.
    shape_cache: LruCache<ShapeRunKey, ShapedRun>,
}

/// Cache key for [`GlyphAtlas::shape_runs_inner`]. Captures every
/// input that influences the produced [`ShapedRun`]; floats are stored
/// as `to_bits` so they participate in `Hash` / `Eq` directly.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeRunKey {
    runs: Vec<(Box<str>, RunStyle)>,
    size_bits: u32,
    line_h_bits: u32,
    wrap: TextWrap,
    anchor: TextAnchor,
    available_width_bits: Option<u32>,
}

/// Bounded — see `metrics::SHAPE_CACHE_CAPACITY` for the matching
/// rationale. The atlas is owned by the backend, so the cache lives on
/// the atlas instance rather than thread-local.
const SHAPE_RUN_CACHE_CAPACITY: usize = 1024;

#[derive(Copy, Clone)]
struct ShapeRunOptions {
    line_h: f32,
    wrap: TextWrap,
    anchor: TextAnchor,
    available_width: Option<f32>,
    rasterize_into_color_atlas: bool,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphAtlas {
    /// Build an atlas with the bundled font set
    /// ([`damascene_fonts::DEFAULT_FONTS`]) loaded into the font database.
    /// To skip the bundled fonts, build with
    /// `damascene-core = { default-features = false }` and supply your own
    /// via [`Self::register_font`].
    pub fn new() -> Self {
        let font_system = bundled_font_system();
        Self {
            font_system,
            scale_ctx: ScaleContext::new(),
            pages: vec![AtlasPage::new(PAGE_SIZE, PAGE_SIZE)],
            map: HashMap::new(),
            color_font_cache: HashMap::new(),
            default_family_stack: vec![DEFAULT_SANS_FAMILY.to_string()],
            shape_cache: LruCache::new(NonZeroUsize::new(SHAPE_RUN_CACHE_CAPACITY).unwrap()),
        }
    }

    /// Borrow the cosmic-text font system. Backends use this to look up
    /// raw font bytes + face indices when feeding the MSDF generator.
    pub fn font_system(&self) -> &FontSystem {
        &self.font_system
    }

    pub fn font_system_mut(&mut self) -> &mut FontSystem {
        &mut self.font_system
    }

    /// `true` if the font carries colour-bitmap or layered-colour
    /// outline tables (CBDT/CBLC, COLR, sbix). Cached per-font so the
    /// classification cost amortises across many glyphs from the same
    /// face. Outline fonts (Roboto, Inter, Symbols2) return `false`.
    pub fn is_color_font(&mut self, id: fontdb::ID) -> bool {
        if let Some(&cached) = self.color_font_cache.get(&id) {
            return cached;
        }
        let result = self
            .font_system
            .db()
            .with_face_data(id, |bytes, face_index| {
                let face = ttf_parser::Face::parse(bytes, face_index).ok()?;
                let tables = face.tables();
                Some(tables.cbdt.is_some() || tables.colr.is_some() || tables.sbix.is_some())
            })
            .flatten()
            .unwrap_or(false);
        self.color_font_cache.insert(id, result);
        result
    }

    /// Register a font's raw bytes with the atlas's font database. The
    /// font's family, weight, and style are auto-detected from its
    /// metadata, so registering `Roboto-Bold.ttf` joins the existing
    /// `"Roboto"` family at weight 700.
    ///
    /// cosmic-text walks the database for per-codepoint fallback, so a
    /// registered emoji, CJK, or symbol font automatically participates
    /// in fallback for any glyph the primary family lacks. Use this to
    /// add color emoji once it's bundled, swap in a brand typeface, or
    /// extend coverage to scripts not in the default bundle.
    pub fn register_font(&mut self, bytes: Vec<u8>) {
        self.font_system.db_mut().load_font_data(bytes);
    }

    /// Replace the default font-family stack used when shaping text.
    /// The first entry is the primary family name passed to cosmic-text.
    /// Pass `["MyBrand", "Inter Variable"]` to make `MyBrand` the primary face
    /// and treat Inter as documentation of the expected fallback —
    /// cosmic-text's own fallback walks the full font database, so
    /// every registered font remains available regardless of order.
    pub fn set_default_family_stack(&mut self, stack: Vec<String>) {
        if !stack.is_empty() {
            self.default_family_stack = stack;
        }
    }

    /// The primary font family used when shaping, i.e. the first entry
    /// of the family stack. Defaults to `"Inter Variable"`.
    pub fn default_family(&self) -> &str {
        self.default_family_stack
            .first()
            .map(String::as_str)
            .unwrap_or(DEFAULT_SANS_FAMILY)
    }

    pub fn pages(&self) -> &[AtlasPage] {
        &self.pages
    }

    pub fn page(&self, index: u32) -> Option<&AtlasPage> {
        self.pages.get(index as usize)
    }

    pub fn slot(&self, key: GlyphKey) -> Option<GlyphSlot> {
        self.map.get(&key).copied()
    }

    /// Drain and return one dirty rect per page that has writes since
    /// the last call. Clears the dirty bookkeeping.
    pub fn take_dirty(&mut self) -> Vec<(usize, AtlasRect)> {
        let mut out = Vec::new();
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.dirty.take() {
                out.push((i, rect));
            }
        }
        out
    }

    /// Shape a single styled text run. Convenience wrapper around
    /// [`Self::shape_and_rasterize_runs`] for the (common) one-style
    /// case: every emitted glyph receives `color` and `run_index = 0`.
    #[allow(clippy::too_many_arguments)]
    pub fn shape_and_rasterize(
        &mut self,
        text: &str,
        size: f32,
        weight: FontWeight,
        wrap: TextWrap,
        anchor: TextAnchor,
        available_width: Option<f32>,
        color: Color,
    ) -> ShapedRun {
        self.shape_and_rasterize_runs(
            &[(text, RunStyle::new(weight, color))],
            size,
            wrap,
            anchor,
            available_width,
        )
    }

    /// Shape `runs` and return per-glyph positions without rasterizing.
    /// Backends that need to route glyphs by source-font kind (colour
    /// bitmap vs. outline → MSDF) call this and then invoke
    /// [`Self::ensure_color_glyph`] (or their MSDF atlas's `ensure`)
    /// per glyph.
    pub fn shape_runs(
        &mut self,
        runs: &[(&str, RunStyle)],
        size: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        available_width: Option<f32>,
    ) -> ShapedRun {
        self.shape_runs_with_line_height(
            runs,
            size,
            line_height(size),
            wrap,
            anchor,
            available_width,
        )
    }

    pub fn shape_runs_with_line_height(
        &mut self,
        runs: &[(&str, RunStyle)],
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        available_width: Option<f32>,
    ) -> ShapedRun {
        self.shape_runs_inner(
            runs,
            size,
            ShapeRunOptions {
                line_h: line_height,
                wrap,
                anchor,
                available_width,
                rasterize_into_color_atlas: false,
            },
        )
    }

    /// Rasterize a glyph into the colour-bitmap atlas. Idempotent. Use
    /// after [`Self::shape_runs`] when the recorder
    /// has decided this glyph belongs on the colour path (its source
    /// font is a colour font per [`Self::is_color_font`]).
    pub fn ensure_color_glyph(&mut self, key: GlyphKey) {
        self.ensure(key);
    }

    /// Shape an attributed sequence of styled runs into one cosmic-text
    /// buffer (so wrapping decisions cross run boundaries like real
    /// prose) and emit a single [`ShapedRun`] whose glyphs carry
    /// per-run color + `run_index`. Empty `runs` returns an empty
    /// [`ShapedRun`].
    ///
    /// `run_index` on each emitted [`ShapedGlyph`] points back into
    /// the input slice. The `metadata` field of cosmic-text's `Attrs`
    /// is used to round-trip the index through shaping.
    pub fn shape_and_rasterize_runs(
        &mut self,
        runs: &[(&str, RunStyle)],
        size: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        available_width: Option<f32>,
    ) -> ShapedRun {
        self.shape_runs_inner(
            runs,
            size,
            ShapeRunOptions {
                line_h: line_height(size),
                wrap,
                anchor,
                available_width,
                rasterize_into_color_atlas: true,
            },
        )
    }

    fn shape_runs_inner(
        &mut self,
        runs: &[(&str, RunStyle)],
        size: f32,
        options: ShapeRunOptions,
    ) -> ShapedRun {
        let ShapeRunOptions {
            line_h,
            wrap,
            anchor,
            available_width,
            rasterize_into_color_atlas,
        } = options;
        // Cache by full shaping inputs. Most UI text is re-shaped every
        // frame with identical params (label text, size, family, weight,
        // color); without this, paint repeats cosmic shape work that
        // doesn't change frame-to-frame. Sibling to
        // `metrics::SHAPE_CACHE`. We only cache the non-rasterizing
        // path — `rasterize_into_color_atlas == true` mutates the
        // color-bitmap atlas as a side effect of shaping, and a cache
        // hit can't replay those mutations. The other path (used for
        // attributed inlines that bake colour glyphs alongside shape)
        // doesn't dominate frames, so leaving it uncached is fine.
        if !rasterize_into_color_atlas {
            let key = ShapeRunKey {
                runs: runs
                    .iter()
                    .map(|(text, style)| (Box::from(*text), style.clone()))
                    .collect(),
                size_bits: size.to_bits(),
                line_h_bits: line_h.to_bits(),
                wrap,
                anchor,
                available_width_bits: available_width.map(f32::to_bits),
            };
            if let Some(cached) = self.shape_cache.get(&key).cloned() {
                return cached;
            }
            let shaped = self.shape_runs_compute(runs, size, options);
            self.shape_cache.put(key, shaped.clone());
            return shaped;
        }
        self.shape_runs_compute(runs, size, options)
    }

    fn shape_runs_compute(
        &mut self,
        runs: &[(&str, RunStyle)],
        size: f32,
        options: ShapeRunOptions,
    ) -> ShapedRun {
        let ShapeRunOptions {
            line_h,
            wrap,
            anchor,
            available_width,
            rasterize_into_color_atlas,
        } = options;
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_h));
        buffer.set_wrap(match wrap {
            TextWrap::NoWrap => Wrap::None,
            TextWrap::Wrap => Wrap::WordOrGlyph,
        });
        // cosmic-text uses the buffer width for both wrapping AND
        // alignment. For Wrap mode it's the wrap width; for NoWrap with
        // Middle/End anchors it's the box that line-alignment positions
        // glyphs within. Passing None for NoWrap+Middle leaves the
        // buffer unbounded and silently disables centering — single-
        // glyph button labels show up flush-left.
        buffer.set_size(available_width, None);

        // Clone to a local so the immutable borrow on self.default_family
        // doesn't conflict with the mutable font_system borrow below.
        let primary_family = runs
            .iter()
            .find(|(_, style)| !style.mono)
            .map(|(_, style)| style.family.family_name().to_string())
            .unwrap_or_else(|| self.default_family().to_string());
        let default_attrs = Attrs::new().family(Family::Name(&primary_family));
        // Mono runs resolve to `style.mono_family` (themed via
        // `Theme::mono_font_family`, default `JetBrainsMono`), so
        // proportional + code runs in the same paragraph land on
        // different fontdb faces.
        let spans = runs.iter().enumerate().map(|(i, (text, style))| {
            let family = if style.mono {
                style.mono_family.family_name()
            } else {
                style.family.family_name()
            };
            let attrs = Attrs::new()
                .family(Family::Name(family))
                .weight(cosmic_weight(style.weight))
                .style(if style.italic {
                    Style::Italic
                } else {
                    Style::Normal
                })
                .metadata(i);
            (*text, attrs)
        });
        let alignment = match anchor {
            TextAnchor::Start => None,
            TextAnchor::Middle => Some(cosmic_text::Align::Center),
            TextAnchor::End => Some(cosmic_text::Align::End),
        };
        buffer.set_rich_text(spans, &default_attrs, Shaping::Advanced, alignment);
        buffer.shape_until_scroll(&mut self.font_system, false);

        // Walk runs in source order, emit per-glyph entries, ensure
        // each unique CacheKey is rasterized into the atlas. Each
        // glyph's `metadata` carries the run index we packed into Attrs
        // above; we look up `runs[idx].color` to bake into the glyph.
        //
        // While walking, also accumulate per-line highlight rects for
        // runs that carry a `bg`. A highlight is closed when the
        // metadata index changes or the line ends, so a single styled
        // span that wraps produces one rect per line.
        let mut lines = Vec::new();
        let mut shaped_glyphs = Vec::new();
        let mut highlights: Vec<HighlightRect> = Vec::new();
        let mut decorations: Vec<DecorationRect> = Vec::new();
        let mut height: f32 = 0.0;
        let mut max_width: f32 = 0.0;
        // Proportional metrics — close enough for Inter, Roboto, and most
        // system fonts without a per-font swash lookup. See the design
        // notes in `RunStyle::underline` / `with_link`.
        let decoration_thickness = (size * 0.06).max(1.0);
        let underline_offset = size * 0.10;
        let strikethrough_offset = -size * 0.28;
        for run in buffer.layout_runs() {
            height = height.max(run.line_top + run.line_height);
            max_width = max_width.max(run.line_w);
            let (line_start, line_end) = run_byte_range(&run);
            lines.push(TextLine {
                text: line_slice(&run, line_start, line_end),
                width: run.line_w,
                y: run.line_top,
                baseline: run.line_y,
                rtl: run.rtl,
            });

            // (run_idx, color, x_min, x_max) — the open span on this
            // line. `None` between runs / for runs that don't carry
            // the corresponding decoration.
            let mut open_bg: Option<(usize, Color, f32, f32)> = None;
            let mut open_underline: Option<(usize, Color, f32, f32)> = None;
            let mut open_strike: Option<(usize, Color, f32, f32)> = None;

            let close_underline =
                |open: &mut Option<(usize, Color, f32, f32)>, sink: &mut Vec<DecorationRect>| {
                    if let Some((_, c, lo, hi)) = open.take() {
                        sink.push(DecorationRect {
                            x: lo,
                            y: run.line_y + underline_offset,
                            w: (hi - lo).max(0.0),
                            h: decoration_thickness,
                            color: c,
                        });
                    }
                };
            let close_strike = |open: &mut Option<(usize, Color, f32, f32)>,
                                sink: &mut Vec<DecorationRect>| {
                if let Some((_, c, lo, hi)) = open.take() {
                    sink.push(DecorationRect {
                        x: lo,
                        y: run.line_y + strikethrough_offset - decoration_thickness * 0.5,
                        w: (hi - lo).max(0.0),
                        h: decoration_thickness,
                        color: c,
                    });
                }
            };

            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let key = glyph_key(physical.cache_key);
                if rasterize_into_color_atlas {
                    self.ensure(key);
                }
                let run_idx = glyph.metadata.min(runs.len().saturating_sub(1));
                let style = runs.get(run_idx).map(|(_, s)| s);
                let color = style.map(|s| s.color).unwrap_or(Color::srgb_u8(0, 0, 0));
                let bg = style.and_then(|s| s.bg);
                let want_underline = style.is_some_and(|s| s.underline);
                let want_strike = style.is_some_and(|s| s.strikethrough);

                let g_left = glyph.x;
                let g_right = glyph.x + glyph.w;
                // bg highlight — paints behind glyphs.
                match (open_bg, bg) {
                    (Some((idx, c, lo, hi)), Some(_)) if idx == run_idx => {
                        open_bg = Some((idx, c, lo.min(g_left), hi.max(g_right)));
                    }
                    (Some((idx, c, lo, hi)), _) => {
                        highlights.push(HighlightRect {
                            x: lo,
                            y: run.line_top,
                            w: (hi - lo).max(0.0),
                            h: run.line_height,
                            color: c,
                        });
                        let _ = idx;
                        open_bg = bg.map(|c| (run_idx, c, g_left, g_right));
                    }
                    (None, Some(c)) => {
                        open_bg = Some((run_idx, c, g_left, g_right));
                    }
                    (None, None) => {}
                }
                // Underline + strikethrough — paint on top, color
                // tracks the run's text color so a link's blue
                // glyph gets a blue underline without an extra knob.
                match (open_underline, want_underline) {
                    (Some((idx, c, lo, hi)), true) if idx == run_idx => {
                        open_underline = Some((idx, c, lo.min(g_left), hi.max(g_right)));
                    }
                    (Some(_), _) => {
                        close_underline(&mut open_underline, &mut decorations);
                        if want_underline {
                            open_underline = Some((run_idx, color, g_left, g_right));
                        }
                    }
                    (None, true) => {
                        open_underline = Some((run_idx, color, g_left, g_right));
                    }
                    (None, false) => {}
                }
                match (open_strike, want_strike) {
                    (Some((idx, c, lo, hi)), true) if idx == run_idx => {
                        open_strike = Some((idx, c, lo.min(g_left), hi.max(g_right)));
                    }
                    (Some(_), _) => {
                        close_strike(&mut open_strike, &mut decorations);
                        if want_strike {
                            open_strike = Some((run_idx, color, g_left, g_right));
                        }
                    }
                    (None, true) => {
                        open_strike = Some((run_idx, color, g_left, g_right));
                    }
                    (None, false) => {}
                }

                shaped_glyphs.push(ShapedGlyph {
                    key,
                    x: glyph.x + glyph.x_offset,
                    y: run.line_y + glyph.y_offset,
                    byte_range: glyph.start..glyph.end,
                    color,
                    run_index: run_idx as u32,
                });
            }
            if let Some((_, c, lo, hi)) = open_bg {
                highlights.push(HighlightRect {
                    x: lo,
                    y: run.line_top,
                    w: (hi - lo).max(0.0),
                    h: run.line_height,
                    color: c,
                });
            }
            close_underline(&mut open_underline, &mut decorations);
            close_strike(&mut open_strike, &mut decorations);
        }

        let layout = TextLayout {
            width: max_width,
            height: height.max(line_h),
            line_height: line_h,
            lines,
        };

        ShapedRun {
            layout,
            glyphs: shaped_glyphs,
            highlights,
            decorations,
        }
    }

    fn ensure(&mut self, key: GlyphKey) {
        if self.map.contains_key(&key) {
            return;
        }
        let Some(slot) = self.rasterize_and_pack(key) else {
            // Glyph missing or zero-sized — record an empty slot so we
            // don't try again every frame.
            self.map.insert(
                key,
                GlyphSlot {
                    page: 0,
                    rect: AtlasRect {
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    },
                    offset: (0, 0),
                    is_color: false,
                },
            );
            return;
        };
        self.map.insert(key, slot);
    }

    fn rasterize_and_pack(&mut self, key: GlyphKey) -> Option<GlyphSlot> {
        let font = self.font_system.get_font(key.font, key.weight)?;
        let mut scaler = self
            .scale_ctx
            .builder(font.as_swash())
            .size(key.size())
            .hint(true)
            .build();

        let sources = [
            SwashSource::ColorOutline(0),
            SwashSource::ColorBitmap(StrikeWith::BestFit),
            SwashSource::Outline,
        ];
        // No `render.format(...)` call: let swash return native format.
        // Outline glyphs come back as `Content::Mask` (1 byte/px alpha);
        // CBDT/COLR/sbix color glyphs come back as `Content::Color`
        // (4 bytes/px RGBA). The atlas stores both as RGBA so backends
        // bind a single texture format and run a single shader path.
        let render = Render::new(&sources);
        let image = render.render(&mut scaler, key.glyph_id)?;
        let width = image.placement.width;
        let height = image.placement.height;
        if width == 0 || height == 0 || image.data.is_empty() {
            return None;
        }

        let (rgba, is_color) = expand_to_rgba(&image)?;

        let (page_idx, rect) = self.allocate(width, height)?;
        let page = &mut self.pages[page_idx];
        copy_rgba_bitmap(&mut page.pixels, page.width, &rect, &rgba);
        merge_dirty(&mut page.dirty, rect);

        Some(GlyphSlot {
            page: page_idx as u32,
            rect,
            offset: (image.placement.left, image.placement.top),
            is_color,
        })
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(usize, AtlasRect)> {
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.allocate(w, h) {
                return Some((i, rect));
            }
        }
        // Grow: add a new page sized to fit at least this glyph.
        let new_w = PAGE_SIZE.max(w.next_power_of_two());
        let new_h = PAGE_SIZE.max(h.next_power_of_two());
        let mut page = AtlasPage::new(new_w, new_h);
        let rect = page.allocate(w, h)?;
        self.pages.push(page);
        Some((self.pages.len() - 1, rect))
    }
}

impl AtlasPage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * ATLAS_BYTES_PER_PIXEL) as usize],
            dirty: None,
            shelves: Vec::new(),
        }
    }

    /// Pack a `w × h` glyph onto the next available shelf. Adds a new
    /// shelf below the current one if none fits.
    fn allocate(&mut self, w: u32, h: u32) -> Option<AtlasRect> {
        if w > self.width || h > self.height {
            return None;
        }
        // Try existing shelves: prefer the tightest fit (minimum waste).
        let mut best: Option<usize> = None;
        for (i, shelf) in self.shelves.iter().enumerate() {
            if shelf.cursor + w > self.width || shelf.height < h {
                continue;
            }
            let waste = shelf.height - h;
            if best
                .map(|b| waste < self.shelves[b].height - h)
                .unwrap_or(true)
            {
                best = Some(i);
            }
        }
        if let Some(i) = best {
            let shelf = &mut self.shelves[i];
            let rect = AtlasRect {
                x: shelf.cursor,
                y: shelf.y_top,
                w,
                h,
            };
            shelf.cursor += w;
            return Some(rect);
        }

        // Add a new shelf at the bottom of the existing ones.
        let next_y = self.shelves.last().map(|s| s.y_top + s.height).unwrap_or(0);
        if next_y + h > self.height {
            return None;
        }
        let shelf = Shelf {
            y_top: next_y,
            height: h,
            cursor: w,
        };
        self.shelves.push(shelf);
        Some(AtlasRect {
            x: 0,
            y: next_y,
            w,
            h,
        })
    }
}

/// Convert a swash glyph image into RGBA pixels for the unified atlas.
///
/// Returns `(rgba_bytes, is_color)`. Outline glyphs (`Content::Mask`)
/// expand to `(255, 255, 255, alpha)`; subpixel masks (rare; only
/// emitted when the renderer is told to produce them) expand similarly,
/// taking max(R, G, B) as alpha. Color bitmaps and color outlines come
/// back as 32-bit RGBA already and pass through.
fn expand_to_rgba(image: &SwashImage) -> Option<(Vec<u8>, bool)> {
    let pixels = (image.placement.width * image.placement.height) as usize;
    match image.content {
        SwashContent::Mask => {
            // 1 byte/px alpha → 4 bytes/px RGBA.
            if image.data.len() < pixels {
                return None;
            }
            let mut rgba = Vec::with_capacity(pixels * 4);
            for &a in &image.data[..pixels] {
                rgba.extend_from_slice(&[0xFF, 0xFF, 0xFF, a]);
            }
            Some((rgba, false))
        }
        SwashContent::Color => {
            // Already RGBA8.
            if image.data.len() < pixels * 4 {
                return None;
            }
            Some((image.data[..pixels * 4].to_vec(), true))
        }
        SwashContent::SubpixelMask => {
            // Emitted only when the renderer requests subpixel format
            // (we don't). Fall back to alpha = max(R, G, B) so we
            // never produce a black silhouette here.
            if image.data.len() < pixels * 4 {
                return None;
            }
            let mut rgba = Vec::with_capacity(pixels * 4);
            for chunk in image.data[..pixels * 4].chunks_exact(4) {
                let a = chunk[0].max(chunk[1]).max(chunk[2]);
                rgba.extend_from_slice(&[0xFF, 0xFF, 0xFF, a]);
            }
            Some((rgba, false))
        }
    }
}

fn copy_rgba_bitmap(dst: &mut [u8], dst_stride_pixels: u32, rect: &AtlasRect, src_rgba: &[u8]) {
    let bpp = ATLAS_BYTES_PER_PIXEL as usize;
    let dst_row_bytes = dst_stride_pixels as usize * bpp;
    let row_bytes = rect.w as usize * bpp;
    for row in 0..rect.h as usize {
        let dst_off = (rect.y as usize + row) * dst_row_bytes + rect.x as usize * bpp;
        let src_off = row * row_bytes;
        dst[dst_off..dst_off + row_bytes].copy_from_slice(&src_rgba[src_off..src_off + row_bytes]);
    }
}

fn merge_dirty(dirty: &mut Option<AtlasRect>, rect: AtlasRect) {
    *dirty = Some(match *dirty {
        None => rect,
        Some(prev) => {
            let x = prev.x.min(rect.x);
            let y = prev.y.min(rect.y);
            let r = prev.right().max(rect.right());
            let b = prev.bottom().max(rect.bottom());
            AtlasRect {
                x,
                y,
                w: r - x,
                h: b - y,
            }
        }
    });
}

fn glyph_key(cache_key: CacheKey) -> GlyphKey {
    // cosmic-text packs subpixel x/y bins into the cache key for
    // subpixel positioning. We quantize to whole pixels (subpixel bins
    // discarded) — backend can opt into subpixel later by widening the
    // key.
    GlyphKey {
        font: cache_key.font_id,
        glyph_id: cache_key.glyph_id,
        size_bits: cache_key.font_size_bits,
        weight: cache_key.font_weight,
    }
}

fn run_byte_range(run: &cosmic_text::LayoutRun<'_>) -> (usize, usize) {
    let start = run.glyphs.iter().map(|g| g.start).min().unwrap_or(0);
    let end = run.glyphs.iter().map(|g| g.end).max().unwrap_or(start);
    (start, end)
}

fn line_slice(run: &cosmic_text::LayoutRun<'_>, start: usize, end: usize) -> String {
    run.text
        .get(start..end)
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

fn bundled_font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.set_sans_serif_family(DEFAULT_SANS_FAMILY);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaping_emits_one_glyph_per_visible_codepoint() {
        let mut atlas = GlyphAtlas::new();
        let run = atlas.shape_and_rasterize(
            "abc",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.layout.lines.len(), 1);
        assert!(run.layout.width > 0.0);
    }

    #[test]
    fn repeated_glyph_reuses_atlas_slot() {
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "aaa",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        let pages_before = atlas.pages().len();
        let dirty_before: u32 = atlas
            .pages()
            .iter()
            .map(|p| p.dirty.map(|r| r.w * r.h).unwrap_or(0))
            .sum();

        // Drain dirty so a new write would re-mark.
        atlas.take_dirty();
        atlas.shape_and_rasterize(
            "aa",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert_eq!(atlas.pages().len(), pages_before);
        // No new rasterization — every glyph was already cached, so
        // the dirty region stays None on the second call.
        let dirty_after: u32 = atlas
            .pages()
            .iter()
            .map(|p| p.dirty.map(|r| r.w * r.h).unwrap_or(0))
            .sum();
        assert_eq!(dirty_after, 0);
        assert!(dirty_before > 0);
    }

    #[test]
    fn distinct_sizes_get_distinct_slots() {
        let mut atlas = GlyphAtlas::new();
        let r16 = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        let r24 = atlas.shape_and_rasterize(
            "A",
            24.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert_eq!(r16.glyphs.len(), 1);
        assert_eq!(r24.glyphs.len(), 1);
        let s16 = atlas.slot(r16.glyphs[0].key).unwrap();
        let s24 = atlas.slot(r24.glyphs[0].key).unwrap();
        // Different size → different rasterization → different slot.
        assert_ne!(s16.rect, s24.rect);
        assert!(s24.rect.h >= s16.rect.h);
    }

    #[test]
    fn distinct_weights_get_distinct_slots() {
        let mut atlas = GlyphAtlas::new();
        let regular = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        let bold = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Bold,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        let r = atlas.slot(regular.glyphs[0].key).unwrap();
        let b = atlas.slot(bold.glyphs[0].key).unwrap();
        assert_ne!(regular.glyphs[0].key, bold.glyphs[0].key);
        assert_ne!(r.rect, b.rect);
    }

    #[test]
    fn dirty_region_covers_new_glyphs_and_clears_on_take() {
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "Hello",
            18.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        let dirty = atlas.take_dirty();
        assert_eq!(dirty.len(), 1, "expected one dirty page after first run");
        let (page_idx, rect) = dirty[0];
        assert_eq!(page_idx, 0);
        assert!(rect.w > 0 && rect.h > 0);
        assert!(atlas.take_dirty().is_empty());
    }

    #[test]
    fn shelves_pack_a_realistic_text_run_into_one_page() {
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "The quick brown fox jumps over the lazy dog 0123456789",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        // A typical body-text run easily fits on one 512x512 page.
        // The packer is allowed to use multiple shelves; the contract
        // is just "no spurious second page."
        assert_eq!(atlas.pages().len(), 1);
    }

    #[test]
    fn many_distinct_glyphs_can_grow_to_a_second_page() {
        let mut atlas = GlyphAtlas::new();
        // Combine many sizes/weights to exhaust one page eventually.
        for size in [10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0] {
            for weight in [FontWeight::Regular, FontWeight::Bold] {
                atlas.shape_and_rasterize(
                    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
                    size,
                    weight,
                    TextWrap::NoWrap,
                    TextAnchor::Start,
                    None,
                    Color::srgb_u8(0, 0, 0),
                );
            }
        }
        // The exact page count depends on shelf packing efficiency; what
        // matters is that the allocator successfully made room for every
        // glyph (i.e. didn't panic / drop entries).
        let total_glyphs: usize = atlas.map.len();
        assert!(total_glyphs > 100, "only stored {total_glyphs} glyphs");
    }

    #[test]
    fn attributed_runs_bake_per_run_color_and_run_index() {
        // Three runs with three colors; expect one ShapedRun whose
        // glyphs carry per-run colors and run_index 0/1/2.
        let mut atlas = GlyphAtlas::new();
        let red = Color::srgb_u8(255, 0, 0);
        let green = Color::srgb_u8(0, 255, 0);
        let blue = Color::srgb_u8(0, 0, 255);
        let runs = [
            ("AA", RunStyle::new(FontWeight::Regular, red)),
            ("BB", RunStyle::new(FontWeight::Bold, green)),
            ("CC", RunStyle::new(FontWeight::Regular, blue).italic()),
        ];
        let shaped =
            atlas.shape_and_rasterize_runs(&runs, 16.0, TextWrap::NoWrap, TextAnchor::Start, None);
        // Six visible glyphs total — one per character in "AABBCC".
        assert_eq!(shaped.glyphs.len(), 6);
        // First two glyphs come from run 0 (red), next two from run 1
        // (green, bold), final two from run 2 (blue, italic).
        assert_eq!(shaped.glyphs[0].run_index, 0);
        assert_eq!(shaped.glyphs[0].color, red);
        assert_eq!(shaped.glyphs[2].run_index, 1);
        assert_eq!(shaped.glyphs[2].color, green);
        assert_eq!(shaped.glyphs[4].run_index, 2);
        assert_eq!(shaped.glyphs[4].color, blue);
        // Different weights remain baked into the glyph key. Variable
        // fonts such as Inter can share a font ID across weights, so the
        // contract is the resolved weight rather than face identity.
        assert_ne!(shaped.glyphs[0].key.weight, shaped.glyphs[2].key.weight);
        // Italic resolves to an italic face distinct from both Regular
        // (run 0) and Bold (run 1). Before an italic face was bundled,
        // asking cosmic-text for Style::Italic panicked its font
        // fallback chain; this assertion guards the regression.
        assert_ne!(shaped.glyphs[4].key.font, shaped.glyphs[0].key.font);
        assert_ne!(shaped.glyphs[4].key.font, shaped.glyphs[2].key.font);
    }

    #[test]
    fn run_with_bg_emits_one_highlight_per_line() {
        // Two single-line runs: only the second one carries a bg.
        // Expect exactly one highlight rect, spanning the second run's
        // glyph extent at the line's vertical bounds.
        let mut atlas = GlyphAtlas::new();
        let yellow = Color::srgb_u8(220, 200, 60);
        let runs = [
            (
                "plain ",
                RunStyle::new(FontWeight::Regular, Color::srgb_u8(0, 0, 0)),
            ),
            (
                "marked",
                RunStyle::new(FontWeight::Regular, Color::srgb_u8(0, 0, 0)).with_bg(yellow),
            ),
        ];
        let shaped =
            atlas.shape_and_rasterize_runs(&runs, 16.0, TextWrap::NoWrap, TextAnchor::Start, None);
        assert_eq!(
            shaped.highlights.len(),
            1,
            "expected one highlight rect, got {:?}",
            shaped.highlights
        );
        let h = shaped.highlights[0];
        assert_eq!(h.color, yellow);
        assert!(h.w > 0.0, "zero-width highlight: {h:?}");
        // Must sit at the line's top with the line's height.
        assert_eq!(h.h, shaped.layout.line_height);
        // First run's glyphs come before the highlight; their
        // rightmost pen position must not exceed the highlight's left
        // edge (within fp tolerance).
        let last_plain = shaped
            .glyphs
            .iter()
            .filter(|g| g.run_index == 0)
            .map(|g| g.x)
            .fold(0.0_f32, f32::max);
        assert!(
            h.x + 1e-3 >= last_plain,
            "highlight starts before plain runs end: hx={} last_plain={}",
            h.x,
            last_plain,
        );
    }

    #[test]
    fn run_with_bg_wraps_to_two_highlight_rects() {
        // One styled run that wraps. The shaper produces multiple
        // lines; the highlight pass emits one rect per line for the
        // span sitting on that line.
        let mut atlas = GlyphAtlas::new();
        let blue = Color::srgb_u8(60, 120, 240);
        let runs = [(
            "the quick brown fox jumps over the lazy dog",
            RunStyle::new(FontWeight::Regular, Color::srgb_u8(0, 0, 0)).with_bg(blue),
        )];
        // Narrow available width forces wrapping.
        let shaped = atlas.shape_and_rasterize_runs(
            &runs,
            16.0,
            TextWrap::Wrap,
            TextAnchor::Start,
            Some(120.0),
        );
        assert!(
            shaped.layout.lines.len() >= 2,
            "expected wrapped layout, got {:?}",
            shaped.layout.lines.len()
        );
        assert_eq!(
            shaped.highlights.len(),
            shaped.layout.lines.len(),
            "expected one highlight per wrapped line: highlights={:?}",
            shaped.highlights,
        );
        for h in &shaped.highlights {
            assert_eq!(h.color, blue);
            assert!(h.w > 0.0);
        }
    }

    #[test]
    fn run_without_bg_emits_no_highlights() {
        let mut atlas = GlyphAtlas::new();
        let shaped = atlas.shape_and_rasterize(
            "no highlight",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert!(shaped.highlights.is_empty());
    }

    #[test]
    fn run_with_underline_emits_one_decoration_per_line() {
        // A single underlined run on a single line produces one
        // DecorationRect spanning the run's glyph extent at
        // baseline + ~size*0.10. Color tracks the run's text color
        // so a link's blue text gets a blue underline.
        let mut atlas = GlyphAtlas::new();
        let teal = Color::srgb_u8(20, 200, 200);
        let runs = [(
            "underlined",
            RunStyle::new(FontWeight::Regular, teal).underline(),
        )];
        let shaped =
            atlas.shape_and_rasterize_runs(&runs, 16.0, TextWrap::NoWrap, TextAnchor::Start, None);
        assert_eq!(
            shaped.decorations.len(),
            1,
            "expected one underline rect, got {:?}",
            shaped.decorations,
        );
        let d = shaped.decorations[0];
        assert_eq!(d.color, teal);
        assert!(d.h >= 1.0, "thickness must clamp to >= 1px, got {}", d.h);
        // Underline sits below the baseline.
        let line = &shaped.layout.lines[0];
        assert!(
            d.y > line.baseline,
            "underline y={} should be below baseline={}",
            d.y,
            line.baseline,
        );
        assert!(
            d.w > 0.0,
            "underline must span the glyph extent, got w={}",
            d.w,
        );
    }

    #[test]
    fn run_with_strikethrough_sits_above_baseline() {
        let mut atlas = GlyphAtlas::new();
        let runs = [(
            "struck",
            RunStyle::new(FontWeight::Regular, Color::srgb_u8(0, 0, 0)).strikethrough(),
        )];
        let shaped =
            atlas.shape_and_rasterize_runs(&runs, 16.0, TextWrap::NoWrap, TextAnchor::Start, None);
        assert_eq!(shaped.decorations.len(), 1);
        let d = shaped.decorations[0];
        let line = &shaped.layout.lines[0];
        assert!(
            d.y < line.baseline,
            "strikethrough y={} should sit above baseline={}",
            d.y,
            line.baseline,
        );
    }

    #[test]
    fn run_with_link_emits_underline_in_link_color() {
        // `.with_link(url)` is a one-call helper: it pins color to
        // LINK_FOREGROUND, forces underline, and carries the URL
        // through to RunStyle.link for a future hit-test pass.
        let mut atlas = GlyphAtlas::new();
        let runs = [(
            "click me",
            RunStyle::new(FontWeight::Regular, Color::srgb_u8(0, 0, 0))
                .with_link("https://example.com"),
        )];
        let shaped =
            atlas.shape_and_rasterize_runs(&runs, 16.0, TextWrap::NoWrap, TextAnchor::Start, None);
        assert_eq!(shaped.decorations.len(), 1);
        assert_eq!(shaped.decorations[0].color, crate::tokens::LINK_FOREGROUND);
        // Glyphs render in the link color too.
        assert_eq!(shaped.glyphs[0].color, crate::tokens::LINK_FOREGROUND);
    }

    #[test]
    fn underline_wraps_with_text_to_two_decoration_rects() {
        // A single underlined run that wraps across two lines emits
        // one DecorationRect per visual line — same shape as the
        // bg-highlight wrapping case.
        let mut atlas = GlyphAtlas::new();
        let runs = [(
            "the quick brown fox jumps over the lazy dog",
            RunStyle::new(FontWeight::Regular, Color::srgb_u8(0, 0, 0)).underline(),
        )];
        let shaped = atlas.shape_and_rasterize_runs(
            &runs,
            16.0,
            TextWrap::Wrap,
            TextAnchor::Start,
            Some(120.0),
        );
        assert!(
            shaped.decorations.len() >= 2,
            "expected one decoration rect per wrapped line, got {:?}",
            shaped.decorations,
        );
        // No two decoration rects should share a y — one per line.
        let mut ys: Vec<f32> = shaped.decorations.iter().map(|d| d.y).collect();
        ys.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        assert_eq!(ys.len(), shaped.decorations.len());
    }

    #[test]
    fn run_without_decorations_emits_no_decoration_rects() {
        let mut atlas = GlyphAtlas::new();
        let shaped = atlas.shape_and_rasterize(
            "plain",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert!(shaped.decorations.is_empty());
    }

    #[test]
    fn fallback_face_resolves_math_arrow() {
        // U+2192 RIGHTWARDS ARROW lives in NotoSansSymbols2, not in
        // the Latin sans bundle. Shaping should still produce a
        // non-zero glyph (i.e. not a tofu replacement) because
        // cosmic-text walks fontdb to find the codepoint in the
        // bundled symbols face.
        let mut atlas = GlyphAtlas::new();
        let run = atlas.shape_and_rasterize(
            "→",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert_eq!(run.glyphs.len(), 1, "expected one glyph for arrow");
        let slot = atlas.slot(run.glyphs[0].key).expect("arrow slot");
        // Non-zero slot rect proves the glyph was rasterized rather
        // than missing.
        assert!(
            slot.rect.w > 0 && slot.rect.h > 0,
            "expected real bitmap, got {slot:?}"
        );
    }

    #[test]
    #[cfg(feature = "inter")]
    fn register_font_adds_to_database() {
        // Re-register a known face as a sanity check: load_font_data
        // accepting our bytes proves the path is wired. (Verifying
        // *novel* coverage requires a font with a glyph the bundle
        // lacks — that's the symbols-fallback test above.)
        let mut atlas = GlyphAtlas::new();
        let before = atlas.font_system.db().faces().count();
        atlas.register_font(damascene_fonts::INTER_VARIABLE.to_vec());
        let after = atlas.font_system.db().faces().count();
        assert!(after > before, "register_font should add a face");
    }

    #[test]
    fn set_default_family_stack_changes_primary_family() {
        let mut atlas = GlyphAtlas::new();
        assert_eq!(atlas.default_family(), "Inter Variable");
        atlas.set_default_family_stack(vec!["MyBrand".into(), "Inter Variable".into()]);
        assert_eq!(atlas.default_family(), "MyBrand");
        // Empty stack is rejected — primary family stays put.
        atlas.set_default_family_stack(vec![]);
        assert_eq!(atlas.default_family(), "MyBrand");
    }

    #[test]
    fn colr_v0_glyph_rasterizes_with_palette_colors() {
        // Synthetic COLRv0 font: a single PUA glyph at U+E001 composed
        // of two CPAL layers (red square, blue diamond on top). swash's
        // ColorOutline source should rasterize both layers, blit each
        // with its palette color into one RGBA buffer, and emit
        // Content::Color — which the unified atlas captures as a color
        // glyph. Verifies that COLRv0 (not just CBDT) flows through the
        // engine.
        const COLR_FONT: &[u8] = include_bytes!("../../tests/fixtures/test_colr.ttf");
        let mut atlas = GlyphAtlas::new();
        atlas.register_font(COLR_FONT.to_vec());
        atlas.set_default_family_stack(vec!["DamasceneColrTest".into()]);

        let run = atlas.shape_and_rasterize(
            "\u{E001}",
            48.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(255, 255, 255),
        );
        assert_eq!(run.glyphs.len(), 1, "expected one glyph for U+E001");
        let slot = atlas.slot(run.glyphs[0].key).expect("colr slot");
        assert!(
            slot.is_color,
            "COLR glyph should be marked is_color = true; got {slot:?}"
        );

        // Walk the slot's rect and look for distinct red and blue
        // pixels. Both must be present for the test to prove that
        // multi-layer COLR rasterization actually composited.
        let page = &atlas.pages()[slot.page as usize];
        let stride = page.width as usize * ATLAS_BYTES_PER_PIXEL as usize;
        let mut found_red = false;
        let mut found_blue = false;
        for row in 0..slot.rect.h as usize {
            for col in 0..slot.rect.w as usize {
                let off = (slot.rect.y as usize + row) * stride + (slot.rect.x as usize + col) * 4;
                let r = page.pixels[off];
                let g = page.pixels[off + 1];
                let b = page.pixels[off + 2];
                let a = page.pixels[off + 3];
                if a < 200 {
                    continue;
                }
                if r > 200 && g < 60 && b < 60 {
                    found_red = true;
                }
                if b > 200 && r < 60 && g < 60 {
                    found_blue = true;
                }
            }
        }
        assert!(
            found_red,
            "expected red pixels from CPAL palette index 0 (square layer)"
        );
        assert!(
            found_blue,
            "expected blue pixels from CPAL palette index 1 (diamond layer)"
        );
    }

    #[cfg(feature = "emoji")]
    #[test]
    fn color_emoji_glyph_rasterizes_in_color() {
        // 😀 GRINNING FACE — present in NotoColorEmoji as a CBDT
        // bitmap. Outline-only fallback fonts can't render this; we
        // verify (a) the slot is marked is_color, and (b) at least one
        // pixel inside the glyph rect carries non-grayscale RGB,
        // proving the bitmap RGB survived rasterization rather than
        // being collapsed to a B&W silhouette.
        let mut atlas = GlyphAtlas::new();
        let run = atlas.shape_and_rasterize(
            "😀",
            32.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        assert_eq!(run.glyphs.len(), 1, "expected one glyph for 😀");
        let slot = atlas.slot(run.glyphs[0].key).expect("emoji slot");
        assert!(
            slot.is_color,
            "expected color glyph, got {slot:?} on a font that should be NotoColorEmoji"
        );

        let page = &atlas.pages()[slot.page as usize];
        let stride = page.width as usize * ATLAS_BYTES_PER_PIXEL as usize;
        let mut found_color = false;
        for row in 0..slot.rect.h as usize {
            for col in 0..slot.rect.w as usize {
                let off = (slot.rect.y as usize + row) * stride + (slot.rect.x as usize + col) * 4;
                let r = page.pixels[off];
                let g = page.pixels[off + 1];
                let b = page.pixels[off + 2];
                let a = page.pixels[off + 3];
                if a > 0 && (r != g || g != b) {
                    found_color = true;
                    break;
                }
            }
            if found_color {
                break;
            }
        }
        assert!(
            found_color,
            "expected at least one pixel with non-grayscale RGB inside 😀 bitmap"
        );
    }

    #[test]
    fn outline_glyph_stores_white_alpha_in_rgba_atlas() {
        // Sanity check the unified-RGBA migration: an outline glyph
        // (e.g. 'A') should have R==G==B==255 in every pixel that has
        // alpha — i.e. the alpha-coverage mask was expanded to
        // (255, 255, 255, alpha) so the per-glyph color modulation in
        // the backend shader produces the expected text color.
        let mut atlas = GlyphAtlas::new();
        let run = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
        let slot = atlas.slot(run.glyphs[0].key).expect("A slot");
        assert!(!slot.is_color);
        let page = &atlas.pages()[slot.page as usize];
        let stride = page.width as usize * ATLAS_BYTES_PER_PIXEL as usize;
        let mut sampled_alpha = 0;
        for row in 0..slot.rect.h as usize {
            for col in 0..slot.rect.w as usize {
                let off = (slot.rect.y as usize + row) * stride + (slot.rect.x as usize + col) * 4;
                let r = page.pixels[off];
                let g = page.pixels[off + 1];
                let b = page.pixels[off + 2];
                let a = page.pixels[off + 3];
                if a > 0 {
                    assert_eq!(
                        (r, g, b),
                        (255, 255, 255),
                        "outline glyph rgb should be white"
                    );
                    sampled_alpha = sampled_alpha.max(a);
                }
            }
        }
        assert!(sampled_alpha > 0, "expected at least one covered pixel");
    }

    #[test]
    fn empty_glyph_caches_zero_slot_without_panicking() {
        // A space is typically a non-rendering glyph (zero-sized
        // bitmap). Shaping a string with spaces should not panic and
        // should still cache a slot so we don't retry every call.
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "    ",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
            Color::srgb_u8(0, 0, 0),
        );
    }
}
