//! Orbit camera: interactive state, resolved view/projection, and the
//! 3D→screen projection core uses to place axis/data labels.
//!
//! Two types, split along the controlled-widget seam:
//!
//! - [`CameraState`] is the *transient interactive state* — orbit angles,
//!   zoom, pan — analogous to a scroll offset. It is owned per scene
//!   (keyed in `UiState`, wired up with the El surface and pointer
//!   routing), not by the app, so a plain `chart3d(...).orbit()` is
//!   interactive with no app state. An app may still read or override it.
//! - [`ResolvedCamera`] is the *resolved result* — concrete eye / target
//!   / up / fov / near / far — produced by [`CameraState::resolve`] from
//!   the state plus the scene's data bounds (auto-framing). It carries the
//!   glam matrices the backend uploads and the projection core uses for
//!   labels, so the camera math has one home.

use glam::{Mat4, Vec2, Vec3};

use crate::scene::bounds::Aabb;
use crate::tree::Rect;

/// Default vertical field of view (radians). Auto-framing fits the data
/// bounds to this fov.
pub const DEFAULT_FOV_Y_RADIANS: f32 = std::f32::consts::FRAC_PI_4; // 45°

/// Pitch is clamped just shy of the poles so the up vector never
/// degenerates and orbit stays stable.
const MAX_PITCH: f32 = 1.483_530; // ~85°
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 50.0;

/// Transient orbit-camera state for one scene. Defaults to a pleasant
/// three-quarter view fitted to the data; gestures mutate it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    /// Azimuth around +Y, radians.
    pub yaw: f32,
    /// Elevation, radians; clamped to ±~85° by [`CameraState::orbit`].
    pub pitch: f32,
    /// Multiplier on the auto-fit distance. `1.0` frames the bounds;
    /// `> 1` pulls back, `< 1` moves in.
    pub zoom: f32,
    /// World-space offset added to the framing centre (pan). Gestures
    /// accumulate this; how screen drag maps to world units is the input
    /// layer's job, since it depends on distance.
    pub pan: Vec3,
    /// Whether this state has been auto-framed for the current bounds.
    /// Cleared when the app wants a re-frame (e.g. bounds changed). The
    /// resolve path does not read it — it always frames from current
    /// bounds — but the input/runtime layer uses it to decide when to
    /// reset `pan`/`zoom` to defaults.
    pub framed: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            yaw: std::f32::consts::FRAC_PI_4, // 45°
            pitch: 0.523_599,                 // 30°
            zoom: 1.0,
            pan: Vec3::ZERO,
            framed: false,
        }
    }
}

impl CameraState {
    /// Orbit by deltas (radians). Yaw wraps; pitch clamps near the poles.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + d_pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Multiply the zoom (distance) by `factor`, clamped to a sane range.
    /// `factor > 1` pulls the camera back.
    pub fn zoom_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        }
    }

    /// Pan the framing centre by a world-space delta.
    pub fn pan_by(&mut self, delta: Vec3) {
        self.pan += delta;
    }

    /// Resolve to a concrete camera framed on `bounds`. An invalid/empty
    /// box frames a unit sphere at the origin, so an empty scene still has
    /// a sensible camera.
    pub fn resolve(&self, bounds: Aabb) -> ResolvedCamera {
        let fov_y = DEFAULT_FOV_Y_RADIANS;
        let (center, radius) = if bounds.is_valid() {
            let r = bounds.bounding_radius();
            (bounds.center(), if r > 1e-4 { r } else { 1.0 })
        } else {
            (Vec3::ZERO, 1.0)
        };

        // Distance at which a sphere of `radius` exactly fills the
        // vertical fov, scaled by the zoom multiplier.
        let fit = radius / (fov_y * 0.5).sin();
        let distance = (fit * self.zoom).max(1e-3);

        let target = center + self.pan;
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // Unit direction from target toward the eye.
        let dir = Vec3::new(cp * sy, sp, cp * cy);
        let eye = target + dir * distance;

        let near = (distance - radius).max(distance * 0.01).max(1e-3);
        let far = (distance + radius * 2.0).max(near * 8.0);

        ResolvedCamera {
            eye,
            target,
            up: Vec3::Y,
            fov_y,
            near,
            far,
        }
    }
}

/// A resolved camera: concrete framing plus the matrices and projection
/// the backend and label layer need. Stored in `Scene3DData`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedCamera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl ResolvedCamera {
    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye, self.target, self.up)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect.max(1e-4), self.near, self.far)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    /// Project a world point to screen-space (logical px) within
    /// `viewport`. Returns `None` for points at or behind the camera
    /// plane (`w <= 0`), so label callers cull them rather than drawing a
    /// mirrored ghost. Points in front but outside the rect still return
    /// `Some` — clipping to the rect is the caller's choice.
    pub fn project_to_screen(&self, world: Vec3, viewport: Rect) -> Option<Vec2> {
        let aspect = viewport.w / viewport.h.max(1e-4);
        let clip = self.view_proj(aspect) * world.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w; // x, y in [-1, 1]
        let sx = viewport.x + (ndc.x * 0.5 + 0.5) * viewport.w;
        let sy = viewport.y + (1.0 - (ndc.y * 0.5 + 0.5)) * viewport.h; // flip Y for screen
        Some(Vec2::new(sx, sy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> Aabb {
        Aabb::from_points([Vec3::splat(-1.0), Vec3::splat(1.0)])
    }

    #[test]
    fn resolve_frames_bounds() {
        let cam = CameraState::default().resolve(unit_box());
        // Target is the box centre (no pan).
        assert!((cam.target - Vec3::ZERO).length() < 1e-5);
        // Eye sits the fit distance away; with default zoom the box fits,
        // so the eye is outside the box's bounding radius.
        let dist = (cam.eye - cam.target).length();
        assert!(dist > unit_box().bounding_radius());
        assert!(cam.near > 0.0 && cam.far > cam.near);
    }

    #[test]
    fn target_projects_near_viewport_centre() {
        let cam = CameraState::default().resolve(unit_box());
        let vp = Rect::new(0.0, 0.0, 200.0, 100.0);
        let p = cam.project_to_screen(cam.target, vp).expect("target in front");
        assert!((p.x - 100.0).abs() < 0.5, "x={}", p.x);
        assert!((p.y - 50.0).abs() < 0.5, "y={}", p.y);
    }

    #[test]
    fn point_behind_camera_is_culled() {
        let cam = CameraState::default().resolve(unit_box());
        // Mirror the target across the eye → strictly behind the camera.
        let behind = cam.eye + (cam.eye - cam.target);
        assert!(cam.project_to_screen(behind, Rect::new(0.0, 0.0, 200.0, 100.0)).is_none());
    }

    #[test]
    fn orbit_and_zoom_move_the_eye() {
        let base = CameraState::default().resolve(unit_box());
        let mut s = CameraState::default();
        s.orbit(0.5, 0.0);
        let orbited = s.resolve(unit_box());
        assert!((orbited.eye - base.eye).length() > 1e-3, "orbit moved eye");

        let mut z = CameraState::default();
        z.zoom_by(2.0);
        let zoomed = z.resolve(unit_box());
        let d0 = (base.eye - base.target).length();
        let d1 = (zoomed.eye - zoomed.target).length();
        assert!((d1 - 2.0 * d0).abs() < 1e-3, "zoom doubled distance: {d0} -> {d1}");
    }

    #[test]
    fn pitch_clamps_near_pole() {
        let mut s = CameraState::default();
        s.orbit(0.0, 100.0); // absurd up-tilt
        assert!(s.pitch <= MAX_PITCH + 1e-6);
    }
}
