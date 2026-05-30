use super::support::*;

#[test]
fn shift_tab_moves_focus_backward() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    state.focus_prev();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
}

#[test]
fn enter_key_activates_focused_target() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    state.focus_next();
    state.focus_next();

    let event = state
        .key_down(UiKey::Enter, KeyModifiers::default(), false)
        .expect("activation event");

    assert_eq!(event.kind, UiEventKind::Activate);
    assert_eq!(event.key.as_deref(), Some("inc"));
    assert!(matches!(
        event.key_press.as_ref().map(|p| &p.key),
        Some(UiKey::Enter)
    ));
}

#[test]
fn enter_without_focus_is_key_down() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);

    let event = state
        .key_down(UiKey::Enter, KeyModifiers::default(), false)
        .expect("key event");

    assert_eq!(event.kind, UiEventKind::KeyDown);
    assert_eq!(event.key, None);
}

#[test]
fn tab_changes_focus_without_app_event() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);

    assert!(
        state
            .key_down(UiKey::Tab, KeyModifiers::default(), false)
            .is_none()
    );
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));
}

#[test]
fn hotkey_match_emits_hotkey_event() {
    let mut state = UiState::new();
    state.set_hotkeys(vec![
        (KeyChord::ctrl('f'), "search".to_string()),
        (KeyChord::vim('j'), "down".to_string()),
    ]);

    let event = state
        .key_down(
            UiKey::Character("f".to_string()),
            KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
            false,
        )
        .expect("hotkey event");
    assert_eq!(event.kind, UiEventKind::Hotkey);
    assert_eq!(event.key.as_deref(), Some("search"));

    let down = state
        .key_down(
            UiKey::Character("j".to_string()),
            KeyModifiers::default(),
            false,
        )
        .expect("vim event");
    assert_eq!(down.key.as_deref(), Some("down"));
}

#[test]
fn hotkey_misses_when_modifiers_differ() {
    let mut state = UiState::new();
    state.set_hotkeys(vec![(KeyChord::ctrl('f'), "search".to_string())]);

    // Plain `f` (no modifiers) must not match Ctrl+F.
    let plain = state
        .key_down(
            UiKey::Character("f".to_string()),
            KeyModifiers::default(),
            false,
        )
        .expect("event for unhandled key");
    assert_eq!(plain.kind, UiEventKind::KeyDown);
    assert_eq!(plain.key, None);

    // Ctrl+Shift+F also differs from Ctrl+F (strict modifier match).
    let extra = state
        .key_down(
            UiKey::Character("f".to_string()),
            KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
            false,
        )
        .expect("event");
    assert_eq!(extra.kind, UiEventKind::KeyDown);
}

#[test]
fn hotkey_wins_over_focused_activate() {
    // A hotkey on Ctrl+Enter should not be intercepted by the
    // focused-Enter activation routing.
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    state.focus_next();
    state.set_hotkeys(vec![(
        KeyChord {
            key: UiKey::Enter,
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
        },
        "submit".to_string(),
    )]);

    let event = state
        .key_down(
            UiKey::Enter,
            KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
            false,
        )
        .expect("event");
    assert_eq!(event.kind, UiEventKind::Hotkey);
    assert_eq!(event.key.as_deref(), Some("submit"));

    // Plain Enter still activates the focused button.
    let activate = state
        .key_down(UiKey::Enter, KeyModifiers::default(), false)
        .expect("event");
    assert_eq!(activate.kind, UiEventKind::Activate);
}

#[test]
fn hotkey_character_match_is_case_insensitive() {
    // Winit reports Shift+a as Character("A"). A `KeyChord::ctrl('a')`
    // with Shift held should still not match (modifier mask differs),
    // but `KeyChord::ctrl_shift('a')` should.
    let mut state = UiState::new();
    state.set_hotkeys(vec![(KeyChord::ctrl_shift('a'), "select-all".to_string())]);

    let event = state
        .key_down(
            UiKey::Character("A".to_string()),
            KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
            false,
        )
        .expect("event");
    assert_eq!(event.key.as_deref(), Some("select-all"));
}
