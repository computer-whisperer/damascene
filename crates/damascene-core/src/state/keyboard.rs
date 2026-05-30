//! Keyboard modifiers, hotkeys, and focused-key dispatch.

use crate::event::{KeyChord, KeyModifiers, KeyPress, UiEvent, UiEventKind, UiKey};

use super::UiState;

impl UiState {
    /// Replace the hotkey registry. Called by the host runner from
    /// `App::hotkeys()` once per build cycle.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.hotkeys.registry = hotkeys;
    }

    /// Update the tracked modifier mask. Hosts call this from their
    /// platform's "modifiers changed" hook (e.g. winit's
    /// `WindowEvent::ModifiersChanged`); the value is stamped into
    /// `UiEvent.modifiers` for every subsequent pointer event so
    /// widgets can detect Shift+click / Ctrl+drag without needing a
    /// per-call modifier parameter.
    pub fn set_modifiers(&mut self, modifiers: KeyModifiers) {
        self.modifiers = modifiers;
    }

    /// Match `key + modifiers` against the registered hotkey chords.
    /// Returns a `Hotkey` event if any registered chord matches; the
    /// `event.key` is the chord's registered name. Used by both the
    /// library-default path and the capture-keys path (hotkeys always
    /// win over a widget's raw key capture).
    pub fn try_hotkey(
        &self,
        key: &UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        let (_, name) = self
            .hotkeys
            .registry
            .iter()
            .find(|(chord, _)| chord.matches(key, modifiers))?;
        Some(UiEvent {
            key: Some(name.clone()),
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key: key.clone(),
                modifiers,
                repeat,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::Hotkey,
        })
    }

    /// Build a raw `KeyDown` event routed to the focused target,
    /// bypassing the library's Tab/Enter/Escape interpretation. Used
    /// by the runner when the focused node has `capture_keys=true`.
    /// Returns `None` if no node is focused.
    pub fn key_down_raw(
        &self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        let target = self.focused.clone()?;
        Some(UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        })
    }

    pub fn key_down(
        &mut self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        if matches!(key, UiKey::Tab) {
            if modifiers.shift {
                self.focus_prev();
            } else {
                self.focus_next();
            }
            self.set_focus_visible(true);
            return None;
        }

        // Hotkeys win over focused-Enter activation: a focused button
        // with no hotkey on Enter still activates, but Ctrl+Enter (if
        // registered) routes to its hotkey instead. Registration order
        // is precedence — first match wins.
        if let Some(event) = self.try_hotkey(&key, modifiers, repeat) {
            return Some(event);
        }

        let target = self.focused.clone();
        // `:focus-visible` rule: raise the ring only when the key is
        // unambiguous keyboard interaction with the focused widget —
        // navigation arrows / Home / End / PageUp / PageDown, or
        // Enter / Space activation. A Ctrl/Cmd/Alt-held key is a
        // global shortcut; the focused widget is incidental and
        // shouldn't flash. Character / function / Escape keys also
        // don't count — they're typing, dismissal, or app actions,
        // not "I'm steering this widget with the keyboard." Tab
        // already raised the ring above when it moved focus.
        if target.is_some() && raises_focus_visible(&key, modifiers) {
            self.set_focus_visible(true);
        }
        let kind = match (&key, target.is_some()) {
            (UiKey::Enter | UiKey::Space, true) => UiEventKind::Activate,
            (UiKey::Escape, _) => UiEventKind::Escape,
            _ => UiEventKind::KeyDown,
        };
        Some(UiEvent {
            key: target.as_ref().map(|t| t.key.clone()),
            target,
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            path: None,
            pointer_kind: None,
            wheel_delta: None,
            kind,
        })
    }
}

/// Whether `key` (with `modifiers` held) should turn on the focus
/// ring on a pointer-focused widget. Conservative whitelist — see
/// [`UiState::key_down`] for the rationale.
fn raises_focus_visible(key: &UiKey, modifiers: KeyModifiers) -> bool {
    if modifiers.ctrl || modifiers.alt || modifiers.logo {
        return false;
    }
    matches!(
        key,
        UiKey::ArrowUp
            | UiKey::ArrowDown
            | UiKey::ArrowLeft
            | UiKey::ArrowRight
            | UiKey::Home
            | UiKey::End
            | UiKey::PageUp
            | UiKey::PageDown
            | UiKey::Enter
            | UiKey::Space
    )
}
