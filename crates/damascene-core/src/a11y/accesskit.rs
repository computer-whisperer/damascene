//! Lowering of the laid-out [`El`] tree into an AccessKit
//! [`TreeUpdate`](::accesskit::TreeUpdate), plus the id interning that
//! lets assistive-technology [`ActionRequest`](::accesskit::ActionRequest)s
//! route back to elements. Compiled only with the `accessibility`
//! feature; hosts reach it through
//! [`RunnerCore::accessibility_tree_update`] /
//! [`RunnerCore::accessibility_action`] rather than calling in here
//! directly.
//!
//! Shape: a full tree is emitted on every call (AccessKit adapters
//! diff internally; this is the egui-proven pattern), and only while
//! the host reports an assistive technology actually connected, so
//! idle cost is zero. Structural containers with no semantic
//! contribution are *hoisted* — their children attach to the nearest
//! emitted ancestor — which keeps the platform tree close to what a
//! screen-reader user expects instead of mirroring layout nesting.
//!
//! Known v1 limits (tracked in `docs/ACCESSIBILITY_PLAN.md`): bounds
//! come from layout rects and don't yet subtract enclosing scroll
//! offsets, and text inputs expose role/name/value only — the
//! character-level AccessKit text protocol is a later arc.
//!
//! [`El`]: crate::tree::El
//! [`RunnerCore::accessibility_tree_update`]: crate::runtime::RunnerCore::accessibility_tree_update
//! [`RunnerCore::accessibility_action`]: crate::runtime::RunnerCore::accessibility_action

use std::sync::Arc;

use ::accesskit as ak;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::a11y::{LiveRegion, Role};
use crate::state::UiState;
use crate::tree::El;

/// Interning table between damascene `computed_id`s (path-shaped
/// strings, stable while a node lives) and the `u64` [`ak::NodeId`]s
/// AccessKit speaks. Owned by the runner so ids stay stable across
/// frames; entries whose nodes left the tree are pruned on each
/// emission, so a stale id in a late-arriving action request simply
/// fails to resolve (a no-op, the right outcome).
#[derive(Default)]
pub struct AccessKitIds {
    forward: FxHashMap<Arc<str>, u64>,
    reverse: FxHashMap<u64, Arc<str>>,
    next: u64,
}

impl AccessKitIds {
    fn id_for(&mut self, computed_id: &Arc<str>) -> ak::NodeId {
        if let Some(&v) = self.forward.get(computed_id) {
            return ak::NodeId(v);
        }
        let v = self.next;
        self.next += 1;
        self.forward.insert(computed_id.clone(), v);
        self.reverse.insert(v, computed_id.clone());
        ak::NodeId(v)
    }

    /// The `computed_id` a platform [`ak::NodeId`] refers to, if it is
    /// still (or was recently) in the tree.
    pub fn computed_id_for(&self, id: ak::NodeId) -> Option<&Arc<str>> {
        self.reverse.get(&id.0)
    }

    fn retain_live(&mut self, live: &FxHashSet<Arc<str>>) {
        self.forward.retain(|cid, _| live.contains(cid));
        let forward = &self.forward;
        self.reverse.retain(|_, cid| forward.contains_key(cid));
    }
}

/// Roles whose accessible name derives from their text content when no
/// `aria_label` override is set — the HTML accname
/// name-from-content set. Text leaves inside such a node are absorbed
/// into the name instead of emitted as separate static-text nodes, so
/// a `button("Save")` announces once.
fn names_from_content(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::Checkbox
            | Role::Switch
            | Role::Radio
            | Role::Tab
            | Role::MenuItem
            | Role::MenuItemCheckbox
            | Role::MenuItemRadio
            | Role::Option
            | Role::Link
            | Role::Heading
            | Role::Tooltip
            | Role::Cell
            | Role::ColumnHeader
            | Role::GridCell
            | Role::ListItem
            | Role::Alert
            | Role::Status
            | Role::Paragraph
    )
}

fn map_role(role: Role) -> ak::Role {
    match role {
        Role::Button => ak::Role::Button,
        Role::Checkbox => ak::Role::CheckBox,
        Role::Switch => ak::Role::Switch,
        Role::Radio => ak::Role::RadioButton,
        Role::RadioGroup => ak::Role::RadioGroup,
        Role::Slider => ak::Role::Slider,
        Role::SpinButton => ak::Role::SpinButton,
        Role::Textbox => ak::Role::TextInput,
        Role::Tab => ak::Role::Tab,
        Role::TabList => ak::Role::TabList,
        Role::TabPanel => ak::Role::TabPanel,
        Role::Menu => ak::Role::Menu,
        Role::MenuBar => ak::Role::MenuBar,
        Role::MenuItem => ak::Role::MenuItem,
        Role::MenuItemCheckbox => ak::Role::MenuItemCheckBox,
        Role::MenuItemRadio => ak::Role::MenuItemRadio,
        Role::Listbox => ak::Role::ListBox,
        Role::Option => ak::Role::ListBoxOption,
        Role::Link => ak::Role::Link,
        Role::Heading => ak::Role::Heading,
        Role::Img => ak::Role::Image,
        Role::Group => ak::Role::Group,
        Role::Dialog => ak::Role::Dialog,
        Role::AlertDialog => ak::Role::AlertDialog,
        Role::Alert => ak::Role::Alert,
        Role::Status => ak::Role::Status,
        Role::Log => ak::Role::Log,
        Role::ProgressBar => ak::Role::ProgressIndicator,
        Role::Tooltip => ak::Role::Tooltip,
        Role::List => ak::Role::List,
        Role::ListItem => ak::Role::ListItem,
        Role::Table => ak::Role::Table,
        Role::Row => ak::Role::Row,
        Role::Cell => ak::Role::Cell,
        Role::ColumnHeader => ak::Role::ColumnHeader,
        Role::Combobox => ak::Role::ComboBox,
        Role::Separator => ak::Role::Splitter,
        Role::Toolbar => ak::Role::Toolbar,
        Role::Grid => ak::Role::Grid,
        Role::GridCell => ak::Role::GridCell,
        Role::Figure => ak::Role::Figure,
        Role::Math => ak::Role::Math,
        Role::Paragraph => ak::Role::Paragraph,
        // Presentation strips semantics; a presentation node only gets
        // here when it is focusable, where a generic container is the
        // honest remainder.
        Role::Presentation => ak::Role::GenericContainer,
    }
}

/// Concatenate the visible text of `node` and its descendants
/// (skipping `aria_hidden` subtrees), the accname name-from-content
/// walk. Joined with single spaces; leading/trailing whitespace
/// trimmed per piece.
fn collect_text(node: &El, out: &mut String) {
    if node.a11y.as_deref().is_some_and(|p| p.hidden) {
        return;
    }
    if let Some(text) = &node.text {
        let piece = text.trim();
        if !piece.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(piece);
        }
    }
    for child in &node.children {
        collect_text(child, out);
    }
}

struct Emit<'a> {
    nodes: Vec<(ak::NodeId, ak::Node)>,
    live: FxHashSet<Arc<str>>,
    ids: &'a mut AccessKitIds,
}

/// Lower the laid-out tree into a full-tree [`ak::TreeUpdate`].
///
/// `scale_factor` maps damascene's logical pixels to the physical
/// pixels platform accessibility APIs expect; it is applied once as a
/// transform on the root node. `focused` comes from
/// [`UiState::focused`]; AccessKit requires a focus in every update,
/// falling back to the root when nothing (or something unmapped) is
/// focused.
pub fn tree_update(
    root: &El,
    ui_state: &UiState,
    scale_factor: f32,
    ids: &mut AccessKitIds,
) -> ak::TreeUpdate {
    let root_id = ids.id_for(&root.computed_id);
    let mut emit = Emit {
        nodes: Vec::new(),
        live: FxHashSet::default(),
        ids,
    };
    emit.live.insert(root.computed_id.clone());

    let mut children = Vec::new();
    for child in &root.children {
        emit_node(child, &mut children, &mut emit, false);
    }

    let mut root_node = ak::Node::new(ak::Role::Window);
    if scale_factor != 1.0 {
        root_node.set_transform(ak::Affine::scale(f64::from(scale_factor)));
    }
    root_node.set_children(children);
    emit.nodes.push((root_id, root_node));

    let focus = ui_state
        .focused
        .as_ref()
        .and_then(|t| {
            let fid = emit.ids.forward.get(&t.node_id).copied().map(ak::NodeId)?;
            emit.nodes.iter().any(|(id, _)| *id == fid).then_some(fid)
        })
        .unwrap_or(root_id);

    let Emit { nodes, live, ids } = emit;
    ids.retain_live(&live);

    ak::TreeUpdate {
        nodes,
        tree: Some(ak::Tree::new(root_id)),
        // Damascene renders one window-level tree; the reserved root
        // tree id is the single-tree convention.
        tree_id: ak::TreeId::ROOT,
        focus,
    }
}

/// Emit `node` (and subtree) into `emit`, appending the platform ids
/// of emitted roots to `parent_children`. Non-semantic structural
/// nodes are hoisted: their children attach to the nearest emitted
/// ancestor. `inside_named` marks that an ancestor absorbed text
/// content into its accessible name, so plain text leaves under it
/// stay silent.
fn emit_node(
    node: &El,
    parent_children: &mut Vec<ak::NodeId>,
    emit: &mut Emit<'_>,
    inside_named: bool,
) {
    let props = node.a11y.as_deref();
    if props.is_some_and(|p| p.hidden) {
        return;
    }

    let role = props.and_then(|p| p.role);
    // Semantic contribution: an explicit role or any ARIA prop, a
    // focusable (interactive) node, image/link content, or a visible
    // text leaf not already absorbed into an ancestor's name.
    // `Role::Presentation` strips implicit semantics: it contributes
    // only if focusable (an interactive thing is never presentational
    // to AT).
    let presentation = role == Some(Role::Presentation);
    let semantic = if presentation {
        node.focusable
    } else {
        role.is_some()
            || props.is_some_and(|p| {
                p.label.is_some()
                    || p.description.is_some()
                    || p.live.is_some()
                    || p.checked.is_some()
                    || p.expanded.is_some()
                    || p.selected.is_some()
                    || p.pressed.is_some()
                    || p.value.is_some()
                    || p.value_text.is_some()
            })
            || node.focusable
            || node.image.is_some()
            || node.text_link.is_some()
            || (node.text.is_some() && !inside_named)
    };

    if !semantic {
        for child in &node.children {
            emit_node(child, parent_children, emit, inside_named);
        }
        return;
    }

    // Resolve the platform role: explicit role wins; then content
    // facts (image → Image, link → Link, bare text → static text);
    // interactive-but-roleless is Unknown so it still surfaces (the
    // missing-role lint pushes authors to do better).
    let is_text_leaf = role.is_none() && node.image.is_none() && !node.focusable;
    let ak_role = match role {
        Some(r) => map_role(r),
        None if node.image.is_some() => ak::Role::Image,
        None if node.text_link.is_some() => ak::Role::Link,
        None if node.focusable => ak::Role::Unknown,
        None => ak::Role::Label,
    };
    let mut n = ak::Node::new(ak_role);

    let r = node.computed_rect;
    n.set_bounds(ak::Rect {
        x0: f64::from(r.x),
        y0: f64::from(r.y),
        x1: f64::from(r.x + r.w),
        y1: f64::from(r.y + r.h),
    });

    // Accessible name: explicit label, else name-from-content for the
    // roles that take one. Text leaves carry their text as the value
    // (static-text convention); textboxes carry content as value too.
    let label = props.and_then(|p| p.label.clone());
    let absorbing = label.is_none() && role.is_some_and(names_from_content);
    if let Some(label) = label {
        n.set_label(label);
    } else if absorbing {
        let mut text = String::new();
        collect_text(node, &mut text);
        if !text.is_empty() {
            n.set_label(text);
        }
    } else if is_text_leaf && let Some(text) = &node.text {
        n.set_value(text.clone());
    }
    if role == Some(Role::Textbox) {
        let mut value = String::new();
        collect_text(node, &mut value);
        n.set_value(value);
    }

    if let Some(p) = props {
        if let Some(desc) = &p.description {
            n.set_description(desc.clone());
        }
        if let Some(live) = p.live {
            n.set_live(match live {
                LiveRegion::Polite => ak::Live::Polite,
                LiveRegion::Assertive => ak::Live::Assertive,
            });
        }
        // Checked and pressed both lower to AccessKit's toggled state;
        // widgets set exactly one of them (checkables vs toggle
        // buttons).
        if let Some(toggled) = p.checked.or(p.pressed) {
            n.set_toggled(if toggled {
                ak::Toggled::True
            } else {
                ak::Toggled::False
            });
        }
        if let Some(expanded) = p.expanded {
            n.set_expanded(expanded);
        }
        if let Some(selected) = p.selected {
            n.set_selected(selected);
        }
        if p.disabled {
            n.set_disabled();
        }
        if p.modal {
            n.set_modal();
        }
        if let Some(level) = p.level {
            n.set_level(usize::from(level));
        }
        if let Some((now, min, max)) = p.value {
            n.set_numeric_value(now);
            n.set_min_numeric_value(min);
            n.set_max_numeric_value(max);
        }
        if let Some(value_text) = &p.value_text {
            n.set_value(value_text.clone());
        }
    }
    // A tooltip doubles as the description when no explicit one is set.
    if props.is_none_or(|p| p.description.is_none())
        && let Some(tooltip) = &node.tooltip
    {
        n.set_description(tooltip.clone());
    }

    let disabled = props.is_some_and(|p| p.disabled);
    if node.focusable && !disabled {
        n.add_action(ak::Action::Focus);
        // Keyed focusables are the activation contract
        // (`is_click_or_activate`); Click routes back as `Activate`.
        if node.key.is_some() {
            n.add_action(ak::Action::Click);
        }
        if props.is_some_and(|p| p.value.is_some()) {
            n.add_action(ak::Action::Increment);
            n.add_action(ak::Action::Decrement);
        }
    }

    let id = emit.ids.id_for(&node.computed_id);
    emit.live.insert(node.computed_id.clone());
    let mut children = Vec::new();
    let absorb_children = absorbing || role == Some(Role::Textbox);
    for child in &node.children {
        emit_node(child, &mut children, emit, inside_named || absorb_children);
    }
    n.set_children(children);
    emit.nodes.push((id, n));
    parent_children.push(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;
    use crate::tree::{El, Kind, Rect, column};
    use crate::widgets::button::button;
    use crate::widgets::text::text;
    use crate::{layout, runtime::RunnerCore};

    fn lay_out(mut tree: El) -> (El, UiState) {
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        (tree, state)
    }

    fn demo_tree() -> El {
        column([
            button("Save").key("save"),
            El::new(Kind::Group)
                .key("telemetry")
                .focusable()
                .role(Role::Checkbox)
                .aria_label("Enable telemetry")
                .aria_checked(true),
            text("Hello world"),
        ])
    }

    /// Every child id referenced by an emitted node must itself be
    /// emitted, ids must be unique, and the focus id must resolve.
    fn assert_integrity(update: &ak::TreeUpdate) {
        let defined: FxHashSet<ak::NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
        assert_eq!(defined.len(), update.nodes.len(), "duplicate node ids");
        for (id, node) in &update.nodes {
            for child in node.children() {
                assert!(
                    defined.contains(child),
                    "node {id:?} references undefined child {child:?}"
                );
            }
        }
        assert!(defined.contains(&update.focus), "dangling focus id");
    }

    fn node_with_label<'a>(
        update: &'a ak::TreeUpdate,
        label: &str,
    ) -> Option<&'a (ak::NodeId, ak::Node)> {
        update
            .nodes
            .iter()
            .find(|(_, n)| n.label().is_some_and(|l| l == label))
    }

    #[test]
    fn emits_roles_names_states_and_actions() {
        let (tree, state) = lay_out(demo_tree());
        let mut ids = AccessKitIds::default();
        let update = tree_update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);

        let (_, save) = node_with_label(&update, "Save").expect("button emitted");
        assert_eq!(save.role(), ak::Role::Button, "stock button self-annotates");
        assert!(save.supports_action(ak::Action::Focus));
        assert!(save.supports_action(ak::Action::Click));

        let (_, cb) = node_with_label(&update, "Enable telemetry").expect("checkbox emitted");
        assert_eq!(cb.role(), ak::Role::CheckBox);
        assert_eq!(cb.toggled(), Some(ak::Toggled::True));

        // The standalone text leaf surfaces as static text; the
        // button's own label text is absorbed into its name, not
        // emitted as a second static-text node.
        let labels: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == ak::Role::Label)
            .collect();
        assert_eq!(labels.len(), 1, "exactly the standalone text leaf");
        assert_eq!(labels[0].1.value(), Some("Hello world"));

        // Nothing focused → focus falls back to the window root.
        let (root_id, root) = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == ak::Role::Window)
            .expect("window root");
        assert_eq!(update.focus, *root_id);
        assert!(!root.children().is_empty());
    }

    #[test]
    fn focused_target_maps_to_platform_focus() {
        let (tree, mut state) = lay_out(demo_tree());
        let target = crate::focus::focus_order(&tree)
            .into_iter()
            .find(|t| t.key == "save")
            .expect("save in focus order");
        state.focused = Some(target);

        let mut ids = AccessKitIds::default();
        let update = tree_update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);
        let (save_id, _) = node_with_label(&update, "Save").expect("button emitted");
        assert_eq!(update.focus, *save_id);
    }

    #[test]
    fn aria_hidden_subtree_is_absent() {
        let (tree, state) = lay_out(column([
            button("Visible").key("v"),
            button("Secret").key("s").aria_hidden(),
        ]));
        let mut ids = AccessKitIds::default();
        let update = tree_update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);
        assert!(node_with_label(&update, "Visible").is_some());
        assert!(node_with_label(&update, "Secret").is_none());
    }

    #[test]
    fn ids_stay_stable_across_frames_and_prune_dead_nodes() {
        let (tree, state) = lay_out(demo_tree());
        let mut ids = AccessKitIds::default();
        let first = tree_update(&tree, &state, 1.0, &mut ids);
        let second = tree_update(&tree, &state, 1.0, &mut ids);
        let save_first = node_with_label(&first, "Save").unwrap().0;
        let save_second = node_with_label(&second, "Save").unwrap().0;
        assert_eq!(save_first, save_second, "same node, same id");

        // A tree without the button prunes its interning entry.
        let (smaller, state2) = lay_out(column([text("Hello world")]));
        let _ = tree_update(&smaller, &state2, 1.0, &mut ids);
        assert!(
            ids.computed_id_for(save_first).is_none(),
            "dead node pruned from the id table"
        );
    }

    #[test]
    fn click_action_synthesizes_activate() {
        let mut core = RunnerCore::new();
        let (tree, _) = lay_out(demo_tree());
        core.last_tree = Some(tree);
        let update = core.accessibility_tree_update(1.0).expect("tree present");
        let save_id = node_with_label(&update, "Save").unwrap().0;

        let events = core.accessibility_action(ak::ActionRequest {
            action: ak::Action::Click,
            target_tree: ak::TreeId::ROOT,
            target_node: save_id,
            data: None,
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, crate::event::UiEventKind::Activate);
        assert!(events[0].is_route("save"));
    }

    #[test]
    fn focus_action_queues_focus_request() {
        let mut core = RunnerCore::new();
        let (tree, _) = lay_out(demo_tree());
        core.last_tree = Some(tree);
        let update = core.accessibility_tree_update(1.0).expect("tree present");
        let cb_id = node_with_label(&update, "Enable telemetry").unwrap().0;

        let events = core.accessibility_action(ak::ActionRequest {
            action: ak::Action::Focus,
            target_tree: ak::TreeId::ROOT,
            target_node: cb_id,
            data: None,
        });
        assert!(events.is_empty(), "focus routes internally");
        // The request resolves against the focus order on the next
        // drain — the same path App::drain_focus_requests takes.
        core.ui_state
            .sync_focus_order(core.last_tree.as_ref().unwrap());
        core.ui_state.drain_focus_requests();
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("telemetry")
        );
    }
}
