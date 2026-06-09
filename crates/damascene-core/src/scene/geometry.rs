//! Backend-neutral 3D geometry and the app-owned, versioned handles that
//! carry it into a scene draw-op.
//!
//! # The handle pattern
//!
//! Geometry follows the same identity/caching shape as
//! [`crate::surface::AppTexture`], but inverted across the device
//! boundary. `AppTexture` wraps a *GPU* texture the app allocates; a
//! [`GeometryHandle`] wraps *CPU* data the app owns and the backend
//! uploads. This keeps the scene fully backend-neutral — the app never
//! touches a device — while still giving the backend a stable
//! [`GeometryId`] to cache GPU buffers against and a monotonic revision
//! to decide when a re-upload is needed.
//!
//! Create a handle once, store it in app state, and reference it from the
//! El tree every frame (cloning a handle is a cheap `Arc` bump, never a
//! geometry copy). Mutate with [`GeometryHandle::set`]; the backend
//! re-uploads only when the revision advances. Geometry that merely moves
//! is a per-frame transform (a uniform), not a `set`, so it never
//! re-uploads.
//!
//! Vertex types here speak glam ([`Vec3`] positions/normals) for the
//! attributes apps build with, and authoring-space sRGBA `[f32; 4]` for
//! colour. They are `#[repr(C)]` `Copy` logical types; each backend maps
//! them to its own GPU vertex layout at upload (e.g. padding to `vec4`
//! where a uniform/storage layout needs it, as the volumetric renderer
//! does). `bytemuck` is available in core, so a backend may also cast
//! directly once a `Pod` layout is settled.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;

use crate::scene::bounds::Aabb;

/// Stable identity for one [`GeometryHandle`]'s GPU buffer cache entry.
/// Allocated once when the handle is created and constant for its life;
/// backends key their vertex/index buffers on it. Re-creating a handle
/// (`GeometryHandle::new`) yields a fresh id, so the old buffer falls off
/// the cache like any unused entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeometryId(pub u64);

/// Allocate a fresh [`GeometryId`]. Called by [`GeometryHandle::new`];
/// app code goes through the handle constructor, not this directly.
pub fn next_geometry_id() -> GeometryId {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    GeometryId(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// One mesh vertex: object-space position and normal. Colour/material is
/// per-mark, not per-vertex (see `Material` in the scene style).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    /// Object-space position (the mark's transform maps it to world).
    pub position: Vec3,
    /// Object-space normal, used for lighting. Expected unit length.
    pub normal: Vec3,
}

/// A point/marker for scatter data. `color` is **authoring-space** sRGBA;
/// the backend converts to the runner's working linear space at upload.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScenePoint {
    /// Object-space position (the mark's transform maps it to world).
    pub position: Vec3,
    /// Authoring-space sRGBA, converted to the working space at upload.
    pub color: [f32; 4],
}

/// A line segment. `color` is **authoring-space** sRGBA (see [`ScenePoint`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSegment {
    /// Object-space start point.
    pub start: Vec3,
    /// Object-space end point.
    pub end: Vec3,
    /// Authoring-space sRGBA, converted to the working space at upload.
    pub color: [f32; 4],
}

/// Triangle geometry. Indexed when `indices` is `Some`, otherwise a flat
/// triangle list over `vertices`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// The vertex set.
    pub vertices: Vec<MeshVertex>,
    /// Triangle indices into `vertices`; `None` draws `vertices` as a
    /// flat triangle list.
    pub indices: Option<Vec<u32>>,
}

/// A batch of points/markers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointData {
    /// The points, in draw order.
    pub points: Vec<ScenePoint>,
}

/// A batch of line segments.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineData {
    /// The segments, in draw order.
    pub segments: Vec<LineSegment>,
}

/// Geometry that can report its own bounds, so handles can cache an
/// [`Aabb`] for camera auto-framing and axis tick ranges.
pub trait GeometryData: Send + Sync + 'static {
    /// The [`Aabb`] enclosing every position in the batch —
    /// [`Aabb::EMPTY`] when there are none.
    fn compute_bounds(&self) -> Aabb;
}

impl GeometryData for MeshData {
    fn compute_bounds(&self) -> Aabb {
        Aabb::from_points(self.vertices.iter().map(|v| v.position))
    }
}

impl GeometryData for PointData {
    fn compute_bounds(&self) -> Aabb {
        Aabb::from_points(self.points.iter().map(|p| p.position))
    }
}

impl GeometryData for LineData {
    fn compute_bounds(&self) -> Aabb {
        let mut bb = Aabb::EMPTY;
        for seg in &self.segments {
            bb.expand(seg.start);
            bb.expand(seg.end);
        }
        bb
    }
}

/// Shared inner store: the current data (behind an `Arc` so a backend can
/// snapshot it cheaply under a short lock) plus its cached bounds.
struct Inner<T> {
    data: Arc<T>,
    bounds: Aabb,
}

struct Store<T> {
    id: GeometryId,
    rev: AtomicU64,
    inner: Mutex<Inner<T>>,
}

/// An app-owned, versioned handle to one batch of geometry.
///
/// Cheap to clone (`Arc` bump). See the [module docs](self) for the
/// upload/caching contract. Type aliases [`MeshHandle`], [`PointsHandle`],
/// and [`LinesHandle`] name the concrete instantiations used by the marks.
#[derive(Clone)]
pub struct GeometryHandle<T> {
    store: Arc<Store<T>>,
}

impl<T: GeometryData> GeometryHandle<T> {
    /// Create a handle, allocating a fresh [`GeometryId`] and computing
    /// bounds. Revision starts at 1.
    pub fn new(data: T) -> Self {
        let bounds = data.compute_bounds();
        Self {
            store: Arc::new(Store {
                id: next_geometry_id(),
                rev: AtomicU64::new(1),
                inner: Mutex::new(Inner {
                    data: Arc::new(data),
                    bounds,
                }),
            }),
        }
    }

    /// Replace the geometry and advance the revision, recomputing bounds.
    /// The backend re-uploads on its next draw because the revision moved.
    ///
    /// This is the baseline update path: it re-uploads the whole buffer.
    /// Finer-grained `update_range` / `append` paths are designed to slot
    /// in later (the handle is the stable surface) without breaking
    /// callers of `set`.
    pub fn set(&self, data: T) {
        let bounds = data.compute_bounds();
        {
            let mut inner = self.store.inner.lock().unwrap();
            inner.data = Arc::new(data);
            inner.bounds = bounds;
        }
        self.store.rev.fetch_add(1, Ordering::Release);
    }

    /// Stable identity for backend buffer caches.
    pub fn id(&self) -> GeometryId {
        self.store.id
    }

    /// Monotonic revision; advances on every [`GeometryHandle::set`].
    pub fn revision(&self) -> u64 {
        self.store.rev.load(Ordering::Acquire)
    }

    /// Cached bounds of the current geometry.
    pub fn bounds(&self) -> Aabb {
        self.store.inner.lock().unwrap().bounds
    }

    /// Snapshot the current data and revision for upload. Returns an
    /// `Arc` clone (no geometry copy) plus the revision it corresponds to,
    /// so the backend can cache by `(id, revision)`.
    pub fn snapshot(&self) -> (Arc<T>, u64) {
        let inner = self.store.inner.lock().unwrap();
        let rev = self.store.rev.load(Ordering::Acquire);
        (Arc::clone(&inner.data), rev)
    }
}

impl<T: GeometryData> std::fmt::Debug for GeometryHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeometryHandle")
            .field("id", &self.id().0)
            .field("revision", &self.revision())
            .field("bounds", &self.bounds())
            .finish()
    }
}

/// Handle to triangle geometry (small mesh models, surfaces).
pub type MeshHandle = GeometryHandle<MeshData>;
/// Handle to point/marker geometry (scatter data).
pub type PointsHandle = GeometryHandle<PointData>;
/// Handle to line geometry (axes, wireframe, series, error bars).
pub type LinesHandle = GeometryHandle<LineData>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = PointsHandle::new(PointData::default());
        let b = PointsHandle::new(PointData::default());
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn set_advances_revision_and_recomputes_bounds() {
        let h = PointsHandle::new(PointData::default());
        assert_eq!(h.revision(), 1);
        assert!(!h.bounds().is_valid()); // empty

        h.set(PointData {
            points: vec![
                ScenePoint {
                    position: Vec3::ZERO,
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: Vec3::new(2.0, 4.0, 6.0),
                    color: [1.0; 4],
                },
            ],
        });
        assert_eq!(h.revision(), 2);
        let bb = h.bounds();
        assert_eq!(bb.min, Vec3::ZERO);
        assert_eq!(bb.max, Vec3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn snapshot_tracks_current_revision() {
        let h = MeshHandle::new(MeshData::default());
        let (_d0, r0) = h.snapshot();
        assert_eq!(r0, 1);
        h.set(MeshData {
            vertices: vec![MeshVertex {
                position: Vec3::ZERO,
                normal: Vec3::Y,
            }],
            indices: None,
        });
        let (d1, r1) = h.snapshot();
        assert_eq!(r1, 2);
        assert_eq!(d1.vertices.len(), 1);
    }

    #[test]
    fn clone_shares_store() {
        let h = LinesHandle::new(LineData::default());
        let c = h.clone();
        assert_eq!(h.id(), c.id());
        c.set(LineData {
            segments: vec![LineSegment {
                start: Vec3::ZERO,
                end: Vec3::ONE,
                color: [1.0; 4],
            }],
        });
        // Mutation through the clone is visible through the original.
        assert_eq!(h.revision(), 2);
        assert!(h.bounds().is_valid());
    }
}
