//! [`ColorSpace`] + its enum components.

use std::num::NonZeroU32;

/// Color primaries — which RGB triangle the channel values live in.
///
/// All four named variants use the D65 white point, so conversions
/// between them are pure 3×3 matrix multiplies in linear light (no
/// chromatic adaptation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primaries {
    /// Rec.709 / sRGB primaries. D65 white.
    Srgb,
    /// DCI-P3 with D65 white (Display-P3).
    DisplayP3,
    /// Rec.2020 / BT.2020 / UHDTV primaries. D65 white.
    Bt2020,
    /// Adobe RGB (1998) primaries. D65 white.
    AdobeRgb,
}

/// Optical-electronic transfer function applied to the channel values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransferFunction {
    /// Linear light. 1.0 = reference white.
    Linear,
    /// sRGB piecewise approximation of gamma ≈2.2.
    Srgb,
    /// BT.1886 (gamma 2.4, broadcast reference).
    Bt1886,
    /// SMPTE ST 2084 (PQ). Normalized so 1.0 = 10000 nits.
    Pq,
    /// Hybrid Log-Gamma.
    Hlg,
    /// Pure gamma exponent.
    Gamma(GammaExponent),
}

/// Gamma exponent × 100 (so 2.2 → 220) for `Eq`/`Hash`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GammaExponent(NonZeroU32);

impl GammaExponent {
    /// Construct from `× 100` integer (e.g. `220` for gamma 2.2). Returns
    /// `None` for `0`.
    pub const fn from_x100(n: u32) -> Option<Self> {
        match NonZeroU32::new(n) {
            Some(n) => Some(Self(n)),
            None => None,
        }
    }

    pub fn from_f32(g: f32) -> Option<Self> {
        let n = (g * 100.0).round() as u32;
        NonZeroU32::new(n).map(Self)
    }

    pub fn to_f32(self) -> f32 {
        self.0.get() as f32 / 100.0
    }
}

/// Complete description of how a buffer's pixel values map to light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorSpace {
    pub primaries: Primaries,
    pub transfer: TransferFunction,
    /// Reference white luminance in nits. SDR convention: 100. HDR
    /// clients typically specify 100–203.
    pub reference_luminance_nits: f32,
}

impl Eq for ColorSpace {}

impl std::hash::Hash for ColorSpace {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.primaries.hash(h);
        self.transfer.hash(h);
        self.reference_luminance_nits.to_bits().hash(h);
    }
}

impl ColorSpace {
    /// sRGB primaries, sRGB transfer, 100 nit ref white. The default for
    /// authored UI content.
    pub const SRGB: Self = Self {
        primaries: Primaries::Srgb,
        transfer: TransferFunction::Srgb,
        reference_luminance_nits: 100.0,
    };

    /// sRGB primaries, linear transfer, 100 nit ref white. The default
    /// renderer working space.
    pub const SRGB_LINEAR: Self = Self {
        primaries: Primaries::Srgb,
        transfer: TransferFunction::Linear,
        reference_luminance_nits: 100.0,
    };

    /// scRGB-style: sRGB primaries, linear, extended value range. Same
    /// representation as [`SRGB_LINEAR`](Self::SRGB_LINEAR); distinct
    /// constant for author intent (negative + above-1 values welcome).
    pub const SCRGB_LINEAR: Self = Self::SRGB_LINEAR;

    /// DCI-P3 primaries, sRGB transfer, D65 white. Display-P3.
    pub const DISPLAY_P3: Self = Self {
        primaries: Primaries::DisplayP3,
        transfer: TransferFunction::Srgb,
        reference_luminance_nits: 100.0,
    };

    /// DCI-P3 primaries, linear transfer.
    pub const DISPLAY_P3_LINEAR: Self = Self {
        primaries: Primaries::DisplayP3,
        transfer: TransferFunction::Linear,
        reference_luminance_nits: 100.0,
    };

    /// BT.2020 primaries, linear transfer.
    pub const BT2020_LINEAR: Self = Self {
        primaries: Primaries::Bt2020,
        transfer: TransferFunction::Linear,
        reference_luminance_nits: 100.0,
    };

    /// BT.2020 primaries, PQ transfer (HDR10).
    pub const BT2020_PQ: Self = Self {
        primaries: Primaries::Bt2020,
        transfer: TransferFunction::Pq,
        reference_luminance_nits: 100.0,
    };

    /// BT.2020 primaries, HLG transfer.
    pub const BT2020_HLG: Self = Self {
        primaries: Primaries::Bt2020,
        transfer: TransferFunction::Hlg,
        reference_luminance_nits: 100.0,
    };

    /// Adobe RGB.
    pub const ADOBE_RGB: Self = Self {
        primaries: Primaries::AdobeRgb,
        // Adobe RGB is defined with a 2.2 gamma; we model it explicitly
        // rather than reusing the sRGB piecewise.
        transfer: TransferFunction::Gamma(match GammaExponent::from_x100(220) {
            Some(g) => g,
            None => unreachable!(),
        }),
        reference_luminance_nits: 100.0,
    };
}
