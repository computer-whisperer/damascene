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
//! `aetna-core` stays backend-neutral and is "not a game engine".
//!
//! ## Math vocabulary
//!
//! The scene API speaks [`glam`] — the de-facto standard apps already
//! reach for — so callers pass their own `Vec3`/`Mat4` directly. glam is
//! **re-exported here** ([`scene::glam`](glam)); reference it through this
//! path so downstream code pins the same (pre-1.0) version and avoids the
//! two-incompatible-`Vec3` footgun. The one type glam lacks, an
//! axis-aligned [`Aabb`], lives in [`bounds`].
//!
//! Built so far (the geometry foundation):
//!
//! - [`bounds`]: [`Aabb`] over `glam::Vec3`.
//! - [`geometry`]: logical vertex types and the app-owned, versioned
//!   [`GeometryHandle`] that carries geometry into a scene without the app
//!   ever touching a device.

pub mod bounds;
pub mod geometry;

/// Re-exported so downstream code can pin aetna's exact glam version.
pub use glam;

pub use bounds::Aabb;
pub use geometry::{
    GeometryData, GeometryHandle, GeometryId, LineData, LineSegment, LinesHandle, MeshData,
    MeshHandle, MeshVertex, PointData, PointsHandle, ScenePoint, next_geometry_id,
};
