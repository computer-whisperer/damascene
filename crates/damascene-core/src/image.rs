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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::tree::Rect;

/// A raster image. RGBA8 pixels, top-left origin, row-major. Cheap
/// `Arc`-backed clone; backends key their texture cache off
/// [`Self::content_hash`] so two equal `Image`s share a GPU slot.
#[derive(Clone)]
pub struct Image {
    inner: Arc<ImageInner>,
}

struct ImageInner {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    content_hash: u64,
}

impl Image {
    /// Build from RGBA8 pixels. Panics if `pixels.len() != width *
    /// height * 4`. Hashes the pixel buffer + dimensions to derive a
    /// stable content identity used for backend caching.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let expected = (width as usize) * (height as usize) * 4;
        assert_eq!(
            pixels.len(),
            expected,
            "Image::from_rgba8: expected {expected} bytes ({width}x{height} RGBA8), got {}",
            pixels.len(),
        );
        let mut h = DefaultHasher::new();
        width.hash(&mut h);
        height.hash(&mut h);
        pixels.hash(&mut h);
        let content_hash = h.finish();
        Self {
            inner: Arc::new(ImageInner {
                pixels,
                width,
                height,
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

    /// RGBA8 pixel buffer, length `width * height * 4`. Top-left origin.
    pub fn pixels(&self) -> &[u8] {
        &self.inner.pixels
    }

    /// Stable hash of `(width, height, pixels)`. Backends use this as
    /// the key into their per-image texture cache.
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
