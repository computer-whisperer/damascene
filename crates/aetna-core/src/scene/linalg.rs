//! Minimal 3D linear algebra for the scene module.
//!
//! `aetna-core` is deliberately dependency-light and 2D-shaped: it has
//! [`crate::affine::Affine2`] for UI transforms but no 3D vector/matrix
//! types, and it does not depend on `glam`. The scene draw-op needs just
//! enough 3D math to resolve a camera and project label anchors to
//! screen space (the backends build their own GPU-side matrices). This
//! module supplies that minimum — `Vec3`, a column-major `Mat4`, and an
//! axis-aligned bounding box — and nothing more.
//!
//! Conventions match wgpu / glam so a `Mat4` built here can be uploaded
//! to a backend uniform unchanged: right-handed, column-major storage,
//! and a perspective projection whose clip-space depth is `[0, 1]`.

/// A 3-component vector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    pub const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub const fn splat(v: f32) -> Self {
        Self { x: v, y: v, z: v }
    }

    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    pub fn scale(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Normalize, returning [`Vec3::Y`] for a (near-)zero vector so
    /// callers never divide by zero building a basis.
    pub fn normalize_or_up(self) -> Vec3 {
        let len = self.length();
        if len <= f32::EPSILON {
            Vec3::Y
        } else {
            self.scale(1.0 / len)
        }
    }

    pub fn min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    pub fn max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }

    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

impl From<[f32; 3]> for Vec3 {
    fn from(a: [f32; 3]) -> Self {
        Vec3::new(a[0], a[1], a[2])
    }
}

impl From<Vec3> for [f32; 3] {
    fn from(v: Vec3) -> Self {
        v.to_array()
    }
}

/// A 4x4 matrix in column-major storage: `cols[c]` is column `c`, so
/// `cols[c][r]` is row `r` of column `c` — the layout wgpu expects for a
/// `mat4x4<f32>` uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    pub cols: [[f32; 4]; 4],
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        cols: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// Matrix product `self * rhs`.
    pub fn mul(self, rhs: Mat4) -> Mat4 {
        let mut out = [[0.0f32; 4]; 4];
        for (c, out_col) in out.iter_mut().enumerate() {
            for r in 0..4 {
                out_col[r] = self.cols[0][r] * rhs.cols[c][0]
                    + self.cols[1][r] * rhs.cols[c][1]
                    + self.cols[2][r] * rhs.cols[c][2]
                    + self.cols[3][r] * rhs.cols[c][3];
            }
        }
        Mat4 { cols: out }
    }

    /// Right-handed look-at view matrix (matches `glam::Mat4::look_at_rh`).
    pub fn look_at_rh(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
        let f = target.sub(eye).normalize_or_up();
        let s = f.cross(up).normalize_or_up();
        let u = s.cross(f);
        Mat4 {
            cols: [
                [s.x, u.x, -f.x, 0.0],
                [s.y, u.y, -f.y, 0.0],
                [s.z, u.z, -f.z, 0.0],
                [-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0],
            ],
        }
    }

    /// Right-handed perspective projection with clip-space depth in
    /// `[0, 1]` (matches `glam::Mat4::perspective_rh`, the wgpu/Vulkan
    /// convention).
    pub fn perspective_rh(fov_y_radians: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let (sin, cos) = (0.5 * fov_y_radians).sin_cos();
        let h = cos / sin;
        let w = h / aspect.max(f32::EPSILON);
        let r = far / (near - far);
        Mat4 {
            cols: [
                [w, 0.0, 0.0, 0.0],
                [0.0, h, 0.0, 0.0],
                [0.0, 0.0, r, -1.0],
                [0.0, 0.0, r * near, 0.0],
            ],
        }
    }

    /// Transform a point, returning the homogeneous result `(x, y, z, w)`
    /// without the perspective divide so callers can test `w` (a point
    /// behind the camera has `w <= 0`).
    pub fn transform_point4(self, p: Vec3) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for (r, slot) in out.iter_mut().enumerate() {
            *slot = self.cols[0][r] * p.x
                + self.cols[1][r] * p.y
                + self.cols[2][r] * p.z
                + self.cols[3][r];
        }
        out
    }

    /// Row-major `[[f32; 4]; 4]` for uniforms that expect that layout.
    /// (Storage here is column-major, so this transposes.)
    pub fn to_cols_array_2d(self) -> [[f32; 4]; 4] {
        self.cols
    }
}

/// An axis-aligned bounding box. An empty box (no points) is represented
/// by `min > max`; [`Aabb::is_valid`] reports this.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// The canonical empty box: `min = +inf`, `max = -inf`, so the first
    /// [`Aabb::expand`] sets exact bounds.
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
        self.min.add(self.max).scale(0.5)
    }

    /// Half the diagonal length — a convenient framing radius. Zero for an
    /// empty or degenerate box.
    pub fn bounding_radius(self) -> f32 {
        if !self.is_valid() {
            return 0.0;
        }
        self.max.sub(self.min).scale(0.5).length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_mul_unit() {
        let m = Mat4::perspective_rh(1.0, 1.5, 0.1, 100.0);
        assert_eq!(m.mul(Mat4::IDENTITY), m);
        assert_eq!(Mat4::IDENTITY.mul(m), m);
    }

    #[test]
    fn look_at_places_eye_at_origin_looking_down_neg_z() {
        // Eye on +Z looking at origin: target maps in front (clip w > 0),
        // a point behind the eye maps to w <= 0.
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let proj = Mat4::perspective_rh(60f32.to_radians(), 1.0, 0.1, 100.0);
        let vp = proj.mul(view);
        let front = vp.transform_point4(Vec3::ZERO);
        assert!(front[3] > 0.0, "origin should be in front of the camera");
        let behind = vp.transform_point4(Vec3::new(0.0, 0.0, 10.0));
        assert!(behind[3] <= 0.0, "point behind eye should have w <= 0");
    }

    #[test]
    fn aabb_from_points() {
        let bb = Aabb::from_points([
            Vec3::new(-1.0, 0.0, 2.0),
            Vec3::new(3.0, -4.0, 0.0),
        ]);
        assert!(bb.is_valid());
        assert_eq!(bb.min, Vec3::new(-1.0, -4.0, 0.0));
        assert_eq!(bb.max, Vec3::new(3.0, 0.0, 2.0));
        assert_eq!(bb.center(), Vec3::new(1.0, -2.0, 1.0));
    }

    #[test]
    fn empty_aabb_is_invalid_with_zero_radius() {
        assert!(!Aabb::EMPTY.is_valid());
        assert_eq!(Aabb::EMPTY.bounding_radius(), 0.0);
        assert_eq!(Aabb::from_points([]), Aabb::EMPTY);
    }
}
