//! App-owned GPU textures composited into the paint stream.
//!
//! Where [`crate::image::Image`] hands Damascene a CPU pixel buffer that the
//! backend uploads and content-hash caches, an [`AppTexture`] wraps a
//! GPU texture the *app* allocates, fills, and resizes itself. Damascene
//! samples it during paint — no upload, no per-frame copy.
//!
//! This is the affordance for content that doesn't fit the quad-instance
//! shader model: 3D viewports, video frames, externally rasterised
//! canvases. The widget that displays one is [`crate::tree::surface`].
//!
//! # Sizing contract
//!
//! The source texture's pixel dimensions are **independent of the
//! rendered size**. By default, `surface()` samples the full texture
//! across its resolved layout rect with bilinear filtering; use
//! [`crate::tree::El::surface_fit`] for `Contain`, `Cover`, or natural
//! size projection, and [`crate::tree::El::surface_transform`] for
//! destination-space affine transforms. See [`crate::tree::surface`]
//! for sizing strategies (pixel-accurate, viewport-driven
//! re-allocation, aspect-ratio wrappers).
//!
//! # Backend dispatch
//!
//! Backend-neutral: [`AppTexture`] is an `Arc<dyn AppTextureBackend>`,
//! and each Damascene backend (`damascene-wgpu`, `damascene-vulkano`) supplies its
//! own concrete impl plus a constructor (e.g. `damascene_wgpu::app_texture`).
//! The runtime downcasts in the backend's record path; everything above
//! the backend boundary stays neutral.

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Pixel format of an [`AppTexture`]. The widget composites by sampling
/// the texture; the backend picks a sampler / shader path that matches.
///
/// 0.3.6 ships the three RGBA8 variants below — enough for 3D viewport
/// output (typically a surface-format-matching `*Srgb`), video decoded
/// to RGBA, and rumble-style animated frames. Future variants (HDR,
/// YUV) slot in here without breaking the widget surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFormat {
    /// 8-bit RGBA, sRGB-encoded. Sampling decodes to linear, matching
    /// the rest of Damascene's pipeline (`stock::image`, text, rounded_rect).
    Rgba8UnormSrgb,
    /// 8-bit BGRA, sRGB-encoded. The native swapchain format on most
    /// platforms — apps that render their 3D scene into a swapchain-
    /// shaped texture can hand it in directly.
    Bgra8UnormSrgb,
    /// 8-bit RGBA, linear. For content that's already in linear space
    /// (e.g. tone-mapped HDR collapsed to 8-bit, ink rasterisers) and
    /// shouldn't go through an extra sRGB decode.
    Rgba8Unorm,
    /// 16-bit float RGBA, linear extended-range. For HDR content authored
    /// in scene-linear light — values may exceed `1.0` (and the surface
    /// holds them verbatim). Composited like [`Self::Rgba8Unorm`] (linear,
    /// no sRGB decode); the extra range only carries through to the display
    /// when the swapchain is itself an extended-range float surface (see
    /// the host's HDR swapchain selection), otherwise it clamps at output.
    Rgba16Float,
}

/// How an [`AppTexture`] composes with widgets painted underneath it.
///
/// The choice affects blend state and lets opaque content skip blend
/// math; it does *not* change z-order. Widgets above the surface in the
/// paint stream still paint over it, regardless of mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceAlpha {
    /// Texture carries premultiplied alpha. Default; matches Damascene's
    /// internal blend convention.
    #[default]
    Premultiplied,
    /// Texture is fully opaque. Backend skips blending — pixels written
    /// to the surface rect replace whatever was there. Pick this for 3D
    /// viewports and video where every output pixel is non-transparent.
    Opaque,
    /// Texture carries straight (unpremultiplied) alpha. Backend
    /// premultiplies in the shader before blending. Convenient for
    /// content authored in a paint app or rasterised by a third-party
    /// vector library that doesn't premultiply.
    Straight,
}

/// Stable identity for an [`AppTexture`]. Allocated by the constructor
/// that wraps the underlying GPU texture; backends cache their bind
/// groups / descriptor sets keyed on this id, so it must not be reused
/// for a different texture during the lifetime of the wrapping
/// `AppTexture`.
///
/// Apps that recreate their texture (resize, format change) get a fresh
/// id — the previous bind group falls off the cache after one frame,
/// like any other unused entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppTextureId(pub u64);

/// Allocate a fresh [`AppTextureId`]. Used by backend constructors. App
/// code should not call this directly — go through the backend's
/// `app_texture(...)` constructor instead.
pub fn next_app_texture_id() -> AppTextureId {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    AppTextureId(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Backend implementation of an [`AppTexture`]. Implemented by
/// `damascene-wgpu` and `damascene-vulkano` against their native texture types;
/// the runtime downcasts via [`Self::as_any`] in the backend's record
/// path.
pub trait AppTextureBackend: Send + Sync + fmt::Debug + 'static {
    /// Stable identity allocated by the constructor — must round-trip
    /// the same value on every call for the lifetime of `self`.
    fn id(&self) -> AppTextureId;

    /// Pixel size of the underlying texture. The backend uses this for
    /// sanity checks; the widget rect comes from layout, not from here.
    fn size_px(&self) -> (u32, u32);

    /// Pixel format of the underlying texture. Used by the backend to
    /// pick a sampler / shader path.
    fn format(&self) -> SurfaceFormat;

    /// Downcast hatch for the backend's record path. Each backend
    /// asserts the trait object is its own concrete type; mixing
    /// backends in one runtime is unsupported.
    fn as_any(&self) -> &dyn Any;

    /// Human-readable concrete backend type for diagnostics.
    fn backend_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// An app-owned GPU texture handed to Damascene for compositing. Cheap
/// `Arc`-backed clone; pass into [`crate::tree::surface`] to display.
///
/// Construct via the backend constructor — `damascene_wgpu::app_texture` or
/// `damascene_vulkano::app_texture`. The wrapper is type-erased so the El
/// tree and paint stream stay backend-neutral.
#[derive(Clone)]
pub struct AppTexture {
    inner: Arc<dyn AppTextureBackend>,
}

impl AppTexture {
    /// Wrap a backend-supplied implementation. Constructors in
    /// `damascene-wgpu` / `damascene-vulkano` are the intended entry points.
    pub fn from_backend(inner: Arc<dyn AppTextureBackend>) -> Self {
        Self { inner }
    }

    pub fn id(&self) -> AppTextureId {
        self.inner.id()
    }

    pub fn size_px(&self) -> (u32, u32) {
        self.inner.size_px()
    }

    pub fn format(&self) -> SurfaceFormat {
        self.inner.format()
    }

    /// Borrow the backend impl as a trait object. Backends call this
    /// from their record path and downcast to their concrete type.
    pub fn backend(&self) -> &dyn AppTextureBackend {
        &*self.inner
    }

    /// Human-readable concrete backend type for diagnostics.
    pub fn backend_name(&self) -> &'static str {
        self.inner.backend_name()
    }
}

impl fmt::Debug for AppTexture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (w, h) = self.size_px();
        f.debug_struct("AppTexture")
            .field("id", &self.id().0)
            .field("size_px", &(w, h))
            .field("format", &self.format())
            .finish()
    }
}

/// Source of pixels for a [`crate::tree::Kind::Surface`] widget.
///
/// Today only [`Self::Texture`] is shipped. A `Callback(...)` variant
/// is planned as a future, more efficient path that hands the backend
/// encoder to the app during paint; the `Source` enum exists from day
/// one so that addition is non-breaking for callers.
#[derive(Clone, Debug)]
pub enum SurfaceSource {
    /// App-owned, app-filled GPU texture. Sampled by the backend during
    /// the existing paint pass — no shared encoder, no extra render
    /// pass.
    Texture(AppTexture),
}

/// The full surface configuration a [`crate::tree::Kind::Surface`] El
/// carries — source texture plus the compositing knobs. Grouped into
/// one heap box on `El` (`El::surface`) so the ~40 bytes of surface
/// state don't ride along on every non-surface node; the
/// `El::surface_*` modifiers allocate it on first use.
#[derive(Clone, Debug)]
pub struct SurfaceProps {
    /// Where the pixels come from. `None` until
    /// [`crate::tree::El::surface_source`] is set — the paint pass
    /// emits nothing without a source, whatever the other knobs say.
    pub source: Option<SurfaceSource>,
    /// How the texture composes with widgets painted below it.
    /// Defaults to [`SurfaceAlpha::Premultiplied`].
    pub alpha: SurfaceAlpha,
    /// How the texture projects into the resolved rect. Defaults to
    /// [`crate::image::ImageFit::Fill`] — stretch to the rect, ignoring
    /// aspect ratio. `Contain` / `Cover` / `None` mirror the
    /// corresponding modes on [`crate::tree::image`].
    pub fit: crate::image::ImageFit,
    /// Affine applied to the texture quad in destination space, around
    /// the centre of the post-fit rect. Defaults to identity. Composes
    /// after `fit`: the fit projection picks the destination rect, then
    /// this matrix transforms it (rotate, scale, translate, shear). The
    /// auto-clip scissor still clamps to the El's content rect, so
    /// transforms that move the texture outside that rect are cropped.
    pub transform: crate::affine::Affine2,
}

impl Default for SurfaceProps {
    fn default() -> Self {
        Self {
            source: None,
            alpha: SurfaceAlpha::Premultiplied,
            // `Fill` (not `ImageFit::default()`'s `Contain`) — the
            // historical surface default: stretch to the rect.
            fit: crate::image::ImageFit::Fill,
            transform: crate::affine::Affine2::IDENTITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_stable() {
        let a = next_app_texture_id();
        let b = next_app_texture_id();
        assert_ne!(a, b);
        assert_eq!(a, AppTextureId(a.0));
    }

    #[test]
    fn surface_alpha_default_is_premultiplied() {
        assert_eq!(SurfaceAlpha::default(), SurfaceAlpha::Premultiplied);
    }
}
