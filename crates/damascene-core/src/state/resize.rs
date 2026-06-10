//! Edge-resize state for [`El::user_resizable`](crate::tree::El::user_resizable)
//! panes (issue #106).
//!
//! Mirrors the scroll subsystem's division of labor: the layout pass
//! publishes per-frame grab-band geometry ([`ResizeBand`]), the
//! runtime captures and drives drags ([`EdgeResizeDrag`]) the same way
//! it drives scrollbar-thumb drags, and the user-dragged sizes persist
//! across frames in [`ResizeState::overrides`] the way scroll offsets
//! do. Apps read the result back through [`UiState::user_size`].

use rustc_hash::FxHashMap;

use crate::tree::{Axis, Rect};

use super::UiState;

/// Thickness of the invisible grab band straddling a resizable pane's
/// seam edge, in logical pixels. Centered on the edge — half overlaps
/// the pane, half its neighbor — and consumes no layout space. Matches
/// [`crate::widgets::resize_handle::HANDLE_THICKNESS`] so the two
/// affordances feel the same under the pointer.
pub const RESIZE_BAND_THICKNESS: f32 = 8.0;

/// Runtime state for `.user_resizable()` panes. One subsystem inside
/// [`UiState`], shaped like `ScrollState`: persistent overrides plus
/// per-frame band scratch and an in-flight drag.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResizeState {
    /// User-dragged main-axis size (logical pixels) per resizable
    /// pane, keyed by the pane's `key` when it has one, else by
    /// `computed_id`. Persists across frames; the layout pre-pass
    /// rewrites the pane's declared size from this map.
    pub(crate) overrides: FxHashMap<String, f32>,
    /// Grab bands published by the last layout — per-frame scratch,
    /// rebuilt every layout like the scrollbar thumb maps.
    pub(crate) bands: Vec<ResizeBand>,
    /// Active edge drag, set by `pointer_down` when a primary press
    /// lands in a band, driven by `pointer_moved`, cleared by
    /// `pointer_up`. Pre-empts normal hit-test like the scrollbar
    /// thumb drag.
    pub(crate) drag: Option<EdgeResizeDrag>,
    /// Storage id of the band currently under the pointer, tracked by
    /// `pointer_moved` so crossing a band edge triggers a redraw (the
    /// host re-resolves the cursor per frame, so without the redraw
    /// the resize arrows would never appear).
    pub(crate) hovered_band: Option<String>,
}

/// One pane's grab band for the current frame, written by layout.
#[derive(Clone, Debug)]
pub(crate) struct ResizeBand {
    /// Override-storage id: the pane's `key` when present, else its
    /// `computed_id`.
    pub(crate) id: String,
    /// The pane's `key`, for routing the release event. Unkeyed panes
    /// resize fine but emit nothing.
    pub(crate) key: Option<String>,
    /// `computed_id` of the parent container — used to refuse capture
    /// when an overlay (dialog, popover) covers the seam, mirroring
    /// the covered-scrollbar guard.
    pub(crate) container_id: String,
    /// The band's hit rect in logical pixels, straddling the seam.
    pub(crate) band: Rect,
    /// The parent's layout axis: `Row` resizes width with a horizontal
    /// drag, `Column` resizes height with a vertical one.
    pub(crate) axis: Axis,
    /// `+1.0` when dragging in the +axis direction grows the pane
    /// (trailing-edge band), `-1.0` for a leading-edge band on a
    /// last-child pane.
    pub(crate) sign: f32,
    /// The pane's laid-out main-axis extent this frame — the drag's
    /// starting value.
    pub(crate) current: f32,
    /// Lower drag clamp, from `min_width`/`min_height`.
    pub(crate) min: f32,
    /// Upper drag clamp, from `max_width`/`max_height`, additionally
    /// capped at the parent's inner extent so the seam can't be
    /// dragged out of reach.
    pub(crate) max: f32,
}

/// Active edge-resize drag. All geometry is captured at `pointer_down`
/// so a mid-drag relayout can't retroactively shift the cursor-to-size
/// correspondence — same reasoning as [`super::ThumbDrag`].
#[derive(Clone, Debug)]
pub(crate) struct EdgeResizeDrag {
    /// Override-storage id (see [`ResizeBand::id`]).
    pub(crate) id: String,
    /// The pane's `key`, for the release event.
    pub(crate) key: Option<String>,
    /// Resize axis captured from the band.
    pub(crate) axis: Axis,
    /// Drag-direction sign captured from the band.
    pub(crate) sign: f32,
    /// Pointer position along the resize axis at `pointer_down`.
    pub(crate) anchor: f32,
    /// The pane's main-axis extent at `pointer_down`.
    pub(crate) initial: f32,
    /// Clamp range captured from the band.
    pub(crate) min: f32,
    /// Clamp range captured from the band.
    pub(crate) max: f32,
}

impl UiState {
    /// The grab band under `(x, y)`, if any. Bands are narrow and
    /// rarely overlap; first match wins.
    pub(crate) fn resize_band_at(&self, x: f32, y: f32) -> Option<&ResizeBand> {
        self.resize.bands.iter().find(|b| b.band.contains(x, y))
    }

    /// The user-dragged size (logical pixels) of a `.user_resizable()`
    /// pane, by key. `None` until the user's first drag (the pane is
    /// still at its declared size) or after [`Self::clear_user_size`].
    /// Also exposed on `BuildCx`/`EventCx`; the
    /// [`crate::UiEventKind::Resized`] event routed to the pane's key
    /// on drag release is the natural moment to read and persist it.
    pub fn user_size(&self, key: &str) -> Option<f32> {
        self.resize.overrides.get(key).copied()
    }

    /// Pre-seed or programmatically set a `.user_resizable()` pane's
    /// size, by key — e.g. restoring a persisted sidebar width at
    /// startup. Takes effect on the next layout, clamped to the pane's
    /// min/max exactly as a drag would be.
    pub fn set_user_size(&mut self, key: impl Into<String>, px: f32) {
        self.resize.overrides.insert(key.into(), px);
    }

    /// Forget the user-dragged size for a pane, returning it to its
    /// declared size on the next layout.
    pub fn clear_user_size(&mut self, key: &str) {
        self.resize.overrides.remove(key);
    }
}
