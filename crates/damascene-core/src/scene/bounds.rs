//! Axis-aligned bounding box over [`glam::Vec3`].
//!
//! glam supplies the vectors and matrices the scene needs; the one thing
//! it has no type for is a bounding box, so the scene module defines this
//! small helper. Bounds drive camera auto-framing and axis tick ranges.

use glam::{Mat4, Vec3};

/// An axis-aligned bounding box. An empty box (no points) has
/// `min = +inf`, `max = -inf`; [`Aabb::is_valid`] reports this so callers
/// can skip framing a scene with no geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// The canonical empty box. The first [`Aabb::expand`] sets exact
    /// bounds because `min`/`max` start at the opposite infinities.
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn is_valid(self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z
    }

    pub fn expand(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(self, o: Aabb) -> Aabb {
        Aabb {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    /// Build a box enclosing all positions. Returns [`Aabb::EMPTY`] for an
    /// empty iterator.
    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Aabb {
        let mut bb = Aabb::EMPTY;
        for p in points {
            bb.expand(p);
        }
        bb
    }

    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half the diagonal length — a convenient framing radius. Zero for an
    /// empty or degenerate box.
    pub fn bounding_radius(self) -> f32 {
        if !self.is_valid() {
            return 0.0;
        }
        ((self.max - self.min) * 0.5).length()
    }

    /// The box enclosing this one after `m` is applied — transform all 8
    /// corners and re-bound. Used to combine per-mark geometry bounds (in
    /// each mark's transform) into world-space scene bounds for framing.
    /// An empty box stays empty.
    pub fn transformed(self, m: Mat4) -> Aabb {
        if !self.is_valid() {
            return Aabb::EMPTY;
        }
        let mut out = Aabb::EMPTY;
        for i in 0..8u8 {
            let corner = Vec3::new(
                if i & 1 == 0 { self.min.x } else { self.max.x },
                if i & 2 == 0 { self.min.y } else { self.max.y },
                if i & 4 == 0 { self.min.z } else { self.max.z },
            );
            out.expand(m.transform_point3(corner));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_points() {
        let bb = Aabb::from_points([Vec3::new(-1.0, 0.0, 2.0), Vec3::new(3.0, -4.0, 0.0)]);
        assert!(bb.is_valid());
        assert_eq!(bb.min, Vec3::new(-1.0, -4.0, 0.0));
        assert_eq!(bb.max, Vec3::new(3.0, 0.0, 2.0));
        assert_eq!(bb.center(), Vec3::new(1.0, -2.0, 1.0));
    }

    #[test]
    fn empty_is_invalid_with_zero_radius() {
        assert!(!Aabb::EMPTY.is_valid());
        assert_eq!(Aabb::EMPTY.bounding_radius(), 0.0);
        assert_eq!(Aabb::from_points([]), Aabb::EMPTY);
    }

    #[test]
    fn transformed_translates_and_stays_empty_when_empty() {
        let bb = Aabb::from_points([Vec3::splat(-1.0), Vec3::splat(1.0)]);
        let moved = bb.transformed(Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)));
        assert_eq!(moved.min, Vec3::new(9.0, -1.0, -1.0));
        assert_eq!(moved.max, Vec3::new(11.0, 1.0, 1.0));
        assert!(!Aabb::EMPTY.transformed(Mat4::IDENTITY).is_valid());
    }
}
