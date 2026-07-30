//! `RunnerCore` — the backend-agnostic half of every Damascene runner.
//!
//! Holds interaction state ([`UiState`], `last_tree`) and paint scratch
//! buffers (`quad_scratch`, `runs`, `paint_items`) plus the geometry
//! context (`viewport_px`, `surface_size_override`) needed to project
//! layout's logical-pixel rects into physical-pixel scissors. Exposes
//! the identical interaction methods both backends ship: `pointer_*`,
//! `key_down`, `set_hotkeys`, `set_animation_mode`, `ui_state`,
//! `rect_of_key`, `debug_summary`, `set_surface_size`, plus the layout
//! / paint-stream stages that are pure CPU work.
//!
//! Each backend's `Runner` *contains* a `RunnerCore` and forwards the
//! interaction methods to it; only the GPU resources (pipelines,
//! buffers, atlases) and the actual GPU upload + draw work stay
//! per-backend. The split shares what's identical without a trait —
//! same shape as `crate::paint`, larger surface.
//!
//! ## What this module does NOT own
//!
//! - **Pipeline registration.** Each backend builds its own
//!   `pipelines: HashMap<ShaderHandle, BackendPipeline>` because the
//!   pipeline value type is GPU-specific.
//! - **Text upload.** Glyph atlas pages live on the GPU as backend
//!   images; the `TextPaint` that owns them is per-backend. Core
//!   reaches into it through the [`TextRecorder`] trait during the
//!   paint stream loop, then the backend flushes its atlas separately.
//! - **GPU upload of `quad_scratch` / frame uniforms.** Backend
//!   responsibility — `prepare()` orchestrates the full sequence.
//! - **`draw()`.** Both backends walk `core.paint_items` + `core.runs`
//!   themselves because the encoder type (and lifetime) diverges.
//!
//! ## Why no `Painter` trait
//!
//! Extracting a `trait Painter { fn prepare(...); fn draw(...); fn
//! set_scissor(...); }` was considered so backends would share *one*
//! abstraction surface. We declined: the only call sites left after
//! this module + [`crate::paint`] are the two
//! `prepare()` GPU-upload tails and the two `draw()` walks, and both
//! need backend-typed handles (`wgpu::RenderPass<'_>` /
//! `AutoCommandBufferBuilder<...>`) that no trait can hide without
//! generics that re-fragment the surface. A `Painter` trait would
//! reduce to a 1-method `set_scissor` indirection plus host-side
//! ceremony — dead weight. The duplication that *is* worth abstracting
//! is the host harness (winit init, swapchain management,
//! `damascene-{wgpu,vulkano}-demo::run`) — and that lives a layer above
//! the paint surface, not inside it. Revisit if a third backend lands
//! or if the GPU-upload sequences diverge enough to make a typed-state
//! interface earn its keep.

use std::cmp::Ordering;
use std::ops::Range;
use std::time::Duration;

use web_time::Instant;

use crate::color::ColorSpace;
use crate::draw_ops::{self, DrawOpsStats};
use crate::event::{
    KeyChord, KeyModifiers, LogicalKey, NamedKey, PhysicalKey, Pointer, PointerButton, PointerKind,
    UiEvent, UiEventKind, UiTarget,
};
use crate::focus;
use crate::hit_test;
use crate::ir::{DrawOp, TextAnchor};
use crate::layout;
use crate::paint::{
    DEFAULT_WORKING_COLOR_SPACE, InstanceRun, PaintItem, PhysicalScissor, QuadInstance, close_run,
    pack_instance_in, physical_scissor,
};
use crate::shader::ShaderHandle;
use crate::state::{
    AnimationMode, LONG_PRESS_DELAY, LatentViewportPan, MOUSE_DRAG_SLOP, SelectionDragGranularity,
    TOUCH_DRAG_THRESHOLD, TouchGestureState, UiState,
};
use crate::text::atlas::RunStyle;
use crate::text::metrics::TextLayoutCacheStats;
use crate::theme::Theme;
use crate::toast;
use crate::tooltip;
use crate::tree::{Color, El, FontWeight, Rect, TextWrap};

/// Logical-pixel overlap kept between the pre-page and post-page
/// viewport when the user clicks the scroll track above/below the
/// thumb. Matches browser convention: paging by `viewport_h - overlap`
/// preserves the bottom (resp. top) row across the jump so context
/// isn't lost.
const SCROLL_PAGE_OVERLAP: f32 = 24.0;

/// Reported back from each backend's `prepare(...)` per frame.
///
/// Two redraw deadlines:
///
/// - [`Self::next_layout_redraw_in`] — the next frame that needs a
///   full rebuild + layout pass. Driven by widget
///   [`crate::tree::El::redraw_within`] requests, animations still
///   settling, and pending tooltip / toast fades. The host must call
///   the backend's full `prepare(...)` (build → layout → paint →
///   render) when this elapses.
/// - [`Self::next_paint_redraw_in`] — the next frame a time-driven
///   shader needs but layout state is unchanged (e.g. spinner /
///   skeleton / progress-indeterminate / `samples_time=true` custom
///   shaders). The host can call the backend's lighter `repaint(...)`
///   path which reuses the cached `DrawOp` list, advances
///   `frame.time`, and skips rebuild + layout. Skipping the layout
///   path is only safe when no input has been processed since the
///   last full prepare; hosts must upgrade to the full path on any
///   input event.
///
/// Legacy aggregates [`Self::needs_redraw`] and [`Self::next_redraw_in`]
/// fold both lanes (OR / `min`) for hosts that don't want to split paths.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareResult {
    /// Legacy "any redraw needed?" — OR of `next_layout_redraw_in.is_some()`
    /// and `next_paint_redraw_in.is_some()`, plus animation-settling /
    /// tooltip-pending bools the runtime tracks internally.
    pub needs_redraw: bool,
    /// Legacy combined deadline — `min(next_layout_redraw_in,
    /// next_paint_redraw_in)`. Hosts that don't distinguish layout
    /// from paint-only redraws can keep reading this.
    pub next_redraw_in: Option<std::time::Duration>,
    /// Tightest deadline among signals that need a full rebuild +
    /// layout: widget `redraw_within`, animations still settling,
    /// tooltip / toast pending. `Some(ZERO)` for "now."
    pub next_layout_redraw_in: Option<std::time::Duration>,
    /// Tightest deadline among time-driven shaders. The host can
    /// service this with a paint-only frame (reuse cached ops, just
    /// advance `frame.time`). `Some(ZERO)` for "every frame" (the
    /// default for `is_continuous()` shaders today).
    pub next_paint_redraw_in: Option<std::time::Duration>,
    pub timings: PrepareTimings,
}

/// Outcome of a pointer-move dispatch through
/// [`RunnerCore::pointer_moved`] (or its backend wrappers).
///
/// Wayland and most X11 compositors deliver `CursorMoved` at very
/// high frequency while the cursor sits over the surface — even
/// sub-pixel jitter or per-frame compositor sync ticks count as
/// movement. The vast majority of those moves are visual no-ops
/// (the hovered node didn't change, no drag is active, no scrollbar
/// is dragging), so hosts must gate `request_redraw` on
/// `needs_redraw` to avoid spinning the rebuild + layout + render
/// pipeline on every cursor sample.
#[derive(Debug, Default)]
pub struct PointerMove {
    /// Events to dispatch through `App::on_event`. Empty when the
    /// move didn't trigger a `Drag` or selection update.
    pub events: Vec<UiEvent>,
    /// `true` when the runtime's visual state changed enough to
    /// warrant a redraw — hovered identity changed, scrollbar drag
    /// updated a scroll offset, or `events` is non-empty.
    pub needs_redraw: bool,
}

/// What [`RunnerCore::prepare_layout`] returns: the resolved
/// [`DrawOp`] list plus the redraw deadlines split into two lanes (see
/// [`PrepareResult`] for the lane semantics).
///
/// Wrapped in a struct so additions (new redraw signals, lane
/// metadata) don't churn every backend's `prepare` call site.
pub struct LayoutPrepared {
    pub ops: Vec<DrawOp>,
    pub needs_redraw: bool,
    pub next_layout_redraw_in: Option<std::time::Duration>,
    pub next_paint_redraw_in: Option<std::time::Duration>,
}

/// Per-stage CPU timing inside each backend's `prepare`. Cheap to
/// compute (a handful of `Instant::now()` calls per frame) and useful
/// for finding the dominant cost when frame budget is tight.
///
/// Stages:
/// - `layout`: layout pass + focus order sync + state apply + animation tick.
/// - `draw_ops`: tree → DrawOp[] resolution.
/// - `paint`: paint-stream loop (quad packing + text shaping via cosmic-text).
/// - `gpu_upload`: backend-side instance buffer write + atlas flush + frame uniforms.
/// - `snapshot`: cloning the laid-out tree for next-frame hit-testing.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareTimings {
    pub layout: Duration,
    pub layout_intrinsic_cache: layout::LayoutIntrinsicCacheStats,
    pub layout_prune: layout::LayoutPruneStats,
    pub draw_ops: Duration,
    pub draw_ops_culled_text_ops: u64,
    pub paint: Duration,
    pub paint_culled_ops: u64,
    pub gpu_upload: Duration,
    pub snapshot: Duration,
    pub text_layout_cache: TextLayoutCacheStats,
}

/// Backend-agnostic runner state.
///
/// Each backend's `Runner` owns one of these as its `core` field and
/// forwards the public interaction surface to it. The fields are `pub`
/// so backends can read them in `draw()` (which has to traverse
/// `paint_items` + `runs` against backend-specific pipeline and
/// instance-buffer objects).
pub struct RunnerCore {
    pub ui_state: UiState,
    /// Snapshot of the last laid-out tree, kept so pointer events
    /// arriving between frames hit-test against the geometry the user
    /// is actually looking at.
    pub last_tree: Option<El>,
    /// Whether [`Self::last_tree`] contains any text-link run, computed
    /// once per [`Self::snapshot`]. Gates the per-pointer-event
    /// [`hit_test::link_at`] tree walks so apps without links (most)
    /// don't pay a full-tree walk on every cursor move.
    last_tree_has_links: bool,

    /// Per-frame quad instance scratch — backends `bytemuck::cast_slice`
    /// this into their VBO upload.
    pub quad_scratch: Vec<QuadInstance>,
    pub runs: Vec<InstanceRun>,
    pub paint_items: Vec<PaintItem>,

    /// Cached [`DrawOp`] list, reused by [`Self::prepare_paint_cached`]
    /// for paint-only frames (time-driven shader animation when layout
    /// state is unchanged — only `frame.time` advances). Backends are
    /// expected to overwrite this with the ops returned from
    /// [`Self::prepare_layout`] once they're done with the frame's
    /// `prepare_paint` call.
    pub last_ops: Vec<DrawOp>,

    /// Physical viewport size in pixels. Backends use this for `draw()`
    /// scissor binding (logical scissors get projected into this space
    /// inside `prepare_paint`).
    pub viewport_px: (u32, u32),
    /// When set, overrides the physical viewport derived from
    /// `viewport.w * scale_factor` so paint-side scissor math matches
    /// the actual swapchain extent. Backends call
    /// [`Self::set_surface_size`] from their host's surface-config /
    /// resize hook to keep this in lockstep.
    pub surface_size_override: Option<(u32, u32)>,

    /// Theme used when resolving implicit widget surfaces to shaders.
    pub theme: Theme,

    /// The color space the renderer composites in. Every authored
    /// [`Color`](crate::color::Color) is converted into this space exactly
    /// once at the paint boundary. Defaults to
    /// [`DEFAULT_WORKING_COLOR_SPACE`] (sRGB-linear).
    ///
    /// This field governs the *quad* path ([`pack_instance_in`]) that all
    /// backends share. Backends are responsible for honoring it in their
    /// own text / icon / image color packing too — read it via
    /// [`Self::working_color_space`] and pass it to
    /// [`crate::paint::rgba_f32_in`]. The `damascene-wgpu` backend does this;
    /// `damascene-vulkano` and `damascene-ash` currently assume sRGB-linear and
    /// must be updated before driving a wide-gamut surface.
    pub working_color_space: ColorSpace,

    /// Introspected instance-attribute name maps per custom shader,
    /// registered by the backend's `register_shader_with` (issue #99).
    /// When present for a shader, the quad fold routes its uniforms by
    /// WGSL field name ([`crate::paint::pack_instance_named`]) instead
    /// of positional `vec_a`..`vec_e` only.
    shader_slot_maps: rustc_hash::FxHashMap<&'static str, crate::paint::slots::ShaderSlotMap>,
    /// `(shader, key)` pairs already warned about, so a standing
    /// Rust↔WGSL mismatch logs once instead of every frame.
    warned_shader_uniforms: rustc_hash::FxHashSet<(&'static str, &'static str)>,
}

impl Default for RunnerCore {
    fn default() -> Self {
        Self::new()
    }
}

impl RunnerCore {
    pub fn new() -> Self {
        Self {
            ui_state: UiState::default(),
            last_tree: None,
            last_tree_has_links: false,
            quad_scratch: Vec::new(),
            runs: Vec::new(),
            paint_items: Vec::new(),
            last_ops: Vec::new(),
            viewport_px: (1, 1),
            surface_size_override: None,
            theme: Theme::default(),
            working_color_space: DEFAULT_WORKING_COLOR_SPACE,
            shader_slot_maps: Default::default(),
            warned_shader_uniforms: Default::default(),
        }
    }

    /// Install the introspected instance-attribute name map for a
    /// custom shader. Backends call this from `register_shader_with`
    /// (after [`crate::paint::slots::introspect_wgsl`]); from then on
    /// the shader's uniforms route by WGSL field name and keys that
    /// match no declared attribute warn at first draw.
    pub fn register_shader_slots(
        &mut self,
        name: &'static str,
        map: crate::paint::slots::ShaderSlotMap,
    ) {
        self.shader_slot_maps.insert(name, map);
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The color space the renderer composites in. Backends read this to
    /// pack text / icon / image colors into the same working space the
    /// shared quad path uses.
    pub fn working_color_space(&self) -> ColorSpace {
        self.working_color_space
    }

    /// Set the working color space. Hosts call this after negotiating a
    /// surface format with the display server (see
    /// `damascene-winit-wgpu`). Backends must also refresh any recorder-local
    /// copy of the working space they hold.
    pub fn set_working_color_space(&mut self, space: ColorSpace) {
        self.working_color_space = space;
    }

    /// Override the physical viewport size. Call after the host's
    /// surface configure or resize so scissor math sees the swapchain's
    /// real extent (fractional `scale_factor` round-trips can otherwise
    /// land `viewport_px` one pixel off and trip
    /// `set_scissor_rect` validation).
    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.surface_size_override = Some((width.max(1), height.max(1)));
    }

    pub fn ui_state(&self) -> &UiState {
        &self.ui_state
    }

    pub fn debug_summary(&self) -> String {
        self.ui_state.debug_summary()
    }

    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.ui_state.rect_of_key(key)
    }

    /// Whether a primary press at `(x, y)` (logical pixels) would
    /// land on a node that opted into [`crate::tree::El::capture_keys`]
    /// — the marker the library uses to identify text-input-style
    /// widgets that consume raw key events when focused.
    ///
    /// Hosts use this to make focus-driven side-effect decisions in
    /// the user-gesture context of a DOM pointerdown listener before
    /// the press is actually dispatched. The most common use is the
    /// web host's soft-keyboard plumbing: a hidden textarea must be
    /// focused synchronously inside the pointerdown handler for iOS
    /// to summon the on-screen keyboard, but only when the tap will
    /// actually focus an Damascene text input. Pure read — does not
    /// mutate any state.
    ///
    /// Returns `false` when the press misses every hit-test target
    /// or the laid-out tree is not yet available.
    pub fn would_press_focus_text_input(&self, x: f32, y: f32) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        let Some(target) = hit_test::hit_test_target(tree, &self.ui_state, (x, y)) else {
            return false;
        };
        find_capture_keys(tree, &target.node_id).unwrap_or(false)
    }

    // ---- Input plumbing ----

    /// Pointer moved to `p.x, p.y` (logical px). Updates the hovered
    /// node (readable via `ui_state().hovered`) and, if the primary
    /// button is currently held, returns a `Drag` event routed to the
    /// originally pressed target. The event's `modifiers` field
    /// reflects the mask currently tracked on `UiState` (set by the
    /// host via `set_modifiers`).
    ///
    /// `p.button` is ignored — pointer move events do not carry a
    /// button press. `p.kind` is recorded on emitted events as
    /// [`UiEvent::pointer_kind`] so apps can specialize for touch
    /// vs. mouse / pen.
    pub fn pointer_moved(&mut self, p: Pointer) -> PointerMove {
        let Pointer { x, y, kind, .. } = p;
        // Previous position, before we overwrite it — used to clear a scene
        // hover tooltip on the move that leaves the scene (scenes aren't
        // hover hit-targets, so `hover_changed` may not fire on exit).
        let prev_pos = self.ui_state.pointer_pos;
        self.ui_state.pointer_pos = Some((x, y));
        self.ui_state.pointer_kind = kind;

        // Active scrollbar drag: translate cursor delta into
        // `scroll.offsets` updates. The drag is captured at
        // `pointer_down` so we can map directly onto the scroll
        // container without going through hit-test, and we suppress
        // the normal hover/Drag event emission while it's in flight.
        if let Some(drag) = self.ui_state.scroll.thumb_drag.clone() {
            let dy = y - drag.start_pointer_y;
            let new_offset = if drag.track_remaining > 0.0 {
                drag.start_offset + dy * (drag.max_offset / drag.track_remaining)
            } else {
                drag.start_offset
            };
            let clamped = new_offset.clamp(0.0, drag.max_offset);
            let prev = self.ui_state.scroll.offsets.insert(drag.scroll_id, clamped);
            let changed = prev.is_none_or(|old| (old - clamped).abs() > f32::EPSILON);
            return PointerMove {
                events: Vec::new(),
                needs_redraw: changed,
            };
        }

        // Active edge-resize drag (issue #106): translate the pointer
        // delta along the resize axis into a size override, exactly as
        // the thumb drag translates into a scroll offset. Captured at
        // `pointer_down`, suppresses hover/Drag emission while in
        // flight; the next layout reads the override back as `Fixed`.
        if let Some(drag) = self.ui_state.resize.drag.clone() {
            let pos = match drag.axis {
                crate::tree::Axis::Column => y,
                _ => x,
            };
            let next = (drag.initial + (pos - drag.anchor) * drag.sign).clamp(drag.min, drag.max);
            let prev = self.ui_state.resize.overrides.insert(drag.id, next);
            let changed = prev.is_none_or(|old| (old - next).abs() > f32::EPSILON);
            return PointerMove {
                events: Vec::new(),
                needs_redraw: changed,
            };
        }

        // Active camera drag: orbit/pan the captured scene. Like the
        // scrollbar drag, this is global once captured and suppresses
        // hover/Drag emission while in flight.
        if self.ui_state.camera_drag_active() {
            let changed = self.ui_state.drag_camera_to(x, y);
            return PointerMove {
                events: Vec::new(),
                needs_redraw: changed,
            };
        }

        // Active viewport pan: translate the cursor delta into the
        // viewport's pan. Global once captured, like the scrollbar /
        // camera drags; suppresses hover/Drag emission while in flight.
        if self.ui_state.viewport_pan_active() {
            let changed = self.ui_state.drag_viewport_to(x, y);
            return PointerMove {
                events: Vec::new(),
                needs_redraw: changed,
            };
        }

        // Active plot pan: same discipline as the viewport pan above.
        if self.ui_state.plot_pan_active() {
            let changed = self.ui_state.drag_plot_to(x, y);
            return PointerMove {
                events: Vec::new(),
                needs_redraw: changed,
            };
        }

        // Active plot box-zoom selection: track the cursor so the selection
        // band redraws; the zoom itself applies on release.
        if self.ui_state.plot_zoom_active() {
            let changed = self.ui_state.drag_plot_zoom_to(x, y);
            return PointerMove {
                events: Vec::new(),
                needs_redraw: changed,
            };
        }

        // Latent viewport pan → real pan: the press was captured toward
        // a click on a keyed child (see `pointer_down`), and the pointer
        // has now travelled past the mouse slop while held — the gesture
        // is a drag, not a click. Cancel the pending click the same way
        // a touch drift commits to scroll (`PointerCancel` to the
        // pressed target, `PointerLeave` to the hovered one), then begin
        // the pan anchored at the original press point so the sub-slop
        // motion isn't lost. Below the slop the move falls through and
        // behaves exactly as before (Drag emission, hover, styling).
        if let Some(latent) = self.ui_state.viewport.latent_pan.clone() {
            let (dx, dy) = (x - latent.press.0, y - latent.press.1);
            if (dx * dx + dy * dy).sqrt() >= MOUSE_DRAG_SLOP {
                self.ui_state.viewport.latent_pan = None;
                let modifiers = self.ui_state.modifiers;
                let mut out = Vec::new();
                self.cancel_press_for_scroll(&mut out, x, y, kind, modifiers);
                // A press that became a drag can't seed a multi-click:
                // MULTI_CLICK_DIST is as wide as the slop, so without
                // this reset a re-press near the original point would
                // read as a double-click across the pan.
                self.ui_state.click.last = None;
                self.ui_state.begin_viewport_pan(
                    latent.viewport_id,
                    PointerButton::Primary,
                    latent.press.0,
                    latent.press.1,
                );
                self.ui_state.drag_viewport_to(x, y);
                // Redraw unconditionally: even a bounds-clamped pan must
                // repaint to clear the cancelled press styling.
                return PointerMove {
                    events: out,
                    needs_redraw: true,
                };
            }
        }

        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        // Stash the previous hover target so we can pair Leave/Enter
        // events on identity change. `set_hovered` mutates the state
        // and only returns whether identity flipped.
        let prev_hover = self.ui_state.hovered.clone();
        let hover_changed = self.ui_state.set_hovered(hit, Instant::now());
        // Track the link URL under the pointer separately from keyed
        // hover so the cursor resolver can flip to `Pointer` over text
        // runs that aren't themselves hit-test targets. A change here
        // (entering or leaving a link) needs a redraw so the host's
        // per-frame cursor resolution reads the new value.
        let prev_hovered_link = self.ui_state.hovered_link.clone();
        let new_hovered_link = self
            .linkable_tree()
            .and_then(|t| hit_test::link_at(t, (x, y)));
        let link_hover_changed = new_hovered_link != prev_hovered_link;
        self.ui_state.hovered_link = new_hovered_link;
        // Same tracking for edge-resize grab bands: crossing a band
        // edge changes the resolved cursor (resize arrows), and the
        // host only re-resolves the cursor after a redraw — so the
        // transition itself must request one even though no hover
        // identity changed (the band straddles inside a single node).
        let prev_hovered_band = self.ui_state.resize.hovered_band.clone();
        let new_hovered_band = self.ui_state.resize_band_at(x, y).map(|b| b.id.clone());
        let band_hover_changed = new_hovered_band != prev_hovered_band;
        self.ui_state.resize.hovered_band = new_hovered_band;
        let modifiers = self.ui_state.modifiers;

        let mut out = Vec::new();

        // Hover-transition events: Leave on the prior target (when
        // there was one), Enter on the new target (when there is one).
        // Both fire on identity change only — cursor moves *within* the
        // same hovered node are visual no-ops here, matching the
        // redraw-debouncing semantics. Always Leave-then-Enter so apps
        // observe the cleared state before the new one.
        //
        // Touch gating: a touchscreen has no resting hover. Without a
        // press, a stray pointermove (very rare on touch — most
        // platforms only fire pointermove during contact) should not
        // synthesize a hover transition. With a press, hover identity
        // changes during a drag are real and fire normally so widgets
        // along the drag path can react. `pointer_down` and
        // `pointer_up` separately stamp the contact-driven enter and
        // leave for touch.
        let touch_no_press = matches!(kind, PointerKind::Touch) && self.ui_state.pressed.is_none();
        if hover_changed && !touch_no_press {
            if let Some(prev) = prev_hover {
                out.push(UiEvent {
                    key: Some(prev.key.clone()),
                    target: Some(prev),
                    pointer: Some((x, y)),
                    key_press: None,
                    text: None,
                    selection: None,
                    modifiers,
                    click_count: 0,
                    path: None,
                    pointer_kind: Some(kind),
                    wheel_delta: None,
                    kind: UiEventKind::PointerLeave,
                });
            }
            if let Some(new) = self.ui_state.hovered.clone() {
                out.push(UiEvent {
                    key: Some(new.key.clone()),
                    target: Some(new),
                    pointer: Some((x, y)),
                    key_press: None,
                    text: None,
                    selection: None,
                    modifiers,
                    click_count: 0,
                    path: None,
                    pointer_kind: Some(kind),
                    wheel_delta: None,
                    kind: UiEventKind::PointerEnter,
                });
            }
        }

        // Touch gesture state machine: resolve the tap / drag / scroll
        // ambiguity before falling through to selection / drag
        // emission. Mouse and pen pointers stay at `None` here and
        // bypass the machine entirely.
        if matches!(kind, PointerKind::Touch) {
            match self.ui_state.touch_gesture.clone() {
                TouchGestureState::Pending {
                    initial,
                    consumes_drag,
                    started_at,
                } => {
                    let dx = x - initial.0;
                    let dy = y - initial.1;
                    if (dx * dx + dy * dy).sqrt() < TOUCH_DRAG_THRESHOLD {
                        // Below threshold — could still be a tap.
                        // Suppress selection / drag emission for this
                        // move; return only any hover events that
                        // already accumulated.
                        let needs_redraw = hover_changed
                            || link_hover_changed
                            || band_hover_changed
                            || !out.is_empty();
                        return PointerMove {
                            events: out,
                            needs_redraw,
                        };
                    }
                    if consumes_drag {
                        // The press target opted in via
                        // `consumes_touch_drag` — commit to drag and
                        // fall through to the normal drag emission
                        // below (this move and subsequent ones).
                        self.ui_state.touch_gesture = TouchGestureState::None;
                    } else {
                        // Commit to scroll — or to a viewport pan.
                        // Cancel the press either way so the widget
                        // that thought it was being clicked sees
                        // `PointerCancel` + `PointerLeave` and stops
                        // receiving further events for this gesture.
                        let now = Instant::now();
                        // The press target, before the cancel below
                        // consumes it — the pan takeover must know
                        // whether the contact began on the viewport's
                        // own content or on a widget floated over it.
                        let press_target = self.ui_state.pressed.clone();
                        self.cancel_press_for_scroll(&mut out, x, y, kind, modifiers);
                        // Sign: a finger dragging *down* should expose
                        // content above (scroll position decreases).
                        // `pointer_wheel`'s `dy` matches mouse-wheel
                        // convention where positive = scroll-down, so
                        // we negate the finger's positive Δy.
                        let scroll_dy = initial.1 - y;
                        // A pannable viewport under the contact takes
                        // the drag (issue #126 — touch analog of the
                        // mouse latent pan) unless a scrollable *inside*
                        // it consumes the motion first: lists laid over
                        // a map scroll, the map pans from anywhere
                        // else, and any *outer* page scroll loses to
                        // the viewport (the embedded-map convention).
                        // Touch contacts arrive as the primary button,
                        // so dedicated-trigger viewports keep today's
                        // scroll routing; so does an overlay-occluded
                        // press (same rule as the mouse paths, #127),
                        // and so does a press whose target lives in a
                        // layer floated *over* the viewport (a toast
                        // card, an overlay list) rather than inside it
                        // — the same subtree rule the mouse latent pan
                        // applies at arming.
                        let pan_vp =
                            self.ui_state
                                .viewport_at(initial.0, initial.1)
                                .filter(|(id, cfg)| {
                                    cfg.pan_trigger.matches(PointerButton::Primary, modifiers)
                                        && self.last_tree.as_ref().is_none_or(|t| {
                                            !hit_test::occluded_by_overlay(t, initial, id)
                                        })
                                        && press_target.as_ref().is_none_or(|p| {
                                            self.last_tree.as_ref().is_some_and(|t| {
                                                find_inside_subtree(t, id, &p.node_id, false)
                                                    .unwrap_or(false)
                                            })
                                        })
                                });
                        if let Some((vp_id, _)) = pan_vp {
                            // Axis dominance (issue #130): the inner
                            // scrollable is offered the gesture only
                            // when the initial motion is vertical-
                            // dominant. Scrollables on this path move
                            // vertically only, so a mostly-horizontal
                            // swipe has nothing a list can consume —
                            // without this gate, a few pixels of
                            // vertical drift would commit the whole
                            // gesture to jittering the list instead of
                            // panning the viewport. Ties go to the
                            // scrollable, matching the platforms'
                            // scroll bias. (If horizontal scrollables
                            // ever grow on this path, this should ask
                            // the scrollable for its movable axes
                            // rather than assume vertical.)
                            let vertical_dominant = dy.abs() >= dx.abs();
                            // Innermost scrollable descendant of the
                            // viewport that can consume this delta, if
                            // any (targets come back outer→inner).
                            let mut inner_step = None;
                            if vertical_dominant && let Some(tree) = self.last_tree.as_ref() {
                                for id in
                                    hit_test::scroll_targets_at(tree, initial).into_iter().rev()
                                {
                                    if find_inside_subtree(tree, &vp_id, &id, false)
                                        .unwrap_or(false)
                                        && let Some(step) =
                                            self.ui_state.scroll_by_id(&id, scroll_dy)
                                    {
                                        inner_step = Some(step);
                                        break;
                                    }
                                }
                            }
                            if let Some(step) = inner_step {
                                let dt = now
                                    .duration_since(started_at)
                                    .as_secs_f32()
                                    .max(1.0 / 120.0);
                                self.ui_state.touch_gesture = TouchGestureState::Scrolling {
                                    last_pos: (x, y),
                                    last_time: now,
                                    velocity: step.applied_delta / dt,
                                    scroll_id: Some(step.scroll_id),
                                };
                            } else {
                                // Pan, anchored at the initial contact
                                // so the pre-threshold motion isn't
                                // lost. `pan_drag` pre-empts everything
                                // from here (moves early-return on it;
                                // release ends it in `pointer_up`), so
                                // the gesture machine stands down.
                                self.ui_state.touch_gesture = TouchGestureState::None;
                                self.ui_state.begin_viewport_pan(
                                    vp_id,
                                    PointerButton::Primary,
                                    initial.0,
                                    initial.1,
                                );
                                self.ui_state.drag_viewport_to(x, y);
                            }
                            return PointerMove {
                                events: out,
                                needs_redraw: true,
                            };
                        }
                        let step = self.last_tree.as_ref().and_then(|tree| {
                            self.ui_state.scroll_by_pointer(tree, initial, scroll_dy)
                        });
                        let dt = now
                            .duration_since(started_at)
                            .as_secs_f32()
                            .max(1.0 / 120.0);
                        let velocity = step.as_ref().map(|s| s.applied_delta / dt).unwrap_or(0.0);
                        self.ui_state.touch_gesture = TouchGestureState::Scrolling {
                            last_pos: (x, y),
                            last_time: now,
                            velocity,
                            scroll_id: step.map(|s| s.scroll_id),
                        };
                        return PointerMove {
                            events: out,
                            needs_redraw: true,
                        };
                    }
                }
                TouchGestureState::Scrolling {
                    last_pos,
                    last_time,
                    velocity,
                    scroll_id,
                } => {
                    let now = Instant::now();
                    let scroll_dy = last_pos.1 - y;
                    let step = scroll_id
                        .as_ref()
                        .and_then(|id| self.ui_state.scroll_by_id(id, scroll_dy))
                        .or_else(|| {
                            self.last_tree.as_ref().and_then(|tree| {
                                self.ui_state.scroll_by_pointer(tree, (x, y), scroll_dy)
                            })
                        });
                    let dt = now.duration_since(last_time).as_secs_f32().max(1.0 / 240.0);
                    let sample_velocity =
                        step.as_ref().map(|s| s.applied_delta / dt).unwrap_or(0.0);
                    let velocity = sample_velocity * 0.65 + velocity * 0.35;
                    self.ui_state.touch_gesture = TouchGestureState::Scrolling {
                        last_pos: (x, y),
                        last_time: now,
                        velocity,
                        scroll_id: step.map(|s| s.scroll_id).or(scroll_id),
                    };
                    return PointerMove {
                        events: out,
                        needs_redraw: true,
                    };
                }
                TouchGestureState::None => {
                    // Already committed to drag (or there was no press
                    // to gate). Fall through.
                }
                TouchGestureState::LongPressed => {
                    // The long-press already fired. For static text it
                    // may have started a runtime-owned selection drag;
                    // for editable text it leaves the original press
                    // captured so movement can emit Drag to the input.
                    self.extend_selection_drag_at(x, y, kind, modifiers, &mut out);
                    if self.ui_state.pressed.is_none() {
                        let needs_redraw = hover_changed
                            || link_hover_changed
                            || band_hover_changed
                            || !out.is_empty();
                        return PointerMove {
                            events: out,
                            needs_redraw,
                        };
                    }
                }
            }
        }

        // Selection drag-extend takes precedence over the focusable
        // Drag emission. Cross-leaf: if the pointer hits a selectable
        // leaf, head migrates there. Otherwise we project the pointer
        // onto the closest selectable leaf in document order so that
        // dragging *past* the last leaf extends to its end (rather
        // than snapping the head home to the anchor leaf).
        self.extend_selection_drag_at(x, y, kind, modifiers, &mut out);

        // Drag: pointer moved while primary button is down → emit Drag
        // to the originally pressed target. Cursor escape from the
        // pressed node is the *normal* drag-extend case (e.g. text
        // selection inside an editable widget); we keep emitting until
        // pointer_up clears `pressed`.
        if let Some(p) = self.ui_state.pressed.clone() {
            // Caret-blink reset: drag-selecting inside a text input
            // is ongoing editing activity, so keep the caret solid
            // for the duration of the drag.
            if self.focused_captures_keys() {
                self.ui_state.bump_caret_activity(Instant::now());
            }
            out.push(UiEvent {
                key: Some(p.key.clone()),
                target: Some(p),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: self.ui_state.current_click_count(),
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::Drag,
            });
        }

        // Scenes with hover tooltips redraw on every move over them (and on
        // the move that leaves) so the tooltip tracks / clears — a move
        // within a single node otherwise reports no change.
        let over_hover_scene = self.ui_state.pointer_over_hover_scene(x, y)
            || prev_pos.is_some_and(|(px, py)| self.ui_state.pointer_over_hover_scene(px, py));
        // Plots with a crosshair likewise redraw on every move over them (and
        // on the move that leaves) so the crosshair tracks / clears.
        let over_crosshair_plot = self.ui_state.pointer_over_crosshair_plot(x, y)
            || prev_pos.is_some_and(|(px, py)| self.ui_state.pointer_over_crosshair_plot(px, py));
        let needs_redraw = hover_changed
            || link_hover_changed
            || band_hover_changed
            || !out.is_empty()
            || over_hover_scene
            || over_crosshair_plot;
        PointerMove {
            events: out,
            needs_redraw,
        }
    }

    /// Pointer left the window — clear hover / press trackers.
    /// Returns a `PointerLeave` event for the previously hovered
    /// target (when there was one) so apps can run hover-leave side
    /// effects symmetrically with `PointerEnter`. Cursor positions on
    /// the leave event are the last known pointer position before the
    /// pointer exited, since winit no longer reports coordinates once
    /// the cursor is outside the window.
    pub fn pointer_left(&mut self) -> Vec<UiEvent> {
        let last_pos = self.ui_state.pointer_pos;
        let prev_hover = self.ui_state.hovered.clone();
        let modifiers = self.ui_state.modifiers;
        // pointer_left is a mouse-only signal — touch has no "cursor
        // outside the window" state. Tag the leave event with the
        // last-known modality so apps that branch on touch don't see
        // a phantom Mouse-tagged leave for what was a touch session.
        let kind = self.ui_state.pointer_kind;
        self.ui_state.pointer_pos = None;
        self.ui_state.set_hovered(None, Instant::now());
        self.ui_state.pressed = None;
        self.ui_state.pressed_secondary = None;
        self.ui_state.touch_gesture = TouchGestureState::None;
        // The press the latent pan was arbitrating is gone with it.
        self.ui_state.viewport.latent_pan = None;
        self.ui_state.cancel_scroll_momentum();
        // Pointer leaves the window → no link is hovered or pressed
        // anymore. Clearing here keeps a stale `Pointer` cursor from
        // sticking after the user moves the mouse out of the canvas
        // and lets re-entry recompute against the actual current
        // position.
        self.ui_state.hovered_link = None;
        self.ui_state.pressed_link = None;
        // Resize-band hover clears with the pointer; an in-flight edge
        // drag is abandoned at whatever size it reached (the override
        // is already stored), mirroring the press cleanup above.
        self.ui_state.resize.hovered_band = None;
        self.ui_state.resize.drag = None;

        let mut out = Vec::new();
        if let Some(prev) = prev_hover {
            out.push(UiEvent {
                key: Some(prev.key.clone()),
                target: Some(prev),
                pointer: last_pos,
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::PointerLeave,
            });
        }
        out
    }

    /// The platform cancelled the active pointer sequence — a touch
    /// contact stolen by a system gesture, a pen palm-rejection, a
    /// `pointercancel` on web. Abandons every in-flight gesture without
    /// synthesizing clicks or applying release effects: the pressed
    /// target sees `PointerCancel`, the hovered target `PointerLeave`,
    /// and all captures release where they are — pans and camera drags
    /// stay at the pose they reached, a box-zoom selection is discarded
    /// (never applied), an edge-resize keeps its stored override.
    ///
    /// Hosts route `TouchPhase::Cancelled` / `pointercancel` here rather
    /// than synthesizing a `pointer_up` (which would apply release
    /// semantics like the box-zoom, and only ends the matching button's
    /// capture) or a `pointer_left` (which leaves gesture captures alive
    /// for the next contact to inherit).
    pub fn pointer_cancelled(&mut self) -> Vec<UiEvent> {
        let (x, y) = self.ui_state.pointer_pos.unwrap_or_default();
        let kind = self.ui_state.pointer_kind;
        let modifiers = self.ui_state.modifiers;
        let mut out = Vec::new();
        // PointerCancel to the pressed target, PointerLeave to the
        // hovered one; clears pressed / links / selection drag.
        self.cancel_press_for_scroll(&mut out, x, y, kind, modifiers);
        self.ui_state.viewport.latent_pan = None;
        self.ui_state.viewport.pan_drag = None;
        self.ui_state.cancel_camera_drag();
        self.ui_state.cancel_plot_gestures();
        self.ui_state.scroll.thumb_drag = None;
        self.ui_state.resize.drag = None;
        self.ui_state.resize.hovered_band = None;
        self.ui_state.touch_gesture = TouchGestureState::None;
        self.ui_state.cancel_scroll_momentum();
        self.ui_state.pointer_pos = None;
        out
    }

    /// A file is being dragged over the window at logical-pixel
    /// coordinates `(x, y)`. Hosts call this from
    /// `WindowEvent::HoveredFile`. Hit-tests at the cursor position and
    /// emits a `FileHovered` event routed to the keyed leaf at that
    /// point (or window-level when the cursor is over no keyed
    /// surface). Multi-file drags fire one event per file — winit
    /// reports each file separately and the host forwards each call
    /// into this method.
    ///
    /// The hover state is *not* tracked across files; apps that want
    /// to count active hovered files do so themselves between
    /// `FileHovered` and the eventual `FileHoverCancelled` /
    /// `FileDropped`.
    pub fn file_hovered(&mut self, path: std::path::PathBuf, x: f32, y: f32) -> Vec<UiEvent> {
        self.ui_state.pointer_pos = Some((x, y));
        let target = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        let key = target.as_ref().map(|t| t.key.clone());
        vec![UiEvent {
            key,
            target,
            pointer: Some((x, y)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: self.ui_state.modifiers,
            click_count: 0,
            path: Some(path),
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::FileHovered,
        }]
    }

    /// The user moved a hovered file off the window without dropping
    /// (or pressed Escape). Window-level event — not routed to any
    /// keyed leaf, since winit doesn't tell us which file was being
    /// dragged. Apps clear any drop-zone affordance state.
    pub fn file_hover_cancelled(&mut self) -> Vec<UiEvent> {
        vec![UiEvent {
            key: None,
            target: None,
            pointer: self.ui_state.pointer_pos,
            key_press: None,
            text: None,
            selection: None,
            modifiers: self.ui_state.modifiers,
            click_count: 0,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::FileHoverCancelled,
        }]
    }

    /// A file was dropped on the window at logical-pixel coordinates
    /// `(x, y)`. Hosts call this from `WindowEvent::DroppedFile`.
    /// Same routing as [`Self::file_hovered`] — keyed leaf at the drop
    /// point, or window-level. One event per file.
    pub fn file_dropped(&mut self, path: std::path::PathBuf, x: f32, y: f32) -> Vec<UiEvent> {
        self.ui_state.pointer_pos = Some((x, y));
        let target = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        let key = target.as_ref().map(|t| t.key.clone());
        vec![UiEvent {
            key,
            target,
            pointer: Some((x, y)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: self.ui_state.modifiers,
            click_count: 0,
            path: Some(path),
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::FileDropped,
        }]
    }

    /// Primary/secondary/middle pointer button pressed at `(x, y)`.
    /// For the primary button, focuses the hit target and stashes it
    /// as the pressed target; emits a `PointerDown` event so widgets
    /// like text_input can react at down-time (e.g., set the selection
    /// anchor before any drag extends it). Secondary/middle store on a
    /// separate channel and never emit a `PointerDown`.
    ///
    /// Also drives the library's text-selection manager: a primary
    /// press on a `selectable` text leaf starts a drag and produces a
    /// `SelectionChanged` event; a press on any other element clears
    /// any active static-text selection by emitting a
    /// `SelectionChanged` with an empty range.
    pub fn pointer_down(&mut self, p: Pointer) -> Vec<UiEvent> {
        let Pointer {
            x, y, button, kind, ..
        } = p;
        self.ui_state.pointer_kind = kind;
        self.ui_state.cancel_scroll_momentum();
        // Any new press invalidates a pending click-vs-pan decision from
        // an earlier one (re-armed below if this press qualifies).
        self.ui_state.viewport.latent_pan = None;
        // Scrollbar track pre-empts normal hit-test: a primary press
        // inside a scrollable's track column either captures a thumb
        // drag (when the press lands inside the visible thumb rect)
        // or pages the scroll offset by a viewport (when it lands
        // above or below the thumb). Both branches suppress focus /
        // press / event chains for the press itself; `pointer_moved`
        // then drives the drag (no-op for paged clicks) and
        // `pointer_up` clears the drag.
        if matches!(button, PointerButton::Primary)
            && let Some((scroll_id, _track, thumb_rect)) = self
                .ui_state
                .thumb_at(x, y)
                .filter(|(scroll_id, _, _)| self.scrollbar_can_capture(scroll_id, x, y))
        {
            let metrics = self
                .ui_state
                .scroll
                .metrics
                .get(&scroll_id)
                .copied()
                .unwrap_or_default();
            let start_offset = self
                .ui_state
                .scroll
                .offsets
                .get(&scroll_id)
                .copied()
                .unwrap_or(0.0);

            // Grab when the press lands inside the visible thumb;
            // page otherwise. The track is wider than the thumb
            // horizontally, so this branch is decided by `y` alone.
            let grabbed = y >= thumb_rect.y && y <= thumb_rect.y + thumb_rect.h;
            if grabbed {
                let track_remaining = (metrics.viewport_h - thumb_rect.h).max(0.0);
                self.ui_state.scroll.thumb_drag = Some(crate::state::ThumbDrag {
                    scroll_id,
                    start_pointer_y: y,
                    start_offset,
                    track_remaining,
                    max_offset: metrics.max_offset,
                });
            } else {
                // Click-to-page. Browser convention: each press
                // shifts the offset by ~one viewport with a small
                // overlap so context isn't lost. Direction is
                // decided by which side of the thumb the press
                // landed on.
                let page = (metrics.viewport_h - SCROLL_PAGE_OVERLAP).max(0.0);
                let delta = if y < thumb_rect.y { -page } else { page };
                let new_offset = (start_offset + delta).clamp(0.0, metrics.max_offset);
                self.ui_state.scroll.offsets.insert(scroll_id, new_offset);
            }
            return Vec::new();
        }

        // Edge-resize grab band (issue #106): a primary press inside a
        // `.user_resizable()` pane's seam band captures a resize drag.
        // Same pre-emption slot as the scrollbar thumb (which wins
        // where the two overlap — it's painted on top): suppresses
        // focus / press / event chains for the press itself;
        // `pointer_moved` drives the drag, `pointer_up` releases it.
        if matches!(button, PointerButton::Primary)
            && let Some(band) = self
                .ui_state
                .resize_band_at(x, y)
                .filter(|b| self.resize_band_can_capture(b, x, y))
                .cloned()
        {
            let anchor = match band.axis {
                crate::tree::Axis::Column => y,
                _ => x,
            };
            self.ui_state.resize.drag = Some(crate::state::resize::EdgeResizeDrag {
                id: band.id,
                key: band.key,
                axis: band.axis,
                sign: band.sign,
                anchor,
                initial: band.current,
                min: band.min,
                max: band.max,
            });
            return Vec::new();
        }

        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));

        // Camera gesture: a press over a 3D scene viewport — and not on a
        // more-specific interactive target laid over it — may capture an
        // orbit/pan/dolly, per the scene's navigation scheme. Like the
        // scrollbar drag it suppresses focus/press for the press itself;
        // `pointer_moved` drives it and `pointer_up` clears it.
        //
        // The hit may be nothing, or the scene's own keyed node: a scene
        // given `.key(...)` becomes a hit-test target, but that key is the
        // scene itself, so it must not suppress its own camera drag. Any
        // *other* hit (a real widget layered over the scene) still does.
        // An overlay's *dead space* hits nothing, though, so it needs the
        // explicit occlusion check the wheel path uses (issue #127): a
        // press on a modal floated over the scene must not orbit the
        // canvas underneath it.
        if let Some(id) = self.ui_state.scene_at(x, y)
            && hit.as_ref().is_none_or(|h| *h.node_id == *id)
            && let Some(mode) = self
                .ui_state
                .scene_drag_mode(&id, button, self.ui_state.modifiers)
            && self
                .last_tree
                .as_ref()
                .is_none_or(|t| !hit_test::occluded_by_overlay(t, (x, y), &id))
        {
            self.ui_state.begin_camera_drag(id, mode, button, x, y);
            return Vec::new();
        }

        // Viewport pan: a press over a `viewport()` whose pan trigger
        // matches the button + modifiers begins a pan. Pre-empts focus /
        // press like the other captures.
        //
        // A *dedicated* trigger — any non-primary button or a modifier
        // mask — can't collide with a plain content click, so it pans
        // anywhere over the viewport (even over a keyed child). The
        // *default* trigger (plain primary, no modifiers) would steal
        // every child click, so it only pans on the viewport background:
        // the press must hit nothing or the viewport's own node, the same
        // "not on a more-specific target" rule the camera drag uses. This
        // is what keeps content clickable while empty space drags it. A
        // default-trigger press on a keyed child isn't dead, though — it
        // arms a latent pan at the end of this method, converted past
        // the mouse slop in `pointer_moved` (issue #125).
        //
        // Overlay dead space hits nothing too, so like the camera and
        // wheel paths this needs the explicit occlusion check (issue
        // #127): a press-drag on a modal floated over the canvas must
        // not pan the viewport underneath it.
        if let Some((vp_id, cfg)) = self.ui_state.viewport_at(x, y)
            && cfg.pan_trigger.matches(button, self.ui_state.modifiers)
            && self
                .last_tree
                .as_ref()
                .is_none_or(|t| !hit_test::occluded_by_overlay(t, (x, y), &vp_id))
        {
            let dedicated = cfg.pan_trigger.button != PointerButton::Primary
                || cfg.pan_trigger.modifiers != crate::event::KeyModifiers::default();
            let on_background = hit.as_ref().is_none_or(|h| *h.node_id == *vp_id);
            if dedicated || on_background {
                self.ui_state.begin_viewport_pan(vp_id, button, x, y);
                return Vec::new();
            }
        }

        // Plot interaction: a primary press on a plot's data rect resets the
        // view (double-click) or begins a drag whose action depends on the
        // plot's control scheme — the primary drag does one of box-zoom / pan
        // and `Shift` does the other. Plots have no interactive children, so
        // the press hits the plot node itself. Overlay dead space hits
        // nothing too, so the same occlusion rule as the camera / pan
        // captures applies (issue #127): a press on a modal floated over
        // the plot must not zoom or reset it.
        if matches!(button, PointerButton::Primary)
            && let Some((plot_id, metrics)) = self.ui_state.plot_at(x, y)
            && hit.as_ref().is_none_or(|h| *h.node_id == *plot_id)
            && self
                .last_tree
                .as_ref()
                .is_none_or(|t| !hit_test::occluded_by_overlay(t, (x, y), &plot_id))
        {
            // Count clicks ourselves: this branch returns before the shared
            // click-count call below, and a double-click resets the view.
            let clicks =
                self.ui_state
                    .next_click_count(Instant::now(), (x, y), Some(plot_id.as_str()));
            if clicks >= 2 {
                self.ui_state.reset_plot_view(&plot_id);
            } else {
                // The scheme picks what the unmodified drag does; Shift inverts.
                let pan = match metrics.controls {
                    crate::plot::PlotControls::ZoomDrag => self.ui_state.modifiers.shift,
                    crate::plot::PlotControls::PanDrag => !self.ui_state.modifiers.shift,
                };
                if pan {
                    self.ui_state.begin_plot_pan(plot_id, x, y);
                } else {
                    self.ui_state.begin_plot_zoom(plot_id, x, y);
                }
            }
            return Vec::new();
        }

        // Only the primary button drives focus + the visual press
        // envelope. Secondary/middle clicks shouldn't yank focus from
        // the currently-focused element (matches browser/native behavior
        // where right-clicking a button doesn't take focus).
        if !matches!(button, PointerButton::Primary) {
            // Stash the down-target on the secondary/middle channel so
            // pointer_up can confirm the click landed on the same node.
            self.ui_state.pressed_secondary = hit.map(|h| (h, button));
            return Vec::new();
        }

        // Stash any link URL the press lands on before the keyed-
        // target walk consumes the press. Cleared in `pointer_up`,
        // which only emits `LinkActivated` if the up position resolves
        // to the same URL — same press-then-confirm contract as a
        // normal `Click`. A press that misses every link clears any
        // stale value from the previous press so a drag-released-
        // elsewhere never fires a link from an earlier interaction.
        self.ui_state.pressed_link = self
            .linkable_tree()
            .and_then(|t| hit_test::link_at(t, (x, y)));
        self.ui_state.set_focus_from_pointer(hit.clone());
        // `:focus-visible` rule: pointer-driven focus suppresses the
        // ring; widgets that want it on click opt in via
        // `always_show_focus_ring`.
        self.ui_state.set_focus_visible(false);
        self.ui_state.pressed = hit.clone();
        // A press on the hovered node dismisses any tooltip for
        // the rest of this hover session — matches native UIs.
        self.ui_state.tooltip.dismissed_for_hover = true;
        let modifiers = self.ui_state.modifiers;

        // Click counting: extend a multi-click sequence when the press
        // lands on the same target inside the time + distance window.
        let now = Instant::now();
        let click_count =
            self.ui_state
                .next_click_count(now, (x, y), hit.as_ref().map(|t| t.node_id.as_ref()));

        let mut out = Vec::new();

        // Touch contact starts hover for this gesture. Mouse / pen
        // already track hover continuously through `pointer_moved`,
        // so this branch is touch-only — without it, a touch tap
        // would fire `PointerDown` and `Click` with no preceding
        // `PointerEnter`, and any hover-driven visual envelope on
        // the target would never advance for the duration of the
        // contact.
        if matches!(kind, PointerKind::Touch) {
            let prev_hover = self.ui_state.hovered.clone();
            let hover_changed = self.ui_state.set_hovered(hit.clone(), now);
            if hover_changed {
                if let Some(prev) = prev_hover {
                    out.push(UiEvent {
                        key: Some(prev.key.clone()),
                        target: Some(prev),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        selection: None,
                        modifiers,
                        click_count: 0,
                        path: None,
                        pointer_kind: Some(kind),
                        wheel_delta: None,
                        kind: UiEventKind::PointerLeave,
                    });
                }
                if let Some(new) = hit.clone() {
                    out.push(UiEvent {
                        key: Some(new.key.clone()),
                        target: Some(new),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        selection: None,
                        modifiers,
                        click_count: 0,
                        path: None,
                        pointer_kind: Some(kind),
                        wheel_delta: None,
                        kind: UiEventKind::PointerEnter,
                    });
                }
            }
            // Enter the gesture state machine. Decide upfront whether
            // the press target (or any ancestor) consumes touch drag,
            // so the threshold-cross branch in `pointer_moved` doesn't
            // re-walk the tree once per move. A press that hits dead
            // space (no keyed leaf) defaults to "doesn't consume" —
            // scroll wins, matching the natural mobile expectation
            // that swiping over background pans the page.
            let consumes_drag = hit
                .as_ref()
                .and_then(|t| {
                    self.last_tree
                        .as_ref()
                        .and_then(|tree| find_consumes_touch_drag(tree, &t.node_id, false))
                })
                .unwrap_or(false);
            self.ui_state.touch_gesture = TouchGestureState::Pending {
                initial: (x, y),
                consumes_drag,
                started_at: now,
            };
        }

        if let Some(p) = hit.clone() {
            // Caret-blink reset: a press inside the focused widget
            // (e.g., to reposition the caret in an already-focused
            // input) is editing activity. The earlier `set_focus`
            // call bumps when focus *changes*; this catches the
            // same-target case so click-to-move-caret resets the
            // blink too.
            if self.focused_captures_keys() {
                self.ui_state.bump_caret_activity(now);
            }
            out.push(UiEvent {
                key: Some(p.key.clone()),
                target: Some(p),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::PointerDown,
            });
        }

        // Selection routing. The selection hit-test is independent of
        // the focusable hit: a `text(...).key("p").selectable()` leaf is
        // both a (non-focusable) keyed PointerDown target and a
        // selectable text leaf. Apps see both events; selection drag
        // starts in either case. A press that lands on neither a
        // selectable nor a focusable widget clears any active
        // selection.
        if let Some(point) = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::selection_point_at(t, (x, y)))
        {
            self.start_selection_drag(point, &mut out, modifiers, (x, y), click_count, kind);
        } else if !self.ui_state.current_selection.is_empty() {
            // Clear-on-click only when the press lands somewhere that
            // can't take selection ownership itself.
            //
            // - If the press is on the widget that already owns the
            //   selection (same key), the widget's PointerDown
            //   handler updates its own caret; a runtime clear here
            //   races and collapses the app's selection back to
            //   default. (User-visible bug: caret alternated between
            //   the click position and byte 0 on every other click.)
            //
            // - If the press is on a *different* capture_keys widget
            //   (e.g., dragging from one text_input into another),
            //   that widget's PointerDown will replace the selection
            //   with one anchored at the click position. The runtime
            //   clear would arrive after the replace and wipe the
            //   anchor — so when the drag began, only `head` would
            //   advance and `anchor` would default to 0, jumping the
            //   selection start to the beginning of the text.
            //
            // Press on a regular focusable (button, etc.) or in dead
            // space still clears, matching the browser idiom.
            let click_handles_selection = match (&hit, &self.ui_state.current_selection.range) {
                (Some(h), Some(range)) => {
                    h.key == range.anchor.key
                        || h.key == range.head.key
                        || self
                            .last_tree
                            .as_ref()
                            .and_then(|t| find_capture_keys(t, &h.node_id))
                            .unwrap_or(false)
                }
                _ => false,
            };
            if !click_handles_selection {
                out.push(selection_event(
                    crate::selection::Selection::default(),
                    modifiers,
                    Some((x, y)),
                    Some(kind),
                ));
                self.ui_state.current_selection = crate::selection::Selection::default();
                self.ui_state.selection.drag = None;
            }
        }

        // Latent viewport pan (issue #125): the viewport branch above
        // deliberately lets a default-trigger press on a keyed child fall
        // through to the press-toward-click capture — but decided at
        // press time, that would leave a drag *starting* on a child dead.
        // Arm the movement-time arbitration instead: `pointer_moved`
        // converts this press into a real pan once it travels past
        // MOUSE_DRAG_SLOP; released within the slop it clicks as normal.
        //
        // Armed only when the drag has no more specific meaning. Every
        // capture above (scrollbar, edge-resize, camera, plot, immediate
        // pan) returned before this point; touch runs its own Pending
        // machine; a selection drag started this press means motion
        // extends the selection; a drag-consuming or capture-keys target
        // (slider scrub, text-input drag-select) owns its own drag; and
        // the pressed node must actually live inside the viewport — a
        // widget floating *over* it keeps its gesture, matching the
        // camera-drag rule.
        if !matches!(kind, PointerKind::Touch)
            && self.ui_state.selection.drag.is_none()
            && let Some(h) = self.ui_state.pressed.as_ref()
            && let Some((vp_id, cfg)) = self.ui_state.viewport_at(x, y)
            && cfg.pan_trigger.matches(button, modifiers)
            && let Some(tree) = self.last_tree.as_ref()
            && find_inside_subtree(tree, &vp_id, &h.node_id, false).unwrap_or(false)
            && !find_consumes_touch_drag(tree, &h.node_id, false).unwrap_or(false)
            && !find_capture_keys(tree, &h.node_id).unwrap_or(false)
        {
            self.ui_state.viewport.latent_pan = Some(LatentViewportPan {
                viewport_id: vp_id,
                press: (x, y),
            });
        }

        out
    }

    /// Stamp a new [`crate::state::SelectionDrag`] and emit a
    /// `SelectionChanged` event seeded by `point`. For
    /// `click_count == 2` the anchor / head pair expands to the word
    /// range around `point.byte`; for `click_count >= 3` it expands to
    /// the whole leaf (static-text triple-click typically wants the
    /// paragraph). For other counts (single click, default) the
    /// selection is collapsed at `point`.
    fn start_selection_drag(
        &mut self,
        point: crate::selection::SelectionPoint,
        out: &mut Vec<UiEvent>,
        modifiers: KeyModifiers,
        pointer: (f32, f32),
        click_count: u8,
        kind: PointerKind,
    ) {
        let leaf_text = self
            .last_tree
            .as_ref()
            .and_then(|t| crate::selection::find_keyed_text(t, &point.key))
            .unwrap_or_default();
        let (anchor_byte, head_byte) = match click_count {
            2 => crate::selection::word_range_at(&leaf_text, point.byte),
            n if n >= 3 => (0, leaf_text.len()),
            _ => (point.byte, point.byte),
        };
        let granularity = match click_count {
            2 => SelectionDragGranularity::Word,
            n if n >= 3 => SelectionDragGranularity::Leaf,
            _ => SelectionDragGranularity::Character,
        };
        let anchor = crate::selection::SelectionPoint::new(point.key.clone(), anchor_byte);
        let head = crate::selection::SelectionPoint::new(point.key.clone(), head_byte);
        let new_sel = crate::selection::Selection {
            range: Some(crate::selection::SelectionRange {
                anchor: anchor.clone(),
                head: head.clone(),
            }),
        };
        self.ui_state.current_selection = new_sel.clone();
        self.ui_state.selection.drag = Some(crate::state::SelectionDrag {
            anchor,
            head,
            granularity,
        });
        out.push(selection_event(
            new_sel,
            modifiers,
            Some(pointer),
            Some(kind),
        ));
    }

    fn extend_selection_drag_at(
        &mut self,
        x: f32,
        y: f32,
        kind: PointerKind,
        modifiers: KeyModifiers,
        out: &mut Vec<UiEvent>,
    ) {
        let Some(drag) = self.ui_state.selection.drag.clone() else {
            return;
        };
        let Some(tree) = self.last_tree.as_ref() else {
            return;
        };
        let raw_head =
            head_for_drag(tree, &self.ui_state, (x, y)).unwrap_or_else(|| drag.anchor.clone());
        let (anchor, head) = selection_range_for_drag(tree, &self.ui_state, &drag, raw_head);
        let new_sel = crate::selection::Selection {
            range: Some(crate::selection::SelectionRange { anchor, head }),
        };
        if new_sel != self.ui_state.current_selection {
            self.ui_state.current_selection = new_sel.clone();
            out.push(selection_event(
                new_sel,
                modifiers,
                Some((x, y)),
                Some(kind),
            ));
        }
    }

    /// Whether an edge-resize band may capture a press at `(x, y)` —
    /// the covered-scrollbar guard adapted to bands: when an overlay
    /// (dialog, popover) covers the seam, the hit-test target at the
    /// press point is outside the band's parent container, and the
    /// band must not steal the press from it. A band straddles two
    /// sibling panes, so the guard checks containment in the shared
    /// parent rather than in the resizable pane itself.
    fn resize_band_can_capture(
        &self,
        band: &crate::state::resize::ResizeBand,
        x: f32,
        y: f32,
    ) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        hit_test::hit_test_target(tree, &self.ui_state, (x, y))
            .is_none_or(|target| target_id_in_subtree(&band.container_id, &target.node_id))
    }

    fn scrollbar_can_capture(&self, scroll_id: &str, x: f32, y: f32) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        let target_allows_capture = hit_test::hit_test_target(tree, &self.ui_state, (x, y))
            .is_none_or(|target| target_id_in_subtree(scroll_id, &target.node_id));
        if !target_allows_capture {
            return false;
        }
        hit_test::scroll_targets_at(tree, (x, y))
            .iter()
            .any(|id| id == scroll_id)
    }

    /// Cancel an in-flight touch press because the gesture committed
    /// to scrolling. Emits `PointerCancel` for the pressed target
    /// (so widgets can roll back any setup they did at
    /// `PointerDown`) and `PointerLeave` for the hovered target
    /// (mirroring the contact-driven hover model from
    /// [`Self::pointer_up`]). Clears `pressed` so subsequent moves
    /// don't emit `Drag`, and clears the selection drag so the press
    /// doesn't keep extending a text selection from inside the
    /// scroll motion.
    fn cancel_press_for_scroll(
        &mut self,
        out: &mut Vec<UiEvent>,
        x: f32,
        y: f32,
        kind: PointerKind,
        modifiers: KeyModifiers,
    ) {
        let pressed = self.ui_state.pressed.take();
        let hovered = self.ui_state.hovered.clone();
        self.ui_state.set_hovered(None, Instant::now());
        self.ui_state.pressed_secondary = None;
        self.ui_state.pressed_link = None;
        // Clear link hover too: moves during the committed gesture
        // return before the link-hover update, so a stale value would
        // pin the hand cursor for the whole drag.
        self.ui_state.hovered_link = None;
        self.ui_state.selection.drag = None;
        if let Some(p) = pressed {
            out.push(UiEvent {
                key: Some(p.key.clone()),
                target: Some(p),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::PointerCancel,
            });
        }
        if let Some(h) = hovered {
            out.push(UiEvent {
                key: Some(h.key.clone()),
                target: Some(h),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::PointerLeave,
            });
        }
    }

    /// Pointer released. For the primary button, fires `PointerUp`
    /// (always, with the originally pressed target so drag-aware
    /// widgets see drag-end) and additionally `Click` if the release
    /// landed on the same node as the down. For secondary / middle,
    /// fires the corresponding click variant when the up landed on the
    /// same node; no analogue of `PointerUp` since drag is a primary-
    /// button concept here.
    pub fn pointer_up(&mut self, p: Pointer) -> Vec<UiEvent> {
        let Pointer {
            x, y, button, kind, ..
        } = p;
        self.ui_state.pointer_kind = kind;
        // Any primary release ends the click-vs-pan arbitration. Disarm
        // before the capture-release early-returns below: the pan /
        // camera ends are button-agnostic, so one of them can consume
        // this release (a capture another button started), and a latent
        // pan stranded past that point would convert on a later
        // button-less hover move. Within the slop the disarm is the
        // whole story — the pressed/hit compare below clicks as normal.
        if matches!(button, PointerButton::Primary) {
            self.ui_state.viewport.latent_pan = None;
        }
        // Scrollbar drag ends without producing app-level events —
        // the press never went through `pressed` / `pressed_secondary`
        // so there's nothing else to clean up. Released from anywhere;
        // the drag is global once captured, matching native scrollbars.
        if matches!(button, PointerButton::Primary) && self.ui_state.scroll.thumb_drag.is_some() {
            self.ui_state.scroll.thumb_drag = None;
            self.ui_state.touch_gesture = TouchGestureState::None;
            return Vec::new();
        }

        // An edge-resize drag releases like the scrollbar drag — the
        // press never reached `pressed`, so no Click/PointerUp chain.
        // The one event it does produce: `Resized`, routed to the
        // pane's key (when it has one), so apps know the natural
        // moment to read `user_size(key)` and persist it.
        if matches!(button, PointerButton::Primary)
            && let Some(drag) = self.ui_state.resize.drag.take()
        {
            self.ui_state.touch_gesture = TouchGestureState::None;
            let Some(key) = drag.key else {
                return Vec::new();
            };
            return vec![UiEvent {
                key: Some(key),
                target: None,
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers: self.ui_state.modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::Resized,
            }];
        }

        // A camera drag releases without producing app-level events — the
        // press was captured before focus/press, so there's nothing to
        // confirm. Released from anywhere (the drag is global once
        // captured), but only by the button that began it (issue #128):
        // another button's release falls through to its own handling
        // below instead of killing the drag.
        if self.ui_state.end_camera_drag(button) {
            self.ui_state.touch_gesture = TouchGestureState::None;
            return Vec::new();
        }

        // A viewport pan releases like the camera drag — captured before
        // focus/press, so there's nothing to confirm. Same button
        // discipline.
        if self.ui_state.end_viewport_pan(button) {
            self.ui_state.touch_gesture = TouchGestureState::None;
            return Vec::new();
        }

        // A plot pan releases the same way. Plot gestures only ever
        // begin on a primary press, so the gate is a plain button match.
        if matches!(button, PointerButton::Primary) && self.ui_state.end_plot_pan() {
            self.ui_state.touch_gesture = TouchGestureState::None;
            return Vec::new();
        }

        // A box-zoom selection applies its zoom on release.
        if matches!(button, PointerButton::Primary) && self.ui_state.end_plot_zoom() {
            self.ui_state.touch_gesture = TouchGestureState::None;
            return Vec::new();
        }

        // Touch gesture cleanup. Reset the state machine first so the
        // logic below sees a fresh slate; if the gesture had already
        // committed to scrolling or fired a long-press, release should
        // not synthesize Click / PointerUp. Editable long-press keeps a
        // press captured for drag-extension, so clear it here.
        let was_long_pressed =
            matches!(self.ui_state.touch_gesture, TouchGestureState::LongPressed);
        let momentum = match &self.ui_state.touch_gesture {
            TouchGestureState::Scrolling {
                velocity,
                scroll_id,
                ..
            } if matches!(kind, PointerKind::Touch) => {
                Some((scroll_id.clone(), *velocity, Instant::now()))
            }
            _ => None,
        };
        let was_scrolling_or_long = matches!(
            self.ui_state.touch_gesture,
            TouchGestureState::Scrolling { .. } | TouchGestureState::LongPressed
        );
        self.ui_state.touch_gesture = TouchGestureState::None;
        if was_scrolling_or_long {
            if let Some((scroll_id, velocity, now)) = momentum {
                self.ui_state
                    .start_scroll_momentum(scroll_id, velocity, now);
            }
            if was_long_pressed {
                self.ui_state.pressed = None;
                self.ui_state.pressed_secondary = None;
                self.ui_state.pressed_link = None;
                self.ui_state.selection.drag = None;
                self.ui_state.set_hovered(None, Instant::now());
            }
            return Vec::new();
        }

        // End any active text-selection drag. The selection itself
        // persists; only the "currently dragging" flag goes away.
        if matches!(button, PointerButton::Primary) {
            self.ui_state.selection.drag = None;
        }

        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        let modifiers = self.ui_state.modifiers;
        let mut out = Vec::new();
        match button {
            PointerButton::Primary => {
                let pressed = self.ui_state.pressed.take();
                let click_count = self.ui_state.current_click_count();
                if let Some(p) = pressed.clone() {
                    out.push(UiEvent {
                        key: Some(p.key.clone()),
                        target: Some(p),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        selection: None,
                        modifiers,
                        click_count,
                        path: None,
                        pointer_kind: Some(kind),
                        wheel_delta: None,
                        kind: UiEventKind::PointerUp,
                    });
                }
                if let (Some(p), Some(h)) = (pressed, hit)
                    && p.node_id == h.node_id
                {
                    // Toast dismiss buttons are runtime-managed —
                    // the click drops the matching toast from the
                    // queue and is *not* surfaced to the app, so
                    // `on_event` doesn't have to know about toast
                    // bookkeeping.
                    if let Some(id) = toast::parse_dismiss_key(&p.key) {
                        self.ui_state.dismiss_toast(id);
                    } else {
                        out.push(UiEvent {
                            key: Some(p.key.clone()),
                            target: Some(p),
                            pointer: Some((x, y)),
                            key_press: None,
                            text: None,
                            selection: None,
                            modifiers,
                            click_count,
                            path: None,
                            pointer_kind: Some(kind),
                            wheel_delta: None,
                            kind: UiEventKind::Click,
                        });
                    }
                }
                // Link click — surface the URL as a separate event so
                // the app's link policy is independent of any keyed
                // ancestor's `Click`. Press-then-confirm: the up
                // position must resolve to the same URL as the down
                // (cancel-on-drag-away, matching native link UX).
                if let Some(pressed_url) = self.ui_state.pressed_link.take() {
                    let up_link = self
                        .linkable_tree()
                        .and_then(|t| hit_test::link_at(t, (x, y)));
                    if up_link.as_ref() == Some(&pressed_url) {
                        out.push(UiEvent {
                            key: Some(pressed_url),
                            target: None,
                            pointer: Some((x, y)),
                            key_press: None,
                            text: None,
                            selection: None,
                            modifiers,
                            click_count: 1,
                            path: None,
                            pointer_kind: Some(kind),
                            wheel_delta: None,
                            kind: UiEventKind::LinkActivated,
                        });
                    }
                }
            }
            PointerButton::Secondary | PointerButton::Middle => {
                let pressed = self.ui_state.pressed_secondary.take();
                if let (Some((p, b)), Some(h)) = (pressed, hit)
                    && b == button
                    && p.node_id == h.node_id
                {
                    let event_kind = match button {
                        PointerButton::Secondary => UiEventKind::SecondaryClick,
                        PointerButton::Middle => UiEventKind::MiddleClick,
                        PointerButton::Primary => unreachable!(),
                    };
                    out.push(UiEvent {
                        key: Some(p.key.clone()),
                        target: Some(p),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        selection: None,
                        modifiers,
                        click_count: 1,
                        path: None,
                        pointer_kind: Some(kind),
                        wheel_delta: None,
                        kind: event_kind,
                    });
                }
            }
        }

        // Touch contact ends → clear hover. Mouse / pen keep tracking
        // hover after a release because the pointer is still over
        // something; a finger lifting off the screen has no analog,
        // so the hover envelope must wind down. Mirrors the synthetic
        // `PointerEnter` that `pointer_down` emits for touch.
        if matches!(kind, PointerKind::Touch)
            && let Some(prev) = self.ui_state.hovered.clone()
        {
            self.ui_state.set_hovered(None, Instant::now());
            out.push(UiEvent {
                key: Some(prev.key.clone()),
                target: Some(prev),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::PointerLeave,
            });
        }

        out
    }

    pub fn key_down(
        &mut self,
        logical: LogicalKey,
        physical: PhysicalKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Vec<UiEvent> {
        // Capture path: when the focused node opted into raw key
        // capture, editing keys are delivered as raw `KeyDown` events
        // to the focused target. Hotkeys still match first — an app's
        // global Ctrl+S beats a text input's local consumption of S.
        // Escape is both an editing key and the generic "exit editing"
        // command: route it to the widget first so it can collapse a
        // selection, then clear focus.
        if self.focused_captures_keys() {
            if let Some(event) = self
                .ui_state
                .try_hotkey(&logical, physical, modifiers, repeat)
            {
                return vec![event];
            }
            // Caret-blink reset: any key arriving at a capture_keys
            // widget is text-editing activity (caret motion, edit,
            // shortcut), so the caret should snap back to solid even
            // when the app doesn't propagate its `Selection` back via
            // `App::selection()`. Without this, hammering arrow keys
            // produces no visible blink reset.
            self.ui_state.bump_caret_activity(Instant::now());
            self.ui_state.set_focus_visible(true);
            let blur_after = logical.named() == Some(NamedKey::Escape);
            let out = self
                .ui_state
                .key_down_raw(logical, physical, modifiers, repeat)
                .into_iter()
                .collect();
            if blur_after {
                self.ui_state.set_focus(None);
                self.ui_state.set_focus_visible(false);
            }
            return out;
        }

        // Arrow-nav: if the focused node sits inside an arrow-navigable
        // group (a popover_panel of menu items, a tabs_list, a radio
        // group, a calendar grid), the arrows the group's orientation
        // covers plus Home / End move focus among its focusable members
        // rather than emitting a `KeyDown` event. Keys outside the
        // orientation fall through (Left/Right in a vertical menu stay
        // routable so a menubar can switch menus). Hotkeys are still
        // matched first so a global Ctrl+ArrowUp chord beats group
        // navigation.
        if matches!(
            logical.named(),
            Some(
                NamedKey::ArrowUp
                    | NamedKey::ArrowDown
                    | NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
                    | NamedKey::Home
                    | NamedKey::End
            )
        ) && let Some((mode, members)) = self.focused_arrow_nav_group()
            && mode.handles(&logical)
        {
            if let Some(event) = self
                .ui_state
                .try_hotkey(&logical, physical, modifiers, repeat)
            {
                return vec![event];
            }
            self.move_focus_in_group(&logical, mode, &members);
            return Vec::new();
        }

        let mut out: Vec<UiEvent> = self
            .ui_state
            .key_down(logical, physical, modifiers, repeat)
            .into_iter()
            .collect();

        // Esc clears any active text selection (parallels the
        // pointer_down "press lands outside selectable+focusable"
        // path). The Escape event itself still fires so apps can
        // dismiss popovers / modals; the SelectionChanged is emitted
        // alongside it. This only runs in the non-capture-keys path,
        // so pressing Esc while typing in an input doesn't clobber
        // the input's selection — matching browser behavior.
        if matches!(out.first().map(|e| e.kind), Some(UiEventKind::Escape))
            && !self.ui_state.current_selection.is_empty()
        {
            self.ui_state.current_selection = crate::selection::Selection::default();
            self.ui_state.selection.drag = None;
            out.push(selection_event(
                crate::selection::Selection::default(),
                modifiers,
                None,
                None,
            ));
        }

        out
    }

    /// Look up the focused node's nearest [`El::arrow_nav`] parent in
    /// the last laid-out tree and return the group's orientation plus
    /// its focusable members (the navigation targets). Returns `None`
    /// when no node is focused, the tree hasn't been built yet, or the
    /// focused element isn't inside an arrow-navigable parent.
    fn focused_arrow_nav_group(&self) -> Option<(crate::tree::ArrowNav, Vec<UiTarget>)> {
        let focused = self.ui_state.focused.as_ref()?;
        let tree = self.last_tree.as_ref()?;
        focus::arrow_nav_group(tree, &focused.node_id)
    }

    /// Move the focused element to the appropriate group member for
    /// `key`. Linear modes step by one in tree order (saturating at
    /// the ends — no wrap, so holding the key doesn't loop visually);
    /// `Home` / `End` jump to the first / last member. In a
    /// [`Grid`](crate::tree::ArrowNav::Grid) group, `Up` / `Down` move
    /// to the geometrically nearest member in the row above / below —
    /// disabled (unfocusable) cells aren't members, so navigation
    /// lands on the nearest enabled day rather than dead-ending.
    fn move_focus_in_group(
        &mut self,
        logical: &LogicalKey,
        mode: crate::tree::ArrowNav,
        members: &[UiTarget],
    ) {
        if members.is_empty() {
            return;
        }
        let focused_id = match self.ui_state.focused.as_ref() {
            Some(t) => t.node_id.clone(),
            None => return,
        };
        let idx = members.iter().position(|t| t.node_id == focused_id);
        let grid = mode == crate::tree::ArrowNav::Grid;
        let next_idx = match (logical.named(), idx) {
            (Some(NamedKey::ArrowUp), Some(i)) if grid => {
                match grid_vertical_step(members, i, -1.0) {
                    Some(j) => j,
                    None => return,
                }
            }
            (Some(NamedKey::ArrowDown), Some(i)) if grid => {
                match grid_vertical_step(members, i, 1.0) {
                    Some(j) => j,
                    None => return,
                }
            }
            (Some(NamedKey::ArrowUp | NamedKey::ArrowLeft), Some(i)) => i.saturating_sub(1),
            (Some(NamedKey::ArrowDown | NamedKey::ArrowRight), Some(i)) => {
                (i + 1).min(members.len() - 1)
            }
            (Some(NamedKey::Home), _) => 0,
            (Some(NamedKey::End), _) => members.len() - 1,
            _ => return,
        };
        if Some(next_idx) != idx {
            self.ui_state.set_focus(Some(members[next_idx].clone()));
            self.ui_state.set_focus_visible(true);
        }
    }

    /// Look up the focused node in the last laid-out tree and return
    /// its `capture_keys` flag — i.e. whether the focused widget is a
    /// text-input-style consumer of raw key events. False when no
    /// node is focused or the tree hasn't been built yet. Hosts use
    /// this each frame to mirror "is a text input active?" into
    /// platform UI affordances (most notably the on-screen keyboard).
    pub fn focused_captures_keys(&self) -> bool {
        let Some(focused) = self.ui_state.focused.as_ref() else {
            return false;
        };
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        find_capture_keys(tree, &focused.node_id).unwrap_or(false)
    }

    /// OS-composed text input (printable characters after dead-key /
    /// shift / IME composition). Routed to the focused element as a
    /// `TextInput` event. Returns `None` if no node has focus, or if
    /// `text` is empty (some platforms emit empty composition strings
    /// during IME selection).
    pub fn text_input(&mut self, text: String) -> Option<UiEvent> {
        if text.is_empty() {
            return None;
        }
        let target = self.ui_state.focused.clone()?;
        let modifiers = self.ui_state.modifiers;
        // Caret-blink reset: typing into the focused widget is
        // text-editing activity. See the matching bump in `key_down`.
        self.ui_state.bump_caret_activity(Instant::now());
        Some(UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: None,
            key_press: None,
            text: Some(text),
            selection: None,
            modifiers,
            click_count: 0,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::TextInput,
        })
    }

    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.ui_state.set_hotkeys(hotkeys);
    }

    /// Push the app's current [`crate::selection::Selection`] into the
    /// runtime so the painter can draw highlight bands. Hosts call
    /// this once per frame alongside `set_hotkeys`, sourcing the value
    /// from [`crate::event::App::selection`].
    pub fn set_selection(&mut self, selection: crate::selection::Selection) {
        if self.ui_state.current_selection != selection {
            self.ui_state.bump_caret_activity(Instant::now());
        }
        self.ui_state.current_selection = selection;
    }

    /// Resolve the runtime's current selection to a text payload using
    /// the most recently laid-out tree. Returns `None` when nothing is
    /// selected or the selection's keyed leaves are missing from the
    /// snapshot (typically because they scrolled out of a
    /// [`crate::widgets::virtual_list`] since the selection was made).
    ///
    /// This is the wiring `Ctrl+C` / `Ctrl+X` should use from a host.
    /// A naive "rebuild the app tree and walk it" approach silently
    /// breaks for virtualized panes: virtual_list rows are realized
    /// during layout, not build, so a freshly built tree doesn't
    /// contain them and selections inside a chat-style virtualized
    /// pane resolve to `None`. `last_tree` already has the visible
    /// rows realized at their live scroll offset.
    pub fn selected_text(&self) -> Option<String> {
        self.selected_text_for(&self.ui_state.current_selection)
    }

    /// Like [`Self::selected_text`], but resolves an explicit
    /// [`crate::selection::Selection`] against the last laid-out tree —
    /// useful immediately after an event handler updates
    /// [`crate::event::App::selection`] but before the host has
    /// rebroadcast it via [`Self::set_selection`].
    pub fn selected_text_for(&self, selection: &crate::selection::Selection) -> Option<String> {
        let tree = self.last_tree.as_ref()?;
        crate::selection::selected_text(tree, selection)
    }

    /// Queue toast specs onto the runtime's toast stack. Each spec
    /// is stamped with a monotonic id and `expires_at = now + ttl`;
    /// the next `prepare_layout` call drops expired entries and
    /// synthesizes a `toast_stack` floating layer over the rest.
    /// Hosts wire this from `App::drain_toasts` once per frame.
    pub fn push_toasts(&mut self, specs: Vec<crate::toast::ToastSpec>) {
        let now = Instant::now();
        for spec in specs {
            self.ui_state.push_toast(spec, now);
        }
    }

    /// Programmatically dismiss a single toast by id. Mostly useful
    /// when the app wants to cancel a long-TTL toast in response to
    /// some external event (e.g., the connection reconnected).
    pub fn dismiss_toast(&mut self, id: u64) {
        self.ui_state.dismiss_toast(id);
    }

    /// Queue programmatic focus requests by widget key. Each entry is
    /// resolved during the next `prepare_layout`, after the focus
    /// order has been rebuilt from the new tree; unmatched keys drop
    /// silently. Hosts wire this from [`crate::event::App::drain_focus_requests`]
    /// once per frame, alongside `push_toasts`.
    pub fn push_focus_requests(&mut self, keys: Vec<String>) {
        self.ui_state.push_focus_requests(keys);
    }

    /// Queue programmatic scroll-to-row requests targeting virtual
    /// lists by key. Each request is consumed during layout of the
    /// matching list, where viewport height and row heights are
    /// known. Hosts wire this from [`crate::event::App::drain_scroll_requests`]
    /// once per frame, alongside `push_focus_requests`.
    pub fn push_scroll_requests(&mut self, requests: Vec<crate::scroll::ScrollRequest>) {
        self.ui_state.push_scroll_requests(requests);
    }

    /// Queue programmatic [`crate::viewport::ViewportRequest`]s
    /// (fit-to-content / reset / center) targeting `viewport()` containers
    /// by key. Each is consumed during layout of the matching viewport,
    /// where its live rect and content extents are known. Hosts wire this
    /// from [`crate::event::App::drain_viewport_requests`] once per frame,
    /// alongside `push_scroll_requests`.
    pub fn push_viewport_requests(&mut self, requests: Vec<crate::viewport::ViewportRequest>) {
        self.ui_state.push_viewport_requests(requests);
    }

    /// Queue programmatic plot view requests for the next prepare pass.
    /// Hosts forward [`crate::event::App::drain_plot_requests`] here once
    /// per frame, next to the viewport requests.
    pub fn push_plot_requests(&mut self, requests: Vec<crate::plot::PlotRequest>) {
        self.ui_state.push_plot_requests(requests);
    }

    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.ui_state.set_animation_mode(mode);
    }

    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        self.ui_state.cancel_scroll_momentum();
        // A 3D scene under the pointer takes the wheel as zoom, before any
        // scroll routing (so the scene doesn't also scroll its container).
        if self.ui_state.camera_wheel_zoom(tree, x, y, dy) {
            return true;
        }
        // A `viewport()` under the pointer likewise consumes the wheel as
        // cursor-anchored zoom before scroll routing, so the canvas zooms
        // instead of scrolling an enclosing container.
        if self.ui_state.viewport_wheel_zoom(tree, x, y, dy) {
            return true;
        }
        // A plot under the pointer consumes the wheel as cursor-anchored
        // zoom (the time axis, with Y auto-scaling), before scroll routing.
        if self.ui_state.plot_wheel_zoom(tree, x, y, dy) {
            return true;
        }
        self.ui_state.pointer_wheel(tree, (x, y), dy)
    }

    /// Build a routed wheel event for the keyed target under `(x, y)`.
    ///
    /// Hosts should dispatch this before calling [`Self::pointer_wheel`].
    /// If the app consumes the returned event, skip the fallback scroll
    /// call; otherwise, call `pointer_wheel` to preserve Damascene's default
    /// scroll behavior.
    pub fn pointer_wheel_event(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Option<UiEvent> {
        if dx.abs() <= f32::EPSILON && dy.abs() <= f32::EPSILON {
            return None;
        }
        let tree = self.last_tree.as_ref()?;
        let target = hit_test::hit_test_target(tree, &self.ui_state, (x, y))?;
        self.ui_state.cancel_scroll_momentum();
        Some(UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some((x, y)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: self.ui_state.modifiers,
            click_count: 0,
            path: None,
            pointer_kind: Some(self.ui_state.pointer_kind),
            wheel_delta: Some((dx, dy)),
            kind: UiEventKind::PointerWheel,
        })
    }

    /// Drain any time-driven input events whose deadline has passed
    /// at `now`. Currently the only such event is the touch
    /// long-press: a `Pending` touch held in place past
    /// [`LONG_PRESS_DELAY`] fires `LongPress` at the original press
    /// coords, usually after cancelling the originally pressed target.
    /// Editable capture-keys targets keep their press captured so
    /// movement can emit `Drag` for selection extension. The gesture
    /// state transitions to `LongPressed` so the eventual finger lift
    /// produces no further events.
    ///
    /// Hosts call this once per frame *before* dispatching pointer /
    /// keyboard events so the long-press fires deterministically
    /// before any subsequent input. Returns `Vec::new()` when no
    /// deadline has elapsed; cheap to call every frame.
    pub fn poll_input(&mut self, now: Instant) -> Vec<UiEvent> {
        let TouchGestureState::Pending {
            initial,
            started_at,
            ..
        } = self.ui_state.touch_gesture.clone()
        else {
            return Vec::new();
        };
        if now.duration_since(started_at) < LONG_PRESS_DELAY {
            return Vec::new();
        }
        let mut out = Vec::new();
        let modifiers = self.ui_state.modifiers;
        let kind = PointerKind::Touch;
        let (x, y) = initial;
        let press_target = self.ui_state.pressed.clone();
        let preserves_press_for_drag = press_target.as_ref().is_some_and(|t| {
            self.last_tree
                .as_ref()
                .and_then(|tree| find_capture_keys(tree, &t.node_id))
                .unwrap_or(false)
        });
        if preserves_press_for_drag {
            self.ui_state.pressed_secondary = None;
            self.ui_state.pressed_link = None;
            self.ui_state.selection.drag = None;
        } else {
            // PointerCancel + LongPress to the originally pressed
            // target. `cancel_press_for_scroll` already does the
            // bookkeeping (clear pressed / pressed_secondary / hovered /
            // selection.drag and emit PointerCancel + PointerLeave);
            // reuse it so scroll-cancel and non-editable long-press
            // cancellation stay aligned.
            self.cancel_press_for_scroll(&mut out, x, y, kind, modifiers);
        }
        if let Some(t) = press_target {
            out.push(UiEvent {
                key: Some(t.key.clone()),
                target: Some(t),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::LongPress,
            });
        } else {
            // Press landed in dead space (no keyed leaf). Still fire
            // the LongPress with no target so window-level handlers
            // (drop zones, full-viewport context menus) can react.
            out.push(UiEvent {
                key: None,
                target: None,
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                selection: None,
                modifiers,
                click_count: 0,
                path: None,
                pointer_kind: Some(kind),
                wheel_delta: None,
                kind: UiEventKind::LongPress,
            });
        }
        if !preserves_press_for_drag
            && let Some(point) = self
                .last_tree
                .as_ref()
                .and_then(|t| hit_test::selection_point_at(t, (x, y)))
        {
            self.start_selection_drag(point, &mut out, modifiers, (x, y), 2, kind);
        }
        self.ui_state.touch_gesture = TouchGestureState::LongPressed;
        out
    }

    /// Time remaining until the next time-driven input deadline at
    /// `now`, or `None` when nothing is pending. Hosts fold this into
    /// their redraw scheduling so a held touch fires its long-press
    /// even when the user holds perfectly still — without it,
    /// `request_redraw` is never called and the deadline never
    /// fires.
    ///
    /// `Some(Duration::ZERO)` means "deadline already elapsed; call
    /// `poll_input` immediately."
    pub fn next_input_deadline(&self, now: Instant) -> Option<std::time::Duration> {
        if self.ui_state.has_scroll_momentum() {
            return Some(std::time::Duration::ZERO);
        }
        let TouchGestureState::Pending { started_at, .. } = self.ui_state.touch_gesture.clone()
        else {
            return None;
        };
        let elapsed = now.duration_since(started_at);
        Some(LONG_PRESS_DELAY.saturating_sub(elapsed))
    }

    // ---- Per-frame staging ----

    /// Layout + state apply + animation tick + viewport projection +
    /// `DrawOp` resolution. Returns the resolved op list and whether
    /// visual animations need another frame; writes per-stage timings
    /// into `timings` (`layout` + `draw_ops`).
    ///
    /// `samples_time` answers "does this shader's output depend on
    /// `frame.time`?" The runtime calls it once per draw op when no
    /// other in-flight motion has already requested a redraw; any
    /// `true` answer keeps `needs_redraw` set so the host idle loop
    /// keeps ticking. Stock shaders self-report through
    /// [`crate::shader::StockShader::is_continuous`]; backends layer
    /// on the registered set of `samples_time=true` custom shaders.
    /// Callers that have no time-driven shaders pass
    /// [`Self::no_time_shaders`].
    pub fn prepare_layout<F>(
        &mut self,
        root: &mut El,
        viewport: Rect,
        scale_factor: f32,
        timings: &mut PrepareTimings,
        samples_time: F,
    ) -> LayoutPrepared
    where
        F: Fn(&ShaderHandle) -> bool,
    {
        let t0 = Instant::now();
        let scroll_momentum_pending = self.ui_state.tick_scroll_momentum(t0);
        // Tooltip + toast synthesis run before the real layout: assign
        // ids first so the tooltip pass can resolve the hover anchor
        // by computed_id, then append the runtime-managed floating
        // layers. The subsequent `layout::layout` call re-assigns
        // (idempotently — same path shapes produce the same ids) and
        // lays out the appended layers alongside everything else.
        let mut needs_redraw = {
            crate::profile_span!("prepare::layout");
            {
                crate::profile_span!("prepare::layout::assign_ids");
                layout::assign_ids(root);
            }
            let tooltip_pending = {
                crate::profile_span!("prepare::layout::tooltip");
                tooltip::synthesize_tooltip(root, &self.ui_state, t0)
            };
            let toast_pending = {
                crate::profile_span!("prepare::layout::toast");
                toast::synthesize_toasts(root, &mut self.ui_state, t0)
            };
            {
                crate::profile_span!("prepare::layout::apply_metrics");
                self.theme.apply_metrics(root);
            }
            {
                crate::profile_span!("prepare::layout::layout");
                // `assign_ids` ran above (so tooltip/toast synthesis
                // could resolve nodes by id), and the synthesize
                // functions called `assign_id_appended` on the layers
                // they pushed — so the recursive id walk inside
                // `layout::layout` would be a wasted second pass over
                // the entire tree. Use `layout_post_assign` to skip it.
                layout::layout_post_assign(root, &mut self.ui_state, viewport);
                // Drop scroll requests that didn't match any virtual
                // list this frame (the matching list may have been
                // removed from the tree, or the app may have raced a
                // state change that retired the key).
                self.ui_state.clear_pending_scroll_requests();
                // Same for viewport requests that matched no viewport.
                self.ui_state.clear_pending_viewport_requests();
                // Bound the persistent scroll side-maps (LRU over
                // absent identities; issue #57).
                // (plus the viewport pan/zoom and plot view maps) —
                // one fused walk for all three.
                self.ui_state.gc_side_maps(root);
                // Resolve each plot's view (auto-fit / Y-autoscale) and
                // data-rect metrics now that layout has run, so draw_ops
                // and the gesture router read a settled view.
                self.ui_state.prepare_plots(root);
                // Drop plot requests that matched no plot this frame,
                // like the scroll / viewport queues above.
                self.ui_state.clear_pending_plot_requests();
            }
            {
                crate::profile_span!("prepare::layout::sync_orders");
                self.ui_state.sync_focus_and_selection_order(root);
            }
            {
                crate::profile_span!("prepare::layout::sync_layer_focus");
                focus::sync_layer_focus(root, &mut self.ui_state);
            }
            {
                // Drain after popover auto-focus so explicit app
                // requests win when both fire on the same frame
                // (e.g. a hotkey opens a popover and then jumps focus
                // to a non-default child).
                crate::profile_span!("prepare::layout::drain_focus_requests");
                self.ui_state.drain_focus_requests();
            }
            {
                crate::profile_span!("prepare::layout::apply_state");
                self.ui_state.apply_to_state();
            }
            self.viewport_px = self.surface_size_override.unwrap_or_else(|| {
                (
                    (viewport.w * scale_factor).ceil().max(1.0) as u32,
                    (viewport.h * scale_factor).ceil().max(1.0) as u32,
                )
            });
            let animations = {
                crate::profile_span!("prepare::layout::tick_animations");
                self.ui_state
                    .tick_visual_animations(root, Instant::now(), self.theme.palette())
            };
            // Advance keyed scene cameras toward their goals (data
            // re-centre / focus requests spring; gestures land in slice c).
            // Unsettled cameras keep the frame requesting redraw, like a
            // settling visual animation.
            let cameras_animating = {
                crate::profile_span!("prepare::layout::tick_cameras");
                self.ui_state.tick_scene_cameras(root, Instant::now())
            };
            // A viewport smooth navigation in flight keeps frames
            // coming until it lands (the flight is sampled inside the
            // layout pass above; arrival removes the entry). Expired
            // flights layout didn't reach (scroll-pruned subtrees)
            // don't pin frames — they snap lazily when next laid out.
            let viewport_flights = self.ui_state.viewport.any_flight_animating(Instant::now());
            animations
                || cameras_animating
                || viewport_flights
                || tooltip_pending
                || toast_pending
                || scroll_momentum_pending
        };
        let t_after_layout = Instant::now();
        timings.layout_intrinsic_cache = layout::take_intrinsic_cache_stats();
        timings.layout_prune = layout::take_prune_stats();
        let (ops, draw_ops_stats) = {
            crate::profile_span!("prepare::draw_ops");
            let mut stats = DrawOpsStats::default();
            let ops = draw_ops::draw_ops_with_theme_and_stats(
                root,
                &self.ui_state,
                &self.theme,
                &mut stats,
            );
            (ops, stats)
        };
        let t_after_draw_ops = Instant::now();
        timings.layout = t_after_layout - t0;
        timings.draw_ops = t_after_draw_ops - t_after_layout;
        timings.draw_ops_culled_text_ops = draw_ops_stats.culled_text_ops;
        // Surface the hovered scatter point the draw-op pass picked (a frame
        // late, like the depth map) so the app can read it next build.
        self.ui_state
            .set_hovered_scene_point(draw_ops_stats.hovered_scene_point);
        timings.text_layout_cache = crate::text::metrics::take_shape_cache_stats();

        // Two-lane deadline split:
        //
        // - **Layout lane**: signals that require a rebuild + layout
        //   pass to render correctly on the next frame. Animation
        //   settling, tooltip / toast pending, and widget
        //   `redraw_within` requests all change the El tree's visual
        //   state at their deadline.
        // - **Paint lane**: time-driven shaders (stock continuous, or
        //   `samples_time=true` custom). The El tree is unchanged; only
        //   `frame.time` needs to advance. Hosts that want to skip
        //   layout for these can run a paint-only frame via
        //   [`Self::prepare_paint_cached`] + [`Self::last_ops`].
        //
        // Bool-shaped layout signals (animations settling, tooltip /
        // toast pending) map to `Duration::ZERO`. The widget
        // `redraw_within` aggregate is folded in via `min`.
        let shader_needs_redraw = ops.iter().any(|op| op_is_continuous(op, &samples_time));
        let widget_redraw = draw_ops_stats.redraw_within;
        // Fold time-driven input in so held touches and active touch
        // momentum drive redraws even when no other animation / shader
        // / widget signal is asking for one. Otherwise the host falls
        // idle and input physics never advance until the next pointer
        // event.
        let input_deadline = self.next_input_deadline(Instant::now());
        let widget_redraw = match (widget_redraw, input_deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };

        let next_layout_redraw_in = match (needs_redraw, widget_redraw) {
            // An immediate redraw is already due; a widget deadline can
            // only be later, so it never extends the wait.
            (true, _) => Some(std::time::Duration::ZERO),
            (false, d) => d,
        };
        let next_paint_redraw_in = if shader_needs_redraw {
            Some(std::time::Duration::ZERO)
        } else {
            None
        };
        if next_layout_redraw_in.is_some() || next_paint_redraw_in.is_some() {
            needs_redraw = true;
        }

        // Ops are returned by value (not cached on `self`) so the
        // caller can borrow them into the per-frame `prepare_paint`
        // without also locking `&mut self`. The wrapper hands them
        // back to `self.last_ops` after paint — see [`Self::last_ops`].
        LayoutPrepared {
            ops,
            needs_redraw,
            next_layout_redraw_in,
            next_paint_redraw_in,
        }
    }

    /// Run [`Self::prepare_paint`] against the cached
    /// [`Self::last_ops`] from the most recent
    /// [`Self::prepare_layout`] call. Used by hosts that service a
    /// paint-only redraw (driven by
    /// [`PrepareResult::next_paint_redraw_in`]) without re-running
    /// build + layout.
    ///
    /// The caller is responsible for the same paint-time invariants as
    /// [`Self::prepare_paint`]: call `text.frame_begin()` first, and
    /// ensure no input has been processed since the last
    /// `prepare_layout` (otherwise hover / press state is stale and a
    /// full prepare is required instead).
    pub fn prepare_paint_cached<F1, F2>(
        &mut self,
        is_registered: F1,
        samples_backdrop: F2,
        text: &mut dyn TextRecorder,
        scale_factor: f32,
        timings: &mut PrepareTimings,
    ) where
        F1: Fn(&ShaderHandle) -> bool,
        F2: Fn(&ShaderHandle) -> bool,
    {
        // `prepare_paint` only touches `self.{quad_scratch, runs,
        // paint_items}`, not `self.last_ops`, but the borrow checker
        // can't see that — split-borrow via `mem::take` + restore.
        let ops = std::mem::take(&mut self.last_ops);
        self.prepare_paint(
            &ops,
            is_registered,
            samples_backdrop,
            text,
            scale_factor,
            timings,
        );
        self.last_ops = ops;
    }

    /// Standard "no custom time-driven shaders" closure for
    /// [`Self::prepare_layout`]. Backends that haven't wired up the
    /// custom-shader registry yet pass this; only stock shaders that
    /// self-report via `is_continuous()` participate in the scan.
    pub fn no_time_shaders(_shader: &ShaderHandle) -> bool {
        false
    }

    /// Re-evaluate the paint-lane deadline against the currently-cached
    /// [`Self::last_ops`]. Used by backends serving a paint-only frame
    /// (`repaint(...)`) so they can re-arm
    /// [`PrepareResult::next_paint_redraw_in`] without re-running
    /// `prepare_layout`. Returns `Some(Duration::ZERO)` when any cached
    /// op still binds a continuous shader.
    pub fn scan_continuous_shaders<F>(&self, samples_time: F) -> Option<std::time::Duration>
    where
        F: Fn(&ShaderHandle) -> bool,
    {
        let any = self
            .last_ops
            .iter()
            .any(|op| op_is_continuous(op, &samples_time));
        if any {
            Some(std::time::Duration::ZERO)
        } else {
            None
        }
    }

    /// Walk the resolved `DrawOp` list, packing quads into
    /// `quad_scratch` + grouping them into `runs`, interleaving text
    /// records via the backend-supplied [`TextRecorder`]. Returns the
    /// number of quad instances written (so the backend can size its
    /// instance buffer).
    ///
    /// Callers must call `text.frame_begin()` themselves *before*
    /// invoking this — `prepare_paint` does not call it for them
    /// because backends often want to clear other per-frame text
    /// scratch in the same step.
    pub fn prepare_paint<F1, F2>(
        &mut self,
        ops: &[DrawOp],
        is_registered: F1,
        samples_backdrop: F2,
        text: &mut dyn TextRecorder,
        scale_factor: f32,
        timings: &mut PrepareTimings,
    ) where
        F1: Fn(&ShaderHandle) -> bool,
        F2: Fn(&ShaderHandle) -> bool,
    {
        crate::profile_span!("prepare::paint");
        let t0 = Instant::now();
        self.quad_scratch.clear();
        self.runs.clear();
        self.paint_items.clear();

        let mut current: Option<(ShaderHandle, Option<PhysicalScissor>)> = None;
        let mut run_first: u32 = 0;
        // At most one snapshot per frame. Auto-inserted before
        // the first paint that samples the backdrop.
        let mut snapshot_emitted = false;

        for op in ops {
            match op {
                DrawOp::Quad {
                    rect,
                    scissor,
                    shader,
                    uniforms,
                    ..
                } => {
                    if !is_registered(shader) {
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !snapshot_emitted && samples_backdrop(shader) {
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        current = None;
                        run_first = self.quad_scratch.len() as u32;
                        self.paint_items.push(PaintItem::BackdropSnapshot);
                        snapshot_emitted = true;
                    }
                    // Custom shaders with an introspected slot map get
                    // name-routed uniforms with drift detection; stock
                    // shaders and unintrospected customs keep the
                    // positional path (issue #99).
                    let inst = match shader {
                        ShaderHandle::Custom(name) => match self.shader_slot_maps.get(name) {
                            Some(slot_map) => {
                                let mut unknown = Vec::new();
                                let inst = crate::paint::pack_instance_named(
                                    *rect,
                                    uniforms,
                                    self.working_color_space,
                                    slot_map,
                                    &mut unknown,
                                );
                                for key in unknown {
                                    if self.warned_shader_uniforms.insert((name, key)) {
                                        let declared: Vec<&str> =
                                            slot_map.declared().map(|(n, _)| n).collect();
                                        log::warn!(
                                            "damascene: shader `{name}` declares no \
                                                 instance attribute named `{key}` — the \
                                                 value lands nowhere. Declared names: \
                                                 {declared:?} (plus the positional aliases \
                                                 vec_a..vec_e). Rename one side so the Rust \
                                                 packing and WGSL unpacking agree."
                                        );
                                    }
                                }
                                inst
                            }
                            None => {
                                pack_instance_in(*rect, *shader, uniforms, self.working_color_space)
                            }
                        },
                        _ => pack_instance_in(*rect, *shader, uniforms, self.working_color_space),
                    };

                    let key = (*shader, phys);
                    if current != Some(key) {
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        current = Some(key);
                        run_first = self.quad_scratch.len() as u32;
                    }
                    self.quad_scratch.push(inst);
                }
                DrawOp::GlyphRun {
                    rect,
                    scissor,
                    color,
                    text: glyph_text,
                    size,
                    line_height,
                    family,
                    mono_family,
                    weight,
                    mono,
                    wrap,
                    anchor,
                    underline,
                    strikethrough,
                    link,
                    tabular_numerals,
                    letter_spacing,
                    ..
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let mut style = crate::text::atlas::RunStyle::new(*weight, *color)
                        .family(*family)
                        .mono_family(*mono_family);
                    if *mono {
                        style = style.mono();
                    }
                    if *tabular_numerals {
                        style = style.tabular_numerals();
                    }
                    if *letter_spacing != 0.0 {
                        style = style.with_letter_spacing(*letter_spacing);
                    }
                    if *underline {
                        style = style.underline();
                    }
                    if *strikethrough {
                        style = style.strikethrough();
                    }
                    if let Some(url) = link {
                        style = style.with_link(url.clone());
                    }
                    let layers = text.record(
                        *rect,
                        phys,
                        &style,
                        glyph_text,
                        *size,
                        *line_height,
                        *wrap,
                        *anchor,
                        scale_factor,
                    );
                    for index in layers {
                        self.paint_items.push(PaintItem::Text(index));
                    }
                }
                DrawOp::AttributedText {
                    rect,
                    scissor,
                    runs,
                    size,
                    line_height,
                    wrap,
                    anchor,
                    ..
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let layers = text.record_runs(
                        *rect,
                        phys,
                        runs,
                        *size,
                        *line_height,
                        *wrap,
                        *anchor,
                        scale_factor,
                    );
                    for index in layers {
                        self.paint_items.push(PaintItem::Text(index));
                    }
                }
                DrawOp::Icon {
                    rect,
                    scissor,
                    source,
                    color,
                    size,
                    stroke_width,
                    ..
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let recorded = text.record_icon(
                        *rect,
                        phys,
                        source,
                        *color,
                        *size,
                        *stroke_width,
                        scale_factor,
                    );
                    match recorded {
                        RecordedPaint::Text(layers) => {
                            for index in layers {
                                self.paint_items.push(PaintItem::Text(index));
                            }
                        }
                        RecordedPaint::Icon(runs) => {
                            for index in runs {
                                self.paint_items.push(PaintItem::IconRun(index));
                            }
                        }
                    }
                }
                DrawOp::Image {
                    rect,
                    scissor,
                    image,
                    tint,
                    radius,
                    fit,
                    range_limit,
                    ..
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let recorded = text.record_image(
                        *rect,
                        phys,
                        image,
                        *tint,
                        *radius,
                        *fit,
                        *range_limit,
                        scale_factor,
                    );
                    for index in recorded {
                        self.paint_items.push(PaintItem::Image(index));
                    }
                }
                DrawOp::AppTexture {
                    rect,
                    scissor,
                    texture,
                    alpha,
                    transform,
                    ..
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let recorded = text.record_app_texture(
                        *rect,
                        phys,
                        texture,
                        *alpha,
                        *transform,
                        scale_factor,
                    );
                    for index in recorded {
                        self.paint_items.push(PaintItem::AppTexture(index));
                    }
                }
                DrawOp::Vector {
                    rect,
                    scissor,
                    asset,
                    render_mode,
                    ..
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let recorded =
                        text.record_vector(*rect, phys, asset, *render_mode, scale_factor);
                    for index in recorded {
                        self.paint_items.push(PaintItem::Vector(index));
                    }
                }
                DrawOp::Scene3D {
                    id,
                    rect,
                    scissor,
                    scene,
                } => {
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    if !paint_rect_visible(*rect, *scissor, self.viewport_px, scale_factor) {
                        timings.paint_culled_ops += 1;
                        continue;
                    }
                    // Close the current quad run so paint ordering stays
                    // correct: the scene composites at this position, after
                    // everything painted beneath it. Backends without a
                    // scene renderer leave the default no-op recorder, so no
                    // `PaintItem` is emitted and the scene draws nothing.
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let recorded = text.record_scene3d(*rect, phys, id, scene, scale_factor);
                    for index in recorded {
                        self.paint_items.push(PaintItem::Scene3D(index));
                    }
                }
                DrawOp::BackdropSnapshot => {
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;
                    // Cap at one snapshot per frame; an explicit op only
                    // lands if the auto-emitter hasn't fired yet.
                    if !snapshot_emitted {
                        self.paint_items.push(PaintItem::BackdropSnapshot);
                        snapshot_emitted = true;
                    }
                }
            }
        }
        close_run(
            &mut self.runs,
            &mut self.paint_items,
            current,
            run_first,
            self.quad_scratch.len() as u32,
        );
        timings.paint = Instant::now() - t0;
    }

    /// Take a clone of the laid-out tree for next-frame hit-testing.
    /// Call after the per-frame work completes (GPU upload, atlas
    /// flush, etc.) so the snapshot reflects final geometry. Writes
    /// `timings.snapshot`.
    ///
    /// Prefer [`Self::snapshot_owned`] when the caller is done with the
    /// tree — it skips the whole-tree deep clone.
    pub fn snapshot(&mut self, root: &El, timings: &mut PrepareTimings) {
        self.snapshot_owned(root.clone(), timings);
    }

    /// Like [`Self::snapshot`] but takes ownership of the laid-out
    /// tree, avoiding the deep clone entirely. This is the per-frame
    /// path for the backends: the tree is rebuilt from scratch next
    /// frame anyway, so handing it over costs nothing.
    pub fn snapshot_owned(&mut self, root: El, timings: &mut PrepareTimings) {
        crate::profile_span!("prepare::snapshot");
        let t0 = Instant::now();
        self.last_tree_has_links = tree_has_links(&root);
        self.last_tree = Some(root);
        timings.snapshot = Instant::now() - t0;
    }

    /// Resolve the pointer cursor from the snapshot tree. Hosts call
    /// this right after `prepare` (which consumes the tree into the
    /// snapshot); paint-only frames keep the previous cursor.
    pub fn snapshot_cursor(&self) -> crate::cursor::Cursor {
        self.last_tree
            .as_ref()
            .map(|tree| self.ui_state.cursor(tree))
            .unwrap_or_default()
    }

    /// The snapshot tree, but only when it can possibly yield a link —
    /// the read side of the once-per-frame `last_tree_has_links` flag.
    /// Per-pointer-event `link_at` walks go through this so link-free
    /// apps skip the walk entirely.
    fn linkable_tree(&self) -> Option<&El> {
        self.last_tree.as_ref().filter(|_| self.last_tree_has_links)
    }
}

fn paint_rect_visible(
    rect: Rect,
    scissor: Option<Rect>,
    viewport_px: (u32, u32),
    scale_factor: f32,
) -> bool {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return false;
    }
    let scale = scale_factor.max(f32::EPSILON);
    let viewport = Rect::new(
        0.0,
        0.0,
        viewport_px.0 as f32 / scale,
        viewport_px.1 as f32 / scale,
    );
    let Some(clip) = scissor.map_or(Some(viewport), |s| s.intersect(viewport)) else {
        return false;
    };
    rect.intersect(clip).is_some()
}

fn target_id_in_subtree(root_id: &str, target_id: &str) -> bool {
    target_id == root_id
        || target_id
            .strip_prefix(root_id)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// Whether this op binds a shader whose output depends on `frame.time`.
/// Stock shaders self-report through
/// [`crate::shader::StockShader::is_continuous`]; custom shaders
/// answer through the host-supplied closure (which the backend wires
/// to its `samples_time=true` registration set). See
/// [`RunnerCore::prepare_layout`].
fn op_is_continuous<F>(op: &DrawOp, samples_time: &F) -> bool
where
    F: Fn(&ShaderHandle) -> bool,
{
    match op.shader() {
        Some(handle @ ShaderHandle::Stock(s)) => s.is_continuous() || samples_time(handle),
        Some(handle @ ShaderHandle::Custom(_)) => samples_time(handle),
        None => false,
    }
}

/// Vertical step inside an [`crate::tree::ArrowNav::Grid`] group:
/// from member `from`, find the geometrically nearest member whose
/// center sits strictly above (`dir < 0`) or below (`dir > 0`) — first
/// by vertical distance (nearest row), then by horizontal distance
/// (nearest column within it). Layout geometry rather than index
/// arithmetic keeps the walk correct when rows have gaps: unfocusable
/// cells (disabled days) simply aren't members. Returns `None` at the
/// grid's edge — the focus stays put, matching the linear modes'
/// saturating ends.
fn grid_vertical_step(members: &[UiTarget], from: usize, dir: f32) -> Option<usize> {
    let cur = &members[from].rect;
    let (cx, cy) = (cur.x + cur.w / 2.0, cur.y + cur.h / 2.0);
    members
        .iter()
        .enumerate()
        .filter(|(i, t)| {
            let ty = t.rect.y + t.rect.h / 2.0;
            // Strictly past the focused cell's own row in `dir`; half
            // the cell height as the row threshold tolerates sub-pixel
            // layout jitter without matching same-row neighbors.
            *i != from && (ty - cy) * dir > cur.h / 2.0
        })
        .min_by(|(_, a), (_, b)| {
            let key = |t: &UiTarget| {
                let tx = t.rect.x + t.rect.w / 2.0;
                let ty = t.rect.y + t.rect.h / 2.0;
                ((ty - cy).abs(), (tx - cx).abs())
            };
            key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
}

/// Find the `capture_keys` flag of the node whose `computed_id`
/// equals `id`, walking the laid-out tree. Returns `None` when the id
/// isn't found (the focused target outlived its node — a one-frame
/// race after a rebuild).
/// Whether any node in the tree carries a text-link run. Computed once
/// per [`RunnerCore::snapshot`] to gate the per-pointer-event
/// [`hit_test::link_at`] walks.
fn tree_has_links(node: &El) -> bool {
    node.text_link.is_some() || node.children.iter().any(tree_has_links)
}

pub(crate) fn find_capture_keys(node: &El, id: &str) -> Option<bool> {
    if node.computed_id == id.into() {
        return Some(node.capture_keys);
    }
    node.children.iter().find_map(|c| find_capture_keys(c, id))
}

/// Walk the tree looking for the node with `computed_id == id` and
/// return whether it (or any ancestor on the path to it) opted into
/// [`crate::tree::El::consumes_touch_drag`]. Returns `None` if the
/// id isn't in the tree.
///
/// Inheritance lets a compound widget mark its outer surface and
/// have presses on inner keyed children — a slider's thumb, the
/// number-scrubber's handle — also consume touch drag without each
/// piece needing to flip the flag.
fn find_consumes_touch_drag(node: &El, id: &str, ancestor_consumes: bool) -> Option<bool> {
    let consumes = ancestor_consumes || node.consumes_touch_drag;
    if node.computed_id == id.into() {
        return Some(consumes);
    }
    node.children
        .iter()
        .find_map(|c| find_consumes_touch_drag(c, id, consumes))
}

/// Walk the tree looking for the node with `computed_id == id` and
/// return whether it sits inside the subtree rooted at the node with
/// `computed_id == ancestor_id` (the ancestor itself counts). Returns
/// `None` if `id` isn't in the tree. Same shape as
/// [`find_consumes_touch_drag`], accumulating containment instead of a
/// flag.
fn find_inside_subtree(
    node: &El,
    ancestor_id: &str,
    id: &str,
    inside_ancestor: bool,
) -> Option<bool> {
    let inside = inside_ancestor || node.computed_id == ancestor_id.into();
    if node.computed_id == id.into() {
        return Some(inside);
    }
    node.children
        .iter()
        .find_map(|c| find_inside_subtree(c, ancestor_id, id, inside))
}

/// Construct a `SelectionChanged` event carrying the new selection.
fn selection_event(
    new_sel: crate::selection::Selection,
    modifiers: KeyModifiers,
    pointer: Option<(f32, f32)>,
    pointer_kind: Option<PointerKind>,
) -> UiEvent {
    UiEvent {
        kind: UiEventKind::SelectionChanged,
        key: None,
        target: None,
        pointer,
        key_press: None,
        text: None,
        selection: Some(new_sel),
        modifiers,
        click_count: 0,
        path: None,
        pointer_kind,
        wheel_delta: None,
    }
}

/// Resolve the head's [`SelectionPoint`] for the current pointer
/// position during a drag. Browser-style projection rules:
///
/// - If the pointer hits a selectable leaf, head goes there.
/// - Otherwise, head goes to the closest selectable leaf in document
///   order, with `(x, y)` projected onto that leaf's vertical extent.
///   Above all leaves → first leaf at byte 0; below all → last leaf
///   at end; in the gap between two adjacent leaves → whichever is
///   nearer in y.
/// - Horizontally outside the chosen leaf's text → snap to the
///   leaf's left edge (byte 0) or right edge (`text.len()`).
fn head_for_drag(
    root: &El,
    ui_state: &UiState,
    point: (f32, f32),
) -> Option<crate::selection::SelectionPoint> {
    if let Some(p) = hit_test::selection_point_at(root, point) {
        return Some(p);
    }

    let order = &ui_state.selection.order;
    if order.is_empty() {
        return None;
    }
    // Prefer a leaf whose vertical extent contains the pointer's y;
    // otherwise pick the y-closest leaf. min_by visits in document
    // order so ties (multiple leaves at the same y-distance) resolve
    // to the earliest one.
    let target = order
        .iter()
        .find(|t| point.1 >= t.rect.y && point.1 < t.rect.y + t.rect.h)
        .or_else(|| {
            order.iter().min_by(|a, b| {
                let da = y_distance(a.rect, point.1);
                let db = y_distance(b.rect, point.1);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
        })?;
    let target_rect = target.rect;
    let cy = point
        .1
        .clamp(target_rect.y, target_rect.y + target_rect.h - 1.0);
    if let Some(p) = hit_test::selection_point_at(root, (point.0, cy)) {
        return Some(p);
    }
    // Couldn't hit-test (likely because the pointer's x is outside
    // the leaf's rendered text width). Snap to the nearest edge.
    let leaf_len = find_text_len(root, &target.node_id).unwrap_or(0);
    let byte = if point.0 < target_rect.x { 0 } else { leaf_len };
    Some(crate::selection::SelectionPoint {
        key: target.key.clone(),
        byte,
    })
}

fn selection_range_for_drag(
    root: &El,
    ui_state: &UiState,
    drag: &crate::state::SelectionDrag,
    raw_head: crate::selection::SelectionPoint,
) -> (
    crate::selection::SelectionPoint,
    crate::selection::SelectionPoint,
) {
    match drag.granularity {
        SelectionDragGranularity::Character => (drag.anchor.clone(), raw_head),
        SelectionDragGranularity::Word => {
            let text = crate::selection::find_keyed_text(root, &raw_head.key).unwrap_or_default();
            let (lo, hi) = crate::selection::word_range_at(&text, raw_head.byte);
            if point_cmp(ui_state, &raw_head, &drag.anchor) == Ordering::Less {
                (
                    drag.head.clone(),
                    crate::selection::SelectionPoint::new(raw_head.key, lo),
                )
            } else {
                (
                    drag.anchor.clone(),
                    crate::selection::SelectionPoint::new(raw_head.key, hi),
                )
            }
        }
        SelectionDragGranularity::Leaf => {
            let len = crate::selection::find_keyed_text(root, &raw_head.key)
                .map(|text| text.len())
                .unwrap_or(raw_head.byte);
            if point_cmp(ui_state, &raw_head, &drag.anchor) == Ordering::Less {
                (
                    drag.head.clone(),
                    crate::selection::SelectionPoint::new(raw_head.key, 0),
                )
            } else {
                (
                    drag.anchor.clone(),
                    crate::selection::SelectionPoint::new(raw_head.key, len),
                )
            }
        }
    }
}

fn point_cmp(
    ui_state: &UiState,
    a: &crate::selection::SelectionPoint,
    b: &crate::selection::SelectionPoint,
) -> Ordering {
    let order_index = |key: &str| {
        ui_state
            .selection
            .order
            .iter()
            .position(|target| target.key == key)
            .unwrap_or(usize::MAX)
    };
    order_index(&a.key)
        .cmp(&order_index(&b.key))
        .then_with(|| a.byte.cmp(&b.byte))
}

fn y_distance(rect: Rect, y: f32) -> f32 {
    if y < rect.y {
        rect.y - y
    } else if y > rect.y + rect.h {
        y - (rect.y + rect.h)
    } else {
        0.0
    }
}

fn find_text_len(node: &El, id: &str) -> Option<usize> {
    if node.computed_id == id.into() {
        if let Some(source) = &node.selection_source {
            return Some(source.visible_len());
        }
        return node.text.as_ref().map(|t| t.len());
    }
    node.children.iter().find_map(|c| find_text_len(c, id))
}

/// Recorded output from an icon draw op. Backends without a vector-icon
/// path use `Text` fallback layers; wgpu can return dedicated icon runs.
pub enum RecordedPaint {
    Text(Range<usize>),
    Icon(Range<usize>),
}

/// Glyph-recording surface implemented by each backend's `TextPaint`.
/// `prepare_paint` calls into it exactly the same way wgpu and vulkano
/// would call their per-backend equivalents.
pub trait TextRecorder {
    /// Append per-glyph instances for `text` and return the range of
    /// indices written into the backend's `TextLayer` storage. Each
    /// returned index lands in `paint_items` as a `PaintItem::Text`.
    ///
    /// `style` carries weight + color + (optional) decoration flags
    /// — backends fold it into a single-element `(text, style)` slice
    /// and run the same shaping path as [`Self::record_runs`].
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        style: &RunStyle,
        text: &str,
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize>;

    /// Append per-glyph instances for an attributed paragraph (one
    /// shaped run with per-character RunStyle metadata). Wrapping
    /// decisions cross run boundaries — the result is one ShapedRun
    /// just like a single-style call.
    #[allow(clippy::too_many_arguments)]
    fn record_runs(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        runs: &[(String, RunStyle)],
        size: f32,
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize>;

    /// Append a vector icon. Backends with a native vector painter
    /// override this; the default keeps experimental/simple backends on
    /// the previous text-symbol fallback. Built-in icons fall back to
    /// their named glyph; app-supplied SVG icons fall back to a
    /// generic placeholder since they have no canonical glyph.
    #[allow(clippy::too_many_arguments)]
    fn record_icon(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &crate::icons::svg::IconSource,
        color: Color,
        size: f32,
        _stroke_width: f32,
        scale_factor: f32,
    ) -> RecordedPaint {
        let glyph = match source {
            crate::icons::svg::IconSource::Builtin(name) => name.fallback_glyph(),
            crate::icons::svg::IconSource::Custom(_) => "?",
            crate::icons::svg::IconSource::UnknownName(_) => {
                crate::tree::IconName::AlertCircle.fallback_glyph()
            }
        };
        RecordedPaint::Text(self.record(
            rect,
            scissor,
            &RunStyle::new(FontWeight::Regular, color),
            glyph,
            size,
            crate::text::metrics::line_height(size),
            TextWrap::NoWrap,
            TextAnchor::Middle,
            scale_factor,
        ))
    }

    /// Append a raster image draw. Backends with texture sampling
    /// override this and return one or more indices into their image
    /// storage (each index lands in `paint_items` as
    /// `PaintItem::Image`). The default returns an empty range —
    /// backends without raster support paint nothing for image Els
    /// (the SVG fallback emits a labelled placeholder rect on its own).
    #[allow(clippy::too_many_arguments)]
    fn record_image(
        &mut self,
        _rect: Rect,
        _scissor: Option<PhysicalScissor>,
        _image: &crate::image::Image,
        _tint: Option<Color>,
        _radius: crate::tree::Corners,
        _fit: crate::image::ImageFit,
        _range_limit: crate::image::DynamicRangeLimit,
        _scale_factor: f32,
    ) -> Range<usize> {
        0..0
    }

    /// Append an app-owned-texture composite. Backends with surface
    /// support override this and return one or more indices into their
    /// surface storage (each lands in `paint_items` as
    /// `PaintItem::AppTexture`). The default returns an empty range so
    /// backends without surface support paint nothing for surface Els.
    fn record_app_texture(
        &mut self,
        _rect: Rect,
        _scissor: Option<PhysicalScissor>,
        _texture: &crate::surface::AppTexture,
        _alpha: crate::surface::SurfaceAlpha,
        _transform: crate::affine::Affine2,
        _scale_factor: f32,
    ) -> Range<usize> {
        0..0
    }

    /// Append an app-supplied vector draw. Backends with vector
    /// support override this and return one or more indices into their
    /// vector storage (each lands in `paint_items` as
    /// `PaintItem::Vector`). The default returns an empty range so
    /// backends without vector support paint nothing.
    fn record_vector(
        &mut self,
        _rect: Rect,
        _scissor: Option<PhysicalScissor>,
        _asset: &crate::vector::VectorAsset,
        _render_mode: crate::vector::VectorRenderMode,
        _scale_factor: f32,
    ) -> Range<usize> {
        0..0
    }

    /// Append a 3D scene composite. Backends with a scene renderer
    /// override this: they ensure GPU buffers for the scene's
    /// revision-keyed geometry handles, ensure a per-node offscreen
    /// target sized to `rect` * `scale_factor`, and return one or more
    /// indices into their scene storage (each lands in `paint_items` as
    /// `PaintItem::Scene3D`). `id` is the node's stable id — backends key
    /// the offscreen-target cache on it so a resized or revisited scene
    /// reuses its target across frames. The default returns an empty
    /// range so backends without a scene renderer paint nothing (the SVG
    /// fallback emits a labelled placeholder rect on its own).
    fn record_scene3d(
        &mut self,
        _rect: Rect,
        _scissor: Option<PhysicalScissor>,
        _id: &str,
        _scene: &std::sync::Arc<crate::scene::Scene3DData>,
        _scale_factor: f32,
    ) -> Range<usize> {
        0..0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::PointerId;
    use crate::shader::{ShaderHandle, StockShader, UniformBlock};

    /// Minimal recorder for tests that don't exercise the text path.
    struct NoText;
    impl TextRecorder for NoText {
        fn record(
            &mut self,
            _rect: Rect,
            _scissor: Option<PhysicalScissor>,
            _style: &RunStyle,
            _text: &str,
            _size: f32,
            _line_height: f32,
            _wrap: TextWrap,
            _anchor: TextAnchor,
            _scale_factor: f32,
        ) -> Range<usize> {
            0..0
        }
        fn record_runs(
            &mut self,
            _rect: Rect,
            _scissor: Option<PhysicalScissor>,
            _runs: &[(String, RunStyle)],
            _size: f32,
            _line_height: f32,
            _wrap: TextWrap,
            _anchor: TextAnchor,
            _scale_factor: f32,
        ) -> Range<usize> {
            0..0
        }
    }

    #[derive(Default)]
    struct CountingText {
        records: usize,
    }

    impl TextRecorder for CountingText {
        fn record(
            &mut self,
            _rect: Rect,
            _scissor: Option<PhysicalScissor>,
            _style: &RunStyle,
            _text: &str,
            _size: f32,
            _line_height: f32,
            _wrap: TextWrap,
            _anchor: TextAnchor,
            _scale_factor: f32,
        ) -> Range<usize> {
            self.records += 1;
            0..0
        }

        fn record_runs(
            &mut self,
            _rect: Rect,
            _scissor: Option<PhysicalScissor>,
            _runs: &[(String, RunStyle)],
            _size: f32,
            _line_height: f32,
            _wrap: TextWrap,
            _anchor: TextAnchor,
            _scale_factor: f32,
        ) -> Range<usize> {
            self.records += 1;
            0..0
        }
    }

    fn empty_text_layout(line_height: f32) -> crate::text::metrics::TextLayout {
        crate::text::metrics::TextLayout {
            lines: Vec::new(),
            width: 0.0,
            height: 0.0,
            line_height,
        }
    }

    // ---- input plumbing ----

    /// A tree with one focusable button at (10,10,80,40) keyed "btn",
    /// plus an optional capture_keys text input at (10,60,80,40) keyed
    /// "ti". layout() runs against a 200x200 viewport so the rects
    /// land where we expect.
    fn lay_out_input_tree(capture: bool) -> RunnerCore {
        use crate::tree::*;
        let ti = if capture {
            crate::widgets::text::text("input").key("ti").capture_keys()
        } else {
            crate::widgets::text::text("noop").key("ti").focusable()
        };
        let mut tree =
            crate::column([crate::widgets::button::button("Btn").key("btn"), ti]).padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    #[test]
    fn pointer_up_emits_pointer_up_then_click() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_moved(Pointer::moving(cx, cy));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let events = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![UiEventKind::PointerUp, UiEventKind::Click]);
    }

    #[test]
    fn pointer_wheel_event_routes_to_keyed_target() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.center_x();
        let cy = btn_rect.center_y();

        let event = core
            .pointer_wheel_event(cx, cy, 0.0, 40.0)
            .expect("wheel over keyed button");

        assert_eq!(event.kind, UiEventKind::PointerWheel);
        assert_eq!(event.route(), Some("btn"));
        assert_eq!(event.pointer_pos(), Some((cx, cy)));
        assert_eq!(event.wheel_delta(), Some((0.0, 40.0)));
        assert_eq!(event.wheel_dy(), Some(40.0));
        assert_eq!(event.target_rect(), Some(btn_rect));
        assert_eq!(event.pointer_kind, Some(PointerKind::Mouse));
    }

    /// Build a tree containing a single inline paragraph with one
    /// linked run, layout to a fixed viewport, and return the runner +
    /// the absolute rect of the paragraph. The linked text is long
    /// enough that probes well into the paragraph land safely inside
    /// the link for any plausible proportional font.
    fn lay_out_link_tree() -> (RunnerCore, Rect, &'static str) {
        use crate::tree::*;
        const URL: &str = "https://github.com/computer-whisperer/damascene";
        let mut tree = crate::column([crate::text_runs([
            crate::text("Visit "),
            crate::text("github.com/computer-whisperer/damascene").link(URL),
            crate::text("."),
        ])])
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 600.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let para = core
            .last_tree
            .as_ref()
            .and_then(|t| t.children.first())
            .map(|p| p.computed_rect)
            .expect("paragraph rect");
        (core, para, URL)
    }

    #[test]
    fn snapshot_computes_the_links_flag_that_gates_link_walks() {
        // Link-free tree → flag off, so per-pointer-move `link_at`
        // walks are skipped entirely; any text-link run flips it on.
        let core = lay_out_input_tree(false);
        assert!(
            !core.last_tree_has_links,
            "input tree has no links; the flag must stay off"
        );
        let (core, ..) = lay_out_link_tree();
        assert!(
            core.last_tree_has_links,
            "linked paragraph must flip the flag on"
        );
    }

    #[test]
    fn pointer_up_on_link_emits_link_activated_with_url() {
        let (mut core, para, url) = lay_out_link_tree();
        // Probe ~100 logical pixels in — past the "Visit " prefix
        // (~40px in default UI font) and well inside the long linked
        // run, which extends ~250+px from there.
        let cx = para.x + 100.0;
        let cy = para.y + para.h * 0.5;
        core.pointer_moved(Pointer::moving(cx, cy));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let events = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let link = events
            .iter()
            .find(|e| e.kind == UiEventKind::LinkActivated)
            .expect("LinkActivated event");
        assert_eq!(link.key.as_deref(), Some(url));
    }

    #[test]
    fn pointer_up_after_drag_off_link_does_not_activate() {
        let (mut core, para, _url) = lay_out_link_tree();
        let press_x = para.x + 100.0;
        let cy = para.y + para.h * 0.5;
        core.pointer_moved(Pointer::moving(press_x, cy));
        core.pointer_down(Pointer::mouse(press_x, cy, PointerButton::Primary));
        // Release far below the paragraph — the user dragged off the
        // link before letting go, which native browsers treat as
        // cancel.
        let events = core.pointer_up(Pointer::mouse(press_x, 180.0, PointerButton::Primary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert!(
            !kinds.contains(&UiEventKind::LinkActivated),
            "drag-off-link should cancel the link activation; got {kinds:?}",
        );
    }

    #[test]
    fn disabled_control_resolves_not_allowed_cursor_on_hover() {
        // `.disabled()` declares Cursor::NotAllowed; the hit-test still
        // returns keyed nodes under `block_pointer` as hover targets
        // (only click routing is suppressed), so hovering a disabled
        // control must resolve the inert cursor (issue #71).
        use crate::cursor::Cursor;
        let mut tree = crate::widgets::button::button("Save")
            .key("save")
            .disabled();
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 100.0),
        );
        let rect = tree.computed_rect;
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core.pointer_moved(Pointer::moving(
            rect.x + rect.w * 0.5,
            rect.y + rect.h * 0.5,
        ));
        let snap = core.last_tree.as_ref().expect("tree").clone();
        assert_eq!(core.ui_state.cursor(&snap), Cursor::NotAllowed);
    }

    #[test]
    fn pointer_moved_over_link_resolves_cursor_to_pointer_and_requests_redraw() {
        use crate::cursor::Cursor;
        let (mut core, para, _url) = lay_out_link_tree();
        let cx = para.x + 100.0;
        let cy = para.y + para.h * 0.5;
        // Pointer initially well outside the paragraph.
        let initial = core.pointer_moved(Pointer::moving(para.x - 50.0, cy));
        assert!(
            !initial.needs_redraw,
            "moving in empty space shouldn't request a redraw"
        );
        let tree = core.last_tree.as_ref().expect("tree").clone();
        assert_eq!(
            core.ui_state.cursor(&tree),
            Cursor::Default,
            "no link under pointer → default cursor"
        );
        // Move onto the link — needs_redraw flips so the host
        // re-resolves the cursor on the next frame.
        let onto = core.pointer_moved(Pointer::moving(cx, cy));
        assert!(
            onto.needs_redraw,
            "entering a link region should flag a redraw so the cursor refresh isn't stale"
        );
        assert_eq!(
            core.ui_state.cursor(&tree),
            Cursor::Pointer,
            "pointer over a link → Pointer cursor"
        );
        // Move back off — should flag a redraw again so the cursor
        // returns to Default.
        let off = core.pointer_moved(Pointer::moving(para.x - 50.0, cy));
        assert!(
            off.needs_redraw,
            "leaving a link region should flag a redraw"
        );
        assert_eq!(core.ui_state.cursor(&tree), Cursor::Default);
    }

    #[test]
    fn pointer_up_on_unlinked_text_does_not_emit_link_activated() {
        let (mut core, para, _url) = lay_out_link_tree();
        // Click 1px in from the left edge — inside the "Visit "
        // prefix, before the linked run starts.
        let cx = para.x + 1.0;
        let cy = para.y + para.h * 0.5;
        core.pointer_moved(Pointer::moving(cx, cy));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let events = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert!(
            !kinds.contains(&UiEventKind::LinkActivated),
            "click on the unlinked prefix should not surface a link event; got {kinds:?}",
        );
    }

    #[test]
    fn pointer_up_off_target_emits_only_pointer_up() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        // Release off-target (well outside any keyed node).
        let events = core.pointer_up(Pointer::mouse(180.0, 180.0, PointerButton::Primary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![UiEventKind::PointerUp],
            "drag-off-target should still surface PointerUp so widgets see drag-end"
        );
    }

    #[test]
    fn pointer_moved_while_pressed_emits_drag() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let drag = core
            .pointer_moved(Pointer::moving(cx + 30.0, cy))
            .events
            .into_iter()
            .find(|e| e.kind == UiEventKind::Drag)
            .expect("drag while pressed");
        assert_eq!(drag.target.as_ref().map(|t| t.key.as_str()), Some("btn"));
        assert_eq!(drag.pointer, Some((cx + 30.0, cy)));
    }

    #[test]
    fn toast_dismiss_click_removes_toast_and_suppresses_click_event() {
        use crate::toast::ToastSpec;
        use crate::tree::Size;
        // Build a fresh runner, queue a toast, prepare once so the
        // toast layer is laid out, then synthesize a click on its
        // dismiss button.
        let mut core = RunnerCore::new();
        core.ui_state
            .push_toast(ToastSpec::success("hi"), Instant::now());
        let toast_id = core.ui_state.toasts()[0].id;

        // Build & lay out a tree with the toast layer appended.
        // Root is `stack(...)` (Axis::Overlay) so the synthesized
        // toast layer overlays rather than competing for flex space.
        let mut tree: El = crate::stack(std::iter::empty::<El>())
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        crate::layout::assign_ids(&mut tree);
        let _ = crate::toast::synthesize_toasts(&mut tree, &mut core.ui_state, Instant::now());
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 800.0, 600.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);

        let dismiss_key = format!("toast-dismiss-{toast_id}");
        let dismiss_rect = core.rect_of_key(&dismiss_key).expect("dismiss button");
        let cx = dismiss_rect.x + dismiss_rect.w * 0.5;
        let cy = dismiss_rect.y + dismiss_rect.h * 0.5;

        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let events = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        // PointerUp still fires (kept generic so drag-aware widgets
        // observe drag-end); Click is intercepted by the toast
        // bookkeeping.
        assert!(
            !kinds.contains(&UiEventKind::Click),
            "Click on toast-dismiss should not be surfaced: {kinds:?}",
        );
        // Dismissal marks the toast for its exit animation rather than
        // removing it; the synthesis pass drops it once the exit
        // window elapses.
        let toast = core
            .ui_state
            .toasts()
            .iter()
            .find(|t| t.id == toast_id)
            .expect("dismissed toast stays queued through its exit window");
        assert!(toast.dismissed, "dismiss-click marks the toast dismissed");

        let mut tree2: El = crate::stack(std::iter::empty::<El>())
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        crate::layout::assign_ids(&mut tree2);
        let exit_open = Instant::now();
        let _ = crate::toast::synthesize_toasts(&mut tree2, &mut core.ui_state, exit_open);
        let after = exit_open + crate::toast::TOAST_EXIT_WINDOW + Duration::from_millis(10);
        let mut tree3: El = crate::stack(std::iter::empty::<El>())
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        crate::layout::assign_ids(&mut tree3);
        let _ = crate::toast::synthesize_toasts(&mut tree3, &mut core.ui_state, after);
        assert!(
            core.ui_state.toasts().iter().all(|t| t.id != toast_id),
            "toast {toast_id} should leave the queue after its exit window",
        );
    }

    #[test]
    fn pointer_moved_without_press_emits_no_drag() {
        let mut core = lay_out_input_tree(false);
        let events = core.pointer_moved(Pointer::moving(50.0, 50.0)).events;
        // No press → no Drag emission. Hover-transition events
        // (PointerEnter/Leave) may fire; just assert nothing in the
        // out vec carries the Drag kind.
        assert!(!events.iter().any(|e| e.kind == UiEventKind::Drag));
    }

    #[test]
    fn spinner_in_tree_keeps_needs_redraw_set() {
        // stock::spinner reads frame.time, so the host must keep
        // calling prepare() even when no animation is in flight. Pin
        // the contract: a tree with no other motion still reports
        // needs_redraw=true when a spinner is present.
        use crate::widgets::spinner::spinner;
        let mut tree = crate::column([spinner()]);
        let mut core = RunnerCore::new();
        let mut t = PrepareTimings::default();
        let LayoutPrepared { needs_redraw, .. } = core.prepare_layout(
            &mut tree,
            Rect::new(0.0, 0.0, 200.0, 200.0),
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        assert!(
            needs_redraw,
            "tree with a spinner must request continuous redraw",
        );

        // Same shape without a spinner — needs_redraw stays false once
        // any state envelopes settle, demonstrating the signal is
        // spinner-driven rather than always-on.
        let mut bare = crate::column([crate::widgets::text::text("idle")]);
        let mut core2 = RunnerCore::new();
        let mut t2 = PrepareTimings::default();
        let LayoutPrepared {
            needs_redraw: needs_redraw2,
            ..
        } = core2.prepare_layout(
            &mut bare,
            Rect::new(0.0, 0.0, 200.0, 200.0),
            1.0,
            &mut t2,
            RunnerCore::no_time_shaders,
        );
        assert!(
            !needs_redraw2,
            "tree without time-driven shaders should idle: got needs_redraw={needs_redraw2}",
        );
    }

    #[test]
    fn custom_samples_time_shader_keeps_needs_redraw_set() {
        // Pin the generalization: a tree binding a *custom* shader
        // whose name appears in the host's `samples_time` set must
        // request continuous redraw the same way stock::spinner does.
        let mut tree = crate::column([crate::tree::El::new(crate::tree::Kind::Custom("anim"))
            .shader(crate::shader::ShaderBinding::custom("my_animated_glow"))
            .width(crate::tree::Size::Fixed(32.0))
            .height(crate::tree::Size::Fixed(32.0))]);
        let mut core = RunnerCore::new();
        let mut t = PrepareTimings::default();

        let LayoutPrepared {
            needs_redraw: idle, ..
        } = core.prepare_layout(
            &mut tree,
            Rect::new(0.0, 0.0, 200.0, 200.0),
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        assert!(
            !idle,
            "without a samples_time registration the host should idle",
        );

        let mut t2 = PrepareTimings::default();
        let LayoutPrepared {
            needs_redraw: animated,
            ..
        } = core.prepare_layout(
            &mut tree,
            Rect::new(0.0, 0.0, 200.0, 200.0),
            1.0,
            &mut t2,
            |handle| matches!(handle, ShaderHandle::Custom("my_animated_glow")),
        );
        assert!(
            animated,
            "custom shader registered as samples_time=true must request continuous redraw",
        );
    }

    #[test]
    fn redraw_within_aggregates_to_minimum_visible_deadline() {
        use std::time::Duration;
        let mut tree = crate::column([
            // 16ms
            crate::widgets::text::text("a")
                .redraw_within(Duration::from_millis(16))
                .width(crate::tree::Size::Fixed(20.0))
                .height(crate::tree::Size::Fixed(20.0)),
            // 50ms — the slower request should NOT win against 16ms.
            crate::widgets::text::text("b")
                .redraw_within(Duration::from_millis(50))
                .width(crate::tree::Size::Fixed(20.0))
                .height(crate::tree::Size::Fixed(20.0)),
        ]);
        let mut core = RunnerCore::new();
        let mut t = PrepareTimings::default();
        let LayoutPrepared {
            needs_redraw,
            next_layout_redraw_in,
            ..
        } = core.prepare_layout(
            &mut tree,
            Rect::new(0.0, 0.0, 200.0, 200.0),
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        assert!(needs_redraw, "redraw_within must lift the legacy bool");
        assert_eq!(
            next_layout_redraw_in,
            Some(Duration::from_millis(16)),
            "tightest visible deadline wins, on the layout lane",
        );
    }

    #[test]
    fn redraw_within_off_screen_widget_is_ignored() {
        use std::time::Duration;
        // Layout-rect-based visibility: place the animated widget below
        // the viewport via a tall preceding spacer in a hugging
        // column. The child's computed rect is at y≈150, which lies
        // outside a 0..100 viewport, so the visibility filter must
        // skip it and the host must idle.
        let mut tree = crate::column([
            crate::tree::spacer().height(crate::tree::Size::Fixed(150.0)),
            crate::widgets::text::text("offscreen")
                .redraw_within(Duration::from_millis(16))
                .width(crate::tree::Size::Fixed(10.0))
                .height(crate::tree::Size::Fixed(10.0)),
        ]);
        let mut core = RunnerCore::new();
        let mut t = PrepareTimings::default();
        let LayoutPrepared {
            next_layout_redraw_in,
            ..
        } = core.prepare_layout(
            &mut tree,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        assert_eq!(
            next_layout_redraw_in, None,
            "off-screen redraw_within must not contribute to the aggregate",
        );
    }

    #[test]
    fn redraw_within_clipped_out_widget_is_ignored() {
        use std::time::Duration;

        let clipped = crate::column([crate::widgets::text::text("clipped")
            .redraw_within(Duration::from_millis(16))
            .width(crate::tree::Size::Fixed(10.0))
            .height(crate::tree::Size::Fixed(10.0))])
        .clip()
        .width(crate::tree::Size::Fixed(100.0))
        .height(crate::tree::Size::Fixed(20.0))
        .layout(|ctx| {
            vec![Rect::new(
                ctx.container.x,
                ctx.container.y + 30.0,
                10.0,
                10.0,
            )]
        });
        let mut tree = crate::column([clipped]);

        let mut core = RunnerCore::new();
        let mut t = PrepareTimings::default();
        let LayoutPrepared {
            next_layout_redraw_in,
            ..
        } = core.prepare_layout(
            &mut tree,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        assert_eq!(
            next_layout_redraw_in, None,
            "redraw_within inside an inherited clip but outside the clip rect must not contribute",
        );
    }

    #[test]
    fn pointer_moved_within_same_hovered_node_does_not_request_redraw() {
        // Wayland delivers CursorMoved at very high frequency while
        // the cursor sits over the surface. Hosts gate request_redraw
        // on `needs_redraw`; this test pins the contract so we don't
        // regress to the unconditional-redraw behaviour that pegged
        // settings_modal at 100% CPU under cursor activity.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let (cx, cy) = (btn.x + btn.w * 0.5, btn.y + btn.h * 0.5);

        // First move enters the button — hover identity changes, so a
        // PointerEnter fires (no preceding Leave because no prior
        // hover target).
        let first = core.pointer_moved(Pointer::moving(cx, cy));
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].kind, UiEventKind::PointerEnter);
        assert_eq!(first.events[0].key.as_deref(), Some("btn"));
        assert!(
            first.needs_redraw,
            "entering a focusable should warrant a redraw",
        );

        // Same node, slightly different coords. Hover identity is
        // unchanged, no drag is active — must not redraw or emit any
        // events.
        let second = core.pointer_moved(Pointer::moving(cx + 1.0, cy));
        assert!(second.events.is_empty());
        assert!(
            !second.needs_redraw,
            "identical hover, no drag → host should idle",
        );

        // Moving off the button into empty space changes hover to
        // None — that's a visible transition (envelope ramps down)
        // and a PointerLeave fires.
        let off = core.pointer_moved(Pointer::moving(0.0, 0.0));
        assert_eq!(off.events.len(), 1);
        assert_eq!(off.events[0].kind, UiEventKind::PointerLeave);
        assert_eq!(off.events[0].key.as_deref(), Some("btn"));
        assert!(
            off.needs_redraw,
            "leaving a hovered node still warrants a redraw",
        );
    }

    #[test]
    fn pointer_move_within_hover_label_scene_keeps_redrawing() {
        // A scene with hover tooltips isn't a hover hit-target, so moving
        // *within* it doesn't change the hovered node — but the tooltip
        // tracks the cursor, so each move must still request a redraw (else
        // lazy rendering strands the tooltip until some other input fires).
        use crate::scene::glam::Vec3;
        use crate::scene::{
            PointData, PointLabels, PointStyle, PointsHandle, ScenePoint, SceneSpec,
        };
        let pts = PointsHandle::new(PointData {
            points: vec![ScenePoint {
                position: Vec3::ZERO,
                color: [1.0; 4],
            }],
        });
        let spec = SceneSpec::new().points_labeled(
            pts,
            PointStyle::default(),
            PointLabels::new(["a"]).on_hover(),
        );
        let mut tree = crate::tree::chart3d(spec).key("scene");
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        // prepare_layout does this each frame; here we drive it directly to
        // populate the hover-label rect list.
        core.ui_state.tick_scene_cameras(&tree, Instant::now());

        let scene = core.rect_of_key("scene").expect("scene rect");
        let (cx, cy) = (scene.center_x(), scene.center_y());

        let enter = core.pointer_moved(Pointer::moving(cx, cy));
        assert!(enter.needs_redraw, "entering a hover-label scene redraws");
        // Same hovered node (the scene), different coords → must still redraw
        // so the tooltip follows the cursor.
        let within = core.pointer_moved(Pointer::moving(cx + 3.0, cy - 2.0));
        assert!(
            within.needs_redraw,
            "moving within a hover-label scene keeps redrawing the tooltip",
        );
        // Leaving the scene redraws once more to clear the tooltip.
        let leave = core.pointer_moved(Pointer::moving(scene.x + scene.w + 20.0, cy));
        assert!(leave.needs_redraw, "leaving clears the tooltip");
    }

    #[test]
    fn pointer_moved_between_keyed_targets_emits_leave_then_enter() {
        // Cursor crossing from one keyed node to another emits a paired
        // PointerLeave (old target) followed by PointerEnter (new
        // target). Apps can observe the cleared state before the new
        // one — important for things like cancelling a hover-intent
        // prefetch on the old target before kicking off one for the
        // new.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let ti = core.rect_of_key("ti").expect("ti rect");

        // Enter btn first.
        let _ = core.pointer_moved(Pointer::moving(btn.x + 4.0, btn.y + 4.0));

        // Cross to ti.
        let cross = core.pointer_moved(Pointer::moving(ti.x + 4.0, ti.y + 4.0));
        let kinds: Vec<UiEventKind> = cross.events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![UiEventKind::PointerLeave, UiEventKind::PointerEnter],
            "paired Leave-then-Enter on cross-target hover transition",
        );
        assert_eq!(cross.events[0].key.as_deref(), Some("btn"));
        assert_eq!(cross.events[1].key.as_deref(), Some("ti"));
        assert!(cross.needs_redraw);
    }

    #[test]
    fn touch_pointer_down_emits_pointer_enter_then_pointer_down() {
        // A touch tap has no preceding `pointer_moved` (most platforms
        // only fire pointermove during contact), so `pointer_down`
        // itself synthesizes the `PointerEnter` that mouse hosts get
        // for free. Without this, hover-driven button visuals would
        // never wake up for the duration of the contact.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let events = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![UiEventKind::PointerEnter, UiEventKind::PointerDown],
        );
        for e in &events {
            assert_eq!(e.pointer_kind, Some(PointerKind::Touch));
        }
        assert_eq!(core.ui_state().hovered_key(), Some("btn"));
    }

    #[test]
    fn touch_pointer_up_emits_pointer_leave_after_click() {
        // Releasing a touch ends the gesture's hover, mirroring the
        // synthetic enter on `pointer_down`. Mouse / pen leave hover
        // tracking continuous; touch must wind down explicitly so
        // hover envelopes don't latch on after release.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let events = core.pointer_up(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                UiEventKind::PointerUp,
                UiEventKind::Click,
                UiEventKind::PointerLeave,
            ],
        );
        assert_eq!(core.ui_state().hovered_key(), None);
    }

    #[test]
    fn touch_pointer_moved_without_press_does_not_emit_hover_transitions() {
        // A touch-modality `pointer_moved` with no active contact
        // (synthetic, mostly — real touch hardware doesn't fire move
        // without contact) must not synthesize a hover transition.
        // Without this guard, an Apple Pencil hovering over the
        // canvas would still drive button hover visuals without ever
        // touching, which is the wrong default — pen sets its own
        // `PointerKind::Pen` so it falls through to mouse semantics.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let mut p = Pointer::moving(btn.x + 4.0, btn.y + 4.0);
        p.kind = PointerKind::Touch;
        let moved = core.pointer_moved(p);
        assert!(
            moved.events.is_empty(),
            "touch move without press should not emit hover events, got {:?}",
            moved.events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn touch_drag_between_targets_still_emits_hover_transitions() {
        // Mid-drag identity changes (finger sliding from one keyed
        // node to another) ARE real hover transitions on touch — the
        // hover gating only suppresses move-without-press, not move-
        // with-press. Widgets along the drag path get the same enter
        // / leave they would on mouse, in the same order.
        //
        // Premise: the press target opts into `consumes_touch_drag`
        // so the touch gesture commits to drag (not scroll). Without
        // that opt-in the runner cancels the press and routes the
        // motion to scroll, which is covered by a separate test.
        use crate::tree::*;
        let mut tree = crate::column([
            crate::widgets::button::button("Btn")
                .key("btn")
                .consumes_touch_drag(),
            crate::widgets::button::button("Other").key("other"),
        ])
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);

        let btn = core.rect_of_key("btn").expect("btn rect");
        let other = core.rect_of_key("other").expect("other rect");
        let _ = core.pointer_down(Pointer::touch(
            btn.x + 4.0,
            btn.y + 4.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let mut move_p = Pointer::moving(other.x + 4.0, other.y + 4.0);
        move_p.kind = PointerKind::Touch;
        let cross = core.pointer_moved(move_p);
        let kinds: Vec<UiEventKind> = cross.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::PointerLeave)
                && kinds.contains(&UiEventKind::PointerEnter),
            "touch drag across targets should emit Leave + Enter, got {kinds:?}",
        );
        // Drag also fires because the press is still held on btn
        // (consumes_touch_drag commits the gesture to drag rather
        // than scroll).
        assert!(kinds.contains(&UiEventKind::Drag));
    }

    #[test]
    fn would_press_focus_text_input_distinguishes_capture_keys() {
        // The capture-keys variant of `lay_out_input_tree` keys a
        // text-input style widget at "ti"; the non-capture variant
        // keys a plain focusable. The query distinguishes the two
        // by walking find_capture_keys against the hit target.
        let core = lay_out_input_tree(true);
        let ti = core.rect_of_key("ti").expect("ti rect");
        let btn = core.rect_of_key("btn").expect("btn rect");

        assert!(
            core.would_press_focus_text_input(ti.center_x(), ti.center_y()),
            "press on capture_keys widget should report true",
        );
        assert!(
            !core.would_press_focus_text_input(btn.center_x(), btn.center_y()),
            "press on plain focusable should report false",
        );
        // Press in dead space → false (no hit target).
        assert!(!core.would_press_focus_text_input(0.0, 0.0));
    }

    #[test]
    fn touch_jiggle_below_threshold_still_taps() {
        // Real touch contact has small involuntary movement between
        // pointer_down and pointer_up. As long as the total motion
        // stays under TOUCH_DRAG_THRESHOLD the gesture must remain a
        // tap — Click fires on release just like a perfectly still
        // press.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        // Jiggle by a few pixels — well under the 10px threshold.
        let mut jiggle = Pointer::moving(cx + 3.0, cy + 2.0);
        jiggle.kind = PointerKind::Touch;
        let _ = core.pointer_moved(jiggle);
        let events = core.pointer_up(Pointer::touch(
            cx + 3.0,
            cy + 2.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Click),
            "small jiggle should not commit to scroll, expected Click in {kinds:?}",
        );
    }

    #[test]
    fn touch_drag_on_consuming_widget_emits_drag_not_cancel() {
        // A press on a node opted into `consumes_touch_drag` (slider,
        // scrubber, resize handle) commits the gesture to drag once
        // the threshold is crossed, so subsequent moves emit the
        // normal `Drag` event and the press is *not* cancelled.
        use crate::tree::*;
        let mut tree = crate::column([crate::widgets::button::button("Drag me")
            .key("draggable")
            .consumes_touch_drag()])
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);

        let r = core.rect_of_key("draggable").expect("rect");
        let cx = r.x + r.w * 0.5;
        let cy = r.y + r.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        // Move past the threshold along x (still inside the widget
        // since the test widget is wide).
        let mut over = Pointer::moving(cx + 30.0, cy);
        over.kind = PointerKind::Touch;
        let moved = core.pointer_moved(over);
        let kinds: Vec<UiEventKind> = moved.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Drag),
            "drag-consuming widget should receive Drag past threshold, got {kinds:?}",
        );
        assert!(
            !kinds.contains(&UiEventKind::PointerCancel),
            "drag-consuming widget should not see PointerCancel, got {kinds:?}",
        );
    }

    #[test]
    fn touch_drag_in_scrollable_cancels_press_and_scrolls() {
        // Press on non-draggable content inside a scroll region:
        // crossing the threshold commits to scroll, which means
        // (a) the press is cancelled (PointerCancel + PointerLeave
        // for the pressed/hovered targets), (b) the scroll offset
        // advances by the move delta, and (c) the subsequent
        // pointer_up does NOT fire Click.
        use crate::tree::*;
        let mut tree = crate::scroll([
            crate::widgets::button::button("row 0")
                .key("row0")
                .height(Size::Fixed(50.0)),
            crate::widgets::button::button("row 1")
                .key("row1")
                .height(Size::Fixed(50.0)),
            crate::widgets::button::button("row 2")
                .key("row2")
                .height(Size::Fixed(50.0)),
            crate::widgets::button::button("row 3")
                .key("row3")
                .height(Size::Fixed(50.0)),
            crate::widgets::button::button("row 4")
                .key("row4")
                .height(Size::Fixed(50.0)),
        ])
        .key("list")
        .height(Size::Fixed(120.0));
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 120.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let scroll_id = core
            .last_tree
            .as_ref()
            .map(|t| t.computed_id.clone())
            .expect("scroll id");

        // Press inside row1, near the middle of the viewport, so a
        // 40px upward drag still lands inside the scrollable region
        // — `pointer_wheel` only routes when the up-finger position
        // is inside a scrollable's rect.
        let row1 = core.rect_of_key("row1").expect("row1");
        let cx = row1.x + row1.w * 0.5;
        let cy = row1.y + row1.h * 0.5;

        // Press on row1.
        let down_events = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        // Sanity: PointerDown was emitted.
        assert!(
            down_events
                .iter()
                .any(|e| matches!(e.kind, UiEventKind::PointerDown)),
            "expected PointerDown on press",
        );

        // Drag finger upward by 40px (past the 10px threshold). Sign
        // convention: finger moving up = positive scroll delta =
        // content scrolls down (offset increases).
        let mut up_finger = Pointer::moving(cx, cy - 40.0);
        up_finger.kind = PointerKind::Touch;
        let move_events = core.pointer_moved(up_finger);
        let kinds: Vec<UiEventKind> = move_events.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::PointerCancel),
            "scroll commit should fire PointerCancel, got {kinds:?}",
        );
        assert!(
            !kinds.contains(&UiEventKind::Drag),
            "scroll commit should NOT emit Drag, got {kinds:?}",
        );

        // Scroll offset advanced by ~the finger delta (40px).
        let offset = core.ui_state().scroll_offset(&scroll_id);
        assert!(
            offset > 30.0 && offset <= 50.0,
            "scroll offset should advance ~40px after a 40px finger drag, got {offset}",
        );

        // Releasing now does NOT fire Click — the press was already
        // cancelled, so pointer_up returns nothing app-facing.
        let up_events = core.pointer_up(Pointer::touch(
            cx,
            cy - 40.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let up_kinds: Vec<UiEventKind> = up_events.iter().map(|e| e.kind).collect();
        assert!(
            !up_kinds.contains(&UiEventKind::Click),
            "scroll-committed gesture must not fire Click on release, got {up_kinds:?}",
        );
    }

    #[test]
    fn touch_scroll_release_starts_momentum_after_fast_swipe_outside_viewport() {
        use crate::tree::*;
        let mut tree = crate::scroll((0..20).map(|i| {
            crate::widgets::button::button(format!("row {i}"))
                .key(format!("row{i}"))
                .height(Size::Fixed(50.0))
        }))
        .key("list")
        .height(Size::Fixed(120.0));
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 120.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let scroll_id = core
            .last_tree
            .as_ref()
            .map(|t| t.computed_id.clone())
            .expect("scroll id");

        let row1 = core.rect_of_key("row1").expect("row1");
        let cx = row1.x + row1.w * 0.5;
        let cy = row1.y + row1.h * 0.5;

        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let mut up_finger = Pointer::moving(cx, cy - 80.0);
        up_finger.kind = PointerKind::Touch;
        core.pointer_moved(up_finger);
        let before_release = core.ui_state().scroll_offset(&scroll_id);
        core.pointer_up(Pointer::touch(
            cx,
            cy - 80.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));

        assert!(
            core.ui_state.has_scroll_momentum(),
            "quick touch scroll release should retain inertial velocity"
        );
        assert_eq!(
            core.next_input_deadline(Instant::now()),
            Some(Duration::ZERO),
            "active scroll momentum should request the next layout frame"
        );

        let ticked = core
            .ui_state
            .tick_scroll_momentum(Instant::now() + Duration::from_millis(16));
        let after_tick = core.ui_state().scroll_offset(&scroll_id);
        assert!(ticked, "momentum tick should report visual work");
        assert!(
            after_tick > before_release,
            "momentum should continue in release direction: before={before_release}, after={after_tick}"
        );
    }

    #[test]
    fn pointer_left_emits_leave_for_prior_hover() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let _ = core.pointer_moved(Pointer::moving(btn.x + 4.0, btn.y + 4.0));

        let events = core.pointer_left();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, UiEventKind::PointerLeave);
        assert_eq!(events[0].key.as_deref(), Some("btn"));
    }

    #[test]
    fn pointer_left_with_no_prior_hover_emits_nothing() {
        let mut core = lay_out_input_tree(false);
        // No prior pointer_moved into a keyed target — pointer_left
        // should be a no-op event-wise (state still gets cleared).
        let events = core.pointer_left();
        assert!(events.is_empty());
    }

    #[test]
    fn poll_input_before_long_press_delay_emits_nothing() {
        // A held touch that hasn't yet crossed LONG_PRESS_DELAY
        // should not produce a long-press event when polled.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        // 100ms < 500ms — too early.
        let polled = core.poll_input(Instant::now() + Duration::from_millis(100));
        assert!(polled.is_empty(), "should not fire before delay");
    }

    #[test]
    fn poll_input_after_long_press_delay_fires_cancel_then_long_press() {
        // After holding past LONG_PRESS_DELAY, poll_input emits
        // PointerCancel (cleaning up the in-flight press) followed by
        // a LongPress event keyed to the originally pressed target.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let polled = core.poll_input(Instant::now() + LONG_PRESS_DELAY + Duration::from_millis(10));
        let kinds: Vec<UiEventKind> = polled.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::PointerCancel),
            "expected PointerCancel before LongPress, got {kinds:?}",
        );
        let long_press = polled
            .iter()
            .find(|e| matches!(e.kind, UiEventKind::LongPress))
            .expect("LongPress event missing");
        assert_eq!(
            long_press.key.as_deref(),
            Some("btn"),
            "LongPress should target the originally pressed node",
        );
        assert_eq!(
            long_press.pointer_kind,
            Some(PointerKind::Touch),
            "LongPress is touch-only",
        );
    }

    #[test]
    fn touch_long_press_on_editable_preserves_drag_extension() {
        let mut core = lay_out_input_tree(true);
        let ti = core.rect_of_key("ti").expect("ti rect");
        let cx = ti.x + 4.0;
        let cy = ti.y + ti.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));

        let polled = core.poll_input(Instant::now() + LONG_PRESS_DELAY + Duration::from_millis(10));
        assert!(
            polled.iter().any(|e| e.kind == UiEventKind::LongPress),
            "editable long-press should emit LongPress"
        );
        assert!(
            !polled.iter().any(|e| e.kind == UiEventKind::PointerCancel),
            "editable long-press keeps the press captured so drag can extend selection"
        );

        let mut moved = Pointer::moving(cx + 40.0, cy);
        moved.kind = PointerKind::Touch;
        let drag = core.pointer_moved(moved);
        assert!(
            drag.events.iter().any(|e| e.kind == UiEventKind::Drag),
            "held touch move after editable long-press should emit Drag"
        );

        let up_events = core.pointer_up(Pointer::touch(
            cx + 40.0,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        assert!(
            up_events.is_empty(),
            "long-press release should not synthesize click or pointer-up"
        );
    }

    #[test]
    fn pointer_up_after_long_press_emits_no_click() {
        // Once the long-press fires, lifting the finger silently
        // releases — no Click, no PointerUp routed to the original
        // target.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let _ = core.poll_input(Instant::now() + LONG_PRESS_DELAY + Duration::from_millis(10));
        let up_events = core.pointer_up(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        assert!(
            up_events.is_empty(),
            "lift after long-press emits nothing, got {:?}",
            up_events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn moving_past_threshold_before_long_press_cancels_the_timer() {
        // A drift past TOUCH_DRAG_THRESHOLD before the long-press
        // deadline commits the gesture (to scroll or drag), which
        // means the long-press should NOT fire even when we later
        // poll past LONG_PRESS_DELAY.
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        let _ = core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        // Move 30px past threshold — gesture commits.
        let mut over = Pointer::moving(cx + 30.0, cy);
        over.kind = PointerKind::Touch;
        let _ = core.pointer_moved(over);
        // Poll well past the long-press deadline — should be empty.
        let polled = core.poll_input(Instant::now() + LONG_PRESS_DELAY + Duration::from_millis(10));
        assert!(
            polled.is_empty(),
            "long-press should not fire after gesture committed",
        );
    }

    // ---- latent viewport pan: click-vs-pan arbitration (issue #125) ----

    /// Lay out a viewport tree against 400x300 and return the runner.
    fn lay_out_viewport_tree(mut tree: El) -> RunnerCore {
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        core.ui_state.sync_focus_order(&tree);
        core.ui_state.sync_selection_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    /// A default-trigger, free-bounds viewport keyed "vp" around `child`.
    /// The child hugs its fixed size at the origin, so everything else
    /// inside 400x300 is viewport background.
    fn free_viewport(child: El) -> El {
        crate::tree::viewport([child])
            .key("vp")
            .pan_bounds(crate::viewport::PanBounds::Free)
    }

    /// A 120x60 keyed click target — the "flow card" of the issue.
    fn vp_card() -> El {
        use crate::tree::*;
        crate::widgets::button::button("Card")
            .key("card")
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(60.0))
    }

    #[test]
    fn latent_pan_sub_slop_release_still_clicks() {
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        // Jiggle under MOUSE_DRAG_SLOP — still a click candidate.
        core.pointer_moved(Pointer::moving(cx + 2.0, cy + 1.0));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "sub-slop motion must not begin a pan"
        );
        let events = core.pointer_up(Pointer::mouse(cx + 2.0, cy + 1.0, PointerButton::Primary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Click),
            "sub-slop release must still click, got {kinds:?}"
        );
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert_eq!(
            view.pan,
            (0.0, 0.0),
            "sub-slop jiggle must not move the pan"
        );
    }

    #[test]
    fn latent_pan_converts_past_slop_cancels_click_and_anchors_at_press() {
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let moved = core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        let kinds: Vec<UiEventKind> = moved.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::PointerCancel),
            "conversion must cancel the pending click, got {kinds:?}"
        );
        assert!(moved.needs_redraw, "conversion clears press styling");
        assert!(core.ui_state.viewport_pan_active());
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        // Anchored at the press point: the full 20px lands in the pan —
        // no motion lost to the slop.
        assert!(
            (view.pan.0 - 20.0).abs() < 0.01 && view.pan.1.abs() < 0.01,
            "pan should track the full press-to-cursor delta, got {:?}",
            view.pan
        );
        // Further motion keeps driving the pan from the same anchor.
        core.pointer_moved(Pointer::moving(cx + 30.0, cy + 5.0));
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert!(
            (view.pan.0 - 30.0).abs() < 0.01 && (view.pan.1 - 5.0).abs() < 0.01,
            "pan should keep tracking, got {:?}",
            view.pan
        );
        let up = core.pointer_up(Pointer::mouse(cx + 30.0, cy + 5.0, PointerButton::Primary));
        assert!(
            up.is_empty(),
            "a converted pan releases without click, got {:?}",
            up.iter().map(|e| e.kind).collect::<Vec<_>>()
        );
        assert!(!core.ui_state.viewport_pan_active());
    }

    #[test]
    fn viewport_background_press_still_pans_immediately() {
        // Regression guard: the press-time rule for background presses
        // is untouched by the latent arbitration — no slop needed.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        core.pointer_down(Pointer::mouse(300.0, 250.0, PointerButton::Primary));
        assert!(
            core.ui_state.viewport_pan_active(),
            "background press must begin the pan at press time"
        );
    }

    #[test]
    fn dedicated_trigger_still_pans_over_child_at_press() {
        // A non-primary trigger can't collide with child clicks, so it
        // pans immediately even over the card — no latent conversion.
        let mut core =
            lay_out_viewport_tree(free_viewport(vp_card()).pan_button(PointerButton::Secondary));
        let r = core.rect_of_key("card").expect("card rect");
        core.pointer_down(Pointer::mouse(
            r.x + r.w * 0.5,
            r.y + r.h * 0.5,
            PointerButton::Secondary,
        ));
        assert!(
            core.ui_state.viewport_pan_active(),
            "dedicated trigger must pan at press time even over a child"
        );
    }

    #[test]
    fn latent_pan_yields_to_drag_consuming_child() {
        // A widget that owns its drag (slider-style) keeps it: motion
        // past the slop stays a Drag to the child, never a pan.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card().consumes_touch_drag()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let moved = core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "drag-consumer keeps its drag"
        );
        let kinds: Vec<UiEventKind> = moved.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Drag) && !kinds.contains(&UiEventKind::PointerCancel),
            "child should keep receiving Drag, got {kinds:?}"
        );
    }

    #[test]
    fn latent_pan_yields_to_capture_keys_child() {
        // Drag inside an editable widget is selection extension — a
        // text input inside a viewport must not turn drags into pans.
        let ti = crate::widgets::text::text("input").key("ti").capture_keys();
        let mut core = lay_out_viewport_tree(free_viewport(ti));
        let r = core.rect_of_key("ti").expect("ti rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let moved = core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "editable keeps its drag"
        );
        let kinds: Vec<UiEventKind> = moved.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Drag) && !kinds.contains(&UiEventKind::PointerCancel),
            "input should keep receiving Drag, got {kinds:?}"
        );
    }

    #[test]
    fn latent_pan_yields_to_text_selection_drag() {
        // A press that starts a static-text selection keeps selecting;
        // dragging across selectable text inside a viewport must not
        // convert into a pan.
        let p = crate::widgets::text::text("Some selectable paragraph text.")
            .key("p")
            .selectable();
        let mut core = lay_out_viewport_tree(free_viewport(p));
        let r = core.rect_of_key("p").expect("p rect");
        let (cx, cy) = (r.x + 4.0, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert!(
            core.ui_state.selection.drag.is_some(),
            "press starts selection"
        );
        core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "selection drag must keep priority over the pan"
        );
    }

    #[test]
    fn interleaved_pan_release_does_not_strand_the_latent_pan() {
        // Two viewports side by side: A pans on a dedicated secondary
        // trigger, B on the default. Hold a secondary pan on A, press
        // primary on B's card (arms a latent pan), release primary. The
        // latent must disarm on that release (issue #125), the release
        // must land as the card's click rather than being swallowed by
        // A's capture, and A's pan must survive until its own button
        // releases (issue #128). Afterwards, a button-less hover move
        // must neither convert a stale latent nor drag a stale press.
        use crate::tree::*;
        let vp_a = crate::tree::viewport([crate::widgets::button::button("A")
            .key("card_a")
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))])
        .key("vpa")
        .pan_button(PointerButton::Secondary)
        .pan_bounds(crate::viewport::PanBounds::Free);
        let mut core = lay_out_viewport_tree(crate::row([vp_a, free_viewport(vp_card())]));
        // Secondary press over A (dedicated: pans from anywhere over it).
        core.pointer_down(Pointer::mouse(100.0, 250.0, PointerButton::Secondary));
        assert!(core.ui_state.viewport_pan_active(), "dedicated pan on A");
        // Primary press on B's card while the pan is held — arms the latent.
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        // Primary release: A's capture must not swallow it — the sub-slop
        // press-release is the card's click, and A keeps panning.
        let up = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let up_kinds: Vec<UiEventKind> = up.iter().map(|e| e.kind).collect();
        assert!(
            up_kinds.contains(&UiEventKind::Click),
            "primary release clicks the card despite A's pan, got {up_kinds:?}"
        );
        assert!(
            core.ui_state.viewport_pan_active(),
            "A's pan survives the unrelated primary release"
        );
        core.pointer_up(Pointer::mouse(100.0, 250.0, PointerButton::Secondary));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "A's own release ends it"
        );
        // All buttons up. A hover move past the slop must not convert.
        let moved = core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "no button held — a stale latent pan must not begin a drag"
        );
        let kinds: Vec<UiEventKind> = moved.events.iter().map(|e| e.kind).collect();
        assert!(
            !kinds.contains(&UiEventKind::PointerCancel),
            "no phantom press-cancel on a button-less move, got {kinds:?}"
        );
        assert!(
            !kinds.contains(&UiEventKind::Drag),
            "no stale press keeps emitting Drag after release, got {kinds:?}"
        );
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert_eq!(view.pan, (0.0, 0.0), "B must not have panned");
    }

    #[test]
    fn unrelated_release_does_not_end_a_viewport_pan() {
        // Issue #128: a secondary click mid-drag must not kill a
        // primary-initiated pan — only the initiating button ends it.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        core.pointer_down(Pointer::mouse(300.0, 250.0, PointerButton::Primary));
        assert!(core.ui_state.viewport_pan_active(), "background pan begins");
        core.pointer_down(Pointer::mouse(300.0, 250.0, PointerButton::Secondary));
        core.pointer_up(Pointer::mouse(300.0, 250.0, PointerButton::Secondary));
        assert!(
            core.ui_state.viewport_pan_active(),
            "pan survives an unrelated button's release"
        );
        // Motion keeps driving the pan…
        core.pointer_moved(Pointer::moving(320.0, 250.0));
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert!(
            (view.pan.0 - 20.0).abs() < 0.01,
            "still panning: {:?}",
            view.pan
        );
        // …until the initiating button releases.
        core.pointer_up(Pointer::mouse(320.0, 250.0, PointerButton::Primary));
        assert!(!core.ui_state.viewport_pan_active());
    }

    #[test]
    fn latent_pan_conversion_resets_the_multi_click_chain() {
        // MULTI_CLICK_DIST is as wide as the slop: without the reset, a
        // press → convert-to-pan → release → re-press near the original
        // point inside the double-click window would count as click 2.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        assert!(core.ui_state.viewport_pan_active(), "converted");
        core.pointer_up(Pointer::mouse(cx + 20.0, cy, PointerButton::Primary));
        // Immediate re-press at the original point: a fresh click 1.
        let events = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let down = events
            .iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .expect("PointerDown on re-press");
        assert_eq!(
            down.click_count, 1,
            "a press that became a pan must not seed a double-click"
        );
    }

    /// A touch move (host cursor-move events carry no button).
    fn touch_move(x: f32, y: f32) -> Pointer {
        let mut p = Pointer::moving(x, y);
        p.kind = PointerKind::Touch;
        p
    }

    #[test]
    fn touch_drag_on_viewport_child_pans_past_threshold() {
        // Issue #126: the touch analog of the mouse latent pan — a drag
        // starting on a keyed child of a default-trigger viewport
        // commits to a viewport pan (anchored at the initial contact),
        // not to scroll routing that has nothing to scroll.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let moved = core.pointer_moved(touch_move(cx + 30.0, cy));
        let kinds: Vec<UiEventKind> = moved.events.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::PointerCancel),
            "commit cancels the pending tap, got {kinds:?}"
        );
        assert!(core.ui_state.viewport_pan_active(), "drag commits to a pan");
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert!(
            (view.pan.0 - 30.0).abs() < 0.01 && view.pan.1.abs() < 0.01,
            "anchored at the initial contact — full delta lands, got {:?}",
            view.pan
        );
        let up = core.pointer_up(Pointer::touch(
            cx + 30.0,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        assert!(
            up.is_empty(),
            "a committed pan releases without click, got {:?}",
            up.iter().map(|e| e.kind).collect::<Vec<_>>()
        );
        assert!(!core.ui_state.viewport_pan_active());
    }

    #[test]
    fn touch_tap_on_viewport_child_still_clicks() {
        // Sub-threshold contact stays a tap — the viewport arbitration
        // must not eat taps on cards.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(cx + 3.0, cy));
        let up = core.pointer_up(Pointer::touch(
            cx + 3.0,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let kinds: Vec<UiEventKind> = up.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Click),
            "tap clicks, got {kinds:?}"
        );
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert_eq!(view.pan, (0.0, 0.0), "tap must not pan");
    }

    #[test]
    fn touch_drag_over_scrollable_inside_viewport_scrolls_it() {
        // A scrollable *inside* the viewport content keeps the drag
        // (mobile convention: a list laid over a map scrolls; the map
        // pans from anywhere else).
        use crate::tree::*;
        let rows: Vec<El> = (0..20)
            .map(|i| {
                crate::widgets::button::button("Row")
                    .key(format!("row{i}"))
                    .width(Size::Fixed(160.0))
                    .height(Size::Fixed(40.0))
            })
            .collect();
        let list = crate::scroll(rows)
            .key("list")
            .width(Size::Fixed(180.0))
            .height(Size::Fixed(120.0));
        let mut core = lay_out_viewport_tree(free_viewport(list));
        let list_id = core
            .ui_state
            .layout
            .key_index
            .get("list")
            .expect("list id")
            .to_string();
        // Vertical drag over the list body.
        core.pointer_down(Pointer::touch(
            90.0,
            80.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(90.0, 40.0));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "the inner scrollable wins the vertical drag"
        );
        assert!(
            matches!(
                core.ui_state.touch_gesture,
                TouchGestureState::Scrolling { .. }
            ),
            "gesture commits to scrolling the list"
        );
        let offset = core
            .ui_state
            .scroll
            .offsets
            .get(list_id.as_str())
            .copied()
            .unwrap_or(0.0);
        assert!(offset > 1.0, "the list scrolled, offset {offset}");
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert_eq!(view.pan, (0.0, 0.0), "viewport must not pan");
    }

    #[test]
    fn touch_horizontal_drag_over_scrollable_inside_viewport_pans() {
        // Issue #130: axis dominance. A mostly-horizontal swipe over a
        // vertical list embedded in a viewport must pan the viewport —
        // the list has no horizontal freedom, and without the dominance
        // gate its consuming the few pixels of vertical drift would
        // lock the whole gesture to jittering the list.
        use crate::tree::*;
        let rows: Vec<El> = (0..20)
            .map(|i| {
                crate::widgets::button::button("Row")
                    .key(format!("row{i}"))
                    .width(Size::Fixed(160.0))
                    .height(Size::Fixed(40.0))
            })
            .collect();
        let list = crate::scroll(rows)
            .key("list")
            .width(Size::Fixed(180.0))
            .height(Size::Fixed(120.0));
        let mut core = lay_out_viewport_tree(free_viewport(list));
        let list_id = core
            .ui_state
            .layout
            .key_index
            .get("list")
            .expect("list id")
            .to_string();
        // 30px horizontal with 3px of *upward* drift — upward makes
        // the drift consumable by a list at offset 0 (finger up =
        // scroll down), so this fails without the dominance gate
        // instead of passing vacuously because the list couldn't
        // move anyway.
        core.pointer_down(Pointer::touch(
            90.0,
            80.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(120.0, 77.0));
        assert!(
            core.ui_state.viewport_pan_active(),
            "horizontal-dominant drag pans the viewport"
        );
        let offset = core
            .ui_state
            .scroll
            .offsets
            .get(list_id.as_str())
            .copied()
            .unwrap_or(0.0);
        assert!(
            offset.abs() < 0.01,
            "the list must not consume the vertical drift, offset {offset}"
        );
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert!(
            (view.pan.0 - 30.0).abs() < 0.01 && (view.pan.1 + 3.0).abs() < 0.01,
            "pan anchored at the initial contact, got {:?}",
            view.pan
        );
    }

    #[test]
    fn touch_vertical_drag_with_drift_still_scrolls_inner_list() {
        // Companion to the axis-dominance gate (issue #130): a
        // vertical-dominant drag with a little horizontal drift must
        // keep scrolling the embedded list — dominance, not purity.
        use crate::tree::*;
        let rows: Vec<El> = (0..20)
            .map(|i| {
                crate::widgets::button::button("Row")
                    .key(format!("row{i}"))
                    .width(Size::Fixed(160.0))
                    .height(Size::Fixed(40.0))
            })
            .collect();
        let list = crate::scroll(rows)
            .key("list")
            .width(Size::Fixed(180.0))
            .height(Size::Fixed(120.0));
        let mut core = lay_out_viewport_tree(free_viewport(list));
        let list_id = core
            .ui_state
            .layout
            .key_index
            .get("list")
            .expect("list id")
            .to_string();
        // 40px up with 3px of horizontal drift.
        core.pointer_down(Pointer::touch(
            90.0,
            80.0,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(93.0, 40.0));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "vertical-dominant drag stays with the inner scrollable"
        );
        let offset = core
            .ui_state
            .scroll
            .offsets
            .get(list_id.as_str())
            .copied()
            .unwrap_or(0.0);
        assert!(offset > 1.0, "the list scrolled, offset {offset}");
    }

    #[test]
    fn touch_drag_on_scrollable_floating_over_viewport_scrolls_it() {
        // A scrollable in a sibling layer painted *over* the viewport
        // (no block_pointer) is neither ancestor nor descendant of it —
        // the pan takeover must yield to the press target's own layer,
        // exactly like the mouse paths do via their hit checks.
        use crate::tree::*;
        let rows: Vec<El> = (0..20)
            .map(|i| {
                crate::widgets::button::button("Row")
                    .key(format!("row{i}"))
                    .width(Size::Fixed(160.0))
                    .height(Size::Fixed(40.0))
            })
            .collect();
        let list = crate::scroll(rows)
            .key("list")
            .width(Size::Fixed(180.0))
            .height(Size::Fixed(120.0));
        let mut core = lay_out_viewport_tree(crate::stack([free_viewport(vp_card()), list]));
        let list_id = core
            .ui_state
            .layout
            .key_index
            .get("list")
            .expect("list id")
            .to_string();
        let r = core.rect_of_key("row1").expect("row rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(cx, cy - 40.0));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "the floating list keeps its drag — no pan underneath"
        );
        let offset = core
            .ui_state
            .scroll
            .offsets
            .get(list_id.as_str())
            .copied()
            .unwrap_or(0.0);
        assert!(offset > 1.0, "the list scrolled, offset {offset}");
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert_eq!(view.pan, (0.0, 0.0), "viewport must not pan");
    }

    #[test]
    fn touch_drag_on_child_of_dedicated_trigger_viewport_does_not_pan() {
        // Touch contacts arrive as the primary button, so a viewport
        // that pans on a dedicated trigger keeps today's scroll routing
        // (its pan stays wheel/programmatic-only on touch).
        let mut core =
            lay_out_viewport_tree(free_viewport(vp_card()).pan_button(PointerButton::Secondary));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(cx + 30.0, cy));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "dedicated trigger must not pan on a touch drag"
        );
    }

    #[test]
    fn touch_drag_over_viewport_child_beats_the_outer_page_scroll() {
        // A viewport embedded in a scroll page owns drags over its
        // content — the embedded-map convention.
        use crate::tree::*;
        let vp = free_viewport(vp_card())
            .width(Size::Fixed(300.0))
            .height(Size::Fixed(150.0));
        let mut children: Vec<El> = vec![vp];
        children.extend((0..10).map(|i| {
            crate::widgets::button::button("Filler")
                .key(format!("f{i}"))
                .height(Size::Fixed(100.0))
        }));
        let mut core = lay_out_viewport_tree(crate::scroll(children).key("page"));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(cx, cy + 30.0));
        assert!(
            core.ui_state.viewport_pan_active(),
            "the viewport wins over the enclosing page scroll"
        );
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert!(
            (view.pan.1 - 30.0).abs() < 0.01,
            "pan tracks the drag, got {:?}",
            view.pan
        );
    }

    #[test]
    fn pointer_cancel_abandons_captures_without_release_effects() {
        // A system cancel (touch stolen by an OS gesture) mid-pan: the
        // capture must release where it is, so the next contact starts
        // fresh instead of inheriting a live pan whose release then
        // eats a tap.
        let mut core = lay_out_viewport_tree(free_viewport(vp_card()));
        let r = core.rect_of_key("card").expect("card rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        core.pointer_moved(touch_move(cx + 30.0, cy));
        assert!(core.ui_state.viewport_pan_active(), "pan in flight");
        core.pointer_cancelled();
        assert!(
            !core.ui_state.viewport_pan_active(),
            "cancel abandons the pan capture"
        );
        // The view keeps what the drag reached — cancel is not an undo.
        let view = core.ui_state.viewport_view_by_key("vp").expect("vp view");
        assert!((view.pan.0 - 30.0).abs() < 0.01);
        // A fresh tap afterwards clicks normally — nothing inherited.
        core.pointer_down(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let up = core.pointer_up(Pointer::touch(
            cx,
            cy,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let kinds: Vec<UiEventKind> = up.iter().map(|e| e.kind).collect();
        assert!(
            kinds.contains(&UiEventKind::Click),
            "next tap clicks, got {kinds:?}"
        );
    }

    #[test]
    fn pointer_cancel_discards_plot_box_zoom() {
        // Unlike release (which applies the swept span), a cancel must
        // throw the selection away.
        let mut core = plot_under_modal(false);
        let before = core.ui_state.plot_view_by_key("p").expect("plot view");
        core.pointer_down(Pointer::mouse(120.0, 100.0, PointerButton::Primary));
        assert!(core.ui_state.plot_zoom_active(), "box-zoom in flight");
        core.pointer_moved(Pointer::moving(280.0, 200.0));
        core.pointer_cancelled();
        assert!(!core.ui_state.plot_zoom_active(), "cancel clears the drag");
        let after = core.ui_state.plot_view_by_key("p").expect("plot view");
        assert_eq!(
            (before.x.min, before.x.max),
            (after.x.min, after.x.max),
            "the swept span must not be applied"
        );
    }

    /// The #127 fixture: a full-frame default-trigger viewport with an
    /// optional centered modal floated over it. The modal body is
    /// unkeyed, so a press on the panel lands on block_pointer dead
    /// space; presses outside the panel land on the keyed dismiss
    /// scrim (never pan-eligible either — correct modal semantics).
    fn vp_under_modal(with_modal: bool) -> RunnerCore {
        use crate::tree::*;
        let modal = with_modal.then(|| {
            crate::widgets::overlay::modal(
                "detail",
                "Detail",
                [crate::widgets::text::text("plain detail text")
                    .width(Size::Fixed(200.0))
                    .height(Size::Fixed(120.0))],
            )
        });
        lay_out_viewport_tree(crate::widgets::overlay::overlays(
            free_viewport(vp_card()),
            [modal],
        ))
    }

    #[test]
    fn press_on_modal_dead_space_does_not_pan_the_viewport_underneath() {
        // Issue #127: a modal's block_pointer dead space hits nothing,
        // which the pan branch used to read as "viewport background" —
        // press-dragging on the modal panned the canvas underneath it.
        // Mirrors the wheel path's occlusion rule.
        let mut core = vp_under_modal(true);
        // The centered modal panel covers the root center.
        core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "press on the modal must not pan the canvas under it"
        );
        // Same point without the modal is viewport background: it pans —
        // the guard must not overreach.
        let mut core = vp_under_modal(false);
        core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            core.ui_state.viewport_pan_active(),
            "background press without the modal still pans"
        );
    }

    /// The #127 camera fixture: a chart3d scene with an optional
    /// centered modal floated over it.
    fn scene_under_modal(with_modal: bool) -> RunnerCore {
        use crate::scene::{PointData, PointsHandle, ScenePoint, SceneSpec};
        let handle = PointsHandle::new(PointData {
            points: vec![
                ScenePoint {
                    position: crate::scene::glam::Vec3::splat(-1.0),
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: crate::scene::glam::Vec3::splat(1.0),
                    color: [1.0; 4],
                },
            ],
        });
        let modal = with_modal.then(|| {
            crate::widgets::overlay::modal(
                "detail",
                "Detail",
                [crate::widgets::text::text("plain detail text")
                    .width(crate::tree::Size::Fixed(200.0))
                    .height(crate::tree::Size::Fixed(120.0))],
            )
        });
        let mut tree = crate::widgets::overlay::overlays(
            crate::tree::chart3d(SceneSpec::new().points(handle)),
            [modal],
        );
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        core.ui_state.sync_focus_order(&tree);
        core.ui_state.tick_scene_cameras(&tree, Instant::now());
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    /// The #127 plot fixture: a full-frame plot with an optional
    /// centered modal floated over it.
    fn plot_under_modal(with_modal: bool) -> RunnerCore {
        use crate::plot::{PlotSpec, Sample, Scale, SeriesHandle, line};
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
        let p = crate::tree::plot(
            PlotSpec::new()
                .x(Scale::linear())
                .y(Scale::linear())
                .add_mark(line(&h)),
        )
        .key("p");
        let modal = with_modal.then(|| {
            crate::widgets::overlay::modal(
                "detail",
                "Detail",
                [crate::widgets::text::text("plain detail text")
                    .width(crate::tree::Size::Fixed(200.0))
                    .height(crate::tree::Size::Fixed(120.0))],
            )
        });
        let mut tree = crate::widgets::overlay::overlays(p, [modal]);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        core.ui_state.prepare_plots(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    #[test]
    fn press_on_modal_dead_space_does_not_zoom_the_plot_underneath() {
        // Same occlusion rule for the plot press capture (issue #127):
        // the plot wheel path already had the guard, the press path
        // could box-zoom (or double-click-reset) under a modal.
        let mut core = plot_under_modal(true);
        core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            !core.ui_state.plot_zoom_active() && !core.ui_state.plot_pan_active(),
            "press on the modal must not start a plot gesture under it"
        );
        // Same point without the modal starts the scheme's drag — the
        // guard must not overreach.
        let mut core = plot_under_modal(false);
        core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            core.ui_state.plot_zoom_active() || core.ui_state.plot_pan_active(),
            "bare plot press still starts the drag without the modal"
        );
    }

    #[test]
    fn press_on_modal_dead_space_does_not_orbit_the_scene_underneath() {
        // Same guard for the camera-drag capture (issue #127).
        let mut core = scene_under_modal(true);
        assert!(
            core.ui_state.scene_at(200.0, 150.0).is_some(),
            "scene covers the center"
        );
        // Press on the centered modal panel (unkeyed body → dead space).
        core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            !core.ui_state.camera_drag_active(),
            "press on the modal must not orbit the scene under it"
        );
        // Same point without the modal captures the orbit — the guard
        // must not overreach.
        let mut core = scene_under_modal(false);
        core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            core.ui_state.camera_drag_active(),
            "bare scene press still orbits without the modal"
        );
    }

    #[test]
    fn latent_pan_ignores_widget_floating_over_viewport() {
        // A keyed widget layered *over* the viewport (not a descendant)
        // keeps its own gesture: no latent pan arms for its press.
        use crate::tree::*;
        let tree = crate::stack([
            free_viewport(vp_card()),
            crate::widgets::button::button("Hud")
                .key("hud")
                .width(Size::Fixed(60.0))
                .height(Size::Fixed(30.0)),
        ]);
        let mut core = lay_out_viewport_tree(tree);
        let r = core.rect_of_key("hud").expect("hud rect");
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_moved(Pointer::moving(cx + 20.0, cy));
        assert!(
            !core.ui_state.viewport_pan_active(),
            "overlay press must not convert into a viewport pan"
        );
    }

    #[test]
    fn ui_state_hovered_key_returns_leaf_key() {
        let mut core = lay_out_input_tree(false);
        assert_eq!(core.ui_state().hovered_key(), None);

        let btn = core.rect_of_key("btn").expect("btn rect");
        core.pointer_moved(Pointer::moving(btn.x + 4.0, btn.y + 4.0));
        assert_eq!(core.ui_state().hovered_key(), Some("btn"));

        // Off-target → None again.
        core.pointer_moved(Pointer::moving(0.0, 0.0));
        assert_eq!(core.ui_state().hovered_key(), None);
    }

    #[test]
    fn ui_state_is_hovering_within_walks_subtree() {
        // Card (keyed, focusable) wraps an inner icon-button (keyed).
        // is_hovering_within("card") should be true whenever the
        // cursor is on the card body OR on the inner button.
        use crate::tree::*;
        let mut tree = crate::column([crate::stack([
            crate::widgets::button::button("Inner").key("inner_btn")
        ])
        .key("card")
        .focusable()
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(60.0))])
        .padding(20.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);

        // Pre-hover: false everywhere.
        assert!(!core.ui_state().is_hovering_within("card"));
        assert!(!core.ui_state().is_hovering_within("inner_btn"));

        // Hover the inner button. Both the leaf and its ancestor card
        // should report subtree-hover true.
        let inner = core.rect_of_key("inner_btn").expect("inner rect");
        core.pointer_moved(Pointer::moving(inner.x + 4.0, inner.y + 4.0));
        assert!(core.ui_state().is_hovering_within("card"));
        assert!(core.ui_state().is_hovering_within("inner_btn"));

        // Unrelated / unknown keys read as false.
        assert!(!core.ui_state().is_hovering_within("not_a_key"));

        // Off the tree — both flip back to false.
        core.pointer_moved(Pointer::moving(0.0, 0.0));
        assert!(!core.ui_state().is_hovering_within("card"));
        assert!(!core.ui_state().is_hovering_within("inner_btn"));
    }

    #[test]
    fn hover_driven_scale_via_is_hovering_within_plus_animate() {
        // gh#10. The recipe that replaces a declarative
        // hover_translate / hover_scale / hover_tint API: the build
        // closure reads `cx.is_hovering_within(key)` and writes the
        // target prop value; `.animate(...)` eases between build
        // values across frames. End-to-end check that hover transition
        // → eased scale settle.
        use crate::Theme;
        use crate::anim::Timing;
        use crate::tree::*;

        // Helper that mirrors the documented recipe — closure over a
        // hover boolean so the test can drive the rebuild deterministically.
        let build_card = |hovering: bool| -> El {
            let scale = if hovering { 1.05 } else { 1.0 };
            crate::column([crate::stack(
                [crate::widgets::button::button("Inner").key("inner_btn")],
            )
            .key("card")
            .focusable()
            .scale(scale)
            .animate(Timing::SPRING_QUICK)
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(60.0))])
            .padding(20.0)
        };

        let mut core = RunnerCore::new();
        // Settled mode so the animate tick snaps each retarget to its
        // value — lets us verify final-state values without timing.
        core.ui_state
            .set_animation_mode(crate::state::AnimationMode::Settled);

        // Frame 1: not hovering → app builds with scale=1.0.
        let theme = Theme::default();
        let cx_pre = crate::BuildCx::new(&theme).with_ui_state(core.ui_state());
        assert!(!cx_pre.is_hovering_within("card"));
        let mut tree = build_card(cx_pre.is_hovering_within("card"));
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core.ui_state
            .tick_visual_animations(&mut tree, web_time::Instant::now(), theme.palette());
        let card_at_rest = tree.children[0].clone();
        assert!((card_at_rest.scale - 1.0).abs() < 1e-3);

        // Hover the card. is_hovering_within flips true.
        let card_rect = core.rect_of_key("card").expect("card rect");
        core.pointer_moved(Pointer::moving(card_rect.x + 4.0, card_rect.y + 4.0));

        // Frame 2: app sees hovering=true, rebuilds with scale=1.05.
        // Settled animate tick snaps scale to the new target.
        let cx_hot = crate::BuildCx::new(&theme).with_ui_state(core.ui_state());
        assert!(cx_hot.is_hovering_within("card"));
        let mut tree = build_card(cx_hot.is_hovering_within("card"));
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        core.snapshot(&tree, &mut t);
        core.ui_state
            .tick_visual_animations(&mut tree, web_time::Instant::now(), theme.palette());
        let card_hot = tree.children[0].clone();
        assert!(
            (card_hot.scale - 1.05).abs() < 1e-3,
            "hover should drive card scale to 1.05 via animate; got {}",
            card_hot.scale,
        );

        // Unhover → app rebuilds with scale=1.0; settled tick snaps back.
        core.pointer_moved(Pointer::moving(0.0, 0.0));
        let cx_cold = crate::BuildCx::new(&theme).with_ui_state(core.ui_state());
        assert!(!cx_cold.is_hovering_within("card"));
        let mut tree = build_card(cx_cold.is_hovering_within("card"));
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        core.snapshot(&tree, &mut t);
        core.ui_state
            .tick_visual_animations(&mut tree, web_time::Instant::now(), theme.palette());
        let card_after = tree.children[0].clone();
        assert!((card_after.scale - 1.0).abs() < 1e-3);
    }

    #[test]
    fn file_dropped_routes_to_keyed_leaf_at_pointer() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let path = std::path::PathBuf::from("/tmp/screenshot.png");
        let events = core.file_dropped(path.clone(), btn.x + 4.0, btn.y + 4.0);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.kind, UiEventKind::FileDropped);
        assert_eq!(event.key.as_deref(), Some("btn"));
        assert_eq!(event.path.as_deref(), Some(path.as_path()));
        assert_eq!(event.pointer, Some((btn.x + 4.0, btn.y + 4.0)));
    }

    #[test]
    fn file_dropped_outside_keyed_surface_emits_window_level_event() {
        let mut core = lay_out_input_tree(false);
        // Drop in the padding band — outside any keyed leaf.
        let path = std::path::PathBuf::from("/tmp/screenshot.png");
        let events = core.file_dropped(path.clone(), 1.0, 1.0);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.kind, UiEventKind::FileDropped);
        assert!(
            event.target.is_none(),
            "drop outside any keyed surface routes window-level",
        );
        assert!(event.key.is_none());
        // Path still flows through so a global drop sink can pick it up.
        assert_eq!(event.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn file_hovered_then_cancelled_pair() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let path = std::path::PathBuf::from("/tmp/a.png");

        let hover = core.file_hovered(path.clone(), btn.x + 4.0, btn.y + 4.0);
        assert_eq!(hover.len(), 1);
        assert_eq!(hover[0].kind, UiEventKind::FileHovered);
        assert_eq!(hover[0].key.as_deref(), Some("btn"));
        assert_eq!(hover[0].path.as_deref(), Some(path.as_path()));

        let cancel = core.file_hover_cancelled();
        assert_eq!(cancel.len(), 1);
        assert_eq!(cancel[0].kind, UiEventKind::FileHoverCancelled);
        assert!(cancel[0].target.is_none());
        assert!(cancel[0].path.is_none());
    }

    #[test]
    fn build_cx_hover_accessors_default_off_without_state() {
        use crate::Theme;
        let theme = Theme::default();
        let cx = crate::BuildCx::new(&theme);
        assert_eq!(cx.hovered_key(), None);
        assert!(!cx.is_hovering_within("anything"));
    }

    #[test]
    fn build_cx_hover_accessors_delegate_when_state_attached() {
        use crate::Theme;
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        core.pointer_moved(Pointer::moving(btn.x + 4.0, btn.y + 4.0));

        let theme = Theme::default();
        let cx = crate::BuildCx::new(&theme).with_ui_state(core.ui_state());
        assert_eq!(cx.hovered_key(), Some("btn"));
        assert!(cx.is_hovering_within("btn"));
        assert!(!cx.is_hovering_within("ti"));
    }

    #[test]
    fn build_cx_rect_of_key_reads_previous_layout() {
        use crate::Theme;
        let core = lay_out_input_tree(false);
        let theme = Theme::default();

        let detached = crate::BuildCx::new(&theme);
        assert_eq!(detached.rect_of_key("btn"), None);

        let cx = crate::BuildCx::new(&theme).with_ui_state(core.ui_state());
        assert_eq!(cx.rect_of_key("btn"), core.rect_of_key("btn"));
        assert_eq!(cx.rect_of_key("missing"), None);
    }

    #[test]
    fn event_cx_rect_of_key_answers_from_laid_out_state() {
        let core = lay_out_input_tree(false);

        let detached = crate::EventCx::new();
        assert_eq!(detached.rect_of_key("btn"), None);

        let cx = crate::EventCx::new().with_ui_state(core.ui_state());
        let rect = cx.rect_of_key("btn").expect("btn rect via EventCx");
        assert_eq!(Some(rect), core.rect_of_key("btn"));
        assert_eq!(cx.rect_of_key("missing"), None);
    }

    #[test]
    fn event_cx_viewport_and_diagnostics_mirror_build_cx() {
        // Detached context: every frame-context accessor answers None /
        // falls through to the wide branch, matching BuildCx semantics.
        let detached = crate::EventCx::new();
        assert_eq!(detached.viewport(), None);
        assert_eq!(detached.viewport_width(), None);
        assert_eq!(detached.viewport_height(), None);
        assert!(!detached.viewport_below(10_000.0));
        assert!(detached.diagnostics().is_none());

        let diagnostics = crate::HostDiagnostics {
            backend: "Test",
            ..Default::default()
        };
        let cx = crate::EventCx::new()
            .with_viewport(800.0, 600.0)
            .with_diagnostics(&diagnostics);
        assert_eq!(cx.viewport(), Some((800.0, 600.0)));
        assert_eq!(cx.viewport_width(), Some(800.0));
        assert_eq!(cx.viewport_height(), Some(600.0));
        assert!(cx.viewport_below(900.0));
        assert!(!cx.viewport_below(800.0));
        assert_eq!(cx.diagnostics().map(|d| d.backend), Some("Test"));
    }

    fn lay_out_paragraph_tree() -> RunnerCore {
        use crate::tree::*;
        let mut tree = crate::column([
            crate::widgets::text::text("First paragraph of text.")
                .key("p1")
                .selectable(),
            crate::widgets::text::text("Second paragraph of text.")
                .key("p2")
                .selectable(),
        ])
        .padding(20.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        core.ui_state.sync_focus_order(&tree);
        core.ui_state.sync_selection_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    #[test]
    fn pointer_down_on_selectable_text_emits_selection_changed() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cx = p1.x + 4.0;
        let cy = p1.y + p1.h * 0.5;
        let events = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let sel_event = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .expect("SelectionChanged emitted");
        let new_sel = sel_event
            .selection
            .as_ref()
            .expect("SelectionChanged carries a selection");
        let range = new_sel.range.as_ref().expect("collapsed selection at hit");
        assert_eq!(range.anchor.key, "p1");
        assert_eq!(range.head.key, "p1");
        assert_eq!(range.anchor.byte, range.head.byte);
        assert!(core.ui_state.selection.drag.is_some());
    }

    #[test]
    fn touch_long_press_on_selectable_text_selects_word_and_drags() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let p2 = core.rect_of_key("p2").expect("p2 rect");
        let x = p1.x + 4.0;
        let y = p1.y + p1.h * 0.5;

        let _ = core.pointer_down(Pointer::touch(
            x,
            y,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        let polled = core.poll_input(Instant::now() + LONG_PRESS_DELAY + Duration::from_millis(10));
        let selection = polled
            .iter()
            .rev()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .and_then(|e| e.selection.as_ref())
            .expect("touch long-press should select text");
        let range = selection.range.as_ref().expect("word selection");
        assert_eq!(range.anchor.key, "p1");
        assert_eq!(range.head.key, "p1");
        assert_eq!(range.anchor.byte, 0);
        assert_eq!(range.head.byte, 5);
        assert!(
            core.ui_state.selection.drag.is_some(),
            "long-pressed selectable text should stay ready for drag extension"
        );

        let mut moved = Pointer::moving(p2.x + 8.0, p2.y + p2.h * 0.5);
        moved.kind = PointerKind::Touch;
        let events = core.pointer_moved(moved).events;
        let selection = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .and_then(|e| e.selection.as_ref())
            .unwrap_or(&core.ui_state.current_selection);
        let range = selection.range.as_ref().expect("extended selection");
        assert_eq!(range.anchor.key, "p1");
        assert_eq!(range.head.key, "p2");

        let _ = core.pointer_up(Pointer::touch(
            p2.x + 8.0,
            p2.y + p2.h * 0.5,
            PointerButton::Primary,
            PointerId::PRIMARY,
        ));
        assert!(
            core.ui_state.selection.drag.is_none(),
            "selection drag should end on lift"
        );
    }

    #[test]
    fn pointer_drag_on_selectable_text_extends_head() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cx = p1.x + 4.0;
        let cy = p1.y + p1.h * 0.5;
        core.pointer_moved(Pointer::moving(cx, cy));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));

        // Drag to the right inside p1.
        let events = core
            .pointer_moved(Pointer::moving(p1.x + p1.w - 10.0, cy))
            .events;
        let sel_event = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .expect("Drag emits SelectionChanged");
        let new_sel = sel_event.selection.as_ref().unwrap();
        let range = new_sel.range.as_ref().unwrap();
        assert_eq!(range.anchor.key, "p1");
        assert_eq!(range.head.key, "p1");
        assert!(
            range.head.byte > range.anchor.byte,
            "head should advance past anchor (anchor={}, head={})",
            range.anchor.byte,
            range.head.byte
        );
    }

    #[test]
    fn double_click_hold_drag_inside_selectable_word_keeps_word_selected() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cx = p1.x + 4.0;
        let cy = p1.y + p1.h * 0.5;

        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let down = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let sel = down
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .and_then(|e| e.selection.as_ref())
            .and_then(|s| s.range.as_ref())
            .expect("double-click selects word");
        assert_eq!(sel.anchor.byte, 0);
        assert_eq!(sel.head.byte, 5);

        let events = core.pointer_moved(Pointer::moving(cx + 1.0, cy)).events;
        assert!(
            !events
                .iter()
                .any(|e| e.kind == UiEventKind::SelectionChanged),
            "drag jitter within the double-clicked word should not collapse the selection"
        );
        let range = core
            .ui_state
            .current_selection
            .range
            .as_ref()
            .expect("selection persists");
        assert_eq!(range.anchor.byte, 0);
        assert_eq!(range.head.byte, 5);
    }

    #[test]
    fn pointer_up_clears_drag_but_keeps_selection() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cx = p1.x + 4.0;
        let cy = p1.y + p1.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_moved(Pointer::moving(p1.x + p1.w - 10.0, cy));
        let _ = core.pointer_up(Pointer::mouse(
            p1.x + p1.w - 10.0,
            cy,
            PointerButton::Primary,
        ));
        assert!(
            core.ui_state.selection.drag.is_none(),
            "drag flag should clear on pointer_up"
        );
        assert!(
            !core.ui_state.current_selection.is_empty(),
            "selection itself should persist after pointer_up"
        );
    }

    #[test]
    fn drag_past_a_leaf_bottom_keeps_head_in_that_leaf_not_anchor() {
        // Regression: a previous helper (`byte_in_anchor_leaf`)
        // projected any out-of-leaf pointer back onto the anchor leaf.
        // That meant moving the cursor below p2's bottom edge while
        // dragging from p1 caused the head to snap home to p1 — the
        // selection band visibly shrank back instead of extending.
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let p2 = core.rect_of_key("p2").expect("p2 rect");
        // Anchor in p1.
        core.pointer_down(Pointer::mouse(
            p1.x + 4.0,
            p1.y + p1.h * 0.5,
            PointerButton::Primary,
        ));
        // Drag into p2 first — head migrates.
        core.pointer_moved(Pointer::moving(p2.x + 8.0, p2.y + p2.h * 0.5));
        // Now move WELL BELOW p2's rect (well below all selectables).
        // Head should remain in p2 (last leaf in this fixture is p2).
        let events = core
            .pointer_moved(Pointer::moving(p2.x + 8.0, p2.y + p2.h + 200.0))
            .events;
        let sel = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .map(|e| e.selection.as_ref().unwrap().clone())
            // No SelectionChanged emitted means the value didn't move
            // — read it back from the live UiState directly.
            .unwrap_or_else(|| core.ui_state.current_selection.clone());
        let r = sel.range.as_ref().expect("selection still active");
        assert_eq!(r.anchor.key, "p1", "anchor unchanged");
        assert_eq!(
            r.head.key, "p2",
            "head must stay in p2 even when pointer is below p2's rect"
        );
    }

    #[test]
    fn drag_into_a_sibling_selectable_extends_head_into_that_leaf() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let p2 = core.rect_of_key("p2").expect("p2 rect");
        // Anchor at the start of p1.
        core.pointer_down(Pointer::mouse(
            p1.x + 4.0,
            p1.y + p1.h * 0.5,
            PointerButton::Primary,
        ));
        // Drag down into p2.
        let events = core
            .pointer_moved(Pointer::moving(p2.x + 8.0, p2.y + p2.h * 0.5))
            .events;
        let sel_event = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .expect("Drag emits SelectionChanged");
        let new_sel = sel_event.selection.as_ref().unwrap();
        let range = new_sel.range.as_ref().unwrap();
        assert_eq!(range.anchor.key, "p1", "anchor stays in p1");
        assert_eq!(range.head.key, "p2", "head migrates into p2");
    }

    #[test]
    fn pointer_down_on_focusable_owning_selection_does_not_clear_it() {
        // Regression: clicking inside a text_input (focusable but not
        // a `.selectable()` leaf) used to fire SelectionChanged-empty
        // because selection_point_at missed and the runtime's
        // clear-fallback didn't notice the click landed on the same
        // widget that owned the active selection. The input's
        // PointerDown set the caret, then the empty SelectionChanged
        // collapsed it back to byte 0 every other click.
        let mut core = lay_out_input_tree(true);
        // Seed a selection in the input's key — this is what the
        // text_input would have written back via apply_event_with.
        core.set_selection(crate::selection::Selection::caret("ti", 3));
        let ti = core.rect_of_key("ti").expect("ti rect");
        let cx = ti.x + ti.w * 0.5;
        let cy = ti.y + ti.h * 0.5;

        let events = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let cleared = events.iter().find(|e| {
            e.kind == UiEventKind::SelectionChanged
                && e.selection.as_ref().map(|s| s.is_empty()).unwrap_or(false)
        });
        assert!(
            cleared.is_none(),
            "click on the selection-owning input must not emit a clearing SelectionChanged"
        );
        assert_eq!(
            core.ui_state.current_selection,
            crate::selection::Selection::caret("ti", 3),
            "runtime mirror is preserved when the click owns the selection"
        );
    }

    #[test]
    fn pointer_down_into_a_different_capture_keys_widget_does_not_clear_first() {
        // Regression: clicking into text_input A while the selection
        // lives in text_input B used to trigger the runtime's
        // clear-fallback. The empty SelectionChanged arrived after
        // A's PointerDown (which had set anchor = head = click pos),
        // collapsing the app's selection to default. The next Drag
        // event then read `selection.within(A) = None`, defaulted
        // anchor to 0, and only advanced head — so dragging into A
        // started the selection from byte 0 of the text instead of
        // the click position.
        let mut core = lay_out_input_tree(true);
        // Active selection lives in some other key, not "ti".
        core.set_selection(crate::selection::Selection::caret("other", 4));
        let ti = core.rect_of_key("ti").expect("ti rect");
        let cx = ti.x + ti.w * 0.5;
        let cy = ti.y + ti.h * 0.5;

        let events = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let cleared = events.iter().any(|e| {
            e.kind == UiEventKind::SelectionChanged
                && e.selection.as_ref().map(|s| s.is_empty()).unwrap_or(false)
        });
        assert!(
            !cleared,
            "click on a different capture_keys widget must not race-clear the selection"
        );
    }

    #[test]
    fn pointer_down_on_non_selectable_clears_existing_selection() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cy = p1.y + p1.h * 0.5;
        // Establish a selection in p1.
        core.pointer_down(Pointer::mouse(p1.x + 4.0, cy, PointerButton::Primary));
        core.pointer_up(Pointer::mouse(p1.x + 4.0, cy, PointerButton::Primary));
        assert!(!core.ui_state.current_selection.is_empty());

        // Press in empty space (no selectable, no focusable).
        let events = core.pointer_down(Pointer::mouse(2.0, 2.0, PointerButton::Primary));
        let cleared = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .expect("clearing emits SelectionChanged");
        let new_sel = cleared.selection.as_ref().unwrap();
        assert!(new_sel.is_empty(), "new selection should be empty");
        assert!(core.ui_state.current_selection.is_empty());
    }

    #[test]
    fn pointer_down_in_dead_space_clears_focus() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("btn")
        );

        core.pointer_down(Pointer::mouse(2.0, 2.0, PointerButton::Primary));

        assert_eq!(core.ui_state.focused.as_ref().map(|t| t.key.as_str()), None);
    }

    #[test]
    fn key_down_bumps_caret_activity_when_focused_widget_captures_keys() {
        // Showcase-style scenario: the app doesn't propagate its
        // Selection back via App::selection(), so set_selection always
        // sees the default-empty value and never bumps. The runtime
        // bump path catches arrow-key navigation directly.
        let mut core = lay_out_input_tree(true);
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "ti")
            .cloned();
        core.ui_state.set_focus(target); // focus moves → first bump
        let after_focus = core.ui_state.caret.activity_at.expect("focus bump");

        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        let after_arrow = core
            .ui_state
            .caret
            .activity_at
            .expect("arrow key bumps even without app-side selection");
        assert!(
            after_arrow > after_focus,
            "ArrowRight to a capture_keys focused widget bumps caret activity"
        );
    }

    #[test]
    fn text_input_bumps_caret_activity_when_focused() {
        let mut core = lay_out_input_tree(true);
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "ti")
            .cloned();
        core.ui_state.set_focus(target);
        let after_focus = core.ui_state.caret.activity_at.unwrap();

        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = core.text_input("a".into());
        let after_text = core.ui_state.caret.activity_at.unwrap();
        assert!(
            after_text > after_focus,
            "TextInput to focused widget bumps caret activity"
        );
    }

    #[test]
    fn pointer_down_inside_focused_input_bumps_caret_activity() {
        // Clicking again inside an already-focused capture_keys widget
        // doesn't change the focus target, so set_focus is a no-op
        // for activity. The runtime catches this so click-to-move-
        // caret resets the blink.
        let mut core = lay_out_input_tree(true);
        let ti = core.rect_of_key("ti").expect("ti rect");
        let cx = ti.x + ti.w * 0.5;
        let cy = ti.y + ti.h * 0.5;

        // First click → focus moves → bump.
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let after_first = core.ui_state.caret.activity_at.unwrap();

        // Second click on the same input → focus doesn't move, but
        // it's still caret-relevant activity.
        std::thread::sleep(std::time::Duration::from_millis(2));
        core.pointer_down(Pointer::mouse(cx + 1.0, cy, PointerButton::Primary));
        let after_second = core
            .ui_state
            .caret
            .activity_at
            .expect("second click bumps too");
        assert!(
            after_second > after_first,
            "click within already-focused capture_keys widget still bumps"
        );
    }

    #[test]
    fn arrow_key_through_apply_event_mutates_selection_and_bumps_on_set() {
        // End-to-end check that the path used by the text_input
        // example does in fact differ-then-bump on each arrow-key
        // press. If this regresses, the caret won't reset its blink
        // when the user moves the cursor — exactly what the polish
        // pass is meant to fix.
        use crate::widgets::text_input;
        let mut sel = crate::selection::Selection::caret("ti", 2);
        let mut value = String::from("hello");

        let mut core = RunnerCore::new();
        // Seed the runtime mirror so the first set_selection below
        // doesn't bump from "default → caret(2)".
        core.set_selection(sel.clone());
        let baseline = core.ui_state.caret.activity_at;

        // Build a synthetic ArrowRight KeyDown for the input's key.
        let arrow_right = UiEvent {
            key: Some("ti".into()),
            target: None,
            pointer: None,
            key_press: Some(crate::event::KeyPress {
                logical: LogicalKey::Named(NamedKey::ArrowRight),
                physical: PhysicalKey::Unidentified,
                modifiers: KeyModifiers::default(),
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        };

        // 1. App's on_event would call into this path:
        let mutated = text_input::apply_event(&mut value, &mut sel, &arrow_right, "ti");
        assert!(mutated, "ArrowRight should mutate selection");
        assert_eq!(
            sel.within("ti").unwrap().head,
            3,
            "head moved one char right (h-e-l-l-o, byte 2 → 3)"
        );

        // 2. Next frame's set_selection sees the new value → bump.
        std::thread::sleep(std::time::Duration::from_millis(2));
        core.set_selection(sel);
        let after = core.ui_state.caret.activity_at.unwrap();
        // If a baseline existed, the new bump must be later. Either
        // way the activity is now Some, which the .unwrap() above
        // already enforced.
        if let Some(b) = baseline {
            assert!(after > b, "arrow-key flow should bump activity");
        }
    }

    #[test]
    fn set_selection_bumps_caret_activity_only_when_value_changes() {
        let mut core = lay_out_paragraph_tree();
        // First call with the default selection — no bump (mirror is
        // already default-empty).
        core.set_selection(crate::selection::Selection::default());
        assert!(
            core.ui_state.caret.activity_at.is_none(),
            "no-op set_selection should not bump activity"
        );

        // Move the selection to a real range — bump.
        let sel_a = crate::selection::Selection::caret("p1", 3);
        core.set_selection(sel_a.clone());
        let bumped_at = core
            .ui_state
            .caret
            .activity_at
            .expect("first real selection bumps");

        // Same selection again — must NOT bump (else every frame
        // re-bumps and the caret never blinks).
        core.set_selection(sel_a.clone());
        assert_eq!(
            core.ui_state.caret.activity_at,
            Some(bumped_at),
            "set_selection with same value is a no-op"
        );

        // Caret at a different byte (simulating arrow-key motion) →
        // bump again.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let sel_b = crate::selection::Selection::caret("p1", 7);
        core.set_selection(sel_b);
        let new_bump = core.ui_state.caret.activity_at.expect("second bump");
        assert!(
            new_bump > bumped_at,
            "moving the caret bumps activity again",
        );
    }

    #[test]
    fn escape_clears_active_selection_and_emits_selection_changed() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cy = p1.y + p1.h * 0.5;
        // Drag-select inside p1 to establish a non-empty selection.
        core.pointer_down(Pointer::mouse(p1.x + 4.0, cy, PointerButton::Primary));
        core.pointer_moved(Pointer::moving(p1.x + p1.w - 10.0, cy));
        core.pointer_up(Pointer::mouse(
            p1.x + p1.w - 10.0,
            cy,
            PointerButton::Primary,
        ));
        assert!(!core.ui_state.current_selection.is_empty());

        let events = core.key_down(
            LogicalKey::Named(NamedKey::Escape),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![UiEventKind::Escape, UiEventKind::SelectionChanged],
            "Esc emits Escape (for popover dismiss) AND SelectionChanged"
        );
        let cleared = events
            .iter()
            .find(|e| e.kind == UiEventKind::SelectionChanged)
            .unwrap();
        assert!(cleared.selection.as_ref().unwrap().is_empty());
        assert!(core.ui_state.current_selection.is_empty());
    }

    #[test]
    fn consecutive_clicks_on_same_target_extend_count() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;

        // First press: count = 1.
        let down1 = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let pd1 = down1
            .iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .expect("PointerDown emitted");
        assert_eq!(pd1.click_count, 1, "first press starts the sequence");
        let up1 = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let click1 = up1
            .iter()
            .find(|e| e.kind == UiEventKind::Click)
            .expect("Click emitted");
        assert_eq!(
            click1.click_count, 1,
            "Click carries the same count as its PointerDown"
        );

        // Second press immediately after, same target: count = 2.
        let down2 = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let pd2 = down2
            .iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .unwrap();
        assert_eq!(pd2.click_count, 2, "second press extends the sequence");
        let up2 = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert_eq!(
            up2.iter()
                .find(|e| e.kind == UiEventKind::Click)
                .unwrap()
                .click_count,
            2
        );

        // Third: count = 3.
        let down3 = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let pd3 = down3
            .iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .unwrap();
        assert_eq!(pd3.click_count, 3, "third press → triple-click");
        core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
    }

    #[test]
    fn click_count_resets_when_target_changes() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let ti = core.rect_of_key("ti").expect("ti rect");

        // Press on btn → count=1.
        let down1 = core.pointer_down(Pointer::mouse(
            btn.x + btn.w * 0.5,
            btn.y + btn.h * 0.5,
            PointerButton::Primary,
        ));
        assert_eq!(
            down1
                .iter()
                .find(|e| e.kind == UiEventKind::PointerDown)
                .unwrap()
                .click_count,
            1
        );
        let _ = core.pointer_up(Pointer::mouse(
            btn.x + btn.w * 0.5,
            btn.y + btn.h * 0.5,
            PointerButton::Primary,
        ));

        // Press on ti (different target) → count resets to 1.
        let down2 = core.pointer_down(Pointer::mouse(
            ti.x + ti.w * 0.5,
            ti.y + ti.h * 0.5,
            PointerButton::Primary,
        ));
        let pd2 = down2
            .iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .unwrap();
        assert_eq!(
            pd2.click_count, 1,
            "press on a new target resets the multi-click sequence"
        );
    }

    #[test]
    fn double_click_on_selectable_text_selects_word_at_hit() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cy = p1.y + p1.h * 0.5;
        // Click near the start of "First paragraph of text." — twice
        // within the multi-click window.
        let cx = p1.x + 4.0;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        // The current selection should now span the first word.
        let sel = &core.ui_state.current_selection;
        let r = sel.range.as_ref().expect("selection set");
        assert_eq!(r.anchor.key, "p1");
        assert_eq!(r.head.key, "p1");
        // "First" is 5 bytes.
        assert_eq!(r.anchor.byte.min(r.head.byte), 0);
        assert_eq!(r.anchor.byte.max(r.head.byte), 5);
    }

    #[test]
    fn triple_click_on_selectable_text_selects_whole_leaf() {
        let mut core = lay_out_paragraph_tree();
        let p1 = core.rect_of_key("p1").expect("p1 rect");
        let cy = p1.y + p1.h * 0.5;
        let cx = p1.x + 4.0;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let sel = &core.ui_state.current_selection;
        let r = sel.range.as_ref().expect("selection set");
        assert_eq!(r.anchor.byte, 0);
        // "First paragraph of text." is 24 bytes.
        assert_eq!(r.head.byte, 24);
    }

    #[test]
    fn click_count_resets_when_press_drifts_outside_distance_window() {
        let mut core = lay_out_input_tree(false);
        let btn = core.rect_of_key("btn").expect("btn rect");
        let cx = btn.x + btn.w * 0.5;
        let cy = btn.y + btn.h * 0.5;

        let _ = core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));

        // Move 10 px (well outside MULTI_CLICK_DIST=4.0). Even if same
        // target, the second press starts a fresh sequence.
        let down2 = core.pointer_down(Pointer::mouse(cx + 10.0, cy, PointerButton::Primary));
        let pd2 = down2
            .iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .unwrap();
        assert_eq!(pd2.click_count, 1);
    }

    #[test]
    fn escape_with_no_selection_emits_only_escape() {
        let mut core = lay_out_paragraph_tree();
        assert!(core.ui_state.current_selection.is_empty());
        let events = core.key_down(
            LogicalKey::Named(NamedKey::Escape),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![UiEventKind::Escape],
            "no selection → no SelectionChanged side-effect"
        );
    }

    /// Build a 200x200 viewport hosting a `scroll([rows...])` whose
    /// content overflows so the thumb is present.
    fn lay_out_scroll_tree() -> (RunnerCore, String) {
        use crate::tree::*;
        let mut tree = crate::scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let scroll_id = tree.computed_id.clone();
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        (core, scroll_id.to_string())
    }

    #[test]
    fn thumb_pointer_down_captures_drag_and_suppresses_events() {
        let (mut core, scroll_id) = lay_out_scroll_tree();
        let thumb = core
            .ui_state
            .scroll
            .thumb_rects
            .get(&scroll_id)
            .copied()
            .expect("scrollable should have a thumb");
        let event = core.pointer_down(Pointer::mouse(
            thumb.x + thumb.w * 0.5,
            thumb.y + thumb.h * 0.5,
            PointerButton::Primary,
        ));
        assert!(
            event.is_empty(),
            "thumb press should not emit PointerDown to the app"
        );
        let drag = core
            .ui_state
            .scroll
            .thumb_drag
            .as_ref()
            .expect("scroll.thumb_drag should be set after pointer_down on thumb");
        assert_eq!(drag.scroll_id, scroll_id);
    }

    #[test]
    fn track_click_above_thumb_pages_up_below_pages_down() {
        let (mut core, scroll_id) = lay_out_scroll_tree();
        let track = core
            .ui_state
            .scroll
            .thumb_tracks
            .get(&scroll_id)
            .copied()
            .expect("scrollable should have a track");
        let thumb = core
            .ui_state
            .scroll
            .thumb_rects
            .get(&scroll_id)
            .copied()
            .unwrap();
        let metrics = core
            .ui_state
            .scroll
            .metrics
            .get(&scroll_id)
            .copied()
            .unwrap();

        // Press in the track below the thumb at offset 0 → page down.
        let evt = core.pointer_down(Pointer::mouse(
            track.x + track.w * 0.5,
            thumb.y + thumb.h + 10.0,
            PointerButton::Primary,
        ));
        assert!(evt.is_empty(), "track press should not surface PointerDown");
        assert!(
            core.ui_state.scroll.thumb_drag.is_none(),
            "track click outside the thumb should not start a drag",
        );
        let after_down = core.ui_state.scroll_offset(&scroll_id);
        let expected_page = (metrics.viewport_h - SCROLL_PAGE_OVERLAP).max(0.0);
        assert!(
            (after_down - expected_page.min(metrics.max_offset)).abs() < 0.5,
            "page-down offset = {after_down} (expected ~{expected_page})",
        );
        // pointer_up after a track-page is a no-op (no drag to clear).
        let _ = core.pointer_up(Pointer::mouse(0.0, 0.0, PointerButton::Primary));

        // Re-layout to refresh the thumb position at the new offset,
        // then click-to-page up.
        let mut tree = lay_out_scroll_tree_only();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let track = core
            .ui_state
            .scroll
            .thumb_tracks
            .get(&*tree.computed_id)
            .copied()
            .unwrap();
        let thumb = core
            .ui_state
            .scroll
            .thumb_rects
            .get(&*tree.computed_id)
            .copied()
            .unwrap();

        core.pointer_down(Pointer::mouse(
            track.x + track.w * 0.5,
            thumb.y - 4.0,
            PointerButton::Primary,
        ));
        let after_up = core.ui_state.scroll_offset(&tree.computed_id);
        assert!(
            after_up < after_down,
            "page-up should reduce offset: before={after_down} after={after_up}",
        );
    }

    #[test]
    fn scrollbar_press_does_not_bypass_covering_scrim() {
        let (mut core, scroll_id) =
            lay_out_scroll_tree_with_layer(crate::widgets::overlay::scrim("modal:dismiss"));
        let thumb = core
            .ui_state
            .scroll
            .thumb_rects
            .get(&scroll_id)
            .copied()
            .expect("scrollable should have a thumb");

        let events = core.pointer_down(Pointer::mouse(
            thumb.x + thumb.w * 0.5,
            thumb.y + thumb.h * 0.5,
            PointerButton::Primary,
        ));

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, UiEventKind::PointerDown);
        assert_eq!(events[0].route(), Some("modal:dismiss"));
        assert!(
            core.ui_state.scroll.thumb_drag.is_none(),
            "covered scrollbar thumb must not capture drag",
        );
    }

    #[test]
    fn scrollbar_press_does_not_bypass_block_pointer_layer() {
        use crate::tree::*;

        let (mut core, scroll_id) =
            lay_out_scroll_tree_with_layer(El::new(Kind::Group).fill_size().block_pointer());
        let thumb = core
            .ui_state
            .scroll
            .thumb_rects
            .get(&scroll_id)
            .copied()
            .expect("scrollable should have a thumb");

        let events = core.pointer_down(Pointer::mouse(
            thumb.x + thumb.w * 0.5,
            thumb.y + thumb.h * 0.5,
            PointerButton::Primary,
        ));

        assert!(
            events.is_empty(),
            "block_pointer layer should swallow the press without scrolling",
        );
        assert!(
            core.ui_state.scroll.thumb_drag.is_none(),
            "covered scrollbar thumb must not capture drag",
        );
    }

    /// Same fixture as `lay_out_scroll_tree` but doesn't build a
    /// fresh `RunnerCore` — useful when tests want to re-layout
    /// against an existing core to refresh thumb rects after a
    /// scroll offset change.
    fn lay_out_scroll_tree_only() -> El {
        use crate::tree::*;
        crate::scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0))
    }

    fn lay_out_scroll_tree_with_layer(layer: El) -> (RunnerCore, String) {
        use crate::tree::*;

        let scroll = lay_out_scroll_tree_only();
        let mut tree = crate::stack([scroll, layer]).fill_size();
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let scroll_id = tree.children[0].computed_id.clone();
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        (core, scroll_id.to_string())
    }

    /// `row([nav (resizable, 200px, clamp 120..420), main (Fill)])` in
    /// an 800x400 viewport — the canonical `.user_resizable()` sidebar
    /// (issue #106). Optionally wrapped in a stack under `layer`.
    fn lay_out_resizable_tree(layer: Option<El>) -> RunnerCore {
        use crate::tree::*;
        let row = crate::tree::row([
            column(Vec::<El>::new())
                .key("nav")
                .user_resizable()
                .width(Size::Fixed(200.0))
                .min_width(120.0)
                .max_width(420.0),
            column(Vec::<El>::new()).width(Size::Fill(1.0)),
        ])
        .height(Size::Fill(1.0));
        let mut tree = match layer {
            Some(layer) => crate::stack([row, layer]).fill_size(),
            None => row,
        };
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 800.0, 400.0),
        );
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    #[test]
    fn resize_band_drag_updates_user_size_and_emits_resized_on_release() {
        let mut core = lay_out_resizable_tree(None);

        // Press on the seam (x=200) captures the drag silently.
        let events = core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(events.is_empty(), "band press must not reach the app");
        assert!(core.ui_state.resize.drag.is_some());

        // Drag right 80px → the override tracks absolutely.
        let mv = core.pointer_moved(Pointer::mouse(280.0, 150.0, PointerButton::Primary));
        assert!(mv.events.is_empty(), "drag is captured, no app events");
        assert!(mv.needs_redraw, "size change must redraw");
        assert_eq!(core.ui_state.user_size("nav"), Some(280.0));

        // Way past max → clamps to max_width.
        let _ = core.pointer_moved(Pointer::mouse(700.0, 150.0, PointerButton::Primary));
        assert_eq!(core.ui_state.user_size("nav"), Some(420.0));

        // Release emits exactly one `Resized` routed to the pane key.
        let up = core.pointer_up(Pointer::mouse(700.0, 150.0, PointerButton::Primary));
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].kind, UiEventKind::Resized);
        assert_eq!(up[0].route(), Some("nav"));
        assert!(core.ui_state.resize.drag.is_none());
    }

    #[test]
    fn resize_band_hover_flips_cursor_and_requests_redraw() {
        let mut core = lay_out_resizable_tree(None);
        let tree = core.last_tree.clone().expect("snapshot stored a tree");

        // Far from the seam: default cursor.
        let _ = core.pointer_moved(Pointer::mouse(400.0, 150.0, PointerButton::Primary));
        assert_eq!(core.ui_state.cursor(&tree), crate::cursor::Cursor::Default);

        // Crossing onto the band must request a redraw (the host only
        // re-resolves the cursor after one) and resolve the resize
        // arrows even though no keyed hover changed.
        let mv = core.pointer_moved(Pointer::mouse(201.0, 150.0, PointerButton::Primary));
        assert!(mv.needs_redraw, "band entry must redraw for the cursor");
        assert_eq!(core.ui_state.cursor(&tree), crate::cursor::Cursor::EwResize);

        // During a captured drag the cursor stays put even when the
        // pointer wanders off the band.
        let _ = core.pointer_down(Pointer::mouse(201.0, 150.0, PointerButton::Primary));
        let _ = core.pointer_moved(Pointer::mouse(390.0, 150.0, PointerButton::Primary));
        assert_eq!(core.ui_state.cursor(&tree), crate::cursor::Cursor::EwResize);
    }

    #[test]
    fn resize_band_press_does_not_bypass_covering_scrim() {
        let mut core =
            lay_out_resizable_tree(Some(crate::widgets::overlay::scrim("modal:dismiss")));

        let events = core.pointer_down(Pointer::mouse(200.0, 150.0, PointerButton::Primary));
        assert!(
            core.ui_state.resize.drag.is_none(),
            "covered band must not capture the press",
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, UiEventKind::PointerDown);
        assert_eq!(events[0].route(), Some("modal:dismiss"));
    }

    #[test]
    fn thumb_drag_translates_pointer_delta_into_scroll_offset() {
        let (mut core, scroll_id) = lay_out_scroll_tree();
        let thumb = core
            .ui_state
            .scroll
            .thumb_rects
            .get(&scroll_id)
            .copied()
            .unwrap();
        let metrics = core
            .ui_state
            .scroll
            .metrics
            .get(&scroll_id)
            .copied()
            .unwrap();
        let track_remaining = (metrics.viewport_h - thumb.h).max(0.0);

        let press_y = thumb.y + thumb.h * 0.5;
        core.pointer_down(Pointer::mouse(
            thumb.x + thumb.w * 0.5,
            press_y,
            PointerButton::Primary,
        ));
        // Drag 20 px down — offset should advance by `20 * max_offset / track_remaining`.
        let evt = core.pointer_moved(Pointer::moving(thumb.x + thumb.w * 0.5, press_y + 20.0));
        assert!(
            evt.events.is_empty(),
            "thumb-drag move should suppress Drag event",
        );
        let offset = core.ui_state.scroll_offset(&scroll_id);
        let expected = 20.0 * (metrics.max_offset / track_remaining);
        assert!(
            (offset - expected).abs() < 0.5,
            "offset {offset} (expected {expected})",
        );
        // Overshooting clamps to max_offset.
        core.pointer_moved(Pointer::moving(thumb.x + thumb.w * 0.5, press_y + 9999.0));
        let offset = core.ui_state.scroll_offset(&scroll_id);
        assert!(
            (offset - metrics.max_offset).abs() < 0.5,
            "overshoot offset {offset} (expected {})",
            metrics.max_offset
        );
        // Release clears the drag without emitting events.
        let events = core.pointer_up(Pointer::mouse(thumb.x, press_y, PointerButton::Primary));
        assert!(events.is_empty(), "thumb release shouldn't emit events");
        assert!(core.ui_state.scroll.thumb_drag.is_none());
    }

    #[test]
    fn secondary_click_does_not_steal_focus_or_press() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        // Focus elsewhere first via primary click on the input.
        let ti_rect = core.rect_of_key("ti").expect("ti rect");
        let tx = ti_rect.x + ti_rect.w * 0.5;
        let ty = ti_rect.y + ti_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(tx, ty, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(tx, ty, PointerButton::Primary));
        let focused_before = core.ui_state.focused.as_ref().map(|t| t.key.clone());
        // Right-click on the button.
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Secondary));
        let events = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Secondary));
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![UiEventKind::SecondaryClick]);
        let focused_after = core.ui_state.focused.as_ref().map(|t| t.key.clone());
        assert_eq!(
            focused_before, focused_after,
            "right-click must not steal focus"
        );
        assert!(
            core.ui_state.pressed.is_none(),
            "right-click must not set primary press"
        );
    }

    #[test]
    fn text_input_routes_to_focused_only() {
        let mut core = lay_out_input_tree(false);
        // No focus yet → no event.
        assert!(core.text_input("a".into()).is_none());
        // Focus the button via primary click.
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        let event = core.text_input("hi".into()).expect("focused → event");
        assert_eq!(event.kind, UiEventKind::TextInput);
        assert_eq!(event.text.as_deref(), Some("hi"));
        assert_eq!(event.target.as_ref().map(|t| t.key.as_str()), Some("btn"));
        // Empty text → no event (some IME paths emit empty composition).
        assert!(core.text_input(String::new()).is_none());
    }

    #[test]
    fn capture_keys_bypasses_tab_traversal_for_focused_node() {
        // Focus the capture_keys input. Tab should NOT move focus —
        // it should be delivered as a raw KeyDown to the input.
        let mut core = lay_out_input_tree(true);
        let ti_rect = core.rect_of_key("ti").expect("ti rect");
        let tx = ti_rect.x + ti_rect.w * 0.5;
        let ty = ti_rect.y + ti_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(tx, ty, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(tx, ty, PointerButton::Primary));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti"),
            "primary click on capture_keys node still focuses it"
        );

        let events = core.key_down(
            LogicalKey::Named(NamedKey::Tab),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(events.len(), 1, "Tab → exactly one KeyDown");
        let event = &events[0];
        assert_eq!(event.kind, UiEventKind::KeyDown);
        assert_eq!(event.target.as_ref().map(|t| t.key.as_str()), Some("ti"));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti"),
            "Tab inside capture_keys must NOT move focus"
        );
    }

    #[test]
    fn escape_blurs_capture_keys_after_delivering_raw_keydown() {
        let mut core = lay_out_input_tree(true);
        let ti_rect = core.rect_of_key("ti").expect("ti rect");
        let tx = ti_rect.x + ti_rect.w * 0.5;
        let ty = ti_rect.y + ti_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(tx, ty, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(tx, ty, PointerButton::Primary));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti")
        );

        let events = core.key_down(
            LogicalKey::Named(NamedKey::Escape),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );

        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.kind, UiEventKind::KeyDown);
        assert_eq!(event.target.as_ref().map(|t| t.key.as_str()), Some("ti"));
        assert!(matches!(
            event.key_press.as_ref().map(|p| &p.logical),
            Some(LogicalKey::Named(NamedKey::Escape))
        ));
        assert_eq!(core.ui_state.focused.as_ref().map(|t| t.key.as_str()), None);
    }

    #[test]
    fn pointer_down_focus_does_not_raise_focus_visible() {
        // `:focus-visible` semantics: clicking a widget focuses it but
        // does NOT light up the focus ring. Verify the runtime flag.
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("btn"),
            "primary click focuses the button",
        );
        assert!(
            !core.ui_state.focus_visible,
            "click focus must not raise focus_visible — ring stays off",
        );
    }

    #[test]
    fn tab_key_raises_focus_visible_so_ring_appears() {
        let mut core = lay_out_input_tree(false);
        // Pre-focus via click so focus_visible starts low.
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert!(!core.ui_state.focus_visible);
        // Tab moves focus and should raise the ring.
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Tab),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            core.ui_state.focus_visible,
            "Tab must raise focus_visible so the ring paints on the new target",
        );
    }

    #[test]
    fn click_after_tab_clears_focus_visible_again() {
        // Tab raises the ring; a subsequent click on a focusable widget
        // suppresses it again — the user is back on the pointer.
        let mut core = lay_out_input_tree(false);
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Tab),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(core.ui_state.focus_visible, "Tab raises ring");
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert!(
            !core.ui_state.focus_visible,
            "pointer-down clears focus_visible — ring fades back out",
        );
    }

    #[test]
    fn keypress_on_focused_widget_raises_focus_visible_after_click() {
        // Click a focused-but-non-text widget, then nudge with a key
        // (e.g. arrow on a slider). The keypress is keyboard
        // interaction → ring lights up even though focus didn't move.
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert!(!core.ui_state.focus_visible);
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            core.ui_state.focus_visible,
            "non-Tab key on focused widget raises focus_visible",
        );
    }

    #[test]
    fn selected_text_resolves_a_selection_inside_a_virtual_list() {
        // Regression: a build-the-tree-then-walk-it path would miss
        // virtual_list children, because rows are realized in layout
        // (not build) — copy/cut from a visible row in a chat-style
        // virtualized pane silently produced an empty clipboard. The
        // runtime helper reads `last_tree`, which already has the
        // visible rows realized at the live scroll offset.
        use crate::selection::{Selection, SelectionPoint, SelectionRange};
        use crate::tree::*;

        // 20 rows; each row is a keyed selectable leaf so the
        // selection can point at it directly. 50px high so a 200px
        // viewport realizes the first few rows on the initial pass.
        let mut tree = virtual_list_dyn(
            20,
            50.0,
            |i| format!("row-{i}"),
            |i| {
                crate::widgets::text::text(format!("row {i} text"))
                    .key(format!("row-{i}"))
                    .selectable()
                    .height(Size::Fixed(50.0))
            },
        );
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);

        // Select the middle of "row 1 text" — bytes 0..9 = "row 1 tex".
        let selection = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("row-1", 0),
                head: SelectionPoint::new("row-1", 9),
            }),
        };
        core.set_selection(selection);

        assert_eq!(
            core.selected_text().as_deref(),
            Some("row 1 tex"),
            "runtime.selected_text() must walk last_tree (realized rows) — \
             a build-only path would miss virtual_list children entirely",
        );
    }

    #[test]
    fn shortcut_chord_does_not_raise_focus_visible() {
        // Pointer-click focuses the button and suppresses the ring.
        // Tapping or holding a bare modifier (Ctrl, Shift, …) before
        // the second half of a chord must NOT light the ring, and
        // completing the chord (e.g. Ctrl+C) must NOT light it
        // either — the focused widget is incidental to a global
        // shortcut, matching browser `:focus-visible` heuristics.
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert!(!core.ui_state.focus_visible);

        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Control),
            PhysicalKey::Unidentified,
            ctrl,
            false,
        );
        assert!(
            !core.ui_state.focus_visible,
            "bare Ctrl press must not raise focus_visible on a pointer-focused widget",
        );
        let _ = core.key_down(
            LogicalKey::Character("c".into()),
            PhysicalKey::Unidentified,
            ctrl,
            false,
        );
        assert!(
            !core.ui_state.focus_visible,
            "Ctrl+C is a shortcut, not interaction with the focused widget",
        );

        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Shift),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            !core.ui_state.focus_visible,
            "bare Shift press must not raise focus_visible",
        );
        let _ = core.key_down(
            LogicalKey::Character("a".into()),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            !core.ui_state.focus_visible,
            "bare character keys are typing/activation guesses, not navigation",
        );
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Escape),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            !core.ui_state.focus_visible,
            "Escape is dismissal, not navigation — no ring",
        );
    }

    #[test]
    fn arrow_nav_in_sibling_group_raises_focus_visible() {
        let mut core = lay_out_arrow_nav_tree();
        // The fixture pre-sets focus directly without going through
        // the runtime; ensure the flag starts low.
        core.ui_state.set_focus_visible(false);
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            core.ui_state.focus_visible,
            "arrow-nav within an arrow_nav_siblings group is keyboard navigation",
        );
    }

    #[test]
    fn capture_keys_falls_back_to_default_when_focus_off_capturing_node() {
        // Tree has both a normal-focusable button and a capture_keys
        // input. Focus the button (normal focusable). Tab should then
        // do library-default focus traversal.
        let mut core = lay_out_input_tree(true);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(Pointer::mouse(cx, cy, PointerButton::Primary));
        let _ = core.pointer_up(Pointer::mouse(cx, cy, PointerButton::Primary));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("btn"),
            "primary click focuses button"
        );
        // Tab should move focus to the next focusable (the input).
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Tab),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti"),
            "Tab from non-capturing focused does library-default traversal"
        );
    }

    /// A column whose three buttons sit inside an `arrow_nav_siblings`
    /// parent (the shape `popover_panel` produces). Layout runs against
    /// a 200x300 viewport with 10px padding; each button is 80px wide
    /// and 36px tall stacked vertically, plenty inside the clip.
    fn lay_out_arrow_nav_tree() -> RunnerCore {
        use crate::tree::*;
        let mut tree = crate::column([
            crate::widgets::button::button("Red").key("opt-red"),
            crate::widgets::button::button("Green").key("opt-green"),
            crate::widgets::button::button("Blue").key("opt-blue"),
        ])
        .arrow_nav_siblings()
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 300.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        // Pre-focus the middle option (the typical state right after a
        // popover opens — we'll exercise transitions from there).
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "opt-green")
            .cloned();
        core.ui_state.set_focus(target);
        core
    }

    #[test]
    fn arrow_nav_moves_focus_among_siblings() {
        let mut core = lay_out_arrow_nav_tree();

        // ArrowDown moves to next sibling, no event emitted (it was
        // consumed by the navigation path).
        let down = core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(down.is_empty(), "arrow-nav consumes the key event");
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-blue"),
        );

        // ArrowUp moves back.
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowUp),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-green"),
        );

        // Home jumps to first.
        core.key_down(
            LogicalKey::Named(NamedKey::Home),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-red"),
        );

        // End jumps to last.
        core.key_down(
            LogicalKey::Named(NamedKey::End),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-blue"),
        );
    }

    #[test]
    fn arrow_nav_saturates_at_ends() {
        let mut core = lay_out_arrow_nav_tree();
        // Walk to the first option and try to go before it.
        core.key_down(
            LogicalKey::Named(NamedKey::Home),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowUp),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-red"),
            "ArrowUp at top stays at top — no wrap",
        );
        // Same at the bottom.
        core.key_down(
            LogicalKey::Named(NamedKey::End),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-blue"),
            "ArrowDown at bottom stays at bottom — no wrap",
        );
    }

    #[test]
    fn arrow_nav_vertical_lets_horizontal_arrows_fall_through() {
        // Regression guard for menubars: a vertical group must not
        // consume Left/Right, so the app can still route them (e.g.
        // switching between menubar menus).
        let mut core = lay_out_arrow_nav_tree();
        let out = core.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-green"),
            "ArrowRight must not move focus in a Vertical group",
        );
        assert!(
            !out.is_empty(),
            "ArrowRight falls through to normal KeyDown routing",
        );
    }

    /// Lay out a real `toggle_group` (a Horizontal arrow-nav row) and
    /// pre-focus the first item.
    fn lay_out_horizontal_group() -> RunnerCore {
        let mut tree = crate::widgets::toggle::toggle_group(
            "view",
            &"list",
            [("list", "List"), ("grid", "Grid"), ("map", "Map")],
        )
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 400.0, 100.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "view:toggle:list")
            .cloned();
        core.ui_state.set_focus(target);
        core
    }

    #[test]
    fn arrow_nav_horizontal_group_steps_on_left_right() {
        // Issue #63: toggle_group / tabs_list are Horizontal groups —
        // Left/Right step among the items, Up/Down fall through.
        let mut core = lay_out_horizontal_group();

        let right = core.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(right.is_empty(), "ArrowRight is consumed by the group");
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("view:toggle:grid"),
        );

        core.key_down(
            LogicalKey::Named(NamedKey::ArrowLeft),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("view:toggle:list"),
        );

        core.key_down(
            LogicalKey::Named(NamedKey::End),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("view:toggle:map"),
        );

        let down = core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("view:toggle:map"),
            "ArrowDown must not move focus in a Horizontal group",
        );
        assert!(
            !down.is_empty(),
            "ArrowDown falls through to normal KeyDown routing",
        );
    }

    #[test]
    fn arrow_nav_radio_group_steps_on_both_axes() {
        // ARIA radio-group: Up/Left previous, Down/Right next,
        // regardless of the group's visual axis (issue #63).
        let mut tree = crate::widgets::radio::radio_group(
            "theme",
            &"light",
            [("light", "Light"), ("dark", "Dark"), ("auto", "Auto")],
        )
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 300.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "theme:radio:light")
            .cloned();
        core.ui_state.set_focus(target);

        core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("theme:radio:dark"),
            "ArrowDown steps to the next radio",
        );
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("theme:radio:auto"),
            "ArrowRight also steps in a Both group",
        );
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowLeft),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("theme:radio:dark"),
            "ArrowLeft steps back in a Both group",
        );
    }

    /// Lay out a two-week `calendar_month` (days 1–14, day `disabled`
    /// optionally grayed out) and pre-focus the given day.
    fn lay_out_calendar(focus_day: u32, disabled_day: Option<u32>) -> RunnerCore {
        use crate::widgets::calendar::{CalendarDay, calendar_month};
        let days = (1..=14).map(|d| {
            let day = CalendarDay::new(format!("2026-06-{d:02}"), d.to_string());
            if Some(d) == disabled_day {
                day.disabled()
            } else {
                day
            }
        });
        let mut tree = calendar_month("cal", "June 2026", days);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 600.0, 400.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let key = format!("cal:day:2026-06-{focus_day:02}");
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == key)
            .cloned();
        assert!(target.is_some(), "day {focus_day} should be focusable");
        core.ui_state.set_focus(target);
        core
    }

    #[test]
    fn arrow_nav_calendar_grid_moves_two_dimensionally() {
        // Issue #63: the day grid is an ArrowNav::Grid group —
        // Left/Right step in tree order, Up/Down move between weeks.
        let mut core = lay_out_calendar(3, None);

        let down = core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(down.is_empty(), "ArrowDown is consumed by the grid");
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("cal:day:2026-06-10"),
            "ArrowDown lands on the same weekday one week later",
        );

        core.key_down(
            LogicalKey::Named(NamedKey::ArrowRight),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("cal:day:2026-06-11"),
        );

        core.key_down(
            LogicalKey::Named(NamedKey::ArrowUp),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("cal:day:2026-06-04"),
            "ArrowUp lands on the same weekday one week earlier",
        );

        // Up from the first week has no row above — focus stays put.
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowUp),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("cal:day:2026-06-04"),
            "ArrowUp at the top edge saturates",
        );
    }

    #[test]
    fn arrow_nav_calendar_grid_skips_disabled_days() {
        // Disabled cells aren't focusable, so they aren't group
        // members; Up/Down land on the geometrically nearest enabled
        // day in the target week instead of dead-ending.
        let mut core = lay_out_calendar(3, Some(10));
        core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        let focused = core
            .ui_state
            .focused
            .as_ref()
            .map(|t| t.key.clone())
            .expect("focus survives the move");
        assert!(
            focused == "cal:day:2026-06-09" || focused == "cal:day:2026-06-11",
            "ArrowDown over a disabled day lands on a neighbor in the \
             same week, got {focused}",
        );
    }

    /// Build a tree shaped like a real app's `build()` output: a
    /// background row with a "Trigger" button, optionally with a
    /// dropdown popover layered on top.
    fn build_popover_tree(open: bool) -> El {
        use crate::widgets::button::button;
        use crate::widgets::overlay::overlay;
        use crate::widgets::popover::{dropdown, menu_item};
        let mut layers: Vec<El> = vec![button("Trigger").key("trigger")];
        if open {
            layers.push(dropdown(
                "menu",
                "trigger",
                [
                    menu_item("A").key("item-a"),
                    menu_item("B").key("item-b"),
                    menu_item("C").key("item-c"),
                ],
            ));
        }
        overlay(layers).padding(20.0)
    }

    /// Run a full per-frame layout pass against `tree` so all the
    /// post-layout hooks (focus order sync, popover focus stack, etc.)
    /// fire just like a real frame.
    fn run_frame(core: &mut RunnerCore, tree: &mut El) {
        let mut t = PrepareTimings::default();
        core.prepare_layout(
            tree,
            Rect::new(0.0, 0.0, 400.0, 300.0),
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        core.snapshot(tree, &mut t);
    }

    #[test]
    fn popover_open_pushes_focus_and_auto_focuses_first_item() {
        let mut core = RunnerCore::new();
        let mut closed = build_popover_tree(false);
        run_frame(&mut core, &mut closed);
        // Pre-focus the trigger as if the user tabbed to it before
        // opening the menu.
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
        );

        // Open the popover. The runtime should snapshot the trigger
        // onto the focus stack and auto-focus the first menu item.
        let mut open = build_popover_tree(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("item-a"),
            "popover open should auto-focus the first menu item",
        );
        assert_eq!(
            core.ui_state.layer_focus.focus_stack.len(),
            1,
            "trigger should be saved on the focus stack",
        );
        assert_eq!(
            core.ui_state.layer_focus.focus_stack[0]
                .1
                .as_ref()
                .map(|t| t.key.as_str()),
            Some("trigger"),
            "saved focus should be the pre-open target",
        );
    }

    #[test]
    fn popover_close_restores_focus_to_trigger() {
        let mut core = RunnerCore::new();
        let mut closed = build_popover_tree(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        // Open → focus walks to the menu.
        let mut open = build_popover_tree(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("item-a"),
        );

        // Close → focus restored to trigger, stack drained.
        let mut closed_again = build_popover_tree(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
            "closing the popover should pop the saved focus",
        );
        assert!(
            core.ui_state.layer_focus.focus_stack.is_empty(),
            "focus stack should be drained after restore",
        );
    }

    #[test]
    fn popover_close_does_not_override_intentional_focus_move() {
        let mut core = RunnerCore::new();
        // Tree with a second focusable button outside the popover so
        // the user can "click somewhere else" while the menu is open.
        let build = |open: bool| -> El {
            use crate::widgets::button::button;
            use crate::widgets::overlay::overlay;
            use crate::widgets::popover::{dropdown, menu_item};
            let main = crate::row([
                button("Trigger").key("trigger"),
                button("Other").key("other"),
            ]);
            let mut layers: Vec<El> = vec![main];
            if open {
                layers.push(dropdown("menu", "trigger", [menu_item("A").key("item-a")]));
            }
            overlay(layers).padding(20.0)
        };

        let mut closed = build(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        let mut open = build(true);
        run_frame(&mut core, &mut open);
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);

        // Simulate an intentional focus move to a sibling that is
        // outside the popover (e.g. the user re-tabbed somewhere). Do
        // this by setting focus directly while the popover is still in
        // the tree — the existing focus-order contains "other".
        let other = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "other")
            .cloned();
        core.ui_state.set_focus(other);

        let mut closed_again = build(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("other"),
            "focus moved before close should not be overridden by restore",
        );
        assert!(core.ui_state.layer_focus.focus_stack.is_empty());
    }

    #[test]
    fn nested_popovers_stack_and_unwind_focus_correctly() {
        let mut core = RunnerCore::new();
        // Two siblings layered at El root: an outer popover anchored to
        // the trigger, and an inner popover anchored to a button inside
        // the outer panel. Both are real popovers — separate
        // popover_layer ids — so the runtime sees them stack.
        let build = |outer: bool, inner: bool| -> El {
            use crate::widgets::button::button;
            use crate::widgets::overlay::overlay;
            use crate::widgets::popover::{Anchor, popover, popover_panel};
            let main = button("Trigger").key("trigger");
            let mut layers: Vec<El> = vec![main];
            if outer {
                layers.push(popover(
                    "outer",
                    Anchor::below_key("trigger"),
                    popover_panel([button("Open inner").key("inner-trigger")]),
                ));
            }
            if inner {
                layers.push(popover(
                    "inner",
                    Anchor::below_key("inner-trigger"),
                    popover_panel([button("X").key("inner-a"), button("Y").key("inner-b")]),
                ));
            }
            overlay(layers).padding(20.0)
        };

        // Frame 1: nothing open, focus on the trigger.
        let mut closed = build(false, false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        // Frame 2: outer opens. Save trigger, focus inner-trigger.
        let mut outer = build(true, false);
        run_frame(&mut core, &mut outer);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("inner-trigger"),
        );
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);

        // Frame 3: inner also opens. Save inner-trigger, focus inner-a.
        let mut both = build(true, true);
        run_frame(&mut core, &mut both);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("inner-a"),
        );
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 2);

        // Frame 4: inner closes. Pop → restore inner-trigger.
        let mut outer_only = build(true, false);
        run_frame(&mut core, &mut outer_only);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("inner-trigger"),
        );
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);

        // Frame 5: outer closes. Pop → restore trigger.
        let mut none = build(false, false);
        run_frame(&mut core, &mut none);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
        );
        assert!(core.ui_state.layer_focus.focus_stack.is_empty());
    }

    /// Main view with two focusable buttons plus (optionally) a stock
    /// `modal()` layered above it — the issue #132 shape.
    fn build_modal_tree(open: bool) -> El {
        use crate::widgets::button::button;
        use crate::widgets::overlay::{modal, overlay};
        let main = crate::row([
            button("Trigger").key("trigger"),
            button("Other").key("other"),
        ]);
        let mut layers: Vec<El> = vec![main];
        if open {
            layers.push(modal(
                "confirm",
                "Confirm?",
                [button("OK").key("m-ok"), button("Cancel").key("m-cancel")],
            ));
        }
        overlay(layers).padding(20.0)
    }

    #[test]
    fn modal_open_auto_focuses_and_close_restores() {
        let mut core = RunnerCore::new();
        let mut closed = build_modal_tree(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        // Open: snapshot the trigger, focus the modal's first control.
        let mut open = build_modal_tree(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-ok"),
            "modal open should auto-focus its first focusable (issue #132)",
        );
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);

        // Close: restore the trigger.
        let mut closed_again = build_modal_tree(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
            "modal close should restore the pre-open focus",
        );
        assert!(core.ui_state.layer_focus.focus_stack.is_empty());
    }

    #[test]
    fn modal_autofocus_overrides_first_focusable() {
        use crate::widgets::button::button;
        use crate::widgets::overlay::{modal, overlay};
        // Confirm-style modal: the destructive action comes first, but
        // Cancel carries `.autofocus()` so it gets the default focus —
        // HTML's `autofocus` rule.
        let build = |open: bool| -> El {
            let mut layers: Vec<El> = vec![button("Trigger").key("trigger")];
            if open {
                layers.push(modal(
                    "confirm",
                    "Delete everything?",
                    [
                        button("Delete").key("m-delete"),
                        button("Cancel").key("m-cancel").autofocus(),
                    ],
                ));
            }
            overlay(layers).padding(20.0)
        };
        let mut core = RunnerCore::new();
        let mut closed = build(false);
        run_frame(&mut core, &mut closed);
        let mut open = build(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-cancel"),
            "an autofocus-flagged child should win over the first focusable",
        );
    }

    #[test]
    fn modal_traps_tab_inside_its_subtree() {
        let mut core = RunnerCore::new();
        let mut open = build_modal_tree(true);
        run_frame(&mut core, &mut open);

        // The scoped focus order must contain only the modal's
        // controls — everything behind the scrim is inert.
        let keys: Vec<&str> = core
            .ui_state
            .focus
            .order
            .iter()
            .map(|t| t.key.as_str())
            .collect();
        assert_eq!(
            keys,
            vec!["m-ok", "m-cancel"],
            "focus order should be scoped to the open modal",
        );

        // Tab wraps within the modal instead of walking behind it.
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-ok"),
        );
        for expected in ["m-cancel", "m-ok", "m-cancel"] {
            let _ = core.key_down(
                LogicalKey::Named(NamedKey::Tab),
                PhysicalKey::Unidentified,
                KeyModifiers::default(),
                false,
            );
            assert_eq!(
                core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
                Some(expected),
                "Tab must cycle inside the modal",
            );
        }

        // An explicit focus request for an inert widget is dropped.
        core.ui_state.push_focus_requests(vec!["other".into()]);
        let mut open_again = build_modal_tree(true);
        run_frame(&mut core, &mut open_again);
        assert_ne!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("other"),
            "focus requests must not escape an open modal",
        );
    }

    #[test]
    fn popover_above_modal_stays_reachable() {
        use crate::widgets::button::button;
        use crate::widgets::overlay::{modal, overlay};
        use crate::widgets::popover::{dropdown, menu_item};
        // A dropdown opened from a control inside the modal is composed
        // as a later sibling layer (the documented overlays() pattern),
        // not a descendant. It must stay in the scoped Tab order.
        let build = |menu_open: bool| -> El {
            let mut layers: Vec<El> = vec![
                button("Outside").key("outside"),
                modal("settings", "Settings", [button("Choose…").key("m-choose")]),
            ];
            if menu_open {
                layers.push(dropdown(
                    "menu",
                    "m-choose",
                    [menu_item("A").key("item-a"), menu_item("B").key("item-b")],
                ));
            }
            overlay(layers).padding(20.0)
        };
        let mut core = RunnerCore::new();
        let mut modal_only = build(false);
        run_frame(&mut core, &mut modal_only);
        let mut with_menu = build(true);
        run_frame(&mut core, &mut with_menu);

        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("item-a"),
            "the dropdown above the modal should auto-focus normally",
        );
        let keys: Vec<&str> = core
            .ui_state
            .focus
            .order
            .iter()
            .map(|t| t.key.as_str())
            .collect();
        assert!(
            keys.contains(&"item-a") && keys.contains(&"m-choose"),
            "scoped order should span the modal and the menu above it; got {keys:?}",
        );
        assert!(
            !keys.contains(&"outside"),
            "controls behind the modal stay inert; got {keys:?}",
        );
    }

    #[test]
    fn sheet_gets_the_layer_focus_lifecycle() {
        use crate::widgets::button::button;
        use crate::widgets::overlay::overlay;
        use crate::widgets::sheet::{SheetSide, sheet};
        let build = |open: bool| -> El {
            let mut layers: Vec<El> = vec![button("Trigger").key("trigger")];
            if open {
                layers.push(sheet(
                    "filters",
                    SheetSide::Right,
                    [button("Apply").key("s-apply")],
                ));
            }
            overlay(layers).padding(20.0)
        };
        let mut core = RunnerCore::new();
        let mut closed = build(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        let mut open = build(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("s-apply"),
            "sheet open should auto-focus like a modal",
        );

        let mut closed_again = build(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
            "sheet close should restore the pre-open focus",
        );
    }

    #[test]
    fn sibling_layer_churn_does_not_reopen_the_modal() {
        use crate::widgets::button::button;
        use crate::widgets::overlay::{modal, overlay};
        // A modal's identity must not depend on its slot index: when a
        // sibling overlay slot (toast, banner) closes, the modal's
        // computed_id must stay put — an id change would read as
        // close+reopen and re-run auto-focus, yanking the user's Tab
        // position mid-form. The stock wrappers pin this with a
        // `{key}:layer` key.
        let build = |banner: bool| -> El {
            let mut layers: Vec<El> = vec![button("Main").key("main")];
            if banner {
                layers.push(button("Banner").key("banner"));
            }
            layers.push(modal(
                "form",
                "Edit",
                [button("A").key("m-a"), button("B").key("m-b")],
            ));
            overlay(layers).padding(20.0)
        };
        let mut core = RunnerCore::new();
        let mut with_banner = build(true);
        run_frame(&mut core, &mut with_banner);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-a"),
        );
        // User tabs to the second control.
        let _ = core.key_down(
            LogicalKey::Named(NamedKey::Tab),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-b"),
        );
        // The banner slot closes; the modal stays. Focus must not be
        // yanked back to the auto-focus target.
        let mut without_banner = build(false);
        run_frame(&mut core, &mut without_banner);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-b"),
            "sibling layer churn must not re-trigger the modal's auto-focus",
        );
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);
    }

    #[test]
    fn modal_without_focusables_blurs_behind_scrim_focus() {
        use crate::widgets::button::button;
        use crate::widgets::overlay::{modal, overlay};
        use crate::widgets::text::text;
        // An info-style modal with nothing focusable inside must not
        // leave keyboard routing on the control behind the scrim —
        // Enter would activate a button the user can't see or click.
        let build = |open: bool| -> El {
            let mut layers: Vec<El> = vec![button("Trigger").key("trigger")];
            if open {
                layers.push(modal("info", "Heads up", [text("Read this.")]));
            }
            overlay(layers).padding(20.0)
        };
        let mut core = RunnerCore::new();
        let mut closed = build(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        let mut open = build(true);
        run_frame(&mut core, &mut open);
        assert!(
            core.ui_state.focused.is_none(),
            "behind-scrim focus must blur when the modal has no focusables",
        );
        let events = core.key_down(
            LogicalKey::Named(NamedKey::Enter),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert!(
            !events.iter().any(|e| e.key.as_deref() == Some("trigger")),
            "Enter must not reach the control behind the scrim",
        );

        // Close still restores: the saved entry survives the blur.
        let mut closed_again = build(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
            "the pre-open focus should come back on close",
        );
    }

    #[test]
    fn layer_opened_with_nothing_focused_keeps_stack_paired() {
        // A modal that opens as the very first thing shown (nothing
        // focused yet — Rumble's unlock prompt) must still push a
        // stack entry, so a layer stacked above it pops its OWN saved
        // focus on close instead of stealing an outer entry.
        use crate::widgets::button::button;
        use crate::widgets::overlay::{modal, overlay};
        use crate::widgets::popover::{dropdown, menu_item};
        let build = |modal_open: bool, menu_open: bool| -> El {
            let mut layers: Vec<El> = vec![button("Main").key("main")];
            if modal_open {
                layers.push(modal("m", "Modal", [button("Pick").key("m-pick")]));
            }
            if menu_open {
                layers.push(dropdown("menu", "m-pick", [menu_item("A").key("item-a")]));
            }
            overlay(layers).padding(20.0)
        };
        let mut core = RunnerCore::new();
        // Modal opens with nothing focused: one (empty) stack entry.
        let mut modal_only = build(true, false);
        run_frame(&mut core, &mut modal_only);
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-pick"),
        );

        // Menu above it: second entry saves m-pick.
        let mut both = build(true, true);
        run_frame(&mut core, &mut both);
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 2);

        // Menu closes: m-pick restored (not the modal's None entry).
        let mut modal_again = build(true, false);
        run_frame(&mut core, &mut modal_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("m-pick"),
            "the menu must pop its own saved focus",
        );
        assert_eq!(core.ui_state.layer_focus.focus_stack.len(), 1);

        // Modal closes: nothing to restore, stack drained cleanly.
        let mut none = build(false, false);
        run_frame(&mut core, &mut none);
        assert!(core.ui_state.layer_focus.focus_stack.is_empty());
    }

    #[test]
    fn arrow_nav_does_not_intercept_outside_navigable_groups() {
        // Reuse the input tree (no arrow_nav_siblings parent). Arrow
        // keys must produce a regular `KeyDown` event so a
        // capture_keys widget can interpret them as caret motion.
        let mut core = lay_out_input_tree(false);
        let target = core
            .ui_state
            .focus
            .order
            .iter()
            .find(|t| t.key == "btn")
            .cloned();
        core.ui_state.set_focus(target);
        let events = core.key_down(
            LogicalKey::Named(NamedKey::ArrowDown),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        );
        assert_eq!(
            events.len(),
            1,
            "ArrowDown without navigable parent → event"
        );
        assert_eq!(events[0].kind, UiEventKind::KeyDown);
    }

    fn quad(shader: ShaderHandle) -> DrawOp {
        DrawOp::Quad {
            id: "q".into(),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            scissor: None,
            shader,
            uniforms: UniformBlock::new(),
        }
    }

    #[test]
    fn prepare_paint_skips_ops_outside_viewport() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        core.viewport_px = (100, 100);
        let ops = vec![
            DrawOp::Quad {
                id: "offscreen".into(),
                rect: Rect::new(0.0, 150.0, 10.0, 10.0),
                scissor: None,
                shader: ShaderHandle::Stock(StockShader::RoundedRect),
                uniforms: UniformBlock::new(),
            },
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(&ops, |_| true, |_| false, &mut NoText, 1.0, &mut timings);

        assert_eq!(timings.paint_culled_ops, 1);
        assert_eq!(
            core.runs.len(),
            1,
            "only the visible quad should become a paint run"
        );
    }

    #[test]
    fn prepare_paint_does_not_shape_text_outside_clip() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        core.viewport_px = (100, 100);
        let ops = vec![
            DrawOp::GlyphRun {
                id: "offscreen-text".into(),
                rect: Rect::new(0.0, 150.0, 80.0, 20.0),
                scissor: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
                shader: ShaderHandle::Stock(StockShader::Text),
                color: Color::srgb_u8a(255, 255, 255, 255),
                text: "offscreen".into(),
                size: 14.0,
                line_height: 20.0,
                family: Default::default(),
                mono_family: Default::default(),
                weight: FontWeight::Regular,
                mono: false,
                wrap: TextWrap::NoWrap,
                anchor: TextAnchor::Start,
                layout: empty_text_layout(20.0),
                underline: false,
                strikethrough: false,
                link: None,
                tabular_numerals: false,
                letter_spacing: 0.0,
            },
            DrawOp::GlyphRun {
                id: "visible-text".into(),
                rect: Rect::new(0.0, 10.0, 80.0, 20.0),
                scissor: Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
                shader: ShaderHandle::Stock(StockShader::Text),
                color: Color::srgb_u8a(255, 255, 255, 255),
                text: "visible".into(),
                size: 14.0,
                line_height: 20.0,
                family: Default::default(),
                mono_family: Default::default(),
                weight: FontWeight::Regular,
                mono: false,
                wrap: TextWrap::NoWrap,
                anchor: TextAnchor::Start,
                layout: empty_text_layout(20.0),
                underline: false,
                strikethrough: false,
                link: None,
                tabular_numerals: false,
                letter_spacing: 0.0,
            },
        ];
        let mut text = CountingText::default();
        let mut timings = PrepareTimings::default();
        core.prepare_paint(&ops, |_| true, |_| false, &mut text, 1.0, &mut timings);

        assert_eq!(timings.paint_culled_ops, 1);
        assert_eq!(text.records, 1, "offscreen text must not be shaped");
    }

    #[test]
    fn samples_backdrop_inserts_snapshot_before_first_glass_quad() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let ops = vec![
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Custom("liquid_glass")),
            quad(ShaderHandle::Custom("liquid_glass")),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(
            &ops,
            |_| true,
            |s| matches!(s, ShaderHandle::Custom(name) if *name == "liquid_glass"),
            &mut NoText,
            1.0,
            &mut timings,
        );

        let kinds: Vec<&'static str> = core
            .paint_items
            .iter()
            .map(|p| match p {
                PaintItem::QuadRun(_) => "Q",
                PaintItem::IconRun(_) => "I",
                PaintItem::Text(_) => "T",
                PaintItem::Image(_) => "M",
                PaintItem::AppTexture(_) => "A",
                PaintItem::Vector(_) => "V",
                PaintItem::Scene3D(_) => "3",
                PaintItem::BackdropSnapshot => "S",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["Q", "S", "Q", "Q"],
            "expected one stock run, snapshot, then a glass run, then a foreground stock run"
        );
    }

    #[test]
    fn no_snapshot_when_no_glass_drawn() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let ops = vec![
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(&ops, |_| true, |_| false, &mut NoText, 1.0, &mut timings);
        assert!(
            !core
                .paint_items
                .iter()
                .any(|p| matches!(p, PaintItem::BackdropSnapshot)),
            "no glass shader registered → no snapshot"
        );
    }

    /// A `DrawOp::Scene3D` whose backend overrides `record_scene3d`
    /// produces a `PaintItem::Scene3D`; one that leaves the default no-op
    /// recorder produces nothing. This locks the prepare_paint wiring
    /// independent of any GPU backend.
    #[test]
    fn scene3d_op_emits_paint_item_only_when_recorder_records() {
        use crate::scene::{Aabb, CameraState, LightRig, Scene3DData, SceneStyle};

        fn scene_op() -> DrawOp {
            let scene = Scene3DData {
                meshes: Vec::new(),
                points: Vec::new(),
                lines: Vec::new(),
                camera: CameraState::default().resolve(Aabb::EMPTY),
                lights: LightRig::default(),
                style: SceneStyle::default(),
                capture_depth: false,
            };
            DrawOp::Scene3D {
                id: "scene".into(),
                rect: Rect::new(0.0, 0.0, 40.0, 40.0),
                scissor: None,
                scene: std::sync::Arc::new(scene),
            }
        }

        // Default recorder (NoText) never overrides record_scene3d → no item.
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let mut timings = PrepareTimings::default();
        core.prepare_paint(
            &[scene_op()],
            |_| true,
            |_| false,
            &mut NoText,
            1.0,
            &mut timings,
        );
        assert!(
            !core
                .paint_items
                .iter()
                .any(|p| matches!(p, PaintItem::Scene3D(_))),
            "default no-op recorder must not emit a Scene3D paint item",
        );

        // A recorder that records one scene → exactly one Scene3D item.
        struct SceneRecorder {
            calls: usize,
        }
        impl TextRecorder for SceneRecorder {
            fn record(
                &mut self,
                _: Rect,
                _: Option<PhysicalScissor>,
                _: &RunStyle,
                _: &str,
                _: f32,
                _: f32,
                _: TextWrap,
                _: TextAnchor,
                _: f32,
            ) -> Range<usize> {
                0..0
            }
            fn record_runs(
                &mut self,
                _: Rect,
                _: Option<PhysicalScissor>,
                _: &[(String, RunStyle)],
                _: f32,
                _: f32,
                _: TextWrap,
                _: TextAnchor,
                _: f32,
            ) -> Range<usize> {
                0..0
            }
            fn record_scene3d(
                &mut self,
                _: Rect,
                _: Option<PhysicalScissor>,
                id: &str,
                _: &std::sync::Arc<Scene3DData>,
                _: f32,
            ) -> Range<usize> {
                assert_eq!(id, "scene", "node id threads through to the recorder");
                let start = self.calls;
                self.calls += 1;
                start..self.calls
            }
        }

        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let mut rec = SceneRecorder { calls: 0 };
        let mut timings = PrepareTimings::default();
        core.prepare_paint(
            &[scene_op()],
            |_| true,
            |_| false,
            &mut rec,
            1.0,
            &mut timings,
        );
        let scenes = core
            .paint_items
            .iter()
            .filter(|p| matches!(p, PaintItem::Scene3D(_)))
            .count();
        assert_eq!(
            scenes, 1,
            "recorded scene must emit exactly one Scene3D item"
        );
    }

    /// A `chart3d` that is given a `.key(...)` must still orbit. Keying a
    /// node makes it a hit-test target, so a press over the scene now
    /// *hits* the scene's own node; the camera-drag gate must treat that
    /// hit as "nothing in the way" and still begin the drag. Regression for
    /// the bug where a keyed scene silently lost orbit/pan (only wheel-zoom,
    /// which bypasses the gate, kept working).
    #[test]
    fn keyed_scene_still_begins_camera_drag() {
        use crate::scene::glam::Vec3;
        use crate::scene::{PointData, PointsHandle, ScenePoint, SceneSpec};
        use crate::tree::chart3d;

        let spec = || {
            SceneSpec::new().points(PointsHandle::new(PointData {
                points: vec![
                    ScenePoint {
                        position: Vec3::splat(-1.0),
                        color: [1.0; 4],
                    },
                    ScenePoint {
                        position: Vec3::splat(1.0),
                        color: [1.0; 4],
                    },
                ],
            }))
        };

        // Lay out, tick the camera (so the scene registers a viewport rect
        // for hit-routing), snapshot for hit-testing, then press at centre.
        let drag_active_after_press = |mut tree: crate::tree::El| {
            let mut core = RunnerCore::new();
            crate::layout::layout(
                &mut tree,
                &mut core.ui_state,
                Rect::new(0.0, 0.0, 200.0, 200.0),
            );
            core.ui_state.tick_scene_cameras(&tree, Instant::now());
            let mut t = PrepareTimings::default();
            core.snapshot(&tree, &mut t);
            core.pointer_down(Pointer::mouse(100.0, 100.0, PointerButton::Primary));
            core.ui_state.camera_drag_active()
        };

        // Unkeyed: works today (baseline).
        assert!(
            drag_active_after_press(chart3d(spec())),
            "unkeyed scene should begin a camera drag"
        );
        // Keyed: the regression — must also begin a drag.
        assert!(
            drag_active_after_press(chart3d(spec()).key("scene")),
            "keyed scene must still begin a camera drag (its own node hit must not suppress it)"
        );
    }

    /// The plot's control scheme selects what the unmodified primary drag
    /// does: `ZoomDrag` (default) box-zooms and Shift pans; `PanDrag` is the
    /// inverse. Double-click reset and wheel are scheme-independent.
    #[test]
    fn plot_control_scheme_routes_primary_drag() {
        use crate::event::KeyModifiers;
        use crate::plot::{PlotControls, PlotSpec, Sample, Scale, SeriesHandle, line};
        use crate::tree::plot;

        let make = |controls| {
            let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(10.0, 10.0)]);
            plot(
                PlotSpec::new()
                    .x(Scale::linear())
                    .y(Scale::linear())
                    .add_mark(line(&h))
                    .controls(controls),
            )
            .key("p")
        };

        // (zoom_active, pan_active) after a centre press, with/without Shift.
        let press = |mut tree: crate::tree::El, shift: bool| {
            let mut core = RunnerCore::new();
            crate::layout::layout(
                &mut tree,
                &mut core.ui_state,
                Rect::new(0.0, 0.0, 200.0, 200.0),
            );
            core.ui_state.prepare_plots(&tree);
            let mut t = PrepareTimings::default();
            core.snapshot(&tree, &mut t);
            core.ui_state.set_modifiers(KeyModifiers {
                shift,
                ..Default::default()
            });
            core.pointer_down(Pointer::mouse(100.0, 100.0, PointerButton::Primary));
            (
                core.ui_state.plot_zoom_active(),
                core.ui_state.plot_pan_active(),
            )
        };

        // ZoomDrag (default): plain drag box-zooms, Shift pans.
        assert_eq!(press(make(PlotControls::ZoomDrag), false), (true, false));
        assert_eq!(press(make(PlotControls::ZoomDrag), true), (false, true));
        // PanDrag: plain drag pans, Shift box-zooms.
        assert_eq!(press(make(PlotControls::PanDrag), false), (false, true));
        assert_eq!(press(make(PlotControls::PanDrag), true), (true, false));
    }

    #[test]
    fn at_most_one_snapshot_per_frame() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let ops = vec![
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Custom("g")),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Custom("g")),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(
            &ops,
            |_| true,
            |s| matches!(s, ShaderHandle::Custom("g")),
            &mut NoText,
            1.0,
            &mut timings,
        );
        let snapshots = core
            .paint_items
            .iter()
            .filter(|p| matches!(p, PaintItem::BackdropSnapshot))
            .count();
        assert_eq!(snapshots, 1, "backdrop depth is capped at 1");
    }
}
