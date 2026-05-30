#![allow(unused_imports)]

pub(crate) use super::super::*;
pub(crate) use crate::cursor::Cursor;
pub(crate) use crate::event::{KeyChord, UiEventKind, UiKey};
pub(crate) use crate::hit_test::hit_test;
pub(crate) use crate::layout::{assign_ids, layout};
pub(crate) use crate::palette::Palette;
pub(crate) use crate::tree::*;
pub(crate) use crate::{button, column, row, scroll};
pub(crate) use web_time::Instant;

pub(crate) fn lay_out_counter() -> (El, UiState) {
    let mut tree = column([
        crate::text("0"),
        row([button("-").key("dec"), button("+").key("inc")]),
    ])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    (tree, state)
}
pub(crate) fn find_id_for_kind(node: &El, key: &str) -> Option<String> {
    if matches!(node.kind, Kind::Scroll) && node.key.as_deref() == Some(key) {
        return Some(node.computed_id.clone());
    }
    node.children.iter().find_map(|c| find_id_for_kind(c, key))
}
pub(crate) fn find_fill(node: &El, key: &str) -> Option<Color> {
    if node.key.as_deref() == Some(key) {
        return node.fill;
    }
    node.children.iter().find_map(|c| find_fill(c, key))
}
pub(crate) fn envelope_for(
    node: &El,
    state: &UiState,
    key: &str,
    kind: EnvelopeKind,
) -> Option<f32> {
    if node.key.as_deref() == Some(key) {
        return Some(state.envelope(&node.computed_id, kind));
    }
    node.children
        .iter()
        .find_map(|c| envelope_for(c, state, key, kind))
}
pub(crate) fn find_node<'a>(node: &'a El, key: &str) -> Option<&'a El> {
    if node.key.as_deref() == Some(key) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_node(c, key))
}
pub(crate) fn find_rect(node: &El, state: &UiState, key: &str) -> Option<Rect> {
    if node.key.as_deref() == Some(key) {
        return Some(state.rect(&node.computed_id));
    }
    node.children.iter().find_map(|c| find_rect(c, state, key))
}
pub(crate) fn node_state(node: &El, state: &UiState, key: &str) -> InteractionState {
    let mut found = None;
    find_node_state(node, state, key, &mut found);
    found.unwrap_or_default()
}
pub(crate) fn find_node_state(
    node: &El,
    state: &UiState,
    key: &str,
    out: &mut Option<InteractionState>,
) {
    if node.key.as_deref() == Some(key) {
        *out = Some(state.node_state(&node.computed_id));
        return;
    }
    for c in &node.children {
        find_node_state(c, state, key, out);
        if out.is_some() {
            return;
        }
    }
}
pub(crate) fn target(node: &El, state: &UiState, key: &str) -> UiTarget {
    let rect = find_rect(node, state, key).expect("target rect");
    UiTarget {
        key: key.to_string(),
        node_id: find_id(node, key).expect("target id"),
        rect,
        tooltip: None,
        scroll_offset_y: 0.0,
    }
}
pub(crate) fn find_id(node: &El, key: &str) -> Option<String> {
    if node.key.as_deref() == Some(key) {
        return Some(node.computed_id.clone());
    }
    node.children.iter().find_map(|c| find_id(c, key))
}
