//! Pan/zoom [`viewport`](crate::tree::viewport) configuration, the
//! content↔screen transform, programmatic requests, and read-back types.
//!
//! A `viewport()` is a clipped window onto a content layer the user can
//! pan and zoom — the CSS `overflow: hidden` wrapper around a
//! `transform: translate(pan) scale(zoom)` content box, with the wheel
//! and drag gestures handled natively.
//!
//! The transform is **origin-anchored**: pan/zoom are expressed relative
//! to the viewport's own inner top-left, so the reset state is always
//! `pan = (0, 0)`, `zoom = 1.0` regardless of where the viewport sits on
//! screen — it survives window resizes without recomputation.
//!
//! Apps push [`ViewportRequest`]s the same way they push
//! [`crate::scroll::ScrollRequest`]s: fire-and-forget descriptors the
//! layout pass resolves against the live viewport rect and content
//! extents (only known mid-frame), writing the resulting pan/zoom into
//! viewport state so the same frame renders the new framing.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::event::{KeyModifiers, PointerButton};

/// The pointer gesture that begins a pan drag inside a
/// [`viewport`](crate::tree::viewport). Defaults to a plain primary-button
/// drag on empty content; set [`KeyModifiers`] and/or a different
/// [`PointerButton`] (e.g. middle-button or space-drag, Figma-style) via
/// [`El::pan_button`](crate::tree::El::pan_button) /
/// [`El::pan_modifier`](crate::tree::El::pan_modifier).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanTrigger {
    /// Button that must be pressed to start a pan.
    pub button: PointerButton,
    /// Exact modifier mask that must be held. Extra modifiers held
    /// beyond this mask do **not** match — mirrors
    /// [`KeyChord`](crate::event::KeyChord).
    pub modifiers: KeyModifiers,
}

impl Default for PanTrigger {
    fn default() -> Self {
        Self {
            button: PointerButton::Primary,
            modifiers: KeyModifiers::default(),
        }
    }
}

impl PanTrigger {
    /// True when a press of `button` with the current `modifiers` mask
    /// should begin a pan. The modifier match is exact.
    pub fn matches(self, button: PointerButton, modifiers: KeyModifiers) -> bool {
        self.button == button && self.modifiers == modifiers
    }
}

/// How far a [`viewport`](crate::tree::viewport) may be panned relative
/// to its content — the clamp policy applied to pan after every drag,
/// wheel-zoom, and programmatic request. Set via
/// [`El::pan_bounds`](crate::tree::El::pan_bounds).
///
/// The keyword names mirror CSS sizing keywords: pick the policy by what
/// the content *is*, not by tuning a px margin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PanBounds {
    /// Content can't be dragged off-screen: content larger than the
    /// viewport shows no empty gutter past its edges, and content smaller
    /// than the viewport stays fully inside. Good for documents, images,
    /// and fixed-extent maps. This is the default.
    #[default]
    Contain,
    /// Any content point can be parked at the viewport center: the
    /// content bounding box is kept overlapping the viewport's center, so
    /// the left-most node of a graph (or any node) can sit mid-frame.
    /// Good for node graphs, DAGs, and freeform canvases.
    Center,
    /// No clamping at all — content can be panned anywhere, including
    /// entirely out of view. The app owns any bounding it wants.
    Free,
}

/// Declarative framing policy for a [`viewport`](crate::tree::viewport)
/// — whether the widget itself keeps the content framed, and when it
/// hands control to the user. Set via
/// [`El::fit_policy`](crate::tree::El::fit_policy).
///
/// This is the *sustained* counterpart of the one-shot
/// [`ViewportRequest::FitContent`]: the policy lives on the config and
/// is maintained by the layout pass, so the framing survives container
/// resizes and content changes without the app diffing rects across
/// frames.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum FitPolicy {
    /// The app drives framing via [`ViewportRequest`]s; the widget only
    /// clamps. This is the default and the pre-policy behaviour.
    #[default]
    Manual,
    /// Keep the content fit to the frame — on mount, on every container
    /// resize, and on content-extent changes — until the user pans or
    /// zooms, which releases the policy to free pan/zoom. A
    /// [`ViewportRequest::ResetView`] (or `FitContent`) re-arms it.
    /// The photo / PDF / diagram-viewer default: opens fit, tracks the
    /// window, hands over on first touch.
    Contain {
        /// Margin in logical px between the content bbox and the
        /// viewport edge, as in [`ViewportRequest::FitContent`].
        padding: f32,
    },
    /// Always keep the content fit; pan / zoom gestures are disabled
    /// (the wheel falls through to any enclosing scroll). For
    /// thumbnails and fixed previews that must always show everything.
    Lock {
        /// Margin in logical px between the content bbox and the
        /// viewport edge.
        padding: f32,
    },
}

/// Configuration for a [`viewport`](crate::tree::viewport) container,
/// carried on [`El`](crate::tree::El) and read by the layout / input
/// passes. Present (`Some`) exactly on viewport nodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportConfig {
    /// Smallest zoom factor the wheel / pinch may reach. `0.2` shows
    /// content at one-fifth size.
    pub min_zoom: f32,
    /// Largest zoom factor the wheel / pinch may reach.
    pub max_zoom: f32,
    /// What pointer gesture pans the content.
    pub pan_trigger: PanTrigger,
    /// How far the content may be panned relative to the viewport.
    pub pan_bounds: PanBounds,
    /// Whether the widget maintains a content-fit framing itself. See
    /// [`FitPolicy`]; defaults to [`FitPolicy::Manual`].
    pub fit: FitPolicy,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            min_zoom: 0.2,
            max_zoom: 5.0,
            pan_trigger: PanTrigger::default(),
            pan_bounds: PanBounds::default(),
            fit: FitPolicy::default(),
        }
    }
}

/// The current pan/zoom of a [`viewport`](crate::tree::viewport),
/// persisted per node across rebuilds and readable with
/// [`UiState::viewport_view`](crate::state::UiState::viewport_view) (e.g.
/// to display a zoom percentage).
///
/// The transform maps a **content-space** point `c` (the coordinate a
/// child was laid out at, with no pan/zoom) to a **screen** point, given
/// the viewport's inner top-left `origin`:
///
/// ```text
/// screen = origin + pan + zoom * (c - origin)
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportView {
    /// Screen-space translation of the content layer, in logical px.
    pub pan: (f32, f32),
    /// Uniform zoom factor. `1.0` means content space equals screen
    /// space.
    pub zoom: f32,
}

impl Default for ViewportView {
    /// The reset framing: no pan, unit zoom.
    fn default() -> Self {
        Self {
            pan: (0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl ViewportView {
    /// Map a content-space point to screen space about `origin` (the
    /// viewport's inner top-left).
    pub fn project(self, p: (f32, f32), origin: (f32, f32)) -> (f32, f32) {
        (
            origin.0 + self.pan.0 + self.zoom * (p.0 - origin.0),
            origin.1 + self.pan.1 + self.zoom * (p.1 - origin.1),
        )
    }

    /// Inverse of [`Self::project`]: map a screen-space point back into
    /// content space.
    pub fn unproject(self, s: (f32, f32), origin: (f32, f32)) -> (f32, f32) {
        (
            origin.0 + (s.0 - origin.0 - self.pan.0) / self.zoom,
            origin.1 + (s.1 - origin.1 - self.pan.1) / self.zoom,
        )
    }

    /// Recompute `pan` for a zoom change to `new_zoom` that keeps the
    /// content point currently under the screen point `anchor` fixed —
    /// the cursor-anchored ("zoom toward the mouse") behaviour. `origin`
    /// is the viewport's inner top-left.
    pub fn zoom_about(self, new_zoom: f32, anchor: (f32, f32), origin: (f32, f32)) -> Self {
        // Solve project(unproject(anchor)) == anchor for the new pan:
        //   pan' = (anchor - origin) * (1 - r) + pan * r,  r = new/old.
        let r = new_zoom / self.zoom;
        let dx = anchor.0 - origin.0;
        let dy = anchor.1 - origin.1;
        Self {
            pan: (
                dx * (1.0 - r) + self.pan.0 * r,
                dy * (1.0 - r) + self.pan.1 * r,
            ),
            zoom: new_zoom,
        }
    }
}

/// What an app produces to drive a [`viewport`](crate::tree::viewport)
/// programmatically. Each request targets a viewport by its `.key(...)`
/// and is consumed during that viewport's layout, where the live inner
/// rect and content extents are known. Push them once per build via
/// [`UiState::push_viewport_requests`](crate::state::UiState::push_viewport_requests).
#[derive(Clone, Debug, PartialEq)]
pub enum ViewportRequest {
    /// Frame all content: choose the largest zoom (within the configured
    /// `min_zoom..=max_zoom`) that fits the content bounding box inside
    /// the viewport with `padding` logical px of margin on every side,
    /// then center it. The "fit to view" / "zoom to fit" command.
    ///
    /// On a viewport with an armed [`FitPolicy::Contain`] /
    /// [`FitPolicy::Lock`], this re-arms the policy and the policy's own
    /// `padding` wins (the request's is ignored) — the sustained fit is
    /// the single source of the framing there. Same for [`Self::ResetView`],
    /// whose home framing under those policies is the fit, not 1:1.
    FitContent {
        /// `.key(...)` of the target viewport.
        key: String,
        /// Margin in logical px between the content bbox and the
        /// viewport edge.
        padding: f32,
    },
    /// Snap back to the reset framing: `pan = (0, 0)`, `zoom = 1.0`.
    ResetView {
        /// `.key(...)` of the target viewport.
        key: String,
    },
    /// Pan (keeping the current zoom) so the given content-space point
    /// lands at the center of the viewport.
    CenterOn {
        /// `.key(...)` of the target viewport.
        key: String,
        /// Point in content coordinates (the same space children are
        /// laid out in: logical px, pre-transform).
        point: (f32, f32),
    },
}

impl ViewportRequest {
    /// The `.key(...)` of the viewport this request targets.
    pub fn key(&self) -> &str {
        match self {
            ViewportRequest::FitContent { key, .. }
            | ViewportRequest::ResetView { key }
            | ViewportRequest::CenterOn { key, .. } => key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: (f32, f32), b: (f32, f32)) {
        assert!(
            (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn project_unproject_roundtrip() {
        let origin = (100.0, 50.0);
        let v = ViewportView {
            pan: (30.0, -20.0),
            zoom: 2.5,
        };
        let p = (140.0, 75.0);
        approx(v.unproject(v.project(p, origin), origin), p);
    }

    #[test]
    fn identity_is_a_noop() {
        let origin = (10.0, 10.0);
        let v = ViewportView::default();
        approx(v.project((200.0, 300.0), origin), (200.0, 300.0));
    }

    #[test]
    fn zoom_about_keeps_cursor_point_fixed() {
        // The content point under the cursor before the zoom must remain
        // under the cursor after the zoom — the cursor-anchored invariant.
        let origin = (0.0, 0.0);
        let v = ViewportView {
            pan: (15.0, 40.0),
            zoom: 1.0,
        };
        let cursor = (250.0, 120.0);
        let before = v.unproject(cursor, origin);
        let zoomed = v.zoom_about(3.0, cursor, origin);
        let after = zoomed.unproject(cursor, origin);
        approx(before, after);
        // And the cursor still projects from that same content point.
        approx(zoomed.project(before, origin), cursor);
    }

    #[test]
    fn pan_trigger_matches_exactly() {
        let t = PanTrigger::default();
        assert!(t.matches(PointerButton::Primary, KeyModifiers::default()));
        assert!(!t.matches(PointerButton::Middle, KeyModifiers::default()));
        // Extra modifier beyond the (empty) mask does not match.
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        assert!(!t.matches(PointerButton::Primary, shift));
    }
}
