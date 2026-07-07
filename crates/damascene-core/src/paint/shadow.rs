//! Elevation → drop-shadow recipes (Tailwind's box-shadow scale).
//!
//! An element's `shadow` stays a single scalar "elevation level"
//! end-to-end (`El::shadow(f32)` → the `shadow` uniform → `params.z`
//! in `stock::rounded_rect`), but the *rendering* of a level is a
//! Tailwind-style **two-layer** shadow: each layer is `(dy, blur,
//! spread, alpha)` in CSS box-shadow terms, with the negative spreads
//! that give Tailwind its tight penumbra. The stock tokens anchor the
//! scale exactly on Tailwind v4:
//!
//! | level | Tailwind    | layer 1           | layer 2            |
//! |-------|-------------|-------------------|--------------------|
//! | 2     | shadow-xs   | 0 1 2 0 / 0.05    | —                  |
//! | 4     | shadow-sm   | 0 1 3 0 / 0.10    | 0 1 2 -1 / 0.10    |
//! | 12    | shadow-md   | 0 4 6 -1 / 0.10   | 0 2 4 -2 / 0.10    |
//! | 24    | shadow-lg   | 0 10 15 -3 / 0.10 | 0 4 6 -4 / 0.10    |
//! | 40    | shadow-xl   | 0 20 25 -5 / 0.10 | 0 8 10 -6 / 0.10   |
//!
//! Levels between anchors interpolate componentwise (so a zoomed
//! level stays smooth); levels past 40 scale the xl geometry
//! proportionally. Alphas are the raw CSS values — SVG export uses
//! them as-is (SVG rasterizers composite in sRGB like browsers), while
//! `rounded_rect.wgsl` applies the same dark-over-light gamma
//! compensation as glyph coverage so linear-light blending lands on
//! the browser look.
//!
//! The WGSL mirror of this table lives in
//! `shaders/rounded_rect.wgsl` (`shadow_layer1` / `shadow_layer2`);
//! keep the two in sync — `tests::wgsl_table_matches` greps the
//! shader source for each anchor row.

/// One shadow layer in CSS box-shadow terms: `[dy, blur, spread,
/// alpha]`, logical px (x-offset is always 0 in the Tailwind scale).
pub type ShadowLayer = [f32; 4];

/// Elevation anchors: `(level, [layer1, layer2])`. See module docs.
pub const SHADOW_ANCHORS: [(f32, [ShadowLayer; 2]); 6] = [
    (0.0, [[0.0; 4], [0.0; 4]]),
    (2.0, [[1.0, 2.0, 0.0, 0.05], [0.0; 4]]),
    (4.0, [[1.0, 3.0, 0.0, 0.10], [1.0, 2.0, -1.0, 0.10]]),
    (12.0, [[4.0, 6.0, -1.0, 0.10], [2.0, 4.0, -2.0, 0.10]]),
    (24.0, [[10.0, 15.0, -3.0, 0.10], [4.0, 6.0, -4.0, 0.10]]),
    (40.0, [[20.0, 25.0, -5.0, 0.10], [8.0, 10.0, -6.0, 0.10]]),
];

/// The two shadow layers an elevation level renders with.
pub fn shadow_recipe(level: f32) -> [ShadowLayer; 2] {
    if level <= 0.0 {
        return [[0.0; 4], [0.0; 4]];
    }
    let (top_level, top_layers) = SHADOW_ANCHORS[SHADOW_ANCHORS.len() - 1];
    if level >= top_level {
        // Past xl: scale the geometry proportionally, keep the alpha.
        let s = level / top_level;
        return top_layers.map(|l| [l[0] * s, l[1] * s, l[2] * s, l[3]]);
    }
    let mut lo = SHADOW_ANCHORS[0];
    for hi in SHADOW_ANCHORS[1..].iter() {
        if level <= hi.0 {
            let t = (level - lo.0) / (hi.0 - lo.0);
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            return [
                std::array::from_fn(|i| lerp(lo.1[0][i], hi.1[0][i])),
                std::array::from_fn(|i| lerp(lo.1[1][i], hi.1[1][i])),
            ];
        }
        lo = *hi;
    }
    unreachable!("level < top anchor is always bracketed");
}

/// Halo the recipe needs beyond the layout rect, as `(sides, top,
/// bottom)` in logical px: a layer's silhouette sits `spread` outside
/// the rect (negative pulls in), its penumbra reaches `blur` beyond
/// that, and the whole thing is shifted down by `dy`.
pub fn shadow_extents(level: f32) -> (f32, f32, f32) {
    let layers = shadow_recipe(level);
    let mut sides = 0.0_f32;
    let mut top = 0.0_f32;
    let mut bottom = 0.0_f32;
    for l in layers {
        if l[3] <= 0.0 {
            continue;
        }
        let reach = l[1] + l[2]; // blur + spread
        sides = sides.max(reach);
        top = top.max(reach - l[0]);
        bottom = bottom.max(reach + l[0]);
    }
    (sides.max(0.0), top.max(0.0), bottom.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens;

    #[test]
    fn anchors_reproduce_tailwind_recipes_exactly() {
        assert_eq!(
            shadow_recipe(tokens::SHADOW_XS),
            [[1.0, 2.0, 0.0, 0.05], [0.0; 4]]
        );
        assert_eq!(
            shadow_recipe(tokens::SHADOW_SM),
            [[1.0, 3.0, 0.0, 0.10], [1.0, 2.0, -1.0, 0.10]]
        );
        assert_eq!(
            shadow_recipe(tokens::SHADOW_MD),
            [[4.0, 6.0, -1.0, 0.10], [2.0, 4.0, -2.0, 0.10]]
        );
        assert_eq!(
            shadow_recipe(tokens::SHADOW_LG),
            [[10.0, 15.0, -3.0, 0.10], [4.0, 6.0, -4.0, 0.10]]
        );
    }

    #[test]
    fn zero_and_negative_levels_have_no_shadow() {
        assert_eq!(shadow_recipe(0.0), [[0.0; 4], [0.0; 4]]);
        assert_eq!(shadow_recipe(-3.0), [[0.0; 4], [0.0; 4]]);
        assert_eq!(shadow_extents(0.0), (0.0, 0.0, 0.0));
    }

    #[test]
    fn interpolation_is_smooth_and_monotone() {
        // Between sm (4) and md (12) every geometric component grows.
        let a = shadow_recipe(5.0);
        let b = shadow_recipe(11.0);
        assert!(a[0][1] < b[0][1], "blur grows with level");
        assert!(a[0][0] < b[0][0], "dy grows with level");
        // Past xl the geometry keeps scaling, alpha stays put.
        let xl = shadow_recipe(40.0);
        let xxl = shadow_recipe(80.0);
        assert_eq!(xxl[0][1], xl[0][1] * 2.0);
        assert_eq!(xxl[0][3], xl[0][3]);
    }

    #[test]
    fn extents_cover_the_softened_silhouette() {
        // lg: layer1 blur 15, spread −3, dy 10 → sides 12, top 2,
        // bottom 22 (layer2's 6−4±4 is smaller everywhere).
        assert_eq!(shadow_extents(tokens::SHADOW_LG), (12.0, 2.0, 22.0));
        // xs: single layer blur 2, dy 1.
        assert_eq!(shadow_extents(tokens::SHADOW_XS), (2.0, 1.0, 3.0));
    }

    #[test]
    fn wgsl_table_matches() {
        // The shader mirrors SHADOW_ANCHORS as WGSL constants; a silent
        // drift would render different shadows than draw_ops reserves
        // room for (clipped halos). Require every anchor row's numbers
        // to appear in the shader source.
        let wgsl = include_str!("../../shaders/rounded_rect.wgsl");
        for (level, layers) in SHADOW_ANCHORS[1..].iter() {
            for (li, l) in layers.iter().enumerate() {
                if l[3] <= 0.0 {
                    continue;
                }
                let row = format!("vec4<f32>({:?}, {:?}, {:?}, {:?})", l[0], l[1], l[2], l[3]);
                assert!(
                    wgsl.contains(&row),
                    "rounded_rect.wgsl is missing anchor level {level} layer {li}: {row}"
                );
            }
        }
    }
}
