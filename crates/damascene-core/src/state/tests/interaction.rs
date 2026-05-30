use super::support::*;

#[test]
fn ui_state_applies_hover() {
    let (tree, mut state) = lay_out_counter();
    state.hovered = Some(target(&tree, &state, "inc"));
    state.apply_to_state();
    assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Hover);
    assert_eq!(node_state(&tree, &state, "dec"), InteractionState::Default);
}

#[test]
fn ui_state_press_wins_over_hover_on_same_key() {
    let (tree, mut state) = lay_out_counter();
    let inc = target(&tree, &state, "inc");
    state.hovered = Some(inc.clone());
    state.pressed = Some(inc);
    state.apply_to_state();
    assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);
}

#[test]
fn ui_state_press_decays_when_pointer_drags_off_pressed_target() {
    // `:active`-style behaviour: the press visual only renders while
    // the pointer is still over the originally-pressed node. Drag
    // off → pressed target falls back to Default; the newly-hovered
    // node gets its own Hover.
    let (tree, mut state) = lay_out_counter();
    let inc = target(&tree, &state, "inc");
    let dec = target(&tree, &state, "dec");

    // Press on inc, pointer still on inc → Press.
    state.hovered = Some(inc.clone());
    state.pressed = Some(inc.clone());
    state.apply_to_state();
    assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);

    // Drag off inc onto dec while still holding the button.
    state.hovered = Some(dec);
    state.apply_to_state();
    assert_eq!(
        node_state(&tree, &state, "inc"),
        InteractionState::Default,
        "press visual cancels when pointer leaves the pressed target",
    );
    assert_eq!(
        node_state(&tree, &state, "dec"),
        InteractionState::Hover,
        "the newly-hovered node still gets its own hover state",
    );

    // Drag back onto inc → Press resumes.
    state.hovered = Some(inc);
    state.apply_to_state();
    assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);
}

#[test]
fn ui_state_press_decays_when_pointer_leaves_window() {
    // Same shape as drag-off, but the pointer leaves the window
    // entirely (hovered = None). The press visual should decay so
    // the user sees "release here cancels" feedback even when the
    // cursor is outside the surface.
    let (tree, mut state) = lay_out_counter();
    let inc = target(&tree, &state, "inc");
    state.hovered = Some(inc.clone());
    state.pressed = Some(inc);
    state.apply_to_state();
    assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);

    state.hovered = None;
    state.apply_to_state();
    assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Default);
}
