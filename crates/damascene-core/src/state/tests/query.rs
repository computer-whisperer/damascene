use super::support::*;

#[test]
fn rect_of_key_finds_laid_out_node_rect() {
    let (tree, state) = lay_out_counter();
    let inc_by_helper = find_rect(&tree, &state, "inc").expect("inc rect");
    assert_eq!(state.rect_of_key(&tree, "inc"), Some(inc_by_helper));
    assert_eq!(state.rect_of_key(&tree, "missing"), None);
}

#[test]
fn target_of_key_carries_key_id_and_rect() {
    let (tree, state) = lay_out_counter();
    let target = state.target_of_key(&tree, "dec").expect("dec target");
    assert_eq!(target.key, "dec");
    assert_eq!(target.node_id, find_id(&tree, "dec").expect("dec id"));
    assert_eq!(
        target.rect,
        find_rect(&tree, &state, "dec").expect("dec rect")
    );
}
