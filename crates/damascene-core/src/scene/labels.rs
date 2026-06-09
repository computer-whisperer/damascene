//! Per-point text labels for scatter marks: persistent annotations and
//! hover tooltips.
//!
//! Like [`axes`](crate::scene::axes), this is *presentation*: the text and
//! how to show it live on the mark ([`PointDraw`](crate::scene::PointDraw)),
//! not in the uploaded geometry ([`ScenePoint`](crate::scene::ScenePoint)
//! stays a pure `repr(C)` GPU vertex). The draw-op pass projects the labels
//! through the resolved camera and emits text — persistent labels reuse the
//! shared `scene_label` seam (depth-occluded like axis labels), hover
//! tooltips pick the point under the cursor and draw a styled chip.
//!
//! Modelled on the plotly scatter `text` / `hovertext` / `textposition`
//! vocabulary — the de-facto paradigm for labelled scatter data.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::sync::Arc;

use crate::color::Color;

/// When a mark's point labels are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LabelDisplay {
    /// Persistent: every point with non-empty text is labelled (the app is
    /// responsible for keeping that sparse enough to read).
    Always,
    /// On hover only: the point under the cursor shows its text as a chip.
    #[default]
    Hover,
}

/// The scatter point currently under the cursor, surfaced to the app via
/// [`BuildCx::hovered_scene_point`](crate::event::BuildCx::hovered_scene_point).
///
/// Scene marks have no keys (they're positional), so a point is identified by
/// the scene node, the mark's index in [`SceneSpec`](crate::scene::SceneSpec)'s
/// point list, and the point's index within that mark. The app indexes its own
/// data by `point` to drive a detail panel / highlight / linked view — the 3D
/// analogue of reading a `PointerEnter` key off a 2D hit-target. Picked from
/// the same [`LabelDisplay::Hover`] path that draws the built-in tooltip chip,
/// so it honours the same depth-occlusion and behind-camera culling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePointPick {
    /// `computed_id` of the `Scene3D` node the point belongs to.
    pub scene: String,
    /// Index of the point mark within the scene's point list.
    pub mark: usize,
    /// Index of the point within that mark's geometry.
    pub point: usize,
}

/// Where a label sits relative to its point's projected screen position.
/// The gap from the marker is derived from the point size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LabelPlacement {
    /// Centred on the point (axis labels use this).
    Center,
    /// Above the marker — the usual scatter-label spot.
    #[default]
    Above,
    /// Below the marker.
    Below,
    /// Left of the marker.
    Left,
    /// Right of the marker.
    Right,
}

/// Per-point labels for a scatter mark.
///
/// `text` is parallel to the mark's points; an empty string (or an index
/// past the end) means "no label". Build with [`PointLabels::new`] and the
/// `display` / `placement` / styling setters.
#[derive(Clone, Debug, PartialEq)]
pub struct PointLabels {
    /// Label text, indexed alongside the mark's points.
    pub text: Arc<[String]>,
    /// When labels show: persistently, or as a hover tooltip (the default).
    pub display: LabelDisplay,
    /// Where persistent labels sit relative to the marker. Defaults to
    /// [`LabelPlacement::Above`].
    pub placement: LabelPlacement,
    /// Text colour. `None` uses a theme token (foreground for persistent
    /// labels, popover-foreground for tooltip chips).
    pub color: Option<Color>,
    /// Label font size in logical pixels.
    pub size: f32,
}

impl PointLabels {
    /// Labels from any string iterator, defaulting to hover tooltips.
    pub fn new(text: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            text: text.into_iter().map(Into::into).collect(),
            display: LabelDisplay::default(),
            placement: LabelPlacement::default(),
            color: None,
            size: 11.0,
        }
    }

    /// Show every labelled point persistently rather than on hover.
    pub fn always(mut self) -> Self {
        self.display = LabelDisplay::Always;
        self
    }

    /// Show on hover (the default).
    pub fn on_hover(mut self) -> Self {
        self.display = LabelDisplay::Hover;
        self
    }

    /// Set where persistent labels sit relative to the marker.
    pub fn placement(mut self, placement: LabelPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Override the label text colour.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the label font size (logical px).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// The label for point `i`, or `None` when absent / empty.
    pub fn get(&self, i: usize) -> Option<&str> {
        self.text
            .get(i)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
}
