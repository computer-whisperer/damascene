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
//!
//! Sprite size is normalised, not proportional to the source view box:
//! every asset rasterises with its longer view-box side mapped to
//! [`REFERENCE_VIEW_DIM`] × `px_per_unit` atlas pixels (64 px at the
//! defaults), whatever units it was authored in. A lucide icon
//! (24-unit box) and a logo exported in millimetres (500-unit box)
//! therefore cost the same to build and occupy the same atlas area
//! (issue #146 — unnormalised, the latter built a 1371² sprite that
//! forced a fresh 2048² page and stalled the first frame for seconds).
//! The per-slot [`IconMsdfSlot::px_per_unit`] carries the effective
//! density so the UV / spread math stays exact.
//!
//! One exception to the 64 px target: a distance field cannot represent
//! features thinner than a texel, so an asset whose strokes are thin
//! *relative to its view box* (a 4-unit stroke in a 320-unit box maps
//! to 0.8 px) would drop out into disconnected blobs. The density is
//! therefore floored so the asset's thinnest stroke resolves to at
//! least [`MIN_STROKE_SPRITE_PX`] atlas pixels, capped at a
//! [`MAX_SPRITE_DIM`]-px longer side so the #146 bound still holds.
//! Sprites only grow for assets that need it; thin *fill* features
//! (necks, slivers) are not measured and keep the 64 px target.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::collections::HashMap;

use super::msdf::{IconMsdf, build_icon_msdf};
use super::svg::IconSource;
use crate::tree::IconName;

/// Default atlas pixels per view-box unit *at the reference view box*
/// ([`REFERENCE_VIEW_DIM`]). 64 px/(24 unit view box) ≈ 2.67 px/unit
/// gives ~64-pixel sprites, which is sharp enough for the 16–48 px UI
/// sizes we care about. Assets with other view-box sizes are rescaled
/// so their longer side also lands on 64 px (see the module docs).
pub const DEFAULT_PX_PER_UNIT: f64 = 64.0 / 24.0;
/// View-box extent (in source units) at which `px_per_unit` applies
/// verbatim — lucide's 24-unit box. Every asset's longer view-box side
/// is mapped to `REFERENCE_VIEW_DIM × px_per_unit` atlas pixels, so
/// sprite size is independent of the units an asset was authored in.
pub const REFERENCE_VIEW_DIM: f64 = 24.0;
/// Default MTSDF spread radius in atlas pixels.
pub const DEFAULT_SPREAD: f64 = 6.0;
/// Default baked stroke width in source view-box units (lucide).
pub const DEFAULT_STROKE_WIDTH: f64 = 2.0;
/// Minimum atlas pixels the thinnest stroke of an asset must span in
/// its rasterised MTSDF. Below ~1 px a stroke is sub-texel and the
/// field drops out into disconnected blobs; 3 px keeps round caps and
/// joins intact when the sprite is magnified on screen.
pub const MIN_STROKE_SPRITE_PX: f64 = 3.0;
/// Hard bound on a sprite's longer side in atlas pixels when the
/// stroke floor raises the density above the 64 px target. Keeps the
/// issue-#146 guarantee that no asset can stall the frame or blow the
/// 1024² pages, whatever units it was authored in.
pub const MAX_SPRITE_DIM: f64 = 256.0;

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
    /// A built-in icon, keyed by its [`IconName`] discriminant.
    Builtin(IconName),
    /// An app-supplied [`crate::SvgIcon`], keyed by its SVG-source content hash.
    Custom(u64),
    /// A programmatic [`crate::vector::VectorAsset`], keyed by its structural content hash.
    Vector(u64),
}

impl IconKey {
    /// Key for an [`IconSource`]: built-ins by name, custom SVGs by
    /// content hash.
    pub fn from_source(source: &IconSource) -> Self {
        match source {
            IconSource::Builtin(name) => IconKey::Builtin(*name),
            IconSource::Custom(svg) => IconKey::Custom(svg.content_hash()),
            // Unknown names paint AlertCircle; key them to it so they
            // share the atlas slot rather than churning a new entry.
            IconSource::UnknownName(_) => IconKey::Builtin(crate::tree::IconName::AlertCircle),
        }
    }
}

/// Atlas cache key: one slot per `(icon, quantised stroke width)` pair.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct IconMsdfKey {
    /// Identity of the vector icon source.
    pub icon: IconKey,
    /// Stroke width quantised to 0.25-unit steps (so 2.0 → 8, 1.5 → 6).
    pub stroke_q: u16,
}

impl IconMsdfKey {
    /// Build a key from an icon source and an unquantised stroke width
    /// (in source view-box units), quantising to 0.25-unit steps.
    pub fn new(source: &IconSource, stroke_width: f32) -> Self {
        let q = ((stroke_width.max(0.25) * 4.0).round() as i32).clamp(1, u16::MAX as i32) as u16;
        Self {
            icon: IconKey::from_source(source),
            stroke_q: q,
        }
    }

    /// The quantised stroke width in source view-box units (`stroke_q / 4`).
    pub fn stroke_width(&self) -> f32 {
        self.stroke_q as f32 / 4.0
    }
}

/// An axis-aligned pixel rectangle within an atlas page.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IconRect {
    /// Left edge in atlas pixels.
    pub x: u32,
    /// Top edge in atlas pixels.
    pub y: u32,
    /// Width in atlas pixels.
    pub w: u32,
    /// Height in atlas pixels.
    pub h: u32,
}

impl IconRect {
    /// Exclusive right edge (`x + w`) in atlas pixels.
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    /// Exclusive bottom edge (`y + h`) in atlas pixels.
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// Where one icon's MTSDF lives in the atlas, plus the metrics a
/// backend needs to place and shade its quad.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct IconMsdfSlot {
    /// Index of the atlas page holding this icon (see [`IconMsdfAtlas::page`]).
    pub page: u32,
    /// Pixel rectangle of the MTSDF within that page.
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

/// One RGBA8 atlas texture page (RGB = MSDF, A = true SDF) plus its
/// shelf-packing state. Backends upload `pixels` to a GPU texture and
/// re-upload the regions reported by [`IconMsdfAtlas::take_dirty`].
pub struct IconMsdfPage {
    /// Page width in pixels.
    pub width: u32,
    /// Page height in pixels.
    pub height: u32,
    /// RGBA8 pixel data, row-major, `width * height * 4` bytes.
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

/// The icon MTSDF atlas: caches one rasterized MTSDF per
/// [`IconMsdfKey`], shelf-packed into RGBA8 pages. See the module docs
/// for keying and stroke-width semantics.
pub struct IconMsdfAtlas {
    pages: Vec<IconMsdfPage>,
    map: HashMap<IconMsdfKey, Option<IconMsdfSlot>>,
    px_per_unit: f64,
    spread: f64,
    /// Run fdsm's MTSDF error-correction pass per icon. Off by default
    /// (see [`Self::set_error_correction`]).
    error_correction: bool,
}

impl Default for IconMsdfAtlas {
    fn default() -> Self {
        Self::new(DEFAULT_PX_PER_UNIT, DEFAULT_SPREAD)
    }
}

impl IconMsdfAtlas {
    /// Empty atlas with one initial page. `px_per_unit` is the
    /// rasterisation density at the [`REFERENCE_VIEW_DIM`] view box
    /// (atlas pixels per source unit for a 24-unit asset, see
    /// [`DEFAULT_PX_PER_UNIT`]) — every sprite's longer side comes out
    /// at `REFERENCE_VIEW_DIM × px_per_unit` pixels; `spread` is the
    /// MTSDF half-range in atlas pixels (see [`DEFAULT_SPREAD`]).
    pub fn new(px_per_unit: f64, spread: f64) -> Self {
        Self {
            pages: vec![IconMsdfPage::new(PAGE_SIZE, PAGE_SIZE)],
            map: HashMap::new(),
            px_per_unit,
            spread,
            error_correction: false,
        }
    }

    /// Enable or disable fdsm's MTSDF error-correction pass (default
    /// off). The shader's alpha-channel SDF fallback already masks the
    /// artifacts it targets; leaving it off saves ~22% of per-icon
    /// generation cost. Only affects icons rasterized after the call.
    pub fn set_error_correction(&mut self, on: bool) {
        self.error_correction = on;
    }

    /// Atlas pixels per source view-box unit at the
    /// [`REFERENCE_VIEW_DIM`] view box. The density actually used for
    /// a given asset is scaled by `REFERENCE_VIEW_DIM / max(vw, vh)`
    /// (see [`Self::sprite_px_per_unit`]).
    pub fn px_per_unit(&self) -> f64 {
        self.px_per_unit
    }

    /// Effective rasterisation density for an asset with `view_box`:
    /// the longer view-box side maps to `REFERENCE_VIEW_DIM ×
    /// px_per_unit` atlas pixels (64 at the defaults), so sprite size
    /// is bounded regardless of authoring units. Degenerate boxes
    /// return the base density unchanged and are rejected downstream
    /// by [`build_icon_msdf`].
    pub fn sprite_px_per_unit(&self, view_box: [f32; 4]) -> f64 {
        let long_side = f64::from(view_box[2].max(view_box[3]));
        if long_side > 0.0 && long_side.is_finite() {
            self.px_per_unit * REFERENCE_VIEW_DIM / long_side
        } else {
            self.px_per_unit
        }
    }

    /// [`Self::sprite_px_per_unit`] with the thin-stroke floor applied
    /// (see the module docs): if the asset's thinnest stroke would span
    /// fewer than [`MIN_STROKE_SPRITE_PX`] atlas pixels at the
    /// icon-normalised density, the density rises until it does — or
    /// until the sprite's longer side hits [`MAX_SPRITE_DIM`],
    /// whichever comes first. `current_color_stroke_width` stands in
    /// for `currentColor` strokes, whose width lives outside the asset
    /// (mirrors `build_icon_msdf`).
    pub fn sprite_px_per_unit_for_asset(
        &self,
        asset: &crate::vector::VectorAsset,
        current_color_stroke_width: f64,
    ) -> f64 {
        let base = self.sprite_px_per_unit(asset.view_box);
        let long_side = f64::from(asset.view_box[2].max(asset.view_box[3]));
        if long_side <= 0.0 || !long_side.is_finite() {
            return base;
        }
        let min_stroke = asset
            .paths
            .iter()
            .filter_map(|p| p.stroke.as_ref())
            .map(|s| {
                if matches!(s.color, crate::vector::VectorColor::CurrentColor) {
                    current_color_stroke_width
                } else {
                    f64::from(s.width)
                }
            })
            .filter(|w| *w > 0.0 && w.is_finite())
            .fold(f64::INFINITY, f64::min);
        if !min_stroke.is_finite() {
            return base;
        }
        let floor = MIN_STROKE_SPRITE_PX / min_stroke;
        let cap = MAX_SPRITE_DIM / long_side;
        base.max(floor.min(cap))
    }

    /// MTSDF spread radius in atlas pixels used when rasterising icons.
    pub fn spread(&self) -> f64 {
        self.spread
    }

    /// All atlas pages, indexed by [`IconMsdfSlot::page`].
    pub fn pages(&self) -> &[IconMsdfPage] {
        &self.pages
    }

    /// The page at `index` ([`IconMsdfSlot::page`]), if it exists.
    pub fn page(&self, index: u32) -> Option<&IconMsdfPage> {
        self.pages.get(index as usize)
    }

    /// Look up an already-rasterised slot without generating anything.
    /// `None` if the key was never ensured or produced no contours.
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
            self.sprite_px_per_unit_for_asset(asset, key.stroke_width() as f64),
            self.spread,
            key.stroke_width() as f64,
            self.error_correction,
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
    ///
    /// The sprite is icon-sized whatever the asset's view box (see
    /// [`Self::sprite_px_per_unit`]): mask mode is an icon-class
    /// fidelity path, so a logo authored in millimetres builds as
    /// cheaply as a lucide glyph instead of stalling the first frame
    /// on a page-sized rasterisation (issue #146).
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
        // SVG `clip-path` regions resolve geometrically at sprite
        // tolerance before rasterisation. The cache key above hashes
        // the clip table, so clipped and unclipped assets never share
        // a slot. Density is chosen from the pre-flatten asset — its
        // strokes are still strokes there, so the thin-stroke floor
        // sees them.
        let px_per_unit = self.sprite_px_per_unit_for_asset(asset, 1.0);
        let flattened;
        let asset = if asset.has_clips() {
            flattened = asset.flatten_clips((0.25 / px_per_unit.max(f64::EPSILON)) as f32, 1.0);
            &flattened
        } else {
            asset
        };
        // The default-stroke-width parameter is only consulted by
        // `build_icon_msdf` for paths whose stroke is `currentColor`.
        // Programmatic `VectorAsset`s express their stroke width
        // explicitly on each `VectorStroke`, so this default is
        // unused — the value 1.0 is just a sane fallback.
        let msdf = build_icon_msdf(asset, px_per_unit, self.spread, 1.0, self.error_correction);
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

    /// Sprite long side for an asset whose view box is `dim` units
    /// square, built through the vector-asset path.
    fn vector_sprite(dim: f32) -> (IconMsdfSlot, usize) {
        use crate::vector::{
            VectorAsset, VectorColor, VectorFill, VectorFillRule, VectorPath, VectorSegment,
        };
        // A filled diamond spanning the box; shape is irrelevant, only
        // the view-box extent matters for sizing.
        let c = dim / 2.0;
        let path = VectorPath {
            segments: vec![
                VectorSegment::MoveTo([c, 0.0]),
                VectorSegment::LineTo([dim, c]),
                VectorSegment::LineTo([c, dim]),
                VectorSegment::LineTo([0.0, c]),
                VectorSegment::Close,
            ],
            fill: Some(VectorFill {
                color: VectorColor::CurrentColor,
                opacity: 1.0,
                rule: VectorFillRule::NonZero,
            }),
            stroke: None,
            clip: None,
        };
        let asset = VectorAsset::from_paths([0.0, 0.0, dim, dim], vec![path]);
        let mut atlas = IconMsdfAtlas::default();
        let slot = atlas.ensure_vector_asset(&asset).expect("slot");
        (slot, atlas.pages().len())
    }

    // Issue #146: sprite size must not scale with the authoring units.
    #[test]
    fn large_view_box_vector_asset_builds_an_icon_sized_sprite() {
        let (slot, pages) = vector_sprite(509.5);
        let (reference, _) = vector_sprite(24.0);
        assert_eq!(
            (slot.rect.w, slot.rect.h),
            (reference.rect.w, reference.rect.h),
            "509.5-unit box must rasterise to the same sprite as a 24-unit one"
        );
        // 64 px + 2 × 6 px spread = 76 — never a page-sized sprite.
        assert!(slot.rect.w <= 80 && slot.rect.h <= 80, "{:?}", slot.rect);
        assert_eq!(pages, 1, "must not spill onto a fresh page");
        // The slot reports the effective density so UV math stays exact.
        assert!(
            (f64::from(slot.px_per_unit) - DEFAULT_PX_PER_UNIT * 24.0 / 509.5).abs() < 1e-4,
            "{}",
            slot.px_per_unit
        );
    }

    #[test]
    fn small_view_box_assets_scale_up_to_the_reference_sprite() {
        let (small, _) = vector_sprite(16.0);
        let (reference, _) = vector_sprite(24.0);
        assert_eq!(
            (small.rect.w, small.rect.h),
            (reference.rect.w, reference.rect.h)
        );
    }

    /// Sprite for a `w × h`-unit box crossed by a single solid stroke
    /// of `stroke_w` units, built through the vector-asset path.
    fn stroked_vector_sprite(w: f32, h: f32, stroke_w: f32) -> IconMsdfSlot {
        use crate::vector::{
            VectorAsset, VectorColor, VectorLineCap, VectorLineJoin, VectorPath, VectorSegment,
            VectorStroke,
        };
        let path = VectorPath {
            segments: vec![
                VectorSegment::MoveTo([0.0, h * 0.5]),
                VectorSegment::LineTo([w, h * 0.5]),
            ],
            fill: None,
            stroke: Some(VectorStroke {
                color: VectorColor::Solid(crate::Color::srgb_u8(255, 255, 255)),
                opacity: 1.0,
                width: stroke_w,
                line_cap: VectorLineCap::Round,
                line_join: VectorLineJoin::Round,
                miter_limit: 4.0,
            }),
            clip: None,
        };
        let asset = VectorAsset::from_paths([0.0, 0.0, w, h], vec![path]);
        let mut atlas = IconMsdfAtlas::default();
        atlas.ensure_vector_asset(&asset).expect("slot")
    }

    // The hero-sparkline regression: a 4-unit stroke in a 320-unit box
    // is 0.8 px at the icon-normalised density — sub-texel, so the
    // field dropped out into disconnected blobs.
    #[test]
    fn thin_strokes_floor_the_sprite_density() {
        let slot = stroked_vector_sprite(320.0, 96.0, 4.0);
        // Floor: 3 px / 4 units = 0.75 px/unit (under the 256-px cap).
        assert!(
            (f64::from(slot.px_per_unit) - MIN_STROKE_SPRITE_PX / 4.0).abs() < 1e-4,
            "{}",
            slot.px_per_unit
        );
        // 320 × 0.75 + 2 × 6 spread = 252 px wide.
        assert!(slot.rect.w >= 240 && slot.rect.w <= 260, "{:?}", slot.rect);
    }

    #[test]
    fn the_stroke_floor_respects_the_sprite_cap() {
        // A 0.5-unit hairline in a 320-unit box asks for 6 px/unit —
        // a 1932-px sprite. The cap wins: 256 px / 320 units = 0.8.
        let slot = stroked_vector_sprite(320.0, 96.0, 0.5);
        assert!(
            (f64::from(slot.px_per_unit) - MAX_SPRITE_DIM / 320.0).abs() < 1e-4,
            "{}",
            slot.px_per_unit
        );
        assert!(slot.rect.w <= 270, "{:?}", slot.rect);
    }

    #[test]
    fn icon_class_strokes_keep_the_reference_density() {
        // Lucide-shaped: a 2-unit stroke in a 24-unit box is 5.3 px at
        // the reference density — the floor must not touch it.
        let slot = stroked_vector_sprite(24.0, 24.0, 2.0);
        assert!(
            (f64::from(slot.px_per_unit) - DEFAULT_PX_PER_UNIT).abs() < 1e-4,
            "{}",
            slot.px_per_unit
        );
        assert!(slot.rect.w <= 80 && slot.rect.h <= 80, "{:?}", slot.rect);
    }

    #[test]
    fn custom_svg_icons_normalise_too() {
        use crate::SvgIcon;
        const BIG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 960 720"><circle cx="480" cy="360" r="300" fill="#ff0000"/></svg>"##;
        let custom = IconSource::Custom(SvgIcon::parse(BIG).unwrap());
        let mut atlas = IconMsdfAtlas::default();
        let slot = atlas.ensure(&custom, 2.0).unwrap();
        // Long side (960) → 64 px; short side scales with the aspect.
        assert!(slot.rect.w <= 80, "{:?}", slot.rect);
        assert!(slot.rect.h < slot.rect.w, "{:?}", slot.rect);
        assert_eq!(atlas.pages().len(), 1);
    }

    #[test]
    fn builtin_density_is_unchanged_by_normalisation() {
        let atlas = IconMsdfAtlas::default();
        let d = atlas.sprite_px_per_unit([0.0, 0.0, 24.0, 24.0]);
        assert!((d - DEFAULT_PX_PER_UNIT).abs() < 1e-12);
        assert_eq!(
            atlas.sprite_px_per_unit([0.0, 0.0, 0.0, 0.0]),
            DEFAULT_PX_PER_UNIT
        );
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
