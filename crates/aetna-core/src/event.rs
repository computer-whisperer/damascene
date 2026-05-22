//! Event types and the [`App`] trait.
//!
//! State-driven rebuilds, routed events, keyboard input, and automatic
//! hover/press/focus visuals. See `docs/LIBRARY_VISION.md` for the application
//! model this fits into.
//!
//! This module owns the *types* — what the host's `App::on_event` sees
//! and what gets registered as hotkeys. The state machine that produces
//! these events lives in [`crate::state::UiState`]; the routing helpers
//! live in [`mod@crate::hit_test`] and [`mod@crate::focus`].
//!
//! # The model
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Counter { value: i32 }
//!
//! impl App for Counter {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             h1(format!("{}", self.value)),
//!             row([
//!                 button("-").key("dec"),
//!                 button("+").key("inc"),
//!             ]),
//!         ])
//!     }
//!     fn on_event(&mut self, e: UiEvent) {
//!         if e.is_click_or_activate("inc") {
//!             self.value += 1;
//!         } else if e.is_click_or_activate("dec") {
//!             self.value -= 1;
//!         }
//!     }
//! }
//! ```
//!
//! - **Identity** is `El::key`. Tag a node with `.key("...")` and it's
//!   hit-testable (and gets automatic hover/press visuals).
//! - **The build closure is pure.** It reads `&self`, returns a fresh
//!   tree. The library tracks pointer state, hovered key, pressed key
//!   internally and applies visual deltas after build but before layout
//!   completes.
//! - **Events flow back via `on_event`.** The library hit-tests pointer
//!   events against the most-recently-laid-out tree and emits
//!   [`UiEvent`]s when something is clicked. The host's `App::on_event`
//!   updates state; the renderer reports whether animation state needs
//!   another redraw.

use crate::tree::{El, Rect};

/// Hit-test target metadata. `key` is the author-facing route, while
/// `node_id` is the stable laid-out tree path used by artifacts.
///
/// `tooltip` snapshots the node's tooltip text at the moment the
/// target was constructed, so the tooltip pass doesn't have to walk
/// the live tree to resolve it. This is what makes tooltips work on
/// virtual-list rows: hit-testing reads `last_tree` (where the row
/// has been realized), and the cached text survives into the next
/// frame's `synthesize_tooltip` even though that frame's tree hasn't
/// rebuilt its virtual-list children yet.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct UiTarget {
    pub key: String,
    pub node_id: String,
    pub rect: Rect,
    pub tooltip: Option<String>,
    /// Scroll offset of the deepest scroll subtree inside this hit
    /// target, in logical pixels. `0.0` for widgets that don't
    /// contain a scroll. Used by widgets like
    /// [`crate::widgets::text_area`] to convert a pointer in viewport
    /// space (what the user clicks) into content space (what
    /// cosmic-text's `hit_byte` and `caret_xy` work in) — without
    /// this, clicks after scrolling land on the wrong line because
    /// the content has been shifted up by `scroll_offset_y` while
    /// the outer's `rect` hasn't moved.
    pub scroll_offset_y: f32,
}

/// Which mouse button (or pointer button) generated a pointer event.
/// The host backend translates its native button id to one of these
/// before calling `pointer_down` / `pointer_up`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    /// Left mouse, primary touch, or pen tip. Drives `Click`.
    Primary,
    /// Right mouse or two-finger touch. Drives `SecondaryClick` —
    /// typically opens a context menu.
    Secondary,
    /// Middle mouse / scroll-wheel click. No library default; surfaced
    /// as `MiddleClick` for apps that want it (autoscroll, paste-on-X).
    Middle,
}

/// Physical kind of pointer that produced an event. Mirrors the DOM
/// `PointerEvent.pointerType`. Backends without a real signal pass
/// [`PointerKind::Mouse`].
///
/// The runtime uses this to specialize behavior that does not transfer
/// across modalities — for example, `Touch` has no resting hover state
/// and gates `PointerEnter`/`PointerLeave` accordingly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PointerKind {
    /// Mouse, trackpad, or any device that reports continuous hover.
    #[default]
    Mouse,
    /// Touchscreen. No hover state; contact starts with `pointer_down`.
    Touch,
    /// Pen / stylus. Behaves like `Mouse` for hover, but backends may
    /// surface pressure in [`Pointer::pressure`].
    Pen,
}

/// Stable per-pointer identifier within a frame. Mirrors the DOM
/// `PointerEvent.pointerId`. Backends with only one pointer pass
/// [`PointerId::PRIMARY`]; multi-touch backends keep IDs stable for the
/// lifetime of a single contact.
///
/// The runtime currently routes only the primary contact; secondary IDs
/// are reserved for future multi-touch / gesture work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PointerId(pub u32);

impl PointerId {
    /// The conventional ID for backends that have only one pointer
    /// (mouse-only hosts, synthetic test events, the first touch
    /// contact when multi-touch IDs are not tracked).
    pub const PRIMARY: PointerId = PointerId(0);
}

/// One pointer sample, in logical pixels. The argument shape for
/// [`crate::runtime::RunnerCore::pointer_moved`],
/// [`crate::runtime::RunnerCore::pointer_down`], and
/// [`crate::runtime::RunnerCore::pointer_up`].
///
/// Modeled on the DOM `PointerEvent` interface so backends that
/// already speak browser pointer events can map fields directly.
/// `button` is meaningful on `pointer_down` / `pointer_up` and is
/// ignored on `pointer_moved`; constructors default it to
/// [`PointerButton::Primary`] for that case.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// X coordinate in logical pixels relative to the window origin.
    pub x: f32,
    /// Y coordinate in logical pixels relative to the window origin.
    pub y: f32,
    /// Which button this event refers to. Ignored by `pointer_moved`.
    pub button: PointerButton,
    /// Physical kind of pointer (mouse / touch / pen).
    pub kind: PointerKind,
    /// Stable per-pointer ID. Use [`PointerId::PRIMARY`] for
    /// single-pointer backends.
    pub id: PointerId,
    /// Normalized pressure in `0.0..=1.0` when the device reports it
    /// (pen, force-touch). `None` when unavailable; mouse backends
    /// always pass `None`.
    pub pressure: Option<f32>,
}

impl Pointer {
    /// A mouse-driven pointer at `(x, y)` for the given button. Use
    /// from mouse-only hosts and synthetic tests.
    pub fn mouse(x: f32, y: f32, button: PointerButton) -> Self {
        Self {
            x,
            y,
            button,
            kind: PointerKind::Mouse,
            id: PointerId::PRIMARY,
            pressure: None,
        }
    }

    /// A mouse pointer for `pointer_moved`, where `button` is
    /// irrelevant. Equivalent to
    /// [`Pointer::mouse(x, y, PointerButton::Primary)`][Self::mouse].
    pub fn moving(x: f32, y: f32) -> Self {
        Self::mouse(x, y, PointerButton::Primary)
    }

    /// A touch contact at `(x, y)` carrying the given pointer ID.
    /// Backends translating browser `PointerEvent` should pass the
    /// browser's `pointerId` directly.
    pub fn touch(x: f32, y: f32, button: PointerButton, id: PointerId) -> Self {
        Self {
            x,
            y,
            button,
            kind: PointerKind::Touch,
            id,
            pressure: None,
        }
    }
}

/// Keyboard key values normalized by the core library. This keeps the
/// core independent from host/windowing crates while covering the
/// navigation and activation keys the library owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiKey {
    Enter,
    Escape,
    Tab,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// Backspace — deletes the grapheme before the caret.
    Backspace,
    /// Forward delete — deletes the grapheme after the caret.
    Delete,
    /// Home — caret to start of line.
    Home,
    /// End — caret to end of line.
    End,
    /// PageUp — coarse-step navigation (sliders adjust by a larger
    /// amount; lists scroll a viewport).
    PageUp,
    /// PageDown — coarse-step navigation (sliders adjust by a larger
    /// amount; lists scroll a viewport).
    PageDown,
    Character(String),
    Other(String),
}

/// OS modifier-key mask. The four fields mirror the platform-standard
/// modifier set; this struct is intentionally **not** `#[non_exhaustive]`
/// so callers can use struct-literal syntax with `..Default::default()`
/// to spell precise modifier combinations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyPress {
    pub key: UiKey,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
}

/// A keyboard chord for app-level hotkey registration. Match a key with
/// an exact modifier mask: `KeyChord::ctrl('f')` does not also match
/// `Ctrl+Shift+F`, and `KeyChord::vim('j')` does not match if any
/// modifier is held.
///
/// Register chords from [`App::hotkeys`]; the library matches them
/// against incoming key presses ahead of focus activation routing and
/// emits a [`UiEvent`] with `kind = UiEventKind::Hotkey` and `key`
/// equal to the registered name.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyChord {
    pub key: UiKey,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    /// A bare key with no modifiers (vim-style). `KeyChord::vim('j')`
    /// matches the `j` key with no Ctrl/Shift/Alt/Logo held.
    pub fn vim(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers::default(),
        }
    }

    /// `Ctrl+<char>`.
    pub fn ctrl(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
        }
    }

    /// `Ctrl+Shift+<char>`.
    pub fn ctrl_shift(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        }
    }

    /// A named key with no modifiers (e.g. `KeyChord::named(UiKey::Escape)`).
    pub fn named(key: UiKey) -> Self {
        Self {
            key,
            modifiers: KeyModifiers::default(),
        }
    }

    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Strict match: keys equal AND modifier mask is identical. Holding
    /// extra modifiers does not match a chord that didn't request them.
    pub fn matches(&self, key: &UiKey, modifiers: KeyModifiers) -> bool {
        key_eq(&self.key, key) && self.modifiers == modifiers
    }
}

fn key_eq(a: &UiKey, b: &UiKey) -> bool {
    match (a, b) {
        (UiKey::Character(x), UiKey::Character(y)) => x.eq_ignore_ascii_case(y),
        _ => a == b,
    }
}

/// User-facing event. The host's [`App::on_event`] receives one of these
/// per discrete user action.
///
/// Most apps should not destructure every field. Prefer the convenience
/// methods on this type for common routes:
///
/// ```
/// # use aetna_core::prelude::*;
/// # struct Counter { value: i32 }
/// # impl App for Counter {
/// # fn build(&self, _cx: &BuildCx) -> El { button("+").key("inc") }
/// fn on_event(&mut self, event: UiEvent) {
///     if event.is_click_or_activate("inc") {
///         self.value += 1;
///     }
/// }
/// # }
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct UiEvent {
    /// Route string for this event.
    ///
    /// For pointer and focus events, this is the [`El::key`][crate::El::key]
    /// of the target node. For [`UiEventKind::Hotkey`], this is the
    /// action name returned from [`App::hotkeys`]. For window-level
    /// keyboard events such as Escape with no focused target, this is
    /// `None`.
    ///
    /// Prefer [`Self::route`] or [`Self::is_click_or_activate`] in app
    /// code. The field remains public for direct pattern matching.
    pub key: Option<String>,
    /// Full hit-test target for events routed to a concrete element.
    pub target: Option<UiTarget>,
    /// Pointer position in logical pixels when the event was emitted.
    pub pointer: Option<(f32, f32)>,
    /// Keyboard payload for key events.
    pub key_press: Option<KeyPress>,
    /// Composed text payload for [`UiEventKind::TextInput`] events.
    pub text: Option<String>,
    /// Library-emitted selection state for
    /// [`UiEventKind::SelectionChanged`] events. Carries the new
    /// [`crate::selection::Selection`] after the runtime resolved a
    /// pointer interaction. The app folds this into its
    /// `Selection` field the same way it folds `apply_event` results
    /// into a [`crate::widgets::text_input::TextSelection`].
    pub selection: Option<crate::selection::Selection>,
    /// Modifier mask captured at the moment this event was emitted. For
    /// keyboard events this duplicates `key_press.modifiers`; for
    /// pointer events it's the host-tracked modifier state at the time
    /// of the click / drag (used by widgets like text_input that need
    /// to detect Shift+click for "extend selection").
    pub modifiers: KeyModifiers,
    /// Click number within a multi-click sequence. Set to 1 for single
    /// click, 2 for double-click, 3 for triple-click, etc. The runtime
    /// increments this when consecutive `PointerDown`s land on the same
    /// target within ~500ms and ~4px of the previous click. `Drag`
    /// events emitted while the final click is held keep the active
    /// sequence count so text widgets can preserve word / line
    /// granularity. `0` means "not applicable" — set on events outside
    /// pointer click / drag routing.
    ///
    /// `text_input` / `text_area` and the static-text selection
    /// manager read this to map double-click → select word, triple-
    /// click → select line.
    pub click_count: u8,
    /// File system path for [`UiEventKind::FileHovered`] /
    /// [`UiEventKind::FileDropped`] events. Multi-file drag-drops fire
    /// one event per file (matching the underlying winit semantics);
    /// each event carries one path. `PathBuf` rather than `String`
    /// because Windows wide-char paths and unusual Unix paths aren't
    /// guaranteed to be UTF-8.
    pub path: Option<std::path::PathBuf>,
    /// Modality of the pointer that produced this event. `None` for
    /// non-pointer events (hotkeys, keyboard activation, file drops
    /// without a tracked pointer). Apps that need to specialize for
    /// touch (accessibility, analytics, alternate affordances) read
    /// this; most app code can ignore it.
    pub pointer_kind: Option<PointerKind>,
    /// Wheel delta in logical pixels for [`UiEventKind::PointerWheel`].
    ///
    /// Positive `dy` means "scroll down" in the same coordinate system
    /// used by Aetna's scroll containers. Hosts normalize line-based
    /// and pixel-based wheel input before setting this field.
    pub wheel_delta: Option<(f32, f32)>,
    pub kind: UiEventKind,
}

impl UiEvent {
    /// Synthesize a click event for the given route key.
    ///
    /// Intended for tests, headless automation, and snapshot
    /// fixtures that drive UI logic without a real pointer history.
    /// All optional fields default to `None`; modifiers are empty.
    pub fn synthetic_click(key: impl Into<String>) -> Self {
        Self {
            kind: UiEventKind::Click,
            key: Some(key.into()),
            target: None,
            pointer: None,
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
        }
    }

    /// Route string for this event, if any.
    ///
    /// For pointer/focus events this is the target element key. For
    /// hotkeys this is the registered action name.
    pub fn route(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Target element key, if this event was routed to an element.
    ///
    /// Unlike [`Self::route`], this returns `None` for app-level
    /// hotkey actions because those do not have a concrete element
    /// target.
    pub fn target_key(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.key.as_str())
    }

    /// True when this event's route equals `key`.
    pub fn is_route(&self, key: &str) -> bool {
        self.route() == Some(key)
    }

    /// True for a primary click or keyboard activation on `key`.
    ///
    /// This is the most common button/menu route in app code.
    pub fn is_click_or_activate(&self, key: &str) -> bool {
        matches!(self.kind, UiEventKind::Click | UiEventKind::Activate) && self.is_route(key)
    }

    /// True for a registered hotkey action name.
    pub fn is_hotkey(&self, action: &str) -> bool {
        self.kind == UiEventKind::Hotkey && self.is_route(action)
    }

    /// Pointer position in logical pixels, if this event carries one.
    pub fn pointer_pos(&self) -> Option<(f32, f32)> {
        self.pointer
    }

    /// Pointer x coordinate in logical pixels, if this event carries one.
    pub fn pointer_x(&self) -> Option<f32> {
        self.pointer.map(|(x, _)| x)
    }

    /// Pointer y coordinate in logical pixels, if this event carries one.
    pub fn pointer_y(&self) -> Option<f32> {
        self.pointer.map(|(_, y)| y)
    }

    /// Wheel delta in logical pixels, if this is a pointer wheel event.
    pub fn wheel_delta(&self) -> Option<(f32, f32)> {
        self.wheel_delta
    }

    /// Vertical wheel delta in logical pixels, if this is a pointer
    /// wheel event.
    pub fn wheel_dy(&self) -> Option<f32> {
        self.wheel_delta.map(|(_, dy)| dy)
    }

    /// Rectangle of the routed target from the last layout pass.
    /// This is the target's transformed visual rect, not any
    /// `hit_overflow` band that may also route pointer events to it.
    pub fn target_rect(&self) -> Option<Rect> {
        self.target.as_ref().map(|t| t.rect)
    }

    /// OS-composed text payload for [`UiEventKind::TextInput`].
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }
}

/// What kind of event happened.
///
/// This enum is non-exhaustive so Aetna can add new input events
/// without breaking downstream apps. Match the variants you handle and
/// include a wildcard arm for everything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiEventKind {
    /// Primary-button pointer down + up landed on the same node.
    Click,
    /// Primary-button click landed on a text run carrying a
    /// [`crate::tree::El::text_link`] URL. The URL is in [`UiEvent::key`].
    /// Apps decide whether to honor it (filtering, confirmation,
    /// platform-appropriate open via [`App::drain_link_opens`] +
    /// host-side opener). Aetna doesn't open URLs itself — it surfaces
    /// the click and lets the app route it.
    LinkActivated,
    /// Secondary-button (right-click) pointer down + up landed on the
    /// same node. Used for context menus.
    SecondaryClick,
    /// Middle-button pointer down + up landed on the same node.
    MiddleClick,
    /// Focused element was activated by keyboard (Enter/Space).
    Activate,
    /// Escape was pressed. Routed to the focused element when present,
    /// otherwise emitted as a window-level event.
    Escape,
    /// A registered hotkey chord matched. `event.key` is the registered
    /// name (the second element of the `(KeyChord, String)` pair).
    Hotkey,
    /// Other keyboard input.
    KeyDown,
    /// Composed text input — printable characters from the OS, after
    /// dead-key composition / IME / shift mapping. Routed to the
    /// focused element. Distinct from `KeyDown(Character(_))`: the
    /// latter is the raw key event used for shortcuts and navigation;
    /// `TextInput` is the grapheme stream a text field should consume.
    TextInput,
    /// Pointer moved while the primary button was held down. Routed
    /// to the originally pressed target so a widget can extend a
    /// selection / scrub a slider / move a draggable. `event.pointer`
    /// carries the current logical-pixel position; `event.target` is
    /// the node where the drag began.
    Drag,
    /// Primary pointer button released. Fires regardless of whether
    /// the up landed on the same node as the down — paired with
    /// `Click` (which only fires on a same-node match), this lets
    /// drag-aware widgets always observe drag-end.
    /// `event.target` is the originally pressed node;
    /// `event.pointer` is the up position.
    PointerUp,
    /// Primary pointer button pressed on a hit-test target. Routed
    /// before the eventual `Click` (which fires on up-on-same-target).
    /// Used by widgets like text_input that need to react at
    /// down-time — e.g., to set the selection anchor before any drag
    /// extends it. `event.target` is the down-target,
    /// `event.pointer` is the down position, and `event.modifiers`
    /// carries the modifier mask (Shift+click for extend-selection).
    PointerDown,
    /// Mouse wheel / trackpad scroll input routed to the keyed element
    /// under the pointer. Emitted before Aetna's default scroll
    /// handling; apps can consume it by returning `true` from
    /// [`App::on_wheel_event`]. `event.wheel_delta` carries the
    /// normalized logical-pixel delta.
    PointerWheel,
    /// The library's selection manager resolved a pointer interaction
    /// on selectable text and wants the app to update its
    /// [`crate::selection::Selection`] state. `event.selection`
    /// carries the new value (an empty `Selection` clears).
    /// Emitted by `pointer_down`, `pointer_moved` (during a drag),
    /// and the runtime's escape / dismiss paths.
    SelectionChanged,
    /// Pointer crossed onto a keyed hit-test target. Routed to the
    /// newly hovered leaf — `event.target` is the new hover target,
    /// `event.pointer` is the current pointer position. Fires
    /// once per identity change, including the initial hover when the
    /// pointer first enters a keyed region from nothing.
    ///
    /// Use for transition-driven side effects (sound on hover-enter,
    /// analytics, hover-intent prefetch) — read state via
    /// [`crate::BuildCx::hovered_key`] /
    /// [`crate::BuildCx::is_hovering_within`] when you just need to
    /// branch the build output. Both surfaces stay coherent because
    /// the runtime debounces redraws and events to the same
    /// hover-identity transitions.
    ///
    /// Always paired with a preceding `PointerLeave` for the previous
    /// target (when there was one). Apps that want subtree-aware
    /// behavior (parent stays "hot" while a child is hovered) should
    /// query `is_hovering_within` rather than tracking enter/leave on
    /// every keyed descendant.
    PointerEnter,
    /// Pointer crossed off a keyed hit-test target — either onto a
    /// different keyed target (paired with a following `PointerEnter`)
    /// or off any keyed surface entirely. Routed to the leaf that
    /// just lost hover — `event.target` is the previous hover target,
    /// `event.pointer` is the current pointer position (or the last
    /// known position when the pointer left the window).
    PointerLeave,
    /// The runner is abandoning a press because the gesture became
    /// something else — currently only fired when a touch contact's
    /// movement crosses the touch-scroll threshold and the press
    /// target did not opt in via `consumes_touch_drag`. The contact
    /// has *not* lifted; the user is still touching the screen, but
    /// from the widget's perspective the press is gone (no
    /// subsequent `Drag`, no `Click`, no `PointerUp`). Routed to the
    /// originally pressed target — apps that handle `PointerDown`
    /// for in-flight visual / state setup should also handle
    /// `PointerCancel` to roll it back.
    ///
    /// Browser-initiated pointer cancels (OS gesture takeover, etc.)
    /// currently come through as `PointerUp` rather than this event;
    /// that may change.
    PointerCancel,
    /// A touch contact has been held in place past
    /// [`crate::state::LONG_PRESS_DELAY`] without lifting or moving
    /// past the gesture threshold. Fired exactly once per qualifying
    /// press. For normal targets this is fired immediately after a
    /// `PointerCancel` is dispatched to the originally pressed target;
    /// the underlying primary press is consumed by the long-press, so
    /// no subsequent `Click` or `PointerUp` follows. Capture-keys
    /// editable targets keep the press captured so movement after the
    /// long-press can emit `Drag` to extend text selection. The
    /// eventual finger lift is silently swallowed.
    ///
    /// `event.target` is the keyed leaf at the press point (same
    /// node that received the cancelled `PointerDown`), `event.pointer`
    /// is the original press coords (not the current finger position
    /// — the contact may have drifted within the gesture-threshold
    /// radius before firing), and `event.pointer_kind` is always
    /// `PointerKind::Touch`.
    ///
    /// Mouse and pen pointers never produce this event — right-click
    /// goes through `PointerDown` with [`PointerButton::Secondary`]
    /// instead, which is the desktop-shape signal for the same
    /// "open a context menu here" intent. Apps that want both paths
    /// to drive the same menu match on either kind.
    LongPress,
    /// A file is being dragged over the window (the user hasn't
    /// released yet). `event.path` carries the file's path; multi-file
    /// drags fire one event per file, matching the underlying winit
    /// semantics. `event.target` is the keyed leaf at the current
    /// pointer position when one was hit, otherwise `None`
    /// (drop-zone overlays that span the window can match on
    /// `event.target.is_none()` or filter by their own key).
    ///
    /// Apps use this to highlight a drop zone before the drop lands.
    /// Always paired with either a later `FileHoverCancelled` (the
    /// user moved off without releasing) or `FileDropped` (the user
    /// released).
    FileHovered,
    /// The user moved a hovered file off the window without releasing,
    /// or pressed Escape. Window-level event (`event.target` is
    /// `None`) — apps clear any drop-zone affordance state regardless
    /// of which keyed leaf was previously highlighted.
    FileHoverCancelled,
    /// A file was dropped on the window. `event.path` carries the
    /// path; multi-file drops fire one event per file. `event.target`
    /// is the keyed leaf at the drop position, or `None` if the drop
    /// landed outside any keyed surface — apps that want a global drop
    /// target match on `target.is_none()` or treat unrouted events as
    /// hits to a single window-level upload sink.
    FileDropped,
}

/// Per-frame, read-only context for [`App::build`].
///
/// The runner snapshots the app's [`crate::Theme`] before calling
/// `build` and exposes it through `cx.theme()` / `cx.palette()` so app
/// code can branch on the active palette (a custom widget that picks
/// between two non-token colors based on dark vs. light, for instance).
/// `BuildCx` is the explicit handle for this — token references inside
/// widgets resolve through the palette automatically and don't need it.
///
/// Future fields like viewport metrics or frame phase will live here so
/// the API stays additive: adding a new accessor on `BuildCx` doesn't
/// break apps that ignore the context.
#[derive(Copy, Clone, Debug)]
pub struct BuildCx<'a> {
    theme: &'a crate::Theme,
    ui_state: Option<&'a crate::state::UiState>,
    diagnostics: Option<&'a HostDiagnostics>,
    /// Logical-pixel viewport this frame is being built for, when the
    /// host attached one. Apps query this via [`Self::viewport`] /
    /// [`Self::viewport_below`] to branch layout on phone-vs-desktop
    /// without threading the surface size through their own state.
    viewport: Option<(f32, f32)>,
    /// Logical-pixel insets the host wants the app to inset its
    /// layout by — content underneath these bands is obscured by
    /// platform chrome and shouldn't host interactive widgets.
    /// Today only the bottom inset is populated, by the web host's
    /// VisualViewport listener when the on-screen keyboard appears;
    /// the same field will carry status-bar / notch / home-indicator
    /// insets when native mobile hosts land.
    safe_area: Option<crate::tree::Sides>,
}

/// Why the current frame is being built. Hosts set this before each
/// `request_redraw` so apps that surface a diagnostic overlay can show
/// what kind of input is driving the redraw cadence.
///
/// `Other` is the conservative default: it covers redraws the host
/// can't attribute (idle redraws driven by external `request_redraw`
/// callers, the initial paint, etc.). Specific variants narrow the
/// reason when the host can.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FrameTrigger {
    /// Host can't attribute the redraw to a specific cause.
    #[default]
    Other,
    /// Initial paint after surface configuration.
    Initial,
    /// Surface resize / DPI change.
    Resize,
    /// Pointer move, button, or wheel.
    Pointer,
    /// Keyboard / IME input.
    Keyboard,
    /// Inside-out animation deadline elapsed (one of the visible
    /// widgets asked for a future frame via `redraw_within`, or a
    /// visual animation is still settling). Drives the layout-path
    /// (full rebuild + prepare).
    Animation,
    /// Time-driven shader deadline elapsed (e.g. stock spinner /
    /// skeleton / progress-indeterminate, or a custom shader
    /// registered with `samples_time=true`). Drives the paint-only
    /// path: `frame.time` advances but layout state is unchanged.
    ShaderPaint,
    /// Periodic host-config cadence (`HostConfig::redraw_interval`).
    Periodic,
}

impl FrameTrigger {
    /// Short, fixed-width tag for diagnostic overlays.
    pub fn label(self) -> &'static str {
        match self {
            FrameTrigger::Other => "other",
            FrameTrigger::Initial => "initial",
            FrameTrigger::Resize => "resize",
            FrameTrigger::Pointer => "pointer",
            FrameTrigger::Keyboard => "keyboard",
            FrameTrigger::Animation => "animation",
            FrameTrigger::ShaderPaint => "shader-paint",
            FrameTrigger::Periodic => "periodic",
        }
    }
}

/// Per-frame diagnostic snapshot the host hands the app via
/// [`BuildCx::diagnostics`]. Apps that surface a debug overlay (e.g.
/// the showcase status block) read this each build to display the
/// active backend, frame cadence, and what triggered the redraw.
/// Timing fields describe the last completed rendered frame, not the
/// frame currently being built; the host cannot know current layout /
/// paint timings until after `App::build` returns.
///
/// Hosts populate every field they can; `backend` is a static string
/// (`"WebGPU"`, `"Vulkan"`, `"Metal"`, `"DX12"`, `"GL"`) so the app
/// doesn't need to depend on `wgpu` to read it. Time fields use
/// `std::time::Duration`, which works on both native and wasm32 — only
/// `Instant::now()` is the wasm-incompatible piece, and that stays on
/// the host side.
#[derive(Clone, Debug)]
pub struct HostDiagnostics {
    /// Render backend in human-readable form.
    pub backend: &'static str,
    /// Current surface size in physical pixels.
    pub surface_size: (u32, u32),
    /// Display scale factor (`physical / logical`).
    pub scale_factor: f32,
    /// Active MSAA sample count (1 = MSAA off).
    pub msaa_samples: u32,
    /// Frame counter; increments every redraw the host actually
    /// renders. Useful for verifying that an animated source is
    /// progressing.
    pub frame_index: u64,
    /// Wall-clock time between this redraw and the previous one.
    /// `Duration::ZERO` for the first frame (no prior frame).
    pub last_frame_dt: std::time::Duration,
    /// Time spent in the app's `build` method for the last completed
    /// frame. `Duration::ZERO` before the first full frame and on
    /// paint-only frames that skipped build.
    pub last_build: std::time::Duration,
    /// Total time spent in the backend `prepare` call for the last
    /// completed frame.
    pub last_prepare: std::time::Duration,
    /// Sub-stage inside `prepare`: layout pass, focus/selection sync,
    /// state application, and animation tick.
    pub last_layout: std::time::Duration,
    /// Intrinsic-measurement cache hits during the last layout pass.
    pub last_layout_intrinsic_cache_hits: u64,
    /// Intrinsic-measurement cache misses during the last layout pass.
    pub last_layout_intrinsic_cache_misses: u64,
    /// Direct scroll children whose descendants were skipped during
    /// layout because the child was outside the scroll viewport.
    pub last_layout_pruned_subtrees: u64,
    /// Descendant nodes assigned zero rects as part of scroll layout
    /// pruning during the last layout pass.
    pub last_layout_pruned_nodes: u64,
    /// Sub-stage inside `prepare`: laid-out tree to backend-neutral
    /// `DrawOp` list.
    pub last_draw_ops: std::time::Duration,
    /// Text draw ops skipped during draw-op generation because their
    /// glyph rect did not intersect the inherited clip.
    pub last_draw_ops_culled_text_ops: u64,
    /// Sub-stage inside `prepare`: paint-stream packing and text
    /// shaping/rasterization recording.
    pub last_paint: std::time::Duration,
    /// Paint ops skipped because their painted rect did not intersect
    /// the effective clip/viewport in the last completed frame.
    pub last_paint_culled_ops: u64,
    /// Sub-stage inside `prepare`: backend-side buffer writes, glyph
    /// atlas uploads, and frame uniforms.
    pub last_gpu_upload: std::time::Duration,
    /// Sub-stage inside `prepare`: clone the laid-out tree for
    /// next-frame hit-testing.
    pub last_snapshot: std::time::Duration,
    /// Time spent encoding/submitting/presenting the last completed
    /// frame after `prepare`.
    pub last_submit: std::time::Duration,
    /// Layout-side text-cache hits during the last completed full
    /// prepare.
    pub last_text_layout_cache_hits: u64,
    /// Layout-side text-cache misses during the last completed full
    /// prepare.
    pub last_text_layout_cache_misses: u64,
    /// Estimated layout-side text-cache evictions during the last
    /// completed full prepare.
    pub last_text_layout_cache_evictions: u64,
    /// Total UTF-8 bytes shaped on layout-cache misses during the last
    /// completed full prepare.
    pub last_text_layout_shaped_bytes: u64,
    /// Why the host triggered this frame.
    pub trigger: FrameTrigger,
}

impl Default for HostDiagnostics {
    fn default() -> Self {
        Self {
            backend: "?",
            surface_size: (0, 0),
            scale_factor: 1.0,
            msaa_samples: 1,
            frame_index: 0,
            last_frame_dt: std::time::Duration::ZERO,
            last_build: std::time::Duration::ZERO,
            last_prepare: std::time::Duration::ZERO,
            last_layout: std::time::Duration::ZERO,
            last_layout_intrinsic_cache_hits: 0,
            last_layout_intrinsic_cache_misses: 0,
            last_layout_pruned_subtrees: 0,
            last_layout_pruned_nodes: 0,
            last_draw_ops: std::time::Duration::ZERO,
            last_draw_ops_culled_text_ops: 0,
            last_paint: std::time::Duration::ZERO,
            last_paint_culled_ops: 0,
            last_gpu_upload: std::time::Duration::ZERO,
            last_snapshot: std::time::Duration::ZERO,
            last_submit: std::time::Duration::ZERO,
            last_text_layout_cache_hits: 0,
            last_text_layout_cache_misses: 0,
            last_text_layout_cache_evictions: 0,
            last_text_layout_shaped_bytes: 0,
            trigger: FrameTrigger::default(),
        }
    }
}

impl<'a> BuildCx<'a> {
    /// Construct a [`BuildCx`] borrowing the supplied theme. Hosts call
    /// this once per frame after [`App::theme`] and before [`App::build`].
    /// Hosts that own a [`crate::state::UiState`] should chain
    /// [`Self::with_ui_state`] so the app can read interaction state
    /// (hover) during build via [`Self::hovered_key`] /
    /// [`Self::is_hovering_within`].
    pub fn new(theme: &'a crate::Theme) -> Self {
        Self {
            theme,
            ui_state: None,
            diagnostics: None,
            viewport: None,
            safe_area: None,
        }
    }

    /// Attach the runtime's [`crate::state::UiState`] so build-time
    /// accessors (`hovered_key`, `is_hovering_within`) can answer.
    /// When omitted, those accessors return `None` / `false` — useful
    /// for headless rendering paths that don't track interaction
    /// state.
    pub fn with_ui_state(mut self, ui_state: &'a crate::state::UiState) -> Self {
        self.ui_state = Some(ui_state);
        self
    }

    /// Attach a [`HostDiagnostics`] snapshot for this frame. Hosts call
    /// this when they want apps to surface debug overlays (e.g. the
    /// showcase status block); apps that don't read `diagnostics()`
    /// pay nothing for it. Headless render paths leave it `None`.
    pub fn with_diagnostics(mut self, diagnostics: &'a HostDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Attach the logical-pixel viewport size for this frame. Hosts
    /// chain this so apps can branch on viewport metrics during build
    /// (responsive layout, phone-vs-desktop splits) without threading
    /// surface size through their own state. Headless render paths
    /// without a meaningful viewport leave it unset.
    pub fn with_viewport(mut self, width: f32, height: f32) -> Self {
        self.viewport = Some((width, height));
        self
    }

    /// Attach the host's reported safe-area insets in logical pixels.
    /// Hosts chain this when platform chrome (on-screen keyboard,
    /// notch, status bar, home indicator) is obscuring some band of
    /// the viewport. Apps read it via [`Self::safe_area`] /
    /// [`Self::safe_area_bottom`] and inset their interactive content
    /// accordingly. Hosts that don't report safe-area metrics omit
    /// this; apps see `Sides::zero()` from the read accessors.
    pub fn with_safe_area(mut self, sides: crate::tree::Sides) -> Self {
        self.safe_area = Some(sides);
        self
    }

    /// Per-frame diagnostic snapshot from the host (backend, frame
    /// cadence, trigger reason, etc.), or `None` when the host did
    /// not attach one. Apps display this in optional debug overlays.
    pub fn diagnostics(&self) -> Option<&HostDiagnostics> {
        self.diagnostics
    }

    /// The active runtime theme for this frame.
    pub fn theme(&self) -> &crate::Theme {
        self.theme
    }

    /// Shorthand for `self.theme().palette()`.
    pub fn palette(&self) -> &crate::Palette {
        self.theme.palette()
    }

    /// Logical-pixel viewport `(width, height)` the host attached for
    /// this frame, or `None` for headless render paths. Apps use this
    /// to branch layout on viewport metrics — see [`Self::viewport_below`]
    /// for the common phone-vs-desktop breakpoint case.
    pub fn viewport(&self) -> Option<(f32, f32)> {
        self.viewport
    }

    /// Logical-pixel viewport width the host attached for this frame,
    /// or `None` when no viewport is available. Convenience for the
    /// common single-axis branch (`cx.viewport_width().map_or(false,
    /// |w| w < 600.0)`).
    pub fn viewport_width(&self) -> Option<f32> {
        self.viewport.map(|(w, _)| w)
    }

    /// Logical-pixel viewport height the host attached for this frame,
    /// or `None` when no viewport is available.
    pub fn viewport_height(&self) -> Option<f32> {
        self.viewport.map(|(_, h)| h)
    }

    /// True iff the attached viewport's width is strictly less than
    /// `threshold` logical pixels. Returns `false` when no viewport is
    /// attached so headless / desktop-default paths fall through to
    /// the wider branch — apps that want the opposite default can
    /// match on [`Self::viewport_width`] directly.
    ///
    /// Use for the common breakpoint split:
    /// ```ignore
    /// if cx.viewport_below(600.0) {
    ///     phone_layout()
    /// } else {
    ///     desktop_layout()
    /// }
    /// ```
    pub fn viewport_below(&self, threshold: f32) -> bool {
        self.viewport_width().is_some_and(|w| w < threshold)
    }

    /// Logical-pixel safe-area insets the host reports for this frame
    /// (`Sides::zero()` when nothing was attached). Today this is
    /// populated only by aetna-web when the on-screen keyboard
    /// shrinks the visual viewport — `bottom` carries the keyboard
    /// height; future native mobile hosts will additionally populate
    /// `top` for status-bar / notch and `bottom` for home-indicator.
    ///
    /// Apps inset their root layout (or just the focused-input
    /// region) by these amounts so interactive content doesn't sit
    /// underneath platform chrome. The runtime does not auto-apply
    /// this — apps decide where the inset matters.
    pub fn safe_area(&self) -> crate::tree::Sides {
        self.safe_area.unwrap_or_default()
    }

    /// Convenience: just the bottom inset, in logical pixels. Most
    /// commonly the soft-keyboard height.
    pub fn safe_area_bottom(&self) -> f32 {
        self.safe_area().bottom
    }

    /// Key of the leaf node currently under the pointer, or `None`
    /// when nothing is hovered or this `BuildCx` was built without a
    /// `UiState` (headless rendering paths).
    ///
    /// Use for branching the build output on hover state without
    /// mirroring it via `App::on_event` handlers — e.g., a sidebar
    /// row that previews details in a side pane based on what's
    /// currently hovered.
    ///
    /// For region-aware queries (parent stays "hot" while a child is
    /// hovered), prefer [`Self::is_hovering_within`].
    pub fn hovered_key(&self) -> Option<&str> {
        self.ui_state?.hovered_key()
    }

    /// True iff `key`'s node — or any descendant of it — is the
    /// current hover target. Subtree-aware, matching the semantics of
    /// [`crate::tree::El::hover_alpha`]. Returns `false` when this
    /// `BuildCx` has no attached `UiState` or when `key` isn't in the
    /// current tree.
    ///
    /// Reads the underlying tracker, not the eased subtree envelope —
    /// the boolean flips immediately on hit-test identity change.
    pub fn is_hovering_within(&self, key: &str) -> bool {
        self.ui_state
            .is_some_and(|state| state.is_hovering_within(key))
    }
}

/// The application contract. Implement this on your state struct and
/// pass it to a host runner (e.g., `aetna_winit_wgpu::run`).
pub trait App {
    /// Refresh app-owned external state immediately before a frame is
    /// built.
    ///
    /// Hosts call this once per redraw before [`Self::build`]. Use it
    /// for polling an external source, reconciling optimistic local
    /// state with a backend snapshot, or advancing host-owned live data
    /// that should be visible in the next tree. Keep expensive work
    /// outside the render loop; this hook is still on the frame path.
    ///
    /// Default: no-op.
    fn before_build(&mut self) {}

    /// Project current state into a scene tree. Called whenever the
    /// host requests a redraw, after [`Self::before_build`]. Prefer to
    /// keep this pure: read current state and return a fresh tree.
    ///
    /// `cx` carries per-frame, read-only context (active theme, future
    /// viewport / phase metadata). Apps that don't need to branch on
    /// the theme during construction can ignore the parameter — token
    /// references in widget code resolve through the palette
    /// automatically.
    fn build(&self, cx: &BuildCx) -> El;

    /// Update state in response to a routed event. Default: no-op.
    fn on_event(&mut self, _event: UiEvent) {}

    /// Update state in response to routed wheel input.
    ///
    /// Return `true` to consume the wheel and suppress Aetna's default
    /// scroll routing. The default forwards to [`Self::on_event`] and
    /// returns `false`, so existing apps can observe wheel events
    /// without opting out of normal scrolling.
    fn on_wheel_event(&mut self, event: UiEvent) -> bool {
        self.on_event(event);
        false
    }

    /// The application's current text [`crate::selection::Selection`].
    /// Read by the host once per frame so the library can paint
    /// highlight bands and resolve `selected_text` for clipboard.
    /// Apps that own a `Selection` field return a clone here; the
    /// default returns the empty selection.
    fn selection(&self) -> crate::selection::Selection {
        crate::selection::Selection::default()
    }

    /// App-level hotkey registry. The library matches incoming key
    /// presses against this list before its own focus-activation
    /// routing; a match emits a [`UiEvent`] with `kind =
    /// UiEventKind::Hotkey` and `key = Some(name)`.
    ///
    /// Called once per build cycle; the host runner snapshots the list
    /// alongside `build()` so the chords stay in sync with state.
    /// Default: no hotkeys.
    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        Vec::new()
    }

    /// Drain pending toast notifications produced since the last
    /// frame. The runtime calls this once per `prepare_layout`,
    /// stamps each spec with a monotonic id and `expires_at = now +
    /// ttl`, queues it in the runtime toast state, and
    /// synthesizes a `toast_stack` layer at the El root so the
    /// rendered tree mirrors the visible state. Apps typically
    /// accumulate specs in a `Vec<ToastSpec>` field from event
    /// handlers, then `mem::take` it here.
    ///
    /// **Root requirement:** apps that produce toasts (or use
    /// `.tooltip(text)` on any node) must wrap their
    /// [`Self::build`] return value in `overlays(main, [])` so the
    /// runtime can append the floating layer as an overlay sibling
    /// — same convention used for popovers and modals. Debug
    /// builds panic if the synthesizer runs against a non-overlay
    /// root.
    ///
    /// Default: no toasts.
    fn drain_toasts(&mut self) -> Vec<crate::toast::ToastSpec> {
        Vec::new()
    }

    /// Drain pending programmatic focus requests produced since the
    /// last frame. The runtime calls this once per `prepare_layout`,
    /// after the focus order has been rebuilt from the new tree, and
    /// resolves each entry against the keyed focusables. Unmatched
    /// keys (widget absent from the rebuilt tree, or not focusable)
    /// are dropped silently.
    ///
    /// This is the imperative companion to keyboard `Tab` traversal:
    /// use it for affordances like *Ctrl+F → focus the search input*,
    /// *jump-to-match → focus the matched row*, or *open inline edit
    /// → focus the field*. Apps typically accumulate keys in a
    /// `Vec<String>` field from event handlers and `mem::take` it
    /// here.
    ///
    /// Multiple requests in one frame resolve in order; the last
    /// successfully-resolved key is the one focused.
    ///
    /// Default: no requests.
    fn drain_focus_requests(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// Drain pending programmatic scroll requests. The runtime
    /// resolves each request during layout, using live viewport rects
    /// and row-height/content geometry that apps should not duplicate.
    /// Unmatched keys and out-of-range row indices drop silently.
    ///
    /// Use [`crate::scroll::ScrollRequest::ToRow`] for virtual-list
    /// affordances such as jump-to-search-result, reveal selected row,
    /// or scroll-to-top-on-tab-change. Use
    /// [`crate::scroll::ScrollRequest::EnsureVisible`] for widgets
    /// with an internal scroll viewport, including fixed-height
    /// [`crate::widgets::text_area`] caret-into-view after accepted
    /// edit/navigation events. Apps typically accumulate requests in a
    /// `Vec<ScrollRequest>` field from event handlers and
    /// `mem::take` it here.
    ///
    /// Default: no requests.
    fn drain_scroll_requests(&mut self) -> Vec<crate::scroll::ScrollRequest> {
        Vec::new()
    }

    /// Drain pending URL-open requests produced since the last frame.
    /// Hosts call this once per frame and route each URL to a
    /// platform-appropriate opener — `window.open` in the wasm host,
    /// the `open` crate (or equivalent) on native.
    ///
    /// The library emits [`UiEventKind::LinkActivated`] when a click
    /// lands on a text run carrying a link URL, but it does not act
    /// on the URL itself: opening a link is an app concern (apps may
    /// want to confirm, filter by scheme, route through an internal
    /// router, or no-op entirely). Apps that want the default
    /// browser-style behavior accumulate URLs from
    /// [`UiEventKind::LinkActivated`] in their `on_event` handler and
    /// return them here; apps that don't override this method drop
    /// link clicks on the floor.
    ///
    /// Default: no requests.
    fn drain_link_opens(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// Custom shaders this app needs registered. Each entry carries
    /// the shader name, its WGSL source, and per-flag opt-ins
    /// (backdrop sampling, time-driven motion). The host runner
    /// registers them once at startup via
    /// `Runner::register_shader_with(name, wgsl, samples_backdrop, samples_time)`.
    ///
    /// Backends that don't support backdrop sampling skip entries with
    /// `samples_backdrop=true`; any node bound to such a shader will
    /// draw nothing on those backends rather than mis-render.
    /// `samples_time=true` declares that the shader's output depends
    /// on `frame.time`, which keeps the host idle loop ticking while
    /// any node is bound to it.
    ///
    /// Default: no shaders.
    fn shaders(&self) -> Vec<AppShader> {
        Vec::new()
    }

    /// Runtime paint theme for this app. Hosts apply it to the renderer
    /// before preparing each frame so stateful apps can switch global
    /// material routing without backend-specific calls.
    fn theme(&self) -> crate::Theme {
        crate::Theme::default()
    }
}

/// One custom shader registration, returned from [`App::shaders`].
#[derive(Clone, Copy, Debug)]
pub struct AppShader {
    pub name: &'static str,
    pub wgsl: &'static str,
    /// Reads the prior pass's color target (`@group(2) backdrop_tex`).
    /// Backends without backdrop support skip these.
    pub samples_backdrop: bool,
    /// Reads `frame.time` and so requires continuous redraw whenever
    /// any node is bound to it. The runtime ORs this into
    /// `PrepareResult::needs_redraw` per frame.
    pub samples_time: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;

    #[test]
    fn viewport_unset_returns_none_and_breakpoint_returns_false() {
        let theme = Theme::default();
        let cx = BuildCx::new(&theme);
        assert!(cx.viewport().is_none());
        assert!(cx.viewport_width().is_none());
        assert!(!cx.viewport_below(600.0));
    }

    #[test]
    fn viewport_set_exposes_width_and_height() {
        let theme = Theme::default();
        let cx = BuildCx::new(&theme).with_viewport(420.0, 800.0);
        assert_eq!(cx.viewport(), Some((420.0, 800.0)));
        assert_eq!(cx.viewport_width(), Some(420.0));
        assert_eq!(cx.viewport_height(), Some(800.0));
    }

    #[test]
    fn viewport_below_uses_strict_less_than() {
        let theme = Theme::default();
        let cx = BuildCx::new(&theme).with_viewport(600.0, 800.0);
        assert!(!cx.viewport_below(600.0), "boundary is exclusive");
        assert!(cx.viewport_below(601.0));
        assert!(!cx.viewport_below(599.0));
    }
}
