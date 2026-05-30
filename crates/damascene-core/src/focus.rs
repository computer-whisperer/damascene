//! Linear focus traversal — collects the focusable keyed nodes into the
//! order Tab/Shift-Tab walks. Ancestors with `clip` shrink the visible
//! rect so a focusable that's been scrolled out of view is dropped.
//!
//! Reads computed rects from `UiState`'s layout side map; the tree
//! itself only carries identity (`computed_id`).

use crate::event::UiTarget;
use crate::state::UiState;
use crate::tree::{El, Kind, Rect};

/// Find the focusable siblings inside the focused element's nearest
/// `arrow_nav_siblings` parent, returning them in tree order (so an
/// arrow-key handler can index them directly). Returns `None` when no
/// such parent contains the focused element — that's the signal that
/// arrow keys should fall through to the default `KeyDown` path.
///
/// Sibling collection mirrors [`focus_order`]: only `focusable` keyed
/// nodes that survive the inherited clip are included. The returned
/// list always contains the currently-focused element when one
/// matches; callers locate it by `node_id` to compute next / prev /
/// first / last.
pub fn arrow_nav_group(root: &El, ui_state: &UiState, focused_id: &str) -> Option<Vec<UiTarget>> {
    find_group(root, ui_state, None, focused_id)
}

fn find_group(
    node: &El,
    ui_state: &UiState,
    inherited_clip: Option<Rect>,
    focused_id: &str,
) -> Option<Vec<UiTarget>> {
    let computed = ui_state.rect(&node.computed_id);
    let clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(computed),
        }
    } else {
        inherited_clip
    };

    // If this node is an arrow-navigable parent, check whether the
    // focused element is one of its direct children. If so, this is
    // the group to return — collect its focusable siblings.
    if node.arrow_nav_siblings && node.children.iter().any(|c| c.computed_id == focused_id) {
        let mut siblings: Vec<UiTarget> = Vec::new();
        for child in &node.children {
            collect_focusable_self(child, ui_state, clip, &mut siblings);
        }
        return Some(siblings);
    }

    // Otherwise, recurse — the focused element might be inside a
    // deeper arrow-navigable group.
    for child in &node.children {
        if let Some(group) = find_group(child, ui_state, clip, focused_id) {
            return Some(group);
        }
    }
    None
}

/// Append `node`'s [`UiTarget`] if it's focusable, keyed, and inside
/// the visible clip. Mirrors the per-node rule used by [`focus_order`]
/// without recursing into descendants — the arrow-nav group is
/// strictly the immediate children of the navigable parent.
fn collect_focusable_self(
    node: &El,
    ui_state: &UiState,
    clip: Option<Rect>,
    out: &mut Vec<UiTarget>,
) {
    let computed = ui_state.rect(&node.computed_id);
    if node.focusable
        && let Some(key) = &node.key
        && clip
            .map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    {
        out.push(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: computed,
            tooltip: node.tooltip.clone(),
            scroll_offset_y: 0.0,
        });
    }
}

/// Collect focusable, keyed nodes in tree order (Tab walks forward,
/// Shift-Tab walks backward). Nodes outside their inherited clip are
/// skipped.
pub fn focus_order(root: &El, ui_state: &UiState) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_focus(root, ui_state, None, &mut out);
    out
}

/// Collect selectable, keyed nodes in document (tree) order. Same
/// clip rules as [`focus_order`]: nodes outside their inherited clip
/// are skipped. The selection manager indexes into this list to
/// resolve pointer hits against keys and to walk cross-element
/// selections in document order.
pub fn selection_order(root: &El, ui_state: &UiState) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_selectable(root, ui_state, None, &mut out);
    out
}

fn collect_selectable(
    node: &El,
    ui_state: &UiState,
    inherited_clip: Option<Rect>,
    out: &mut Vec<UiTarget>,
) {
    let computed = ui_state.rect(&node.computed_id);
    let clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(computed),
        }
    } else {
        inherited_clip
    };
    if node.selectable
        && let Some(key) = &node.key
        && clip
            .map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    {
        out.push(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: computed,
            tooltip: node.tooltip.clone(),
            scroll_offset_y: 0.0,
        });
    }
    for child in &node.children {
        collect_selectable(child, ui_state, clip, out);
    }
}

fn collect_focus(
    node: &El,
    ui_state: &UiState,
    inherited_clip: Option<Rect>,
    out: &mut Vec<UiTarget>,
) {
    let computed = ui_state.rect(&node.computed_id);
    let clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(computed),
        }
    } else {
        inherited_clip
    };
    if node.focusable
        && let Some(key) = &node.key
        && clip
            .map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    {
        out.push(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: computed,
            tooltip: node.tooltip.clone(),
            scroll_offset_y: 0.0,
        });
    }
    for child in &node.children {
        collect_focus(child, ui_state, clip, out);
    }
}

/// Reconcile the focus stack against the `popover_layer` nodes in
/// `root`. Detects open / close transitions by diffing against the
/// previous frame's set of layer ids:
///
/// - **Layer opened** (id present now, absent before): snapshot the
///   current focus onto the focus stack and auto-focus the first
///   focusable inside the new layer.
/// - **Layer closed** (id absent now, present before): pop the stack.
///   Restore the saved focus only when no other focus is currently set
///   — typically the case after Escape / dismiss-scrim, where the
///   element inside the layer ceased to exist. If focus moved
///   intentionally elsewhere first (e.g. user clicked another widget),
///   the saved entry is discarded so we don't yank focus back.
///
/// Must run after [`UiState::sync_focus_order`] so focus has already
/// been retargeted / cleared against the new tree.
pub fn sync_popover_focus(root: &El, ui_state: &mut UiState) {
    let new_layers = collect_popover_layer_ids(root);
    let old_layers = std::mem::take(&mut ui_state.popover_focus.layer_ids);

    // Process closes first, in reverse tree order (innermost first), so
    // a same-frame close-then-reopen of a deeper layer pops the right
    // saved focus before pushing the new one.
    for id in old_layers.iter().rev() {
        if !new_layers.contains(id) {
            let saved = ui_state.popover_focus.focus_stack.pop();
            if ui_state.focused.is_none()
                && let Some(target) = saved
                && ui_state
                    .focus
                    .order
                    .iter()
                    .any(|t| t.node_id == target.node_id)
            {
                ui_state.focused = Some(target);
            }
        }
    }

    // Then process opens in tree order so nested layers stack their
    // saved focus correctly (outer layer's pre-open focus pushed first).
    for id in &new_layers {
        if !old_layers.contains(id) {
            if let Some(current) = ui_state.focused.clone() {
                ui_state.popover_focus.focus_stack.push(current);
            }
            if let Some(first) = first_focusable_in(root, id, ui_state) {
                ui_state.focused = Some(first);
            }
        }
    }

    ui_state.popover_focus.layer_ids = new_layers;
}

/// Collect the `computed_id` of every `Kind::Custom("popover_layer")`
/// node in `root`, in tree order.
fn collect_popover_layer_ids(root: &El) -> Vec<String> {
    let mut out = Vec::new();
    walk_popover_layers(root, &mut out);
    out
}

fn walk_popover_layers(node: &El, out: &mut Vec<String>) {
    if matches!(node.kind, Kind::Custom("popover_layer")) {
        out.push(node.computed_id.clone());
    }
    for child in &node.children {
        walk_popover_layers(child, out);
    }
}

/// Find the first focusable, keyed node inside the subtree rooted at
/// the node whose `computed_id == layer_id`. Uses the same clip-aware
/// rule as [`focus_order`].
fn first_focusable_in(root: &El, layer_id: &str, ui_state: &UiState) -> Option<UiTarget> {
    let (subtree, inherited_clip) = locate_subtree(root, ui_state, None, layer_id)?;
    let mut out = Vec::new();
    collect_focus(subtree, ui_state, inherited_clip, &mut out);
    out.into_iter().next()
}

/// Walk to the node with `target_id`, returning that node and the clip
/// rect inherited from its ancestors (so the caller can resume the
/// usual clip-aware focus walk).
fn locate_subtree<'a>(
    node: &'a El,
    ui_state: &UiState,
    inherited_clip: Option<Rect>,
    target_id: &str,
) -> Option<(&'a El, Option<Rect>)> {
    let computed = ui_state.rect(&node.computed_id);
    let clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(computed),
        }
    } else {
        inherited_clip
    };
    if node.computed_id == target_id {
        return Some((node, clip));
    }
    for child in &node.children {
        if let Some(found) = locate_subtree(child, ui_state, clip, target_id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::tree::*;
    use crate::{button, column, row};

    #[test]
    fn focus_order_collects_keyed_focusable_nodes() {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let order = focus_order(&tree, &state);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["dec", "inc"]);
    }
}
