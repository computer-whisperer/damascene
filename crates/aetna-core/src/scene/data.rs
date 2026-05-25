//! `Scene3DData`: the backend-neutral payload of a `DrawOp::Scene3D`.
//!
//! Assembled fresh each frame from the El tree's marks (cheap — it holds
//! `Arc`-cloned [geometry handles](crate::scene::GeometryHandle), not
//! geometry copies) plus the resolved camera, light rig, and style. The
//! backend walks these draw lists, uploads any geometry whose revision
//! advanced, and renders. See `docs/SCENE3D_PLAN.md`.

use glam::Mat4;

use crate::scene::bounds::Aabb;
use crate::scene::camera::ResolvedCamera;
use crate::scene::geometry::{LinesHandle, MeshHandle, PointsHandle};
use crate::scene::style::{LightRig, LineStyle, Material, PointStyle, SceneStyle};

/// A mesh mark: geometry handle + object→world transform + material.
#[derive(Clone, Debug)]
pub struct MeshDraw {
    pub geometry: MeshHandle,
    pub transform: Mat4,
    pub material: Material,
}

/// A point/scatter mark: geometry handle + transform + style (per-point
/// colour is in the geometry).
#[derive(Clone, Debug)]
pub struct PointDraw {
    pub geometry: PointsHandle,
    pub transform: Mat4,
    pub style: PointStyle,
}

/// A line mark: geometry handle + transform + style (per-segment colour is
/// in the geometry).
#[derive(Clone, Debug)]
pub struct LineDraw {
    pub geometry: LinesHandle,
    pub transform: Mat4,
    pub style: LineStyle,
}

/// Everything a backend needs to render one scene, all backend-neutral.
#[derive(Clone, Debug)]
pub struct Scene3DData {
    pub meshes: Vec<MeshDraw>,
    pub points: Vec<PointDraw>,
    pub lines: Vec<LineDraw>,
    pub camera: ResolvedCamera,
    pub lights: LightRig,
    pub style: SceneStyle,
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
