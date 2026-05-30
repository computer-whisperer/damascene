use super::support::*;

#[test]
fn sync_selection_order_collects_keyed_selectable_leaves_in_tree_order() {
    let mut tree = column([
        crate::text("Alpha").key("a").selectable(),
        crate::text("Bravo (not selectable)"),
        crate::text("Charlie").key("c").selectable(),
        crate::text("Delta (selectable but unkeyed)").selectable(),
    ])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.sync_selection_order(&tree);

    let order = state.selection_order();
    let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
    // Only the keyed-and-selectable leaves should appear, in tree
    // order. The unkeyed selectable leaf is silently excluded —
    // selection requires stable identity.
    assert_eq!(keys, vec!["a", "c"]);
}
