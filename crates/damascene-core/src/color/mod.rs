//! Color values + colorspace conversion + perceptual interpolation.
//!
//! Every [`Color`] carries an explicit [`ColorSpace`] — primaries, transfer
//! function, reference luminance. Operations that mix colors
//! ([`Color::lighten`] / [`darken`](Color::darken) / [`mix`](Color::mix) /
//! [`interpolate`](Color::interpolate)) work through perceptually-uniform
//! [`Oklab`] regardless of the input space.
//!
//! The naming vocabulary mirrors the Wayland `wp_color_management_v1`
//! parametric image-description builder (primaries enum, transfer enum,
//! reference luminance in nits) and the ITU/SMPTE specs both build on. A
//! `Color` is a *value* in some space; a [`ColorSpace`] is *how to
//! interpret* those channel values as light.
//!
//! ## Working space at paint time
//!
//! The paint stream converts every `Color` into the renderer's *working
//! color space* (default [`ColorSpace::SRGB_LINEAR`]) exactly once at the
//! upload boundary. Stock shaders receive premultiplied f32x4 in that
//! working space. The custom-shader contract documents the same.
//!
//! ## Why Color is fat
//!
//! `Color` is ~44 bytes — bigger than the old u8 RGBA shape. The hot path
//! (per-frame `QuadInstance` upload) stores `[f32; 4]` after the working-
//! space conversion, so the extra bytes only land in transient build-time
//! storage (`UniformBlock`, theme tokens in `.rodata`, per-`El` styled
//! fields). The tradeoff buys self-describing space, no silent sRGB
//! assumptions, and a single place where TF / primary math lives.

mod caps;
mod convert;
mod oklab;
mod space;

pub use caps::{
    ColorFeature, ColorManagementStatus, ColorPreferences, CompositorColorTargets,
    HostColorCapabilities, RenderIntent,
};
pub use convert::primaries_matrix;
pub use oklab::Oklab;
pub use space::{ColorSpace, GammaExponent, Primaries, TransferFunction};

/// A color value in some [`ColorSpace`].
///
/// Channel values are floats so any range is representable — sRGB-encoded
/// 8-bit sources just store `r/255`, scRGB-linear sources can hold
/// negatives and values above 1.0, etc.
///
/// `token` names the theme palette entry that produced this color and
/// allows render-time palette substitution (see [`crate::theme::Palette`]).
/// Alpha-only ops ([`with_alpha`](Color::with_alpha)) preserve it;
/// rgb-modifying ops ([`lighten`](Color::lighten),
/// [`darken`](Color::darken), [`mix`](Color::mix),
/// [`interpolate`](Color::interpolate), [`convert_to`](Color::convert_to))
/// strip it — once rgb has been derived, swapping the palette would
/// silently discard the derivation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub space: ColorSpace,
    pub token: Option<&'static str>,
}

impl Eq for Color {}

// `Hash` over `f32` via `to_bits`: required because `Color` and
// `ColorSpace` participate in hashed keys (text atlas run dedup, etc.).
// Two colors that compare `PartialEq` will hash equal because both sides
// reduce to the same bit pattern.
impl std::hash::Hash for Color {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.r.to_bits().hash(h);
        self.g.to_bits().hash(h);
        self.b.to_bits().hash(h);
        self.a.to_bits().hash(h);
        self.space.hash(h);
        self.token.hash(h);
    }
}

impl Color {
    /// sRGB-encoded color from 8-bit channels (alpha = 255).
    #[inline]
    pub const fn srgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::srgb_u8a(r, g, b, 255)
    }

    /// sRGB-encoded color from 8-bit channels.
    #[inline]
    pub const fn srgb_u8a(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
            space: ColorSpace::SRGB,
            token: None,
        }
    }

    /// sRGB-encoded color with a theme palette token attached.
    #[inline]
    pub const fn srgb_token(name: &'static str, r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
            space: ColorSpace::SRGB,
            token: Some(name),
        }
    }

    /// Linear sRGB-primary color. `[0, 1]` covers the SDR range; values
    /// outside that range are allowed (see also [`scrgb_linear`](Color::scrgb_linear)).
    #[inline]
    pub const fn srgb_linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::SRGB_LINEAR, r, g, b, a)
    }

    /// scRGB linear: sRGB primaries, linear transfer, extended value range
    /// allowed. Same representation as [`srgb_linear`](Color::srgb_linear);
    /// distinct constructor for author intent.
    #[inline]
    pub const fn scrgb_linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::SCRGB_LINEAR, r, g, b, a)
    }

    /// Display-P3 color (DCI-P3 primaries, sRGB transfer, D65 white).
    /// The space most wide-gamut authored content lives in.
    #[inline]
    pub const fn display_p3(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::DISPLAY_P3, r, g, b, a)
    }

    /// Linear Display-P3 color.
    #[inline]
    pub const fn display_p3_linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::DISPLAY_P3_LINEAR, r, g, b, a)
    }

    /// BT.2020-primaries linear color. `[0, 1]` covers the SDR range.
    #[inline]
    pub const fn bt2020_linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::BT2020_LINEAR, r, g, b, a)
    }

    /// BT.2020 + SMPTE ST 2084 (PQ) HDR color. `1.0` = 10000 nits.
    #[inline]
    pub const fn bt2020_pq(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::BT2020_PQ, r, g, b, a)
    }

    /// BT.2020 + Hybrid Log-Gamma HDR color.
    #[inline]
    pub const fn bt2020_hlg(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self::in_space(ColorSpace::BT2020_HLG, r, g, b, a)
    }

    /// Escape hatch: explicit channel + space, no token.
    #[inline]
    pub const fn in_space(space: ColorSpace, r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r,
            g,
            b,
            a,
            space,
            token: None,
        }
    }

    /// Replace alpha. Preserves the token (alpha-only ops keep palette
    /// derivation valid).
    #[inline]
    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Same as [`with_alpha`](Self::with_alpha) but takes a u8 in `[0, 255]`
    /// for ergonomic interop with hex authoring.
    #[inline]
    pub fn with_alpha_u8(self, a: u8) -> Self {
        self.with_alpha(a as f32 / 255.0)
    }

    /// Encode as sRGB 8-bit RGBA, converting from the source space.
    /// Out-of-range values are clamped to `[0, 255]`. Used by SVG /
    /// hex-emitting paths.
    pub fn to_srgb_u8a(self) -> [u8; 4] {
        let srgb = self.convert_to(ColorSpace::SRGB);
        [
            float_to_u8(srgb.r),
            float_to_u8(srgb.g),
            float_to_u8(srgb.b),
            float_to_u8(srgb.a),
        ]
    }

    /// Convert into `target`, decoding the source TF, applying the
    /// primary matrix (if primaries differ), re-encoding the target TF.
    /// Strips the token — once rgb has been derived in a different space,
    /// the token's palette-resolution semantics no longer apply.
    pub fn convert_to(self, target: ColorSpace) -> Self {
        if self.space == target {
            return self;
        }
        let [r, g, b] = convert::convert_rgb([self.r, self.g, self.b], self.space, target);
        Self {
            r,
            g,
            b,
            a: self.a,
            space: target,
            token: None,
        }
    }

    /// Materialize a premultiplied f32x4 in the renderer's working space.
    /// The single boundary every paint upload crosses.
    pub fn to_working_space(self, working: ColorSpace) -> [f32; 4] {
        let c = self.convert_to(working);
        let a = c.a.clamp(0.0, 1.0);
        [c.r * a, c.g * a, c.b * a, a]
    }

    /// Convert to perceptual [`Oklab`]. Goes via linear sRGB regardless
    /// of source space.
    pub fn to_oklab(self) -> Oklab {
        let lin = self.convert_to(ColorSpace::SRGB_LINEAR);
        oklab::linear_srgb_to_oklab(lin.r, lin.g, lin.b, lin.a)
    }

    /// Perceptual interpolation between two colors. Always proceeds
    /// through [`Oklab`] (hue-preserving, no muddy mid-gray on
    /// complementary lerps). Result lands in `self.space`. Strips the
    /// token.
    pub fn interpolate(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let a = self.to_oklab();
        let b = other.to_oklab();
        let lerped = Oklab {
            l: a.l + (b.l - a.l) * t,
            a: a.a + (b.a - a.a) * t,
            b: a.b + (b.b - a.b) * t,
            alpha: a.alpha + (b.alpha - a.alpha) * t,
        };
        let [lr, lg, lb, la] = oklab::oklab_to_linear_srgb(lerped);
        Color::in_space(ColorSpace::SRGB_LINEAR, lr, lg, lb, la).convert_to(self.space)
    }

    /// Lighten toward white (perceptual). Strips the token.
    pub fn lighten(self, t: f32) -> Self {
        let white = Color::srgb_u8(255, 255, 255).convert_to(self.space);
        self.interpolate(Color { a: self.a, ..white }, t.clamp(0.0, 1.0))
    }

    /// Darken toward black (perceptual). Strips the token.
    pub fn darken(self, t: f32) -> Self {
        let black = Color::in_space(self.space, 0.0, 0.0, 0.0, self.a);
        self.interpolate(black, t.clamp(0.0, 1.0))
    }

    /// Linearly interpolate between two colors (perceptual). Alias for
    /// [`interpolate`](Self::interpolate) preserved for API parity with the
    /// pre-refactor `Color::mix`.
    #[inline]
    pub fn mix(self, other: Self, t: f32) -> Self {
        self.interpolate(other, t)
    }
}

fn float_to_u8(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn srgb_u8_roundtrip() {
        let c = Color::srgb_u8a(250, 128, 64, 200);
        let [r, g, b, a] = c.to_srgb_u8a();
        assert_eq!([r, g, b, a], [250, 128, 64, 200]);
    }

    #[test]
    fn srgb_token_constructor() {
        let c = Color::srgb_token("primary", 250, 250, 250, 255);
        assert_eq!(c.token, Some("primary"));
        assert_eq!(c.space, ColorSpace::SRGB);
    }

    #[test]
    fn convert_strips_token() {
        let c = Color::srgb_token("primary", 200, 100, 50, 255);
        let conv = c.convert_to(ColorSpace::SRGB_LINEAR);
        assert_eq!(conv.token, None, "convert_to must strip the token");
        assert_eq!(conv.space, ColorSpace::SRGB_LINEAR);
    }

    #[test]
    fn convert_srgb_to_linear_and_back() {
        let c = Color::srgb_u8(128, 200, 64);
        let lin = c.convert_to(ColorSpace::SRGB_LINEAR);
        let back = lin.convert_to(ColorSpace::SRGB);
        assert!(approx_eq(back.r, c.r, 1e-5));
        assert!(approx_eq(back.g, c.g, 1e-5));
        assert!(approx_eq(back.b, c.b, 1e-5));
    }

    #[test]
    fn convert_srgb_white_to_bt2020_stays_white() {
        let white = Color::srgb_u8(255, 255, 255);
        let bt = white.convert_to(ColorSpace::BT2020_LINEAR);
        // BT.2020 and sRGB both use D65; white maps to (1, 1, 1).
        assert!(approx_eq(bt.r, 1.0, 1e-4));
        assert!(approx_eq(bt.g, 1.0, 1e-4));
        assert!(approx_eq(bt.b, 1.0, 1e-4));
    }

    #[test]
    fn convert_same_space_is_identity_with_token_kept() {
        let c = Color::srgb_token("primary", 200, 100, 50, 255);
        let same = c.convert_to(ColorSpace::SRGB);
        assert_eq!(same.token, Some("primary"));
        assert_eq!(same, c);
    }

    #[test]
    fn working_space_premultiplies_alpha() {
        let c = Color::srgb_linear(1.0, 0.5, 0.25, 0.5);
        let [r, g, b, a] = c.to_working_space(ColorSpace::SRGB_LINEAR);
        assert!(approx_eq(r, 0.5, 1e-6));
        assert!(approx_eq(g, 0.25, 1e-6));
        assert!(approx_eq(b, 0.125, 1e-6));
        assert!(approx_eq(a, 0.5, 1e-6));
    }

    #[test]
    fn interpolate_strips_token_and_keeps_self_space() {
        let a = Color::srgb_token("primary", 255, 0, 0, 255);
        let b = Color::srgb_u8(0, 0, 255);
        let mid = a.interpolate(b, 0.5);
        assert_eq!(mid.token, None);
        assert_eq!(mid.space, ColorSpace::SRGB);
    }

    #[test]
    fn interpolate_endpoints_match_inputs() {
        let a = Color::srgb_u8(255, 0, 0);
        let b = Color::srgb_u8(0, 0, 255);
        let zero = a.interpolate(b, 0.0);
        let one = a.interpolate(b, 1.0);
        // OkLab round-trip can drift by a couple of u8 levels; allow it.
        assert!(approx_eq(zero.r, a.r, 0.01));
        assert!(approx_eq(zero.g, a.g, 0.01));
        assert!(approx_eq(zero.b, a.b, 0.01));
        assert!(approx_eq(one.r, b.r, 0.01));
        assert!(approx_eq(one.g, b.g, 0.01));
        assert!(approx_eq(one.b, b.b, 0.01));
    }

    #[test]
    fn lighten_moves_toward_white() {
        let c = Color::srgb_u8(50, 50, 50);
        let lighter = c.lighten(0.5);
        assert!(lighter.r > c.r);
        assert!(lighter.g > c.g);
        assert!(lighter.b > c.b);
    }

    #[test]
    fn darken_moves_toward_black() {
        let c = Color::srgb_u8(200, 200, 200);
        let darker = c.darken(0.5);
        assert!(darker.r < c.r);
        assert!(darker.g < c.g);
        assert!(darker.b < c.b);
    }

    #[test]
    fn with_alpha_preserves_token() {
        let c = Color::srgb_token("primary", 250, 250, 250, 255);
        let faded = c.with_alpha(0.5);
        assert_eq!(faded.token, Some("primary"));
        assert!(approx_eq(faded.a, 0.5, 1e-6));
    }
}
