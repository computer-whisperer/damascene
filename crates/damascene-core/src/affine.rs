//! 2D affine transforms.
//!
//! [`Affine2`] is a 2x3 affine matrix used by surface compositing to
//! apply rotation / scale / translation / shear to an app-owned texture
//! while it is being written into its destination rect. The matrix is
//! stored column-major as `(a, b, c, d, tx, ty)` so an Damascene point
//! `(x, y)` maps to `(a*x + c*y + tx, b*x + d*y + ty)`. The bottom row
//! of the implicit 3x3 is `[0, 0, 1]` — pure affine, no perspective.
//!
//! Composition uses the math convention: `A * B` applied to `v` is
//! `A * (B * v)`. So `Affine2::rotate(t) * Affine2::scale(s) *
//! Affine2::translate(tx, ty)` applied to a point translates first,
//! then scales, then rotates.
//!
//! ```
//! use damascene_core::affine::Affine2;
//!
//! let m = Affine2::translate(10.0, 0.0);
//! let (x, y) = m.transform_point(1.0, 2.0);
//! assert_eq!((x, y), (11.0, 2.0));
//!
//! // Rotate-around-origin then translate: composed as T * R.
//! let m = Affine2::translate(5.0, 0.0) * Affine2::rotate(std::f32::consts::FRAC_PI_2);
//! let (x, y) = m.transform_point(1.0, 0.0);
//! assert!((x - 5.0).abs() < 1e-5);
//! assert!((y - 1.0).abs() < 1e-5);
//! ```

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::ops::Mul;

/// A 2x3 affine matrix:
///
/// ```text
/// [ a  c  tx ]
/// [ b  d  ty ]
/// ```
///
/// Default is the identity. See module docs for composition semantics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2 {
    /// Row 0, column 0 — x-from-x factor (cosine term of a rotation).
    pub a: f32,
    /// Row 1, column 0 — y-from-x factor (sine term of a rotation).
    pub b: f32,
    /// Row 0, column 1 — x-from-y factor (negative sine of a rotation).
    pub c: f32,
    /// Row 1, column 1 — y-from-y factor (cosine term of a rotation).
    pub d: f32,
    /// Translation on x.
    pub tx: f32,
    /// Translation on y.
    pub ty: f32,
}

impl Affine2 {
    /// The identity transform — leaves every point unchanged.
    pub const IDENTITY: Affine2 = Affine2 {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// A pure translation by `(x, y)`.
    pub const fn translate(x: f32, y: f32) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: x,
            ty: y,
        }
    }

    /// A uniform scale by `s` around the origin.
    pub const fn scale(s: f32) -> Self {
        Self::scale_xy(s, s)
    }

    /// A non-uniform scale by `(sx, sy)` around the origin. Negative
    /// factors flip on that axis (e.g. `scale_xy(-1.0, 1.0)` mirrors
    /// horizontally).
    pub const fn scale_xy(sx: f32, sy: f32) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// A rotation by `radians` around the origin. Positive angles
    /// rotate from +x toward +y (which is "clockwise" in Damascene's
    /// y-down screen space).
    pub fn rotate(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self {
            a: c,
            b: s,
            c: -s,
            d: c,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Apply this matrix to a point.
    pub fn transform_point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// True if this is exactly the identity matrix. Backends use this
    /// to short-circuit the affine path on the common case.
    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }
}

impl Default for Affine2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mul for Affine2 {
    type Output = Affine2;

    fn mul(self, rhs: Affine2) -> Affine2 {
        Affine2 {
            a: self.a * rhs.a + self.c * rhs.b,
            b: self.b * rhs.a + self.d * rhs.b,
            c: self.a * rhs.c + self.c * rhs.d,
            d: self.b * rhs.c + self.d * rhs.d,
            tx: self.a * rhs.tx + self.c * rhs.ty + self.tx,
            ty: self.b * rhs.tx + self.d * rhs.ty + self.ty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn identity_leaves_points_unchanged() {
        let m = Affine2::IDENTITY;
        assert_eq!(m.transform_point(3.0, 4.0), (3.0, 4.0));
    }

    #[test]
    fn translate_offsets_points() {
        let m = Affine2::translate(2.0, 3.0);
        assert_eq!(m.transform_point(0.0, 0.0), (2.0, 3.0));
        assert_eq!(m.transform_point(1.0, 1.0), (3.0, 4.0));
    }

    #[test]
    fn scale_uniform_and_xy() {
        assert_eq!(Affine2::scale(2.0).transform_point(3.0, 4.0), (6.0, 8.0));
        assert_eq!(
            Affine2::scale_xy(-1.0, 1.0).transform_point(5.0, 5.0),
            (-5.0, 5.0)
        );
    }

    #[test]
    fn rotate_quarter_turn() {
        let m = Affine2::rotate(std::f32::consts::FRAC_PI_2);
        let (x, y) = m.transform_point(1.0, 0.0);
        assert!(approx_eq(x, 0.0) && approx_eq(y, 1.0));
    }

    #[test]
    fn composition_applies_right_factor_first() {
        // T * R applied to (1,0): rotate first → (0,1), then translate → (5,1).
        let m = Affine2::translate(5.0, 0.0) * Affine2::rotate(std::f32::consts::FRAC_PI_2);
        let (x, y) = m.transform_point(1.0, 0.0);
        assert!(approx_eq(x, 5.0) && approx_eq(y, 1.0));
    }

    #[test]
    fn composition_with_identity_is_idempotent() {
        let m = Affine2::rotate(0.4) * Affine2::scale(1.5);
        assert_eq!(m * Affine2::IDENTITY, m);
        assert_eq!(Affine2::IDENTITY * m, m);
    }

    #[test]
    fn is_identity_only_for_identity() {
        assert!(Affine2::IDENTITY.is_identity());
        assert!(!Affine2::translate(0.0, 1.0).is_identity());
        assert!(!Affine2::scale(1.000_001).is_identity());
    }
}
