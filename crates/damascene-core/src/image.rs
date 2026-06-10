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
//! `docs/COLOR_MANAGEMENT.md`); on SDR surfaces out-of-gamut chroma
//! clips at the target while over-bright luminance rolls off
//! gracefully (see [`DynamicRangeLimit`]).
//!
//! The luminance contract in one line: **a pixel at the source's
//! reference white displays at the output's reference white.** PQ
//! sources are anchored by their tagged
//! [`reference_luminance_nits`](crate::color::ColorSpace::reference_luminance_nits)
//! (203 for [`ColorSpace::BT2020_PQ`] per BT.2408 — override the field
//! if your master is graded differently); everything brighter is HDR
//! headroom, remastered per [`DynamicRangeLimit`]. See
//! [`to_scrgb_f16`](Image::to_scrgb_f16) for the full statement.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::color::{ColorSpace, Primaries, TransferFunction, decode_transfer, primaries_matrix};
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
    /// Bytes per RGBA pixel in this format (4, 8, or 16).
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

    /// Image width in pixels.
    pub fn width(&self) -> u32 {
        self.inner.width
    }

    /// Image height in pixels.
    pub fn height(&self) -> u32 {
        self.inner.height
    }

    /// Channel encoding of the pixel buffer.
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
    /// `1.0`; both survive on float textures.
    ///
    /// ## Luminance contract
    ///
    /// Working-space `1.0` displays at the output's reference white
    /// (the renderer scales to the swapchain's encoding, e.g.
    /// `white_scale` on scRGB). Relative transfers (sRGB, gamma,
    /// linear) already encode `1.0` = reference white and convert
    /// as-is. PQ is absolute (signal 1.0 = 10000 nits), so this
    /// conversion anchors it: a pixel at the source's
    /// [`reference_luminance_nits`](ColorSpace::reference_luminance_nits)
    /// (203 for [`ColorSpace::BT2020_PQ`], per BT.2408) converts to
    /// working-space `1.0`, and a 1000-nit highlight lands at ~4.9× —
    /// HDR headroom the per-image remaster grades into the panel's
    /// volume (see [`DynamicRangeLimit`]). HLG is scene-referred and
    /// currently decodes without an OOTF or anchoring — its contract
    /// is still open. Note [`crate::color::Color`] conversion does
    /// *not* anchor PQ (UI colors stay encoding-literal); the anchor
    /// is an image-pipeline behavior.
    pub fn to_scrgb_f16(&self) -> Vec<u16> {
        self.to_scrgb_f16_with_peak().0
    }

    /// [`Self::to_scrgb_f16`] plus the image's measured content peak:
    /// the maximum linear RGB channel value over all pixels, in
    /// working-space units (`1.0` = reference white). For a still image
    /// this is its effective MaxCLL — backends cache it per texture and
    /// feed it to the luminance remaster (see
    /// [`DynamicRangeLimit`] and `docs/COLOR_MANAGEMENT.md`). Alpha is
    /// ignored (the remaster runs on straight rgb before the blend
    /// premultiply). Non-finite channel values are skipped.
    pub fn to_scrgb_f16_with_peak(&self) -> (Vec<u16>, f32) {
        let inner = &*self.inner;
        let tf = inner.color_space.transfer;
        let matrix = (inner.color_space.primaries != Primaries::Srgb)
            .then(|| primaries_matrix(inner.color_space.primaries, Primaries::Srgb));
        // PQ decodes to absolute luminance (1.0 = 10000 nits); anchor it
        // so the source's reference white lands at working-space 1.0.
        // Relative transfers already put reference white at 1.0. HLG is
        // scene-referred — its anchoring (OOTF) is still open, see
        // docs/COLOR_MANAGEMENT.md.
        let lum_scale = match tf {
            TransferFunction::Pq => {
                let r = inner.color_space.reference_luminance_nits;
                debug_assert!(
                    r > 0.0,
                    "Image::to_scrgb_f16: PQ source tagged with \
                     non-positive reference_luminance_nits ({r}); the \
                     reference white anchors absolute PQ luminance into \
                     the working space"
                );
                10_000.0 / r
            }
            _ => 1.0,
        };
        let px = (inner.width as usize) * (inner.height as usize);
        let mut out = Vec::with_capacity(px * 4);
        let mut peak = 0.0f32;

        // Stream pixels as f32 RGBA in source encoding, decode the TF
        // (LUT for the integer formats), change primaries, encode f16.
        let mut push = |rgba: [f32; 4]| {
            let lin = match matrix {
                Some(m) => mat3_mul_vec3(m, [rgba[0], rgba[1], rgba[2]]),
                None => [rgba[0], rgba[1], rgba[2]],
            };
            let lin = [lin[0] * lum_scale, lin[1] * lum_scale, lin[2] * lum_scale];
            for c in lin {
                // `max` drops NaN; the finite check drops +inf (a half
                // float bit pattern decoders do produce).
                if c.is_finite() {
                    peak = peak.max(c);
                }
            }
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
        (out, peak)
    }

    /// Return this image downscaled so its larger side fits `max_dim`,
    /// or a cheap (`Arc`-bump) clone when it already fits. An image
    /// larger than the device's `max_*_dimension_2d` (8192 by default;
    /// 4096 on some adapters) would fail GPU texture creation and panic
    /// the process — content the user merely clicked on. Every backend
    /// calls this with its device limit before upload (issue #78).
    ///
    /// When it downscales, the resample runs in linear scRGB f16
    /// working space (box-average, each source pixel read once) so
    /// colour averages correctly and HDR brights `>1.0` survive; the
    /// result is an `RgbaF16` / [`ColorSpace::SCRGB_LINEAR`] image
    /// regardless of source format, so an oversized 8-bit sRGB source
    /// uploads through the f16 path — the memory-for-simplicity trade
    /// of a single resampler. Aspect ratio is preserved, so a source
    /// oversized on one axis shrinks on both. Logs a `warn` on the
    /// downscale; the fit case is silent.
    pub fn downscaled_to_fit(&self, max_dim: u32) -> Image {
        let (w, h) = (self.inner.width, self.inner.height);
        if max_dim == 0 || w.max(h) <= max_dim {
            return self.clone();
        }
        let (bits, nw, nh) = downscale_scrgb_f16(&self.to_scrgb_f16(), w, h, max_dim);
        log::warn!(
            "image {w}x{h} exceeds the GPU texture dimension limit ({max_dim}); \
             downscaled to {nw}x{nh} to avoid a create_texture validation failure"
        );
        Image::from_rgba_f16_bits_in(ColorSpace::SCRGB_LINEAR, nw, nh, bits)
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

/// Box-average downscale of interleaved scRGB f16 RGBA pixels (raw f16
/// bit patterns, `w * h * 4` long) so the larger side fits `max_dim`,
/// preserving aspect ratio. Returns the resampled bits and the new
/// dimensions. Averaging is in linear working space (the f16 values
/// already are linear), so colour and `>1.0` brights survive; alpha
/// averages straight. Each source pixel is read exactly once — the cost
/// is one pass over the original. Only called when downscaling
/// (`max_dim < max(w, h)`), so every target maps to ≥1 source pixel.
fn downscale_scrgb_f16(bits: &[u16], w: u32, h: u32, max_dim: u32) -> (Vec<u16>, u32, u32) {
    let scale = f64::from(max_dim) / f64::from(w.max(h));
    let nw = ((f64::from(w) * scale).floor() as u32).clamp(1, max_dim);
    let nh = ((f64::from(h) * scale).floor() as u32).clamp(1, max_dim);

    let decode = |x: u32, y: u32, c: usize| -> f32 {
        let idx = (y as usize * w as usize + x as usize) * 4 + c;
        half::f16::from_bits(bits[idx]).to_f32()
    };
    // Source-pixel span [s0, s1) covering target index `t` of `n` over a
    // source extent of `src`. With `src >= n` (downscale) it's non-empty.
    let span = |t: u32, n: u32, src: u32| {
        let s0 = (u64::from(t) * u64::from(src) / u64::from(n)) as u32;
        let s1 = ((u64::from(t + 1) * u64::from(src) / u64::from(n)) as u32).max(s0 + 1);
        (s0, s1.min(src))
    };

    let mut out = Vec::with_capacity(nw as usize * nh as usize * 4);
    for ty in 0..nh {
        let (sy0, sy1) = span(ty, nh, h);
        for tx in 0..nw {
            let (sx0, sx1) = span(tx, nw, w);
            let mut acc = [0.0f64; 4];
            let mut n = 0.0f64;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += f64::from(decode(sx, sy, c));
                    }
                    n += 1.0;
                }
            }
            for a in acc {
                out.push(half::f16::from_f32((a / n) as f32).to_bits());
            }
        }
    }
    (out, nw, nh)
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

/// How much of the output's HDR headroom an image draw may use.
/// Mirrors CSS `dynamic-range-limit`.
///
/// Backends remaster image content whose measured peak exceeds the
/// resolved limit: a hue-preserving BT.2390 roll-off maps the image's
/// luminance range into the limit at sample time, re-derived live when
/// the output's headroom changes (window moves, HDR toggles). Content
/// that already fits renders untouched — ordinary SDR art never pays
/// for this. See `docs/COLOR_MANAGEMENT.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DynamicRangeLimit {
    /// Tonemap to SDR: the image may not exceed reference white.
    Standard,
    /// Bright but bounded: at most [`Self::CONSTRAINED_HIGH_HEADROOM`]×
    /// reference white (less when the output offers less). For grids /
    /// feeds of HDR content where full-blast highlights would be
    /// hostile. (CSS `constrained-high`; the exact ceiling is
    /// UA-defined there too.)
    ConstrainedHigh,
    /// Use the output's full headroom — remaster only what the panel
    /// cannot show. Default, matching the CSS initial value.
    #[default]
    NoLimit,
}

impl DynamicRangeLimit {
    /// The `ConstrainedHigh` headroom ceiling, in multiples of
    /// reference white.
    pub const CONSTRAINED_HIGH_HEADROOM: f32 = 2.0;

    /// Resolve to a luminance limit in working-space units (multiples
    /// of reference white), given the output's available `headroom`
    /// (`target_max / reference`, `1.0` on SDR, `f32::INFINITY` when
    /// the output declared no maximum).
    pub fn resolve(self, headroom: f32) -> f32 {
        let headroom = headroom.max(1.0);
        match self {
            DynamicRangeLimit::Standard => 1.0,
            DynamicRangeLimit::ConstrainedHigh => headroom.min(Self::CONSTRAINED_HIGH_HEADROOM),
            DynamicRangeLimit::NoLimit => headroom,
        }
    }
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
    fn pq_anchors_reference_white_to_working_one() {
        // PQ encode of 203 nits (the BT.2408 reference white that
        // BT2020_PQ carries) ≈ signal 0.5807. After anchoring it must
        // land at working-space 1.0 — i.e. display at the output's
        // reference white, not 203/10000 = dark.
        let img = Image::from_rgba_f32_in(
            ColorSpace::BT2020_PQ,
            1,
            1,
            vec![0.5807, 0.5807, 0.5807, 1.0],
        );
        let (out, peak) = img.to_scrgb_f16_with_peak();
        for c in &out[..3] {
            assert!((f16_val(*c) - 1.0).abs() < 0.02, "got {}", f16_val(*c));
        }
        assert!((peak - 1.0).abs() < 0.02, "got {peak}");
    }

    #[test]
    fn pq_peak_signal_lands_at_headroom_above_reference() {
        // Signal 1.0 = 10000 nits → 10000/203 ≈ 49.3× reference white.
        // The peak must measure post-anchor so the remaster grades it.
        let img = Image::from_rgba_f32_in(ColorSpace::BT2020_PQ, 1, 1, vec![1.0, 1.0, 1.0, 1.0]);
        let (out, peak) = img.to_scrgb_f16_with_peak();
        let expected = 10_000.0 / 203.0;
        assert!(
            (f16_val(out[0]) - expected).abs() / expected < 0.01,
            "got {}",
            f16_val(out[0])
        );
        assert!((peak - expected).abs() / expected < 0.01, "got {peak}");
    }

    #[test]
    fn pq_anchor_honors_overridden_reference_white() {
        // A master graded to 100-nit diffuse white anchors there.
        let space = ColorSpace {
            reference_luminance_nits: 100.0,
            ..ColorSpace::BT2020_PQ
        };
        // PQ encode of 100 nits ≈ signal 0.5081.
        let img = Image::from_rgba_f32_in(space, 1, 1, vec![0.5081, 0.5081, 0.5081, 1.0]);
        let (out, _) = img.to_scrgb_f16_with_peak();
        assert!(
            (f16_val(out[0]) - 1.0).abs() < 0.02,
            "got {}",
            f16_val(out[0])
        );
    }

    #[test]
    fn measured_peak_is_max_linear_channel() {
        // SDR sources peak at most 1.0; HDR floats report their real max.
        let (_, peak) = Image::from_rgba8(1, 1, vec![255, 128, 0, 255]).to_scrgb_f16_with_peak();
        assert!((peak - 1.0).abs() < 1e-3, "got {peak}");

        let img = Image::from_rgba_f32_in(
            ColorSpace::SCRGB_LINEAR,
            2,
            1,
            vec![0.5, 0.5, 0.5, 1.0, 3.75, 0.25, 1.0, 0.5],
        );
        let (_, peak) = img.to_scrgb_f16_with_peak();
        assert!((peak - 3.75).abs() < 0.01, "got {peak}");
    }

    #[test]
    fn measured_peak_skips_non_finite() {
        let img = Image::from_rgba_f32_in(
            ColorSpace::SCRGB_LINEAR,
            1,
            1,
            vec![f32::NAN, f32::INFINITY, 2.0, 1.0],
        );
        let (_, peak) = img.to_scrgb_f16_with_peak();
        assert!((peak - 2.0).abs() < 0.01, "got {peak}");
    }

    #[test]
    fn dynamic_range_limit_resolves_against_headroom() {
        use DynamicRangeLimit::*;
        // 1000-nit panel at 203-nit reference ≈ 4.93× headroom.
        let h = 1000.0 / 203.0;
        assert_eq!(Standard.resolve(h), 1.0);
        assert_eq!(ConstrainedHigh.resolve(h), 2.0);
        assert_eq!(NoLimit.resolve(h), h);
        // SDR: everything collapses to 1.0.
        assert_eq!(NoLimit.resolve(1.0), 1.0);
        assert_eq!(ConstrainedHigh.resolve(1.0), 1.0);
        // No declared maximum: NoLimit never remasters.
        assert_eq!(NoLimit.resolve(f32::INFINITY), f32::INFINITY);
        // Sub-1.0 (bogus) headroom clamps up.
        assert_eq!(NoLimit.resolve(0.5), 1.0);
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

    #[test]
    fn downscaled_to_fit_is_a_cheap_clone_when_within_limit() {
        let img = Image::from_rgba8(4, 4, rgba(4, 4, 0xab));
        let fitted = img.downscaled_to_fit(8);
        // Shares the Arc — same content, untouched format/space.
        assert_eq!(fitted, img);
        assert_eq!(fitted.format(), PixelFormat::Rgba8);
        assert_eq!(fitted.color_space(), ColorSpace::SRGB);
        assert_eq!((fitted.width(), fitted.height()), (4, 4));
        // max_dim == 0 is a no-op too (never happens with real limits).
        assert_eq!(img.downscaled_to_fit(0), img);
    }

    #[test]
    fn downscaled_to_fit_shrinks_oversized_preserving_aspect() {
        // 12x4 → max_dim 6: scale 0.5 → 6x2, routed to scRGB f16.
        let img = Image::from_rgba8(12, 4, rgba(12, 4, 0xff));
        let fitted = img.downscaled_to_fit(6);
        assert_eq!((fitted.width(), fitted.height()), (6, 2));
        assert_eq!(fitted.format(), PixelFormat::RgbaF16);
        assert_eq!(fitted.color_space(), ColorSpace::SCRGB_LINEAR);
    }

    #[test]
    fn downscaled_to_fit_preserves_solid_colour() {
        // Solid sRGB mid-gray (0xbc ≈ 0.5 linear): every averaged output
        // pixel reproduces it once decoded back to working space.
        let img = Image::from_rgba8(16, 16, rgba(16, 16, 0xbc));
        let fitted = img.downscaled_to_fit(4);
        assert_eq!((fitted.width(), fitted.height()), (4, 4));
        let out = fitted.to_scrgb_f16();
        assert_eq!(out.len(), 4 * 4 * 4);
        for px in out.chunks_exact(4) {
            for c in &px[..3] {
                assert!((f16_val(*c) - 0.5).abs() < 0.02, "got {}", f16_val(*c));
            }
            // The helper fills all four channels with 0xbc, so alpha is
            // 188/255 straight (not transfer-decoded like rgb).
            assert!((f16_val(px[3]) - 188.0 / 255.0).abs() < 0.02);
        }
    }
}
