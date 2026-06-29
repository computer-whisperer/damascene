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
        .key_down(
            LogicalKey::Named(NamedKey::Enter),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        )
        .expect("activation event");

    assert_eq!(event.kind, UiEventKind::Activate);
    assert_eq!(event.key.as_deref(), Some("inc"));
    assert!(matches!(
        event.key_press.as_ref().map(|p| &p.logical),
        Some(LogicalKey::Named(NamedKey::Enter))
    ));
}

#[test]
fn enter_without_focus_is_key_down() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);

    let event = state
        .key_down(
            LogicalKey::Named(NamedKey::Enter),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        )
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
            .key_down(
                LogicalKey::Named(NamedKey::Tab),
                PhysicalKey::Unidentified,
                KeyModifiers::default(),
                false
            )
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
            LogicalKey::Character("f".to_string()),
            PhysicalKey::Unidentified,
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
            LogicalKey::Character("j".to_string()),
            PhysicalKey::Unidentified,
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
            LogicalKey::Character("f".to_string()),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        )
        .expect("event for unhandled key");
    assert_eq!(plain.kind, UiEventKind::KeyDown);
    assert_eq!(plain.key, None);

    // Ctrl+Shift+F also differs from Ctrl+F (strict modifier match).
    let extra = state
        .key_down(
            LogicalKey::Character("f".to_string()),
            PhysicalKey::Unidentified,
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
        KeyChord::named(LogicalKey::Named(NamedKey::Enter)).with_modifiers(KeyModifiers {
            ctrl: true,
            ..Default::default()
        }),
        "submit".to_string(),
    )]);

    let event = state
        .key_down(
            LogicalKey::Named(NamedKey::Enter),
            PhysicalKey::Unidentified,
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
        .key_down(
            LogicalKey::Named(NamedKey::Enter),
            PhysicalKey::Unidentified,
            KeyModifiers::default(),
            false,
        )
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
            LogicalKey::Character("A".to_string()),
            PhysicalKey::Unidentified,
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

#[test]
fn physical_chord_matches_position_regardless_of_layout() {
    // A physical chord binds a board position, not a legend. On AZERTY the
    // QWERTY-`W` position produces the logical character `z`; a
    // `KeyChord::physical(KeyW)` must still fire (WASD stays WASD).
    let mut state = UiState::new();
    state.set_hotkeys(vec![(
        KeyChord::physical(PhysicalKey::KeyW),
        "move-forward".to_string(),
    )]);

    let event = state
        .key_down(
            LogicalKey::Character("z".to_string()),
            PhysicalKey::KeyW,
            KeyModifiers::default(),
            false,
        )
        .expect("physical hotkey");
    assert_eq!(event.kind, UiEventKind::Hotkey);
    assert_eq!(event.key.as_deref(), Some("move-forward"));

    // The matching logical character from a *different* position does not
    // fire the physical chord.
    let other = state.key_down(
        LogicalKey::Character("w".to_string()),
        PhysicalKey::KeyZ,
        KeyModifiers::default(),
        false,
    );
    assert!(other.map(|e| e.kind) != Some(UiEventKind::Hotkey));
}

#[test]
fn physical_chord_distinguishes_numpad_from_main_row() {
    // Numpad `1` and the number-row `1` share the logical key and modifier
    // mask; only the physical facet tells them apart (issue #114's numpad
    // case).
    let mut state = UiState::new();
    state.set_hotkeys(vec![(
        KeyChord::physical(PhysicalKey::Numpad1),
        "numpad-one".to_string(),
    )]);

    let main_row = state.key_down(
        LogicalKey::Character("1".to_string()),
        PhysicalKey::Digit1,
        KeyModifiers::default(),
        false,
    );
    assert!(main_row.map(|e| e.kind) != Some(UiEventKind::Hotkey));

    let numpad = state
        .key_down(
            LogicalKey::Character("1".to_string()),
            PhysicalKey::Numpad1,
            KeyModifiers::default(),
            false,
        )
        .expect("numpad hotkey");
    assert_eq!(numpad.kind, UiEventKind::Hotkey);
    assert_eq!(numpad.key.as_deref(), Some("numpad-one"));
}
