//! Backend-neutral data for the `Scene3D` draw-op: small, polished,
//! hardware-accelerated 3D graphs and models.
//!
//! Scope and rationale live in `docs/SCENE3D_PLAN.md`. The short version:
//! a closed-scope 3D scene (point scatter, small lit meshes, lines) is
//! threaded through the backend-neutral draw-op stream rather than bolted
//! on as a host-composed `surface()`, so it renders zero-glue on every
//! backend. This module owns the *data* the op carries — geometry,
//! handles, and (as the feature lands) camera, lighting, and style. The
//! pipelines that render it live in the backend crates, never here:
//! `aetna-core` stays backend-neutral and is "not a game engine."
//!
//! Built so far (the geometry foundation):
//!
//! - [`linalg`]: minimal glam-free `Vec3` / `Mat4` / `Aabb`.
//! - [`geometry`]: logical vertex types and the app-owned, versioned
//!   [`GeometryHandle`] that carries geometry into a scene without the app
//!   ever touching a device.

pub mod geometry;
pub mod linalg;

pub use geometry::{
    GeometryData, GeometryHandle, GeometryId, LineData, LineSegment, LinesHandle, MeshData,
    MeshHandle, MeshVertex, PointData, PointsHandle, ScenePoint, next_geometry_id,
};
pub use linalg::{Aabb, Mat4, Vec3};
