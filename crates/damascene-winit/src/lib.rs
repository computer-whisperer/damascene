//! Pure winit → damascene input mappers.
//!
//! Leaf translation functions with no host state — a custom
//! `ApplicationHandler` calls these from its own `window_event` to
//! produce the values `damascene_core`'s runtime input methods expect.
//! They are render-backend-neutral: the in-tree wgpu host routes every
//! event through these same functions, and vulkano / ash / out-of-tree
//! hosts use them without pulling in any GPU stack (this crate depends
//! on `damascene-core` and `winit` only — see issue #121).
//!
//! Since the three-facet key model (#114) the mappings are total tables
//! over the W3C named-key and physical-code vocabularies; the tests here
//! assert that coverage against `damascene_core`'s `ALL` consts, so a
//! new core variant fails this crate's tests instead of silently
//! degrading to `Unidentified` in every host.
//!
//! Note the winit version coupling: these signatures take this crate's
//! winit types, so a host must use the same winit major version
//! (re-check `Cargo.toml` on upgrades).

#![warn(missing_docs)]

use damascene_core::{Cursor, KeyModifiers, LogicalKey, NamedKey, PhysicalKey, PointerButton};
use winit::event::{Force, MouseButton};
use winit::keyboard::{Key, KeyCode, NamedKey as WinitNamedKey, PhysicalKey as WinitPhysicalKey};
use winit::window::CursorIcon;

/// X-macro table of the named keys winit and damascene share — every
/// entry is a same-named pair (both vocabularies mirror the W3C `key`
/// spec), applied by [`map_key`]'s translation match and re-applied by
/// the totality test to prove the table reaches all of
/// [`NamedKey::ALL`].
macro_rules! for_each_named_key {
    ($apply:ident) => {
        $apply! {
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
    };
}

/// X-macro table of the physical codes winit and damascene spell
/// identically (both follow the W3C `code` spec). The few codes whose
/// spellings differ are bridged explicitly in [`map_physical`], and the
/// totality test proves table + bridges reach all of
/// [`PhysicalKey::ALL`].
macro_rules! for_each_physical_key {
    ($apply:ident) => {
        $apply! {
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
        }
    };
}

/// Translate a winit logical [`Key`] to a damascene [`LogicalKey`] — the
/// key's layout-dependent meaning.
///
/// Named keys map onto [`NamedKey`] (the W3C `key` named set), printable
/// input becomes [`LogicalKey::Character`], and anything without a logical
/// meaning damascene models — dead keys, unmapped/rare named keys —
/// becomes [`LogicalKey::Unidentified`]. The mapping is total (no `None`):
/// a key with no logical identity can still carry a useful
/// [physical][`map_physical`] one, so the caller decides whether to
/// dispatch based on both facets rather than dropping the event here.
pub fn map_key(key: &Key) -> LogicalKey {
    match key {
        Key::Named(named) => match map_named(named) {
            Some(n) => LogicalKey::Named(n),
            None => LogicalKey::Unidentified,
        },
        Key::Character(s) => LogicalKey::Character(s.to_string()),
        _ => LogicalKey::Unidentified,
    }
}

/// Map a winit [`NamedKey`](WinitNamedKey) to damascene's [`NamedKey`].
/// The shared names map 1:1 through [`for_each_named_key!`]; names
/// damascene does not (yet) model return `None` and surface as
/// [`LogicalKey::Unidentified`].
fn map_named(named: &WinitNamedKey) -> Option<NamedKey> {
    macro_rules! same {
        ($($v:ident),+ $(,)?) => {
            Some(match named {
                $( WinitNamedKey::$v => NamedKey::$v, )+
                _ => return None,
            })
        };
    }
    for_each_named_key!(same)
}

/// Translate a winit [`PhysicalKey`](WinitPhysicalKey) to a damascene
/// [`PhysicalKey`] — the layout-independent board position (W3C `code`).
///
/// winit's [`KeyCode`] follows the same W3C `code` spec, so the shared
/// names map 1:1 through `for_each_physical_key!`; the few that differ
/// in spelling (winit's `SuperLeft`/`SuperRight` are the W3C
/// `MetaLeft`/`MetaRight`; `NumpadStar` is `NumpadMultiply`) are bridged
/// explicitly. Native / unmapped codes become
/// [`PhysicalKey::Unidentified`].
pub fn map_physical(physical: WinitPhysicalKey) -> PhysicalKey {
    let code = match physical {
        WinitPhysicalKey::Code(code) => code,
        WinitPhysicalKey::Unidentified(_) => return PhysicalKey::Unidentified,
    };
    macro_rules! same {
        ($($v:ident),+ $(,)?) => {
            match code {
                $( KeyCode::$v => PhysicalKey::$v, )+
                // Spelling bridges (winit → W3C `code`).
                KeyCode::SuperLeft => PhysicalKey::MetaLeft,
                KeyCode::SuperRight => PhysicalKey::MetaRight,
                KeyCode::NumpadStar => PhysicalKey::NumpadMultiply,
                _ => PhysicalKey::Unidentified,
            }
        };
    }
    for_each_physical_key!(same)
}

/// Translate a winit [`MouseButton`] to a damascene [`PointerButton`].
pub fn pointer_button(b: MouseButton) -> Option<PointerButton> {
    match b {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        // Back / Forward / Other → not surfaced; apps that need them can
        // grow the enum.
        _ => None,
    }
}

/// Normalize a winit touch [`Force`] to the `[0, 1]` pressure value
/// `Pointer::with_pressure` expects. `None` in, `None` out — winit
/// reports no force on platforms/devices without a pressure sensor.
pub fn touch_pressure(force: Option<Force>) -> Option<f32> {
    match force? {
        Force::Calibrated {
            force,
            max_possible_force,
            ..
        } if max_possible_force > 0.0 => Some((force / max_possible_force).clamp(0.0, 1.0) as f32),
        Force::Calibrated { force, .. } => Some(force.clamp(0.0, 1.0) as f32),
        Force::Normalized(v) => Some(v.clamp(0.0, 1.0) as f32),
    }
}

/// Translate a damascene [`Cursor`] to winit's [`CursorIcon`]. The
/// damascene enum is a subset of winit's so this stays a 1:1 map; the
/// wildcard arm is a forward-compat safety net (damascene's `Cursor` is
/// `non_exhaustive` — add a new variant in core, add the matching arm
/// here, otherwise it falls back to the platform default; the test
/// against [`Cursor::ALL`] catches the gap).
///
/// Equivalent alternative without this crate: `Cursor::css_name()` in
/// damascene-core parses straight into winit's `CursorIcon`
/// (`cursor.css_name().parse::<CursorIcon>().unwrap_or_default()`).
pub fn winit_cursor(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Default => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::NotAllowed => CursorIcon::NotAllowed,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::Move => CursorIcon::Move,
        Cursor::EwResize => CursorIcon::EwResize,
        Cursor::NsResize => CursorIcon::NsResize,
        Cursor::NwseResize => CursorIcon::NwseResize,
        Cursor::NeswResize => CursorIcon::NeswResize,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::Crosshair => CursorIcon::Crosshair,
        _ => CursorIcon::Default,
    }
}

/// Translate winit's [`ModifiersState`](winit::keyboard::ModifiersState)
/// to damascene [`KeyModifiers`].
pub fn key_modifiers(mods: winit::keyboard::ModifiersState) -> KeyModifiers {
    KeyModifiers {
        shift: mods.shift_key(),
        ctrl: mods.control_key(),
        alt: mods.alt_key(),
        logo: mods.super_key(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every named key core models must be reachable from the winit key
    /// of the same W3C name. When core grows a variant, `NamedKey::ALL`
    /// grows with it (same macro listing) and this fails until the
    /// [`for_each_named_key!`] table gains the entry — instead of the
    /// key silently degrading to `Unidentified` in every winit host.
    #[test]
    fn map_named_covers_every_core_named_key() {
        macro_rules! as_slice {
            ($($v:ident),+ $(,)?) => { &[ $(NamedKey::$v),+ ] };
        }
        let table: &[NamedKey] = for_each_named_key!(as_slice);
        for k in NamedKey::ALL {
            assert!(
                table.contains(k),
                "NamedKey::{k:?} is unreachable from winit — extend for_each_named_key!"
            );
        }
    }

    /// Every physical key core models must be reachable from some winit
    /// `KeyCode` — via the same-spelling table, a spelling bridge, or
    /// the no-position fallback. Same rot guard as the named-key test.
    #[test]
    fn map_physical_covers_every_core_physical_key() {
        macro_rules! as_slice {
            ($($v:ident),+ $(,)?) => { &[ $(PhysicalKey::$v),+ ] };
        }
        let table: &[PhysicalKey] = for_each_physical_key!(as_slice);
        for k in PhysicalKey::ALL {
            let covered = table.contains(k)
                || matches!(
                    k,
                    // Bridged from winit's `SuperLeft`/`SuperRight` spelling.
                    PhysicalKey::MetaLeft | PhysicalKey::MetaRight
                    // Produced by the `WinitPhysicalKey::Unidentified` arm.
                    | PhysicalKey::Unidentified
                );
            assert!(
                covered,
                "PhysicalKey::{k:?} is unreachable from winit — extend for_each_physical_key! (or the bridges in map_physical)"
            );
        }
    }

    /// `winit_cursor` and `Cursor::css_name()` are two spellings of
    /// the same mapping — winit's `CursorIcon` parses CSS cursor
    /// names, so the table here must agree with core's names or the
    /// wgpu-free path drifts. Iterating `Cursor::ALL` also makes this
    /// the totality guard: a new core cursor variant hits the
    /// wildcard `Default` arm and mismatches its parsed CSS name.
    #[test]
    fn winit_cursor_agrees_with_css_name_parsing() {
        for cursor in Cursor::ALL {
            let parsed: CursorIcon = cursor
                .css_name()
                .parse()
                .unwrap_or_else(|_| panic!("css_name {:?} should parse", cursor.css_name()));
            assert_eq!(parsed, winit_cursor(*cursor), "variant {cursor:?}");
        }
    }

    /// winit's `KeyCode` and damascene's `PhysicalKey` both mirror the W3C
    /// `code` set, but a few names differ in spelling — those bridges are
    /// the only thing that can silently rot, so pin them.
    #[test]
    fn map_physical_bridges_winit_spelling_to_w3c() {
        let code = |c| map_physical(WinitPhysicalKey::Code(c));
        assert_eq!(code(KeyCode::SuperLeft), PhysicalKey::MetaLeft);
        assert_eq!(code(KeyCode::SuperRight), PhysicalKey::MetaRight);
        assert_eq!(code(KeyCode::NumpadStar), PhysicalKey::NumpadMultiply);
        // 1:1 names pass straight through, and numpad vs main row stay
        // distinct (the whole point of exposing physical identity).
        assert_eq!(code(KeyCode::KeyA), PhysicalKey::KeyA);
        assert_eq!(code(KeyCode::Numpad1), PhysicalKey::Numpad1);
        assert_ne!(code(KeyCode::Digit1), code(KeyCode::Numpad1));
        // A native scancode with no W3C `code` is Unidentified, never a
        // host-formatted string.
        assert_eq!(
            map_physical(WinitPhysicalKey::Unidentified(
                winit::keyboard::NativeKeyCode::Unidentified
            )),
            PhysicalKey::Unidentified
        );
    }

    /// A named key damascene does not model must surface as
    /// `Unidentified`, never the old `Debug`-string fallback.
    #[test]
    fn map_key_unmapped_named_is_unidentified() {
        assert_eq!(
            map_key(&Key::Named(WinitNamedKey::LaunchMail)),
            LogicalKey::Unidentified
        );
        assert_eq!(
            map_key(&Key::Named(WinitNamedKey::Enter)),
            LogicalKey::Named(NamedKey::Enter)
        );
    }
}
