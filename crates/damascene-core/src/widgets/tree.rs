//! Tree view — a flat list of depth-indented rows (VS Code's tree,
//! ARIA `role="tree"` / `role="treeitem"`).
//!
//! # Oracle (`docs/NAMING_ORACLE.md`)
//!
//! shadcn/ui ships no Tree component, so the widget-layer oracle is
//! silent here; recorded justification for the names: ARIA's tree
//! pattern (`role="tree"`, `role="treeitem"`, `aria-level`,
//! `aria-expanded` — [`tree`], [`tree_item`], `depth`,
//! [`TreeItemOpts::expanded`]) and VS Code's flat virtualized tree for
//! the structure (its `AbstractTree` renders every *visible* node as
//! one 22px row in a flat list; nesting is indent arithmetic, not
//! nested containers). Corpus evidence: both blind voice-validation
//! rounds hand-rolled exactly this shape — a `channel_row` /
//! `user_row` pair plus a flatten loop, ~70 lines of private grammar
//! per app (`damascene-workbench/examples/voice.rs`, `voice_v2.rs`) —
//! and `voice_v2` named the gap directly: "core has no tree view".
//!
//! # Shape
//!
//! **A flat list, not nested containers.** The app flattens its
//! hierarchy itself, emitting one [`tree_item`] per *visible* node
//! with an explicit `depth`; collapsed subtrees are simply not
//! emitted. That keeps expansion state where the library philosophy
//! puts all state — in the app: the row renders
//! [`TreeItemOpts::expanded`], clicking it emits the normal keyed
//! `Click`, and the app flips its own bool and rebuilds.
//!
//! Row anatomy, left to right: a disclosure gutter (chevron when
//! `expanded` is `Some`, an empty spacer of the same width for leaves
//! so labels at one depth form a column), an optional leading icon, an
//! ellipsizing label that fills the remaining width, and an optional
//! trailing slot (count chip, status glyphs). Selection treatments
//! compose through the stock chainables: `.selected()` / `.current()`
//! on the returned row.
//!
//! [`tree`] is the zero-gap column wrapper; it wires vertical
//! arrow-key navigation over the rows (Up/Down/Home/End), which is why
//! rows must be its *direct* children — the flat list is load-bearing,
//! not just stylistic. Rows keep their focus ring inside their bounds
//! (flush stacks, issue #119), so `scroll([tree([...])])` needs no
//! ring gutter.
//!
//! **Not to be confused with [`crate::widgets::collapsible`].** The
//! disclosure gutter here belongs to a *hierarchy* row: 22px chrome
//! rung, depth indent, arrow-key navigation, and the app emitting only
//! the visible descendants. A standalone disclosure *section* — a
//! settings group, a sidebar section header, a collapsed reasoning
//! block — is `collapsible(key, label, open, [...])`, a 40px trigger
//! over an indented body. A collapsible section may well contain a
//! tree; a tree row is never a collapsible.
//!
//! ```ignore
//! // A voice client's channel tree. The app owns `expanded` per
//! // channel and emits members only for expanded channels.
//! tree([
//!     tree_item_with(
//!         "channel:general",
//!         0,
//!         "General",
//!         TreeItemOpts::default()
//!             .expanded(true)
//!             .icon("volume-2")
//!             .trailing(badge("4")),
//!     )
//!     .current(),
//!     tree_item_with(
//!         "member:general:ana",
//!         1,
//!         "ana.ruiz",
//!         TreeItemOpts::default().trailing(
//!             icon("mic-off").icon_size(tokens::ICON_XS),
//!         ),
//!     ),
//!     tree_item("member:general:tom", 1, "tom.hedlund"),
//!     tree_item_with(
//!         "channel:afk",
//!         0,
//!         "AFK",
//!         TreeItemOpts::default().expanded(false).icon("volume-2"),
//!     ),
//! ])
//! // Clicking a row emits `UiEventKind::Click` to its key; the app
//! // flips its own expansion / selection state and rebuilds.
//! ```

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::anim::Timing;
use crate::cursor::Cursor;
use crate::icons::svg::IconSource;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::{IntoIconSource, icon};

/// Row height — the 22px chrome rung ([`crate::metrics::ComponentSize::Xxs`]
/// family), which is also VS Code's fixed tree-row height. Trees are
/// the densest region of a shell; they sit on the same rung as the
/// bars.
pub const TREE_ITEM_HEIGHT: f32 = 22.0;

/// Horizontal indent added per `depth` level.
///
/// Recorded divergence from the oracle: VS Code's
/// `workbench.tree.indent` defaults to 8px (range 4–40), legible there
/// because indent guides mark the levels. Damascene draws no indent
/// guides, so the default steps a full [`tokens::SPACE_4`] per level —
/// deep enough that indentation alone carries the hierarchy.
pub const TREE_INDENT: f32 = 16.0;

/// Options for [`tree_item_with`] — the disclosure / icon / trailing
/// slots of a row.
#[derive(Clone, Debug, Default)]
pub struct TreeItemOpts {
    /// `Some(true)` renders a chevron-down disclosure, `Some(false)`
    /// chevron-right, `None` (a leaf) an empty gutter of the same
    /// width so labels at one depth align across branch and leaf rows.
    /// The chevron is display-only — clicks land on the row's key and
    /// the app flips its own state.
    pub expanded: Option<bool>,
    /// Leading glyph after the disclosure gutter (a channel speaker, a
    /// file-type glyph). Rendered at [`tokens::ICON_XS`] in
    /// [`tokens::MUTED_FOREGROUND`].
    pub icon: Option<IconSource>,
    /// Trailing slot at the row's right edge — a count chip, status
    /// glyphs, a cluster in a hugging `row([...])`. The label fills
    /// the width between icon and this slot.
    pub trailing: Option<El>,
}

impl TreeItemOpts {
    /// Mark the row a branch: `true` renders chevron-down (children
    /// visible below), `false` chevron-right (collapsed). Leave unset
    /// for leaves.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Set the leading glyph (see [`TreeItemOpts::icon`]).
    pub fn icon(mut self, source: impl IntoIconSource) -> Self {
        self.icon = Some(source.into_icon_source());
        self
    }

    /// Set the trailing slot (see [`TreeItemOpts::trailing`]).
    pub fn trailing(mut self, trailing: impl Into<El>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }
}

/// The tree container (ARIA `role="tree"`) — a zero-gap column of
/// [`tree_item`] rows with vertical arrow-key navigation
/// (Up/Down/Home/End) wired over them.
///
/// Rows must be **direct children**: the arrow-nav group collects the
/// wrapper's immediate children, which is one reason the tree is a
/// flat list rather than nested containers. Wrap in
/// `scroll([tree([...])])` (or chain `.scrollable()`) for long trees —
/// rows keep their focus ring inside their bounds, so the scroll
/// scissor never clips a ring band.
#[track_caller]
pub fn tree<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("tree"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .arrow_nav_siblings()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_gap(0.0)
}

/// A leaf row (ARIA `role="treeitem"`) — [`tree_item_with`] with no
/// disclosure, icon, or trailing slot. `depth` indents the row by
/// [`TREE_INDENT`] per level (`aria-level`, zero-based); the empty
/// disclosure gutter keeps its label aligned with sibling branch rows.
///
/// Clicks and Enter/Space emit to `key`; chain `.selected()` /
/// `.current()` from the app's own selection state.
#[track_caller]
pub fn tree_item(key: &str, depth: u32, label: impl Into<String>) -> El {
    tree_item_with(key, depth, label, TreeItemOpts::default()).at_loc(Location::caller())
}

/// A tree row with the full slot anatomy: disclosure gutter (per
/// [`TreeItemOpts::expanded`]), optional leading icon, ellipsizing
/// label, optional trailing slot. See [`tree_item`] for the key /
/// depth / selection contract and the module docs for the flat-list
/// shape.
#[track_caller]
pub fn tree_item_with(
    key: &str,
    depth: u32,
    label: impl Into<String>,
    opts: TreeItemOpts,
) -> El {
    let mut children: Vec<El> = Vec::with_capacity(4);

    children.push(match opts.expanded {
        Some(expanded) => icon(if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .icon_size(tokens::ICON_XS)
        .color(tokens::MUTED_FOREGROUND),
        // Leaves reserve the chevron's box so their labels align with
        // branch labels at the same depth.
        None => El::new(Kind::Custom("tree_item_gutter"))
            .width(Size::Fixed(tokens::ICON_XS))
            .height(Size::Fixed(tokens::ICON_XS)),
    });

    if let Some(source) = opts.icon {
        children.push(
            icon(source)
                .icon_size(tokens::ICON_XS)
                .color(tokens::MUTED_FOREGROUND),
        );
    }

    children.push(
        text(label)
            .label()
            .ellipsis()
            .width(Size::Fill(1.0)),
    );

    if let Some(trailing) = opts.trailing {
        children.push(trailing);
    }

    El::new(Kind::Custom("tree_item"))
        .at_loc(Location::caller())
        .key(key)
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::ListItem)
        .focusable()
        .cursor(Cursor::Pointer)
        // Rows stack flush in a zero-gap column (and usually inside a
        // scroll), so the ring stays inside the bounds — the stock
        // recipe for tightly-stacked focusable rows (issue #119).
        .focus_ring_inside()
        .children(children)
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Start)
        .default_radius(tokens::RADIUS_SM)
        .default_gap(tokens::SPACE_1)
        .default_padding(Sides {
            left: tokens::SPACE_2 + depth as f32 * TREE_INDENT,
            right: tokens::SPACE_2,
            top: 0.0,
            bottom: 0.0,
        })
        .default_height(Size::Fixed(TREE_ITEM_HEIGHT))
        .width(Size::Fill(1.0))
        .animate(Timing::SPRING_QUICK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;

    #[test]
    fn tree_item_indent_steps_by_depth_from_a_space_2_base() {
        for depth in [0u32, 1, 2, 5] {
            let row = tree_item("k", depth, "Label");
            assert_eq!(
                row.padding.left,
                tokens::SPACE_2 + depth as f32 * TREE_INDENT,
                "depth {depth}"
            );
            assert_eq!(row.padding.right, tokens::SPACE_2);
            assert_eq!(row.padding.top, 0.0);
            assert_eq!(row.padding.bottom, 0.0);
        }
    }

    #[test]
    fn leaf_gutter_matches_the_chevron_box_so_labels_align() {
        let open = tree_item_with("a", 0, "Open", TreeItemOpts::default().expanded(true));
        let closed = tree_item_with("b", 0, "Closed", TreeItemOpts::default().expanded(false));
        let leaf = tree_item("c", 0, "Leaf");

        // Branch rows lead with a chevron reflecting `expanded`…
        assert_eq!(open.children[0].kind, Kind::Custom("icon"));
        assert_eq!(
            open.children[0].icon,
            Some(IconSource::Builtin(IconName::ChevronDown))
        );
        assert_eq!(
            closed.children[0].icon,
            Some(IconSource::Builtin(IconName::ChevronRight))
        );
        // …leaves with a spacer of the same layout box, so the label
        // column lines up across branch and leaf rows.
        assert_eq!(leaf.children[0].kind, Kind::Custom("tree_item_gutter"));
        assert_eq!(open.children[0].width, Size::Fixed(tokens::ICON_XS));
        assert_eq!(leaf.children[0].width, open.children[0].width);
    }

    #[test]
    fn icon_slot_sits_between_gutter_and_label() {
        let row = tree_item_with(
            "k",
            0,
            "General",
            TreeItemOpts::default().expanded(true).icon("volume-2"),
        );

        assert_eq!(row.children.len(), 3);
        assert_eq!(row.children[1].kind, Kind::Custom("icon"));
        assert_eq!(row.children[1].text_color, Some(tokens::MUTED_FOREGROUND));
        assert_eq!(row.children[2].text.as_deref(), Some("General"));

        let bare = tree_item("k", 0, "General");
        assert_eq!(bare.children.len(), 2, "no icon slot unless asked for");
    }

    #[test]
    fn trailing_slot_hugs_after_the_filling_label() {
        let chipish = text("4").caption().width(Size::Hug);
        let row = tree_item_with(
            "k",
            0,
            "General",
            TreeItemOpts::default().expanded(true).trailing(chipish),
        );

        let label = &row.children[1];
        assert_eq!(label.text.as_deref(), Some("General"));
        assert_eq!(label.text_role, TextRole::Label);
        assert_eq!(label.text_overflow, TextOverflow::Ellipsis);
        assert_eq!(
            label.width,
            Size::Fill(1.0),
            "the label absorbs slack; the trailing slot keeps its own width"
        );
        let trailing = row.children.last().unwrap();
        assert_eq!(trailing.text.as_deref(), Some("4"));
        assert_eq!(trailing.width, Size::Hug, "trailing slot is not resized");
    }

    #[test]
    fn selected_and_current_compose_on_the_row() {
        let selected = tree_item("k", 1, "Row").selected();
        assert_eq!(selected.surface_role, SurfaceRole::Selected);
        assert!(selected.fill.is_some(), "selected rows read as filled");

        let current = tree_item("k", 1, "Row").current();
        assert_eq!(current.surface_role, SurfaceRole::Current);
        assert!(current.fill.is_some(), "current rows read as filled");

        // Composition must not disturb the recipe's geometry.
        assert_eq!(current.height, Size::Fixed(TREE_ITEM_HEIGHT));
        assert_eq!(
            current.padding.left,
            tokens::SPACE_2 + TREE_INDENT,
            "selection treatments leave the indent alone"
        );
    }

    #[test]
    fn tree_wires_vertical_arrow_nav_over_its_rows() {
        let mut t = tree([
            tree_item_with("channel:a", 0, "A", TreeItemOpts::default().expanded(true)),
            tree_item("member:a:x", 1, "x"),
            tree_item("member:a:y", 1, "y"),
        ]);
        assert_eq!(t.arrow_nav, Some(ArrowNav::Vertical));
        assert_eq!(t.gap, 0.0, "rows stack flush");
        assert_eq!(t.align, Align::Stretch);

        // Membership is the wrapper's direct children — prove the
        // wiring end-to-end through layout + the focus walk.
        let mut state = UiState::new();
        layout(&mut t, &mut state, Rect::new(0.0, 0.0, 240.0, 600.0));
        let order = crate::focus::focus_order(&t);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["channel:a", "member:a:x", "member:a:y"]);

        let (mode, members) =
            crate::focus::arrow_nav_group(&t, &order[0].node_id).expect("rows form a nav group");
        assert_eq!(mode, ArrowNav::Vertical);
        assert_eq!(members.len(), 3);
        assert_eq!(members[1].key, "member:a:x");
    }

    #[test]
    fn tree_item_sits_on_the_22px_chrome_rung() {
        let row = tree_item("k", 0, "Row");

        assert_eq!(TREE_ITEM_HEIGHT, 22.0);
        assert_eq!(row.height, Size::Fixed(TREE_ITEM_HEIGHT));
        assert_eq!(row.metrics_role, Some(MetricsRole::ListItem));
        assert_eq!(row.align, Align::Center);
        assert_eq!(row.width, Size::Fill(1.0));
        assert!(row.focusable);
        assert_eq!(row.key.as_deref(), Some("k"));
        assert_eq!(row.cursor, Some(Cursor::Pointer));
        // Flush stack: ring inside, no paint/hit outsets to collide
        // with the neighboring rows or the scroll scissor.
        assert_eq!(row.focus_ring_placement, FocusRingPlacement::Inside);
        assert_eq!(row.paint_overflow, Sides::zero());
        assert_eq!(row.hit_overflow, Sides::zero());
        assert!(
            row.animate_timing().is_some(),
            "selection / hover changes should ease"
        );
    }
}
