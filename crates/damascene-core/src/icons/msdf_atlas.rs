//! Icon MTSDF atlas — backs the MSDF icon rendering path.
//!
//! Each `(IconKey, base_px_per_unit, stroke_width_q)` slot caches one
//! pre-rasterized MTSDF (RGB = MSDF, A = true single-channel SDF).
//! Pages are RGBA8 — same format the text MTSDF atlas uses, so a
//! backend can spin up the same texture/sampler layout for both.
//!
//! Built-in icons key on the [`IconName`] discriminant; app-supplied
//! [`crate::SvgIcon`]s key on their content hash, so the same SVG used
//! at multiple sites shares one atlas slot.
//!
//! Stroke width is baked into the MSDF at generation time and quantised
//! to 0.25-px steps so we don't blow up the atlas if every record() call
//! passes a slightly different width. Most callers use the default
//! lucide stroke (2.0), so the quantisation rarely matters in practice.

use std::collections::HashMap;

use super::msdf::{IconMsdf, build_icon_msdf};
use super::svg::IconSource;
use crate::tree::IconName;

/// Default atlas pixels per source view-box unit. 64 px/(24 unit
/// view box) ≈ 2.67 px/unit gives ~64-pixel icons, which is sharp
/// enough for the 16–48 px UI sizes we care about.
pub const DEFAULT_PX_PER_UNIT: f64 = 64.0 / 24.0;
/// Default MTSDF spread radius in atlas pixels.
pub const DEFAULT_SPREAD: f64 = 6.0;
/// Default baked stroke width in source view-box units (lucide).
pub const DEFAULT_STROKE_WIDTH: f64 = 2.0;

const PAGE_SIZE: u32 = 1024;
const ICON_PADDING: u32 = 2;
const BYTES_PER_PIXEL: u32 = 4;

/// Identity for a unique vector icon source. Built-ins enumerate;
/// custom SVGs are keyed by their SVG-source content hash; programmatic
/// `VectorAsset`s (used by [`crate::tree::vector`]) are keyed by their
/// structural [`crate::vector::VectorAsset::content_hash`]. Three
/// disjoint variants prevent the (vanishingly unlikely) case where an
/// SVG-text hash coincides with a structural-asset hash from
/// referencing the wrong slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum IconKey {
    Builtin(IconName),
    Custom(u64),
    Vector(u64),
}

impl IconKey {
    pub fn from_source(source: &IconSource) -> Self {
        match source {
            IconSource::Builtin(name) => IconKey::Builtin(*name),
            IconSource::Custom(svg) => IconKey::Custom(svg.content_hash()),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct IconMsdfKey {
    pub icon: IconKey,
    /// Stroke width quantised to 0.25-unit steps (so 2.0 → 8, 1.5 → 6).
    pub stroke_q: u16,
}

impl IconMsdfKey {
    pub fn new(source: &IconSource, stroke_width: f32) -> Self {
        let q = ((stroke_width.max(0.25) * 4.0).round() as i32).clamp(1, u16::MAX as i32) as u16;
        Self {
            icon: IconKey::from_source(source),
            stroke_q: q,
        }
    }

    pub fn stroke_width(&self) -> f32 {
        self.stroke_q as f32 / 4.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IconRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl IconRect {
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct IconMsdfSlot {
    pub page: u32,
    pub rect: IconRect,
    /// Source view box `[vx, vy, vw, vh]` of the icon — caller maps a
    /// destination rect of size `[dw, dh]` to logical px and uses these
    /// to expand by the spread margin.
    pub view_box: [f32; 4],
    /// Atlas pixels per view-box unit.
    pub px_per_unit: f32,
    /// MTSDF spread in atlas pixels.
    pub spread: f32,
}

impl IconMsdfSlot {
    /// Logical-pixel size of the spread margin given the icon's
    /// destination rect width (in logical px).
    pub fn spread_logical(&self, dest_w_logical: f32) -> f32 {
        let logical_per_unit = dest_w_logical / self.view_box[2].max(0.001);
        self.spread * logical_per_unit / self.px_per_unit.max(0.001)
    }
}

#[derive(Copy, Clone)]
struct Shelf {
    y_top: u32,
    height: u32,
    cursor: u32,
}

pub struct IconMsdfPage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    dirty: Option<IconRect>,
    shelves: Vec<Shelf>,
}

impl IconMsdfPage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * BYTES_PER_PIXEL) as usize],
            dirty: None,
            shelves: Vec::new(),
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<IconRect> {
        if w > self.width || h > self.height {
            return None;
        }
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
            let rect = IconRect {
                x: shelf.cursor,
                y: shelf.y_top,
                w,
                h,
            };
            shelf.cursor += w + ICON_PADDING;
            return Some(rect);
        }
        let next_y = self
            .shelves
            .last()
            .map(|s| s.y_top + s.height + ICON_PADDING)
            .unwrap_or(0);
        if next_y + h > self.height {
            return None;
        }
        self.shelves.push(Shelf {
            y_top: next_y,
            height: h,
            cursor: w + ICON_PADDING,
        });
        Some(IconRect {
            x: 0,
            y: next_y,
            w,
            h,
        })
    }
}

pub struct IconMsdfAtlas {
    pages: Vec<IconMsdfPage>,
    map: HashMap<IconMsdfKey, Option<IconMsdfSlot>>,
    px_per_unit: f64,
    spread: f64,
}

impl Default for IconMsdfAtlas {
    fn default() -> Self {
        Self::new(DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD)
    }
}

impl IconMsdfAtlas {
    pub fn new(px_per_unit: f64, spread: f64) -> Self {
        Self {
            pages: vec![IconMsdfPage::new(PAGE_SIZE, PAGE_SIZE)],
            map: HashMap::new(),
            px_per_unit,
            spread,
        }
    }

    pub fn px_per_unit(&self) -> f64 {
        self.px_per_unit
    }

    pub fn spread(&self) -> f64 {
        self.spread
    }

    pub fn pages(&self) -> &[IconMsdfPage] {
        &self.pages
    }

    pub fn page(&self, index: u32) -> Option<&IconMsdfPage> {
        self.pages.get(index as usize)
    }

    pub fn slot(&self, key: IconMsdfKey) -> Option<IconMsdfSlot> {
        self.map.get(&key).copied().flatten()
    }

    /// Rasterise (or look up) the icon's MTSDF and return its slot.
    /// `None` is cached for icons that produce no renderable contours.
    pub fn ensure(&mut self, source: &IconSource, stroke_width: f32) -> Option<IconMsdfSlot> {
        let key = IconMsdfKey::new(source, stroke_width);
        if let Some(entry) = self.map.get(&key) {
            return *entry;
        }
        let asset = source.vector_asset();
        let msdf = build_icon_msdf(
            asset,
            self.px_per_unit,
            self.spread,
            key.stroke_width() as f64,
        );
        let slot = msdf.map(|m| self.pack(m));
        self.map.insert(key, slot);
        slot
    }

    /// Rasterise (or look up) the MTSDF for an app-supplied
    /// [`crate::vector::VectorAsset`] being rendered as an explicit mask
    /// and return its slot. The asset's structural content hash is the
    /// cache key — apps that build the same shape twice share one slot.
    /// Stroke width and other style participate in the hash, so a single
    /// asset has one canonical MTSDF; varying styles produce distinct
    /// slots automatically without per-call quantisation.
    pub fn ensure_vector_asset(
        &mut self,
        asset: &crate::vector::VectorAsset,
    ) -> Option<IconMsdfSlot> {
        let key = IconMsdfKey {
            icon: IconKey::Vector(asset.content_hash()),
            // Stroke width is encoded in the asset's content hash, so
            // the per-key `stroke_q` is unused for vector assets. Pin
            // to 0 so identical assets share one slot.
            stroke_q: 0,
        };
        if let Some(entry) = self.map.get(&key) {
            return *entry;
        }
        // The default-stroke-width parameter is only consulted by
        // `build_icon_msdf` for paths whose stroke is `currentColor`.
        // Programmatic `VectorAsset`s express their stroke width
        // explicitly on each `VectorStroke`, so this default is
        // unused — the value 1.0 is just a sane fallback.
        let msdf = build_icon_msdf(asset, self.px_per_unit, self.spread, 1.0);
        let slot = msdf.map(|m| self.pack(m));
        self.map.insert(key, slot);
        slot
    }

    /// Drain dirty regions since the last call (one per page that has
    /// pending uploads).
    pub fn take_dirty(&mut self) -> Vec<(usize, IconRect)> {
        let mut out = Vec::new();
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.dirty.take() {
                out.push((i, rect));
            }
        }
        out
    }

    fn pack(&mut self, icon: IconMsdf) -> IconMsdfSlot {
        let IconMsdf {
            rgba,
            width,
            height,
            spread,
            px_per_unit,
            view_box,
        } = icon;
        let (page_idx, rect) = self.allocate(width, height);
        let page = &mut self.pages[page_idx];
        copy_rgba_into_rgba(&mut page.pixels, page.width, &rect, &rgba);
        merge_dirty(&mut page.dirty, rect);
        IconMsdfSlot {
            page: page_idx as u32,
            rect,
            view_box,
            px_per_unit,
            spread,
        }
    }

    fn allocate(&mut self, w: u32, h: u32) -> (usize, IconRect) {
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.allocate(w, h) {
                return (i, rect);
            }
        }
        let new_w = PAGE_SIZE.max(w.next_power_of_two());
        let new_h = PAGE_SIZE.max(h.next_power_of_two());
        let mut page = IconMsdfPage::new(new_w, new_h);
        let rect = page
            .allocate(w, h)
            .expect("freshly-sized page must fit the icon");
        self.pages.push(page);
        (self.pages.len() - 1, rect)
    }
}

fn copy_rgba_into_rgba(dst: &mut [u8], stride_pixels: u32, rect: &IconRect, src_rgba: &[u8]) {
    let dst_row_bytes = stride_pixels as usize * BYTES_PER_PIXEL as usize;
    let src_row_bytes = rect.w as usize * 4;
    for row in 0..rect.h as usize {
        let dst_off =
            (rect.y as usize + row) * dst_row_bytes + rect.x as usize * BYTES_PER_PIXEL as usize;
        let src_off = row * src_row_bytes;
        dst[dst_off..dst_off + src_row_bytes]
            .copy_from_slice(&src_rgba[src_off..src_off + src_row_bytes]);
    }
}

fn merge_dirty(dirty: &mut Option<IconRect>, rect: IconRect) {
    *dirty = Some(match *dirty {
        None => rect,
        Some(prev) => {
            let x = prev.x.min(rect.x);
            let y = prev.y.min(rect.y);
            let r = prev.right().max(rect.right());
            let b = prev.bottom().max(rect.bottom());
            IconRect {
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

    fn builtin(name: IconName) -> IconSource {
        IconSource::Builtin(name)
    }

    #[test]
    fn ensure_packs_x_into_first_page() {
        let mut atlas = IconMsdfAtlas::default();
        let slot = atlas.ensure(&builtin(IconName::X), 2.0).expect("X slot");
        assert_eq!(slot.page, 0);
        assert!(slot.rect.w > 0 && slot.rect.h > 0);
        assert_eq!(slot.view_box, [0.0, 0.0, 24.0, 24.0]);
    }

    #[test]
    fn ensure_is_idempotent() {
        let mut atlas = IconMsdfAtlas::default();
        let src = builtin(IconName::Settings);
        let s1 = atlas.ensure(&src, 2.0).unwrap();
        atlas.take_dirty();
        let s2 = atlas.ensure(&src, 2.0).unwrap();
        assert_eq!(s1, s2);
        assert!(atlas.take_dirty().is_empty());
    }

    #[test]
    fn distinct_icons_get_distinct_slots() {
        let mut atlas = IconMsdfAtlas::default();
        let a = atlas.ensure(&builtin(IconName::X), 2.0).unwrap();
        let b = atlas.ensure(&builtin(IconName::Check), 2.0).unwrap();
        assert_ne!(a.rect, b.rect);
    }

    #[test]
    fn different_stroke_widths_get_distinct_slots() {
        let mut atlas = IconMsdfAtlas::default();
        let thin = atlas.ensure(&builtin(IconName::X), 1.0).unwrap();
        let thick = atlas.ensure(&builtin(IconName::X), 3.0).unwrap();
        assert_ne!(thin.rect, thick.rect);
    }

    #[test]
    fn custom_svg_dedups_by_content_hash() {
        use crate::SvgIcon;
        const CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="#ff0000"/></svg>"##;
        let a = IconSource::Custom(SvgIcon::parse(CIRCLE).unwrap());
        let b = IconSource::Custom(SvgIcon::parse(CIRCLE).unwrap());
        let mut atlas = IconMsdfAtlas::default();
        let sa = atlas.ensure(&a, 2.0).unwrap();
        let sb = atlas.ensure(&b, 2.0).unwrap();
        assert_eq!(sa, sb, "same SVG bytes must share an atlas slot");
    }

    #[test]
    fn custom_svg_distinct_from_builtin_with_same_view_box() {
        use crate::SvgIcon;
        const CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="#ff0000"/></svg>"##;
        let custom = IconSource::Custom(SvgIcon::parse(CIRCLE).unwrap());
        let mut atlas = IconMsdfAtlas::default();
        let sa = atlas.ensure(&builtin(IconName::X), 2.0).unwrap();
        let sb = atlas.ensure(&custom, 2.0).unwrap();
        assert_ne!(sa.rect, sb.rect);
    }

    #[test]
    fn stroke_quantisation_round_trip() {
        let src = builtin(IconName::X);
        let k = IconMsdfKey::new(&src, 2.0);
        assert!((k.stroke_width() - 2.0).abs() < 1e-6);
        let k = IconMsdfKey::new(&src, 1.7);
        // 1.7 * 4 = 6.8 → rounds to 7 → 1.75
        assert!((k.stroke_width() - 1.75).abs() < 1e-6);
    }

    #[test]
    fn spread_logical_scales_with_dest_size() {
        let mut atlas = IconMsdfAtlas::default();
        let slot = atlas.ensure(&builtin(IconName::X), 2.0).unwrap();
        // dest 24 logical px equals 1 logical px per unit. Spread of 6 atlas
        // px at ~2.67 atlas-px-per-unit ≈ 2.25 logical px.
        let s = slot.spread_logical(24.0);
        assert!(s > 2.0 && s < 2.5, "{s}");
        // Doubling dest doubles spread in logical px.
        let s2 = slot.spread_logical(48.0);
        assert!((s2 - 2.0 * s).abs() < 1e-3, "s={s} s2={s2}");
    }
}
