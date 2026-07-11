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
/// drag: on empty content the pan starts at the press, and on a keyed
/// child the press stays a click unless it travels a few pixels while
/// held, at which point it converts into a pan (the map-app marker-tap
/// vs map-drag arbitration — clicks stay clicks, drags pan from
/// anywhere). Widgets that own their drag (text selection, sliders,
/// text inputs) keep it. Set [`KeyModifiers`] and/or a different
/// [`PointerButton`] (e.g. middle-button or space-drag, Figma-style) via
/// [`El::pan_button`](crate::tree::El::pan_button) /
/// [`El::pan_modifier`](crate::tree::El::pan_modifier); a dedicated
/// trigger can't collide with clicks, so it pans at press from anywhere.
/// Touch contacts count as the primary button: under the default
/// trigger a drag pans (scrollables inside the content still scroll,
/// and sub-threshold contacts stay taps), while a dedicated-trigger
/// viewport is wheel/programmatic-only on touch.
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

/// How a [`ViewportRequest`] moves the view to its target framing —
/// mirrors the DOM's `ScrollBehavior` (`scrollIntoView({ behavior })`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewportBehavior {
    /// Jump to the target framing immediately (DOM `"instant"`). The
    /// default.
    #[default]
    Instant,
    /// Fly to the target along a smooth zoom-out / translate / zoom-in
    /// path (DOM `"smooth"`; see [`ZoomPath`]), over a duration derived
    /// from the path length — there is deliberately no duration knob,
    /// as with DOM smooth scrolling. A user pan/zoom gesture mid-flight
    /// cancels the flight where it is; a new request retargets from the
    /// in-flight view. Degrades to [`Instant`](Self::Instant) under
    /// [`AnimationMode::Settled`](crate::state::AnimationMode::Settled)
    /// (headless / snapshot rendering) and on a [`FitPolicy::Lock`]
    /// viewport (whose framing is not free to leave home).
    Smooth,
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
    /// With [`ViewportBehavior::Smooth`], the policy re-arms when the
    /// flight *arrives*, so it doesn't snap over the animation.
    FitContent {
        /// `.key(...)` of the target viewport.
        key: String,
        /// Margin in logical px between the content bbox and the
        /// viewport edge.
        padding: f32,
        /// Jump or fly. Defaults to [`ViewportBehavior::Instant`].
        behavior: ViewportBehavior,
    },
    /// Snap back to the reset framing: `pan = (0, 0)`, `zoom = 1.0`.
    ResetView {
        /// `.key(...)` of the target viewport.
        key: String,
        /// Jump or fly. Defaults to [`ViewportBehavior::Instant`].
        behavior: ViewportBehavior,
    },
    /// Pan (keeping the current zoom) so the given content-space point
    /// lands at the center of the viewport.
    CenterOn {
        /// `.key(...)` of the target viewport.
        key: String,
        /// Point in content coordinates (the same space children are
        /// laid out in: logical px, pre-transform).
        point: (f32, f32),
        /// Jump or fly. Defaults to [`ViewportBehavior::Instant`].
        behavior: ViewportBehavior,
    },
    /// Frame an arbitrary content-space rect (issue #122): choose the
    /// largest zoom (within the configured `min_zoom..=max_zoom`) that
    /// fits `rect` inside the viewport with `padding` logical px of
    /// margin on every side, then center it — `scrollIntoView()` for a
    /// pan/zoom canvas. The scope-as-camera primitive: lay content out
    /// once and fly the camera to a region instead of re-rooting the
    /// layout.
    ///
    /// Unlike [`Self::FitContent`], this deliberately frames a *sub*-rect
    /// of larger content, so it does **not** re-arm an armed
    /// [`FitPolicy::Contain`] — it takes the view over exactly like a
    /// user pan/zoom or a [`Self::CenterOn`] (one-shot, off home). A
    /// degenerate rect (`w` and `h` both `<= 0`) is treated as a point:
    /// centered at the current zoom, i.e. [`Self::CenterOn`] its origin.
    FrameRect {
        /// `.key(...)` of the target viewport.
        key: String,
        /// The region to frame, in content coordinates (the same space
        /// children are laid out in: logical px, pre-transform).
        rect: crate::tree::Rect,
        /// Margin in logical px between `rect` and the viewport edge.
        padding: f32,
        /// Jump or fly. Defaults to [`ViewportBehavior::Instant`].
        behavior: ViewportBehavior,
    },
}

impl ViewportRequest {
    /// The `.key(...)` of the viewport this request targets.
    pub fn key(&self) -> &str {
        match self {
            ViewportRequest::FitContent { key, .. }
            | ViewportRequest::ResetView { key, .. }
            | ViewportRequest::CenterOn { key, .. }
            | ViewportRequest::FrameRect { key, .. } => key,
        }
    }

    /// How this request moves the view (jump or fly).
    pub fn behavior(&self) -> ViewportBehavior {
        match self {
            ViewportRequest::FitContent { behavior, .. }
            | ViewportRequest::ResetView { behavior, .. }
            | ViewportRequest::CenterOn { behavior, .. }
            | ViewportRequest::FrameRect { behavior, .. } => *behavior,
        }
    }
}

/// The zoom-out aggressiveness of a [`ZoomPath`] — van Wijk & Nuij's ρ,
/// at the paper's (and d3's) recommended `√2`. Larger values arc further
/// out during the translate phase.
const ZOOM_PATH_RHO: f64 = std::f64::consts::SQRT_2;

/// Absolute floor (content-space px) under which a translation flies as
/// a pure zoom. The effective guard is relative — see
/// [`ZoomPath::new`] — since the arc parameters degenerate whenever the
/// translation is small *relative to the widths*, not just near zero.
const ZOOM_PATH_MIN_TRANSLATION: f64 = 1e-3;

/// A smooth zoom-and-pan path between two viewport framings, after
/// van Wijk & Nuij (*Smooth and efficient zooming and panning*, 2003) —
/// the same path `d3.interpolateZoom` implements. A framing is
/// `(cx, cy, w)`: the content-space point at the viewport's center and
/// the content-space width the viewport shows (`inner.w / zoom`). The
/// path zooms out, translates, and zooms back in along a hyperbolic arc,
/// so mid-flight frames keep both endpoints' context on screen instead
/// of tunneling across the canvas at full magnification.
///
/// Pure math with no clock: sample with a progress fraction `t ∈ [0, 1]`
/// and pace `t` however you like. [`length`](Self::length) is the
/// perceptual path length `S` — the natural basis for a duration. This
/// is a Layer-3 primitive: the smooth [`ViewportRequest`]s ride on it,
/// and a custom host or widget can sample it directly.
#[derive(Clone, Copy, Debug)]
pub struct ZoomPath {
    start: (f32, f32, f32),
    end: (f32, f32, f32),
    kind: PathKind,
    length: f32,
}

/// The two path shapes: a degenerate straight blend when the centers
/// coincide, and the general hyperbolic arc.
#[derive(Clone, Copy, Debug)]
enum PathKind {
    /// Centers (nearly) coincide: geometric width interpolation, linear
    /// center blend across the (sub-pixel) gap.
    Blend,
    /// The general van Wijk arc, precomputed at construction.
    Arc {
        /// Distance between the centers, content-space px.
        d1: f64,
        /// The start parameter `r0` on the hyperbola.
        r0: f64,
        /// `cosh(r0)` / `sinh(r0)`, hoisted out of the per-sample math.
        cosh_r0: f64,
        sinh_r0: f64,
    },
}

impl ZoomPath {
    /// The path from `start` to `end`, each a `(cx, cy, w)` framing with
    /// `w > 0` (non-positive widths are clamped to a tiny epsilon).
    pub fn new(start: (f32, f32, f32), end: (f32, f32, f32)) -> Self {
        let w0 = f64::from(start.2).max(1e-6);
        let w1 = f64::from(end.2).max(1e-6);
        let dx = f64::from(end.0) - f64::from(start.0);
        let dy = f64::from(end.1) - f64::from(start.1);
        let d2 = dx * dx + dy * dy;
        let d1 = d2.sqrt();
        let rho = ZOOM_PATH_RHO;
        let blend = |start, end| Self {
            start,
            end,
            kind: PathKind::Blend,
            length: ((w1 / w0).ln().abs() / rho) as f32,
        };
        // Fly as a pure zoom when the translation is negligible —
        // *relative to the widths*: the arc parameter `b` grows like
        // `w²/(w·d)`, so a sub-pixel translation paired with a large
        // width change would push it into catastrophic-cancellation
        // territory even though the flight is visually a straight zoom.
        if d1 < ZOOM_PATH_MIN_TRANSLATION.max(1e-4 * w0.max(w1)) {
            return blend(start, end);
        }
        let rho2 = rho * rho;
        let b0 = (w1 * w1 - w0 * w0 + rho2 * rho2 * d2) / (2.0 * w0 * rho2 * d1);
        let b1 = (w1 * w1 - w0 * w0 - rho2 * rho2 * d2) / (2.0 * w1 * rho2 * d1);
        // r = log(√(b²+1) − b) = −asinh(b); `asinh` stays exact where the
        // naive form cancels to `ln(0)` for large `b`.
        let r0 = -b0.asinh();
        let r1 = -b1.asinh();
        let length = ((r1 - r0) / rho) as f32;
        // Overflow belt (e.g. `d²` or `w²` past f64 range): degrade to
        // the always-finite blend rather than sampling NaN framings.
        if !(length.is_finite() && length > 0.0) {
            return blend(start, end);
        }
        Self {
            start,
            end,
            kind: PathKind::Arc {
                d1,
                r0,
                cosh_r0: r0.cosh(),
                sinh_r0: r0.sinh(),
            },
            length,
        }
    }

    /// The perceptual path length `S`, in ρ-normalized units: `0` for a
    /// no-op path, ~1 per zoom factor of `e^√2 ≈ 4`, growing with the
    /// translation distance relative to the viewport width.
    pub fn length(&self) -> f32 {
        self.length
    }

    /// The framing at progress `t` (clamped to `[0, 1]`). The endpoints
    /// return the constructor inputs bit-exactly, so arrival lands on
    /// the resolved target with no float drift.
    pub fn sample(&self, t: f32) -> (f32, f32, f32) {
        if t <= 0.0 || self.length == 0.0 {
            return self.start;
        }
        if t >= 1.0 {
            return self.end;
        }
        let (cx0, cy0, w0) = (
            f64::from(self.start.0),
            f64::from(self.start.1),
            f64::from(self.start.2).max(1e-6),
        );
        let (cx1, cy1, w1) = (
            f64::from(self.end.0),
            f64::from(self.end.1),
            f64::from(self.end.2).max(1e-6),
        );
        let t = f64::from(t);
        match self.kind {
            PathKind::Blend => {
                let w = w0 * (w1 / w0).powf(t);
                (
                    (cx0 + t * (cx1 - cx0)) as f32,
                    (cy0 + t * (cy1 - cy0)) as f32,
                    w as f32,
                )
            }
            PathKind::Arc {
                d1,
                r0,
                cosh_r0,
                sinh_r0,
            } => {
                let rho = ZOOM_PATH_RHO;
                let s = t * f64::from(self.length);
                let r = rho * s + r0;
                // u runs 0 → 1 along the center-to-center line.
                let u = w0 / (rho * rho * d1) * (cosh_r0 * r.tanh() - sinh_r0);
                let w = w0 * cosh_r0 / r.cosh();
                (
                    (cx0 + u * (cx1 - cx0)) as f32,
                    (cy0 + u * (cy1 - cy0)) as f32,
                    w as f32,
                )
            }
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

    #[test]
    fn zoom_path_endpoints_are_bit_exact() {
        let a = (100.0, 200.0, 800.0);
        let b = (5000.0, -300.0, 120.0);
        let p = ZoomPath::new(a, b);
        assert_eq!(p.sample(0.0), a);
        assert_eq!(p.sample(1.0), b);
        // Out-of-range progress clamps to the endpoints.
        assert_eq!(p.sample(-0.5), a);
        assert_eq!(p.sample(2.0), b);
        assert!(p.length() > 0.0);
    }

    #[test]
    fn zoom_path_arcs_out_for_long_translations() {
        // A same-zoom flight across the canvas must zoom out mid-path —
        // the width hump is the whole point of the van Wijk arc.
        let p = ZoomPath::new((0.0, 0.0, 800.0), (10_000.0, 0.0, 800.0));
        let (_, _, w_mid) = p.sample(0.5);
        assert!(w_mid > 800.0 * 1.5, "mid-flight width: {w_mid}");
        // And the center crosses the halfway line at half progress
        // (the arc is symmetric for symmetric endpoints).
        let (cx_mid, _, _) = p.sample(0.5);
        assert!((cx_mid - 5_000.0).abs() < 1.0, "mid center: {cx_mid}");
    }

    #[test]
    fn zoom_path_center_progress_is_monotone() {
        let p = ZoomPath::new((0.0, 0.0, 400.0), (3_000.0, 1_500.0, 900.0));
        let mut last = f32::NEG_INFINITY;
        for i in 0..=20 {
            let (cx, _, w) = p.sample(i as f32 / 20.0);
            assert!(cx >= last - 1e-3, "center backtracked at step {i}: {cx}");
            assert!(w > 0.0);
            last = cx;
        }
    }

    #[test]
    fn zoom_path_pure_zoom_is_geometric() {
        let p = ZoomPath::new((50.0, 50.0, 100.0), (50.0, 50.0, 1_600.0));
        let (cx, cy, w) = p.sample(0.5);
        assert!((cx - 50.0).abs() < 1e-3 && (cy - 50.0).abs() < 1e-3);
        // Geometric midpoint of 100 → 1600 is 400.
        assert!((w - 400.0).abs() < 0.5, "geometric mid width: {w}");
        assert!(p.length() > 0.0);
    }

    #[test]
    fn zoom_path_noop_has_zero_length() {
        let a = (10.0, 20.0, 300.0);
        let p = ZoomPath::new(a, a);
        assert_eq!(p.length(), 0.0);
        assert_eq!(p.sample(0.5), a);
    }

    /// Review finding: an f32-noise translation paired with a large
    /// width change must fly as a pure zoom — the arc parameters hit
    /// catastrophic cancellation there (`ln(√(b²+1) − b)` → `ln(0)`).
    #[test]
    fn zoom_path_near_pure_zoom_stays_finite() {
        let p = ZoomPath::new((16_384.0, 8_000.0, 800.0), (16_384.002, 8_000.0, 30_000.0));
        assert!(p.length().is_finite() && p.length() > 0.0);
        for i in 0..=10 {
            let (cx, cy, w) = p.sample(i as f32 / 10.0);
            assert!(
                cx.is_finite() && cy.is_finite() && w.is_finite() && w > 0.0,
                "sample {i}: ({cx}, {cy}, {w})"
            );
        }
    }

    /// Review finding: extreme width ratios push the arc parameter past
    /// the naive formula's precision even with a real translation —
    /// `asinh` keeps it exact.
    #[test]
    fn zoom_path_extreme_width_ratio_stays_finite() {
        let p = ZoomPath::new((0.0, 0.0, 800.0), (6_000.0, 0.0, 5.0e7));
        assert!(p.length().is_finite() && p.length() > 0.0);
        for i in 0..=10 {
            let (cx, cy, w) = p.sample(i as f32 / 10.0);
            assert!(
                cx.is_finite() && cy.is_finite() && w.is_finite() && w > 0.0,
                "sample {i}: ({cx}, {cy}, {w})"
            );
        }
    }

    #[test]
    fn zoom_path_is_reversible() {
        let a = (0.0, 0.0, 500.0);
        let b = (2_000.0, 800.0, 250.0);
        let fwd = ZoomPath::new(a, b);
        let back = ZoomPath::new(b, a);
        assert!((fwd.length() - back.length()).abs() < 1e-4);
        for i in 1..10 {
            let t = i as f32 / 10.0;
            let (fx, fy, fw) = fwd.sample(t);
            let (bx, by, bw) = back.sample(1.0 - t);
            assert!((fx - bx).abs() < 0.1, "x at t={t}: {fx} vs {bx}");
            assert!((fy - by).abs() < 0.1, "y at t={t}: {fy} vs {by}");
            assert!((fw - bw).abs() < 0.1, "w at t={t}: {fw} vs {bw}");
        }
    }
}
