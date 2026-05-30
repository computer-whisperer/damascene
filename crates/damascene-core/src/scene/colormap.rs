//! Scalar→colour maps for encoding a data field onto scatter points (or
//! any per-vertex colour).
//!
//! These are pure CPU helpers: an app maps its values to authoring-space
//! sRGBA `[f32; 4]` and stores them in the geometry ([`ScenePoint::color`]),
//! exactly as if it had picked the colours by hand. Nothing here touches a
//! device or the renderer — the colours ride the normal upload path and
//! get converted to working-linear space by the backend like every other
//! scene colour.
//!
//! The perceptual maps ([`Colormap::Viridis`] and friends) are stored as
//! piecewise-linear anchor tables sampled from the well-known matplotlib
//! maps — close enough for small graphs, and far cheaper than a full
//! 256-entry LUT per map. Sample with [`colormap`].

use glam::Vec3;

use crate::scene::geometry::{PointData, ScenePoint};

/// A scalar→colour gradient. `t` runs `0.0..=1.0`; values outside are
/// clamped. All maps return **authoring-space** sRGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Colormap {
    /// Perceptually uniform blue→green→yellow. The sensible default for
    /// sequential data.
    #[default]
    Viridis,
    /// Perceptually uniform black→red→orange→white.
    Magma,
    /// Perceptually uniform blue→magenta→yellow.
    Plasma,
    /// Blue→green→yellow→red rainbow with even brightness (good for a wide
    /// dynamic range where hue separation matters more than uniformity).
    Turbo,
    /// Blue→grey→yellow, designed to read for the most common forms of
    /// colour-vision deficiency.
    Cividis,
    /// Black→white luminance ramp.
    Grayscale,
}

/// Sample a [`Colormap`] at `t` (clamped to `0.0..=1.0`), returning
/// authoring-space sRGBA with alpha `1.0`.
pub fn colormap(t: f32, map: Colormap) -> [f32; 4] {
    let rgb = match map {
        Colormap::Grayscale => {
            let v = t.clamp(0.0, 1.0);
            [v, v, v]
        }
        Colormap::Viridis => sample(VIRIDIS, t),
        Colormap::Magma => sample(MAGMA, t),
        Colormap::Plasma => sample(PLASMA, t),
        Colormap::Turbo => sample(TURBO, t),
        Colormap::Cividis => sample(CIVIDIS, t),
    };
    [rgb[0], rgb[1], rgb[2], 1.0]
}

/// Piecewise-linear lookup into an evenly-spaced anchor table.
fn sample(anchors: &[[f32; 3]], t: f32) -> [f32; 3] {
    debug_assert!(anchors.len() >= 2);
    let t = t.clamp(0.0, 1.0);
    let last = anchors.len() - 1;
    let scaled = t * last as f32;
    let i = (scaled as usize).min(last - 1);
    let frac = scaled - i as f32;
    let a = anchors[i];
    let b = anchors[i + 1];
    [
        a[0] + (b[0] - a[0]) * frac,
        a[1] + (b[1] - a[1]) * frac,
        a[2] + (b[2] - a[2]) * frac,
    ]
}

impl PointData {
    /// Build scatter points by colour-mapping a scalar field.
    ///
    /// `domain` is the `(min, max)` value range that maps to the colormap's
    /// `0.0..=1.0`; pass the data's own min/max for a full-range ramp, or a
    /// fixed range to keep colours comparable across frames. Values outside
    /// the domain clamp to the endpoints. Positions and values are zipped;
    /// extras in the longer iterator are ignored.
    pub fn from_values(
        positions: impl IntoIterator<Item = Vec3>,
        values: impl IntoIterator<Item = f32>,
        domain: (f32, f32),
        map: Colormap,
    ) -> Self {
        let span = domain.1 - domain.0;
        let points = positions
            .into_iter()
            .zip(values)
            .map(|(position, value)| {
                let t = if span.abs() > f32::EPSILON {
                    (value - domain.0) / span
                } else {
                    0.5
                };
                ScenePoint {
                    position,
                    color: colormap(t, map),
                }
            })
            .collect();
        PointData { points }
    }
}

// ---- anchor tables (sampled from the matplotlib maps) ----
//
// These are hand-sampled colour values, not mathematical constants — a few
// happen to land near `f32::consts` fractions (e.g. 0.318 ≈ `FRAC_1_PI`),
// so `clippy::approx_constant` is allowed on the tables.
#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const VIRIDIS: &[[f32; 3]] = &[
    [0.267, 0.005, 0.329], [0.283, 0.141, 0.458], [0.254, 0.265, 0.530],
    [0.207, 0.372, 0.553], [0.164, 0.471, 0.558], [0.128, 0.567, 0.551],
    [0.135, 0.659, 0.518], [0.267, 0.749, 0.441], [0.478, 0.821, 0.318],
    [0.741, 0.873, 0.150], [0.993, 0.906, 0.144],
];

#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const MAGMA: &[[f32; 3]] = &[
    [0.001, 0.000, 0.014], [0.078, 0.054, 0.211], [0.232, 0.060, 0.438],
    [0.390, 0.111, 0.498], [0.550, 0.161, 0.506], [0.716, 0.215, 0.475],
    [0.868, 0.288, 0.409], [0.967, 0.440, 0.361], [0.994, 0.624, 0.427],
    [0.997, 0.793, 0.580], [0.987, 0.991, 0.749],
];

#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const PLASMA: &[[f32; 3]] = &[
    [0.050, 0.030, 0.528], [0.255, 0.014, 0.615], [0.417, 0.001, 0.658],
    [0.562, 0.052, 0.642], [0.692, 0.166, 0.565], [0.798, 0.280, 0.470],
    [0.881, 0.392, 0.383], [0.949, 0.518, 0.296], [0.988, 0.652, 0.212],
    [0.988, 0.809, 0.145], [0.940, 0.975, 0.131],
];

#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const TURBO: &[[f32; 3]] = &[
    [0.190, 0.072, 0.232], [0.275, 0.408, 0.859], [0.180, 0.718, 0.642],
    [0.337, 0.875, 0.391], [0.575, 0.965, 0.247], [0.795, 0.962, 0.220],
    [0.939, 0.840, 0.221], [0.992, 0.626, 0.150], [0.954, 0.371, 0.077],
    [0.819, 0.184, 0.027], [0.480, 0.016, 0.011],
];

#[rustfmt::skip]
#[allow(clippy::approx_constant)]
const CIVIDIS: &[[f32; 3]] = &[
    [0.000, 0.135, 0.305], [0.000, 0.180, 0.405], [0.196, 0.247, 0.394],
    [0.314, 0.314, 0.392], [0.408, 0.380, 0.396], [0.498, 0.447, 0.395],
    [0.592, 0.518, 0.385], [0.690, 0.591, 0.365], [0.793, 0.667, 0.332],
    [0.900, 0.749, 0.282], [1.000, 0.835, 0.224],
];

#[cfg(test)]
mod tests {
    use super::*;

    const MAPS: &[Colormap] = &[
        Colormap::Viridis,
        Colormap::Magma,
        Colormap::Plasma,
        Colormap::Turbo,
        Colormap::Cividis,
        Colormap::Grayscale,
    ];

    #[test]
    fn samples_stay_in_gamut_and_opaque() {
        for &map in MAPS {
            for i in 0..=20 {
                let c = colormap(i as f32 / 20.0, map);
                assert_eq!(c[3], 1.0, "{map:?} alpha");
                for ch in &c[..3] {
                    assert!(
                        (0.0..=1.0).contains(ch),
                        "{map:?} channel {ch} out of gamut"
                    );
                }
            }
        }
    }

    #[test]
    fn t_clamps_outside_unit_range() {
        for &map in MAPS {
            assert_eq!(colormap(-1.0, map), colormap(0.0, map));
            assert_eq!(colormap(2.0, map), colormap(1.0, map));
        }
    }

    #[test]
    fn grayscale_is_a_luminance_ramp() {
        let mid = colormap(0.5, Colormap::Grayscale);
        assert_eq!(mid, [0.5, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn from_values_maps_domain_to_endpoints() {
        let positions = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let values = [0.0_f32, 5.0, 10.0];
        let pd = PointData::from_values(positions, values, (0.0, 10.0), Colormap::Viridis);
        assert_eq!(pd.points.len(), 3);
        // min value → t=0, max value → t=1.
        assert_eq!(pd.points[0].color, colormap(0.0, Colormap::Viridis));
        assert_eq!(pd.points[2].color, colormap(1.0, Colormap::Viridis));
    }

    #[test]
    fn from_values_degenerate_domain_is_midpoint() {
        let pd = PointData::from_values(
            [Vec3::ZERO, Vec3::X],
            [3.0_f32, 3.0],
            (3.0, 3.0),
            Colormap::Turbo,
        );
        assert_eq!(pd.points[0].color, colormap(0.5, Colormap::Turbo));
    }
}
