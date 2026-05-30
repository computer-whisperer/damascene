//! Geometry primitives used by layout, hit-testing, and painting.

/// A rectangle in **logical pixels**. The host's `scale_factor` is
/// applied at paint time, so layout, hit-testing, and `Rect`-shaped
/// API arguments all speak the same un-scaled coordinate space.
///
/// Origin top-left, +y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn center_x(self) -> f32 {
        self.x + self.w * 0.5
    }

    pub fn center_y(self) -> f32 {
        self.y + self.h * 0.5
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 {
            return None;
        }
        if y2 <= y1 {
            return None;
        }
        Some(Rect::new(x1, y1, x2 - x1, y2 - y1))
    }

    pub fn inset(self, p: Sides) -> Self {
        Self::new(
            self.x + p.left,
            self.y + p.top,
            (self.w - p.left - p.right).max(0.0),
            (self.h - p.top - p.bottom).max(0.0),
        )
    }

    /// Inverse of [`Self::inset`]: extend the rect outward by `p` on each side.
    pub fn outset(self, p: Sides) -> Self {
        Self::new(
            self.x - p.left,
            self.y - p.top,
            self.w + p.left + p.right,
            self.h + p.top + p.bottom,
        )
    }
}

/// Per-side padding/inset values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sides {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Sides {
    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }

    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            left: x,
            right: x,
            top: y,
            bottom: y,
        }
    }

    /// Horizontal-only padding — sets `left` and `right` to `v`,
    /// leaves `top` and `bottom` at `0`. Mirrors Tailwind's `px-N`.
    pub const fn x(v: f32) -> Self {
        Self {
            left: v,
            right: v,
            top: 0.0,
            bottom: 0.0,
        }
    }

    /// Vertical-only padding — sets `top` and `bottom` to `v`,
    /// leaves `left` and `right` at `0`. Mirrors Tailwind's `py-N`.
    pub const fn y(v: f32) -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: v,
            bottom: v,
        }
    }

    /// Left-only padding — sets `left` to `v`, leaves the other three
    /// at `0`. Mirrors Tailwind's `pl-N` and [`Corners::left`].
    pub const fn left(v: f32) -> Self {
        Self {
            left: v,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        }
    }

    /// Right-only padding — sets `right` to `v`, leaves the other three
    /// at `0`. Mirrors Tailwind's `pr-N` and [`Corners::right`].
    pub const fn right(v: f32) -> Self {
        Self {
            left: 0.0,
            right: v,
            top: 0.0,
            bottom: 0.0,
        }
    }

    /// Top-only padding — sets `top` to `v`, leaves the other three
    /// at `0`. Mirrors Tailwind's `pt-N` and [`Corners::top`].
    pub const fn top(v: f32) -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: v,
            bottom: 0.0,
        }
    }

    /// Bottom-only padding — sets `bottom` to `v`, leaves the other
    /// three at `0`. Mirrors Tailwind's `pb-N` and [`Corners::bottom`].
    pub const fn bottom(v: f32) -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: v,
        }
    }

    pub const fn zero() -> Self {
        Self::all(0.0)
    }
}

impl From<f32> for Sides {
    fn from(v: f32) -> Self {
        Sides::all(v)
    }
}

/// Per-corner radius values, in logical pixels.
///
/// `radius` is authored as a single scalar in the common case
/// (`.radius(tokens::RADIUS_MD)` works via [`From<f32>`]). Per-corner
/// shapes are built with [`Corners::top`], [`Corners::bottom`],
/// [`Corners::left`], [`Corners::right`], or by constructing the
/// struct directly. The painter clamps each corner to half the shorter
/// side, so over-large values render as a pill on that corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl Corners {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        }
    }

    /// Round the top two corners (`tl`, `tr`); leave `bl` / `br` at 0.
    /// Use this on a header strip nested in a rounded card so the
    /// strip's top corners follow the card's curve.
    pub const fn top(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            br: 0.0,
            bl: 0.0,
        }
    }

    /// Round the bottom two corners (`bl`, `br`); leave `tl` / `tr` at 0.
    pub const fn bottom(r: f32) -> Self {
        Self {
            tl: 0.0,
            tr: 0.0,
            br: r,
            bl: r,
        }
    }

    /// Round the left two corners (`tl`, `bl`); leave `tr` / `br` at 0.
    pub const fn left(r: f32) -> Self {
        Self {
            tl: r,
            tr: 0.0,
            br: 0.0,
            bl: r,
        }
    }

    /// Round the right two corners (`tr`, `br`); leave `tl` / `bl` at 0.
    pub const fn right(r: f32) -> Self {
        Self {
            tl: 0.0,
            tr: r,
            br: r,
            bl: 0.0,
        }
    }

    /// True when every corner has the same radius. The painter takes
    /// a fast path on uniform corners, and SVG bundle output emits
    /// `<rect rx>` rather than a `<path>`.
    pub fn is_uniform(self) -> bool {
        self.tl == self.tr && self.tr == self.br && self.br == self.bl
    }

    /// True when at least one corner has a non-zero radius.
    pub fn any_nonzero(self) -> bool {
        self.tl > 0.0 || self.tr > 0.0 || self.br > 0.0 || self.bl > 0.0
    }

    /// Largest of the four corner radii. The painter uses this for
    /// shadow / focus-ring SDF approximation, where "loosely the
    /// silhouette of the rounded shape" is enough.
    pub fn max(self) -> f32 {
        self.tl.max(self.tr).max(self.br).max(self.bl)
    }

    /// Pack as a `[f32; 4]` in `(tl, tr, br, bl)` order — the layout the
    /// shader's `slot_e` instance attribute expects.
    pub fn to_array(self) -> [f32; 4] {
        [self.tl, self.tr, self.br, self.bl]
    }
}

impl From<f32> for Corners {
    fn from(r: f32) -> Self {
        Corners::all(r)
    }
}

#[cfg(test)]
mod corners_tests {
    use super::*;

    #[test]
    fn shorthand_constructors_only_round_their_named_corners() {
        let top = Corners::top(8.0);
        assert_eq!(
            top,
            Corners {
                tl: 8.0,
                tr: 8.0,
                br: 0.0,
                bl: 0.0
            }
        );

        let bottom = Corners::bottom(8.0);
        assert_eq!(
            bottom,
            Corners {
                tl: 0.0,
                tr: 0.0,
                br: 8.0,
                bl: 8.0
            }
        );

        let left = Corners::left(8.0);
        assert_eq!(
            left,
            Corners {
                tl: 8.0,
                tr: 0.0,
                br: 0.0,
                bl: 8.0
            }
        );

        let right = Corners::right(8.0);
        assert_eq!(
            right,
            Corners {
                tl: 0.0,
                tr: 8.0,
                br: 8.0,
                bl: 0.0
            }
        );
    }

    #[test]
    fn is_uniform_is_true_only_when_all_four_corners_match() {
        assert!(Corners::all(8.0).is_uniform());
        assert!(Corners::ZERO.is_uniform());
        assert!(!Corners::top(8.0).is_uniform());
    }

    #[test]
    fn from_f32_produces_uniform_corners_for_back_compat() {
        // Existing call sites do `.radius(tokens::RADIUS_MD)` against
        // an f32; the chainable accepts `impl Into<Corners>` and the
        // float promotes to uniform corners.
        let c: Corners = 12.0_f32.into();
        assert_eq!(c, Corners::all(12.0));
    }
}
