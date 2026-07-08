//! Orbit camera: absolute pose, framing policy, resolved view/projection,
//! and the 3D→screen projection core uses to place axis/data labels.
//!
//! Two types, split along the controlled-widget seam:
//!
//! - [`CameraState`] is an *absolute, persistent orbit pose* — world-space
//!   `target` / `distance` / `yaw` / `pitch`, like the volumetric
//!   renderer's camera. It is not re-derived from content each frame;
//!   gestures and programmatic moves mutate it, and (once keyed in
//!   `UiState`) it persists across frames. Whether it auto-frames the data
//!   is the separate, configurable [`Framing`] policy.
//! - [`ResolvedCamera`] is the *resolved result* — concrete eye / target
//!   / up / fov / near / far — produced by [`CameraState::resolve`] from
//!   the pose plus the scene's full view bounds (for near/far). It carries
//!   the glam matrices the backend uploads and the projection core uses for
//!   labels, so the camera math has one home.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use glam::{Mat4, Vec2, Vec3};

use crate::scene::bounds::Aabb;
use crate::tree::Rect;

/// Default vertical field of view (radians). Framing fits the data
/// bounds to this fov.
pub const DEFAULT_FOV_Y_RADIANS: f32 = std::f32::consts::FRAC_PI_4; // 45°

/// Pitch is clamped just shy of the poles so the up vector never
/// degenerates and orbit stays stable.
const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2 - 0.087; // ~5° shy of the pole (~85°)
/// Absolute eye-distance clamps. Wide range — small graphs sit near the
/// bottom, but the camera is a general 3D navigator.
const MIN_DISTANCE: f32 = 1.0e-3;
const MAX_DISTANCE: f32 = 1.0e6;

/// How the camera relates to the scene's data bounds. Decouples "where the
/// camera is" (the absolute [`CameraState`] pose) from "should it track the
/// data" (this policy).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Framing {
    /// Fit the content once, then navigate freely; re-centre on the data
    /// when its bounds change (smoothly, once the keyed camera animates).
    /// The default — "show me the data, then let me look around".
    #[default]
    Auto,
    /// Re-fit the content every frame. For static viewers that should
    /// always frame the data regardless of navigation.
    Fit,
    /// Never auto-fit; the app owns the absolute pose. For app-driven
    /// cameras and fixed viewpoints.
    Manual,
}

/// Pointer navigation scheme for a scene camera, matching the conventions
/// of popular 3D apps. The app picks one on the spec; there is
/// deliberately no built-in scheme-picker widget. The wheel always zooms,
/// regardless of scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CameraControls {
    /// Widget default: left-drag orbits, Shift+left or right-drag pans,
    /// wheel zooms. Left-drag is free to use here — a chart/widget has no
    /// selection to preserve, unlike a 3D editor.
    #[default]
    Orbit,
    /// Blender / Fusion 360: middle-drag orbits, Shift+middle-drag pans.
    Blender,
    /// OnShape: right-drag orbits, middle-drag pans.
    OnShape,
    /// Maya: Alt+left orbits, Alt+middle pans, Alt+right dollies (zoom).
    Maya,
}

/// A declarative camera focus request, set on the scene spec. Whenever it
/// *changes*, the keyed camera animates (springs) to it — so an app can
/// "look here" smoothly by swapping the value in its build. Orbit angles
/// are preserved; only the look-at point and distance move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Focus {
    /// Frame these world-space bounds (centre + fit distance).
    Bounds(Aabb),
    /// Look at a world point from an explicit distance.
    Point {
        /// World-space look-at point.
        target: Vec3,
        /// Eye distance from `target`, world units.
        distance: f32,
    },
}

/// Absolute, persistent orbit-camera pose for one scene. World-space —
/// not re-derived from content each frame (see [`Framing`]). Defaults to a
/// pleasant three-quarter view of a unit sphere at the origin; [`fitted`]
/// re-frames it to data, gestures and programmatic moves mutate it.
///
/// [`fitted`]: CameraState::fitted
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    /// Look-at point in world space.
    pub target: Vec3,
    /// Eye distance from `target`. Multiplicative [`zoom_by`](Self::zoom_by)
    /// keeps the perceived zoom rate constant at any scale.
    pub distance: f32,
    /// Azimuth around +Y, radians.
    pub yaw: f32,
    /// Elevation, radians; clamped to ±~85° by [`orbit`](Self::orbit).
    pub pitch: f32,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            // Frames a unit-radius sphere at the default fov.
            distance: 1.0 / (DEFAULT_FOV_Y_RADIANS * 0.5).sin(),
            yaw: std::f32::consts::FRAC_PI_4,   // 45°
            pitch: std::f32::consts::FRAC_PI_6, // 30°
        }
    }
}

impl CameraState {
    /// Orbit by angular deltas (radians). Pitch clamps near the poles so
    /// the up vector never degenerates.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw += d_yaw;
        self.pitch = (self.pitch + d_pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Multiply the eye distance by `factor`, clamped to a sane range.
    /// `factor > 1` pulls the camera back. Multiplicative so a scroll notch
    /// covers proportional distance whether near or far.
    pub fn zoom_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
    }

    /// Translate the look-at point by a world-space delta (pan).
    pub fn pan_by(&mut self, delta: Vec3) {
        self.target += delta;
    }

    /// Distance at which a sphere of `radius` exactly fills the vertical fov.
    pub fn fit_distance(radius: f32) -> f32 {
        (radius.max(1e-4) / (DEFAULT_FOV_Y_RADIANS * 0.5).sin()).clamp(MIN_DISTANCE, MAX_DISTANCE)
    }

    /// A copy framed on `content`: `target` at the centre and `distance`
    /// fit to the bounds, **preserving the current orbit angles**. Empty
    /// bounds leave a unit sphere at the origin. This is the framing
    /// operation `Framing::Fit` / `Auto` apply.
    pub fn fitted(&self, content: Aabb) -> CameraState {
        let (center, radius) = sphere_of(content);
        CameraState {
            target: center,
            distance: Self::fit_distance(radius),
            yaw: self.yaw,
            pitch: self.pitch,
        }
    }

    /// Default angles, framed on `content`. The auto-framed starting pose.
    pub fn framing(content: Aabb) -> CameraState {
        CameraState::default().fitted(content)
    }

    /// Point the camera at `target` from `distance`, keeping orbit angles.
    pub fn look_at(&mut self, target: Vec3, distance: f32) {
        self.target = target;
        self.distance = distance.clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// A copy satisfying a [`Focus`] request, preserving orbit angles. The
    /// keyed camera springs toward this when the request changes.
    pub fn focused(&self, focus: Focus) -> CameraState {
        match focus {
            Focus::Bounds(b) => self.fitted(b),
            Focus::Point { target, distance } => {
                let mut c = *self;
                c.look_at(target, distance);
                c
            }
        }
    }

    /// World-space eye position implied by the pose.
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // Unit direction from target toward the eye.
        let dir = Vec3::new(cp * sy, sp, cp * cy);
        self.target + dir * self.distance
    }

    /// Resolve to a concrete camera. `view_bounds` is everything that
    /// should stay inside the frustum (content **and** the reference grid /
    /// axes) — near/far are sized from the eye's distance to that, *not*
    /// from the content radius, so geometry larger than the data (a big
    /// grid) is never plane-clipped. The pose is taken as-is; framing
    /// (fitting to data) is applied by the caller before resolving.
    pub fn resolve(&self, view_bounds: Aabb) -> ResolvedCamera {
        let fov_y = DEFAULT_FOV_Y_RADIANS;
        let eye = self.eye();
        let (vc, vr) = if view_bounds.is_valid() {
            let (c, r) = sphere_of(view_bounds);
            (c, r)
        } else {
            // No geometry: a sphere around the target sized to the distance.
            (self.target, self.distance.max(1e-4))
        };
        // Eye-to-view-sphere distance bounds the depth range. Near floors
        // to a small fraction of the eye distance (so geometry right in
        // front of the camera isn't clipped and depth precision scales),
        // never to the content radius.
        let d = (eye - vc).length();
        let near = (d - vr).max(self.distance * 0.02).max(1e-3);
        let far = (d + vr).max(near * 8.0);

        ResolvedCamera {
            eye,
            target: self.target,
            up: Vec3::Y,
            projection: Projection::Perspective { fov_y },
            near,
            far,
        }
    }
}

/// Bounding sphere `(center, radius)` of an Aabb. Invalid/empty bounds
/// yield a unit sphere at the origin so an empty scene still resolves.
fn sphere_of(bounds: Aabb) -> (Vec3, f32) {
    if bounds.is_valid() {
        let r = bounds.bounding_radius();
        (bounds.center(), if r > 1e-4 { r } else { 1.0 })
    } else {
        (Vec3::ZERO, 1.0)
    }
}

/// How a [`ResolvedCamera`] projects. Perspective is the 3D-scene default;
/// orthographic drives 2D plots (and the 2D-lock camera) — it maps a fixed
/// scale-space window to the rect with no foreshortening, so gridlines stay
/// aligned with the data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Perspective projection with vertical field of view `fov_y` (radians),
    /// fit to the viewport aspect.
    Perspective {
        /// Vertical field of view, radians.
        fov_y: f32,
    },
    /// Orthographic projection of the window centred on the camera's
    /// `target`, with the given half-extents in world (scale-space) units.
    /// **Aspect is ignored** — a 2D plot scales its axes independently, so
    /// the half-extents alone map the window to the full rect.
    Orthographic {
        /// Half the window width, world units.
        half_w: f32,
        /// Half the window height, world units.
        half_h: f32,
    },
}

/// A resolved camera: concrete framing plus the matrices and projection
/// the backend and label layer need. Stored in `Scene3DData`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedCamera {
    /// World-space eye position.
    pub eye: Vec3,
    /// World-space look-at point.
    pub target: Vec3,
    /// Up vector — always `Vec3::Y` from [`CameraState::resolve`]
    /// (pitch clamping keeps it from degenerating).
    pub up: Vec3,
    /// How the camera projects (perspective for 3D, orthographic for 2D).
    pub projection: Projection,
    /// Near clip-plane distance from the eye, world units.
    pub near: f32,
    /// Far clip-plane distance from the eye, world units.
    pub far: f32,
}

impl ResolvedCamera {
    /// An orthographic camera that maps the scale-space window centred at
    /// `center` with half-extents `(half_w, half_h)` to the full rect —
    /// the 2D-plot data-layer camera. Looks straight down `-Z` at the
    /// `z = 0` plane the 2D geometry lives on; aspect is ignored (the
    /// half-extents already encode the window's shape).
    pub fn orthographic(center: Vec2, half_w: f32, half_h: f32) -> ResolvedCamera {
        ResolvedCamera {
            eye: Vec3::new(center.x, center.y, 1.0),
            target: Vec3::new(center.x, center.y, 0.0),
            up: Vec3::Y,
            projection: Projection::Orthographic {
                half_w: half_w.max(1e-6),
                half_h: half_h.max(1e-6),
            },
            near: 0.0,
            far: 2.0,
        }
    }

    /// Right-handed look-at view matrix.
    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up)
    }

    /// Projection matrix for `aspect` (width/height), 0..1 depth range
    /// (wgpu convention). Perspective uses `aspect`; orthographic ignores it.
    pub fn proj(&self, aspect: f32) -> Mat4 {
        match self.projection {
            Projection::Perspective { fov_y } => glam::camera::rh::proj::directx::perspective(
                fov_y,
                aspect.max(1e-4),
                self.near,
                self.far,
            ),
            Projection::Orthographic { half_w, half_h } => {
                glam::camera::rh::proj::directx::orthographic(
                    -half_w, half_w, -half_h, half_h, self.near, self.far,
                )
            }
        }
    }

    /// `proj(aspect) * view()` — the matrix backends upload.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    /// Project a world point to screen-space (logical px) within
    /// `viewport`. Returns `None` for points at or behind the camera
    /// plane (`w <= 0`), so label callers cull them rather than drawing a
    /// mirrored ghost. Points in front but outside the rect still return
    /// `Some` — clipping to the rect is the caller's choice.
    pub fn project_to_screen(&self, world: Vec3, viewport: Rect) -> Option<Vec2> {
        self.project_to_screen_with_depth(world, viewport)
            .map(|(p, _)| p)
    }

    /// Like [`project_to_screen`](Self::project_to_screen) but also returns
    /// the point's normalised device depth in `[0, 1]` (wgpu convention:
    /// `0` near, `1` far) — the same space a `Depth32Float` buffer stores,
    /// so callers can depth-test a projected anchor against a captured
    /// scene depth map. `None` when the point is at/behind the camera.
    pub fn project_to_screen_with_depth(&self, world: Vec3, viewport: Rect) -> Option<(Vec2, f32)> {
        let aspect = viewport.w / viewport.h.max(1e-4);
        let clip = self.view_proj(aspect) * world.extend(1.0);
        if clip.w <= 0.0 || !clip.w.is_finite() {
            return None;
        }
        let ndc = clip.truncate() / clip.w; // x, y in [-1, 1]; z in [0, 1]
        if !ndc.is_finite() {
            // Non-finite world positions (a NaN sample in a plot series)
            // must not leak NaN coordinates to callers — SVG output and
            // label anchors both choke on them.
            return None;
        }
        let sx = viewport.x + (ndc.x * 0.5 + 0.5) * viewport.w;
        let sy = viewport.y + (1.0 - (ndc.y * 0.5 + 0.5)) * viewport.h; // flip Y for screen
        Some((Vec2::new(sx, sy), ndc.z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> Aabb {
        Aabb::from_points([Vec3::splat(-1.0), Vec3::splat(1.0)])
    }

    #[test]
    fn fitted_frames_bounds() {
        let cam = CameraState::framing(unit_box());
        // Target is the box centre.
        assert!((cam.target - Vec3::ZERO).length() < 1e-5);
        // Eye sits the fit distance away, outside the bounding radius.
        assert!(cam.distance > unit_box().bounding_radius());
        let r = cam.resolve(unit_box());
        assert!(r.near > 0.0 && r.far > r.near);
        // fitted preserves orbit angles.
        let mut tilted = CameraState::default();
        tilted.orbit(0.3, -0.2);
        let f = tilted.fitted(unit_box());
        assert_eq!((f.yaw, f.pitch), (tilted.yaw, tilted.pitch));
    }

    #[test]
    fn target_projects_near_viewport_centre() {
        let cam = CameraState::framing(unit_box()).resolve(unit_box());
        let vp = Rect::new(0.0, 0.0, 200.0, 100.0);
        let p = cam
            .project_to_screen(cam.target, vp)
            .expect("target in front");
        assert!((p.x - 100.0).abs() < 0.5, "x={}", p.x);
        assert!((p.y - 50.0).abs() < 0.5, "y={}", p.y);
    }

    #[test]
    fn orthographic_maps_window_to_rect() {
        // A window centred at (10, 20) spanning ±5 in x, ±2.5 in y.
        let cam = ResolvedCamera::orthographic(Vec2::new(10.0, 20.0), 5.0, 2.5);
        let vp = Rect::new(0.0, 0.0, 200.0, 100.0);
        // Centre → rect centre.
        let c = cam
            .project_to_screen(Vec3::new(10.0, 20.0, 0.0), vp)
            .expect("center in front");
        assert!((c.x - 100.0).abs() < 1e-3, "x={}", c.x);
        assert!((c.y - 50.0).abs() < 1e-3, "y={}", c.y);
        // Max corner (x right, y up) → top-right pixel (screen Y is flipped).
        let tr = cam
            .project_to_screen(Vec3::new(15.0, 22.5, 0.0), vp)
            .expect("corner in front");
        assert!((tr.x - 200.0).abs() < 1e-3, "x={}", tr.x);
        assert!((tr.y - 0.0).abs() < 1e-3, "y={}", tr.y);
        // Min corner → bottom-left.
        let bl = cam
            .project_to_screen(Vec3::new(5.0, 17.5, 0.0), vp)
            .expect("corner in front");
        assert!((bl.x - 0.0).abs() < 1e-3, "x={}", bl.x);
        assert!((bl.y - 100.0).abs() < 1e-3, "y={}", bl.y);
    }

    #[test]
    fn orthographic_ignores_aspect() {
        // Same window projected through two very different aspect ratios
        // must land at the same place — ortho ignores aspect by design.
        let cam = ResolvedCamera::orthographic(Vec2::ZERO, 1.0, 1.0);
        let a = cam.project_to_screen(Vec3::new(1.0, 0.0, 0.0), Rect::new(0.0, 0.0, 200.0, 100.0));
        let b = cam.project_to_screen(Vec3::new(1.0, 0.0, 0.0), Rect::new(0.0, 0.0, 50.0, 400.0));
        // Right edge maps to the right edge of each rect regardless of shape.
        assert!((a.unwrap().x - 200.0).abs() < 1e-3);
        assert!((b.unwrap().x - 50.0).abs() < 1e-3);
    }

    #[test]
    fn point_behind_camera_is_culled() {
        let cam = CameraState::framing(unit_box()).resolve(unit_box());
        // Mirror the target across the eye → strictly behind the camera.
        let behind = cam.eye + (cam.eye - cam.target);
        assert!(
            cam.project_to_screen(behind, Rect::new(0.0, 0.0, 200.0, 100.0))
                .is_none()
        );
    }

    #[test]
    fn orbit_and_zoom_move_the_eye() {
        let base = CameraState::framing(unit_box());
        let base_eye = base.eye();
        let mut s = base;
        s.orbit(0.5, 0.0);
        assert!((s.eye() - base_eye).length() > 1e-3, "orbit moved eye");

        // Multiplicative zoom doubles the absolute distance.
        let mut z = base;
        z.zoom_by(2.0);
        assert!((z.distance - 2.0 * base.distance).abs() < 1e-3);
    }

    #[test]
    fn pitch_clamps_near_pole() {
        let mut s = CameraState::default();
        s.orbit(0.0, 100.0); // absurd up-tilt
        assert!(s.pitch <= MAX_PITCH + 1e-6);
    }

    #[test]
    fn near_far_track_view_bounds_not_content_radius() {
        // Camera framed to small content (unit box) — the bug was near/far
        // pinned to that ~1.7 radius. A much larger view extent (a big grid)
        // must push near close to the eye and far past the grid corner, so
        // the grid isn't plane-clipped.
        let cam = CameraState::framing(unit_box());
        let grid = Aabb::from_points([Vec3::splat(-10.0), Vec3::splat(10.0)]);
        let r = cam.resolve(grid);

        // Near is a small fraction of the eye distance — NOT ~distance-radius.
        assert!(
            r.near <= cam.distance * 0.05,
            "near should hug the camera, got {} (distance {})",
            r.near,
            cam.distance
        );
        // Far reaches past the farthest grid corner from the eye.
        let far_corner = Vec3::splat(10.0).max(Vec3::splat(-10.0));
        let dist_to_far = (cam.eye() - far_corner).length();
        assert!(
            r.far >= dist_to_far,
            "far {} must cover the far grid corner at {}",
            r.far,
            dist_to_far
        );
    }
}
