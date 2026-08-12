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
//! use damascene_core::prelude::*;
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
//!     fn on_event(&mut self, e: UiEvent, _cx: &EventCx) {
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

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

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
    /// The [`El::key`][method@El::key] route string of the hit node —
    /// what app code matches events against.
    pub key: String,
    /// Stable laid-out tree path of the node, used by artifacts and
    /// tooling that must survive key renames. Shares the tree's
    /// interned id (`Arc<str>`) — targets are rebuilt per frame for
    /// the focus/selection orders, so this must not allocate.
    pub node_id: std::sync::Arc<str>,
    /// The node's laid-out rect in logical pixels, from the layout
    /// pass this target was hit-tested against.
    pub rect: Rect,
    /// Tooltip text snapshotted from the node when the target was
    /// constructed (see the struct docs for why it's cached).
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

impl PointerButton {
    /// Translate a Linux evdev button code (the `button` field of
    /// `wl_pointer.button`, `<linux/input-event-codes.h>`'s `BTN_*`)
    /// to a damascene button: `BTN_LEFT` (0x110) → `Primary`,
    /// `BTN_RIGHT` (0x111) → `Secondary`, `BTN_MIDDLE` (0x112) →
    /// `Middle`. Side/extra/task buttons return `None` — not surfaced
    /// today.
    ///
    /// For raw Wayland hosts (layer-shell bars, notification daemons)
    /// that read pointer buttons off the wire without winit in the
    /// loop.
    pub const fn from_linux_button(code: u32) -> Option<Self> {
        match code {
            0x110 => Some(Self::Primary),
            0x111 => Some(Self::Secondary),
            0x112 => Some(Self::Middle),
            _ => None,
        }
    }
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

/// The **logical** key — the key's current meaning, layout- and
/// modifier-dependent, mirroring the W3C UI Events
/// [`KeyboardEvent.key`](https://www.w3.org/TR/uievents-key/) attribute.
/// This is the right facet for activation, navigation, and accelerators
/// that should follow the printed legend (`Ctrl+S` wherever `S` is).
///
/// Committed text (IME / dead-key composition) is **not** carried here —
/// it arrives as a separate [`UiEventKind::TextInput`] event with the
/// string on [`UiEvent::text`]. So this enum stays the *meaning* of the
/// key, never the produced text. For layout-independent identity (games,
/// rebindable hotkeys, numpad disambiguation) use [`KeyPress::physical`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogicalKey {
    /// A named (non-character-producing) key — `Enter`, `ArrowUp`, `F5`.
    Named(NamedKey),
    /// A character-producing key, carrying the logical character(s) it
    /// stands for (e.g. `"a"`, `"A"`, `"ä"`). Hotkey matching compares
    /// ASCII case-insensitively, so `Character("f")` and `Character("F")`
    /// match the same chord. This is the key's *meaning*, not the text it
    /// commits — typed text flows through [`UiEventKind::TextInput`].
    Character(String),
    /// The host could not identify the key (dead keys mid-composition,
    /// platform keys with no W3C name). Replaces the old `Debug`-string
    /// fallback — never a stringly host-formatted value.
    Unidentified,
}

impl LogicalKey {
    /// The logical character(s) this key stands for, if it is a
    /// character-producing key. `None` for named and unidentified keys.
    pub fn character(&self) -> Option<&str> {
        match self {
            LogicalKey::Character(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The named key this is, if it is a named key. `None` for character
    /// and unidentified keys.
    pub fn named(&self) -> Option<NamedKey> {
        match self {
            LogicalKey::Named(n) => Some(*n),
            _ => None,
        }
    }
}

/// Defines a vocabulary enum together with an `ALL` const listing every
/// variant in declaration order. The input vocabularies (`NamedKey`,
/// `PhysicalKey`, [`crate::Cursor`]) are `#[non_exhaustive]`, so an
/// out-of-tree host mapper cannot use exhaustive `match` to prove it
/// covers the full set; `ALL` is the enumeration hook its totality test
/// iterates instead. Generating enum and const from one listing keeps
/// the two from drifting when the vocabulary grows.
macro_rules! enum_with_all {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        pub enum $name {
            $( $(#[$vmeta])* $variant ),+
        }

        impl $name {
            /// Every variant, in declaration order.
            ///
            /// The enum is `#[non_exhaustive]`, so a host input mapper
            /// cannot prove coverage with an exhaustive `match`; its
            /// totality test iterates this slice instead (see
            /// `damascene-winit`'s tests for the pattern).
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];
        }
    };
}
pub(crate) use enum_with_all;

enum_with_all! {
/// A named (non-character-producing) key value — the named subset of the
/// W3C UI Events [`key`](https://www.w3.org/TR/uievents-key/#named-key-attribute-values)
/// vocabulary (`Enter`, `ArrowUp`, `F5`, `Shift`, …). Hosts map their
/// platform's named keys onto this; anything outside the set surfaces as
/// [`LogicalKey::Unidentified`] rather than a host-formatted string, so
/// the contract never leans on a windowing crate's `Debug` output.
///
/// `#[non_exhaustive]`: the W3C set is large and grows; match with a
/// wildcard arm, and test mapper coverage against [`NamedKey::ALL`].
/// Per-variant docs are omitted — the names are the W3C spec names and
/// self-describing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum NamedKey {
    // Modifier keys.
    Alt,
    AltGraph,
    CapsLock,
    Control,
    Fn,
    FnLock,
    Meta,
    NumLock,
    ScrollLock,
    Shift,
    Super,
    Hyper,
    Symbol,
    // Whitespace / editing / navigation.
    Enter,
    Tab,
    Space,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    End,
    Home,
    PageDown,
    PageUp,
    Backspace,
    Clear,
    Copy,
    CrSel,
    Cut,
    Delete,
    EraseEof,
    ExSel,
    Insert,
    Paste,
    Redo,
    Undo,
    // General-purpose / UI.
    Accept,
    Again,
    Cancel,
    ContextMenu,
    Escape,
    Execute,
    Find,
    Help,
    Pause,
    Play,
    Props,
    Select,
    ZoomIn,
    ZoomOut,
    // Device.
    Eject,
    Power,
    PrintScreen,
    WakeUp,
    // Common media keys.
    AudioVolumeDown,
    AudioVolumeMute,
    AudioVolumeUp,
    MediaPlayPause,
    MediaStop,
    MediaTrackNext,
    MediaTrackPrevious,
    // Function keys.
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
}
}

enum_with_all! {
/// The **physical** key — the layout-independent position on the board,
/// mirroring the W3C UI Events
/// [`KeyboardEvent.code`](https://www.w3.org/TR/uievents-code/) set
/// (`KeyA`, `Numpad1`, `ShiftRight`, `F14`). Unlike [`LogicalKey`] this
/// does not change with keyboard layout or held modifiers: the key west
/// of `KeyS` is `KeyA` on QWERTY, AZERTY, and Dvorak alike.
///
/// This is the right facet for rebindable controls, WASD-style movement,
/// global hotkeys, and telling duplicate keys apart (numpad `Enter` vs
/// the main `Enter`, `ShiftLeft` vs `ShiftRight`) — none of which the
/// logical key or the modifier mask can distinguish.
///
/// Hosts that cannot report a position use [`PhysicalKey::Unidentified`].
/// Names follow the W3C `code` spelling (e.g. `MetaLeft`/`MetaRight`, not
/// winit's `SuperLeft`/`SuperRight`).
///
/// `#[non_exhaustive]`: match with a wildcard arm, and test mapper
/// coverage against [`PhysicalKey::ALL`]. Per-variant docs are omitted —
/// the names are the W3C `code` spec names and self-describing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum PhysicalKey {
    // Writing-system keys.
    Backquote,
    Backslash,
    BracketLeft,
    BracketRight,
    Comma,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Equal,
    IntlBackslash,
    IntlRo,
    IntlYen,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Minus,
    Period,
    Quote,
    Semicolon,
    Slash,
    // Functional keys.
    AltLeft,
    AltRight,
    Backspace,
    CapsLock,
    ContextMenu,
    ControlLeft,
    ControlRight,
    Enter,
    MetaLeft,
    MetaRight,
    ShiftLeft,
    ShiftRight,
    Space,
    Tab,
    // Control pad / arrows.
    Delete,
    End,
    Help,
    Home,
    Insert,
    PageDown,
    PageUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    // Numpad.
    NumLock,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadBackspace,
    NumpadClear,
    NumpadComma,
    NumpadDecimal,
    NumpadDivide,
    NumpadEnter,
    NumpadEqual,
    NumpadMultiply,
    NumpadParenLeft,
    NumpadParenRight,
    NumpadSubtract,
    // System / function.
    Escape,
    PrintScreen,
    ScrollLock,
    Pause,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    /// The host could not report a physical position for this key.
    Unidentified,
}
}

/// OS modifier-key mask. The four fields mirror the platform-standard
/// modifier set; this struct is intentionally **not** `#[non_exhaustive]`
/// so callers can use struct-literal syntax with `..Default::default()`
/// to spell precise modifier combinations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    /// Shift key held.
    pub shift: bool,
    /// Control key held.
    pub ctrl: bool,
    /// Alt / Option key held.
    pub alt: bool,
    /// Logo key held — Super / Windows key / Command.
    pub logo: bool,
}

/// One keyboard key-down, as delivered on [`UiEvent::key_press`].
/// Hosts feed the constituent parts through
/// [`crate::runtime::RunnerCore::key_down`]; the runtime packages them
/// into this struct on the events it emits.
///
/// Two of the three W3C key facets live here: [`logical`](Self::logical)
/// (the key's meaning) and [`physical`](Self::physical) (its
/// layout-independent position). The third — committed text — is a
/// separate [`UiEventKind::TextInput`] event ([`UiEvent::text`]), so this
/// struct never carries produced text.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyPress {
    /// The logical key — meaning under the active layout + modifiers. Use
    /// for activation, navigation, and legend-following accelerators.
    pub logical: LogicalKey,
    /// The physical key — layout-independent board position. Use for
    /// rebindable controls, games, global hotkeys, and disambiguating
    /// duplicate keys (numpad vs main row). [`PhysicalKey::Unidentified`]
    /// when the host can't report a position.
    pub physical: PhysicalKey,
    /// Modifier mask at the moment of the press.
    pub modifiers: KeyModifiers,
    /// True when this press is an OS auto-repeat of a held key rather
    /// than a fresh key-down.
    pub repeat: bool,
}

impl KeyPress {
    /// Construct a key press from its facets. Hosts call this to feed
    /// [`crate::runtime::RunnerCore::key_down`]. `KeyPress` is
    /// `#[non_exhaustive]`, so this constructor is the supported way to
    /// build one outside the core crate.
    pub fn new(
        logical: LogicalKey,
        physical: PhysicalKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Self {
        Self {
            logical,
            physical,
            modifiers,
            repeat,
        }
    }
}

/// Which facet of a key press a [`KeyChord`] matches against.
///
/// Pick the facet by intent: [`Logical`](Self::Logical) for shortcuts
/// that should follow the printed legend (`Ctrl+S` stays on whichever key
/// prints `S`); [`Physical`](Self::Physical) for layout-independent
/// bindings (the WASD cluster stays WASD on AZERTY, where the legends read
/// ZQSD).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChordTrigger {
    /// Match the logical key — layout- and modifier-dependent.
    Logical(LogicalKey),
    /// Match the physical key position — layout-independent.
    Physical(PhysicalKey),
}

/// A keyboard chord for app-level hotkey registration. Matches one key
/// facet with an exact modifier mask: `KeyChord::ctrl('f')` does not also
/// match `Ctrl+Shift+F`, and `KeyChord::vim('j')` does not match if any
/// modifier is held.
///
/// A chord matches either the [`logical`](ChordTrigger::Logical) key
/// (follow the legend) or the [`physical`](ChordTrigger::Physical) key
/// (layout-independent) — see [`ChordTrigger`]. The `vim`/`ctrl`/
/// `ctrl_shift`/`named` constructors build logical chords; [`physical`]
/// builds a physical one.
///
/// Register chords from [`App::hotkeys`]; the library matches them
/// against incoming key presses ahead of focus activation routing and
/// emits a [`UiEvent`] with `kind = UiEventKind::Hotkey` and `key`
/// equal to the registered name.
///
/// [`physical`]: Self::physical
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyChord {
    /// The key facet (logical or physical) the chord matches.
    pub trigger: ChordTrigger,
    /// Exact modifier mask that must be held — extra modifiers do not
    /// match (see [`Self::matches`]).
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    /// A bare logical key with no modifiers (vim-style).
    /// `KeyChord::vim('j')` matches the logical `j` with no
    /// Ctrl/Shift/Alt/Logo held.
    pub fn vim(c: char) -> Self {
        Self {
            trigger: ChordTrigger::Logical(LogicalKey::Character(c.to_string())),
            modifiers: KeyModifiers::default(),
        }
    }

    /// `Ctrl+<char>`, matched on the logical key.
    pub fn ctrl(c: char) -> Self {
        Self {
            trigger: ChordTrigger::Logical(LogicalKey::Character(c.to_string())),
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
        }
    }

    /// `Ctrl+Shift+<char>`, matched on the logical key.
    pub fn ctrl_shift(c: char) -> Self {
        Self {
            trigger: ChordTrigger::Logical(LogicalKey::Character(c.to_string())),
            modifiers: KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        }
    }

    /// A logical key with no modifiers (e.g.
    /// `KeyChord::named(LogicalKey::Named(NamedKey::Escape))`).
    pub fn named(key: LogicalKey) -> Self {
        Self {
            trigger: ChordTrigger::Logical(key),
            modifiers: KeyModifiers::default(),
        }
    }

    /// A physical key position with no modifiers — layout-independent
    /// (e.g. `KeyChord::physical(PhysicalKey::KeyW)` binds the WASD `W`
    /// position regardless of layout).
    pub fn physical(key: PhysicalKey) -> Self {
        Self {
            trigger: ChordTrigger::Physical(key),
            modifiers: KeyModifiers::default(),
        }
    }

    /// Builder-style: replace the chord's modifier mask.
    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Strict match: the chord's facet equals the press's matching facet
    /// AND the modifier mask is identical. Holding extra modifiers does
    /// not match a chord that didn't request them. Logical-character
    /// chords compare ASCII case-insensitively.
    pub fn matches(
        &self,
        logical: &LogicalKey,
        physical: PhysicalKey,
        modifiers: KeyModifiers,
    ) -> bool {
        self.modifiers == modifiers
            && match &self.trigger {
                ChordTrigger::Logical(want) => logical_eq(want, logical),
                ChordTrigger::Physical(want) => *want == physical,
            }
    }
}

fn logical_eq(a: &LogicalKey, b: &LogicalKey) -> bool {
    match (a, b) {
        (LogicalKey::Character(x), LogicalKey::Character(y)) => x.eq_ignore_ascii_case(y),
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
/// # use damascene_core::prelude::*;
/// # struct Counter { value: i32 }
/// # impl App for Counter {
/// # fn build(&self, _cx: &BuildCx) -> El { button("+").key("inc") }
/// fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
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
    /// used by Damascene's scroll containers. Hosts normalize line-based
    /// and pixel-based wheel input before setting this field.
    pub wheel_delta: Option<(f32, f32)>,
    /// What kind of event happened. See [`UiEventKind`] for the
    /// per-variant routing contracts.
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

    /// If this event's route is `prefix:rest`, the rest — see
    /// [`crate::key::suffix`]. The per-item dispatch shape without the
    /// hand-rolled `strip_prefix` chain:
    ///
    /// ```
    /// # use damascene_core::UiEvent;
    /// let event = UiEvent::synthetic_click("thumb:42");
    /// assert_eq!(event.route_suffix("thumb"), Some("42"));
    /// assert_eq!(event.route_suffix("row"), None);
    /// ```
    pub fn route_suffix(&self, prefix: &str) -> Option<&str> {
        crate::key::suffix(self.route()?, prefix)
    }

    /// If this event's route is `prefix:index…`, the first segment
    /// after the prefix parsed as `T` — see [`crate::key::index`].
    /// Unlike the hand-rolled `strip_prefix` + `parse().ok()` chain, a
    /// prefix match whose index fails to parse logs a warning instead
    /// of silently dropping the event.
    ///
    /// ```
    /// # use damascene_core::UiEvent;
    /// let event = UiEvent::synthetic_click("thumb:42");
    /// assert_eq!(event.route_index::<usize>("thumb"), Some(42));
    /// // Select-style routed keys: the leading id still parses.
    /// let event = UiEvent::synthetic_click("profile:7:option:3");
    /// assert_eq!(event.route_index::<u32>("profile"), Some(7));
    /// ```
    pub fn route_index<T: std::str::FromStr>(&self, prefix: &str) -> Option<T> {
        crate::key::index(self.route()?, prefix)
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
/// This enum is non-exhaustive so Damascene can add new input events
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
    /// host-side opener). Damascene doesn't open URLs itself — it surfaces
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
    /// under the pointer. Emitted before Damascene's default scroll
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
    /// A [`user_resizable`](crate::tree::El::user_resizable) pane's
    /// edge drag was released. Routed to the pane's key (unkeyed panes
    /// resize fine but emit nothing); fires once per completed drag,
    /// not per move. Read the final size via
    /// [`EventCx::user_size`]/[`crate::UiState::user_size`] — this is
    /// the natural moment to persist it. The size mutation itself is
    /// runtime-owned, like scrolling: apps only listen when they want
    /// to save the value.
    Resized,
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
/// can't attribute. Specific variants narrow the reason when the
/// host can.
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
    /// Application code asked for a frame through the host's external
    /// wakeup handle (push-driven event-class data — a chat message
    /// arrived, a background task advanced state). Data changed
    /// outside the tree, so this drives the layout path (full rebuild
    /// + prepare), never paint-only.
    External,
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
            FrameTrigger::External => "external",
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
    /// Swapchain reconfigures since bring-up: every
    /// `surface.configure` after the initial one — window resizes,
    /// HDR/SDR format flips, and acquire-failure recoveries
    /// (`Lost`/`Outdated`). Each reconfigure allocates a fresh set of
    /// swapchain buffers, so a value that climbs while window
    /// geometry is stable means the display server is repeatedly
    /// invalidating the swapchain; correlate with GPU buffer-object
    /// growth when chasing compositor-side buffer retention. Hosts
    /// without swapchain ownership report 0.
    pub surface_reconfigures: u64,
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
    /// What the renderer composites in. The paint stream converts every
    /// [`crate::color::Color`] into this space exactly once at the
    /// upload boundary. Defaults to [`crate::color::ColorSpace::SRGB_LINEAR`].
    pub working_color_space: crate::color::ColorSpace,
    /// Wire-side color-management state the host negotiated with the
    /// display server. [`crate::color::ColorManagementStatus::Unavailable`]
    /// on hosts without a color-management protocol (X11, plain Wayland,
    /// macOS / Windows today). See [`crate::color::ColorPreferences`]
    /// for how apps influence the negotiation.
    pub color_management: crate::color::ColorManagementStatus,
    /// Color-relevant facts about the host's GPU presentation surface —
    /// the wgpu / WSI half of color negotiation (advertised formats,
    /// chosen swapchain format, present/alpha mode, adapter). `None` on
    /// hosts that don't present through a wgpu surface (headless render
    /// bins, the vulkano demo). See [`SurfaceColorInfo`].
    pub surface_color: Option<SurfaceColorInfo>,
}

impl Default for HostDiagnostics {
    fn default() -> Self {
        Self {
            backend: "?",
            surface_size: (0, 0),
            scale_factor: 1.0,
            msaa_samples: 1,
            frame_index: 0,
            surface_reconfigures: 0,
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
            working_color_space: crate::paint::DEFAULT_WORKING_COLOR_SPACE,
            color_management: crate::color::ColorManagementStatus::default(),
            surface_color: None,
        }
    }
}

impl HostDiagnostics {
    /// Is this app actually rendering HDR right now — an extended-range
    /// swapchain on an output with HDR evidence?
    ///
    /// This is the check, encoded once so apps never re-derive it:
    /// the compositor's preferred description indicates an HDR output
    /// ([`CompositorColorTargets::indicates_hdr`]) **and** the
    /// negotiated swapchain format can carry extended-range output
    /// ([`SurfaceFormatInfo::wide`], e.g. `Rgba16Float` scRGB). Do
    /// *not* infer HDR from `ColorManagementStatus::Available {
    /// attached }` — on every current host the WSI owns the surface
    /// tag and `attached` stays `None` even in full HDR operation.
    ///
    /// Live: hosts refresh these diagnostics when the compositor's
    /// preferred description changes (`preferred_changed2` — window
    /// moved to another output, HDR toggled), so this flips with the
    /// window. HDR is opt-in via [`crate::color::ColorPreferences`];
    /// a default `sdr_only` app reports `false` even on an HDR output.
    ///
    /// [`CompositorColorTargets::indicates_hdr`]: crate::color::CompositorColorTargets::indicates_hdr
    pub fn hdr_active(&self) -> bool {
        let crate::color::ColorManagementStatus::Available { targets, .. } = &self.color_management
        else {
            return false;
        };
        targets.indicates_hdr()
            && self.surface_color.as_ref().is_some_and(|s| {
                s.formats
                    .iter()
                    .any(|f| f.wide && f.name == s.chosen_format)
            })
    }
}

/// Color-relevant facts about the host's GPU presentation surface — the
/// wgpu / WSI half of color negotiation. The compositor (via
/// [`crate::color::ColorManagementStatus`]) says what it *accepts*; this
/// says what the *swapchain* can represent. The intersection is what the
/// negotiator can actually pick — e.g. a compositor that ingests linear
/// BT.2020 is moot if the surface offers no float format.
///
/// Strings throughout so `damascene-core` needn't depend on `wgpu`.
#[derive(Clone, Debug, Default)]
pub struct SurfaceColorInfo {
    /// Adapter / device name (e.g. `"Intel Graphics (ADL GT2)"`).
    pub adapter: String,
    /// Driver name + version, when the backend reports it.
    pub driver: String,
    /// Color formats the surface advertised, in wgpu's reported order.
    pub formats: Vec<SurfaceFormatInfo>,
    /// The swapchain format negotiation actually chose.
    pub chosen_format: String,
    /// Present mode in use.
    pub present_mode: String,
    /// Composite alpha mode in use.
    pub alpha_mode: String,
}

/// One surface texture format, classified by how it can carry color
/// output. See [`SurfaceColorInfo`].
#[derive(Clone, Debug)]
pub struct SurfaceFormatInfo {
    /// wgpu format name (e.g. `"Rgba16Float"`).
    pub name: String,
    /// Carries an sRGB EOTF in hardware (`*_unorm_srgb`): the GPU encodes
    /// linear → sRGB on store.
    pub srgb: bool,
    /// Can carry wide-gamut / HDR output: a float format (linear-direct —
    /// the compositor does the output encode) or a ≥10-bit format (a
    /// PQ-encode target). 8-bit unorm formats are SDR-only.
    pub wide: bool,
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

    /// Host-reported user accessibility preferences (the CSS
    /// `prefers-*` family) — see [`crate::a11y`]. All-`None` (unknown)
    /// when no [`crate::state::UiState`] is attached or the host never
    /// pushed any. Read [`Self::reduced_motion`] for the common gate;
    /// use the full struct for theme-shaped decisions
    /// ([`crate::a11y::ColorScheme`], [`crate::a11y::Contrast`]).
    pub fn accessibility(&self) -> crate::a11y::AccessibilityPreferences {
        self.ui_state
            .map(|s| s.accessibility_preferences())
            .unwrap_or_default()
    }

    /// `true` iff the user prefers reduced motion (CSS
    /// `prefers-reduced-motion: reduce`). The runtime already snaps
    /// library-owned movement (see [`crate::a11y`]); apps gate their
    /// own decorative motion — parallax, auto-playing movement,
    /// `enter_slide`/`enter_zoom` on large surfaces — on this.
    /// Unknown reads as `false`, matching the web default.
    pub fn reduced_motion(&self) -> bool {
        self.accessibility().prefers_reduced_motion()
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
    /// populated only by damascene-web when the on-screen keyboard
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

    /// The scatter point currently under the cursor in a `chart3d` scene, if
    /// any — the 3D analogue of [`hovered_key`](Self::hovered_key).
    ///
    /// Scene points aren't `El`s, so they can't emit `PointerEnter`/`Leave`
    /// like 2D widgets; this surfaces the same hover pick that draws the
    /// built-in tooltip chip ([`ScenePointPick`] carries the scene id + mark +
    /// point index). Use it to drive a detail panel / highlight / linked view
    /// on hover — branch the build on `cx.hovered_scene_point()` without an
    /// `on_event` handler. Picked a frame late (fine for hover UI) and honours
    /// the chip's depth-occlusion + behind-camera culling.
    ///
    /// [`ScenePointPick`]: crate::scene::ScenePointPick
    pub fn hovered_scene_point(&self) -> Option<&crate::scene::ScenePointPick> {
        self.ui_state?.hovered_scene_point()
    }

    /// The laid-out rect of the keyed node `key` from the *previous*
    /// frame's layout, or `None` when the key wasn't in that tree (or
    /// this `BuildCx` has no attached `UiState` — headless paths).
    ///
    /// The damascene analogue of the DOM's
    /// `getBoundingClientRect()`: layout geometry is retained between
    /// frames, so build code can read where a keyed thing actually
    /// landed. One frame stale by construction — `build` runs before
    /// this frame's layout — which is fine for the usual uses
    /// (branching on a measured-once size, sizing a dependent pane).
    /// Same staleness contract as [`Self::hovered_key`]. For
    /// event-time decisions prefer [`EventCx::rect_of_key`], which
    /// answers at the moment the handler runs.
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.ui_state?.rect_of_key(key)
    }

    /// The live pose of a keyed scene camera by computed id — see
    /// [`UiState::scene_camera`](crate::state::UiState::scene_camera).
    /// Combined with the node rect and a pointer position, an app builds a
    /// screen→world ray (`view_proj().inverse()`) to pick or place geometry on
    /// a plane. The id is a node's computed id, e.g. a build-time
    /// [`BuildCx::rect_of_key`]-keyed scene's resolved id.
    pub fn scene_camera(&self, id: &str) -> Option<crate::scene::CameraState> {
        self.ui_state?.scene_camera(id)
    }

    /// The half-open row-index range the keyed virtual list realized in
    /// the previous frame's layout, or `None` when the key isn't a
    /// laid-out virtual list (or no `UiState` is attached). One frame
    /// stale by construction, same contract as [`Self::rect_of_key`].
    ///
    /// The cache-eviction hook for media-heavy lists: rows outside this
    /// range are off-screen, so their decoded images/thumbnails can be
    /// dropped and recreated by the row builder when scrolled back.
    pub fn visible_range(&self, key: &str) -> Option<std::ops::Range<usize>> {
        self.ui_state?.visible_range(key)
    }

    /// The current pan/zoom of the [`viewport`](crate::tree::viewport)
    /// keyed `key`, from the last layout — for a zoom-percentage readout
    /// or to project content into screen space. `None` until the keyed
    /// viewport has been laid out.
    pub fn viewport_view(&self, key: &str) -> Option<crate::viewport::ViewportView> {
        self.ui_state?.viewport_view_by_key(key)
    }

    /// The bounding box of the keyed viewport's laid-out content in
    /// **content space** (pre-transform), from the last layout — combine
    /// with [`Self::viewport_view`] to draw a minimap / overview rect.
    /// `None` until the keyed viewport has been laid out with measurable
    /// content. See
    /// [`UiState::viewport_content_bounds_by_key`](crate::state::UiState::viewport_content_bounds_by_key).
    pub fn viewport_content_bounds(&self, key: &str) -> Option<Rect> {
        self.ui_state?.viewport_content_bounds_by_key(key)
    }

    /// Whether the keyed viewport is still at its home framing (the
    /// policy fit / last programmatic fit or reset), or the user has
    /// taken the view over — for chrome that shows "Fit" vs a concrete
    /// zoom percentage, or disables a Reset button at home. `None` when
    /// no laid-out node carries `key`. See
    /// [`UiState::viewport_at_home`](crate::state::UiState::viewport_at_home).
    pub fn viewport_at_home(&self, key: &str) -> Option<bool> {
        self.ui_state?.viewport_at_home_by_key(key)
    }

    /// Whether the keyed viewport is mid-flight on a smooth programmatic
    /// navigation
    /// ([`ViewportBehavior::Smooth`](crate::viewport::ViewportBehavior::Smooth))
    /// — for gating input or chrome during a fly-to. `None` when no
    /// laid-out node carries `key`. See
    /// [`UiState::viewport_in_flight`](crate::state::UiState::viewport_in_flight).
    pub fn viewport_in_flight(&self, key: &str) -> Option<bool> {
        self.ui_state?.viewport_in_flight_by_key(key)
    }

    /// The user-dragged size of a keyed
    /// [`user_resizable`](crate::tree::El::user_resizable) pane, in
    /// logical pixels. `None` until the user's first drag — the pane
    /// is still at its declared size. See
    /// [`UiState::user_size`](crate::UiState::user_size).
    pub fn user_size(&self, key: &str) -> Option<f32> {
        self.ui_state?.user_size(key)
    }

    /// The current view of the [`plot`](crate::tree::plot) keyed `key` —
    /// the read half of the **virtual-data pull** loop (see
    /// [`UiState::plot_view_by_key`](crate::state::UiState::plot_view_by_key)
    /// and `docs/PLOT2D_PLAN.md`, decision 5): read the visible window
    /// during `build`, and when it has drifted from what was last loaded,
    /// resample the source over the new range and `set` the series handle.
    /// `None` until the keyed plot has been laid out and resolved, or when
    /// no `UiState` is attached.
    pub fn plot_view(&self, key: &str) -> Option<crate::plot::PlotView> {
        self.ui_state?.plot_view_by_key(key)
    }
}

/// Read-only context passed to [`App::on_event`] /
/// [`App::on_wheel_event`].
///
/// Event handlers regularly need post-layout geometry to make a
/// decision — "which room row is under this drop?", "what size did
/// the lightbox body actually get?" — and the handler's state owns no
/// node, so it can't have carried the rect itself. `EventCx` is the
/// damascene analogue of the DOM's ambient `document`: a handle into
/// the retained layout the user is currently looking at, queryable by
/// key (`element.getBoundingClientRect()` shape). Geometry answers
/// from the last laid-out frame — exactly what's on screen when the
/// event fires.
///
/// Like [`BuildCx`], the struct is opaque so the API stays additive:
/// new accessors don't break apps that ignore the context.
#[derive(Copy, Clone, Debug, Default)]
pub struct EventCx<'a> {
    ui_state: Option<&'a crate::state::UiState>,
    diagnostics: Option<&'a HostDiagnostics>,
    viewport: Option<(f32, f32)>,
}

impl<'a> EventCx<'a> {
    /// Construct an empty context. Headless tests that drive
    /// [`App::on_event`] directly use this; real hosts chain
    /// [`Self::with_ui_state`] so geometry queries can answer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the runtime's [`crate::state::UiState`] so geometry
    /// accessors can answer. Hosts call this at every dispatch site;
    /// when omitted, the accessors return `None`.
    pub fn with_ui_state(mut self, ui_state: &'a crate::state::UiState) -> Self {
        self.ui_state = Some(ui_state);
        self
    }

    /// Attach the host's most recent [`HostDiagnostics`] snapshot —
    /// the one from the last built frame. Hosts chain this at every
    /// dispatch site so handlers can branch on negotiated output
    /// state (e.g. [`HostDiagnostics::hdr_active`], working color
    /// space) without mirroring it from `build` through app state.
    pub fn with_diagnostics(mut self, diagnostics: &'a HostDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Attach the logical-pixel viewport size the user is currently
    /// looking at. Hosts chain this so handlers that make
    /// layout-dependent decisions (grid-column navigation, breakpoint
    /// branches) don't have to stash the viewport from `build` in app
    /// state.
    pub fn with_viewport(mut self, width: f32, height: f32) -> Self {
        self.viewport = Some((width, height));
        self
    }

    /// The host's diagnostic snapshot from the last built frame, or
    /// `None` when the host did not attach one (headless dispatch).
    /// Same data as [`BuildCx::diagnostics`], one frame stale by
    /// construction — events fire between frames.
    pub fn diagnostics(&self) -> Option<&HostDiagnostics> {
        self.diagnostics
    }

    /// Host-reported user accessibility preferences — same contract as
    /// [`BuildCx::accessibility`]. All-`None` when no
    /// [`crate::state::UiState`] is attached.
    pub fn accessibility(&self) -> crate::a11y::AccessibilityPreferences {
        self.ui_state
            .map(|s| s.accessibility_preferences())
            .unwrap_or_default()
    }

    /// `true` iff the user prefers reduced motion — same contract as
    /// [`BuildCx::reduced_motion`].
    pub fn reduced_motion(&self) -> bool {
        self.accessibility().prefers_reduced_motion()
    }

    /// Logical-pixel viewport `(width, height)` at the time this
    /// event fires, or `None` when the host attached none. Same
    /// contract as [`BuildCx::viewport`].
    pub fn viewport(&self) -> Option<(f32, f32)> {
        self.viewport
    }

    /// Logical-pixel viewport width, or `None` when no viewport is
    /// attached. Convenience mirroring [`BuildCx::viewport_width`] —
    /// the common case is event-time navigation math that must agree
    /// with build-time layout (e.g. grid column count).
    pub fn viewport_width(&self) -> Option<f32> {
        self.viewport.map(|(w, _)| w)
    }

    /// Logical-pixel viewport height, or `None` when no viewport is
    /// attached.
    pub fn viewport_height(&self) -> Option<f32> {
        self.viewport.map(|(_, h)| h)
    }

    /// True iff the attached viewport's width is strictly less than
    /// `threshold` logical pixels; `false` when no viewport is
    /// attached. Same semantics as [`BuildCx::viewport_below`] so
    /// build- and event-time breakpoint branches agree.
    pub fn viewport_below(&self, threshold: f32) -> bool {
        self.viewport_width().is_some_and(|w| w < threshold)
    }

    /// The laid-out rect of the keyed node `key`, from the layout the
    /// user is looking at as this event fires. `None` when the key is
    /// absent from that tree (or no `UiState` is attached).
    ///
    /// This is the first-class shape for "the handler needs to know
    /// where a keyed thing landed": resolving a drop target against
    /// row rects on `PointerUp`, stepping zoom from a body's fitted
    /// size, anchoring app-drawn chrome to a control. The event's own
    /// target rect is already on [`UiEvent::target`]; this answers
    /// for *other* keys.
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.ui_state?.rect_of_key(key)
    }

    /// The live pose of a keyed scene camera by computed id (the moment the
    /// handler runs) — see
    /// [`UiState::scene_camera`](crate::state::UiState::scene_camera).
    /// Combined with [`UiEvent::target`]'s rect + pointer, build a
    /// screen→world ray for picking/placement; `id` is typically that
    /// target's `node_id`.
    ///
    /// [`UiEvent::target`]: crate::event::UiEvent::target
    pub fn scene_camera(&self, id: &str) -> Option<crate::scene::CameraState> {
        self.ui_state?.scene_camera(id)
    }

    /// The half-open row-index range the keyed virtual list realized in
    /// the layout the user is looking at — see
    /// [`BuildCx::visible_range`].
    pub fn visible_range(&self, key: &str) -> Option<std::ops::Range<usize>> {
        self.ui_state?.visible_range(key)
    }

    /// The current pan/zoom of the [`viewport`](crate::tree::viewport)
    /// keyed `key`, from the last layout — for a zoom-percentage readout
    /// or to project content into screen space. `None` until the keyed
    /// viewport has been laid out.
    pub fn viewport_view(&self, key: &str) -> Option<crate::viewport::ViewportView> {
        self.ui_state?.viewport_view_by_key(key)
    }

    /// The bounding box of the keyed viewport's laid-out content in
    /// content space — see [`BuildCx::viewport_content_bounds`].
    pub fn viewport_content_bounds(&self, key: &str) -> Option<Rect> {
        self.ui_state?.viewport_content_bounds_by_key(key)
    }

    /// Whether the keyed viewport is still at its home framing — see
    /// [`BuildCx::viewport_at_home`].
    pub fn viewport_at_home(&self, key: &str) -> Option<bool> {
        self.ui_state?.viewport_at_home_by_key(key)
    }

    /// Whether the keyed viewport is mid-flight on a smooth programmatic
    /// navigation — see [`BuildCx::viewport_in_flight`].
    pub fn viewport_in_flight(&self, key: &str) -> Option<bool> {
        self.ui_state?.viewport_in_flight_by_key(key)
    }

    /// The user-dragged size of a keyed
    /// [`user_resizable`](crate::tree::El::user_resizable) pane — the
    /// value to persist when a [`UiEventKind::Resized`] arrives. See
    /// [`BuildCx::user_size`].
    pub fn user_size(&self, key: &str) -> Option<f32> {
        self.ui_state?.user_size(key)
    }

    /// The current view of the [`plot`](crate::tree::plot) keyed `key`,
    /// at event time. Same contract as [`BuildCx::plot_view`].
    pub fn plot_view(&self, key: &str) -> Option<crate::plot::PlotView> {
        self.ui_state?.plot_view_by_key(key)
    }
}

/// The application contract. Implement this on your state struct and
/// pass it to a host runner (e.g., `damascene_winit_wgpu::run`).
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
    /// This is the drain half of the **mailbox pattern** — background
    /// work posts messages over a channel and wakes the host (native:
    /// a channel + the winit host's external `Wakeup`; web:
    /// `damascene_web::Mailbox`), and this hook folds them into app
    /// state so the frame being built sees them. See "Patterns real
    /// apps converge on" in the damascene-core README.
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
    ///
    /// # Page anatomy
    ///
    /// The returned tree is the *whole window*, and a bare
    /// `column([...])` root is almost never what a window wants: it
    /// has no padding (content sits flush against window edges and
    /// clips under rounded window corners) and no overlay root for
    /// `.tooltip()` layers to mount on. Return
    /// [`page`](crate::widgets::page::page) — it bakes the window
    /// padding + overlay root — and wrap it in
    /// [`overlays`](crate::overlays) when the app drives modals or
    /// dropdowns:
    ///
    /// ```ignore
    /// fn build(&self, _cx: &BuildCx) -> El {
    ///     overlays(
    ///         page([toolbar([...]), content()]),
    ///         [self.modal_open.then(|| modal("confirm", "Sure?", [...]))],
    ///     )
    /// }
    /// ```
    ///
    /// For custom anatomy (full-bleed canvases, centered Hug-sized
    /// cards), compose `stack([background, content])` by hand — see
    /// `damascene-fixtures/src/hero.rs` for the expanded idiom.
    fn build(&self, cx: &BuildCx) -> El;

    /// Update state in response to a routed event. Default: no-op.
    ///
    /// `cx` carries read-only frame context — most usefully
    /// [`EventCx::rect_of_key`], for decisions that depend on where a
    /// keyed node landed in the layout the user is looking at
    /// (resolving a drop target on `PointerUp`, stepping zoom from a
    /// measured size). Handlers that don't consult layout ignore it.
    fn on_event(&mut self, _event: UiEvent, _cx: &EventCx) {}

    /// Update state in response to routed wheel input.
    ///
    /// Return `true` to consume the wheel and suppress Damascene's default
    /// scroll routing. The default forwards to [`Self::on_event`] and
    /// returns `false`, so existing apps can observe wheel events
    /// without opting out of normal scrolling.
    fn on_wheel_event(&mut self, event: UiEvent, cx: &EventCx) -> bool {
        self.on_event(event, cx);
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
    ///
    /// # Multi-window scoping
    ///
    /// Hotkeys are scoped per `Runner`, and a multi-window host owns
    /// one `Runner` per window — so the contract is simply: **feed
    /// each window's `Runner` only that window's hotkey list**, and
    /// route each window's key events only to its own `Runner` (which
    /// a winit host does naturally, keyed by `WindowId`). A chord then
    /// fires in the window the OS focused, never globally. There is no
    /// cross-window registry to deduplicate or shadow; "global"
    /// accelerators are app policy — register the chord in every
    /// window's list and treat the resulting per-window `Hotkey` event
    /// as the same action.
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

    /// Drain programmatic [`crate::viewport::ViewportRequest`]s produced
    /// since the last frame — fit-to-content, reset, or center a
    /// [`crate::tree::viewport`] by its `.key(...)`. Hosts call this once
    /// per frame and forward to
    /// [`crate::runtime::RunnerCore::push_viewport_requests`]; each
    /// request is consumed during layout of the matching viewport, where
    /// its live rect and content extents are known. Apps accumulate
    /// requests from event handlers (e.g. a "Fit" toolbar button) in a
    /// `Vec<ViewportRequest>` field and `mem::take` it here, mirroring
    /// [`Self::drain_scroll_requests`].
    ///
    /// Default: no requests.
    fn drain_viewport_requests(&mut self) -> Vec<crate::viewport::ViewportRequest> {
        Vec::new()
    }

    /// Drain programmatic [`crate::plot::PlotRequest`]s produced since
    /// the last frame — fit-all or pin the X window of a
    /// [`crate::tree::plot`] by its `.key(...)`. Hosts call this once per
    /// frame and forward to
    /// [`crate::runtime::RunnerCore::push_plot_requests`]; each request
    /// is consumed during the plot prepare pass, where the live data
    /// bounds are known. Apps accumulate requests from event handlers
    /// (e.g. a "Fit" button or a "last 60 s" preset) in a
    /// `Vec<PlotRequest>` field and `mem::take` it here, mirroring
    /// [`Self::drain_viewport_requests`].
    ///
    /// Default: no requests.
    fn drain_plot_requests(&mut self) -> Vec<crate::plot::PlotRequest> {
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
    /// Registration name that nodes reference to bind the shader.
    pub name: &'static str,
    /// WGSL source the host registers with the backend at startup.
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

    #[test]
    fn pointer_button_from_linux_evdev_codes() {
        assert_eq!(
            PointerButton::from_linux_button(0x110),
            Some(PointerButton::Primary)
        );
        assert_eq!(
            PointerButton::from_linux_button(0x111),
            Some(PointerButton::Secondary)
        );
        assert_eq!(
            PointerButton::from_linux_button(0x112),
            Some(PointerButton::Middle)
        );
        // BTN_SIDE / BTN_EXTRA — not surfaced.
        assert_eq!(PointerButton::from_linux_button(0x113), None);
        assert_eq!(PointerButton::from_linux_button(0), None);
    }
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
    fn build_cx_surfaces_hovered_scene_point() {
        use crate::scene::ScenePointPick;
        let theme = Theme::default();
        let mut ui = crate::state::UiState::new();

        // No attached state, and none stored → nothing to surface.
        assert!(BuildCx::new(&theme).hovered_scene_point().is_none());
        assert!(
            BuildCx::new(&theme)
                .with_ui_state(&ui)
                .hovered_scene_point()
                .is_none()
        );

        // After the runtime stores a pick, the app reads it at build.
        ui.set_hovered_scene_point(Some(ScenePointPick {
            scene: "scene".into(),
            mark: 0,
            point: 4,
        }));
        let cx = BuildCx::new(&theme).with_ui_state(&ui);
        let pick = cx.hovered_scene_point().expect("pick surfaced");
        assert_eq!(
            (pick.scene.as_str(), pick.mark, pick.point),
            ("scene", 0, 4)
        );
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
    fn hdr_active_needs_output_evidence_and_wide_chosen_format() {
        use crate::color::{ColorManagementStatus, CompositorColorTargets, TransferFunction};

        let hdr_targets = CompositorColorTargets {
            preferred_transfer: Some(TransferFunction::Pq),
            ..Default::default()
        };
        let scrgb_surface = SurfaceColorInfo {
            formats: vec![
                SurfaceFormatInfo {
                    name: "Bgra8UnormSrgb".into(),
                    srgb: true,
                    wide: false,
                },
                SurfaceFormatInfo {
                    name: "Rgba16Float".into(),
                    srgb: false,
                    wide: true,
                },
            ],
            chosen_format: "Rgba16Float".into(),
            ..Default::default()
        };

        let mut d = HostDiagnostics::default();
        // Default: no protocol, no surface info.
        assert!(!d.hdr_active());

        // HDR output + scRGB swapchain → active. `attached` stays None
        // on the no-attach host — it must not factor in.
        d.color_management = ColorManagementStatus::Available {
            capabilities: Default::default(),
            attached: None,
            targets: hdr_targets.clone(),
        };
        d.surface_color = Some(scrgb_surface.clone());
        assert!(d.hdr_active());

        // HDR output but the negotiator stayed on 8-bit sRGB (e.g. the
        // app is sdr_only) → not active.
        d.surface_color = Some(SurfaceColorInfo {
            chosen_format: "Bgra8UnormSrgb".into(),
            ..scrgb_surface.clone()
        });
        assert!(!d.hdr_active());

        // Wide swapchain but no HDR evidence from the output → not active.
        d.color_management = ColorManagementStatus::Available {
            capabilities: Default::default(),
            attached: None,
            targets: CompositorColorTargets::default(),
        };
        d.surface_color = Some(scrgb_surface);
        assert!(!d.hdr_active());
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
