//! Linear focus traversal — collects the focusable keyed nodes into the
//! order Tab/Shift-Tab walks. Ancestors with `clip` shrink the visible
//! rect so a focusable that's been scrolled out of view is dropped.
//!
//! Reads computed rects from `UiState`'s layout side map; the tree
//! itself only carries identity (`computed_id`).

use crate::event::UiTarget;
use crate::state::UiState;
use crate::tree::{ArrowNav, El, Kind, Rect};

/// Fold `node`'s own clip into the clip rect inherited from its
/// ancestors. Every focus/selection walk in this module applies the
/// same rule: a clipping node intersects (or, when disjoint, empties)
/// the inherited rect; a non-clipping node passes it through.
fn fold_clip(node: &El, inherited: Option<Rect>) -> Option<Rect> {
    if node.clip {
        Some(match inherited {
            Some(clip) => clip
                .intersect(node.computed_rect)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            None => node.computed_rect,
        })
    } else {
        inherited
    }
}

/// The clip that decides *focus membership*: like [`fold_clip`], but
/// a scrollable's clip is not folded in. Content below the fold of a
/// `scroll()` — or of any node with `.scrollable()`, such as the stock
/// menu panels — is hidden only until it is scrolled to, so it stays
/// in the Tab order and in arrow-nav groups, and the runtime scrolls
/// it into view when keyboard focus lands there (issue #149 — the
/// `overflow: auto` rule in browsers). Every other `.clip()` —
/// overflow-hidden wrappers, and a `viewport()` whose children are
/// panned out of frame — remains a hard clip: what it hides is not
/// reachable and must not take focus.
fn fold_focus_clip(node: &El, inherited: Option<Rect>) -> Option<Rect> {
    if node.layout_pruned {
        // Layout skipped this scroll child as far outside the window:
        // its descendants carry zero-size placeholder rects, so their
        // membership can't be judged by geometry — and they *are*
        // reachable by scrolling, whatever hard clip sits above the
        // scroll. Include them; once focus lands there the reveal
        // scrolls, layout runs for real, and the next sync judges them
        // properly.
        None
    } else if node.scrollable {
        // Everything inside a visible scrollable is reachable by
        // scrolling, whatever hard clip sits above it — so the clip
        // resets here rather than merely not folding the scroll's own.
        // A scroll that is itself entirely clipped away keeps the
        // inherited clip: nothing inside it can be revealed.
        match inherited {
            Some(clip) if clip.intersect(node.computed_rect).is_none() => Some(clip),
            _ => None,
        }
    } else {
        fold_clip(node, inherited)
    }
}

/// Find the focusable group members inside the focused element's
/// nearest [`El::arrow_nav`] parent, returning the group's mode and
/// its members in tree order (so an arrow-key handler can index them
/// directly). Returns `None` when no such parent contains the focused
/// element — that's the signal that arrow keys should fall through to
/// the default `KeyDown` path.
///
/// Membership mirrors [`focus_order`]: only `focusable` keyed nodes
/// that survive the inherited clip are included (a scroll container's
/// clip doesn't count — see `fold_focus_clip`). The linear modes
/// collect the flagged node's direct children; [`ArrowNav::Grid`]
/// collects all focusable descendants, because grid cells live inside
/// intermediate row containers (`calendar_month`'s week rows). The
/// returned list always contains the currently-focused element when
/// one matches; callers locate it by `node_id` to compute next / prev
/// / first / last.
pub fn arrow_nav_group(root: &El, focused_id: &str) -> Option<(ArrowNav, Vec<UiTarget>)> {
    find_group(root, None, focused_id)
}

fn find_group(
    node: &El,
    inherited_clip: Option<Rect>,
    focused_id: &str,
) -> Option<(ArrowNav, Vec<UiTarget>)> {
    let clip = fold_focus_clip(node, inherited_clip);

    // If this node is an arrow-navigable parent, check whether the
    // focused element is a member. If so, this is the group to return
    // — collect its focusable members.
    if let Some(mode) = node.arrow_nav {
        let mut members: Vec<UiTarget> = Vec::new();
        if mode == ArrowNav::Grid {
            for child in &node.children {
                collect_focusable_descendants(child, clip, &mut members);
            }
        } else {
            for child in &node.children {
                collect_focusable_self(child, clip, &mut members);
            }
        }
        if members.iter().any(|t| t.node_id == focused_id.into()) {
            return Some((mode, members));
        }
        // Fall through: the focused element may be inside a nested
        // group deeper in this subtree (e.g. a popover opened from a
        // grid cell).
    }

    // Otherwise, recurse — the focused element might be inside a
    // deeper arrow-navigable group.
    for child in &node.children {
        if let Some(group) = find_group(child, clip, focused_id) {
            return Some(group);
        }
    }
    None
}

/// Recursive variant of [`collect_focusable_self`] for
/// [`ArrowNav::Grid`] groups: appends every focusable keyed descendant
/// in tree order, applying the same clip rules as [`focus_order`].
fn collect_focusable_descendants(node: &El, inherited_clip: Option<Rect>, out: &mut Vec<UiTarget>) {
    let clip = fold_focus_clip(node, inherited_clip);
    collect_focusable_self(node, clip, out);
    for child in &node.children {
        collect_focusable_descendants(child, clip, out);
    }
}

/// Append `node`'s [`UiTarget`] if it's focusable, keyed, and inside
/// the visible clip. Mirrors the per-node rule used by [`focus_order`]
/// without recursing into descendants — the arrow-nav group is
/// strictly the immediate children of the navigable parent.
fn collect_focusable_self(node: &El, clip: Option<Rect>, out: &mut Vec<UiTarget>) {
    let computed = node.computed_rect;
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
/// skipped — except that a scroll container's clip doesn't count
/// (content below the fold is reachable, and keyboard focus scrolls it
/// into view; see `fold_focus_clip`).
pub fn focus_order(root: &El) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_focus(root, None, &mut out);
    out
}

/// Collect selectable, keyed nodes in document (tree) order. Nodes
/// outside their inherited clip are skipped — every clip counts here,
/// including scroll containers (unlike [`focus_order`]). The selection manager indexes into this list to
/// resolve pointer hits against keys and to walk cross-element
/// selections in document order.
pub fn selection_order(root: &El) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_selectable(root, None, &mut out);
    out
}

/// Collect the focus and selection orders in a single tree walk —
/// same per-node rules as [`focus_order`] and [`selection_order`],
/// fused because traversal (and the per-node rect probe) dominates on
/// large trees. Production path for the per-frame sync; the split
/// entry points above remain for arrow-nav groups and tests.
pub fn focus_and_selection_order(root: &El) -> (Vec<UiTarget>, Vec<UiTarget>) {
    let mut focus = Vec::new();
    let mut selection = Vec::new();
    collect_orders(root, (None, None), &mut focus, &mut selection);
    (focus, selection)
}

fn collect_orders(
    node: &El,
    inherited: (Option<Rect>, Option<Rect>),
    focus: &mut Vec<UiTarget>,
    selection: &mut Vec<UiTarget>,
) {
    let computed = node.computed_rect;
    // Two clips: focus membership ignores scroll containers, selection
    // order doesn't (see `fold_focus_clip`).
    let focus_clip = fold_focus_clip(node, inherited.0);
    let sel_clip = fold_clip(node, inherited.1);
    let inside = |clip: Option<Rect>| {
        clip.map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    };
    if let Some(key) = &node.key {
        let wants_focus = node.focusable && inside(focus_clip);
        let wants_selection = node.selectable && inside(sel_clip);
        if wants_focus || wants_selection {
            let target = UiTarget {
                key: key.clone(),
                node_id: node.computed_id.clone(),
                rect: computed,
                tooltip: node.tooltip.clone(),
                scroll_offset_y: 0.0,
            };
            if wants_selection {
                selection.push(target.clone());
            }
            if wants_focus {
                focus.push(target);
            }
        }
    }
    for child in &node.children {
        collect_orders(child, (focus_clip, sel_clip), focus, selection);
    }
}

fn collect_selectable(node: &El, inherited_clip: Option<Rect>, out: &mut Vec<UiTarget>) {
    let computed = node.computed_rect;
    let clip = fold_clip(node, inherited_clip);
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
        collect_selectable(child, clip, out);
    }
}

fn collect_focus(node: &El, inherited_clip: Option<Rect>, out: &mut Vec<UiTarget>) {
    let computed = node.computed_rect;
    let clip = fold_focus_clip(node, inherited_clip);
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
        collect_focus(child, clip, out);
    }
}

/// Which flavour of floating layer a node is, for focus purposes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LayerKind {
    /// Non-blocking floating layer — `Kind::Custom("popover_layer")`,
    /// produced by the popover/dropdown/menu widgets. Gets the focus
    /// lifecycle but does not trap Tab (browsers don't either: Tab can
    /// walk out from under an open menu).
    Popover,
    /// Blocking layer behind a scrim — [`Kind::Modal`] panels
    /// (`modal()` / `dialog()`) and sheet content. Gets the focus
    /// lifecycle *and* scopes Tab traversal to itself while open,
    /// making everything behind the scrim inert (HTML's
    /// `<dialog showModal>` model).
    Blocking,
}

/// Reconcile the focus stack against the floating layers in `root` —
/// popover layers, [`Kind::Modal`] panels, and sheets. Detects open /
/// close transitions by diffing against the previous frame's set of
/// layer ids:
///
/// - **Layer opened** (id present now, absent before): snapshot the
///   current focus onto the focus stack and auto-focus into the new
///   layer — its first `autofocus`-flagged focusable descendant, or
///   its first focusable when none is flagged (HTML `<dialog>`'s
///   focusing rule).
/// - **Layer closed** (id absent now, present before): pop the stack.
///   Restore the saved focus only when no other focus is currently set
///   — typically the case after Escape / dismiss-scrim, where the
///   element inside the layer ceased to exist. If focus moved
///   intentionally elsewhere first (e.g. user clicked another widget),
///   the saved entry is discarded so we don't yank focus back.
///
/// While any blocking layer is open, the Tab order
/// (`ui_state.focus.order`) is additionally scoped to the topmost
/// blocking layer's subtree plus any floating layers stacked above it,
/// so Tab wraps inside the modal instead of walking into controls
/// behind the scrim. Explicit focus requests resolve against the same
/// scoped order, so a request targeting an inert widget is dropped.
///
/// Must run after [`UiState::sync_focus_order`] so focus has already
/// been retargeted / cleared against the new tree.
pub fn sync_layer_focus(root: &El, ui_state: &mut UiState) {
    let new_layers = collect_focus_layers(root);
    let old_ids = std::mem::take(&mut ui_state.layer_focus.layer_ids);

    // Process closes first, in reverse tree order (innermost first), so
    // a same-frame close-then-reopen of a deeper layer restores the
    // right saved focus before saving the new one. Each stack entry is
    // keyed by its layer id, so a close always consumes its own
    // layer's entry regardless of the order layers opened in.
    for id in old_ids.iter().rev() {
        if !new_layers.iter().any(|(new_id, _)| new_id == id) {
            let saved = ui_state
                .layer_focus
                .focus_stack
                .iter()
                .rposition(|(saved_id, _)| saved_id == id)
                .and_then(|pos| ui_state.layer_focus.focus_stack.remove(pos).1);
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

    // Then process opens in tree order so stacked layers save their
    // pre-open focus correctly (outer layer's entry pushed first). An
    // open always pushes an entry — with `None` when nothing was
    // focused — so the close path above always finds one to consume.
    for (id, _) in &new_layers {
        if !old_ids.contains(id) {
            ui_state
                .layer_focus
                .focus_stack
                .push((id.clone(), ui_state.focused.clone()));
            if let Some(first) = auto_focus_target_in(root, id) {
                ui_state.focused = Some(first);
            }
        }
    }

    ui_state.layer_focus.layer_ids = new_layers.iter().map(|(id, _)| id.clone()).collect();

    // Tab trapping: while a blocking layer is open, everything behind
    // it is inert. Scope the focus order to the topmost blocking
    // layer's subtree plus every layer stacked above it in tree order
    // (a dropdown opened from inside a modal is composed as a later
    // sibling layer, not a descendant — it must stay reachable, like
    // HTML's top layer). Dedup guards the descendant case, where a
    // nested layer's targets were already collected with its parent.
    if let Some(cut) = new_layers
        .iter()
        .rposition(|(_, kind)| *kind == LayerKind::Blocking)
    {
        let mut scoped = Vec::new();
        let mut focused_in_scope = false;
        let focused_id = ui_state.focused.as_ref().map(|t| t.node_id.clone());
        for (id, _) in &new_layers[cut..] {
            if let Some((subtree, inherited_clip)) = locate_subtree(root, None, id) {
                collect_focus(subtree, inherited_clip, &mut scoped);
                if let Some(focused_id) = &focused_id
                    && !focused_in_scope
                    && locate_subtree(subtree, None, focused_id).is_some()
                {
                    focused_in_scope = true;
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        scoped.retain(|t| seen.insert(t.node_id.clone()));
        ui_state.focus.order = scoped;

        // Inertness must also cover the already-focused target: if the
        // blocking layer has no focusables (or focus was left behind
        // the scrim), keyboard events would keep routing to a control
        // the user can no longer see or click — Enter would activate
        // behind the scrim. Blur unless the focused NODE lives inside
        // the scope. Membership is by existence in the scope subtree,
        // not by presence in the scoped order, so a focused widget
        // that merely scrolled out of the modal body's clip keeps
        // focus (the soft-keyboard rule in `sync_focus_order`).
        if focused_id.is_some() && !focused_in_scope {
            ui_state.focused = None;
        }
    }
}

/// Collect `(computed_id, kind)` of every focus-lifecycle layer in
/// `root`, in tree order: popover layers, [`Kind::Modal`] panels, and
/// `sheet_content` panels.
fn collect_focus_layers(root: &El) -> Vec<(String, LayerKind)> {
    let mut out = Vec::new();
    walk_focus_layers(root, &mut out);
    out
}

fn walk_focus_layers(node: &El, out: &mut Vec<(String, LayerKind)>) {
    let kind = match node.kind {
        Kind::Custom("popover_layer") => Some(LayerKind::Popover),
        Kind::Modal | Kind::Custom("sheet_content") => Some(LayerKind::Blocking),
        _ => None,
    };
    if let Some(kind) = kind {
        out.push((node.computed_id.to_string(), kind));
    }
    for child in &node.children {
        walk_focus_layers(child, out);
    }
}

/// Pick the auto-focus target inside the subtree rooted at the node
/// whose `computed_id == layer_id`: the first focusable, keyed node
/// flagged [`El::autofocus`], or the subtree's first focusable when
/// none is flagged. Uses the same clip-aware rule as [`focus_order`].
fn auto_focus_target_in(root: &El, layer_id: &str) -> Option<UiTarget> {
    let (subtree, inherited_clip) = locate_subtree(root, None, layer_id)?;
    let mut out = Vec::new();
    collect_focus_autofocus(subtree, inherited_clip, &mut out);
    match out.iter().position(|(_, autofocus)| *autofocus) {
        Some(i) => Some(out.swap_remove(i).0),
        None => out.into_iter().next().map(|(target, _)| target),
    }
}

/// [`collect_focus`] variant that also records each target's
/// [`El::autofocus`] flag, for the layer-open focusing rule.
fn collect_focus_autofocus(
    node: &El,
    inherited_clip: Option<Rect>,
    out: &mut Vec<(UiTarget, bool)>,
) {
    let computed = node.computed_rect;
    let clip = fold_focus_clip(node, inherited_clip);
    if node.focusable
        && let Some(key) = &node.key
        && clip
            .map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    {
        out.push((
            UiTarget {
                key: key.clone(),
                node_id: node.computed_id.clone(),
                rect: computed,
                tooltip: node.tooltip.clone(),
                scroll_offset_y: 0.0,
            },
            node.autofocus,
        ));
    }
    for child in &node.children {
        collect_focus_autofocus(child, clip, out);
    }
}

/// Walk to the node with `target_id`, returning that node and the clip
/// rect inherited from its ancestors (so the caller can resume the
/// usual clip-aware focus walk).
fn locate_subtree<'a>(
    node: &'a El,
    inherited_clip: Option<Rect>,
    target_id: &str,
) -> Option<(&'a El, Option<Rect>)> {
    let clip = fold_focus_clip(node, inherited_clip);
    if node.computed_id == target_id.into() {
        return Some((node, clip));
    }
    for child in &node.children {
        if let Some(found) = locate_subtree(child, clip, target_id) {
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

        let order = focus_order(&tree);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["dec", "inc"]);
    }

    /// A 100×50 clipped column holding two 60px buttons: the second
    /// sits entirely below the clip.
    fn clipped_panel(scrollable: bool) -> El {
        let panel = El::new(Kind::Custom("panel"))
            .axis(Axis::Column)
            .children([
                button("a").key("a").height(Size::Fixed(60.0)),
                button("b").key("b").height(Size::Fixed(60.0)),
            ])
            .width(Size::Fixed(100.0))
            .height(Size::Fixed(50.0))
            .clip();
        if scrollable {
            panel.scrollable()
        } else {
            panel
        }
    }

    #[test]
    fn any_scrollable_clip_keeps_its_hidden_focusables_in_the_order() {
        // Scroll semantics are a per-node flag, not `Kind::Scroll`: the
        // stock menu panels are `Kind::Custom` + `.scrollable()`, and
        // their below-the-fold rows must stay keyboard-reachable.
        for (scrollable, expected) in [(false, vec!["a"]), (true, vec!["a", "b"])] {
            // The root always takes the whole viewport, so the clipped
            // panel must be a child for its Fixed height to apply.
            let mut tree = column([clipped_panel(scrollable)]);
            let mut state = UiState::new();
            layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
            let order = focus_order(&tree);
            let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
            assert_eq!(keys, expected, "scrollable = {scrollable}");
        }
    }
}
