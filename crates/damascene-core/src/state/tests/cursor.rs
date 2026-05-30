use super::support::*;

fn lay_out_cursor_tree() -> (El, UiState) {
    // Panel declares Move; one child has its own `.cursor(Pointer)`
    // (declared); a sibling stack carries no cursor and inherits
    // Move from the panel. Plain stacks (not buttons) so the
    // widget kit's own cursor defaults can't drift the test.
    let mut tree = column([row([
        El::new(Kind::Group).key("undeclared"),
        El::new(Kind::Group).key("declared").cursor(Cursor::Pointer),
    ])])
    .key("panel")
    .cursor(Cursor::Move)
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    (tree, state)
}

#[test]
fn cursor_is_default_when_no_hover_no_press() {
    let (tree, state) = lay_out_cursor_tree();
    assert_eq!(state.cursor(&tree), Cursor::Default);
}

#[test]
fn cursor_returns_hovered_targets_explicit_declaration() {
    let (tree, mut state) = lay_out_cursor_tree();
    state.hovered = Some(target(&tree, &state, "declared"));
    assert_eq!(state.cursor(&tree), Cursor::Pointer);
}

#[test]
fn cursor_inherits_from_ancestor_when_target_undeclared() {
    // The "undeclared" button has no `.cursor(...)`, so the panel's
    // `Move` propagates down — the inheritance rule that lets a
    // pan-surface declare cursor once on the container.
    let (tree, mut state) = lay_out_cursor_tree();
    state.hovered = Some(target(&tree, &state, "undeclared"));
    assert_eq!(state.cursor(&tree), Cursor::Move);
}

#[test]
fn cursor_press_capture_overrides_hovered_target() {
    // Press on the Pointer button, drag onto the Move-inheriting
    // sibling. The cursor stays Pointer for the duration of the
    // press — matches native press-and-hold behaviour.
    let (tree, mut state) = lay_out_cursor_tree();
    let declared = target(&tree, &state, "declared");
    let undeclared = target(&tree, &state, "undeclared");
    state.pressed = Some(declared);
    state.hovered = Some(undeclared);
    assert_eq!(state.cursor(&tree), Cursor::Pointer);
}

#[test]
fn cursor_pressed_overrides_resting_cursor_on_press_target() {
    // `cursor` at rest, `cursor_pressed` while the press anchors
    // here. Mirrors the slider's Grab → Grabbing transition idiom.
    let mut tree = column([El::new(Kind::Group)
        .key("handle")
        .cursor(Cursor::Grab)
        .cursor_pressed(Cursor::Grabbing)]);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let handle = target(&tree, &state, "handle");

    // Hover → resting cursor.
    state.hovered = Some(handle.clone());
    assert_eq!(state.cursor(&tree), Cursor::Grab);

    // Press → pressed-cursor wins (and stays once the pointer
    // wanders off — press capture anchors the cursor).
    state.pressed = Some(handle);
    assert_eq!(state.cursor(&tree), Cursor::Grabbing);
    state.hovered = None;
    assert_eq!(
        state.cursor(&tree),
        Cursor::Grabbing,
        "press capture keeps the pressed cursor stable when the pointer drags off",
    );
}

#[test]
fn cursor_pressed_does_not_inherit_from_ancestor_to_descendant() {
    // Only the literal press target's `cursor_pressed` matters.
    // A parent that declared `cursor_pressed` shouldn't re-skin a
    // descendant's press — ancestors should use `cursor` (which
    // does inherit) when they want subtree-wide affordances.
    let mut tree = column([row([El::new(Kind::Group).key("inner")])
        .key("outer")
        .cursor_pressed(Cursor::Grabbing)]);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    state.pressed = Some(target(&tree, &state, "inner"));
    // Outer's `cursor_pressed` doesn't leak to the inner press
    // target; with no `cursor` chain at all, falls through to
    // Default.
    assert_eq!(state.cursor(&tree), Cursor::Default);
}

#[test]
fn cursor_pressed_falls_through_to_resting_cursor_when_unset() {
    // Press target without `cursor_pressed` still resolves via
    // the standard walk-up — the new branch is purely additive.
    let mut tree = column([El::new(Kind::Group).key("btn").cursor(Cursor::Pointer)]);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    state.pressed = Some(target(&tree, &state, "btn"));
    assert_eq!(state.cursor(&tree), Cursor::Pointer);
}

#[test]
fn cursor_falls_back_to_default_when_target_id_not_in_tree() {
    // Stale tracker (target was removed from the tree mid-frame)
    // shouldn't panic — fall through to Default.
    let (tree, mut state) = lay_out_cursor_tree();
    state.hovered = Some(UiTarget {
        key: "ghost".into(),
        node_id: "no-such-node".into(),
        rect: Rect::default(),
        tooltip: None,
        scroll_offset_y: 0.0,
    });
    assert_eq!(state.cursor(&tree), Cursor::Default);
}
