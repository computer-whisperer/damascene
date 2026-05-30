use super::support::*;

#[test]
fn sync_focus_order_preserves_existing_focus_by_node_id() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
    state.focus_next();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));
    state.focus_next();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));

    let (rebuilt, _) = lay_out_counter();
    state.sync_focus_order(&rebuilt);
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
}
#[test]
fn stale_focus_clears_on_rebuild() {
    let (tree, mut state) = lay_out_counter();
    state.focused = Some(UiTarget {
        key: "gone".into(),
        node_id: "root.missing".into(),
        rect: Rect::default(),
        tooltip: None,
        scroll_offset_y: 0.0,
    });

    state.sync_focus_order(&tree);

    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
}

#[test]
fn set_focus_none_clears_existing_focus() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    state.focus_next();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));

    state.set_focus(None);

    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
}

#[test]
fn push_focus_requests_resolves_known_key() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);

    state.push_focus_requests(vec!["inc".into()]);
    state.drain_focus_requests();

    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
}

#[test]
fn drain_focus_requests_drops_unknown_keys_silently() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    state.focus_next();
    let before = state.focused.as_ref().map(|t| t.key.clone());

    state.push_focus_requests(vec!["does-not-exist".into()]);
    state.drain_focus_requests();

    assert_eq!(state.focused.as_ref().map(|t| t.key.clone()), before);
    assert!(state.focus.pending_requests.is_empty());
}

#[test]
fn drain_focus_requests_last_match_wins() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);

    state.push_focus_requests(vec!["dec".into(), "missing".into(), "inc".into()]);
    state.drain_focus_requests();

    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
}

#[test]
fn drain_focus_requests_clears_queue_each_call() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);

    state.push_focus_requests(vec!["dec".into()]);
    state.drain_focus_requests();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));

    // a second drain with no new requests must be a no-op and must
    // not re-apply the prior request.
    state.focus_next();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
    state.drain_focus_requests();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
}
