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
//! Bounds come from `computed_rect`, which layout finishes in window
//! space — scroll offsets (`apply_scroll_offset`), viewport pan/zoom,
//! and `layout_override` placement are all baked in before the walk
//! runs (pinned by `bounds_track_scroll_offsets`). The one residual
//! gap: paint-time `translate`/`scale` transforms (enter transitions,
//! hover lifts, caret bars) don't reach `computed_rect`, so a node
//! mid-animation reports its settled rect — transient and cosmetic.
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

use super::{collect_text, names_from_content};

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

struct Emit<'a> {
    nodes: Vec<(ak::NodeId, ak::Node)>,
    live: FxHashSet<Arc<str>>,
    ids: &'a mut AccessKitIds,
    ui_state: &'a UiState,
    text_runs: &'a mut TextRunTable,
}

/// Per-frame table mapping synthesized text-run platform ids back to
/// their widget key and source-string byte offsets — how an
/// AT-supplied [`ak::TextSelection`] position resolves to the `(key,
/// byte)` pair the app's [`Selection`](crate::selection::Selection)
/// speaks. Rebuilt on every tree emission; owned by the runner beside
/// [`AccessKitIds`] so a late-arriving action request resolves against
/// the tree the AT actually saw.
#[derive(Default)]
pub struct TextRunTable {
    entries: FxHashMap<u64, TextRunEntry>,
}

/// One synthesized text run's routing data.
pub(crate) struct TextRunEntry {
    /// Widget key of the owning textbox — the `UiEvent` route.
    pub key: String,
    /// Source-string byte offset at each character boundary of the
    /// run, both ends inclusive (`len == character count + 1`), so an
    /// AT `character_index` (0..=count) indexes directly.
    pub char_source_offsets: Vec<u32>,
}

impl TextRunTable {
    pub(crate) fn get(&self, id: ak::NodeId) -> Option<&TextRunEntry> {
        self.entries.get(&id.0)
    }
}

/// Lower the laid-out tree into a full-tree [`ak::TreeUpdate`].
///
/// `scale_factor` maps damascene's logical pixels to the physical
/// pixels platform accessibility APIs expect; it is applied once as a
/// transform on the root node. `focused` comes from
/// [`UiState::focused`]; AccessKit requires a focus in every update,
/// falling back to the root when nothing (or something unmapped) is
/// focused. `text_runs` is rebuilt with the emitted text runs' routing
/// data.
pub fn tree_update(
    root: &El,
    ui_state: &UiState,
    scale_factor: f32,
    ids: &mut AccessKitIds,
    text_runs: &mut TextRunTable,
) -> ak::TreeUpdate {
    let root_id = ids.id_for(&root.computed_id);
    text_runs.entries.clear();
    let mut emit = Emit {
        nodes: Vec::new(),
        live: FxHashSet::default(),
        ids,
        ui_state,
        text_runs,
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
    // The root element can itself be the scroll viewport (fullscreen
    // document apps build `scroll(...)` at the top). `emit_node` never
    // sees the root, so the Window node carries the scroll surface.
    if let Some(m) = ui_state
        .scroll_metrics(&root.computed_id)
        .filter(|m| m.max_offset > crate::state::WHEEL_EPSILON)
    {
        apply_scroll_semantics(&mut root_node, m, ui_state.scroll_offset(&root.computed_id));
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

    let Emit {
        nodes, live, ids, ..
    } = emit;
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
    // A live scroll viewport per the last layout pass (scroll
    // containers and virtual lists share the metrics map). Overflowing
    // ones are semantic even roleless: AT sequential navigation can
    // only reach off-screen content through a container that
    // advertises scrolling. Containers whose content fits stay
    // hoistable structure. The epsilon matches `scroll_by_id`'s
    // refusal band so an advertised scrollable can always move.
    let scroll = emit
        .ui_state
        .scroll_metrics(&node.computed_id)
        .filter(|m| m.max_offset > crate::state::WHEEL_EPSILON);
    // Semantic contribution: an explicit role or any ARIA prop, a
    // focusable (interactive) node, image/link content, a live scroll
    // viewport, or a visible text leaf not already absorbed into an
    // ancestor's name. `Role::Presentation` strips implicit semantics:
    // it contributes only if focusable (an interactive thing is never
    // presentational to AT) — an explicitly presentational scroller
    // is a deliberate author strip and stays out.
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
            || scroll.is_some()
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
    let text_edit = props.and_then(|p| p.text_edit.as_deref());
    let ak_role = match role {
        // A multiline-declared textbox is a distinct platform class —
        // screen readers switch to document-style line navigation.
        Some(Role::Textbox) if text_edit.is_some_and(|t| t.multiline) => {
            ak::Role::MultilineTextInput
        }
        Some(r) => map_role(r),
        None if node.image.is_some() => ak::Role::Image,
        None if node.text_link.is_some() => ak::Role::Link,
        // Before the focusable fallback: a focusable scroll viewport
        // is still best described as one.
        None if scroll.is_some() => ak::Role::ScrollView,
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
    let mut named = false;
    if let Some(label) = label {
        n.set_label(label);
        named = true;
    } else if absorbing {
        let mut text = String::new();
        collect_text(node, &mut text);
        if !text.is_empty() {
            n.set_label(text);
            named = true;
        }
    } else if is_text_leaf && let Some(text) = &node.text {
        n.set_value(text.clone());
        named = true;
    }
    // HTML `title` semantics: a tooltip is the last-resort accessible
    // name when nothing else names the node (an icon-only button whose
    // tooltip is its label). Otherwise it doubles as the description
    // below. Roleless non-leaf nodes may still be named by their text
    // children at the platform layer, so content wins over the tooltip
    // there too.
    let mut tooltip_as_name = false;
    if !named && let Some(tooltip) = &node.tooltip {
        let content_names_it = role.is_none_or(names_from_content) && {
            let mut text = String::new();
            collect_text(node, &mut text);
            !text.is_empty()
        };
        if !content_names_it && !tooltip.trim().is_empty() {
            n.set_label(tooltip.clone());
            tooltip_as_name = true;
        }
    }
    if role == Some(Role::Textbox) {
        if let Some(edit) = text_edit {
            // The declared rendered value — never the placeholder,
            // which `collect_text` would scoop out of an empty field's
            // hint leaf (empty fields used to report value == label ==
            // placeholder). The platform placeholder property carries
            // the hint separately.
            n.set_value(edit.value.clone());
            if let Some(ph) = &edit.placeholder {
                n.set_placeholder(ph.clone());
            }
        } else {
            // Descriptor-less custom textbox: the subtree text is the
            // best available value.
            let mut value = String::new();
            collect_text(node, &mut value);
            n.set_value(value);
        }
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
    // A tooltip doubles as the description when no explicit one is set
    // and it wasn't already promoted to the name above.
    if !tooltip_as_name
        && props.is_none_or(|p| p.description.is_none())
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
        // Text-protocol actions need a key to route the resulting
        // events (SetTextSelection → SelectionChanged,
        // ReplaceSelectedText → TextInput).
        if text_edit.is_some() && node.key.is_some() {
            n.add_action(ak::Action::SetTextSelection);
            n.add_action(ak::Action::ReplaceSelectedText);
        }
    }

    if let Some(m) = scroll {
        let offset = emit.ui_state.scroll_offset(&node.computed_id);
        apply_scroll_semantics(&mut n, m, offset);
    }

    let id = emit.ids.id_for(&node.computed_id);
    emit.live.insert(node.computed_id.clone());
    let mut children = Vec::new();
    // A textbox's element children are its own rendering chrome —
    // value/placeholder text leaves, caret bar, selection bands (which
    // smuggle painter payloads through `text_link`/`tooltip` and would
    // otherwise surface as bogus Link nodes named by the whole
    // document). To AT a text input's content is its *value*, not a
    // subtree; the synthesized text runs are the only children it
    // grows.
    if role == Some(Role::Textbox) {
        if let Some(edit) = text_edit {
            children = emit_text_runs(node, edit, &mut n, emit);
        }
    } else {
        for child in &node.children {
            emit_node(child, &mut children, emit, inside_named || absorbing);
        }
    }
    n.set_children(children);
    emit.nodes.push((id, n));
    parent_children.push(id);
}

/// Stamp scroll semantics on a platform node: position/extent as
/// properties (the Android adapter synthesizes TYPE_VIEW_SCROLLED from
/// `scroll_y` diffs and derives `isScrollable` from the actions),
/// paging as actions (TalkBack swipe-past-the-edge and two-finger
/// scroll arrive as ScrollUp/ScrollDown; routing is
/// scroll-identity-based in `RunnerCore::accessibility_action`).
/// Damascene scrolling is vertical-only, so left/right never appear.
/// Offsets/extents stay logical px — the root transform scales them
/// with the bounds.
fn apply_scroll_semantics(n: &mut ak::Node, m: crate::state::ScrollMetrics, offset: f32) {
    let offset = offset.clamp(0.0, m.max_offset);
    n.set_orientation(ak::Orientation::Vertical);
    n.set_scroll_y(f64::from(offset));
    n.set_scroll_y_min(0.0);
    n.set_scroll_y_max(f64::from(m.max_offset));
    // Directions share `scroll_by_id`'s dead zone so an advertised
    // action is never refused as a no-op.
    if offset > crate::state::WHEEL_EPSILON {
        n.add_action(ak::Action::ScrollUp);
    }
    if offset < m.max_offset - crate::state::WHEEL_EPSILON {
        n.add_action(ak::Action::ScrollDown);
    }
    // Descendants may ask to be revealed (UIA ScrollItem, AT-SPI
    // Component.ScrollTo; Android 0.7.5 has no ACTION_SHOW_ON_SCREEN
    // mapping yet and pages via the actions above instead).
    n.add_child_action(ak::Action::ScrollIntoView);
}

/// One AT "character" cell of a text run: a grapheme cluster, or a
/// ≤255-byte piece of a pathological oversized cluster (AccessKit's
/// per-character byte lengths are `u8`).
struct Cell {
    /// Byte offset in the rendered value.
    start: usize,
    /// Byte length; sums across a run to its value's length.
    len: u8,
    /// X of the cell's left edge in layout-origin coordinates.
    x: f32,
    /// Advance width.
    w: f32,
}

/// A synthesized run and the data needed after the build loop
/// (selection resolution, on-line chaining).
struct BuiltRun {
    id: ak::NodeId,
    /// Rendered-value byte range, trailing hard `\n` included.
    range: std::ops::Range<usize>,
    /// Byte offset of each cell (for byte → character-index lookup).
    cell_starts: Vec<usize>,
}

/// Synthesize the AccessKit text protocol's `TextRun` children for a
/// declared-editable textbox: per-character byte/geometry tables, word
/// starts, the caret/selection state, and the routing table entries
/// that let `SetTextSelection` positions resolve back to `(key, byte)`.
///
/// Geometry comes from re-shaping the rendered value with the value
/// leaf's own style — the same engine and parameters the caret/hit
/// paths use, so reported cluster edges are exactly where the caret
/// paints. A textbox whose declared value isn't rendered as one text
/// leaf (custom widgets with styled inlines) still gets runs, just
/// without per-character geometry: reading and caret logic keep
/// working, magnifier caret-tracking degrades. Lines the shaper
/// resolved as RTL also omit geometry (AccessKit positions are
/// direction-relative; damascene's are visual) and carry an RTL
/// direction override instead.
fn emit_text_runs(
    node: &El,
    edit: &crate::a11y::EditableText,
    input: &mut ak::Node,
    emit: &mut Emit<'_>,
) -> Vec<ak::NodeId> {
    use crate::text::metrics::character_metrics;
    use crate::tree::TextWrap;

    // Attribute inheritance: one direction declaration on the input
    // covers every run; RTL lines override per-run.
    input.set_text_direction(ak::TextDirection::LeftToRight);

    let leaf = if edit.value.is_empty() {
        None
    } else {
        find_text_leaf(node, &edit.value)
    };
    let (metrics, origin, geometry) = match leaf {
        Some(leaf) => {
            let width = (leaf.text_wrap == TextWrap::Wrap).then_some(leaf.computed_rect.w);
            (
                character_metrics(
                    &edit.value,
                    leaf.font_size,
                    leaf.font_family,
                    leaf.font_weight,
                    leaf.font_mono,
                    leaf.text_tabular_numerals,
                    leaf.text_wrap,
                    width,
                ),
                (leaf.computed_rect.x, leaf.computed_rect.y),
                true,
            )
        }
        None => {
            // Empty value, or a declared value with no matching leaf:
            // byte structure only, anchored at the content rect.
            let content = node.computed_rect.inset(node.padding);
            (
                character_metrics(
                    &edit.value,
                    crate::tokens::TEXT_SM.size,
                    crate::tree::FontFamily::default(),
                    crate::tree::FontWeight::Regular,
                    false,
                    false,
                    TextWrap::NoWrap,
                    None,
                ),
                (content.x, content.y),
                false,
            )
        }
    };

    // Word starts over the whole rendered value, so a word split by a
    // soft wrap (or a >255-cell run split) doesn't restart on the next
    // run — AT word navigation walks back across the boundary exactly
    // like Ctrl+Left does.
    let word_start_bytes: FxHashSet<usize> = {
        let mut starts = FxHashSet::default();
        let mut prev_is_word = false;
        for (i, ch) in edit.value.char_indices() {
            let w = crate::selection::is_word_char(ch);
            if w && !prev_is_word {
                starts.insert(i);
            }
            prev_is_word = w;
        }
        starts
    };

    let mut built: Vec<BuiltRun> = Vec::new();
    let mut nodes: Vec<(ak::NodeId, ak::Node)> = Vec::new();
    for line in &metrics.lines {
        // Flatten the line's clusters into u8-sized cells.
        let mut cells: Vec<Cell> = Vec::new();
        let mut byte = line.byte_range.start;
        for (i, &len) in line.char_lengths.iter().enumerate() {
            let (x, w) = (line.char_positions[i], line.char_widths[i]);
            if len <= usize::from(u8::MAX) {
                cells.push(Cell {
                    start: byte,
                    len: len as u8,
                    x,
                    w,
                });
            } else {
                // Pathological >255-byte cluster: split at char
                // boundaries into ≤255-byte pieces, advance shared
                // evenly. AT sees several "characters"; harmless.
                let cluster = &edit.value[byte..byte + len];
                let pieces = len.div_ceil(usize::from(u8::MAX));
                let piece_w = w / pieces as f32;
                let mut piece_start = 0usize;
                let mut piece_i = 0f32;
                for (ci, ch) in cluster.char_indices() {
                    if ci + ch.len_utf8() - piece_start > usize::from(u8::MAX) {
                        cells.push(Cell {
                            start: byte + piece_start,
                            len: (ci - piece_start) as u8,
                            x: x + piece_w * piece_i,
                            w: piece_w,
                        });
                        piece_i += 1.0;
                        piece_start = ci;
                    }
                }
                cells.push(Cell {
                    start: byte + piece_start,
                    len: (len - piece_start) as u8,
                    x: x + piece_w * piece_i,
                    w: piece_w,
                });
            }
            byte += len;
        }

        // ≤255 cells per run: character indices are u8 on the wire.
        let chunks: Vec<&[Cell]> = if cells.is_empty() {
            vec![&[][..]]
        } else {
            cells.chunks(usize::from(u8::MAX)).collect()
        };
        let line_run_ids: Vec<ak::NodeId> = (0..chunks.len())
            .map(|ci| {
                let cid: Arc<str> =
                    format!("{}/textrun{}", node.computed_id, built.len() + ci).into();
                let id = emit.ids.id_for(&cid);
                emit.live.insert(cid);
                id
            })
            .collect();

        for (ci, chunk) in chunks.iter().enumerate() {
            let id = line_run_ids[ci];
            let range_start = chunk.first().map_or(line.byte_range.start, |c| c.start);
            let range_end = chunk
                .last()
                .map_or(line.byte_range.end, |c| c.start + usize::from(c.len));
            let mut rn = ak::Node::new(ak::Role::TextRun);
            rn.set_value(edit.value[range_start..range_end].to_string());
            rn.set_character_lengths(chunk.iter().map(|c| c.len).collect::<Vec<u8>>());

            // Geometry: bounds always (visual extent works for RTL
            // too); per-character positions only for LTR lines, where
            // damascene's visual x offsets are AccessKit's
            // direction-relative offsets.
            let extent = || {
                let x0 = chunk.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
                let x1 = chunk.iter().map(|c| c.x + c.w).fold(0.0f32, f32::max);
                (x0.min(x1), x1)
            };
            if geometry && !chunk.is_empty() {
                let (x0, x1) = extent();
                rn.set_bounds(ak::Rect {
                    x0: f64::from(origin.0 + x0),
                    y0: f64::from(origin.1 + line.rect.1),
                    x1: f64::from(origin.0 + x1),
                    y1: f64::from(origin.1 + line.rect.1 + line.rect.3),
                });
                if !line.rtl {
                    rn.set_character_positions(
                        chunk.iter().map(|c| c.x - x0).collect::<Vec<f32>>(),
                    );
                    rn.set_character_widths(chunk.iter().map(|c| c.w).collect::<Vec<f32>>());
                }
            } else {
                rn.set_bounds(ak::Rect {
                    x0: f64::from(origin.0 + line.rect.0),
                    y0: f64::from(origin.1 + line.rect.1),
                    x1: f64::from(origin.0 + line.rect.0 + line.rect.2),
                    y1: f64::from(origin.1 + line.rect.1 + line.rect.3),
                });
            }
            if line.rtl {
                rn.set_text_direction(ak::TextDirection::RightToLeft);
            }

            rn.set_word_starts(
                chunk
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| word_start_bytes.contains(&c.start))
                    .map(|(i, _)| i as u8)
                    .collect::<Vec<u8>>(),
            );

            // Chunks of one long visual line form a line via the
            // on-line links; distinct visual lines stay unlinked (each
            // is its own line to the consumer).
            if ci > 0 {
                rn.set_previous_on_line(line_run_ids[ci - 1]);
            }
            if ci + 1 < line_run_ids.len() {
                rn.set_next_on_line(line_run_ids[ci + 1]);
            }

            if let Some(key) = &node.key {
                let mut offs: Vec<u32> = chunk
                    .iter()
                    .map(|c| edit.visible_to_source(c.start) as u32)
                    .collect();
                offs.push(edit.visible_to_source(range_end) as u32);
                emit.text_runs.entries.insert(
                    id.0,
                    TextRunEntry {
                        key: key.clone(),
                        char_source_offsets: offs,
                    },
                );
            }

            built.push(BuiltRun {
                id,
                range: range_start..range_end,
                cell_starts: chunk.iter().map(|c| c.start).collect(),
            });
            nodes.push((id, rn));
        }
    }

    // Caret/selection state: the app's source-byte selection, mapped
    // through the display transform onto the runs just built. Only
    // reported while the selection actually lives in this field — a
    // blurred field has no caret to report, matching the painter.
    if let Some(key) = node.key.as_deref()
        && let Some(sel) = emit.ui_state.current_selection.within(key)
    {
        let anchor = resolve_text_position(&built, edit.source_to_visible(sel.anchor));
        let focus = resolve_text_position(&built, edit.source_to_visible(sel.head));
        if let (Some(anchor), Some(focus)) = (anchor, focus) {
            input.set_text_selection(ak::TextSelection { anchor, focus });
        }
    }

    let ids = nodes.iter().map(|(id, _)| *id).collect();
    emit.nodes.extend(nodes);
    ids
}

/// Map a rendered-value byte offset onto the built runs as an AccessKit
/// text position. Offsets at a run boundary resolve into the following
/// run (the consumer normalizes both spellings identically); a caret
/// at a hard line end lands *on* the `\n` cell, per the AccessKit
/// convention.
fn resolve_text_position(built: &[BuiltRun], byte: usize) -> Option<ak::TextPosition> {
    let last = built.last()?;
    for run in built {
        if byte < run.range.end {
            let index = run
                .cell_starts
                .partition_point(|s| *s <= byte)
                .saturating_sub(1);
            return Some(ak::TextPosition {
                node: run.id,
                character_index: index,
            });
        }
    }
    Some(ak::TextPosition {
        node: last.id,
        character_index: last.cell_starts.len(),
    })
}

/// First descendant text leaf rendering exactly `value` — the leaf the
/// text-input widgets paint their value with; its style and rect drive
/// run geometry.
fn find_text_leaf<'t>(node: &'t El, value: &str) -> Option<&'t El> {
    if node.text.as_deref() == Some(value) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_text_leaf(c, value))
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

    /// [`tree_update`] with a throwaway run table, for tests that
    /// don't inspect text-run routing.
    fn update(
        tree: &El,
        state: &UiState,
        scale_factor: f32,
        ids: &mut AccessKitIds,
    ) -> ak::TreeUpdate {
        tree_update(tree, state, scale_factor, ids, &mut TextRunTable::default())
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
        let update = update(&tree, &state, 1.0, &mut ids);
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
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);
        let (save_id, _) = node_with_label(&update, "Save").expect("button emitted");
        assert_eq!(update.focus, *save_id);
    }

    #[test]
    fn tooltip_names_unnamed_controls_and_describes_named_ones() {
        use crate::widgets::button::icon_button;
        let (tree, state) = lay_out(column([
            // Icon-only, no label: the tooltip is promoted to the name
            // (HTML `title` fallback) and must NOT also double as the
            // description.
            icon_button(crate::IconName::Plus)
                .key("add")
                .tooltip("New tab"),
            // Already named by content: the tooltip stays a description.
            button("Save").key("save").tooltip("Write to disk"),
        ]));
        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);

        let (_, add) = node_with_label(&update, "New tab").expect("tooltip promoted to name");
        assert_eq!(add.role(), ak::Role::Button);
        assert_eq!(add.description(), None, "not doubled as description");

        let (_, save) = node_with_label(&update, "Save").expect("content-named button");
        assert_eq!(save.description(), Some("Write to disk"));
    }

    #[test]
    fn synthesized_announcements_emit_as_live_named_nodes() {
        // The runtime's announcement layer must reach the platform
        // tree as named live nodes — a node *added* with live != off
        // and a name is exactly what adapters announce (AT-SPI's
        // object:announcement, and equivalents elsewhere).
        let mut tree = crate::stack([button("Save").key("save")]);
        let mut state = UiState::new();
        let now = web_time::Instant::now();
        state.push_announcement(crate::announce::Announcement::polite("Saved to disk"), now);
        state.push_announcement(
            crate::announce::Announcement::assertive("Connection lost"),
            now,
        );
        crate::layout::assign_ids(&mut tree);
        assert!(crate::announce::synthesize_announcements(
            &mut tree, &mut state, now
        ));
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));

        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);

        let (_, polite) = node_with_label(&update, "Saved to disk").expect("polite node emitted");
        assert_eq!(polite.role(), ak::Role::Status);
        assert_eq!(polite.live(), Some(ak::Live::Polite));

        let (_, assertive) =
            node_with_label(&update, "Connection lost").expect("assertive node emitted");
        assert_eq!(assertive.role(), ak::Role::Alert);
        assert_eq!(assertive.live(), Some(ak::Live::Assertive));
    }

    #[test]
    fn textbox_chrome_stays_out_of_the_platform_tree() {
        // A text input's element children are rendering chrome. The
        // text_area caret/selection paint layers in particular carry
        // painter payloads in `text_link`/`tooltip`; before textbox
        // children were suppressed they surfaced as Link nodes whose
        // accessible name was the entire document.
        let doc = "line one\nline two";
        let selection = crate::selection::Selection::caret("notes", 3);
        let (tree, state) = lay_out(column([crate::widgets::text_area::text_area(
            "notes", doc, &selection,
        )]));
        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);

        assert!(
            !update.nodes.iter().any(|(_, n)| n.role() == ak::Role::Link),
            "paint layers must not leak as Link nodes"
        );
        assert!(
            node_with_label(&update, doc).is_none(),
            "document text must not become any node's accessible name"
        );
        let (_, textbox) = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == ak::Role::MultilineTextInput)
            .expect("text_area lowers as a multiline text input");
        let child_roles: Vec<ak::Role> = textbox
            .children()
            .iter()
            .map(|id| {
                update
                    .nodes
                    .iter()
                    .find(|(nid, _)| nid == id)
                    .expect("child emitted")
                    .1
                    .role()
            })
            .collect();
        assert!(
            child_roles.iter().all(|r| *r == ak::Role::TextRun),
            "synthesized text runs are a textbox's only children: {child_roles:?}"
        );
    }

    fn find_role(update: &ak::TreeUpdate, role: ak::Role) -> &(ak::NodeId, ak::Node) {
        update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == role)
            .unwrap_or_else(|| panic!("no {role:?} node emitted"))
    }

    fn run_nodes<'a>(
        update: &'a ak::TreeUpdate,
        input: &ak::Node,
    ) -> Vec<&'a (ak::NodeId, ak::Node)> {
        input
            .children()
            .iter()
            .map(|id| {
                update
                    .nodes
                    .iter()
                    .find(|(nid, _)| nid == id)
                    .expect("run emitted")
            })
            .collect()
    }

    #[test]
    fn text_input_emits_character_level_runs_and_selection() {
        let selection = crate::selection::Selection::caret("email", 3);
        let (tree, mut state) = lay_out(column([crate::widgets::text_input::text_input(
            "email", "hi there", &selection,
        )]));
        state.current_selection = selection;

        let mut ids = AccessKitIds::default();
        let mut runs = TextRunTable::default();
        let update = tree_update(&tree, &state, 1.0, &mut ids, &mut runs);
        assert_integrity(&update);

        let (_, input) = find_role(&update, ak::Role::TextInput);
        assert_eq!(input.value(), Some("hi there"));
        assert!(input.supports_action(ak::Action::SetTextSelection));
        assert!(input.supports_action(ak::Action::ReplaceSelectedText));

        let run_list = run_nodes(&update, input);
        assert_eq!(run_list.len(), 1, "single-line value, single run");
        let (run_id, run) = run_list[0];
        assert_eq!(run.role(), ak::Role::TextRun);
        assert_eq!(run.value(), Some("hi there"));
        assert_eq!(run.character_lengths(), &[1u8; 8][..]);
        let positions = run.character_positions().expect("geometry present");
        assert_eq!(positions.len(), 8);
        assert!(
            positions.windows(2).all(|w| w[0] <= w[1]),
            "positions non-decreasing: {positions:?}"
        );
        assert!(
            run.bounds().expect("run bounds").x1 > run.bounds().unwrap().x0,
            "run box has extent"
        );
        // "hi there": word starts at 'h' (0) and 't' (3).
        assert_eq!(run.word_starts(), &[0u8, 3][..]);

        // The app's caret (source byte 3) reports as a degenerate
        // selection at character 3 of the run.
        let sel = input.text_selection().expect("caret reported");
        assert_eq!(sel.anchor.node, *run_id);
        assert_eq!(sel.anchor.character_index, 3);
        assert_eq!(sel.focus.character_index, 3);

        // Routing table: run resolves back to the widget key with
        // identity source offsets.
        let entry = runs.get(*run_id).expect("run in routing table");
        assert_eq!(entry.key, "email");
        assert_eq!(
            entry.char_source_offsets,
            (0..=8).collect::<Vec<u32>>(),
            "unmasked field maps identity"
        );
    }

    #[test]
    fn text_area_runs_split_lines_and_carry_newlines() {
        let selection = crate::selection::Selection::default();
        let (tree, state) = lay_out(column([crate::widgets::text_area::text_area(
            "notes", "ab\ncd", &selection,
        )]));
        let mut ids = AccessKitIds::default();
        let mut runs = TextRunTable::default();
        let update = tree_update(&tree, &state, 1.0, &mut ids, &mut runs);
        assert_integrity(&update);

        let (_, input) = find_role(&update, ak::Role::MultilineTextInput);
        assert_eq!(input.value(), Some("ab\ncd"));
        let run_list = run_nodes(&update, input);
        assert_eq!(run_list.len(), 2, "one run per visual line");
        let (_, first) = run_list[0];
        let (_, second) = run_list[1];
        assert_eq!(
            first.value(),
            Some("ab\n"),
            "hard break belongs to its line's run"
        );
        assert_eq!(first.character_lengths(), &[1u8, 1, 1][..]);
        assert_eq!(second.value(), Some("cd"));
        assert!(
            second.bounds().unwrap().y0 > first.bounds().unwrap().y0,
            "second line sits below the first"
        );
        assert!(
            first.next_on_line().is_none(),
            "distinct visual lines are not chained"
        );
    }

    #[test]
    fn masked_input_reports_bullets_and_maps_offsets() {
        use crate::widgets::text_input::{TextInputOpts, text_input_with};
        // "héllo": 5 scalars, 6 bytes — é is 2. Each scalar renders as
        // one 3-byte bullet.
        let selection = crate::selection::Selection::caret("pw", 3);
        let (tree, mut state) = lay_out(column([text_input_with(
            "pw",
            "héllo",
            &selection,
            TextInputOpts::default().password(),
        )]));
        state.current_selection = selection;

        let mut ids = AccessKitIds::default();
        let mut runs = TextRunTable::default();
        let update = tree_update(&tree, &state, 1.0, &mut ids, &mut runs);
        assert_integrity(&update);

        let (_, input) = find_role(&update, ak::Role::TextInput);
        assert_eq!(input.value(), Some("•••••"), "AT reads what the user sees");

        let run_list = run_nodes(&update, input);
        let (run_id, run) = run_list[0];
        assert_eq!(run.character_lengths(), &[3u8; 5][..]);

        // Source byte 3 (after "hé") is display character 2.
        let sel = input.text_selection().expect("caret reported");
        assert_eq!(sel.focus.character_index, 2);

        let entry = runs.get(*run_id).expect("run in routing table");
        assert_eq!(
            entry.char_source_offsets,
            vec![0, 1, 3, 4, 5, 6],
            "display characters map back to source byte boundaries"
        );
    }

    #[test]
    fn empty_input_keeps_placeholder_out_of_value() {
        use crate::widgets::text_input::{TextInputOpts, text_input_with};
        let selection = crate::selection::Selection::default();
        let (tree, state) = lay_out(column([text_input_with(
            "email",
            "",
            &selection,
            TextInputOpts::default().placeholder("Email address"),
        )]));
        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);

        let (_, input) = find_role(&update, ak::Role::TextInput);
        assert_eq!(
            input.value(),
            Some(""),
            "an empty field's value is empty, not its placeholder"
        );
        assert_eq!(input.placeholder(), Some("Email address"));
        assert_eq!(
            input.label(),
            Some("Email address"),
            "placeholder still names the unlabeled field (HTML fallback)"
        );
        let run_list = run_nodes(&update, input);
        assert_eq!(run_list.len(), 1, "empty field keeps one empty run");
        assert_eq!(run_list[0].1.value(), Some(""));
        assert!(run_list[0].1.character_lengths().is_empty());
    }

    #[test]
    fn aria_hidden_subtree_is_absent() {
        let (tree, state) = lay_out(column([
            button("Visible").key("v"),
            button("Secret").key("s").aria_hidden(),
        ]));
        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);
        assert!(node_with_label(&update, "Visible").is_some());
        assert!(node_with_label(&update, "Secret").is_none());
    }

    #[test]
    fn bounds_track_scroll_offsets() {
        // Scroll offsets are baked into `computed_rect` during layout
        // (`apply_scroll_offset` → `shift_subtree_y`), so the bounds
        // the lowering emits are already window-space-correct for
        // scrolled content. This pins that: an earlier module doc
        // claimed the opposite, and text-run caret geometry depends on
        // it staying true.
        use crate::tree::Size;
        let mut tree = crate::tree::scroll((0..6).map(|i| {
            button(format!("row {i}"))
                .key(format!("b{i}"))
                .height(Size::Fixed(50.0))
        }))
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        crate::layout::assign_ids(&mut tree);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let unscrolled = {
            let mut ids = AccessKitIds::default();
            let update = update(&tree, &state, 1.0, &mut ids);
            node_with_label(&update, "row 0")
                .expect("row emitted")
                .1
                .bounds()
                .expect("bounds set")
        };

        state
            .scroll
            .offsets
            .insert(tree.computed_id.to_string(), 80.0);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&update);
        let scrolled = node_with_label(&update, "row 0")
            .expect("scrolled-out content is still emitted")
            .1
            .bounds()
            .expect("bounds set");
        assert!(
            (scrolled.y0 - (unscrolled.y0 - 80.0)).abs() < 0.01,
            "a11y bounds must track the scroll offset: unscrolled y0 = {}, scrolled y0 = {}",
            unscrolled.y0,
            scrolled.y0
        );
    }

    /// The scroll tree the scroll-semantics tests share: 6×50px rows
    /// with 12px gaps (content 360) in a 200px viewport → max offset
    /// 160. Wrapped in a column so the container takes the ordinary
    /// `emit_node` path (the root is emitted separately as Window).
    fn scroll_fixture() -> El {
        use crate::tree::Size;
        column([crate::tree::scroll((0..6).map(|i| {
            button(format!("row {i}"))
                .key(format!("b{i}"))
                .height(Size::Fixed(50.0))
        }))
        .gap(12.0)
        .height(Size::Fixed(200.0))])
    }

    /// The scroll container's computed id — assigned during layout.
    fn fixture_scroll_id(tree: &El) -> String {
        tree.children[0].computed_id.to_string()
    }

    #[test]
    fn scroll_containers_expose_scroll_semantics() {
        let mut tree = scroll_fixture();
        let mut state = UiState::new();
        let mut ids = AccessKitIds::default();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let up = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&up);
        let (_, sv) = find_role(&up, ak::Role::ScrollView);
        assert_eq!(sv.scroll_y(), Some(0.0));
        assert_eq!(sv.scroll_y_min(), Some(0.0));
        assert!(
            sv.scroll_y_max().is_some_and(|m| (m - 160.0).abs() < 0.5),
            "content 360 in viewport 200 → max 160, got {:?}",
            sv.scroll_y_max()
        );
        assert!(!sv.supports_action(ak::Action::ScrollUp), "top edge");
        assert!(sv.supports_action(ak::Action::ScrollDown));
        assert!(sv.child_supports_action(ak::Action::ScrollIntoView));

        // Mid-scroll reports position and both directions.
        state.scroll.offsets.insert(fixture_scroll_id(&tree), 80.0);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let up = update(&tree, &state, 1.0, &mut ids);
        let (_, sv) = find_role(&up, ak::Role::ScrollView);
        assert_eq!(sv.scroll_y(), Some(80.0));
        assert!(sv.supports_action(ak::Action::ScrollUp));
        assert!(sv.supports_action(ak::Action::ScrollDown));

        // Bottom edge drops the forward direction.
        state.scroll.offsets.insert(fixture_scroll_id(&tree), 160.0);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let up = update(&tree, &state, 1.0, &mut ids);
        let (_, sv) = find_role(&up, ak::Role::ScrollView);
        assert!(sv.supports_action(ak::Action::ScrollUp));
        assert!(!sv.supports_action(ak::Action::ScrollDown), "bottom edge");
    }

    #[test]
    fn fitting_scroll_containers_stay_structural() {
        use crate::tree::Size;
        let (tree, state) = lay_out(
            crate::tree::scroll([button("only row").key("r").height(Size::Fixed(50.0))])
                .height(Size::Fixed(200.0)),
        );
        let mut ids = AccessKitIds::default();
        let up = update(&tree, &state, 1.0, &mut ids);
        assert!(
            !up.nodes
                .iter()
                .any(|(_, n)| n.role() == ak::Role::ScrollView),
            "content fits — the container contributes nothing and hoists"
        );
        assert!(
            node_with_label(&up, "only row").is_some(),
            "children attach to the nearest emitted ancestor"
        );
    }

    #[test]
    fn virtual_lists_expose_scroll_semantics() {
        use crate::tree::Size;
        let (tree, state) = lay_out(column([crate::tree::virtual_list(100, 40.0, |i| {
            text(format!("item {i}"))
        })
        .key("feed")
        .height(Size::Fixed(200.0))]));
        let mut ids = AccessKitIds::default();
        let up = update(&tree, &state, 1.0, &mut ids);
        assert_integrity(&up);
        let (_, sv) = find_role(&up, ak::Role::ScrollView);
        assert!(
            sv.scroll_y_max().is_some_and(|m| m > 0.0),
            "virtual lists share the scroll metrics map"
        );
        assert!(sv.supports_action(ak::Action::ScrollDown));
    }

    #[test]
    fn root_scroll_container_scrolls_through_the_window_node() {
        use crate::tree::Size;
        // A fullscreen document app: `scroll(...)` IS the root.
        // `emit_node` never sees the root, so the Window node carries
        // the scroll surface.
        let mut tree = crate::tree::scroll((0..6).map(|i| {
            button(format!("row {i}"))
                .key(format!("b{i}"))
                .height(Size::Fixed(50.0))
        }))
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let mut ids = AccessKitIds::default();
        let up = update(&tree, &state, 1.0, &mut ids);
        let (_, window) = find_role(&up, ak::Role::Window);
        assert!(
            window
                .scroll_y_max()
                .is_some_and(|m| (m - 160.0).abs() < 0.5)
        );
        assert!(window.supports_action(ak::Action::ScrollDown));
    }

    #[test]
    fn scroll_actions_page_the_container() {
        let mut core = RunnerCore::new();
        let mut tree = scroll_fixture();
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let cid = fixture_scroll_id(&tree);
        core.ui_state = state;
        core.last_tree = Some(tree);
        let up = core.accessibility_tree_update(1.0).expect("tree present");
        let (sv_id, _) = find_role(&up, ak::Role::ScrollView);

        let page_down = ak::ActionRequest {
            action: ak::Action::ScrollDown,
            target_tree: ak::TreeId::ROOT,
            target_node: *sv_id,
            data: None,
        };
        let events = core.accessibility_action(page_down.clone());
        assert!(
            events.is_empty(),
            "offsets are retained state, not app events"
        );
        // One page = 90% of the 200px viewport = 180, clamped to max 160.
        assert!((core.ui_state.scroll_offset(&cid) - 160.0).abs() < 0.01);

        // At the bottom a further page is refused, not wrapped.
        core.accessibility_action(page_down);
        assert!((core.ui_state.scroll_offset(&cid) - 160.0).abs() < 0.01);

        core.accessibility_action(ak::ActionRequest {
            action: ak::Action::ScrollUp,
            target_tree: ak::TreeId::ROOT,
            target_node: *sv_id,
            data: None,
        });
        assert!(core.ui_state.scroll_offset(&cid).abs() < 0.01);
    }

    #[test]
    fn scroll_into_view_action_reveals_offscreen_target() {
        let mut core = RunnerCore::new();
        let mut tree = scroll_fixture();
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let cid = fixture_scroll_id(&tree);
        core.ui_state = state;
        core.last_tree = Some(tree);
        let up = core.accessibility_tree_update(1.0).expect("tree present");
        let (row5_id, _) = node_with_label(&up, "row 5").expect("scrolled-out row emitted");

        core.accessibility_action(ak::ActionRequest {
            action: ak::Action::ScrollIntoView,
            target_tree: ak::TreeId::ROOT,
            target_node: *row5_id,
            data: None,
        });
        // Row 5 spans 310..360 in content space; the minimal
        // displacement rests its bottom on the viewport bottom.
        assert!((core.ui_state.scroll_offset(&cid) - 160.0).abs() < 0.01);

        // Re-lay out at the new offset (the next frame); the row is
        // visible and a repeated request holds still.
        let mut tree = scroll_fixture();
        layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        core.last_tree = Some(tree);
        let up = core.accessibility_tree_update(1.0).expect("tree present");
        let (row5_id, row5) = node_with_label(&up, "row 5").expect("row emitted");
        let b = row5.bounds().expect("bounds set");
        assert!(b.y0 >= 0.0 && b.y1 <= 200.0, "revealed: {b:?}");
        core.accessibility_action(ak::ActionRequest {
            action: ak::Action::ScrollIntoView,
            target_tree: ak::TreeId::ROOT,
            target_node: *row5_id,
            data: None,
        });
        assert!((core.ui_state.scroll_offset(&cid) - 160.0).abs() < 0.01);
    }

    #[test]
    fn scroll_into_view_walks_nested_containers() {
        use crate::tree::Size;
        // Outer 200px scroll: 100px header + 150px inner scroll of ten
        // 50px rows. "deep 9" needs both containers to move.
        let mut tree = crate::tree::scroll([
            El::new(Kind::Group).height(Size::Fixed(100.0)),
            crate::tree::scroll((0..10).map(|i| {
                button(format!("deep {i}"))
                    .key(format!("d{i}"))
                    .height(Size::Fixed(50.0))
            }))
            .key("inner")
            .height(Size::Fixed(150.0)),
        ])
        .key("outer")
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        fn find_key<'a>(node: &'a El, key: &str) -> Option<&'a El> {
            if node.key.as_deref() == Some(key) {
                return Some(node);
            }
            node.children.iter().find_map(|c| find_key(c, key))
        }
        let outer_cid = tree.computed_id.to_string();
        let inner_cid = find_key(&tree, "inner")
            .expect("inner container")
            .computed_id
            .to_string();

        let mut core = RunnerCore::new();
        core.ui_state = state;
        core.last_tree = Some(tree);
        let up = core.accessibility_tree_update(1.0).expect("tree present");
        let (deep9_id, _) = node_with_label(&up, "deep 9").expect("deep row emitted");

        core.accessibility_action(ak::ActionRequest {
            action: ak::Action::ScrollIntoView,
            target_tree: ak::TreeId::ROOT,
            target_node: *deep9_id,
            data: None,
        });
        // Inner: row at 450..500 in a 150px viewport → its max, 350.
        // Outer: content 250 in 200 → 50 to bring the inner viewport's
        // bottom edge (and the row resting on it) fully on screen.
        assert!((core.ui_state.scroll_offset(&inner_cid) - 350.0).abs() < 0.01);
        assert!((core.ui_state.scroll_offset(&outer_cid) - 50.0).abs() < 0.01);
    }

    #[test]
    fn ids_stay_stable_across_frames_and_prune_dead_nodes() {
        let (tree, state) = lay_out(demo_tree());
        let mut ids = AccessKitIds::default();
        let first = update(&tree, &state, 1.0, &mut ids);
        let second = update(&tree, &state, 1.0, &mut ids);
        let save_first = node_with_label(&first, "Save").unwrap().0;
        let save_second = node_with_label(&second, "Save").unwrap().0;
        assert_eq!(save_first, save_second, "same node, same id");

        // A tree without the button prunes its interning entry.
        let (smaller, state2) = lay_out(column([text("Hello world")]));
        let _ = update(&smaller, &state2, 1.0, &mut ids);
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

    // ---- Consumer-contract tests -------------------------------------
    //
    // accesskit_consumer is the exact tree-walking code inside every
    // platform adapter (AT-SPI, UIA, NSAccessibility), and it enforces
    // the text protocol's hard invariants with `unwrap()`s — a
    // character_lengths sum mismatch or a selection pointing at a
    // non-run panics in the adapter, not in our code. Driving it over
    // our emitted TreeUpdates makes the invariants executable here.

    fn consumer_input_id(update: &ak::TreeUpdate) -> ak::NodeId {
        update
            .nodes
            .iter()
            .find(|(_, n)| matches!(n.role(), ak::Role::TextInput | ak::Role::MultilineTextInput))
            .expect("text input emitted")
            .0
    }

    #[test]
    fn consumer_reads_documents_words_and_selection() {
        let selection = crate::selection::Selection::caret("email", 3);
        let (tree, mut state) = lay_out(column([crate::widgets::text_input::text_input(
            "email", "hi there", &selection,
        )]));
        state.current_selection = selection;
        let mut ids = AccessKitIds::default();
        let update = update(&tree, &state, 1.0, &mut ids);
        let input_id = consumer_input_id(&update);

        let consumer = accesskit_consumer::Tree::new(update, true);
        let node = consumer
            .state()
            .node_by_tree_local_id(input_id, ak::TreeId::ROOT)
            .expect("input node");
        assert!(node.supports_text_ranges());
        assert_eq!(node.document_range().text(), "hi there");

        // The reported caret resolves and sits at USV index 3.
        let focus = node.text_selection_focus().expect("caret resolves");
        assert_eq!(focus.to_global_usv_index(), 3);

        // Word navigation runs on our word_starts tables: from the
        // document start, the next word start is "there" (index 3) —
        // exactly where Ctrl+Right goes.
        let word = node.document_range().start().forward_to_word_start();
        assert_eq!(word.to_global_usv_index(), 3);
    }

    #[test]
    fn consumer_handles_multiline_emoji_and_masking() {
        // Multiline document with an emoji and a ZWJ cluster: the
        // consumer reconstructs the exact text (its slicing panics on
        // any length-table drift) and steps characters by cluster.
        let doc = "ab\ncd👍\nx👨\u{200d}👩\u{200d}👧y";
        let selection = crate::selection::Selection::default();
        let (tree, state) = lay_out(column([crate::widgets::text_area::text_area(
            "notes", doc, &selection,
        )]));
        let mut ids = AccessKitIds::default();
        let update_ml = update(&tree, &state, 1.0, &mut ids);
        let input_id = consumer_input_id(&update_ml);
        let consumer = accesskit_consumer::Tree::new(update_ml, true);
        let node = consumer
            .state()
            .node_by_tree_local_id(input_id, ak::TreeId::ROOT)
            .expect("input node");
        assert_eq!(node.document_range().text(), doc);

        // Line navigation: from the start, end of line 1 is past
        // "ab\n" (the \n belongs to the line).
        let line_end = node.document_range().start().forward_to_line_end();
        assert_eq!(line_end.to_global_usv_index(), 3);

        // Masked input: the consumer sees bullets only.
        use crate::widgets::text_input::{TextInputOpts, text_input_with};
        let selection = crate::selection::Selection::default();
        let (tree, state) = lay_out(column([text_input_with(
            "pw",
            "hunter2",
            &selection,
            TextInputOpts::default().password(),
        )]));
        let mut ids = AccessKitIds::default();
        let update_pw = update(&tree, &state, 1.0, &mut ids);
        let input_id = consumer_input_id(&update_pw);
        let consumer = accesskit_consumer::Tree::new(update_pw, true);
        let node = consumer
            .state()
            .node_by_tree_local_id(input_id, ak::TreeId::ROOT)
            .expect("input node");
        assert_eq!(node.document_range().text(), "•".repeat(7));
    }

    #[test]
    fn consumer_walks_chunked_long_lines_and_empty_fields() {
        // 300 characters on one unwrapped line exceeds the protocol's
        // u8 character indices, so the line is split into chained runs
        // — the consumer must still read one document and one line.
        let long: String = "abcde ".repeat(50);
        let selection = crate::selection::Selection::caret("long", 299);
        let (tree, mut state) = lay_out(column([crate::widgets::text_input::text_input(
            "long", &long, &selection,
        )]));
        state.current_selection = selection;
        let mut ids = AccessKitIds::default();
        let update_long = update(&tree, &state, 1.0, &mut ids);
        let input_id = consumer_input_id(&update_long);
        let (_, input) = find_role(&update_long, ak::Role::TextInput);
        assert!(input.children().len() > 1, "long line chunks into runs");
        let consumer = accesskit_consumer::Tree::new(update_long, true);
        let node = consumer
            .state()
            .node_by_tree_local_id(input_id, ak::TreeId::ROOT)
            .expect("input node");
        assert_eq!(node.document_range().text(), long);
        let focus = node.text_selection_focus().expect("caret resolves");
        assert_eq!(focus.to_global_usv_index(), 299);
        // The chained runs form ONE line: line-end from the start is
        // the document end.
        let line_end = node.document_range().start().forward_to_line_end();
        assert!(line_end.is_document_end());

        // An empty field still has the Text interface: one empty run.
        let selection = crate::selection::Selection::caret("empty", 0);
        let (tree, mut state) = lay_out(column([crate::widgets::text_input::text_input(
            "empty", "", &selection,
        )]));
        state.current_selection = selection;
        let mut ids = AccessKitIds::default();
        let update_empty = update(&tree, &state, 1.0, &mut ids);
        let input_id = consumer_input_id(&update_empty);
        let consumer = accesskit_consumer::Tree::new(update_empty, true);
        let node = consumer
            .state()
            .node_by_tree_local_id(input_id, ak::TreeId::ROOT)
            .expect("input node");
        assert!(
            node.supports_text_ranges(),
            "empty field keeps the Text interface"
        );
        assert_eq!(node.document_range().text(), "");
        let focus = node.text_selection_focus().expect("caret resolves");
        assert_eq!(focus.to_global_usv_index(), 0);
    }

    #[test]
    fn set_text_selection_action_round_trips_to_a_selection_event() {
        let selection = crate::selection::Selection::caret("email", 0);
        let mut core = RunnerCore::new();
        let (tree, mut state) = lay_out(column([crate::widgets::text_input::text_input(
            "email", "hi there", &selection,
        )]));
        state.current_selection = selection;
        core.ui_state = state;
        core.last_tree = Some(tree);
        let update = core.accessibility_tree_update(1.0).expect("tree present");

        let (input_id, input) = find_role(&update, ak::Role::TextInput);
        let run_id = input.children()[0];

        // Screen reader: "select characters 3..8" (the word "there").
        let events = core.accessibility_action(ak::ActionRequest {
            action: ak::Action::SetTextSelection,
            target_tree: ak::TreeId::ROOT,
            target_node: *input_id,
            data: Some(ak::ActionData::SetTextSelection(ak::TextSelection {
                anchor: ak::TextPosition {
                    node: run_id,
                    character_index: 3,
                },
                focus: ak::TextPosition {
                    node: run_id,
                    character_index: 8,
                },
            })),
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, crate::event::UiEventKind::SelectionChanged);
        assert!(events[0].is_route("email"));
        let sel = events[0].selection.as_ref().expect("carries the selection");
        let view = sel.within("email").expect("lands in the field");
        assert_eq!((view.anchor, view.head), (3, 8));

        // The app folds it; the next emitted tree reports the range.
        core.ui_state.current_selection = sel.clone();
        let update = core.accessibility_tree_update(1.0).expect("tree present");
        let (_, input) = find_role(&update, ak::Role::TextInput);
        let reported = input.text_selection().expect("selection reported");
        assert_eq!(reported.anchor.character_index, 3);
        assert_eq!(reported.focus.character_index, 8);
    }

    #[test]
    fn replace_selected_text_action_synthesizes_text_input() {
        let selection = crate::selection::Selection::caret("email", 0);
        let mut core = RunnerCore::new();
        let (tree, mut state) = lay_out(column([crate::widgets::text_input::text_input(
            "email", "hi there", &selection,
        )]));
        state.current_selection = selection;
        core.ui_state = state;
        core.last_tree = Some(tree);
        let update = core.accessibility_tree_update(1.0).expect("tree present");
        let (input_id, _) = find_role(&update, ak::Role::TextInput);

        let events = core.accessibility_action(ak::ActionRequest {
            action: ak::Action::ReplaceSelectedText,
            target_tree: ak::TreeId::ROOT,
            target_node: *input_id,
            data: Some(ak::ActionData::Value("hello".into())),
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, crate::event::UiEventKind::TextInput);
        assert!(events[0].is_route("email"));
        assert_eq!(events[0].text.as_deref(), Some("hello"));

        // The widget's TextInput contract is replace-the-selection —
        // fold it exactly like an app would, with "hi" selected so the
        // replacement is visible.
        let mut value = "hi there".to_string();
        let mut sel = crate::selection::Selection {
            range: Some(crate::selection::SelectionRange {
                anchor: crate::selection::SelectionPoint::new("email", 0),
                head: crate::selection::SelectionPoint::new("email", 2),
            }),
        };
        assert!(crate::widgets::text_input::apply_event(
            &mut value, &mut sel, &events[0], "email",
        ));
        assert_eq!(value, "hello there");
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
