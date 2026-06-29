//! Pure winit → damascene input mappers.
//!
//! Leaf translation functions with no host state — a custom
//! `ApplicationHandler` calls these from its own `window_event` to
//! produce the values `damascene_wgpu::Runner`'s input methods expect.
//! The built-in run loop routes every event through these same
//! functions.
//!
//! Note the winit version coupling: these signatures take this crate's
//! winit types, so a custom host must use the same winit major version
//! (re-check `Cargo.toml` on upgrades).

use damascene_core::{Cursor, KeyModifiers, LogicalKey, NamedKey, PhysicalKey, PointerButton};
use winit::event::{Force, MouseButton};
use winit::keyboard::{Key, KeyCode, NamedKey as WinitNamedKey, PhysicalKey as WinitPhysicalKey};
use winit::window::CursorIcon;

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
/// The two vocabularies both follow the W3C `key` named set, so the
/// shared names map 1:1; names damascene does not (yet) model return
/// `None` and surface as [`LogicalKey::Unidentified`].
fn map_named(named: &WinitNamedKey) -> Option<NamedKey> {
    // Every arm is a same-named pair (winit and damascene both mirror the
    // W3C `key` vocabulary), so the macro keeps the 1:1 table honest.
    macro_rules! same {
        ($($v:ident),+ $(,)?) => {
            Some(match named {
                $( WinitNamedKey::$v => NamedKey::$v, )+
                _ => return None,
            })
        };
    }
    same!(
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
        Eject,
        Power,
        PrintScreen,
        WakeUp,
        AudioVolumeDown,
        AudioVolumeMute,
        AudioVolumeUp,
        MediaPlayPause,
        MediaStop,
        MediaTrackNext,
        MediaTrackPrevious,
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
    )
}

/// Translate a winit [`PhysicalKey`](WinitPhysicalKey) to a damascene
/// [`PhysicalKey`] — the layout-independent board position (W3C `code`).
///
/// winit's [`KeyCode`] follows the same W3C `code` spec, so the shared
/// names map 1:1; the few that differ in spelling (winit's
/// `SuperLeft`/`SuperRight` are the W3C `MetaLeft`/`MetaRight`;
/// `NumpadStar` is `NumpadMultiply`) are bridged explicitly. Native /
/// unmapped codes become [`PhysicalKey::Unidentified`].
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
    same!(
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
    )
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
/// here, otherwise it falls back to the platform default).
///
/// winit hosts on other render backends (vulkano, ash) don't need this
/// crate for the mapping: `Cursor::css_name()` in damascene-core
/// parses straight into winit's `CursorIcon`
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

    /// `winit_cursor` and `Cursor::css_name()` are two spellings of
    /// the same mapping — winit's `CursorIcon` parses CSS cursor
    /// names, so the table here must agree with core's names or the
    /// wgpu-free path (vulkano/ash winit hosts) drifts.
    #[test]
    fn winit_cursor_agrees_with_css_name_parsing() {
        let all = [
            Cursor::Default,
            Cursor::Pointer,
            Cursor::Text,
            Cursor::NotAllowed,
            Cursor::Grab,
            Cursor::Grabbing,
            Cursor::Move,
            Cursor::EwResize,
            Cursor::NsResize,
            Cursor::NwseResize,
            Cursor::NeswResize,
            Cursor::ColResize,
            Cursor::RowResize,
            Cursor::Crosshair,
        ];
        for cursor in all {
            let parsed: CursorIcon = cursor
                .css_name()
                .parse()
                .unwrap_or_else(|_| panic!("css_name {:?} should parse", cursor.css_name()));
            assert_eq!(parsed, winit_cursor(cursor), "variant {cursor:?}");
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
