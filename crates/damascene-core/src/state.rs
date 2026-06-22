//! [`UiState`] — the renderer's interaction-state side store.
//!
//! Holds pointer position, hovered/pressed/focused targets, per-node
//! scroll offsets, the app-supplied hotkey registry, and the per-(node,
//! prop) animation map. The host doesn't touch this directly; backend
//! runners such as `damascene_wgpu::Runner` own one and route input events
//! through it.
//!
//! Visual delta application: if `pressed` is set, that node renders with
//! `state = Press`. Otherwise, if `hovered` is set, that node renders
//! with `state = Hover`. Press takes precedence so clicking a button
//! that's also hovered shows the press visual, not the hover visual.
//! Focus is independent of both — the focus ring is its own envelope.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

mod animation;
mod camera;
mod click;
mod cursor;
pub(crate) mod dyn_height;
mod focus;
mod interaction;
mod keyboard;
pub(crate) mod query;
pub(crate) mod resize;
mod scroll;
mod selection;
mod toast;
mod types;
mod viewport;
mod widget_state;

use std::fmt::Debug;
// `web_time::Instant` is API-identical to `std::time::Instant` on
// native and uses `performance.now()` on wasm32 — std's `Instant::now()`
// panics in the browser because there is no monotonic clock there.

use crate::event::{KeyModifiers, PointerButton, PointerKind, UiTarget};

pub use types::{
    AnimationMode, EnvelopeKind, LONG_PRESS_DELAY, ScrollMetrics, ThumbDrag, WidgetState,
};
pub(crate) use types::{
    ScrollAnchor, SelectionDrag, SelectionDragGranularity, TOUCH_DRAG_THRESHOLD, TouchGestureState,
    ViewportMetrics, VirtualAnchor, caret_blink_alpha_for,
};

use types::{
    AnimationState, CaretState, ClickState, FocusState, HotkeyState, LayoutState,
    NodeInteractionState, PopoverFocusState, ScrollState, SelectionState, ToastState, TooltipState,
    ViewportState, WidgetStateStore,
};

/// Internal UI state — interaction trackers + the side maps the library
/// writes during layout / state-apply / animation-tick passes. Owned by
/// the renderer; the host doesn't interact with this directly.
///
/// The side maps replace the per-node bookkeeping fields that used to
/// live on `El` (computed rect, interaction state, envelope amounts).
/// Keying is by `El::computed_id`, the path-shaped string assigned by
/// the layout pass.
#[derive(Default)]
pub struct UiState {
    /// Last known pointer position in **logical** pixels. `None` until
    /// the pointer enters the window.
    pub pointer_pos: Option<(f32, f32)>,
    /// Modality of the most recent pointer ingest. Used to stamp
    /// emitted [`crate::event::UiEvent::pointer_kind`] and to gate
    /// hover-only behavior on touch. Defaults to
    /// [`PointerKind::Mouse`] until the first ingest, which matches
    /// historical behavior on hosts without touch.
    pub pointer_kind: PointerKind,
    /// Touch-gesture state machine. Tracks whether the active touch
    /// contact is awaiting threshold disambiguation
    /// ([`TouchGestureState::Pending`]), has committed to scrolling
    /// ([`TouchGestureState::Scrolling`]), or is idle / committed to
    /// drag ([`TouchGestureState::None`]). Mouse and pen pointers
    /// stay at `None`.
    pub(crate) touch_gesture: TouchGestureState,
    /// Hit-test target currently under the pointer, refreshed on every
    /// pointer move. Drives the hover visual and
    /// `PointerEnter`/`PointerLeave` emission.
    pub hovered: Option<UiTarget>,
    /// Target of the active primary-button press — set at
    /// `pointer_down`, cleared at `pointer_up`. Renders with the press
    /// visual (which wins over hover) and receives `Drag` events.
    pub pressed: Option<UiTarget>,
    /// Secondary / middle button down-target, kept on a separate
    /// channel so it doesn't fight the primary `pressed` envelope or
    /// move focus. Carries the button kind so `pointer_up` knows which
    /// click variant to emit. Cleared by `pointer_up` matching the
    /// same button.
    pub(crate) pressed_secondary: Option<(UiTarget, PointerButton)>,
    /// URL of the text-link run under a primary press, when present.
    /// Set by `pointer_down` from `hit_test::link_at`; consumed by
    /// `pointer_up`, which emits `UiEventKind::LinkActivated` only
    /// when the up position lands on the same link URL — same
    /// press-then-confirm contract as a normal `Click`.
    pub(crate) pressed_link: Option<String>,
    /// URL of the text-link run currently under the pointer (no
    /// button press required). Tracked by `pointer_moved` so the
    /// cursor resolver can return [`crate::cursor::Cursor::Pointer`]
    /// over links without the text leaves having to be keyed
    /// hover-test targets. Cleared on `pointer_left`.
    pub(crate) hovered_link: Option<String>,
    /// Keyboard-focused target. Moved by Tab traversal, pointer
    /// presses on focusable nodes, and programmatic focus requests;
    /// receives `Activate` / `Escape` / `KeyDown` / `TextInput`
    /// routing.
    pub focused: Option<UiTarget>,
    /// Whether the focused element should display its focus ring.
    /// Tracks the web platform's `:focus-visible` heuristic: keyboard
    /// focus (Tab, arrow-nav) raises the flag; pointer-down clears it.
    /// Widgets where the ring belongs even on click — text inputs and
    /// text areas, where the ring communicates "this surface is now
    /// active" beyond the caret alone — opt back in via
    /// [`crate::tree::El::always_show_focus_ring`].
    pub focus_visible: bool,
    pub(crate) focus: FocusState,
    /// Mirror of the application's current
    /// [`crate::selection::Selection`]. Set by the host runner once
    /// per frame from [`crate::event::App::selection`]; read by the
    /// painter to draw highlight bands and by the selection manager
    /// to know what's currently active when extending a drag.
    pub current_selection: crate::selection::Selection,
    /// Internal selection traversal and drag state.
    pub(crate) selection: SelectionState,
    pub(crate) click: ClickState,
    pub(crate) caret: CaretState,
    pub(crate) popover_focus: PopoverFocusState,
    pub(crate) tooltip: TooltipState,
    pub(crate) scroll: ScrollState,
    /// Persistent pan/zoom for [`viewport()`](crate::tree::viewport)
    /// containers, plus per-frame layout metrics and the active pan
    /// drag. See [`viewport`](self::viewport).
    pub(crate) viewport: ViewportState,
    /// Edge-resize subsystem for `.user_resizable()` panes: persistent
    /// user-dragged sizes plus per-frame grab-band scratch. See
    /// [`resize`](self::resize).
    pub(crate) resize: resize::ResizeState,
    /// Per-`Scene3D`-node camera poses (current + goal + spring velocity),
    /// keyed by `computed_id`. The library-owned interactive camera; see
    /// [`camera`](self::camera).
    pub(crate) cameras: camera::CameraStore,
    /// Per-`Scene3D`-node depth maps captured by the backend, keyed by
    /// `computed_id`. The draw-op pass reads these to occlude scene-anchored
    /// labels behind geometry; the backend populates them a frame late via
    /// [`scene_depth_mut`](Self::scene_depth_mut). See
    /// [`SceneDepthMap`](crate::scene::SceneDepthMap).
    scene_depth: std::collections::HashMap<String, crate::scene::SceneDepthMap>,
    /// Scatter point under the cursor, picked by the draw-op pass from the
    /// hover-label path and stored a frame late (like [`scene_depth`]). The
    /// app reads it via [`BuildCx::hovered_scene_point`] to drive its own
    /// detail UI on hover. `None` when no scene point is hovered.
    ///
    /// [`scene_depth`]: Self::scene_depth
    /// [`BuildCx::hovered_scene_point`]: crate::event::BuildCx::hovered_scene_point
    hovered_scene_point: Option<crate::scene::ScenePointPick>,
    /// Runtime-managed toast notification queue and id allocator.
    pub(crate) toast: ToastState,
    /// App-declared keyboard shortcuts and their action names.
    pub(crate) hotkeys: HotkeyState,
    /// Visual prop animations, state envelopes, and animation pacing.
    pub(crate) animation: AnimationState,

    // ---- side maps (formerly El bookkeeping) ----
    /// Layout-owned rect and key-index side maps.
    pub(crate) layout: LayoutState,
    /// Per-node interaction states derived from focused/pressed/hovered
    /// trackers by [`Self::apply_to_state`].
    pub(crate) node_states: NodeInteractionState,
    /// Per-(node, type) widget state buckets. The library owns the
    /// storage but never reads the values — they're for widget authors
    /// to stash text-input carets, dropdown open flags, etc. Entries
    /// are GC'd alongside envelopes/animations when a node leaves the
    /// tree (see [`Self::tick_visual_animations`]).
    widget_states: WidgetStateStore,
    /// Last known keyboard modifier mask. Updated by the host runner
    /// from winit's `ModifiersChanged`; pointer events stamp this
    /// value into their `UiEvent.modifiers` so widgets that need to
    /// detect Shift+click / Ctrl+drag can read it without separate
    /// plumbing.
    pub modifiers: KeyModifiers,
}

impl UiState {
    /// An empty state store; equivalent to `Self::default()`. Backend
    /// runners create one at startup and feed every input event
    /// through it.
    pub fn new() -> Self {
        Self::default()
    }

    /// The captured depth map for a `Scene3D` node, if the backend has
    /// produced one. `None` until the first map arrives — callers treat
    /// that as "occlude all labels" (see [`crate::scene::SceneDepthMap`]).
    pub fn scene_depth(&self, id: &str) -> Option<&crate::scene::SceneDepthMap> {
        self.scene_depth.get(id)
    }

    /// Mutable access to the per-node scene depth maps, for the backend to
    /// install freshly read-back maps and GC nodes that have left the tree.
    pub fn scene_depth_mut(
        &mut self,
    ) -> &mut std::collections::HashMap<String, crate::scene::SceneDepthMap> {
        &mut self.scene_depth
    }

    /// Iterate the captured `(id, map)` scene depth maps. Exposed for
    /// backend integration tests; app code reads a single map via
    /// [`scene_depth`](Self::scene_depth).
    #[doc(hidden)]
    pub fn scene_depth_maps(&self) -> impl Iterator<Item = (&str, &crate::scene::SceneDepthMap)> {
        self.scene_depth.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// The scatter point currently under the cursor, if any. Picked by the
    /// draw-op pass and refreshed each prepare (a frame late). App code reads
    /// this through [`BuildCx::hovered_scene_point`](crate::event::BuildCx::hovered_scene_point).
    pub fn hovered_scene_point(&self) -> Option<&crate::scene::ScenePointPick> {
        self.hovered_scene_point.as_ref()
    }

    /// Install the hovered-point pick computed by the draw-op pass. Called by
    /// the runtime right after `draw_ops`; `None` clears a stale pick.
    pub(crate) fn set_hovered_scene_point(&mut self, pick: Option<crate::scene::ScenePointPick>) {
        self.hovered_scene_point = pick;
    }
}

impl Debug for UiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiState")
            .field("pointer_pos", &self.pointer_pos)
            .field("hovered", &self.hovered)
            .field("pressed", &self.pressed)
            .field("focused", &self.focused)
            .field("focus_visible", &self.focus_visible)
            .field("focus", &self.focus)
            .field("popover_focus", &self.popover_focus)
            .field("click", &self.click)
            .field("caret", &self.caret)
            .field("scroll", &self.scroll)
            .field("toast", &self.toast)
            .field("tooltip", &self.tooltip)
            .field("hotkeys", &self.hotkeys)
            .field("animation", &self.animation)
            .field("layout", &self.layout)
            .field("node_states", &self.node_states)
            .field("modifiers", &self.modifiers)
            .field("widget_states", &self.widget_states)
            .finish()
    }
}

#[cfg(test)]
mod tests;
