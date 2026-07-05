//! Data buckets owned by [`UiState`](super::UiState).
//!
//! This module keeps the side-store data shapes separate from the
//! runtime behavior implemented on `UiState`.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use rustc_hash::{FxHashMap, FxHashSet};
use web_time::Instant;

use crate::anim::{AnimProp, Animation};
use crate::event::{KeyChord, UiTarget};
use crate::tree::{InteractionState, Rect};

/// Animation pacing.
///
/// `Live` steps springs by wall-clock time, used by the windowed runner.
/// `Settled` snaps every in-flight animation to its target each tick,
/// used by headless paths so single-frame snapshots are deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationMode {
    /// Step springs by wall-clock time — the windowed runner's mode.
    #[default]
    Live,
    /// Snap every in-flight animation to its target each tick, so
    /// headless single-frame snapshots are deterministic.
    Settled,
}

/// State-driven visual envelope kind. Each is a 0..1 amount written by
/// the animation tick and consumed by [`crate::draw_ops::draw_ops`] to
/// modulate a node's surface visuals (lighten on hover, darken on press,
/// fade in/out the focus ring).
///
/// Two flavours:
///
/// - **Per-node envelopes** (`Hover`, `Press`, `FocusRing`) track whether
///   *this exact node* is the active hover / press / focus target. Drive
///   per-element visuals — hover-lighten, press-darken, focus-ring fade.
///   Exactly one node owns each at a time, mirroring the single-target
///   `apply_to_state` semantics.
/// - **Subtree envelopes** (`SubtreeHover`, `SubtreePress`,
///   `SubtreeFocus`) track whether the active hover / press / focus
///   target is *this node or any descendant*. Drive
///   region-shaped affordances — hover-revealed close icons, action
///   pills that should stay visible while the cursor moves to a
///   focusable child, hover-driven translate / scale / tint. Multiple
///   nodes can be "hot" simultaneously (every ancestor of the leaf
///   target). CSS `:hover` semantics, lifted onto our id-keyed tree.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum EnvelopeKind {
    /// Per-node: this exact node is the hover target (hover-lighten).
    Hover,
    /// Per-node: this exact node is the press target (press-darken).
    Press,
    /// Per-node: this exact node is the focus target (focus-ring fade).
    FocusRing,
    /// Subtree: the hover target is this node or a descendant.
    SubtreeHover,
    /// Subtree: the press target is this node or a descendant.
    SubtreePress,
    /// Subtree: the focus target is this node or a descendant.
    SubtreeFocus,
}

/// Runtime visual animation state: app-authored prop animations plus
/// library-owned hover/press/focus envelopes and their pacing mode.
#[derive(Default)]
pub(crate) struct AnimationState {
    /// In-flight animations keyed by `(computed_id, prop)`. Created
    /// lazily as state transitions happen; trimmed by
    /// [`UiState::tick_visual_animations`](super::UiState::tick_visual_animations)
    /// when their nodes leave the tree.
    pub(crate) animations: FxHashMap<(std::sync::Arc<str>, AnimProp), Animation>,
    /// State-envelope amounts (0..1) per (node, kind), written by the
    /// animation tick. `draw_ops` reads these to modulate the surface
    /// visuals; missing entries read as `0.0`.
    pub(crate) envelopes: FxHashMap<(std::sync::Arc<str>, EnvelopeKind), f32>,
    /// Animation pacing mode. Default is `Live`; headless render
    /// binaries switch to `Settled` so single-frame snapshots reflect
    /// the post-animation visual.
    pub(crate) mode: AnimationMode,
}

impl Debug for AnimationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationState")
            .field("animations", &self.animations)
            .field("envelopes", &self.envelopes)
            .field("mode", &self.mode)
            .finish()
    }
}

/// App-declared keyboard shortcuts captured by the host each frame and
/// matched before focused-widget key handling.
#[derive(Default)]
pub(crate) struct HotkeyState {
    /// App-level hotkey registry; the host snapshots `App::hotkeys()`
    /// each frame and stores it here. Matched in `key_down` ahead of
    /// focus activation.
    pub(crate) registry: Vec<(KeyChord, String)>,
}

impl Debug for HotkeyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotkeyState")
            .field("registry", &self.registry)
            .finish()
    }
}

/// Per-instance state owned by a widget. Widget authors define their own
/// state types (e.g. text-input caret + selection, virtual list scroll
/// offset, dropdown open/closed) and stash them on [`UiState`](super::UiState)
/// keyed by node id via [`UiState::widget_state`](super::UiState::widget_state)
/// / [`UiState::widget_state_mut`](super::UiState::widget_state_mut).
///
/// The library never reads the state itself — it just owns the
/// storage, wipes entries when a node leaves the tree, and surfaces
/// `debug_summary()` in the tree dump so the agent loop can see what
/// the widget thinks.
///
/// # Symmetry
///
/// This is the storage contract for stateful widgets. Stock widgets get
/// no privileged shortcuts; everything they do here, an app-defined
/// widget can do too. See `widget_kit.md`.
pub trait WidgetState: 'static + Debug + Send + Sync {
    /// One-line summary for the tree dump. Default empty (the entry's
    /// type name still shows up via the inspector). Override to surface
    /// the most useful per-frame state — e.g. a text input might
    /// return `"caret=12 sel=8..14"`.
    fn debug_summary(&self) -> String {
        String::new()
    }
}

/// Subtrait combining [`WidgetState`] with [`Any`] so the type-erased
/// box can both call trait methods and downcast back to `T`.
pub(super) trait AnyWidgetState: WidgetState {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn type_name(&self) -> &'static str;
}

impl<T: WidgetState> AnyWidgetState for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// Type-erased per-node widget storage owned by [`UiState`](super::UiState).
/// Public access stays through `UiState::widget_state*`; this store just
/// keeps the raw buckets and their debug summaries together.
#[derive(Default)]
pub(super) struct WidgetStateStore {
    pub(super) entries: HashMap<(String, TypeId), Box<dyn AnyWidgetState>>,
}

impl Debug for WidgetStateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.entries
                    .iter()
                    .map(|((id, _), b)| (id.as_str(), b.type_name(), b.debug_summary())),
            )
            .finish()
    }
}

/// Side maps written by the layout pass and read by hit-testing,
/// drawing, custom layout callbacks, and keyed overlay placement.
#[derive(Default)]
pub(crate) struct LayoutState {
    /// Computed rect per **keyed** node (plus the root), written by
    /// the layout pass. Serves id/key lookups that don't hold the
    /// `El` — [`crate::state::UiState::rect_of_key`], custom-layout
    /// `rect_of_key`/`rect_of_id` anchoring — including the popover
    /// contract of answering with *last frame's* rect for a keyed
    /// anchor that lays out later in the tree. Per-node rects for the
    /// full tree live on the nodes themselves
    /// ([`crate::tree::El::computed_rect`]).
    pub(crate) keyed_rects: FxHashMap<std::sync::Arc<str>, Rect>,
    /// `key -> computed_id` map, refreshed at the top of every layout
    /// pass. Populated only for nodes that carry an author-set `key`;
    /// duplicate keys keep the first entry seen in tree order.
    pub(crate) key_index: FxHashMap<String, std::sync::Arc<str>>,
    /// `computed_id`s already reported as duplicates, so the per-frame
    /// duplicate-id warning fires once per offending id rather than
    /// every layout. Reused across the `UiState`'s lifetime (issue #64).
    pub(crate) warned_duplicate_ids: rustc_hash::FxHashSet<String>,
}

impl Debug for LayoutState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutState")
            .field("keyed_rects", &self.keyed_rects)
            .field("key_index", &self.key_index)
            .finish()
    }
}

/// Resolved per-node interaction state written after input processing
/// and read by animation/drawing passes.
#[derive(Default)]
pub(crate) struct NodeInteractionState {
    pub(crate) nodes: FxHashMap<std::sync::Arc<str>, InteractionState>,
}

impl Debug for NodeInteractionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeInteractionState")
            .field("nodes", &self.nodes)
            .finish()
    }
}

/// Layout snapshot for a scrollable node. Written each frame by
/// `apply_scroll_offset`; read by the scrollbar thumb in `draw_ops`
/// and by `runtime`'s thumb-drag plumbing. `viewport_h` is the
/// scrollable's inner-rect height (post-padding); `content_h` is the
/// total height of its children; `max_offset` is `(content_h -
/// viewport_h).max(0.0)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollMetrics {
    /// Inner-rect (post-padding) height of the scrollable's viewport,
    /// in logical pixels.
    pub viewport_h: f32,
    /// Total height of the scrollable's children, in logical pixels.
    pub content_h: f32,
    /// Largest valid scroll offset: `(content_h - viewport_h).max(0.0)`.
    pub max_offset: f32,
}

/// Granularity for an active text-selection drag. Single-click drags
/// extend by caret position; multi-click drags keep selecting whole
/// units as the pointer moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionDragGranularity {
    Character,
    Word,
    Leaf,
}

/// Active text-selection drag, captured at `pointer_down` on a
/// selectable leaf. For multi-click drags the initial range is kept so
/// crossing back over the selected word / leaf can flip the fixed edge
/// without collapsing to a raw caret position.
#[derive(Clone, Debug)]
pub(crate) struct SelectionDrag {
    pub anchor: crate::selection::SelectionPoint,
    pub head: crate::selection::SelectionPoint,
    pub granularity: SelectionDragGranularity,
}

/// Internal selection-manager state derived from the laid-out tree and
/// active pointer drags. The app-visible selection value remains on
/// `UiState::current_selection` for compatibility.
#[derive(Clone, Debug, Default)]
pub(crate) struct SelectionState {
    /// Selectable text leaves in document (tree) order. Built post-
    /// layout by [`UiState::sync_selection_order`](super::UiState::sync_selection_order);
    /// consulted by the selection manager to map pointer hits to a
    /// [`crate::selection::SelectionPoint`] and to walk cross-element
    /// selections.
    pub(crate) order: Vec<UiTarget>,
    /// Active drag, set by `pointer_down` when the press lands on a
    /// selectable leaf and primary button. Cleared by `pointer_up`.
    pub(crate) drag: Option<SelectionDrag>,
}

/// Internal focus traversal data derived from the laid-out tree. The
/// currently focused target remains on `UiState::focused` for the
/// existing public API.
#[derive(Clone, Debug, Default)]
pub(crate) struct FocusState {
    pub(crate) order: Vec<UiTarget>,
    /// Programmatic focus requests buffered between frames. Hosts
    /// call [`UiState::push_focus_requests`] once per build with the
    /// keys produced by [`crate::event::App::drain_focus_requests`];
    /// `prepare_layout` drains and resolves them after the focus
    /// order has been rebuilt.
    pub(crate) pending_requests: Vec<String>,
}

/// Tracks the latest primary `pointer_down` so the next press can
/// extend a multi-click sequence. The runtime increments `count` when
/// a fresh press lands within `MULTI_CLICK_TIME` and `MULTI_CLICK_DIST`
/// of the previous press on the same hit-target; otherwise the
/// sequence resets to 1.
#[derive(Clone, Debug)]
pub(crate) struct ClickSequence {
    pub time: Instant,
    pub pos: (f32, f32),
    pub target_node_id: Option<String>,
    pub count: u8,
}

/// Runtime multi-click bookkeeping. Tracks the latest primary
/// `pointer_down` so the next press can decide whether to extend the
/// sequence or reset to a single click.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClickState {
    pub(crate) last: Option<ClickSequence>,
}

/// Multi-click time window. A press within this duration of the
/// previous matching press extends the sequence (count += 1).
pub(crate) const MULTI_CLICK_TIME: Duration = Duration::from_millis(500);
/// Multi-click distance window in logical pixels. Wider than typical
/// pointer jitter, narrower than a deliberate move to a new target.
pub(crate) const MULTI_CLICK_DIST: f32 = 4.0;

/// Touch-gesture state machine resolving the tap / drag / scroll /
/// long-press ambiguity. A finger going down can become any of the
/// four; the runner waits for movement or time to commit.
///
/// Mouse and pen pointers stay at [`TouchGestureState::None`] —
/// they don't share this ambiguity (left-button drag *means* drag,
/// right-click *means* context menu).
#[derive(Clone, Debug, Default)]
pub(crate) enum TouchGestureState {
    /// Idle, or the active touch already committed to drag (subsequent
    /// moves go through the regular Drag emission path).
    #[default]
    None,
    /// A touch press is held, awaiting movement or [`LONG_PRESS_DELAY`]
    /// to disambiguate. `consumes_drag` is captured at press time
    /// from the press target's (and ancestors') `consumes_touch_drag`
    /// flag. `started_at` drives the long-press deadline check; the
    /// runtime polls each frame and once `now - started_at >=
    /// LONG_PRESS_DELAY`, the press transitions to [`LongPressed`].
    Pending {
        initial: (f32, f32),
        consumes_drag: bool,
        started_at: Instant,
    },
    /// The active touch crossed the threshold without consuming
    /// drag, so subsequent moves drive scroll instead. The press
    /// has already been cancelled.
    Scrolling {
        last_pos: (f32, f32),
        last_time: Instant,
        velocity: f32,
        scroll_id: Option<String>,
    },
    /// The active touch was held in place past [`LONG_PRESS_DELAY`].
    /// A `LongPress` event has already been emitted. Non-editable
    /// targets have also been cancelled; editable capture-keys targets
    /// keep their press captured so movement can extend selection.
    LongPressed,
}

/// How many logical pixels a touch contact must move from its initial
/// position before the gesture state machine commits to drag or scroll.
/// Below this, the press stays a candidate tap and `Drag` emission is
/// suppressed.
pub(crate) const TOUCH_DRAG_THRESHOLD: f32 = 10.0;

/// Minimum scroll velocity, in logical pixels per second, needed to
/// continue scrolling after a touch release.
pub(crate) const SCROLL_MOMENTUM_MIN_VELOCITY: f32 = 80.0;

/// Scroll momentum stops once friction decays below this velocity.
pub(crate) const SCROLL_MOMENTUM_STOP_VELOCITY: f32 = 12.0;

/// Exponential friction applied to touch scroll momentum. Larger means
/// shorter glide. `4.8` lands in the native-feeling range without making
/// long content feel runaway.
pub(crate) const SCROLL_MOMENTUM_DECAY_PER_SEC: f32 = 4.8;

/// How long a touch contact must be held in place before the runtime
/// synthesizes a `UiEventKind::LongPress` event. 500ms matches the
/// Android long-press default and the upper end of iOS's range; below
/// 400ms scrolls and slow taps misfire as long-presses. Public so
/// host code or tests that simulate touch can match the runtime's
/// own threshold rather than guessing.
pub const LONG_PRESS_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// Caret stays solid for this long after activity (typing, caret
/// motion, focus arriving) before the blink cycle starts. Prevents
/// the caret from disappearing mid-keystroke.
pub(crate) const CARET_BLINK_GRACE: Duration = Duration::from_millis(500);
/// One on / off period of the caret blink. macOS-ish (~530ms each
/// half) but tunable; the painter only ever sees the resolved alpha,
/// not the period itself.
pub(crate) const CARET_BLINK_PERIOD: Duration = Duration::from_millis(1060);

/// Resolve the caret blink alpha for the given activity age. Returns
/// `1.0` while inside the post-activity grace window, then alternates
/// `1.0` (first half of each period) and `0.0` (second half).
pub(crate) fn caret_blink_alpha_for(age: Duration) -> f32 {
    if age < CARET_BLINK_GRACE {
        return 1.0;
    }
    let t = (age - CARET_BLINK_GRACE).as_millis() as u64;
    let half = (CARET_BLINK_PERIOD.as_millis() as u64) / 2;
    if ((t / half) & 1) == 0 { 1.0 } else { 0.0 }
}

/// Runtime blink state for the focused text caret. Text widgets update
/// this through [`UiState::bump_caret_activity`](super::UiState::bump_caret_activity);
/// the animation tick resolves the current alpha for paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct CaretState {
    /// When the focused-input caret last had visible activity (a
    /// selection change or a focus transition). `None` before the
    /// first bump — caret rendering treats that as solid.
    pub(crate) activity_at: Option<Instant>,
    /// Current caret blink alpha in `[0.0, 1.0]`, written by the
    /// animation tick from `activity_at`.
    pub(crate) blink_alpha: f32,
}

/// Active scrollbar thumb drag. `start_pointer_y` and `start_offset`
/// are captured at `pointer_down`; `pointer_moved` updates
/// `scroll.offsets[scroll_id]` to `start_offset + (dy *
/// max_offset / track_remaining)` so the cursor-thumb pixel
/// relationship stays 1:1.
#[derive(Clone, Debug)]
pub struct ThumbDrag {
    /// `computed_id` of the scrollable whose thumb is being dragged.
    pub scroll_id: String,
    /// Pointer y at `pointer_down`, in logical pixels.
    pub start_pointer_y: f32,
    /// The scrollable's offset at `pointer_down`, in logical pixels.
    pub start_offset: f32,
    /// Distance the thumb top can travel — `viewport_h - thumb_h`.
    /// Captured at drag start so a content-resize mid-drag doesn't
    /// retro-actively shift the cursor-thumb correspondence.
    pub track_remaining: f32,
    /// `max_offset` captured at drag start, for the same reason.
    pub max_offset: f32,
}

/// Active inertial scroll after a touch-drag release.
#[derive(Clone, Debug)]
pub(crate) struct ScrollMomentum {
    pub scroll_id: String,
    pub velocity: f32,
    pub last_tick: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct VirtualAnchor {
    pub row_key: String,
    pub row_index: usize,
    pub row_fraction: f32,
    pub viewport_y: f32,
    pub resolved_offset: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct ScrollAnchor {
    pub node_id: String,
    pub rect_fraction: f32,
    pub viewport_y: f32,
    pub resolved_offset: f32,
}

/// Runtime state for scrollable nodes. Kept as one subsystem inside
/// [`UiState`](super::UiState) so layout, paint, and input code do not
/// each grow their own loose side maps.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScrollState {
    /// Scroll offset (logical pixels) per scrollable node, keyed by
    /// `El::computed_id`. The layout pass reads this when positioning a
    /// scrollable's children and writes back the clamped value.
    pub(crate) offsets: FxHashMap<String, f32>,
    /// Per-scrollable layout metrics — viewport height, content
    /// height, max offset — written by the layout pass and read by
    /// `draw_ops` (to size the scrollbar thumb) and the runtime (to
    /// translate thumb-drag delta into offset delta).
    pub(crate) metrics: FxHashMap<String, ScrollMetrics>,
    /// Per-scrollable thumb rect (logical pixels), populated alongside
    /// `metrics` when the scrollable has `scrollbar` enabled and its
    /// content overflows. Read by `draw_ops` to paint the thumb. An
    /// entry is *absent* when the scrollbar is disabled or the content
    /// fits the viewport.
    pub(crate) thumb_rects: FxHashMap<String, Rect>,
    /// Per-scrollable track rect — the full vertical column that
    /// accepts pointer presses (wider than the visible thumb so the
    /// thumb is easy to grab; full viewport height so a click on the
    /// track above/below the thumb pages by a viewport). Same x-extent
    /// as `thumb_rects` but expanded to `SCROLLBAR_HITBOX_WIDTH` and
    /// the inner-rect height. Populated alongside `thumb_rects`.
    pub(crate) thumb_tracks: FxHashMap<String, Rect>,
    /// Realized (visible) row range per virtual list, keyed by
    /// `computed_id` and rewritten every layout (per-frame scratch,
    /// like the thumb maps). `(start, end)` is the half-open global
    /// index range of rows realized this frame. Apps query it via
    /// `BuildCx::visible_range` to drive cache eviction for off-screen
    /// row resources.
    pub(crate) visible_ranges: FxHashMap<String, (usize, usize)>,
    /// Active scrollbar drag, set by `pointer_down` when the press
    /// lands inside a thumb rect, consumed by `pointer_moved` to update
    /// the corresponding `offsets` entry, cleared by `pointer_up`.
    /// Pre-empts normal hit-test so thumb drags don't also fire
    /// app-level pointer events.
    pub(crate) thumb_drag: Option<ThumbDrag>,
    /// Active touch momentum for a scroll container. This is updated
    /// during `prepare_layout` before layout reads the scroll offsets,
    /// then cleared when velocity decays or an edge is hit.
    pub(crate) momentum: Option<ScrollMomentum>,
    /// Per-virtual-list row-height measurement cache, keyed by the
    /// virtual list node's `computed_id`, stable row identity, and
    /// layout-width bucket. Filled by `VirtualMode::Dynamic` as rows
    /// enter the viewport and are measured. Width is part of the key
    /// because wrapped text can change height during horizontal
    /// resizes.
    pub(crate) measured_row_heights: FxHashMap<String, FxHashMap<String, FxHashMap<u32, f32>>>,
    /// Incremental per-row height index for append-only dynamic virtual
    /// lists (`VirtualMode::Dynamic { append_only: true }`), keyed by the
    /// list node's `computed_id`. Persists across frames and is
    /// reconciled trim-then-append each pass so layout avoids the
    /// general path's O(n) per-frame height/key walks (issue #107).
    /// Cold-rebuilt on first frame, width change, or contract violation.
    /// Evicted by the same LRU as `measured_row_heights`.
    pub(crate) dyn_height_index: FxHashMap<String, crate::state::dyn_height::DynHeightIndex>,
    /// Dynamic virtual-list anchor per list. The previous frame's
    /// anchor resolves the current frame; layout then rebases this to
    /// a row point that is visible in the current viewport.
    pub(crate) virtual_anchors: FxHashMap<String, VirtualAnchor>,
    /// Plain scroll-container anchor per scrollable. The previous
    /// frame's visible descendant point resolves the current frame
    /// after content reflows, then layout rebases it to a still-visible
    /// descendant point. This preserves focus during horizontal
    /// resizes that change wrapped content height.
    pub(crate) scroll_anchors: FxHashMap<String, ScrollAnchor>,
    /// Programmatic scroll requests buffered between frames. Hosts
    /// call [`UiState::push_scroll_requests`] once per build with the
    /// requests produced by [`crate::event::App::drain_scroll_requests`];
    /// each is consumed during layout of the matching virtual list,
    /// where the live viewport rect and row-height cache let the
    /// resolver compute the target offset correctly even on first
    /// frame and for unmeasured `virtual_list_dyn` rows.
    pub(crate) pending_requests: Vec<crate::scroll::ScrollRequest>,
    /// "Pin currently engaged" bit for scroll containers built with
    /// [`crate::tree::El::pin_end`]. Keyed by `computed_id`. When set,
    /// the next layout pass snaps the stored offset to `max_offset`
    /// before clamping; a user scroll or programmatic offset write that
    /// lands away from the tail clears it. Entries persist across
    /// frames as long as the container stays mounted; switching a
    /// container away from `pin_end()` removes its entry.
    pub(crate) pin_active: FxHashMap<String, bool>,
    /// Previous-frame `max_offset` for pin-end containers, used to
    /// distinguish "user (or programmatic write) moved the offset off
    /// the tail" from "content grew past the previous tail while we
    /// were pinned." Lives outside [`Self::metrics`] because that map
    /// is rebuilt every layout pass.
    pub(crate) pin_prev_max: FxHashMap<String, f32>,
    /// LRU registry over the identity-keyed persistent maps above
    /// (`offsets`, `measured_row_heights`, `virtual_anchors`,
    /// `scroll_anchors`, `pin_active`, `pin_prev_max`): the last frame
    /// each scrollable / virtual-list identity was seen in the live
    /// tree. Entries deliberately persist after their node unmounts —
    /// that's what restores scroll position when keyed content
    /// re-enters the tree (tab switches, page navigation) — but the
    /// registry bounds the total: once it exceeds the cap, the
    /// longest-unseen absent identities are evicted from every map.
    /// Maintained by `UiState::gc_scroll_state` each frame.
    pub(crate) last_seen: FxHashMap<String, u64>,
    /// Frame counter for [`Self::last_seen`] stamps, advanced by
    /// `UiState::gc_scroll_state`.
    pub(crate) frame: u64,
}

/// Per-`viewport()`-node layout metrics, rewritten every layout pass
/// (per-frame scratch, like the scroll thumb maps). The input pass reads
/// `inner` to anchor cursor zoom and to clamp pan; `content` feeds
/// fit-to-content and pan clamping. Keyed by the viewport's `computed_id`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewportMetrics {
    /// Inner (post-padding) rect of the viewport in screen space — the
    /// window the content is panned/zoomed within, and the `origin`
    /// for the content↔screen transform.
    pub(crate) inner: Rect,
    /// Bounding box of the laid-out content in **content space**
    /// (pre-transform: the coordinates children occupy at pan `(0,0)`,
    /// zoom `1.0`). `None` when the viewport has no measurable content.
    pub(crate) content: Option<Rect>,
    /// The viewport's config (zoom range + pan trigger), copied here so
    /// the input pass can resolve gestures without walking the tree.
    pub(crate) cfg: crate::viewport::ViewportConfig,
}

/// Active pan drag for a [`viewport()`](crate::tree::viewport). Set by
/// `pointer_down` when the press matches the viewport's
/// [`PanTrigger`](crate::viewport::PanTrigger) over empty content,
/// consumed by `pointer_moved` to update the stored pan, cleared by
/// `pointer_up`. Like [`ThumbDrag`], it pre-empts normal hit-test so the
/// drag doesn't also fire app-level pointer events.
#[derive(Clone, Debug)]
pub(crate) struct ViewportPanDrag {
    /// `computed_id` of the viewport being panned.
    pub(crate) viewport_id: String,
    /// Screen point where the drag started.
    pub(crate) start_pointer: (f32, f32),
    /// The viewport's pan at the moment the drag started.
    pub(crate) start_pan: (f32, f32),
}

/// Persistent pan/zoom state for [`viewport()`](crate::tree::viewport)
/// containers, mirroring [`ScrollState`]: per-node views survive rebuilds
/// (keyed by `computed_id`, LRU-bounded) so a viewport restores its
/// framing when keyed content re-enters the tree, while `metrics` /
/// `pan_drag` are transient scratch.
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewportState {
    /// Pan/zoom per viewport node, keyed by `computed_id`. Read by the
    /// layout pass to bake the transform; written back after clamping
    /// and after any programmatic request resolves.
    pub(crate) views: FxHashMap<String, crate::viewport::ViewportView>,
    /// Per-viewport layout metrics (per-frame scratch).
    pub(crate) metrics: FxHashMap<String, ViewportMetrics>,
    /// Active pan drag, pre-empting hit-test while engaged.
    pub(crate) pan_drag: Option<ViewportPanDrag>,
    /// Programmatic [`ViewportRequest`](crate::viewport::ViewportRequest)s
    /// buffered between frames; each is consumed during layout of the
    /// matching viewport.
    pub(crate) pending_requests: Vec<crate::viewport::ViewportRequest>,
    /// LRU registry over `views`: the last frame each viewport identity
    /// was seen live. Maintained by `UiState::gc_viewport_state`.
    pub(crate) last_seen: FxHashMap<String, u64>,
    /// Frame counter for [`Self::last_seen`] stamps.
    pub(crate) frame: u64,
    /// Viewports whose view has been steered away from the home framing —
    /// by a user pan/zoom gesture, a `CenterOn` request, or an app-seeded
    /// view — since the last fit / reset. Read by
    /// [`FitPolicy::Contain`](crate::viewport::FitPolicy::Contain) to stop
    /// re-fitting once the user takes over, and surfaced to apps as
    /// `UiState::viewport_at_home`. Persistent alongside `views` (an
    /// entry only makes sense while its view survives); pruned by `gc`.
    pub(crate) taken_over: rustc_hash::FxHashSet<String>,
}

/// Per-frame resolved layout for one [`plot()`](crate::tree::plot) node:
/// the data rect (the inner area marks draw into, excluding axes/legend)
/// and the axis scales. Written by `draw_ops` when it resolves the plot,
/// read by the gesture router (to unproject the cursor) and the
/// [`plot_view_by_key`](crate::state::UiState::plot_view_by_key) readback.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlotMetrics {
    /// The data rect, in screen-space logical px.
    pub(crate) data_rect: crate::tree::Rect,
    /// Horizontal-axis scale — the warp the gesture router pans/zooms in.
    pub(crate) x_scale: crate::plot::Scale,
    /// Vertical-axis scale.
    pub(crate) y_scale: crate::plot::Scale,
    /// Whether this plot draws a crosshair — so a hover-move over it requests
    /// a redraw to track the cursor, even with no hover-identity change.
    pub(crate) crosshair: bool,
    /// The plot's pointer control scheme — what the primary drag does. Read by
    /// the gesture router on press.
    pub(crate) controls: crate::plot::PlotControls,
}

/// An in-flight plot pan drag, pre-empting hit-test while engaged (the plot
/// counterpart of [`ViewportPanDrag`]).
#[derive(Clone, Debug)]
pub(crate) struct PlotPanDrag {
    /// `computed_id` of the plot being panned.
    pub(crate) plot_id: String,
    /// Pointer position where the drag began.
    pub(crate) start_pointer: (f32, f32),
    /// The view at drag start; the live view is this panned by the cursor
    /// delta (so the drag stays absolute, never accumulating error).
    pub(crate) start_view: crate::plot::PlotView,
}

/// An in-flight directional box-zoom selection on a plot — the scientific
/// click-drag-to-zoom gesture. The release zooms whichever axis the drag
/// favoured (X when the horizontal delta dominates, else Y) to the data span
/// the drag swept; a sub-threshold drag is treated as a click.
#[derive(Clone, Debug)]
pub(crate) struct PlotZoomDrag {
    /// `computed_id` of the plot being zoomed.
    pub(crate) plot_id: String,
    /// Pointer position where the selection began (screen px).
    pub(crate) start_pointer: (f32, f32),
    /// Current pointer position (screen px), tracked as the drag moves.
    pub(crate) cur_pointer: (f32, f32),
}

/// Persistent pan/zoom view state for [`plot()`](crate::tree::plot) nodes,
/// mirroring [`ViewportState`]: per-node [`PlotView`](crate::plot::PlotView)s
/// survive rebuilds (keyed by `computed_id`, LRU-bounded) so a plot keeps
/// its framing when keyed content re-enters the tree, while `metrics` is
/// transient per-frame scratch.
#[derive(Clone, Debug, Default)]
pub(crate) struct PlotState {
    /// Per-plot pan/zoom view, keyed by `computed_id`. `None`-until-present:
    /// `draw_ops` seeds it by auto-fitting the data on first show.
    pub(crate) views: FxHashMap<String, crate::plot::PlotView>,
    /// Per-plot resolved layout (per-frame scratch).
    pub(crate) metrics: FxHashMap<String, PlotMetrics>,
    /// Active pan drag, pre-empting hit-test while engaged.
    pub(crate) pan_drag: Option<PlotPanDrag>,
    /// Active directional box-zoom selection, pre-empting hit-test while
    /// engaged.
    pub(crate) zoom_drag: Option<PlotZoomDrag>,
    /// Plots (by `computed_id`) whose value axis the user has taken manual
    /// control of by box-zooming Y, overriding `spec.y_autoscale` until a
    /// double-click reset. Empty for the common autoscaling case.
    pub(crate) y_manual: FxHashSet<String>,
    /// LRU registry over `views`: the last frame each plot identity was
    /// seen live. Maintained by `UiState::gc_plot_state`.
    pub(crate) last_seen: FxHashMap<String, u64>,
    /// Frame counter for [`Self::last_seen`] stamps.
    pub(crate) frame: u64,
}

/// Runtime queue for toast notifications. Apps provide fire-and-forget
/// [`crate::toast::ToastSpec`] values; the runtime stamps ids and
/// expiry deadlines here before [`crate::toast::synthesize_toasts`]
/// mirrors the queue into a synthetic overlay layer.
#[derive(Clone, Debug, Default)]
pub(crate) struct ToastState {
    pub(crate) queue: Vec<crate::toast::Toast>,
    pub(crate) next_id: u64,
}

/// Runtime hover timing for tooltips. The hovered target itself stays
/// in the general pointer interaction state; this bucket only tracks
/// tooltip-specific delay and per-hover dismissal.
#[derive(Clone, Debug, Default)]
pub(crate) struct TooltipState {
    /// When the current `hovered` target started being hovered. `None`
    /// when nothing is hovered or the pointer is outside the window.
    /// Used by [`crate::tooltip`] to gate the hover-delay timer.
    pub(crate) hover_started_at: Option<Instant>,
    /// True when the user pressed (or clicked) the hovered node during
    /// the current hover session. Suppresses the tooltip until the
    /// pointer leaves and re-enters, matching native behavior.
    pub(crate) dismissed_for_hover: bool,
}

/// Focus bookkeeping for runtime-managed popover layers. The active
/// focus target and tab order stay on `UiState`; this bucket only
/// tracks layer open/close transitions and saved focus restoration.
#[derive(Clone, Debug, Default)]
pub(crate) struct PopoverFocusState {
    /// LIFO of focus targets pushed when popover layers open. Each new
    /// `Kind::Custom("popover_layer")` snapshots the current focus
    /// here and auto-focuses into the layer; closing the layer pops and
    /// restores. See [`crate::focus::sync_popover_focus`].
    pub(crate) focus_stack: Vec<UiTarget>,
    /// `computed_id`s of every popover-layer node in the last laid-out
    /// tree, in tree order. Diffed against the new tree to detect open
    /// / close transitions.
    pub(crate) layer_ids: Vec<String>,
}
