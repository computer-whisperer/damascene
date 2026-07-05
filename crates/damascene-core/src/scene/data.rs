//! `Scene3DData`: the backend-neutral payload of a `DrawOp::Scene3D`.
//!
//! Assembled fresh each frame from the El tree's marks (cheap — it holds
//! `Arc`-cloned [geometry handles](crate::scene::GeometryHandle), not
//! geometry copies) plus the resolved camera, light rig, and style. The
//! backend walks these draw lists, uploads any geometry whose revision
//! advanced, and renders. See `docs/SCENE3D_PLAN.md`.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use glam::Mat4;

use crate::scene::bounds::Aabb;
use crate::scene::camera::ResolvedCamera;
use crate::scene::geometry::{LinesHandle, MeshHandle, PointsHandle};
use crate::scene::style::{LightRig, LineStyle, Material, PointStyle, SceneStyle};

/// A mesh mark: geometry handle + object→world transform + material.
#[derive(Clone, Debug)]
pub struct MeshDraw {
    /// Shared handle to the mesh geometry (uploaded once per revision).
    pub geometry: MeshHandle,
    /// Object→world transform applied to the geometry.
    pub transform: Mat4,
    /// Surface material; alpha < 1 routes through the translucent path.
    pub material: Material,
}

/// A point/scatter mark: geometry handle + transform + style (per-point
/// colour is in the geometry).
#[derive(Clone, Debug)]
pub struct PointDraw {
    /// Shared handle to the point geometry (uploaded once per revision).
    pub geometry: PointsHandle,
    /// Object→world transform applied to the points.
    pub transform: Mat4,
    /// Marker size, shape, and size mode shared by every point in the mark.
    pub style: PointStyle,
    /// Per-point text labels / hover tooltips. `None` = unlabelled. CPU-only
    /// presentation (not uploaded); see [`PointLabels`](crate::scene::PointLabels).
    pub labels: Option<crate::scene::labels::PointLabels>,
    /// Presentation hint: these discs exist only to patch the GPU line
    /// pipeline's butt-cap joins (one disc per polyline vertex, sized to
    /// the line width). Vector backends that draw round joins natively —
    /// the SVG bundle fallback — skip these draws entirely; they are
    /// visually redundant there and dominate output size. `false` for
    /// real data markers (scatter, chart3d points).
    pub line_joins: bool,
}

/// A line mark: geometry handle + transform + style (per-segment colour is
/// in the geometry).
#[derive(Clone, Debug)]
pub struct LineDraw {
    /// Shared handle to the line geometry (uploaded once per revision).
    pub geometry: LinesHandle,
    /// Object→world transform applied to the segments.
    pub transform: Mat4,
    /// Width, pattern, and size mode shared by every segment in the mark.
    pub style: LineStyle,
}

/// Everything a backend needs to render one scene, all backend-neutral.
#[derive(Clone, Debug)]
pub struct Scene3DData {
    /// Mesh marks; opaque ones draw first, translucent ones back-to-front.
    pub meshes: Vec<MeshDraw>,
    /// Point/scatter marks, drawn after meshes so data is never veiled.
    pub points: Vec<PointDraw>,
    /// Line marks, drawn after meshes so data is never veiled.
    pub lines: Vec<LineDraw>,
    /// The resolved (auto-framed) camera for this frame.
    pub camera: ResolvedCamera,
    /// The key + hemispheric-ambient light rig shading lit materials.
    pub lights: LightRig,
    /// Scene-level styling: grid, background, MSAA, axis visibility.
    pub style: SceneStyle,
    /// Whether the backend should capture this scene's depth buffer for
    /// label occlusion. Set when the scene has scene-anchored labels (axis
    /// labels today); lets label-free scenes skip the resolve + read-back
    /// cost. See [`SceneDepthMap`](crate::scene::SceneDepthMap).
    pub capture_depth: bool,
}

impl Scene3DData {
    /// Combined world-space bounds of all marks — each handle's local
    /// bounds transformed by its mark transform, unioned. Empty when there
    /// is no geometry.
    ///
    /// Computed from the draw lists *before* assembling `Scene3DData`,
    /// because the camera is resolved (auto-framed) against these bounds
    /// and then stored in `camera`. Taking slices lets callers compute it
    /// at that point.
    pub fn content_bounds(meshes: &[MeshDraw], points: &[PointDraw], lines: &[LineDraw]) -> Aabb {
        let mut bb = Aabb::EMPTY;
        for m in meshes {
            bb = bb.union(m.geometry.bounds().transformed(m.transform));
        }
        for p in points {
            bb = bb.union(p.geometry.bounds().transformed(p.transform));
        }
        for l in lines {
            bb = bb.union(l.geometry.bounds().transformed(l.transform));
        }
        bb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::geometry::{PointData, PointsHandle, ScenePoint};
    use glam::Vec3;

    #[test]
    fn content_bounds_unions_and_applies_transforms() {
        let pts = PointsHandle::new(PointData {
            points: vec![
                ScenePoint {
                    position: Vec3::splat(-1.0),
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: Vec3::splat(1.0),
                    color: [1.0; 4],
                },
            ],
        });
        let draw = PointDraw {
            geometry: pts,
            transform: Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)),
            style: PointStyle::default(),
            labels: None,
            line_joins: false,
        };
        let bb = Scene3DData::content_bounds(&[], std::slice::from_ref(&draw), &[]);
        assert!(bb.is_valid());
        assert_eq!(bb.min, Vec3::new(4.0, -1.0, -1.0));
        assert_eq!(bb.max, Vec3::new(6.0, 1.0, 1.0));
    }

    #[test]
    fn content_bounds_empty_with_no_marks() {
        assert!(!Scene3DData::content_bounds(&[], &[], &[]).is_valid());
    }
}
