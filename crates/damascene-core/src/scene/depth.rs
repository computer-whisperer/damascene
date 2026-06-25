//! [`SceneDepthMap`] — a CPU-side snapshot of a scene's depth buffer,
//! produced by the backend and consumed by the draw-op pass to occlude
//! scene-anchored labels behind solid geometry.
//!
//! ## Why this exists / the frame-latency contract
//!
//! Labels are emitted in [`draw_ops`](crate::paint::draw_ops) (CPU,
//! backend-neutral) *before* any GPU work, but the only thing that knows
//! "is this world point behind the mesh?" is the scene's depth buffer,
//! which exists during the GPU pass one step later. Rather than read a
//! depth value back synchronously (a pipeline stall), the backend captures
//! the depth map each frame and feeds the *previous* frame's map back into
//! [`UiState`](crate::state::UiState). The occlusion test therefore runs
//! against a map that is a frame (or a few) stale.
//!
//! Two consequences shape the design:
//!
//! - The map carries the [`ResolvedCamera`] and viewport rect that produced
//!   it, and [`occludes`](SceneDepthMap::occludes) projects in *that* space
//!   — so the test is self-consistent even while the live camera orbits.
//!   The label is still *drawn* at the live projection; only the
//!   visible/hidden decision uses the stale map.
//! - When no map exists yet (first frames, just after a resize), callers
//!   treat every label as occluded — better a momentary missing label than
//!   a flash of labels punching through geometry.

use std::sync::Arc;

use glam::Vec3;

use crate::scene::camera::ResolvedCamera;
use crate::tree::Rect;

/// Anchors within this normalised-depth margin of the nearest surface are
/// treated as in front of it — keeps a label sitting just shy of geometry
/// from flickering as the stale map and live pose drift apart.
const DEPTH_BIAS: f32 = 1.0e-3;

/// A captured scene depth buffer plus the camera/viewport that produced it.
///
/// Depth values are row-major, length `width * height`, in normalised
/// device depth `[0, 1]` (`0` near, `1` far). The backend constructs one
/// per `Scene3D` node each frame; [`UiState`](crate::state::UiState) holds
/// the latest available map keyed by the node's `computed_id`.
#[derive(Clone, Debug)]
pub struct SceneDepthMap {
    /// The resolved camera that rendered this depth map.
    pub camera: ResolvedCamera,
    /// The scene viewport rect (logical px) at capture time.
    pub rect: Rect,
    /// Depth grid width in pixels (physical resolution of the offscreen).
    pub width: u32,
    /// Depth grid height in pixels.
    pub height: u32,
    /// Row-major normalised depth, `width * height` values.
    pub depth: Arc<[f32]>,
}

impl SceneDepthMap {
    /// Whether `world` is hidden behind solid scene geometry, judged in the
    /// camera/viewport this map was captured with.
    ///
    /// Returns `true` (occluded) for points behind the camera or projecting
    /// outside the map — the conservative choice, matching the
    /// "occlude until we know otherwise" contract above. Only geometry that
    /// writes depth (meshes) occludes; points and lines do not.
    pub fn occludes(&self, world: Vec3) -> bool {
        if self.width == 0 || self.height == 0 || self.depth.is_empty() {
            return true;
        }
        let Some((p, z)) = self.camera.project_to_screen_with_depth(world, self.rect) else {
            return true; // behind the camera
        };
        let fx = (p.x - self.rect.x) / self.rect.w.max(f32::EPSILON);
        let fy = (p.y - self.rect.y) / self.rect.h.max(f32::EPSILON);
        if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
            return true; // outside the captured viewport
        }
        let tx = ((fx * self.width as f32) as u32).min(self.width - 1);
        let ty = ((fy * self.height as f32) as u32).min(self.height - 1);
        let Some(&surface_z) = self.depth.get((ty * self.width + tx) as usize) else {
            return true;
        };
        // Occluded when the anchor is farther than the nearest surface.
        z > surface_z + DEPTH_BIAS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> ResolvedCamera {
        ResolvedCamera {
            eye: Vec3::new(0.0, 0.0, 5.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: crate::scene::camera::Projection::Perspective {
                fov_y: std::f32::consts::FRAC_PI_4,
            },
            near: 0.1,
            far: 100.0,
        }
    }

    fn rect() -> Rect {
        Rect::new(0.0, 0.0, 200.0, 200.0)
    }

    /// A 1×1 map whose single texel sits at `surface_z`.
    fn map_with(surface_z: f32) -> SceneDepthMap {
        SceneDepthMap {
            camera: camera(),
            rect: rect(),
            width: 1,
            height: 1,
            depth: Arc::from(vec![surface_z]),
        }
    }

    #[test]
    fn far_surface_does_not_occlude_a_point_in_front() {
        // Origin is in front of the eye; an all-far map (empty background)
        // never occludes it.
        assert!(!map_with(1.0).occludes(Vec3::ZERO));
    }

    #[test]
    fn near_surface_occludes_a_point_behind_it() {
        // A surface at the very front (z≈0) hides the origin behind it.
        assert!(map_with(0.0).occludes(Vec3::ZERO));
    }

    #[test]
    fn point_behind_camera_is_occluded() {
        // The eye looks toward -Z; a point behind it never projects.
        assert!(map_with(1.0).occludes(Vec3::new(0.0, 0.0, 20.0)));
    }

    #[test]
    fn point_outside_viewport_is_occluded() {
        // Far off-axis but in front → projects outside the 1×1 map.
        assert!(map_with(1.0).occludes(Vec3::new(100.0, 0.0, 0.0)));
    }

    #[test]
    fn empty_map_occludes_everything() {
        let empty = SceneDepthMap {
            camera: camera(),
            rect: rect(),
            width: 0,
            height: 0,
            depth: Arc::from(Vec::<f32>::new()),
        };
        assert!(empty.occludes(Vec3::ZERO));
    }
}
