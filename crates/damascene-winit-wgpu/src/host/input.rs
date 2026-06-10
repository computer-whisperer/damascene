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

use damascene_core::{Cursor, KeyModifiers, PointerButton, UiKey};
use winit::event::{Force, MouseButton};
use winit::keyboard::{Key, NamedKey};
use winit::window::CursorIcon;

/// Translate a winit logical [`Key`] to a damascene [`UiKey`].
///
/// Named keys with first-class damascene variants map 1:1; printable
/// input becomes [`UiKey::Character`]; every other named key is
/// preserved as [`UiKey::Other`] with winit's debug name (so hotkey
/// chords can still bind e.g. function keys). Dead keys and
/// unidentified keys return `None` — there is nothing meaningful to
/// dispatch.
pub fn map_key(key: &Key) -> Option<UiKey> {
    match key {
        Key::Named(NamedKey::Enter) => Some(UiKey::Enter),
        Key::Named(NamedKey::Escape) => Some(UiKey::Escape),
        Key::Named(NamedKey::Tab) => Some(UiKey::Tab),
        Key::Named(NamedKey::Space) => Some(UiKey::Space),
        Key::Named(NamedKey::ArrowUp) => Some(UiKey::ArrowUp),
        Key::Named(NamedKey::ArrowDown) => Some(UiKey::ArrowDown),
        Key::Named(NamedKey::ArrowLeft) => Some(UiKey::ArrowLeft),
        Key::Named(NamedKey::ArrowRight) => Some(UiKey::ArrowRight),
        Key::Named(NamedKey::Backspace) => Some(UiKey::Backspace),
        Key::Named(NamedKey::Delete) => Some(UiKey::Delete),
        Key::Named(NamedKey::Home) => Some(UiKey::Home),
        Key::Named(NamedKey::End) => Some(UiKey::End),
        Key::Named(NamedKey::PageUp) => Some(UiKey::PageUp),
        Key::Named(NamedKey::PageDown) => Some(UiKey::PageDown),
        Key::Character(s) => Some(UiKey::Character(s.to_string())),
        Key::Named(named) => Some(UiKey::Other(format!("{named:?}"))),
        _ => None,
    }
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
