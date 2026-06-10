//! MTSDF glyph atlas — outline glyphs only.
//!
//! One MTSDF per `(font, glyph)`, sized at a fixed base em and reused
//! at every logical render size. Pages are RGBA8: RGB carries the
//! standard 3-channel MSDF, A carries a true single-channel SDF. The
//! shader uses A as a fallback wherever median(R,G,B) disagrees with
//! it, eliminating the false-outside artifacts that MSDF produces near
//! sharp corners. A backend mirrors pages onto a GPU texture and
//! samples them through the `stock::text_msdf` shader.
//!
//! Color-emoji glyphs flow through the separate
//! [`crate::text::atlas::GlyphAtlas`] (size-keyed RGBA bitmaps). The
//! recorder routes each glyph to whichever atlas matches the source
//! face — outline fonts here, color fonts there.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::collections::HashMap;

use cosmic_text::fontdb;
use ttf_parser::Face;

use crate::text::msdf::{MsdfGlyph, build_glyph_msdf, glyph_advance};

/// Default base em size (atlas pixels). 48 covers UI sizes 10–64 with
/// good fidelity at the cost of ~9 KB per glyph (48×48×4). Smaller
/// values (32) lose noticeable sharpness at body sizes (12–14 px) on
/// 1× displays; larger values (64) only marginally improve quality.
pub const DEFAULT_BASE_EM: u32 = 48;
/// Default MSDF spread radius in atlas pixels. 6 px at 48 base-em gives
/// clean AA with margin for thin strokes; the absolute value scales
/// with base_em (we keep ~12.5% of base).
pub const DEFAULT_SPREAD: f64 = 6.0;

/// Atlas page side. 1024 holds ~600 typical 32-em-px MSDFs without
/// growing.
const PAGE_SIZE: u32 = 1024;

/// Soft cap on resident pages (8 × 1024² RGBA ≈ 32 MB, roughly 4–5K
/// distinct glyphs — comfortably more than a screenful of CJK). At the
/// cap, making room recycles the least-recently-used page in place
/// instead of growing — unless every page was referenced this frame,
/// in which case the atlas grows past the budget (instances already
/// recorded this frame point at their pages' UVs).
const PAGE_BUDGET: usize = 8;

/// Inter-glyph padding (atlas pixels) so neighbour MSDF gradients don't
/// bleed under bilinear filtering.
const GLYPH_PADDING: u32 = 2;

/// Bytes per atlas pixel — RGBA8 (RGB = MSDF distance channels, A=255).
pub const MSDF_BYTES_PER_PIXEL: u32 = 4;

/// Atlas key — outline glyphs are size-independent.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct MsdfGlyphKey {
    /// fontdb face the glyph belongs to (the shaping atlas's
    /// `FontSystem` IDs).
    pub font: fontdb::ID,
    /// Glyph index within the face (not a codepoint).
    pub glyph_id: u16,
}

/// Axis-aligned pixel rect inside an MSDF atlas page (y-down, top-left
/// origin). Units are atlas texels.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MsdfRect {
    /// Left edge in atlas px.
    pub x: u32,
    /// Top edge in atlas px.
    pub y: u32,
    /// Width in atlas px.
    pub w: u32,
    /// Height in atlas px.
    pub h: u32,
}

impl MsdfRect {
    /// One past the right edge: `x + w`.
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    /// One past the bottom edge: `y + h`.
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// Where a cached MSDF glyph lives, plus the metrics the recorder needs
/// to place its quad.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MsdfSlot {
    /// Index into [`MsdfAtlas::pages`] of the page holding the MTSDF.
    pub page: u32,
    /// Pixel rect inside the page where the MTSDF bitmap sits.
    pub rect: MsdfRect,
    /// Pen-relative X of the bitmap top-left, in base-em px (includes
    /// the SDF spread).
    pub bearing_x: f32,
    /// Baseline-relative Y of the bitmap top edge, in base-em px,
    /// y-down (includes spread; typically negative).
    pub bearing_y: f32,
    /// Horizontal advance width in base-em px.
    pub advance: f32,
    /// MSDF spread in base-em px — needed to derive distance from
    /// sampled byte values in the shader.
    pub spread: f32,
}

#[derive(Copy, Clone)]
struct Shelf {
    y_top: u32,
    height: u32,
    cursor: u32,
}

/// One CPU-side MSDF atlas page. Backends sample from a GPU texture
/// mirror they keep in sync via [`MsdfAtlas::take_dirty`].
pub struct MsdfAtlasPage {
    /// Page width in pixels.
    pub width: u32,
    /// Page height in pixels.
    pub height: u32,
    /// Row-major RGBA8.
    pub pixels: Vec<u8>,
    dirty: Option<MsdfRect>,
    shelves: Vec<Shelf>,
    /// Frame stamp of the last allocation or `touch`/`ensure` hit.
    /// Pages stamped before the current frame are recycling candidates
    /// once the atlas reaches [`PAGE_BUDGET`].
    last_used: u64,
}

impl MsdfAtlasPage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * MSDF_BYTES_PER_PIXEL) as usize],
            dirty: None,
            shelves: Vec::new(),
            last_used: 0,
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<MsdfRect> {
        if w > self.width || h > self.height {
            return None;
        }
        // Best-fit on existing shelves (least leftover height).
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
            let rect = MsdfRect {
                x: shelf.cursor,
                y: shelf.y_top,
                w,
                h,
            };
            shelf.cursor += w + GLYPH_PADDING;
            return Some(rect);
        }
        let next_y = self
            .shelves
            .last()
            .map(|s| s.y_top + s.height + GLYPH_PADDING)
            .unwrap_or(0);
        if next_y + h > self.height {
            return None;
        }
        self.shelves.push(Shelf {
            y_top: next_y,
            height: h,
            cursor: w + GLYPH_PADDING,
        });
        Some(MsdfRect {
            x: 0,
            y: next_y,
            w,
            h,
        })
    }
}

/// MSDF glyph cache.
pub struct MsdfAtlas {
    pages: Vec<MsdfAtlasPage>,
    /// `Some(slot)` for a cached glyph, `None` when the glyph has no
    /// outline (whitespace, .notdef without contours) — recorded so the
    /// recorder still gets the advance width without re-trying every
    /// frame.
    map: HashMap<MsdfGlyphKey, MsdfEntry>,
    base_em: u32,
    spread: f64,
    /// Frame counter for page-LRU bookkeeping, bumped once per frame in
    /// [`Self::take_dirty`] (which every backend drains exactly once
    /// per frame in its flush).
    frame: u64,
    /// Resident-page soft cap; [`PAGE_BUDGET`] outside tests.
    page_budget: usize,
    /// How many of the most recent frame ticks count as "in use" for
    /// the page recycler — see [`Self::set_lru_protection_window`].
    lru_protection_window: u64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum MsdfEntry {
    /// Glyph has an outline and is packed into the atlas.
    Slot(MsdfSlot),
    /// Glyph has no outline; only the advance width is meaningful.
    Empty { advance: f32 },
}

impl Default for MsdfAtlas {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_EM, DEFAULT_SPREAD)
    }
}

impl MsdfAtlas {
    /// Build an empty atlas. `base_em` is the em size (atlas px) every
    /// MTSDF is generated at; `spread` is the SDF radius in atlas px.
    /// Use [`DEFAULT_BASE_EM`] / [`DEFAULT_SPREAD`] (via
    /// `MsdfAtlas::default()`) unless tuning quality vs. memory.
    pub fn new(base_em: u32, spread: f64) -> Self {
        Self {
            pages: vec![MsdfAtlasPage::new(PAGE_SIZE, PAGE_SIZE)],
            map: HashMap::new(),
            base_em,
            spread,
            frame: 0,
            page_budget: PAGE_BUDGET,
            lru_protection_window: 1,
        }
    }

    /// Widen the recycler's "referenced this frame" guard to the last
    /// `n` frame ticks (default 1).
    ///
    /// The frame counter advances once per [`Self::take_dirty`], i.e.
    /// once per *backend flush*. When several `Runner`s share one
    /// atlas (see `damascene_wgpu::SharedText`), each runner's flush
    /// ticks the shared counter — so a page referenced by window A's
    /// recorded-but-not-yet-submitted instances would look one-tick
    /// stale during window B's prepare and could be recycled out from
    /// under A's frame. Setting the window to the number of attached
    /// runners protects every page referenced within the last
    /// whole-host frame, whatever the hosts's prepare/render
    /// interleaving. Clamped to at least 1.
    pub fn set_lru_protection_window(&mut self, n: u32) {
        self.lru_protection_window = u64::from(n.max(1));
    }

    /// Em size (atlas px) glyphs are generated at. Backends scale
    /// quads by `render_size / base_em`.
    pub fn base_em(&self) -> u32 {
        self.base_em
    }

    /// SDF spread radius in atlas px, as passed to [`Self::new`].
    pub fn spread(&self) -> f64 {
        self.spread
    }

    /// All resident pages, indexed by [`MsdfSlot::page`]. Backends
    /// mirror each page to a GPU texture.
    pub fn pages(&self) -> &[MsdfAtlasPage] {
        &self.pages
    }

    /// One page by index ([`MsdfSlot::page`]), or `None` if out of
    /// range.
    pub fn page(&self, index: u32) -> Option<&MsdfAtlasPage> {
        self.pages.get(index as usize)
    }

    /// Atlas slot for a cached glyph, if present and non-empty.
    pub fn slot(&self, key: MsdfGlyphKey) -> Option<MsdfSlot> {
        match self.map.get(&key)? {
            MsdfEntry::Slot(s) => Some(*s),
            MsdfEntry::Empty { .. } => None,
        }
    }

    /// [`Self::slot`], but also stamps the slot's page as used this
    /// frame so the LRU page recycler skips it. Backends call this on
    /// their per-glyph draw path; every glyph drawn each frame keeps
    /// its page warm.
    pub fn touch(&mut self, key: MsdfGlyphKey) -> Option<MsdfSlot> {
        let slot = self.slot(key)?;
        self.pages[slot.page as usize].last_used = self.frame;
        Some(slot)
    }

    /// Cached advance width for a glyph (works for both outline and
    /// whitespace entries).
    pub fn advance(&self, key: MsdfGlyphKey) -> Option<f32> {
        Some(match self.map.get(&key)? {
            MsdfEntry::Slot(s) => s.advance,
            MsdfEntry::Empty { advance } => *advance,
        })
    }

    /// Drain dirty rects since the last call (one per page that has new
    /// writes).
    ///
    /// Every backend drains dirty rects exactly once per frame in its
    /// flush, so this call doubles as the atlas's frame tick for
    /// page-LRU bookkeeping.
    pub fn take_dirty(&mut self) -> Vec<(usize, MsdfRect)> {
        self.frame += 1;
        let mut out = Vec::new();
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.dirty.take() {
                out.push((i, rect));
            }
        }
        out
    }

    /// Ensure the glyph is rasterized into the atlas; returns the slot
    /// (or `None` for empty/notdef glyphs).
    pub fn ensure(&mut self, key: MsdfGlyphKey, face: &Face<'_>) -> Option<MsdfSlot> {
        if let Some(entry) = self.map.get(&key) {
            return match entry {
                MsdfEntry::Slot(s) => {
                    let s = *s;
                    // Keep the page warm so the LRU recycler skips it.
                    self.pages[s.page as usize].last_used = self.frame;
                    Some(s)
                }
                MsdfEntry::Empty { .. } => None,
            };
        }
        match build_glyph_msdf(face, key.glyph_id, self.base_em, self.spread) {
            Some(glyph) => {
                let slot = self.pack(glyph);
                self.map.insert(key, MsdfEntry::Slot(slot));
                Some(slot)
            }
            None => {
                let advance = glyph_advance(face, key.glyph_id, self.base_em);
                self.map.insert(key, MsdfEntry::Empty { advance });
                None
            }
        }
    }

    fn pack(&mut self, glyph: MsdfGlyph) -> MsdfSlot {
        let MsdfGlyph {
            rgba,
            width,
            height,
            bearing_x,
            bearing_y,
            advance,
            spread,
        } = glyph;
        let (page_idx, rect) = self.allocate(width, height);
        let page = &mut self.pages[page_idx];
        copy_rgba_into_rgba(&mut page.pixels, page.width, &rect, &rgba);
        merge_dirty(&mut page.dirty, rect);
        MsdfSlot {
            page: page_idx as u32,
            rect,
            bearing_x,
            bearing_y,
            advance,
            spread,
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> (usize, MsdfRect) {
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.allocate(w, h) {
                page.last_used = self.frame;
                return (i, rect);
            }
        }
        // At the page budget, recycle the least-recently-used page that
        // fits the glyph and wasn't referenced this frame (instances
        // recorded this frame point at its UVs). Evicted glyphs that
        // are still on screen re-rasterize on next frame's draw path.
        if self.pages.len() >= self.page_budget
            && let Some(i) = self
                .pages
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.last_used + self.lru_protection_window <= self.frame
                        && p.width >= w
                        && p.height >= h
                })
                .min_by_key(|(_, p)| p.last_used)
                .map(|(i, _)| i)
        {
            self.recycle_page(i);
            let page = &mut self.pages[i];
            let rect = page.allocate(w, h).expect("recycled page fits the glyph");
            page.last_used = self.frame;
            return (i, rect);
        }
        // Grow: past the budget this is the soft-cap escape hatch for
        // frames whose live glyph set genuinely exceeds the budget.
        let new_w = PAGE_SIZE.max(w.next_power_of_two());
        let new_h = PAGE_SIZE.max(h.next_power_of_two());
        let mut page = MsdfAtlasPage::new(new_w, new_h);
        page.last_used = self.frame;
        let rect = page
            .allocate(w, h)
            .expect("freshly-sized page must fit a glyph");
        self.pages.push(page);
        (self.pages.len() - 1, rect)
    }

    /// Reset a page for reuse: forget its glyphs, zero its pixels, and
    /// mark the whole page dirty so backends re-upload their mirror.
    /// The page keeps its index and dimensions, so backend GPU page
    /// arrays stay valid with no API involvement. `Empty` entries
    /// (whitespace advances) occupy no atlas space and are kept.
    fn recycle_page(&mut self, index: usize) {
        let page_idx = index as u32;
        self.map
            .retain(|_, e| !matches!(e, MsdfEntry::Slot(s) if s.page == page_idx));
        let page = &mut self.pages[index];
        page.pixels.fill(0);
        page.shelves.clear();
        page.dirty = Some(MsdfRect {
            x: 0,
            y: 0,
            w: page.width,
            h: page.height,
        });
    }

    #[cfg(test)]
    fn set_page_budget_for_tests(&mut self, budget: usize) {
        self.page_budget = budget;
    }
}

fn copy_rgba_into_rgba(dst: &mut [u8], stride_pixels: u32, rect: &MsdfRect, src_rgba: &[u8]) {
    let dst_row_bytes = stride_pixels as usize * MSDF_BYTES_PER_PIXEL as usize;
    let src_row_bytes = rect.w as usize * 4;
    for row in 0..rect.h as usize {
        let dst_off = (rect.y as usize + row) * dst_row_bytes
            + rect.x as usize * MSDF_BYTES_PER_PIXEL as usize;
        let src_off = row * src_row_bytes;
        let row_bytes = rect.w as usize * 4;
        dst[dst_off..dst_off + row_bytes].copy_from_slice(&src_rgba[src_off..src_off + row_bytes]);
    }
}

fn merge_dirty(dirty: &mut Option<MsdfRect>, rect: MsdfRect) {
    *dirty = Some(match *dirty {
        None => rect,
        Some(prev) => {
            let x = prev.x.min(rect.x);
            let y = prev.y.min(rect.y);
            let r = prev.right().max(rect.right());
            let b = prev.bottom().max(rect.bottom());
            MsdfRect {
                x,
                y,
                w: r - x,
                h: b - y,
            }
        }
    });
}

// The fixture face is Inter, so these tests only build when the `inter`
// font feature is on (it is in the default set).
#[cfg(all(test, feature = "inter"))]
mod tests {
    use super::*;

    fn test_face() -> ttf_parser::Face<'static> {
        ttf_parser::Face::parse(damascene_fonts::INTER_VARIABLE, 0).unwrap()
    }

    fn fake_font_id(seed: u32) -> fontdb::ID {
        let mut db = fontdb::Database::new();
        db.load_font_data(damascene_fonts::INTER_VARIABLE.to_vec());
        let id = db.faces().next().expect("test fontdb has Inter").id;
        let _ = seed;
        id
    }

    fn key(face: &Face<'_>, ch: char) -> MsdfGlyphKey {
        MsdfGlyphKey {
            font: fake_font_id(0),
            glyph_id: face.glyph_index(ch).unwrap().0,
        }
    }

    #[test]
    fn ensure_inserts_glyph_and_marks_dirty() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        let slot = atlas.ensure(key(&face, 'A'), &face).expect("slot");
        assert_eq!(slot.page, 0);
        assert!(slot.rect.w > 0 && slot.rect.h > 0);
        let dirty = atlas.take_dirty();
        assert_eq!(dirty.len(), 1);
        assert!(atlas.take_dirty().is_empty());
    }

    #[test]
    fn ensure_is_idempotent() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        let s1 = atlas.ensure(key(&face, 'A'), &face).unwrap();
        atlas.take_dirty();
        let s2 = atlas.ensure(key(&face, 'A'), &face).unwrap();
        assert_eq!(s1, s2);
        assert!(atlas.take_dirty().is_empty());
    }

    #[test]
    fn whitespace_returns_none_but_caches_advance() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        let space_key = key(&face, ' ');
        assert!(atlas.ensure(space_key, &face).is_none());
        let advance = atlas.advance(space_key).expect("space advance cached");
        assert!(advance > 0.0);
    }

    #[test]
    fn distinct_glyphs_get_distinct_slots() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        let a = atlas.ensure(key(&face, 'A'), &face).unwrap();
        let b = atlas.ensure(key(&face, 'B'), &face).unwrap();
        assert_ne!(a.rect, b.rect);
    }

    #[test]
    fn allocation_recycles_lru_page_at_budget() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        atlas.set_page_budget_for_tests(1);
        let key_a = key(&face, 'A');
        atlas.ensure(key_a, &face).expect("slot");

        // Page 0 was referenced this frame, so a full-page request must
        // grow past the budget rather than clear glyphs whose UVs may
        // already be recorded in this frame's instances.
        let (grown, _) = atlas.allocate(PAGE_SIZE, PAGE_SIZE);
        assert_eq!(grown, 1);
        assert_eq!(atlas.pages().len(), 2);
        assert!(atlas.slot(key_a).is_some());

        // Next frame both pages are stale; the same request recycles
        // the LRU page in place instead of growing further.
        atlas.take_dirty();
        let (recycled, rect) = atlas.allocate(PAGE_SIZE, PAGE_SIZE);
        assert_eq!(recycled, 0);
        assert_eq!(atlas.pages().len(), 2);
        assert_eq!((rect.x, rect.y), (0, 0));
        assert!(atlas.slot(key_a).is_none());
        let dirty = atlas.take_dirty();
        assert!(
            dirty
                .iter()
                .any(|(i, r)| *i == 0 && r.w == PAGE_SIZE && r.h == PAGE_SIZE),
            "expected a full-page dirty rect on page 0, got {dirty:?}"
        );
    }

    #[test]
    fn touch_protects_pages_from_recycling() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        atlas.set_page_budget_for_tests(1);
        let key_a = key(&face, 'A');
        atlas.ensure(key_a, &face).expect("slot");
        atlas.take_dirty();
        // 'A' is drawn again this frame: touch re-stamps its page, so
        // the full-page request must grow rather than recycle it out
        // from under the live glyph.
        assert!(atlas.touch(key_a).is_some());
        let (idx, _) = atlas.allocate(PAGE_SIZE, PAGE_SIZE);
        assert_eq!(idx, 1);
        assert!(atlas.slot(key_a).is_some());
    }

    #[test]
    fn lru_protection_window_spans_multiple_flush_ticks() {
        // Issue #94: with N runners sharing one atlas, the frame
        // counter ticks once per *runner flush*, so a page referenced
        // by runner A's in-flight frame is one tick stale by the time
        // runner B prepares. A protection window of N must keep it.
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        atlas.set_page_budget_for_tests(1);
        atlas.set_lru_protection_window(2);
        let key_a = key(&face, 'A');
        atlas.ensure(key_a, &face).expect("slot");

        // One flush tick (runner A's). Under the default window the
        // page would now be recyclable; with window 2 it must survive
        // runner B's allocation pressure.
        atlas.take_dirty();
        let (idx, _) = atlas.allocate(PAGE_SIZE, PAGE_SIZE);
        assert_eq!(idx, 1, "page must be protected for a second tick");
        assert!(atlas.slot(key_a).is_some());

        // Two ticks later the page is genuinely stale and recycles.
        atlas.take_dirty();
        atlas.take_dirty();
        let (idx, _) = atlas.allocate(PAGE_SIZE, PAGE_SIZE);
        assert!(idx <= 1, "stale page recycles in place");
        assert_eq!(atlas.pages().len(), 3 - 1, "no further growth");
    }

    #[test]
    fn recycling_preserves_empty_advance_entries() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        atlas.set_page_budget_for_tests(1);
        let space = key(&face, ' ');
        let key_a = key(&face, 'A');
        atlas.ensure(space, &face);
        atlas.ensure(key_a, &face).expect("slot");
        atlas.take_dirty();
        // Recycle page 0: the whitespace advance survives (it occupies
        // no atlas space), only the packed glyph is forgotten.
        let (idx, _) = atlas.allocate(PAGE_SIZE, PAGE_SIZE);
        assert_eq!(idx, 0);
        assert!(atlas.slot(key_a).is_none());
        assert!(atlas.advance(space).is_some());
    }

    #[test]
    fn shelf_packer_fits_a_typical_run_in_one_page() {
        let face = test_face();
        let mut atlas = MsdfAtlas::default();
        let font = fake_font_id(0);
        for ch in "The quick brown fox jumps over the lazy dog 0123456789".chars() {
            atlas.ensure(
                MsdfGlyphKey {
                    font,
                    glyph_id: face.glyph_index(ch).map(|g| g.0).unwrap_or(0),
                },
                &face,
            );
        }
        assert_eq!(atlas.pages().len(), 1);
    }
}
