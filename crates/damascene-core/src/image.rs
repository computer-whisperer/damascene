//! App-supplied raster images.
//!
//! Apps construct an [`Image`] once (typically as a `LazyLock` over a
//! decoded byte slice) and embed it in the tree via the [`crate::image`]
//! builder. Identity is content-hashed: two `Image`s built from the same
//! pixels share a backend texture-cache slot. Cloning is a cheap `Arc`
//! bump.
//!
//! ```
//! use std::sync::LazyLock;
//! use damascene_core::prelude::*;
//!
//! static AVATAR: LazyLock<Image> = LazyLock::new(|| {
//!     // 2x2 RGBA8 placeholder. Real apps decode PNG/JPEG once in
//!     // their LazyLock body — `damascene-core` deliberately does not pull
//!     // in image-decoding crates.
//!     Image::from_rgba8(
//!         2, 2,
//!         vec![
//!             0xff, 0x00, 0x00, 0xff,  0x00, 0xff, 0x00, 0xff,
//!             0x00, 0x00, 0xff, 0xff,  0xff, 0xff, 0xff, 0xff,
//!         ],
//!     )
//! });
//!
//! fn cell() -> El {
//!     image(AVATAR.clone()).image_fit(ImageFit::Cover).radius(8.0)
//! }
//! ```
//!
//! Decoding (`png`, `jpeg`, etc.) is intentionally the app's
//! responsibility — keeps `damascene-core` free of heavy media deps and
//! lets each app pick its own decoder + colour-space pipeline.
//!
//! ## Color management
//!
//! Every `Image` carries a [`ColorSpace`] tag describing how its channel
//! values map to light — like an ICC-tagged image in a browser. The
//! plain [`from_rgba8`](Image::from_rgba8) constructor tags
//! [`ColorSpace::SRGB`] (matching the web's untagged-image convention);
//! the `*_in` constructors accept wide-gamut and HDR sources:
//!
//! ```
//! use damascene_core::color::ColorSpace;
//! use damascene_core::image::Image;
//!
//! // A Display-P3 JPEG decoded to 8-bit RGBA:
//! let p3 = Image::from_rgba8_in(
//!     ColorSpace::DISPLAY_P3, 1, 1, vec![0xff, 0x00, 0x00, 0xff],
//! );
//! // A linear float HDR source (EXR, Radiance, …):
//! let hdr = Image::from_rgba_f32_in(
//!     ColorSpace::SCRGB_LINEAR, 1, 1, vec![4.0, 4.0, 4.0, 1.0],
//! );
//! # let _ = (p3, hdr);
//! ```
//!
//! Backends upload 8-bit sRGB images directly (hardware decodes on
//! sample) and normalize everything else through
//! [`to_scrgb_f16`](Image::to_scrgb_f16) onto an extended-range float
//! texture, so wide-gamut and HDR pixels survive to the swapchain
//! losslessly when the surface is extended-range (see
//! `docs/COLOR_MANAGEMENT.md`); on SDR surfaces out-of-range values
//! gamut-clip at the target.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::color::{ColorSpace, Primaries, decode_transfer, primaries_matrix};
use crate::tree::Rect;

fn mat3_mul_vec3(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Channel layout + width of an [`Image`]'s pixel buffer. Always RGBA
/// interleaved, top-left origin, row-major; variants differ in the
/// per-channel encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// 8-bit unsigned normalized per channel — 4 bytes per pixel.
    Rgba8,
    /// 16-bit unsigned normalized per channel — 8 bytes per pixel
    /// (e.g. 16-bit PNG).
    Rgba16,
    /// IEEE 754 half float per channel — 8 bytes per pixel.
    RgbaF16,
    /// f32 per channel — 16 bytes per pixel (e.g. EXR, Radiance HDR).
    RgbaF32,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgba8 => 4,
            PixelFormat::Rgba16 | PixelFormat::RgbaF16 => 8,
            PixelFormat::RgbaF32 => 16,
        }
    }
}

/// A raster image. RGBA pixels (see [`PixelFormat`]) tagged with the
/// [`ColorSpace`] they were authored in; top-left origin, row-major.
/// Cheap `Arc`-backed clone; backends key their texture cache off
/// [`Self::content_hash`] so two equal `Image`s share a GPU slot.
#[derive(Clone)]
pub struct Image {
    inner: Arc<ImageInner>,
}

struct ImageInner {
    /// Raw pixel bytes, native-endian, `width * height *
    /// format.bytes_per_pixel()` long.
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    format: PixelFormat,
    color_space: ColorSpace,
    content_hash: u64,
}

impl Image {
    /// Build from sRGB-encoded RGBA8 pixels — the common case for
    /// decoded PNG/JPEG art. Panics if `pixels.len() != width * height *
    /// 4`. Untagged 8-bit sources should use this (the web's convention
    /// for untagged images is sRGB).
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self::from_rgba8_in(ColorSpace::SRGB, width, height, pixels)
    }

    /// Build from RGBA8 pixels authored in `space` — e.g.
    /// [`ColorSpace::DISPLAY_P3`] for a P3-tagged JPEG. Panics if
    /// `pixels.len() != width * height * 4`.
    pub fn from_rgba8_in(space: ColorSpace, width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self::new_raw(PixelFormat::Rgba8, space, width, height, pixels)
    }

    /// Build from 16-bit unsigned-normalized RGBA pixels authored in
    /// `space` — e.g. a 16-bit PNG. Panics if `pixels.len() != width *
    /// height * 4` (u16 channel values, not bytes).
    pub fn from_rgba16_in(space: ColorSpace, width: u32, height: u32, pixels: Vec<u16>) -> Self {
        Self::check_channel_count("from_rgba16_in", "u16", width, height, pixels.len());
        let bytes = pixels.iter().flat_map(|v| v.to_ne_bytes()).collect();
        Self::new_raw(PixelFormat::Rgba16, space, width, height, bytes)
    }

    /// Build from half-float RGBA pixels given as raw IEEE 754 bit
    /// patterns (the shape most decoders hand f16 data in) authored in
    /// `space`. Panics if `bits.len() != width * height * 4`.
    pub fn from_rgba_f16_bits_in(
        space: ColorSpace,
        width: u32,
        height: u32,
        bits: Vec<u16>,
    ) -> Self {
        Self::check_channel_count(
            "from_rgba_f16_bits_in",
            "f16-bit",
            width,
            height,
            bits.len(),
        );
        let bytes = bits.iter().flat_map(|v| v.to_ne_bytes()).collect();
        Self::new_raw(PixelFormat::RgbaF16, space, width, height, bytes)
    }

    /// Build from f32 RGBA pixels authored in `space` — e.g. a decoded
    /// EXR in [`ColorSpace::SCRGB_LINEAR`]. Panics if `pixels.len() !=
    /// width * height * 4`.
    pub fn from_rgba_f32_in(space: ColorSpace, width: u32, height: u32, pixels: Vec<f32>) -> Self {
        Self::check_channel_count("from_rgba_f32_in", "f32", width, height, pixels.len());
        let bytes = pixels.iter().flat_map(|v| v.to_ne_bytes()).collect();
        Self::new_raw(PixelFormat::RgbaF32, space, width, height, bytes)
    }

    /// Validate a typed constructor's channel-value count so the panic
    /// message speaks in the caller's units (channel values, not bytes —
    /// `new_raw`'s byte assert backstops the internal paths).
    fn check_channel_count(ctor: &str, unit: &str, width: u32, height: u32, got: usize) {
        let expected = (width as usize) * (height as usize) * 4;
        assert_eq!(
            got, expected,
            "Image::{ctor}: expected {expected} {unit} channel values ({width}x{height} RGBA), got {got}",
        );
    }

    fn new_raw(
        format: PixelFormat,
        space: ColorSpace,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Self {
        let expected = (width as usize) * (height as usize) * format.bytes_per_pixel();
        assert_eq!(
            pixels.len(),
            expected,
            "Image: expected {expected} bytes ({width}x{height} {format:?}), got {}",
            pixels.len(),
        );
        let mut h = DefaultHasher::new();
        width.hash(&mut h);
        height.hash(&mut h);
        format.hash(&mut h);
        space.hash(&mut h);
        pixels.hash(&mut h);
        let content_hash = h.finish();
        Self {
            inner: Arc::new(ImageInner {
                pixels,
                width,
                height,
                format,
                color_space: space,
                content_hash,
            }),
        }
    }

    pub fn width(&self) -> u32 {
        self.inner.width
    }

    pub fn height(&self) -> u32 {
        self.inner.height
    }

    pub fn format(&self) -> PixelFormat {
        self.inner.format
    }

    /// The color space the pixel values were authored in.
    pub fn color_space(&self) -> ColorSpace {
        self.inner.color_space
    }

    /// Raw pixel bytes, length `width * height *
    /// format().bytes_per_pixel()`, native-endian. Top-left origin.
    pub fn pixels(&self) -> &[u8] {
        &self.inner.pixels
    }

    /// True when the pixel buffer can upload directly to an 8-bit sRGB
    /// texture and let the sampler decode — RGBA8 in the default
    /// [`ColorSpace::SRGB`]. Everything else goes through
    /// [`Self::to_scrgb_f16`].
    pub fn is_srgb8(&self) -> bool {
        self.inner.format == PixelFormat::Rgba8 && self.inner.color_space == ColorSpace::SRGB
    }

    /// Convert to linear sRGB-primaries extended-range ("scRGB")
    /// half-float pixels for GPU upload: RGBA interleaved, `width *
    /// height * 4` raw f16 bit patterns, alpha unchanged (straight, not
    /// premultiplied — the image shader premultiplies at blend).
    ///
    /// This is the working-space representation every renderer
    /// composites in, so sampling needs no further conversion.
    /// Wide-gamut primaries land outside `[0, 1]` and HDR brights above
    /// `1.0`; both survive on float textures. Transfer functions follow
    /// the same conventions as [`crate::color::Color`] conversion —
    /// notably PQ decodes with `1.0 = 10000 nits` and no reference-
    /// luminance rescale (see `ColorSpace::BT2020_PQ`).
    pub fn to_scrgb_f16(&self) -> Vec<u16> {
        let inner = &*self.inner;
        let tf = inner.color_space.transfer;
        let matrix = (inner.color_space.primaries != Primaries::Srgb)
            .then(|| primaries_matrix(inner.color_space.primaries, Primaries::Srgb));
        let px = (inner.width as usize) * (inner.height as usize);
        let mut out = Vec::with_capacity(px * 4);

        // Stream pixels as f32 RGBA in source encoding, decode the TF
        // (LUT for the integer formats), change primaries, encode f16.
        let mut push = |rgba: [f32; 4]| {
            let lin = match matrix {
                Some(m) => mat3_mul_vec3(m, [rgba[0], rgba[1], rgba[2]]),
                None => [rgba[0], rgba[1], rgba[2]],
            };
            out.push(half::f16::from_f32(lin[0]).to_bits());
            out.push(half::f16::from_f32(lin[1]).to_bits());
            out.push(half::f16::from_f32(lin[2]).to_bits());
            out.push(half::f16::from_f32(rgba[3]).to_bits());
        };

        match inner.format {
            PixelFormat::Rgba8 => {
                let lut: Vec<f32> = (0..=255u32)
                    .map(|v| decode_transfer(v as f32 / 255.0, tf))
                    .collect();
                for p in inner.pixels.chunks_exact(4) {
                    push([
                        lut[p[0] as usize],
                        lut[p[1] as usize],
                        lut[p[2] as usize],
                        p[3] as f32 / 255.0,
                    ]);
                }
            }
            PixelFormat::Rgba16 => {
                let lut: Vec<f32> = (0..=65535u32)
                    .map(|v| decode_transfer(v as f32 / 65535.0, tf))
                    .collect();
                for p in inner.pixels.chunks_exact(8) {
                    let ch = |i: usize| u16::from_ne_bytes([p[i * 2], p[i * 2 + 1]]) as usize;
                    push([lut[ch(0)], lut[ch(1)], lut[ch(2)], ch(3) as f32 / 65535.0]);
                }
            }
            PixelFormat::RgbaF16 => {
                for p in inner.pixels.chunks_exact(8) {
                    let ch = |i: usize| {
                        half::f16::from_bits(u16::from_ne_bytes([p[i * 2], p[i * 2 + 1]])).to_f32()
                    };
                    push([
                        decode_transfer(ch(0), tf),
                        decode_transfer(ch(1), tf),
                        decode_transfer(ch(2), tf),
                        ch(3),
                    ]);
                }
            }
            PixelFormat::RgbaF32 => {
                for p in inner.pixels.chunks_exact(16) {
                    let ch = |i: usize| {
                        f32::from_ne_bytes([p[i * 4], p[i * 4 + 1], p[i * 4 + 2], p[i * 4 + 3]])
                    };
                    push([
                        decode_transfer(ch(0), tf),
                        decode_transfer(ch(1), tf),
                        decode_transfer(ch(2), tf),
                        ch(3),
                    ]);
                }
            }
        }
        out
    }

    /// Stable hash of `(width, height, format, color_space, pixels)`.
    /// Backends use this as the key into their per-image texture cache.
    pub fn content_hash(&self) -> u64 {
        self.inner.content_hash
    }

    /// Short hex label for inspection / dump output, e.g.
    /// `"image:1a2b3c4d"`.
    pub fn label(&self) -> String {
        format!("image:{:08x}", self.inner.content_hash as u32)
    }
}

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        // Arc identity → fast path. Fallback to content hash so two
        // independently constructed `Image`s with equal pixels still
        // compare equal (matches `SvgIcon`'s hash-driven identity).
        Arc::ptr_eq(&self.inner, &other.inner)
            || self.inner.content_hash == other.inner.content_hash
    }
}

impl Eq for Image {}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.inner.width)
            .field("height", &self.inner.height)
            .field("format", &self.inner.format)
            .field("color_space", &self.inner.color_space)
            .field(
                "content_hash",
                &format_args!("{:016x}", self.inner.content_hash),
            )
            .finish()
    }
}

/// How a raster image projects into the rect resolved for its El.
/// Mirrors CSS `object-fit`. The El rect (after `padding`) is the
/// "viewport"; the image is the "content".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageFit {
    /// Scale uniformly so the image fits inside the rect, preserving
    /// aspect ratio. Letterbox bands appear on the side that runs
    /// short. Default — matches the CSS default for `<img>` in most
    /// frameworks.
    #[default]
    Contain,
    /// Scale uniformly so the image covers the rect, preserving aspect
    /// ratio. Excess on the longer axis is clipped via the El's
    /// scissor (the destination rect can extend past the El's content
    /// area; `draw_ops` clips it back).
    Cover,
    /// Stretch the image to the rect, ignoring aspect ratio.
    Fill,
    /// No scaling — paint at the image's natural pixel size, anchored
    /// top-left within the rect. Excess clips via the scissor.
    None,
}

impl ImageFit {
    /// Project an image of natural size `(nw, nh)` into `rect` according
    /// to this fit. The returned rect is where the image should paint;
    /// for `Cover` / `None` it may extend past `rect` and the caller
    /// is expected to scissor-clip to `rect`.
    pub fn project(self, nw: u32, nh: u32, rect: Rect) -> Rect {
        let nw = (nw as f32).max(1.0);
        let nh = (nh as f32).max(1.0);
        match self {
            ImageFit::Fill => rect,
            ImageFit::None => Rect::new(rect.x, rect.y, nw, nh),
            ImageFit::Contain => {
                let scale = (rect.w / nw).min(rect.h / nh).max(0.0);
                let w = nw * scale;
                let h = nh * scale;
                Rect::new(
                    rect.x + (rect.w - w) * 0.5,
                    rect.y + (rect.h - h) * 0.5,
                    w,
                    h,
                )
            }
            ImageFit::Cover => {
                let scale = (rect.w / nw).max(rect.h / nh).max(0.0);
                let w = nw * scale;
                let h = nh * scale;
                Rect::new(
                    rect.x + (rect.w - w) * 0.5,
                    rect.y + (rect.h - h) * 0.5,
                    w,
                    h,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(w: u32, h: u32, byte: u8) -> Vec<u8> {
        vec![byte; (w as usize) * (h as usize) * 4]
    }

    #[test]
    fn from_rgba8_validates_buffer_length() {
        let _ = Image::from_rgba8(2, 2, rgba(2, 2, 0));
    }

    #[test]
    #[should_panic(expected = "expected 16 bytes")]
    fn from_rgba8_panics_on_size_mismatch() {
        let _ = Image::from_rgba8(2, 2, vec![0; 12]);
    }

    #[test]
    fn equal_pixels_share_content_hash() {
        let a = Image::from_rgba8(4, 4, rgba(4, 4, 0xab));
        let b = Image::from_rgba8(4, 4, rgba(4, 4, 0xab));
        assert_eq!(a.content_hash(), b.content_hash());
        assert_eq!(a, b);
    }

    #[test]
    fn different_pixels_get_distinct_hash() {
        let a = Image::from_rgba8(2, 2, rgba(2, 2, 0x00));
        let b = Image::from_rgba8(2, 2, rgba(2, 2, 0xff));
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn same_pixels_different_space_get_distinct_hash() {
        let a = Image::from_rgba8(2, 2, rgba(2, 2, 0xab));
        let b = Image::from_rgba8_in(ColorSpace::DISPLAY_P3, 2, 2, rgba(2, 2, 0xab));
        assert_ne!(a.content_hash(), b.content_hash());
        assert_ne!(a, b);
    }

    #[test]
    fn srgb8_fast_path_predicate() {
        assert!(Image::from_rgba8(1, 1, rgba(1, 1, 0)).is_srgb8());
        assert!(!Image::from_rgba8_in(ColorSpace::DISPLAY_P3, 1, 1, rgba(1, 1, 0)).is_srgb8());
        assert!(!Image::from_rgba_f32_in(ColorSpace::SCRGB_LINEAR, 1, 1, vec![0.0; 4]).is_srgb8());
    }

    fn f16_val(bits: u16) -> f32 {
        half::f16::from_bits(bits).to_f32()
    }

    #[test]
    fn scrgb_conversion_decodes_srgb_tf() {
        // sRGB 0xbc (188) ≈ 0.5 linear.
        let img = Image::from_rgba8(1, 1, vec![188, 188, 188, 255]);
        let out = img.to_scrgb_f16();
        assert_eq!(out.len(), 4);
        for c in &out[..3] {
            assert!((f16_val(*c) - 0.5).abs() < 0.01, "got {}", f16_val(*c));
        }
        assert!((f16_val(out[3]) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn scrgb_conversion_preserves_white_across_primaries() {
        // Pure white is white in every D65 space — the primaries matrix
        // must preserve it.
        let img = Image::from_rgba8_in(ColorSpace::DISPLAY_P3, 1, 1, vec![255, 255, 255, 255]);
        let out = img.to_scrgb_f16();
        for c in &out[..3] {
            assert!((f16_val(*c) - 1.0).abs() < 0.01, "got {}", f16_val(*c));
        }
    }

    #[test]
    fn scrgb_conversion_maps_p3_red_out_of_gamut() {
        // P3 pure red lies outside sRGB: r > 1, g < 0 in scRGB.
        let img = Image::from_rgba8_in(ColorSpace::DISPLAY_P3, 1, 1, vec![255, 0, 0, 255]);
        let out = img.to_scrgb_f16();
        let (r, g) = (f16_val(out[0]), f16_val(out[1]));
        assert!(r > 1.0, "P3 red r = {r}, expected > 1");
        assert!(g < 0.0, "P3 red g = {g}, expected < 0");
    }

    #[test]
    fn scrgb_conversion_passes_linear_floats_through() {
        // HDR-bright scRGB float input is already in the target space —
        // values above 1.0 must survive untouched.
        let img =
            Image::from_rgba_f32_in(ColorSpace::SCRGB_LINEAR, 1, 1, vec![4.0, 0.25, 1.0, 0.5]);
        let out = img.to_scrgb_f16();
        assert!((f16_val(out[0]) - 4.0).abs() < 0.01);
        assert!((f16_val(out[1]) - 0.25).abs() < 0.001);
        assert!((f16_val(out[2]) - 1.0).abs() < 0.001);
        assert!((f16_val(out[3]) - 0.5).abs() < 0.001);
    }

    #[test]
    fn scrgb_conversion_handles_rgba16_and_f16_bits() {
        // 16-bit mid-gray in linear space: 0.5 exactly.
        let half_u16 = (0.5f32 * 65535.0) as u16;
        let img = Image::from_rgba16_in(
            ColorSpace::SRGB_LINEAR,
            1,
            1,
            vec![half_u16, half_u16, half_u16, 65535],
        );
        let out = img.to_scrgb_f16();
        assert!(
            (f16_val(out[0]) - 0.5).abs() < 0.001,
            "got {}",
            f16_val(out[0])
        );

        // f16 bit-pattern round trip through a linear space is identity.
        let bits = half::f16::from_f32(2.5).to_bits();
        let img = Image::from_rgba_f16_bits_in(
            ColorSpace::SCRGB_LINEAR,
            1,
            1,
            vec![bits, bits, bits, half::f16::from_f32(1.0).to_bits()],
        );
        let out = img.to_scrgb_f16();
        assert!((f16_val(out[0]) - 2.5).abs() < 0.01);
    }

    #[test]
    fn fit_contain_letterboxes_horizontally() {
        // 200x100 image into 400x400 rect: contain → 400x200 centred.
        let r = ImageFit::Contain.project(200, 100, Rect::new(0.0, 0.0, 400.0, 400.0));
        assert!((r.w - 400.0).abs() < 0.01);
        assert!((r.h - 200.0).abs() < 0.01);
        assert!((r.x - 0.0).abs() < 0.01);
        assert!((r.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn fit_cover_overflows_horizontally() {
        // 100x200 image into 400x400 rect: cover → 400x800 centred —
        // overflow above and below the rect, scissor crops.
        let r = ImageFit::Cover.project(100, 200, Rect::new(0.0, 0.0, 400.0, 400.0));
        assert!((r.w - 400.0).abs() < 0.01);
        assert!((r.h - 800.0).abs() < 0.01);
        assert!((r.y + 200.0).abs() < 0.01);
    }

    #[test]
    fn fit_fill_stretches() {
        let r = ImageFit::Fill.project(100, 200, Rect::new(10.0, 20.0, 300.0, 50.0));
        assert_eq!(r, Rect::new(10.0, 20.0, 300.0, 50.0));
    }

    #[test]
    fn fit_none_uses_natural_size() {
        let r = ImageFit::None.project(64, 32, Rect::new(10.0, 20.0, 400.0, 400.0));
        assert_eq!(r, Rect::new(10.0, 20.0, 64.0, 32.0));
    }
}
