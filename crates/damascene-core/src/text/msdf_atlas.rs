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

/// Inter-glyph padding (atlas pixels) so neighbour MSDF gradients don't
/// bleed under bilinear filtering.
const GLYPH_PADDING: u32 = 2;

/// Bytes per atlas pixel — RGBA8 (RGB = MSDF distance channels, A=255).
pub const MSDF_BYTES_PER_PIXEL: u32 = 4;

/// Atlas key — outline glyphs are size-independent.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct MsdfGlyphKey {
    pub font: fontdb::ID,
    pub glyph_id: u16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MsdfRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl MsdfRect {
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// Where a cached MSDF glyph lives, plus the metrics the recorder needs
/// to place its quad.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MsdfSlot {
    pub page: u32,
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

pub struct MsdfAtlasPage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8.
    pub pixels: Vec<u8>,
    dirty: Option<MsdfRect>,
    shelves: Vec<Shelf>,
}

impl MsdfAtlasPage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * MSDF_BYTES_PER_PIXEL) as usize],
            dirty: None,
            shelves: Vec::new(),
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
    pub fn new(base_em: u32, spread: f64) -> Self {
        Self {
            pages: vec![MsdfAtlasPage::new(PAGE_SIZE, PAGE_SIZE)],
            map: HashMap::new(),
            base_em,
            spread,
        }
    }

    pub fn base_em(&self) -> u32 {
        self.base_em
    }

    pub fn spread(&self) -> f64 {
        self.spread
    }

    pub fn pages(&self) -> &[MsdfAtlasPage] {
        &self.pages
    }

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
    pub fn take_dirty(&mut self) -> Vec<(usize, MsdfRect)> {
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
                MsdfEntry::Slot(s) => Some(*s),
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
                return (i, rect);
            }
        }
        let new_w = PAGE_SIZE.max(w.next_power_of_two());
        let new_h = PAGE_SIZE.max(h.next_power_of_two());
        let mut page = MsdfAtlasPage::new(new_w, new_h);
        let rect = page
            .allocate(w, h)
            .expect("freshly-sized page must fit a glyph");
        self.pages.push(page);
        (self.pages.len() - 1, rect)
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

#[cfg(test)]
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
