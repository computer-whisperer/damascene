//! The app-owned, versioned data handle a plot mark reads from, and the
//! sample type it holds.
//!
//! A [`SeriesHandle`] is the plot analogue of a
//! [`GeometryHandle`](crate::scene::GeometryHandle): the app creates it once,
//! stores it in its state, clones it into the [`PlotSpec`](crate::plot::PlotSpec)
//! every frame (a cheap `Arc` bump, never a data copy), and mutates it with
//! [`set`](SeriesHandle::set) / [`append`](SeriesHandle::append). It carries
//! the data in **data space** — raw `f64` `(x, y)` samples, so epoch
//! timestamps keep their precision (the plan's decision 7) — and the GPU-side
//! lowering to scale space happens downstream (see
//! [`lower`](crate::plot::lower)).
//!
//! The handle is the single interface for both data paradigms (the plan's
//! decision 5): a **virtual** app resamples its source to the visible window
//! and `set`s a window's worth of points; a **dump-everything** app `set`s the
//! whole series once and lets the plot decimate. Either way the mark just
//! reads whatever the handle holds.

#![warn(missing_docs)]

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::scene::geometry::{GeometryId, next_geometry_id};

/// One data point: an `(x, y)` pair in **data space** (`f64`). For a
/// [`Scale::Time`](crate::plot::Scale::Time) axis, `x` is epoch seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Horizontal-axis value, in data space.
    pub x: f64,
    /// Vertical-axis value, in data space.
    pub y: f64,
}

impl Sample {
    /// A sample at `(x, y)`.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

impl From<(f64, f64)> for Sample {
    fn from((x, y): (f64, f64)) -> Self {
        Self { x, y }
    }
}

/// The data-space extent of a series: the `(min, max)` of each axis, or
/// `None` per axis when there are no finite samples. Cached at mutation time
/// and used to auto-frame the initial [`PlotView`](crate::plot::PlotView) and
/// to size axis tick ranges.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SeriesBounds {
    /// `(min, max)` of `x` over all finite samples, or `None` if none.
    pub x: Option<(f64, f64)>,
    /// `(min, max)` of `y` over all finite samples, or `None` if none.
    pub y: Option<(f64, f64)>,
}

impl SeriesBounds {
    /// The bounds of `samples`, ignoring non-finite values.
    pub fn of(samples: &[Sample]) -> Self {
        let mut x: Option<(f64, f64)> = None;
        let mut y: Option<(f64, f64)> = None;
        for s in samples {
            if s.x.is_finite() {
                x = Some(match x {
                    Some((lo, hi)) => (lo.min(s.x), hi.max(s.x)),
                    None => (s.x, s.x),
                });
            }
            if s.y.is_finite() {
                y = Some(match y {
                    Some((lo, hi)) => (lo.min(s.y), hi.max(s.y)),
                    None => (s.y, s.y),
                });
            }
        }
        Self { x, y }
    }

    /// Union with `other` (per axis).
    pub fn union(self, other: SeriesBounds) -> SeriesBounds {
        SeriesBounds {
            x: union_axis(self.x, other.x),
            y: union_axis(self.y, other.y),
        }
    }
}

fn union_axis(a: Option<(f64, f64)>, b: Option<(f64, f64)>) -> Option<(f64, f64)> {
    match (a, b) {
        (Some((alo, ahi)), Some((blo, bhi))) => Some((alo.min(blo), ahi.max(bhi))),
        (some, None) | (None, some) => some,
    }
}

/// Interior, shared by every clone of one [`SeriesHandle`].
#[derive(Debug)]
struct SeriesStore {
    id: GeometryId,
    samples: Mutex<Arc<Vec<Sample>>>,
    bounds: Mutex<SeriesBounds>,
    rev: AtomicU64,
}

/// App-owned, versioned series data. Create once, clone into the spec each
/// frame (a cheap `Arc` bump), mutate with [`set`](Self::set) /
/// [`append`](Self::append). The stable [`id`](Self::id) keys the GPU buffer
/// cache for the geometry this series lowers to; [`revision`](Self::revision)
/// advances on every mutation so the backend re-uploads only when the data
/// actually changed.
#[derive(Clone, Debug)]
pub struct SeriesHandle {
    inner: Arc<SeriesStore>,
}

impl SeriesHandle {
    /// A handle owning `samples`, with a fresh stable id.
    pub fn new(samples: impl Into<Vec<Sample>>) -> Self {
        let samples = samples.into();
        let bounds = SeriesBounds::of(&samples);
        Self {
            inner: Arc::new(SeriesStore {
                id: next_geometry_id(),
                samples: Mutex::new(Arc::new(samples)),
                bounds: Mutex::new(bounds),
                rev: AtomicU64::new(0),
            }),
        }
    }

    /// An empty handle.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Replace the samples and advance the revision. The hot path for a
    /// **virtual** app: as the user pans/zooms, resample the source to the
    /// visible window and `set` the result.
    pub fn set(&self, samples: impl Into<Vec<Sample>>) {
        let samples = samples.into();
        *self.inner.bounds.lock().unwrap() = SeriesBounds::of(&samples);
        *self.inner.samples.lock().unwrap() = Arc::new(samples);
        self.inner.rev.fetch_add(1, Ordering::Relaxed);
    }

    /// Append samples to the tail and advance the revision — for a live
    /// series that grows over time. Recomputes bounds over the new tail only.
    pub fn append(&self, more: &[Sample]) {
        if more.is_empty() {
            return;
        }
        {
            let mut guard = self.inner.samples.lock().unwrap();
            let mut v = guard.as_ref().clone();
            v.extend_from_slice(more);
            *guard = Arc::new(v);
        }
        {
            let mut b = self.inner.bounds.lock().unwrap();
            *b = b.union(SeriesBounds::of(more));
        }
        self.inner.rev.fetch_add(1, Ordering::Relaxed);
    }

    /// The stable id, constant for the handle's life. Backends key the GPU
    /// buffers for this series' lowered geometry on it.
    pub fn id(&self) -> GeometryId {
        self.inner.id
    }

    /// The current revision; advances on every mutation.
    pub fn revision(&self) -> u64 {
        self.inner.rev.load(Ordering::Relaxed)
    }

    /// The cached data-space [`SeriesBounds`].
    pub fn bounds(&self) -> SeriesBounds {
        *self.inner.bounds.lock().unwrap()
    }

    /// The number of samples currently held.
    pub fn len(&self) -> usize {
        self.inner.samples.lock().unwrap().len()
    }

    /// Whether the series currently holds no samples.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A snapshot of the samples (a cheap `Arc` clone) plus the revision it
    /// was taken at — what the lowering and backend read.
    pub fn snapshot(&self) -> (Arc<Vec<Sample>>, u64) {
        let rev = self.inner.rev.load(Ordering::Relaxed);
        (Arc::clone(&self.inner.samples.lock().unwrap()), rev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_ignore_non_finite() {
        let b = SeriesBounds::of(&[
            Sample::new(1.0, 10.0),
            Sample::new(f64::NAN, 5.0),
            Sample::new(3.0, f64::INFINITY),
        ]);
        assert_eq!(b.x, Some((1.0, 3.0)));
        assert_eq!(b.y, Some((5.0, 10.0)));
    }

    #[test]
    fn empty_bounds_are_none() {
        let b = SeriesBounds::of(&[]);
        assert_eq!(b.x, None);
        assert_eq!(b.y, None);
    }

    #[test]
    fn set_bumps_revision_and_recomputes_bounds() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0)]);
        assert_eq!(h.revision(), 0);
        assert_eq!(h.bounds().x, Some((0.0, 0.0)));
        h.set(vec![Sample::new(-1.0, 2.0), Sample::new(4.0, 9.0)]);
        assert_eq!(h.revision(), 1);
        assert_eq!(h.bounds().x, Some((-1.0, 4.0)));
        assert_eq!(h.bounds().y, Some((2.0, 9.0)));
    }

    #[test]
    fn append_grows_and_unions_bounds() {
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0)]);
        h.append(&[Sample::new(5.0, -3.0)]);
        assert_eq!(h.len(), 2);
        assert_eq!(h.revision(), 1);
        assert_eq!(h.bounds().x, Some((0.0, 5.0)));
        assert_eq!(h.bounds().y, Some((-3.0, 0.0)));
        // appending nothing is a no-op (no revision bump)
        h.append(&[]);
        assert_eq!(h.revision(), 1);
    }

    #[test]
    fn clones_share_storage_and_id() {
        let a = SeriesHandle::new(vec![Sample::new(0.0, 0.0)]);
        let b = a.clone();
        assert_eq!(a.id(), b.id());
        b.set(vec![Sample::new(1.0, 1.0), Sample::new(2.0, 2.0)]);
        // the mutation is visible through the other clone
        assert_eq!(a.len(), 2);
        assert_eq!(a.revision(), 1);
    }

    #[test]
    fn distinct_handles_have_distinct_ids() {
        let a = SeriesHandle::new(Vec::new());
        let b = SeriesHandle::new(Vec::new());
        assert_ne!(a.id(), b.id());
    }
}
