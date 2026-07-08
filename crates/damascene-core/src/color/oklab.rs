//! [`Oklab`] — Björn Ottosson's perceptually-uniform color space for
//! hue-preserving interpolation.
//!
//! Reference: <https://bottosson.github.io/posts/oklab/>. The conversion
//! functions below are a Rust port of the reference implementation in
//! that post (published by Ottosson as public domain / MIT-0), keeping
//! its function names and `l_`/`m_`/`s_` cube-root naming.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

/// A color in the Oklab perceptually-uniform space.
///
/// - `l` is perceptual lightness in `[0, 1]` (roughly).
/// - `a` is green↔red.
/// - `b` is blue↔yellow.
/// - `alpha` is straight (un-premultiplied) opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    /// Perceptual lightness, roughly `[0, 1]`.
    pub l: f32,
    /// Green↔red opponent axis.
    pub a: f32,
    /// Blue↔yellow opponent axis.
    pub b: f32,
    /// Straight (un-premultiplied) opacity.
    pub alpha: f32,
}

impl Oklab {
    /// Convert into a [`super::Color`] in the requested target space. Goes
    /// via linear sRGB.
    pub fn to_color(self, target: super::ColorSpace) -> super::Color {
        let [r, g, b, a] = oklab_to_linear_srgb(self);
        super::Color::in_space(super::ColorSpace::SRGB_LINEAR, r, g, b, a).convert_to(target)
    }
}

pub(super) fn linear_srgb_to_oklab(r: f32, g: f32, b: f32, alpha: f32) -> Oklab {
    // sRGB-primary linear → LMS cone responses.
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

    let l_ = cbrt(l);
    let m_ = cbrt(m);
    let s_ = cbrt(s);

    Oklab {
        l: 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        a: 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        b: 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
        alpha,
    }
}

pub(super) fn oklab_to_linear_srgb(o: Oklab) -> [f32; 4] {
    let l_ = o.l + 0.396_337_78 * o.a + 0.215_803_76 * o.b;
    let m_ = o.l - 0.105_561_346 * o.a - 0.063_854_17 * o.b;
    let s_ = o.l - 0.089_484_18 * o.a - 1.291_485_5 * o.b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        o.alpha,
    ]
}

/// Sign-preserving cube root. Handles the extended-range case (negative
/// linear-RGB values from a scRGB source) cleanly.
fn cbrt(v: f32) -> f32 {
    v.signum() * v.abs().cbrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn white_oklab_is_lightness_one() {
        // (1,1,1) linear sRGB → L ≈ 1.0, a ≈ 0, b ≈ 0.
        let o = linear_srgb_to_oklab(1.0, 1.0, 1.0, 1.0);
        assert!(approx_eq(o.l, 1.0, 1e-3), "L = {}", o.l);
        assert!(approx_eq(o.a, 0.0, 1e-3));
        assert!(approx_eq(o.b, 0.0, 1e-3));
    }

    #[test]
    fn black_oklab_is_zero() {
        let o = linear_srgb_to_oklab(0.0, 0.0, 0.0, 1.0);
        assert!(approx_eq(o.l, 0.0, 1e-6));
        assert!(approx_eq(o.a, 0.0, 1e-6));
        assert!(approx_eq(o.b, 0.0, 1e-6));
    }

    #[test]
    fn linear_srgb_oklab_roundtrip() {
        let cases = [
            (0.1_f32, 0.5, 0.9),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.5, 0.5, 0.5),
        ];
        for (r, g, b) in cases {
            let o = linear_srgb_to_oklab(r, g, b, 1.0);
            let [r2, g2, b2, a2] = oklab_to_linear_srgb(o);
            assert!(approx_eq(r2, r, 1e-3), "r {r} -> {r2}");
            assert!(approx_eq(g2, g, 1e-3), "g {g} -> {g2}");
            assert!(approx_eq(b2, b, 1e-3), "b {b} -> {b2}");
            assert!(approx_eq(a2, 1.0, 1e-6));
        }
    }
}
