//! Color space conversion: transfer functions + primary matrices.
//!
//! `convert_rgb([r,g,b], src, dst)` is the single function the rest of
//! the module calls. It decodes the source transfer function to linear
//! light, applies the source-primaries → XYZ → dest-primaries matrix
//! pair (composed once into a single 3×3), then encodes the destination
//! transfer function.
//!
//! All named primaries (sRGB, Display-P3, BT.2020, AdobeRGB) use D65, so
//! no chromatic adaptation is needed for the matrix step — it's a pure
//! basis change.

use super::space::{ColorSpace, Primaries, TransferFunction};

/// Convert an RGB triple from `src` to `dst`. Alpha is untouched and
/// handled by the caller.
pub(super) fn convert_rgb(rgb: [f32; 3], src: ColorSpace, dst: ColorSpace) -> [f32; 3] {
    // Decode source TF → linear light in source primaries.
    let lin_src = [
        decode_transfer(rgb[0], src.transfer),
        decode_transfer(rgb[1], src.transfer),
        decode_transfer(rgb[2], src.transfer),
    ];

    // Change of primaries (linear light, both D65 → no chromatic adaptation).
    let lin_dst = if src.primaries == dst.primaries {
        lin_src
    } else {
        let m = primaries_matrix(src.primaries, dst.primaries);
        mat3_mul_vec3(m, lin_src)
    };

    // Encode destination TF.
    [
        encode_transfer(lin_dst[0], dst.transfer),
        encode_transfer(lin_dst[1], dst.transfer),
        encode_transfer(lin_dst[2], dst.transfer),
    ]
}

/// Decode a transfer-function-encoded value to linear light.
pub fn decode_transfer(v: f32, tf: TransferFunction) -> f32 {
    match tf {
        TransferFunction::Linear => v,
        TransferFunction::Srgb => srgb_decode(v),
        TransferFunction::Bt1886 => signed_pow(v, 2.4),
        TransferFunction::Pq => pq_decode(v),
        TransferFunction::Hlg => hlg_decode(v),
        TransferFunction::Gamma(g) => signed_pow(v, g.to_f32()),
    }
}

/// Encode a linear-light value through a transfer function.
pub fn encode_transfer(v: f32, tf: TransferFunction) -> f32 {
    match tf {
        TransferFunction::Linear => v,
        TransferFunction::Srgb => srgb_encode(v),
        TransferFunction::Bt1886 => signed_pow(v, 1.0 / 2.4),
        TransferFunction::Pq => pq_encode(v),
        TransferFunction::Hlg => hlg_encode(v),
        TransferFunction::Gamma(g) => signed_pow(v, 1.0 / g.to_f32()),
    }
}

/// Return the 3×3 matrix mapping `src`-primary linear RGB to `dst`-
/// primary linear RGB. All named primaries use D65, so this is RGB→XYZ
/// (dst) ∘ XYZ→RGB (src), no chromatic adaptation.
pub fn primaries_matrix(src: Primaries, dst: Primaries) -> [[f32; 3]; 3] {
    if src == dst {
        return IDENTITY3;
    }
    let to_xyz_src = rgb_to_xyz_d65(src);
    let to_rgb_dst = xyz_to_rgb_d65(dst);
    mat3_mul(to_rgb_dst, to_xyz_src)
}

// --- transfer functions ---------------------------------------------------

fn srgb_decode(v: f32) -> f32 {
    // Sign-preserving for extended range (negative scRGB-style inputs).
    let sign = v.signum();
    let x = v.abs();
    let y = if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    };
    sign * y
}

fn srgb_encode(v: f32) -> f32 {
    let sign = v.signum();
    let x = v.abs();
    let y = if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    sign * y
}

// SMPTE ST 2084 (PQ). Encoded values in [0,1] map to linear [0, 10000] nits;
// we normalize so linear-output of 1.0 = 10000 nits (the
// TransferFunction::Pq convention). Reference-white anchoring is a
// separate, image-pipeline step (Image::to_scrgb_f16) — Color conversion
// stays encoding-literal.
const PQ_M1: f32 = 0.159_301_76;
const PQ_M2: f32 = 78.843_75;
const PQ_C1: f32 = 0.835_937_5;
const PQ_C2: f32 = 18.851_563;
const PQ_C3: f32 = 18.687_5;

fn pq_decode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    let vp = v.powf(1.0 / PQ_M2);
    let num = (vp - PQ_C1).max(0.0);
    let den = PQ_C2 - PQ_C3 * vp;
    (num / den).powf(1.0 / PQ_M1)
}

fn pq_encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    let vp = v.powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * vp) / (1.0 + PQ_C3 * vp)).powf(PQ_M2)
}

// Hybrid Log-Gamma (ARIB STD-B67 / BT.2100). Scene-referred form.
const HLG_A: f32 = 0.178_832_77;
const HLG_B: f32 = 0.284_668_92;
const HLG_C: f32 = 0.559_910_7;

fn hlg_decode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.5 {
        (v * v) / 3.0
    } else {
        (((v - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
    }
}

fn hlg_encode(v: f32) -> f32 {
    let v = v.max(0.0);
    if v <= 1.0 / 12.0 {
        (3.0 * v).sqrt()
    } else {
        HLG_A * (12.0 * v - HLG_B).ln() + HLG_C
    }
}

fn signed_pow(v: f32, e: f32) -> f32 {
    v.signum() * v.abs().powf(e)
}

// --- primaries matrices ---------------------------------------------------
//
// Source: ITU-R BT.709 / SMPTE RP 145 / BT.2020 / Adobe RGB (1998)
// specifications. All matrices map linear-RGB (in the given primaries,
// D65 white) → CIE 1931 XYZ. Their inverses do the reverse.

const IDENTITY3: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[rustfmt::skip]
const SRGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.412_390_8, 0.357_584_3, 0.180_480_8],
    [0.212_639, 0.715_168_7, 0.072_192_3],
    [0.019_330_8, 0.119_194_8, 0.950_532_2],
];

#[rustfmt::skip]
const XYZ_TO_SRGB: [[f32; 3]; 3] = [
    [ 3.240_97,  -1.537_383, -0.498_611],
    [-0.969_244,   1.875_968,  0.041_555_1],
    [ 0.055_630, -0.203_977,  1.056_972],
];

#[rustfmt::skip]
const DISPLAY_P3_TO_XYZ: [[f32; 3]; 3] = [
    [0.486_570_9, 0.265_667_7, 0.198_217_3],
    [0.228_974_8, 0.691_738_8, 0.079_286_4],
    [0.000_000_0, 0.045_113_4, 1.043_944_2],
];

#[rustfmt::skip]
const XYZ_TO_DISPLAY_P3: [[f32; 3]; 3] = [
    [ 2.493_497, -0.931_383_6, -0.402_710_8],
    [-0.829_488_9,  1.762_664_1,  0.023_624_7],
    [ 0.035_845_8, -0.076_172_4,  0.956_884_5],
];

#[rustfmt::skip]
const BT2020_TO_XYZ: [[f32; 3]; 3] = [
    [0.636_958, 0.144_616_9, 0.168_880_8],
    [0.262_700_2, 0.677_998, 0.059_301_7],
    [0.000_000_0, 0.028_072_7, 1.060_985],
];

#[rustfmt::skip]
const XYZ_TO_BT2020: [[f32; 3]; 3] = [
    [ 1.716_651_2, -0.355_670_8, -0.253_366_1],
    [-0.666_684_4,  1.616_481_2,  0.015_768_6],
    [ 0.017_639_9, -0.042_770_1,  0.942_103_1],
];

#[rustfmt::skip]
const ADOBE_RGB_TO_XYZ: [[f32; 3]; 3] = [
    [0.576_669, 0.185_558, 0.188_229_4],
    [0.297_344_7, 0.627_363_6, 0.075_291_7],
    [0.027_031_4, 0.070_688_8, 0.991_337_5],
];

#[rustfmt::skip]
const XYZ_TO_ADOBE_RGB: [[f32; 3]; 3] = [
    [ 2.041_588, -0.564_976_7, -0.344_587_3],
    [-0.969_243_6,  1.875_967_5,  0.041_555_7],
    [ 0.013_444_2, -0.118_362_3,  1.015_174_7],
];

fn rgb_to_xyz_d65(p: Primaries) -> [[f32; 3]; 3] {
    match p {
        Primaries::Srgb => SRGB_TO_XYZ,
        Primaries::DisplayP3 => DISPLAY_P3_TO_XYZ,
        Primaries::Bt2020 => BT2020_TO_XYZ,
        Primaries::AdobeRgb => ADOBE_RGB_TO_XYZ,
    }
}

fn xyz_to_rgb_d65(p: Primaries) -> [[f32; 3]; 3] {
    match p {
        Primaries::Srgb => XYZ_TO_SRGB,
        Primaries::DisplayP3 => XYZ_TO_DISPLAY_P3,
        Primaries::Bt2020 => XYZ_TO_BT2020,
        Primaries::AdobeRgb => XYZ_TO_ADOBE_RGB,
    }
}

fn mat3_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

fn mat3_mul_vec3(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn srgb_transfer_roundtrips() {
        for v in [0.0_f32, 0.005, 0.04, 0.5, 1.0] {
            let r = srgb_encode(srgb_decode(v));
            assert!(approx_eq(r, v, 1e-5), "v={v} r={r}");
        }
    }

    #[test]
    fn pq_transfer_roundtrips() {
        for v in [0.0_f32, 0.1, 0.5, 0.9, 1.0] {
            let r = pq_encode(pq_decode(v));
            assert!(approx_eq(r, v, 1e-4), "v={v} r={r}");
        }
    }

    #[test]
    fn hlg_transfer_roundtrips() {
        for v in [0.0_f32, 0.05, 0.5, 0.9, 1.0] {
            let r = hlg_encode(hlg_decode(v));
            assert!(approx_eq(r, v, 1e-4), "v={v} r={r}");
        }
    }

    #[test]
    fn primaries_white_preserved() {
        for src in [
            Primaries::Srgb,
            Primaries::DisplayP3,
            Primaries::Bt2020,
            Primaries::AdobeRgb,
        ] {
            for dst in [
                Primaries::Srgb,
                Primaries::DisplayP3,
                Primaries::Bt2020,
                Primaries::AdobeRgb,
            ] {
                let m = primaries_matrix(src, dst);
                let out = mat3_mul_vec3(m, [1.0, 1.0, 1.0]);
                for (i, o) in out.iter().enumerate() {
                    assert!(
                        approx_eq(*o, 1.0, 1e-3),
                        "white not preserved {src:?}→{dst:?} ch{i} = {o}"
                    );
                }
            }
        }
    }

    #[test]
    fn srgb_to_srgb_is_identity() {
        let m = primaries_matrix(Primaries::Srgb, Primaries::Srgb);
        assert_eq!(m, IDENTITY3);
    }

    #[test]
    fn srgb_bt2020_roundtrip() {
        let m_fwd = primaries_matrix(Primaries::Srgb, Primaries::Bt2020);
        let m_back = primaries_matrix(Primaries::Bt2020, Primaries::Srgb);
        let v = [0.4, 0.7, 0.1];
        let r = mat3_mul_vec3(m_back, mat3_mul_vec3(m_fwd, v));
        for i in 0..3 {
            assert!(approx_eq(r[i], v[i], 1e-3), "ch{i} {v:?} → {r:?}");
        }
    }
}
