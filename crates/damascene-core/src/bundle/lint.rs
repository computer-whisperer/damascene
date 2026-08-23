//! Lint pass — surfaces the kind of issues an LLM iterating on a UI
//! benefits from knowing about, with provenance so the report only
//! flags things the user code can fix.
//!
//! Categories:
//!
//! - **Raw colors / sizes:** values that aren't tokenized. Often fine
//!   inside library code but a smell in user code.
//! - **Overflow:** child rects extending past their parent, or text
//!   exceeding its container's padded content region (centered text
//!   that spills past the padding reads as visually off-center, even
//!   when it nominally fits inside the outer rect).
//! - **Duplicate IDs:** two nodes with the same computed ID (only
//!   possible via explicit `.key(...)` collisions; pure path IDs are
//!   unique by construction).
//!
//! Provenance: every finding records the source location of the
//! offending node (via `#[track_caller]` propagation up to the user's
//! call site). User code is distinguished from damascene's own widget
//! internals by [`Source::from_library`], which a closure-builder
//! site sets explicitly via [`crate::tree::El::from_library`] when
//! `#[track_caller]` won't reach the user. Findings only attribute to
//! sources where `from_library == false`.
//!
//! Overflow findings (rect and text) walk up to the nearest
//! user-source ancestor for attribution. `#[track_caller]` doesn't
//! propagate through closures, so a widget that builds children
//! inside `.map(...)` either forwards the user's caller via
//! `.at_loc(caller)` (the prevailing pattern in damascene-core today) or
//! marks itself with `.from_library()` so the lint walks up to the
//! user's call site. Either way the user gets a finding pointing at
//! their code, not at damascene-core internals. Raw-color and surface
//! lints are still self-attributed — those are intentional inside
//! widgets and should only fire from user code directly.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::fmt::Write as _;

use crate::layout;
use crate::metrics::MetricsRole;
use crate::state::UiState;
use crate::tree::*;

/// A single lint finding.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Finding {
    /// Which lint fired.
    pub kind: FindingKind,
    /// `computed_id` of the node the finding is attributed to (the node
    /// whose [`crate::tree::El::allow_lint`] can suppress it). For
    /// [`FindingKind::DuplicateId`] it is the shared id itself.
    pub node_id: String,
    /// Source location blamed for the finding — the offending node's own
    /// call site, or the nearest user-source ancestor's for findings that
    /// walk up (see the module docs on provenance). `line == 0` means no
    /// source is available (e.g. `DuplicateId`).
    pub source: Source,
    /// Human/LLM-readable description, including the suggested fix.
    pub message: String,
}

/// What a [`Finding`] is about. Each variant documents the failure
/// shape and the canonical fix; pass a kind to
/// [`crate::tree::El::allow_lint`] to suppress it on one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    /// A fill / stroke / text color authored as raw rgba instead of a
    /// token. Only fired when the node itself is user code — raw colors
    /// are intentional inside library widgets.
    RawColor,
    /// A child rect extends past its parent, or text exceeds its box in
    /// a way wrapping/sizing should fix (see the message for the
    /// shape-specific advice).
    Overflow,
    /// Nowrap text clipped by its box, or a dead `.ellipsis()` on
    /// `Size::Hug` text whose rect can never be constrained — fixes are
    /// `.ellipsis()`, `wrap_text()`, or a constrained width.
    TextOverflow,
    /// Two or more nodes share a computed ID — only possible via
    /// explicit `.key(...)` collisions (pure path IDs are unique by
    /// construction). Emitted post-walk with no source attribution;
    /// suppress via [`LintReport::retain`] if ever needed.
    DuplicateId,
    /// A fixed-size visual child pinned to a row's top (or an overlay's
    /// top-left) beside text/content it should center against — the
    /// `items-center` mistake. Fix with `.align(Align::Center)` (plus
    /// `.justify(Justify::Center)` for overlays).
    Alignment,
    /// A row places text less than 4px after an icon/control slot. Add
    /// `.gap(tokens::SPACE_2)` or use a stock menu/list row.
    Spacing,
    /// `surface_role(SurfaceRole::Panel)` on a node with no fill — the
    /// role only paints stroke + shadow, so the surface reads as a
    /// thin border floating over the parent. Either set a fill
    /// (`tokens::CARD` is the usual choice) or — more often — swap to a
    /// widget like `card()` / `sidebar()` that bundles role + fill +
    /// stroke + radius + shadow correctly. (`Raised` is *also*
    /// decorative but this lint stays narrow to `Panel` since
    /// `button(...).ghost()` legitimately produces a Raised node with
    /// no fill.)
    MissingSurfaceFill,
    /// A `column` / `row` / `stack` whose visual recipe matches a stock
    /// widget (card, sidebar, …). Reach for the named widget instead —
    /// it bundles the right surface role, radius, shadow, and content
    /// padding. The structural smells live in the widget catalog README;
    /// this lint catches the two highest-confidence signatures
    /// (`fill=CARD + stroke=BORDER + radius>0` ⇒ `card()`,
    /// `fill=CARD + stroke=BORDER + width=SIDEBAR_WIDTH` without a Panel
    /// surface role ⇒ `sidebar()`).
    ReinventedWidget,
    /// A focusable node's focus-ring band would render obscured at
    /// runtime — either because the nearest clipping ancestor's scissor
    /// cuts it, or because a later-painted node's rect overlaps the
    /// bleed region and paints on top. The occlusion check runs across
    /// container boundaries: wrapping each control in its own
    /// row/column doesn't shield flush neighbors. Nodes in sibling
    /// overlay layers (scrims, dialogs, tooltips over the page) are
    /// not treated as occluders — layers stack on purpose.
    ///
    /// Common fixes:
    ///
    /// - **Clipped:** give the clipping ancestor (or an intermediate
    ///   container) padding ≥ `tokens::RING_WIDTH` on the clipped
    ///   side so the band lives inside the scissor.
    /// - **Occluded:** add gap between the focusable element and the
    ///   neighbor (≥ `tokens::RING_WIDTH`), or restructure so the
    ///   neighbor doesn't sit on the focusable element's edge.
    FocusRingObscured,
    /// A focusable node sits inside a scrolling ancestor whose
    /// scrollbar thumb is currently rendered (content overflows), and
    /// the focusable's rect overlaps the thumb's track on the x-axis
    /// — so the thumb paints on top of the control whenever the user
    /// scrolls to it.
    ///
    /// The trap is that giving the *scroll itself* horizontal padding
    /// (the natural reading of `FocusRingObscured`'s message) shifts
    /// `inner` and the thumb together: padding clears the focus-ring
    /// scissor, but the thumb still sits in the rightmost
    /// `SCROLLBAR_THUMB_WIDTH + SCROLLBAR_TRACK_INSET` pixels of the
    /// children's visible area.
    ///
    /// Fix: move horizontal padding *inside* the scroll, onto a
    /// wrapper that constrains children to a narrower content rect,
    /// so the thumb sits in a reserved gutter to the right of
    /// content.
    ///
    /// Floating menu panels (`SurfaceRole::Popover` scrollables —
    /// `popover_panel`, `dropdown_menu_content`, which scroll once
    /// the positioner has shrunk them) are exempt: menus use overlay
    /// indicators over their rows by platform convention, and the
    /// panel is library-built, so an app using `select_menu` could
    /// not act on the finding anyway.
    ScrollbarObscuresFocusable,
    /// Two keyed nodes have overlapping effective pointer hit targets
    /// because at least one of them opted into `.hit_overflow(...)`.
    /// The check runs across container boundaries — wrapping each
    /// control in its own row/column doesn't shield flush controls —
    /// but skips ancestor/descendant pairs (nested hit targets resolve
    /// innermost-first) and sibling overlay layers (scrims and floating
    /// layers overlap on purpose). Hit-test resolves by paint order, so
    /// the later-painted node silently owns the collision region while
    /// the earlier one may still visually appear nearby.
    ///
    /// Fix: reduce the hit overflow, add real layout gap/padding, or
    /// restructure so one visible row/control owns the whole intended
    /// target area.
    HitOverflowCollision,
    /// `icon("name")` (or `.icon_name("name")`) where the string isn't
    /// in the built-in vocabulary. It resolves to a visible `AlertCircle`
    /// fallback at paint, so the UI doesn't break — but the intended
    /// glyph is silently wrong, and headlessly the only other signal is
    /// a one-line stderr warning that's easy to miss.
    ///
    /// Fix: use a name from [`crate::all_icon_names`], or pass an app
    /// SvgIcon via `SvgIcon::parse_current_color(include_str!(...))`.
    UnknownIconName,
    /// `.tooltip()` on a node that has no `.key()`. Tooltips fire
    /// through the hit-test pipeline, and `hit_test` only returns
    /// keyed nodes — hover skips past unkeyed leaves to the nearest
    /// keyed ancestor (which has a different `computed_id` and a
    /// different tooltip lookup), so the tooltip is silently dead.
    ///
    /// Fix: add `.key("…")` to the same node that carries the
    /// tooltip. For info-only chrome inside list rows (sha cells,
    /// timestamps, chips, identicon avatars) the usual key is a
    /// synthetic one like `"row:{idx}.<part>"` — its only purpose is
    /// to make the tooltip's hover land. Moving the `.tooltip()` to
    /// a keyed ancestor instead conflates "I want a hover popover
    /// here" with "I'm declaring a click/focus target," and is
    /// usually not what you want.
    DeadTooltip,
    /// A node somewhere in the tree carries `.tooltip()`, but the root
    /// is not an `Axis::Overlay` container — so at runtime
    /// `synthesize_tooltip` has nowhere to push the tooltip layer and
    /// hits a `debug_assert` on first hover (the last possible
    /// moment). This is the same condition checked statically, at
    /// `render_bundle` time.
    ///
    /// Fix: wrap your `App::build` return value in `overlays(main,
    /// [])` (or any `stack(...)` root — `stack` is an overlay
    /// container). Attributed to the root, since that's where the fix
    /// goes.
    TooltipWithoutOverlayRoot,
    /// A filled child paints into a rounded ancestor's corner-curve
    /// area without rounding its own matching corner. The child's
    /// flat-cornered fill obscures the parent's curve and stroke,
    /// producing the "sharp corner superimposed on a radiused
    /// container" artifact.
    ///
    /// The canonical recipe (`card_header([...]).fill(MUTED)` inside
    /// `card([...])`) is auto-fixed by the metrics pass — see
    /// [`crate::metrics`]. This lint catches hand-rolled cases:
    /// reinvented cards with reinvented headers, custom inspector
    /// frames, accordion-like containers, etc.
    ///
    /// Fix: set the matching corner radii on the child
    /// (`.radius(Corners::top(N))` for a header strip,
    /// `Corners::bottom(N)` for a footer), or add padding to the
    /// parent so the child is inset from the curve.
    CornerStackup,
    /// A `surface_role=Panel` node whose direct children sit flush
    /// against one or more of its outer edges with no padding
    /// (neither on the panel nor on the touching child) to inset the
    /// content. The canonical trip is `card([...])` called without
    /// the `card_header` / `card_content` / `card_footer` slot
    /// wrappers and without an explicit `.padding(...)`: `card()`
    /// itself carries no inner padding, so titles paint on the top
    /// stroke, action buttons paint on the bottom stroke, and chip
    /// rows pin to the left edge.
    ///
    /// The check is per-side. A side is treated as "padded" — and so
    /// is not flagged — when either the panel itself pads on that
    /// side, or any child whose rect touches that side carries
    /// inward padding on that side. So the canonical anatomy
    /// (`card_header` pads top/left/right, `card_footer` pads
    /// bottom/left/right, both at `SPACE_6`) stays quiet without
    /// special-casing.
    ///
    /// Fixes:
    ///
    /// - Wrap content in the slot anatomy: `card([card_header([...]),
    ///   card_content([...]), card_footer([...])])` — each slot bakes
    ///   the shadcn `SPACE_6` padding recipe.
    /// - For dense list-row cards where the slot padding feels too
    ///   generous, pad the panel itself:
    ///   `card([...]).padding(Sides::all(tokens::SPACE_4))`.
    UnpaddedSurfacePanel,
    /// A text or icon leaf whose rect sits flush against the viewport
    /// (window) edge with no padding on that side. The root-level
    /// sibling of [`Self::UnpaddedSurfacePanel`]: window chrome
    /// shipped without window padding — toolbar contents against the
    /// window edge, headings clipped by rounded window corners. No
    /// surface role is involved, so the panel lint can't see it.
    ///
    /// Emitted once per viewport side, attributed to the first
    /// offending leaf in tree order (padding the root fixes every
    /// leaf at once).
    ///
    /// Fixes:
    ///
    /// - Return `page([...])` from `App::build` — it bakes the
    ///   `tokens::SPACE_4` window padding (and the overlay root
    ///   tooltips need).
    /// - For hand-rolled roots, pad the container the content lives
    ///   in (see `damascene-fixtures/src/hero.rs`).
    /// - Content that *should* run to the edge (a full-bleed footer
    ///   strip) can `.allow_lint(FindingKind::UnpaddedViewportLeaf)`
    ///   on the flagged leaf.
    UnpaddedViewportLeaf,
    /// A `Size::Fill(_)` child collapsed to (nearly) zero content
    /// space because the parent `Hug`s the same axis. A Hug parent
    /// sizes itself from its children's intrinsics, so `Fill` grants
    /// no extra space unless an ancestor stretches the parent — the
    /// child degenerates to its own measured content (for `text_input`
    /// that's just its padding, ~24px). The failure is silent:
    /// nothing overflows, the layout is "valid", the control is just
    /// unusably narrow (issue #120).
    ///
    /// The canonical trip is wrapping a Fill-width control in a bare
    /// `row([...])` (width defaults to Hug) inside a container that
    /// doesn't stretch it — most often an `Axis::Overlay` wrapper,
    /// since overlay containers cap Hug children at their intrinsic
    /// size and never stretch them.
    ///
    /// The check is geometric, not declarative: Fill-inside-Hug is
    /// usually rescued (cross-axis `Align::Stretch` re-measures Hug
    /// wrappers at the ancestor's extent, roots get the viewport), so
    /// only a child whose resolved rect leaves under ~4px of content
    /// space is flagged.
    ///
    /// Fixes:
    ///
    /// - Set `.width(Size::Fill(1.0))` (or a fixed size) on the Hug
    ///   wrapper so it claims the space its Fill child should share.
    /// - If the wrapper sits in an overlay container, prefer a real
    ///   `row`/`column` ancestor — overlay Hug never stretches.
    CollapsedFillChild,
    /// A `plot()` spec declares both top-level `marks` and `lanes`.
    /// Lanes make the plot a lane plot and the top-level marks are
    /// silently ignored at draw time — put every mark inside a
    /// [`Lane`](crate::plot::Lane), or drop the lanes.
    PlotMarksAndLanes,
    /// A lane plot configures the spec-level `y` axis (a non-linear
    /// scale, a title, or `y_autoscale(false)`), which lane plots
    /// ignore: the vertical axis is the lane stack, and each lane
    /// frames its own data via its [`LaneDomain`](crate::plot::LaneDomain)
    /// (`Digital` / `Fixed` via `.y_window(..)` / `Auto`). Configure
    /// the lane, not the axis.
    PlotLaneYAxisIgnored,
    /// A popover/modal layer composed in-flow instead of at the app
    /// root. Every scrim-bearing widget (`popover`, `select_menu`,
    /// `dropdown`, `context_menu`, `modal`, `dialog`) returns a
    /// viewport-filling layer: the scrim is the click-outside dismiss
    /// surface and the layer's rect is the anchored panel's placement
    /// region. Composed as an ordinary child of a column/scroll it
    /// lays out in-flow, the layer's resolved rect is not the
    /// viewport, and the menu silently never appears (issue #142) —
    /// no panic, nothing draws. Geometry is the signal: a
    /// [`Kind::Scrim`] whose resolved rect differs from the viewport
    /// means its layer wasn't composed at the root.
    ///
    /// Fix: hoist the layer to the root —
    /// `overlays(main, [self.menu_open.then(|| select_menu(...))])`
    /// (or any root `stack`). The trigger stays where it is; the menu
    /// finds it by key. See the `widgets/popover.rs` module docs for
    /// the composition contract and `examples/src/bin/popover.rs` for
    /// the canonical shape.
    MisplacedOverlayLayer,
    /// A focusable (interactive) node with no accessible name: no
    /// `.aria_label(...)`, no text content (for roles that take their
    /// name from content, or roleless nodes), and no `.tooltip(...)`
    /// (which doubles as the last-resort name, HTML `title` style). A
    /// screen-reader user Tabs onto it and hears only the role — the
    /// icon-only-button mistake. Also fires, with a different message,
    /// on a focusable node hidden from assistive technology
    /// (`.aria_hidden()` on it or an ancestor): keyboard focus still
    /// lands there but nothing is announced at all.
    ///
    /// Fix: `.aria_label("…")` naming the *action* ("New tab", "Close",
    /// "Previous month"), or a `.tooltip("…")` (sighted users get the
    /// hover hint, AT users get the name). For the hidden-focusable
    /// variant, either drop `.aria_hidden()` or make the node
    /// non-interactive.
    NoAccessibleName,
    /// Image content (`.image(...)` or an explicit [`Role::Img`](crate::Role::Img)) with
    /// no accessible name and not marked decorative. AT announces an
    /// anonymous "image", which is noise for meaningful images and
    /// clutter for decorative ones.
    ///
    /// Fix: `.alt("…")` describing the image's *content* for
    /// meaningful images, or `.aria_hidden()` for purely decorative
    /// ones (backgrounds, flourishes, an avatar image whose name lives
    /// on the labeled wrapper).
    ImageWithoutAlt,
    /// Text whose contrast against its effective background is below
    /// the WCAG 2.1 AA floor (1.4.3): 4.5:1 for normal text, 3:1 for
    /// large text (≥ 24px, or ≥ 18.7px bold). The background is the
    /// theme-resolved composite of ancestor fills in the renderer's
    /// linear working space, so translucent surfaces and alpha'd text
    /// colors are measured as actually painted.
    ///
    /// The check only runs where the background is statically known: it
    /// skips subtrees under reduced `.opacity(...)`, custom shader
    /// fills, image backdrops, and overlay layers until an opaque fill
    /// re-establishes the surface (a dialog's panel does). Disabled
    /// nodes (`aria_disabled` / `.disabled()`) are exempt, as in WCAG.
    ///
    /// Fix: use a stronger foreground token (`tokens::FOREGROUND`,
    /// `SECONDARY_FOREGROUND`, …) or put the text on a surface it was
    /// paired with — foreground tokens are designed against their
    /// matching surface token (`MUTED_FOREGROUND` on `MUTED`, etc.).
    LowContrastText,
    /// An interactive target painted smaller than
    /// [`tokens::MIN_TARGET_SIZE`](crate::theme::tokens::MIN_TARGET_SIZE) (24px, WCAG 2.5.8 Target Size
    /// Minimum) on an axis, with another target close enough that the
    /// standard's spacing exception fails — a 24px circle centered on
    /// the undersized target intersects a neighboring target (or a
    /// neighboring undersized target's circle). Isolated small targets
    /// stay quiet: hit-testing already inflates them to
    /// [`tokens::MIN_TOUCH_TARGET`](crate::theme::tokens::MIN_TOUCH_TARGET) invisibly, and the spacing
    /// exception passes. Packed small targets are the real failure —
    /// the invisible inflation collides and paint order silently
    /// decides the winner.
    ///
    /// Fix: size interactive chrome at `tokens::CONTROL_HEIGHT` or
    /// larger, or add gap until neighbors clear the 24px circle.
    SmallHitTarget,
}

/// Everything the lint pass found in one tree — produced by [`lint`]
/// and carried on the rendered bundle artifacts.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct LintReport {
    /// All findings, in emission order (per-node findings in tree
    /// order, then the post-walk adjacency/duplicate checks).
    pub findings: Vec<Finding>,
}

impl LintReport {
    /// Drop findings for which `pred` returns `false`. The bulk-filter
    /// escape hatch for cases the per-node [`crate::tree::El::allow_lint`]
    /// modifier can't reach — most notably [`FindingKind::DuplicateId`],
    /// which is emitted post-walk and has no single attribution target.
    /// Most apps should prefer `.allow_lint(...)` on the offending node;
    /// reach for this only when whole-class suppression at the bundle
    /// boundary is what you actually want.
    pub fn retain(&mut self, mut pred: impl FnMut(&Finding) -> bool) {
        self.findings.retain(|f| pred(f));
    }

    /// Render the report as text, one `Kind node=<id> file:line ::
    /// message` line per finding (`"no findings\n"` when empty).
    pub fn text(&self) -> String {
        if self.findings.is_empty() {
            return "no findings\n".to_string();
        }
        let mut s = String::new();
        for f in &self.findings {
            let _ = writeln!(
                s,
                "{kind:?} node={id} {source} :: {msg}",
                kind = f.kind,
                id = f.node_id,
                source = if f.source.line == 0 {
                    "<no-source>".to_string()
                } else {
                    format!("{}:{}", short_path(f.source.file), f.source.line)
                },
                msg = f.message,
            );
        }
        s
    }
}

/// Run the lint pass over `root`.
///
/// Findings are gated on whether the offending node (or its nearest
/// ancestor) was constructed in user code rather than inside damascene's
/// own widget closures. The signal is [`Source::from_library`], set
/// explicitly via [`crate::tree::El::from_library`] at any closure-
/// builder site that doesn't forward `Location::caller()` back to the
/// user. The vast majority of nodes propagate user source through
/// `#[track_caller]` and pass straight through.
///
/// `theme` is the theme the tree will be painted with — the contrast
/// lint resolves color tokens against its palette the same way
/// `draw_ops` does, so a light-palette app is measured against its
/// light surfaces, not the compile-time dark fallback rgb.
pub fn lint(root: &El, ui_state: &UiState, theme: &crate::theme::Theme) -> LintReport {
    let mut r = LintReport::default();
    let mut seen_ids: std::collections::BTreeMap<String, usize> = Default::default();
    let mut flat = FlatTree::new();
    let a11y = A11yWalk {
        hidden: false,
        disabled: false,
        // Hosts clear to the theme-resolved page background; that is
        // the surface everything composites onto.
        bg: Some(KnownBg::from_color(
            theme.resolve(crate::tokens::BACKGROUND),
        )),
    };
    walk(
        root,
        None,
        None,
        &ClipCtx::None,
        FlatTree::ROOT_LAYER,
        a11y,
        theme,
        ui_state,
        &mut r,
        &mut seen_ids,
        &mut flat,
    );
    // Adjacency checks run over the flattened paint-order set rather
    // than per-parent sibling lists, so controls wrapped in their own
    // layout containers (the `row([label, control])`-per-field shape)
    // are still cross-checked — wrapper boundaries don't shield flush
    // controls (issue #37).
    check_hit_overflow_collisions(&flat, &mut r);
    check_focus_ring_occluded(&flat, &mut r);
    check_small_hit_targets(&flat, &mut r);
    for (id, n) in seen_ids {
        if n > 1 {
            r.findings.push(Finding {
                kind: FindingKind::DuplicateId,
                node_id: id.clone(),
                source: Source::default(),
                message: format!("{n} nodes share id {id}"),
            });
        }
    }
    check_tooltip_overlay_root(root, &mut r);
    check_misplaced_overlay_layers(root, &mut r);
    check_unpadded_viewport_leaves(root, &mut r);
    check_unknown_icon_names(&flat, &mut r);
    check_lane_plot_specs(&flat, &mut r);
    r
}

/// Lane-plot spec conflicts: top-level marks alongside lanes (the marks
/// are silently ignored at draw time), and spec-level `y` axis
/// configuration on a lane plot (the vertical axis is the lane stack, so
/// scale/title/autoscale are ignored — configure the lane instead).
fn check_lane_plot_specs(flat: &FlatTree, r: &mut LintReport) {
    for f in flat.nodes.iter() {
        let Some(spec) = &f.el.plot_source else {
            continue;
        };
        if !spec.is_lane_plot() || !is_from_user(f.el.source) {
            continue;
        }
        if !spec.marks.is_empty() {
            push_for(
                r,
                f.el,
                Finding {
                    kind: FindingKind::PlotMarksAndLanes,
                    node_id: f.el.computed_id.clone().to_string(),
                    source: f.el.source,
                    message: format!(
                        "plot spec declares {} top-level mark(s) AND {} lane(s) — a lane \
                         plot ignores top-level marks. Move each mark into a Lane \
                         (Lane::new(label).mark(...)), or drop the lanes",
                        spec.marks.len(),
                        spec.lanes.len(),
                    ),
                },
            );
        }
        let default_y = crate::plot::Axis::default();
        if spec.y.scale != default_y.scale || spec.y.title.is_some() || !spec.y_autoscale {
            push_for(
                r,
                f.el,
                Finding {
                    kind: FindingKind::PlotLaneYAxisIgnored,
                    node_id: f.el.computed_id.clone().to_string(),
                    source: f.el.source,
                    message: "lane plot configures the spec-level y axis (scale/title/\
                              y_autoscale), which lane plots ignore — the vertical axis is \
                              the lane stack. Frame data per lane instead: Lane::digital, \
                              .y_window(min, max), or the default per-lane autoscale"
                        .into(),
                },
            );
        }
    }
}

/// `icon("name")` with a name outside the built-in vocabulary — it
/// paints a fallback `AlertCircle`, so a typo'd or hallucinated icon
/// name surfaces in review instead of only as an stderr warning.
fn check_unknown_icon_names(flat: &FlatTree, r: &mut LintReport) {
    for f in flat.nodes.iter() {
        if !is_from_user(f.el.source) {
            continue;
        }
        if let Some(crate::icons::svg::IconSource::UnknownName(name)) = &f.el.icon {
            push_for(
                r,
                f.el,
                Finding {
                    kind: FindingKind::UnknownIconName,
                    node_id: f.el.computed_id.clone().to_string(),
                    source: f.el.source,
                    message: format!(
                        "unknown icon name `{name}` — rendering AlertCircle. Use a name from all_icon_names(), or pass an app SvgIcon via SvgIcon::parse_current_color(include_str!(...))"
                    ),
                },
            );
        }
    }
}

/// Text/icon leaves flush against the viewport edge with no padding on
/// that side — window chrome shipped without window padding. The root
/// always carries the full viewport rect (`layout_post_assign` inserts
/// it), so the root rect *is* the window frame. Geometry does the
/// accumulated-padding bookkeeping: any ancestor padding on a side
/// insets every descendant off that edge, so a leaf can only touch the
/// edge when the whole chain above it is unpadded there.
///
/// One finding per side, attributed to the first offending leaf in
/// tree order — padding the root fixes all of them, so per-leaf
/// emission would only repeat the same message. Single-node trees are
/// skipped (a bare `text(...)` smoke-rendered through `render_bundle`
/// has no window anatomy to fix), and scroll/virtual-list subtrees are
/// not descended into — their content rects shift with the scroll
/// offset and are clipped by the scroll viewport, so flush coordinates
/// there are coincidence, not window anatomy.
fn check_unpadded_viewport_leaves<'a>(root: &'a El, r: &mut LintReport) {
    const PAD_EPS: f32 = 0.5;
    let touch_eps = crate::tokens::RING_WIDTH;
    let vp = root.computed_rect;
    if vp.w <= PAD_EPS || vp.h <= PAD_EPS {
        return;
    }

    // First offending (leaf, blame) per side: top, right, bottom, left.
    let mut found: [Option<(&'a El, Source)>; 4] = [None; 4];

    fn rec<'a>(
        n: &'a El,
        blame: Option<Source>,
        is_root: bool,
        vp: Rect,
        touch_eps: f32,
        found: &mut [Option<(&'a El, Source)>; 4],
    ) {
        const PAD_EPS: f32 = 0.5;
        let self_blame = if is_from_user(n.source) {
            Some(n.source)
        } else {
            blame
        };
        let is_content_leaf =
            n.text.is_some() || n.icon.is_some() || matches!(n.kind, Kind::Inlines | Kind::Math);
        if is_content_leaf && !is_root {
            let rect = n.computed_rect;
            if rect.w > PAD_EPS && rect.h > PAD_EPS {
                let sides = [
                    ((rect.y - vp.y).abs() <= touch_eps, n.padding.top, 0usize),
                    (
                        (vp.right() - rect.right()).abs() <= touch_eps,
                        n.padding.right,
                        1,
                    ),
                    (
                        (vp.bottom() - rect.bottom()).abs() <= touch_eps,
                        n.padding.bottom,
                        2,
                    ),
                    ((rect.x - vp.x).abs() <= touch_eps, n.padding.left, 3),
                ];
                for (touches, own_pad, side) in sides {
                    if touches && own_pad <= PAD_EPS && found[side].is_none() {
                        found[side] = Some((n, self_blame.unwrap_or(n.source)));
                    }
                }
            }
        }
        if matches!(n.kind, Kind::Inlines) {
            // Inline children carry intentionally zero-size rects; the
            // Inlines block itself holds the geometry and was checked.
            return;
        }
        if n.scrollable || matches!(n.kind, Kind::Scroll | Kind::VirtualList) {
            // Scrolled content lives in content space: its rects are
            // clipped by the scroll viewport and shift with the scroll
            // offset, so a leaf landing flush against the window edge
            // is coincidence, not missing window padding. Any
            // `.scrollable()` node counts (the stock menu panels), not
            // just `scroll()`.
            return;
        }
        for c in &n.children {
            rec(c, self_blame, false, vp, touch_eps, found);
        }
    }
    rec(root, None, true, vp, touch_eps, &mut found);

    const SIDE_NAMES: [&str; 4] = ["top", "right", "bottom", "left"];
    let mut emitted: Vec<*const El> = Vec::new();
    for (side, entry) in found.iter().enumerate() {
        let Some((leaf, blame)) = entry else { continue };
        if emitted.contains(&std::ptr::from_ref(*leaf)) {
            continue; // one leaf flush on several sides → one finding
        }
        emitted.push(std::ptr::from_ref(*leaf));
        let sides: Vec<&str> = (side..4)
            .filter(|&j| matches!(found[j], Some((l, _)) if std::ptr::eq(l, *leaf)))
            .map(|j| SIDE_NAMES[j])
            .collect();
        push_for(
            r,
            leaf,
            Finding {
                kind: FindingKind::UnpaddedViewportLeaf,
                node_id: leaf.computed_id.clone().to_string(),
                source: *blame,
                message: format!(
                    "text/icon content sits flush against the viewport {} edge with no \
                     padding on that side — window chrome needs window padding. Return \
                     `page([...])` from `App::build` (it bakes tokens::SPACE_4 window \
                     padding), or pad the root container.",
                    sides.join("/"),
                ),
            },
        );
    }
}

/// Popover/modal layers composed in-flow instead of at the app root
/// (issue #142). The failure is silent — the layer lays out as an
/// ordinary child, the anchored panel has no placement region, and the
/// menu never draws — so geometry is the detector: every scrim is
/// built `fill_size()` inside a viewport-filling layer, so a
/// [`Kind::Scrim`] whose resolved rect differs from the viewport means
/// its layer wasn't composed at the root. Attributed to the scrim's
/// parent — the El the user's `popover`/`select_menu`/`modal` call
/// returned, so `.allow_lint(..)` on that value suppresses. The
/// message names any scrolling/clipping ancestor context, since a
/// scrolled column is the composition mistake's usual home.
fn check_misplaced_overlay_layers(root: &El, r: &mut LintReport) {
    const EPS: f32 = 1.0;
    let vp = root.computed_rect;
    if vp.w <= EPS || vp.h <= EPS {
        return;
    }

    fn rec(
        n: &El,
        blame: Option<Source>,
        under_scroll_or_clip: bool,
        vp: Rect,
        r: &mut LintReport,
    ) {
        const EPS: f32 = 1.0;
        let self_blame = if is_from_user(n.source) {
            Some(n.source)
        } else {
            blame
        };
        for c in &n.children {
            if !matches!(c.kind, Kind::Scrim) {
                continue;
            }
            let rect = c.computed_rect;
            let misplaced = (rect.x - vp.x).abs() > EPS
                || (rect.y - vp.y).abs() > EPS
                || (rect.w - vp.w).abs() > EPS
                || (rect.h - vp.h).abs() > EPS;
            if !misplaced {
                continue;
            }
            let context = if under_scroll_or_clip {
                " (an ancestor here scrolls or clips, which also scissors the layer)"
            } else {
                ""
            };
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::MisplacedOverlayLayer,
                    node_id: n.computed_id.clone().to_string(),
                    source: self_blame.unwrap_or(n.source),
                    message: format!(
                        "popover/modal layer resolved to {w:.0}x{h:.0} at ({x:.0},{y:.0}) instead \
                         of the {vw:.0}x{vh:.0} viewport — it is composed in-flow, so the menu/\
                         panel never appears{context}. Scrim-bearing layers (popover, select_menu, \
                         dropdown, context_menu, modal, dialog) compose at the app root: \
                         overlays(main, [self.open.then(|| ...)]) or a root stack. See the \
                         widgets/popover.rs module docs",
                        w = rect.w,
                        h = rect.h,
                        x = rect.x,
                        y = rect.y,
                        vw = vp.w,
                        vh = vp.h,
                    ),
                },
            );
        }
        let under = under_scroll_or_clip || n.scrollable || n.clip;
        for c in &n.children {
            rec(c, self_blame, under, vp, r);
        }
    }
    rec(root, None, false, vp, r);
}

/// `.tooltip()` (and any other layer-synthesizing state) needs the root
/// to be an `Axis::Overlay` container — `synthesize_tooltip` pushes the
/// tooltip layer as a root child and `debug_assert`s the axis at
/// hover-time. Check it statically: one finding, attributed to the
/// root, naming the first tooltip carrier. Mirrors the runtime assert's
/// message so both paths teach the same fix.
fn check_tooltip_overlay_root(root: &El, r: &mut LintReport) {
    if root.axis == Axis::Overlay {
        return;
    }
    fn first_tooltip(n: &El) -> Option<&El> {
        if n.tooltip.is_some() {
            return Some(n);
        }
        n.children.iter().find_map(first_tooltip)
    }
    let Some(carrier) = first_tooltip(root) else {
        return;
    };
    push_for(
        r,
        root,
        Finding {
            kind: FindingKind::TooltipWithoutOverlayRoot,
            node_id: root.computed_id.clone().to_string(),
            source: root.source,
            message: format!(
                "a node carries .tooltip() (first: {carrier_id} at {file}:{line}) but the \
                 root is not an Axis::Overlay container, so the tooltip layer has nowhere \
                 to mount — at runtime this panics on first hover. Wrap your `App::build` \
                 return value in `overlays(main, [])`. Got root axis = {axis:?}",
                carrier_id = carrier.computed_id,
                file = short_path(carrier.source.file),
                line = carrier.source.line,
                axis = root.axis,
            ),
        },
    );
}

fn is_from_user(source: Source) -> bool {
    !source.from_library
}

/// Append `finding` to `r` unless `target` opted out of this finding's
/// kind via [`El::allow_lint`]. `target` must be the node whose
/// `computed_id` equals `finding.node_id` — i.e. the lint's attribution
/// target. Centralizing the check here keeps every emission site honest:
/// suppression is strictly per-attributed-node, never inherited from a
/// parent or shared across siblings.
fn push_for(r: &mut LintReport, target: &El, finding: Finding) {
    debug_assert_eq!(
        finding.node_id, *target.computed_id,
        "lint::push_for: target must be the finding's attribution node",
    );
    if target
        .allow_lint
        .as_ref()
        .is_some_and(|list| list.contains(&finding.kind))
    {
        return;
    }
    r.findings.push(finding);
}

/// Background color known to sit behind a node's content, tracked
/// through `walk` for the contrast lint. `rgb` lives in the renderer's
/// linear working space (compositing there matches what actually lands
/// on screen); `token` names the palette token when the color still is
/// one (an opaque token fill), for readable finding messages.
#[derive(Clone, Copy)]
struct KnownBg {
    rgb: [f32; 3],
    token: Option<&'static str>,
}

impl KnownBg {
    /// A theme-resolved color as an opaque backdrop.
    fn from_color(c: crate::color::Color) -> Self {
        Self {
            rgb: linear_rgb(c),
            token: c.token,
        }
    }

    /// Source-over composite `c` (theme-resolved, straight alpha) onto
    /// this backdrop.
    fn under(self, c: crate::color::Color) -> Self {
        let a = c.a.clamp(0.0, 1.0);
        if a >= 0.999 {
            return Self::from_color(c);
        }
        let top = linear_rgb(c);
        Self {
            rgb: [
                top[0] * a + self.rgb[0] * (1.0 - a),
                top[1] * a + self.rgb[1] * (1.0 - a),
                top[2] * a + self.rgb[2] * (1.0 - a),
            ],
            token: None,
        }
    }

    /// `#rrggbb` of the composited backdrop (sRGB-encoded) for finding
    /// messages, with the token name when the color still is one.
    fn describe(self) -> String {
        let hex = srgb_hex(self.rgb);
        match self.token {
            Some(t) => format!("`{t}` ({hex})"),
            None => hex,
        }
    }
}

/// Accessibility facts propagated through `walk` for the a11y lints.
#[derive(Clone, Copy)]
struct A11yWalk {
    /// Self-or-ancestor `.aria_hidden()` — the subtree is invisible to
    /// assistive technology.
    hidden: bool,
    /// Self-or-ancestor `aria_disabled` — WCAG exempts inactive
    /// controls from the contrast requirement.
    disabled: bool,
    /// Composited backdrop behind this node's content, when statically
    /// known. `None` under reduced opacity, shader/image backdrops, and
    /// overlay layers that haven't re-established an opaque surface —
    /// the contrast lint stays quiet there rather than guessing.
    bg: Option<KnownBg>,
}

/// Linear-working-space rgb of a (theme-resolved) color.
fn linear_rgb(c: crate::color::Color) -> [f32; 3] {
    let lin = c.convert_to(crate::color::ColorSpace::SRGB_LINEAR);
    [lin.r, lin.g, lin.b]
}

/// WCAG 2.x relative luminance of linear sRGB rgb. Channels clamp to
/// the SDR range — the AA thresholds are defined there.
fn relative_luminance(rgb: [f32; 3]) -> f32 {
    let r = rgb[0].clamp(0.0, 1.0);
    let g = rgb[1].clamp(0.0, 1.0);
    let b = rgb[2].clamp(0.0, 1.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio between two linear-rgb colors, `>= 1.0`.
fn contrast_ratio(a: [f32; 3], b: [f32; 3]) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `#rrggbb` of a linear-rgb color, sRGB-encoded for display.
fn srgb_hex(rgb: [f32; 3]) -> String {
    let c = crate::color::Color::srgb_linear(rgb[0], rgb[1], rgb[2], 1.0)
        .convert_to(crate::color::ColorSpace::SRGB);
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// Clipping context propagated through `walk`. Carries the nearest
/// clipping ancestor's scissor rect and, for scrollable ancestors,
/// the axis along which content can be scrolled into view (clipping
/// on that axis is benign — focus rings on partially-clipped rows
/// become visible after auto-scroll-on-focus). The scrolling variant
/// also carries the ancestor's `node_id` so descendant checks can
/// look up its `thumb_tracks` entry to detect scrollbar/control
/// overlap (`ScrollbarObscuresFocusable`).
#[derive(Clone)]
enum ClipCtx {
    None,
    /// Non-scrolling clip — the rect cuts on every side.
    Static(Rect),
    /// Scrolling clip — the rect cuts on the cross axis only;
    /// `scroll_axis` records the axis where overflow becomes scroll
    /// (Column = vertical, Row = horizontal).
    Scrolling {
        rect: Rect,
        scroll_axis: Axis,
        node_id: String,
        /// The scrollable is a floating menu surface
        /// (`SurfaceRole::Popover`): its thumb overlays the rows by
        /// platform convention, so `ScrollbarObscuresFocusable` is
        /// not reported for its children.
        overlay_thumb: bool,
    },
}

/// One entry of the flattened paint-order index built during `walk`
/// and consumed by the post-walk adjacency checks
/// ([`check_hit_overflow_collisions`], [`check_focus_ring_occluded`]).
struct FlatNode<'a> {
    el: &'a El,
    rect: Rect,
    /// Exclusive end of this node's subtree in [`FlatTree::nodes`] —
    /// node `j` is a descendant of node `i` iff `i < j <
    /// nodes[i].subtree_end`.
    subtree_end: usize,
    /// Overlay-layer id (index into [`FlatTree::layer_parents`]).
    layer: usize,
    /// Clip context this node paints under (the nearest clipping
    /// ancestor's scissor).
    clip: ClipCtx,
    /// Nearest user-source attribution — the node's own source when it
    /// is from user code, otherwise the closest user-source ancestor's.
    blame: Option<Source>,
}

/// Flattened tree in pre-order, which is paint order: a larger index
/// paints later (on top). Built once per `lint` run alongside the
/// recursive `walk`, so clip/blame propagation can't drift from the
/// per-node checks.
struct FlatTree<'a> {
    nodes: Vec<FlatNode<'a>>,
    /// Overlay-layer tree: each entry holds the parent layer of that
    /// id. Layer [`Self::ROOT_LAYER`] is the root; descending into
    /// each child of an `Axis::Overlay` container opens a fresh layer
    /// parented to the container's own.
    layer_parents: Vec<Option<usize>>,
}

impl<'a> FlatTree<'a> {
    const ROOT_LAYER: usize = 0;

    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            layer_parents: vec![None],
        }
    }

    /// Open a fresh overlay layer parented to `parent`, returning its id.
    fn push_layer(&mut self, parent: usize) -> usize {
        self.layer_parents.push(Some(parent));
        self.layer_parents.len() - 1
    }

    /// True when the two nodes do *not* sit in sibling overlay layers —
    /// i.e. one layer is an ancestor-or-self of the other. Sibling
    /// layers (a scrim vs. the dialog above it, the main page vs. the
    /// tooltip layer) stack on purpose, so adjacency checks skip those
    /// pairs. Everything else — including a node inside an inline
    /// `stack(...)` vs. a node outside it — is comparable.
    fn layers_comparable(&self, a: usize, b: usize) -> bool {
        self.is_layer_ancestor_or_self(a, b) || self.is_layer_ancestor_or_self(b, a)
    }

    fn is_layer_ancestor_or_self(&self, anc: usize, mut layer: usize) -> bool {
        loop {
            if layer == anc {
                return true;
            }
            match self.layer_parents[layer] {
                Some(p) => layer = p,
                None => return false,
            }
        }
    }

    /// True when `nodes[j]` lies inside `nodes[i]`'s subtree.
    fn is_descendant(&self, i: usize, j: usize) -> bool {
        j > i && j < self.nodes[i].subtree_end
    }
}

#[allow(clippy::too_many_arguments)]
fn walk<'a>(
    n: &'a El,
    parent_kind: Option<&Kind>,
    parent_blame: Option<Source>,
    nearest_clip: &ClipCtx,
    layer: usize,
    a11y: A11yWalk,
    theme: &crate::theme::Theme,
    ui_state: &UiState,
    r: &mut LintReport,
    seen: &mut std::collections::BTreeMap<String, usize>,
    flat: &mut FlatTree<'a>,
) {
    *seen.entry(n.computed_id.to_string()).or_default() += 1;
    let computed = n.computed_rect;

    let from_user_self = is_from_user(n.source);
    // Nearest user-source location attributable to this node — itself
    // when self is from user code, otherwise the closest ancestor's
    // user source. Used by overflow findings so widget-composed leaves
    // (e.g. `tab_trigger` built inside `tabs_list`'s `.map(...)`
    // closure, where `Location::caller()` resolves inside damascene-core)
    // still blame the user code that supplied the offending content.
    let self_blame = if from_user_self {
        Some(n.source)
    } else {
        parent_blame
    };

    // Record this node in the flattened paint-order index for the
    // post-walk adjacency checks; `subtree_end` is patched after the
    // children below have been visited.
    let flat_idx = flat.nodes.len();
    flat.nodes.push(FlatNode {
        el: n,
        rect: computed,
        subtree_end: usize::MAX,
        layer,
        clip: nearest_clip.clone(),
        blame: self_blame,
    });

    // Children of an Inlines paragraph are encoded into one
    // AttributedText draw op by draw_ops; their individual rects are
    // intentionally zero-size. Skip the per-text overflow + per-child
    // overflow checks for them — the paragraph as a whole holds the
    // rect, so any overflow lint applies at the Inlines node level.
    let inside_inlines = matches!(parent_kind, Some(Kind::Inlines));

    // Raw colors are intentional inside library widgets; only flag
    // them when the node is itself in user code.
    if from_user_self {
        if let Some(c) = n.fill
            && c.token.is_none()
            && c.a > 0.0
        {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::RawColor,
                    node_id: n.computed_id.clone().to_string(),
                    source: n.source,
                    message: format!(
                        "fill is a raw rgba({},{},{},{}) — use a token",
                        c.r, c.g, c.b, c.a
                    ),
                },
            );
        }
        if let Some(c) = n.stroke
            && c.token.is_none()
            && c.a > 0.0
        {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::RawColor,
                    node_id: n.computed_id.clone().to_string(),
                    source: n.source,
                    message: format!(
                        "stroke is a raw rgba({},{},{},{}) — use a token",
                        c.r, c.g, c.b, c.a
                    ),
                },
            );
        }
        if let Some(c) = n.text_color
            && c.token.is_none()
            && c.a > 0.0
        {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::RawColor,
                    node_id: n.computed_id.clone().to_string(),
                    source: n.source,
                    message: format!(
                        "text_color is a raw rgba({},{},{},{}) — use a token",
                        c.r, c.g, c.b, c.a
                    ),
                },
            );
        }
        // `.tooltip()` on an unkeyed node — silently dead, because
        // hit-test only returns keyed nodes, so hover never lands on
        // this leaf and `synthesize_tooltip` never reads its text.
        // Same "modifier requires unrelated state to take effect"
        // shape as the dead-`.ellipsis()` finding below.
        if n.tooltip.is_some() && n.key.is_none() {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::DeadTooltip,
                    node_id: n.computed_id.clone().to_string(),
                    source: n.source,
                    message: ".tooltip() on a node without .key() never fires — hit-test only \
                         returns keyed nodes, so hover skips past this leaf to the nearest \
                         keyed ancestor. Add .key(\"…\") on the same node that carries the \
                         tooltip; for info-only chrome inside list rows, a synthetic key \
                         like \"row:{idx}.<part>\" is enough."
                        .to_string(),
                },
            );
        }

        // SurfaceRole::Panel only paints stroke + shadow on top of the
        // node's existing fill. Without a fill, the surface reads as a
        // thin border over BACKGROUND — the classic "invisible panel"
        // mistake. Suggest the right widget. (Raised is also
        // decorative but `button(...).ghost()` legitimately leaves a
        // Raised node with no fill, so the lint stays narrow.)
        if n.fill.is_none() && matches!(n.surface_role, SurfaceRole::Panel) {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::MissingSurfaceFill,
                    node_id: n.computed_id.clone().to_string(),
                    source: n.source,
                    message:
                        "surface_role(Panel) without a fill paints only stroke + shadow — \
                         wrap in card() / sidebar() / dialog() for the canonical recipe, or set .fill(tokens::CARD)"
                            .to_string(),
                },
            );
        }

        if matches!(n.surface_role, SurfaceRole::Panel) {
            check_unpadded_surface_panel(n, computed, r, n.source);
        }

        // Reinvented widgets: a plain Group whose visual recipe matches
        // a stock widget. The signatures stay narrow on purpose — both
        // require the canonical token pair (fill = CARD, stroke =
        // BORDER) and a structural marker (a non-zero radius for card,
        // an exact SIDEBAR_WIDTH for sidebar). The real widgets escape
        // these checks: `card()` returns Kind::Card, and `sidebar()`
        // sets surface_role(Panel) — so neither stock widget trips its
        // own lint when the user calls them directly.
        //
        // Skip empty Groups — a `column(Vec::<El>::new())` styled with
        // CARD/BORDER is a pure visual swatch (color sample, divider
        // stub) that's not pretending to be a card. Card-mimics
        // always wrap content.
        if matches!(n.kind, Kind::Group) && !n.children.is_empty() {
            let card_fill = n
                .fill
                .as_ref()
                .and_then(|c| c.token)
                .is_some_and(|t| t == "card");
            let border_stroke = n
                .stroke
                .as_ref()
                .and_then(|c| c.token)
                .is_some_and(|t| t == "border");
            if card_fill && border_stroke {
                let is_panel_surface = matches!(n.surface_role, SurfaceRole::Panel);
                let sidebar_width = matches!(n.width, Size::Fixed(w) if (w - crate::tokens::SIDEBAR_WIDTH).abs() < 0.5);
                if !is_panel_surface {
                    if sidebar_width {
                        push_for(
                            r,
                            n,
                            Finding {
                                kind: FindingKind::ReinventedWidget,
                                node_id: n.computed_id.clone().to_string(),
                                source: n.source,
                                message:
                                    "Group with fill=CARD, stroke=BORDER, width=SIDEBAR_WIDTH reinvents sidebar() — \
                                     use sidebar([sidebar_header(...), sidebar_group([sidebar_menu([sidebar_menu_button(label, current)])])]) \
                                     for the panel surface and the canonical row recipe"
                                        .to_string(),
                            },
                        );
                    } else {
                        // Any other Group with the canonical card-tone
                        // pair is a hand-rolled card-or-aside surface.
                        // Both the "boxed" case (non-zero radius, fits
                        // inside another container) and the "side panel"
                        // case (full-height inspector pane) collapse
                        // into the same recipe — `card([...])` bundles
                        // it. Mention sidebar() too, since for full-bleed
                        // panels with custom widths (e.g. inspector
                        // rails) the right answer might be sidebar().
                        push_for(
                            r,
                            n,
                            Finding {
                                kind: FindingKind::ReinventedWidget,
                                node_id: n.computed_id.clone().to_string(),
                                source: n.source,
                                message:
                                    "Group with fill=CARD, stroke=BORDER reinvents the panel-surface recipe — \
                                     use card([card_header([card_title(\"...\")]), card_content([...])]) / titled_card(\"Title\", [...]) for boxed content, \
                                     or sidebar([...]) for a full-height nav/inspector pane (sidebar() also handles the custom-width case via .width(Size::Fixed(...)))"
                                        .to_string(),
                            },
                        );
                    }
                }
            }
        }
    }

    // ---- Accessibility (arc 2b) ------------------------------------
    // Attribution follows the overflow convention: blame the nearest
    // user-source ancestor, so widget-composed leaves point at the
    // user's call site.
    let a11y_props = n.a11y.as_deref();
    let a11y_hidden = a11y.hidden || a11y_props.is_some_and(|p| p.hidden);
    let a11y_disabled = a11y.disabled || a11y_props.is_some_and(|p| p.disabled);
    let a11y_role = a11y_props.and_then(|p| p.role);

    // The backdrop this node's own content paints on: its own fill
    // composites over the inherited background first. Reduced opacity,
    // shader fills, and image backdrops make the result statically
    // unknowable — the contrast lint goes quiet instead of guessing.
    let own_bg = if n.opacity < 0.999 || n.shader_override.is_some() {
        None
    } else {
        let composited = match n.fill.map(|c| theme.resolve(c)) {
            Some(f) if f.a > 0.0 => match a11y.bg {
                _ if f.a >= 0.999 => Some(KnownBg::from_color(f)),
                Some(bg) => Some(bg.under(f)),
                None => None,
            },
            _ => a11y.bg,
        };
        if n.image.is_some() { None } else { composited }
    };

    // NoAccessibleName: a focusable control a screen-reader user can
    // Tab to but that announces nothing useful — or nothing at all,
    // when it is also `aria_hidden`.
    if n.focusable
        && let Some(blame) = self_blame
    {
        if a11y_hidden {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::NoAccessibleName,
                    node_id: n.computed_id.clone().to_string(),
                    source: blame,
                    message: "focusable node is aria_hidden (on itself or an ancestor) — \
                              keyboard focus still lands here but assistive technology \
                              announces nothing. Drop .aria_hidden() or make the node \
                              non-interactive"
                        .to_string(),
                },
            );
        } else if crate::a11y::accessible_name(n).is_none()
            // Navigable content surfaces (plots / scenes as `Figure`,
            // `viewport()` as `Group`) and keyed table rows (`Row`,
            // read cell by cell) are focusable for keyboard operation
            // (#144), not controls — a name is welcome, not required,
            // as with an HTML `<figure tabindex="0">`.
            && !matches!(
                a11y_role,
                Some(crate::a11y::Role::Figure | crate::a11y::Role::Group | crate::a11y::Role::Row)
            )
        {
            let role_hint = match a11y_role {
                Some(role) => format!("{role:?}"),
                None => "control".to_string(),
            };
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::NoAccessibleName,
                    node_id: n.computed_id.clone().to_string(),
                    source: blame,
                    message: format!(
                        "focusable {role_hint} has no accessible name — a screen reader \
                         announces only the role. Add .aria_label(\"…\") naming the action, \
                         or a .tooltip(\"…\") (doubles as the name)",
                    ),
                },
            );
        }
    }

    // ImageWithoutAlt: image content that is neither named nor marked
    // decorative — announced as an anonymous "image".
    if (n.image.is_some() || a11y_role == Some(crate::a11y::Role::Img))
        && !a11y_hidden
        && crate::a11y::accessible_name(n).is_none()
        && let Some(blame) = self_blame
    {
        push_for(
            r,
            n,
            Finding {
                kind: FindingKind::ImageWithoutAlt,
                node_id: n.computed_id.clone().to_string(),
                source: blame,
                message: "image has no accessible name — add .alt(\"…\") describing its \
                          content, or .aria_hidden() if it is decorative"
                    .to_string(),
            },
        );
    }

    // LowContrastText: WCAG 2.1 AA (1.4.3) against the composited
    // backdrop, measured in the renderer's linear working space.
    // Disabled controls are exempt (as in WCAG); unknown backdrops and
    // reduced-opacity subtrees were handled by `own_bg` above.
    if let Some(bg) = own_bg
        && !a11y_disabled
        && n.opacity >= 0.999
        && n.text.as_deref().is_some_and(|t| !t.trim().is_empty())
        && let Some(blame) = self_blame
    {
        let tc = theme.resolve(n.text_color.unwrap_or(crate::tokens::FOREGROUND));
        if tc.a > 0.0 {
            // Translucent text reads through to the backdrop: composite
            // first, as WCAG prescribes.
            let painted = bg.under(tc.with_alpha(tc.a.min(1.0)));
            let ratio = contrast_ratio(painted.rgb, bg.rgb);
            let large = n.font_size >= 24.0
                || (n.font_size >= 18.66 && matches!(n.font_weight, FontWeight::Bold));
            let required = if large { 3.0 } else { 4.5 };
            if ratio + 0.01 < required {
                let fg_desc = match tc.token {
                    Some(t) => format!("`{t}` ({})", srgb_hex(linear_rgb(tc))),
                    None => srgb_hex(linear_rgb(tc)),
                };
                push_for(
                    r,
                    n,
                    Finding {
                        kind: FindingKind::LowContrastText,
                        node_id: n.computed_id.clone().to_string(),
                        source: blame,
                        message: format!(
                            "text {fg_desc} on {bg_desc} has contrast {ratio:.2}:1, below the \
                             WCAG AA floor of {required}:1 for {size_class} text — use a \
                             stronger foreground token (tokens::FOREGROUND, …) or the surface \
                             the token was designed against",
                            bg_desc = bg.describe(),
                            size_class = if large { "large" } else { "normal" },
                        ),
                    },
                );
            }
        }
    }

    // Row alignment: mirror CSS flex's default `align-items: stretch`,
    // but catch the common UI-row mistake where a fixed-size visual
    // child (icon/badge/control) is pinned to the row top beside a
    // text sibling. The fix is the familiar `items-center` move:
    // `.align(Align::Center)`.
    if let Some(blame) = self_blame {
        lint_row_alignment(n, computed, r, blame);
        lint_overlay_alignment(n, computed, r, blame);
        lint_row_visual_text_spacing(n, r, blame);
    }

    // Text overflow: detect at the node itself (with the node's own
    // padding-aware content region — text_w includes padding so the
    // check fires when the text exceeds the padded content area, not
    // just the bare rect). Attribute to the nearest user-source
    // ancestor so closure-built widget leaves still blame user code.
    if n.text.is_some()
        && !inside_inlines
        && let Some(blame) = self_blame
    {
        let available_width = match n.text_wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => Some(computed.w),
        };
        if let Some(text_layout) = layout::text_layout(n, available_width) {
            let text_w = text_layout.width + n.padding.left + n.padding.right;
            let text_h = text_layout.height + n.padding.top + n.padding.bottom;
            let raw_overflow_x = (text_w - computed.w).max(0.0);
            let overflow_x = if matches!(
                (n.text_wrap, n.text_overflow),
                (TextWrap::NoWrap, TextOverflow::Ellipsis)
            ) {
                0.0
            } else {
                raw_overflow_x
            };
            let overflow_y = (text_h - computed.h).max(0.0);
            if overflow_x > 0.5 || overflow_y > 0.5 {
                let is_clipped_nowrap = overflow_x > 0.5
                    && matches!(
                        (n.text_wrap, n.text_overflow),
                        (TextWrap::NoWrap, TextOverflow::Clip)
                    );
                let kind = if is_clipped_nowrap {
                    FindingKind::TextOverflow
                } else {
                    FindingKind::Overflow
                };
                // Shape-specific advice. A Y-only overflow on a
                // fixed-height box where the text alone would have fit
                // is caused by padding eating the height; "use
                // paragraph() / wrap_text() / a wider box" is the
                // wrong fix. The trap that produces it most often is
                // `.padding(scalar)` going through `From<f32> for
                // Sides` as `Sides::all(scalar)` on a control-height
                // box where the author meant `Sides::xy(scalar, 0)`.
                let pad_y = n.padding.top + n.padding.bottom;
                let height_is_fixed = matches!(n.height, Size::Fixed(_));
                let text_alone_fits_height = text_layout.height <= computed.h + 0.5;
                let padding_eats_fixed_height = overflow_y > 0.5
                    && overflow_x <= 0.5
                    && pad_y > 0.0
                    && text_alone_fits_height
                    && height_is_fixed;
                let cell_h = text_layout.height;
                let box_h = computed.h;
                let message = if kind == FindingKind::TextOverflow {
                    format!(
                        "nowrap text exceeds its box by X={overflow_x:.0}; use .ellipsis(), wrap_text(), or a wider box"
                    )
                } else if padding_eats_fixed_height {
                    let inner_h = (box_h - pad_y).max(0.0);
                    let pad_x_token = if (n.padding.left - n.padding.right).abs() < 0.5 {
                        format!("{:.0}", n.padding.left)
                    } else {
                        "...".to_string()
                    };
                    let control_h = crate::tokens::CONTROL_HEIGHT;
                    format!(
                        "vertical padding ({pad_y:.0}px) makes the inner content rect ({inner_h:.0}px) shorter than the text cell ({cell_h:.0}px) on a fixed-height box ({box_h:.0}px) — \
                         the label can't vertically center and paints into the padding band, off-center by Y={overflow_y:.0}. \
                         Reduce vertical padding (e.g. `Sides::xy({pad_x_token}, 0.0)` — `.padding(scalar)` is `Sides::all(scalar)`, which usually isn't what you want on a control-height box) or increase height (tokens::CONTROL_HEIGHT = {control_h:.0}px)"
                    )
                } else if overflow_y > 0.5 && overflow_x <= 0.5 {
                    format!(
                        "text cell ({cell_h:.0}px) exceeds box height ({box_h:.0}px) by Y={overflow_y:.0}; \
                         increase height, reduce text size, or use paragraph()/wrap_text() with fewer lines"
                    )
                } else {
                    format!(
                        "text content exceeds its box by X={overflow_x:.0} Y={overflow_y:.0}; use paragraph()/wrap_text(), a wider box, or explicit clipping"
                    )
                };
                push_for(
                    r,
                    n,
                    Finding {
                        kind,
                        node_id: n.computed_id.clone().to_string(),
                        source: blame,
                        message,
                    },
                );
            }
        }
    }

    // Overflow: child rect extends past parent. Scrollable parents
    // overflow their content on the main axis by design — that's the
    // whole point — so don't flag children of a scroll viewport.
    // `clip=true` is the general "this container handles overflow by
    // visually truncating" signal — text_input clips its inner group,
    // diff split halves clip at the half boundary, code blocks clip
    // long lines, etc. Author intent here is explicit, so suppress.
    // Inlines parents intentionally zero-size their children (the
    // paragraph paints them as one AttributedText), so per-child rect
    // checks would always fire — suppress. The runtime-synthesized
    // toast_stack uses a custom layout that pins cards to the
    // viewport regardless of its own (parent-allocated) rect, so its
    // children naturally extend past the layer's bounds — also
    // suppress.
    let suppress_overflow = n.scrollable
        || n.clip
        || matches!(n.kind, Kind::Inlines)
        || matches!(n.kind, Kind::Custom("toast_stack"));

    // Dead-ellipsis detection: when this parent's flex layout overran
    // on its main axis, any `Size::Hug` child with `NoWrap + Ellipsis`
    // has a dead truncation chain. `layout::main_size_of` returns
    // `MainSize::Resolved(intrinsic)` for `Size::Hug`, so the child's
    // rect width on the main axis always equals its natural content
    // width — and that's the exact value `draw_ops` passes as the
    // budget to `ellipsize_text_with_family`. Without a constrained
    // rect the truncation branch never trims a glyph. We compute
    // overrun once per parent and flag matching children below.
    let parent_main_overran = !suppress_overflow && flex_main_axis_overflowed(n, computed);

    // Update the nearest-clipping-ancestor rect for descendants. The
    // scissor in `draw_ops` uses `inner_painted_rect` (the layout
    // rect, no padding inset, no overflow outset), so this rect is
    // the right bound to compare descendant ring bands against.
    // Scrollable clips suppress clipping findings on the scroll axis
    // (auto-scroll-on-focus reveals partially-clipped rows there).
    let child_clip = if n.clip {
        if n.scrollable {
            ClipCtx::Scrolling {
                rect: computed,
                scroll_axis: n.axis,
                node_id: n.computed_id.clone().to_string(),
                overlay_thumb: matches!(n.surface_role, crate::tree::SurfaceRole::Popover),
            }
        } else {
            ClipCtx::Static(computed)
        }
    } else {
        nearest_clip.clone()
    };

    for (child_idx, c) in n.children.iter().enumerate() {
        let from_user_child = is_from_user(c.source);
        let child_blame = if from_user_child {
            Some(c.source)
        } else {
            self_blame
        };

        let c_rect = c.computed_rect;
        if !suppress_overflow
            && !rect_contains(computed, c_rect, 0.5)
            && let Some(blame) = child_blame
        {
            let dx_left = (computed.x - c_rect.x).max(0.0);
            let dx_right = (c_rect.right() - computed.right()).max(0.0);
            let dy_top = (computed.y - c_rect.y).max(0.0);
            let dy_bottom = (c_rect.bottom() - computed.bottom()).max(0.0);
            push_for(
                r,
                c,
                Finding {
                    kind: FindingKind::Overflow,
                    node_id: c.computed_id.clone().to_string(),
                    source: blame,
                    message: format!(
                        "child overflows parent {parent_id} by L={dx_left:.0} R={dx_right:.0} T={dy_top:.0} B={dy_bottom:.0}",
                        parent_id = n.computed_id,
                    ),
                },
            );
        }

        // Dead `.ellipsis()` chain on a Hug child of an overran flex
        // parent (see comment on `parent_main_overran` above). Point
        // at the text directly so the user knows which fix to make:
        // the existing per-child Overflow finding fires on the
        // *displaced* sibling, not on the offending Hug text.
        let main_axis_is_hug = match n.axis {
            Axis::Row => matches!(c.width, Size::Hug),
            Axis::Column => matches!(c.height, Size::Hug),
            Axis::Overlay => false,
        };
        if parent_main_overran
            && main_axis_is_hug
            && c.text.is_some()
            && c.text_wrap == TextWrap::NoWrap
            && c.text_overflow == TextOverflow::Ellipsis
            && let Some(blame) = child_blame
        {
            push_for(
                r,
                c,
                Finding {
                    kind: FindingKind::TextOverflow,
                    node_id: c.computed_id.clone().to_string(),
                    source: blame,
                    message:
                        ".ellipsis() has no effect on Size::Hug text — Hug forces the rect to the intrinsic content width, so the truncation budget equals the content and no glyph is ever trimmed. Set Size::Fill(_) or Size::Fixed(_) on the text or on a wrapping container so the layout can constrain the rect."
                            .to_string(),
                },
            );
        }

        // Collapsed Fill child on a Hug parent axis (issue #120). The
        // declared pair alone is far too eager — the library's own
        // showcase has dozens of Fill-inside-Hug children that render
        // fine because cross-axis stretch (or the viewport) rescues
        // the Hug wrapper — so the gate is geometric: the child's
        // resolved rect must have collapsed to (nearly) nothing but
        // its own padding. Spacers collapse by design when a row has
        // no slack, so they're exempt.
        if !matches!(c.kind, Kind::Spacer)
            && let Some(blame) = child_blame
        {
            let axes = [
                (
                    matches!(n.axis, Axis::Row | Axis::Overlay)
                        && matches!(n.width, Size::Hug)
                        && matches!(c.width, Size::Fill(_)),
                    "width",
                    c_rect.w,
                    c.padding.left + c.padding.right,
                ),
                (
                    matches!(n.axis, Axis::Column | Axis::Overlay)
                        && matches!(n.height, Size::Hug)
                        && matches!(c.height, Size::Fill(_)),
                    "height",
                    c_rect.h,
                    c.padding.top + c.padding.bottom,
                ),
            ];
            for (applies, axis_name, extent, pad) in axes {
                if !applies || extent - pad >= 4.0 {
                    continue;
                }
                push_for(
                    r,
                    c,
                    Finding {
                        kind: FindingKind::CollapsedFillChild,
                        node_id: c.computed_id.clone().to_string(),
                        source: blame,
                        message: format!(
                            "Size::Fill {axis_name} collapsed to {extent:.0}px ({content:.0}px of content space) — parent {parent_id} Hugs this axis, so Fill grants only the child's intrinsic size. Set Size::Fill(1.0) {axis_name} on the wrapper (Hug wrappers are only rescued when an ancestor stretches them; Axis::Overlay parents never do)",
                            content = (extent - pad).max(0.0),
                            parent_id = n.computed_id,
                        ),
                    },
                );
            }
        }

        // Corner stackup: a filled child paints into a rounded
        // parent's corner-curve area, obscuring the parent's stroke
        // and curve with a flat corner. The canonical card_header /
        // card_footer recipe is auto-fixed by `metrics`; this check
        // catches the same pattern in hand-rolled containers. Gated
        // on the child being from user code so library widgets that
        // legitimately paint in corner regions don't trip it.
        if from_user_child
            && c.fill.is_some()
            && n.radius.any_nonzero()
            && let Some(blame) = child_blame
        {
            check_corner_stackup(n, computed, c, c_rect, r, blame);
        }

        if from_user_child
            && c.focusable
            && let Some(blame) = child_blame
        {
            check_focus_ring_clipped(c, c_rect, &child_clip, r, blame);
            // Independent of paint_overflow: the focusable's own rect
            // overlaps an ancestor scroll's thumb track (the thumb
            // paints on top of the control whenever it's visible).
            check_scrollbar_overlap(c, c_rect, &child_clip, ui_state, r, blame);
        }

        // Each child of an overlay container starts a fresh overlay
        // layer — sibling layers stack on purpose, so the post-walk
        // adjacency checks skip pairs that diverge at one.
        let child_layer = if matches!(n.axis, Axis::Overlay) {
            flat.push_layer(layer)
        } else {
            layer
        };

        // Overlay layers past the first paint over whatever the earlier
        // siblings put down — statically unknowable pixels, so the
        // contrast lint's backdrop resets to unknown until an opaque
        // fill (a dialog panel, a menu surface) re-establishes it.
        let child_a11y = A11yWalk {
            hidden: a11y_hidden,
            disabled: a11y_disabled,
            bg: if matches!(n.axis, Axis::Overlay) && child_idx > 0 {
                None
            } else {
                own_bg
            },
        };

        walk(
            c,
            Some(&n.kind),
            child_blame,
            &child_clip,
            child_layer,
            child_a11y,
            theme,
            ui_state,
            r,
            seen,
            flat,
        );
    }

    flat.nodes[flat_idx].subtree_end = flat.nodes.len();
}

fn focus_ring_overflow(n: &El) -> Sides {
    match n.focus_ring_placement {
        crate::tree::FocusRingPlacement::Outside => Sides::all(crate::tokens::RING_WIDTH),
        crate::tree::FocusRingPlacement::Inside => Sides::zero(),
    }
}

/// True when any side exceeds the half-pixel epsilon — used both for
/// `.hit_overflow(...)` bands and focus-ring bleed bands.
fn any_side_overflows(sides: Sides) -> bool {
    sides.left > 0.5 || sides.right > 0.5 || sides.top > 0.5 || sides.bottom > 0.5
}

fn clip_rect(ctx: &ClipCtx) -> Option<Rect> {
    match ctx {
        ClipCtx::None => None,
        ClipCtx::Static(rect) | ClipCtx::Scrolling { rect, .. } => Some(*rect),
    }
}

fn clipped_rect(rect: Rect, ctx: &ClipCtx) -> Option<Rect> {
    match clip_rect(ctx) {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    }
}

/// WCAG 2.5.8 Target Size (Minimum): interactive targets painted below
/// [`tokens::MIN_TARGET_SIZE`](crate::tokens::MIN_TARGET_SIZE) on an
/// axis, checked with the standard's *spacing exception* — an
/// undersized target passes when a `MIN_TARGET_SIZE`-diameter circle
/// centered on its (clipped) rect intersects no other target's rect
/// and no other *undersized* target's circle. Isolated small targets
/// therefore stay quiet (hit-testing additionally inflates them to
/// `MIN_TOUCH_TARGET` invisibly); tightly packed ones fire, because
/// there the invisible inflation collides and paint order silently
/// picks a winner. Pair filtering mirrors the other adjacency checks:
/// sibling overlay layers and ancestor/descendant pairs are skipped.
fn check_small_hit_targets(flat: &FlatTree, r: &mut LintReport) {
    let min = crate::tokens::MIN_TARGET_SIZE;
    let radius = min * 0.5;

    // Visible extent of a target under its clip, for size purposes. A
    // scrolling ancestor clips only on its cross axis: a row
    // half-scrolled past the viewport edge is not a small target —
    // focusing it auto-scrolls it into view, so its content-space
    // extent is the honest measure on the scroll axis.
    fn target_rect(rect: Rect, ctx: &ClipCtx) -> Option<Rect> {
        match ctx {
            ClipCtx::None => Some(rect),
            ClipCtx::Static(clip) => rect.intersect(*clip),
            ClipCtx::Scrolling {
                rect: clip,
                scroll_axis,
                ..
            } => {
                let clipped = rect.intersect(*clip)?;
                Some(match scroll_axis {
                    Axis::Column => Rect::new(clipped.x, rect.y, clipped.w, rect.h),
                    Axis::Row | Axis::Overlay => Rect::new(rect.x, clipped.y, rect.w, clipped.h),
                })
            }
        }
    }

    // Interactive pointer targets: keyed (hit-test only returns keyed
    // nodes) and focusable (the activation contract), visibly painted.
    // `visible` (fully clipped) is what can actually be tapped right
    // now and is the geometry blockers are measured with — a target
    // scrolled out of view can't steal taps. `size` is scroll-axis
    // unclipped and is what the undersized check reads.
    let targets: Vec<(usize, Rect, Rect)> = flat
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, f)| f.el.key.is_some() && f.el.focusable)
        .filter_map(|(i, f)| {
            let visible = clipped_rect(f.rect, &f.clip)?;
            let size = target_rect(f.rect, &f.clip)?;
            (visible.w > 0.5 && visible.h > 0.5).then_some((i, visible, size))
        })
        .collect();

    for &(i, visible, size) in &targets {
        if size.w >= min - 0.5 && size.h >= min - 0.5 {
            continue;
        }
        let node = &flat.nodes[i];
        let (cx, cy) = (visible.center_x(), visible.center_y());

        let blocker = targets.iter().find_map(|&(j, other_visible, other_size)| {
            if j == i
                || flat.is_descendant(i, j)
                || flat.is_descendant(j, i)
                || !flat.layers_comparable(node.layer, flat.nodes[j].layer)
            {
                return None;
            }
            // Circle vs. the other target's rect…
            let dx = cx - other_visible.x.max(cx.min(other_visible.right()));
            let dy = cy - other_visible.y.max(cy.min(other_visible.bottom()));
            let hits_rect = dx * dx + dy * dy <= radius * radius;
            // …and circle vs. the other *undersized* target's circle.
            let other_small = other_size.w < min - 0.5 || other_size.h < min - 0.5;
            let hits_circle = other_small && {
                let ox = other_visible.center_x() - cx;
                let oy = other_visible.center_y() - cy;
                ox * ox + oy * oy <= (radius * 2.0) * (radius * 2.0)
            };
            (hits_rect || hits_circle)
                .then(|| flat.nodes[j].el.key.as_deref().unwrap_or("<unkeyed>"))
        });

        let Some(other_key) = blocker else {
            continue;
        };
        let Some(blame) = node.blame else {
            continue;
        };
        push_for(
            r,
            node.el,
            Finding {
                kind: FindingKind::SmallHitTarget,
                node_id: node.el.computed_id.clone().to_string(),
                source: blame,
                message: format!(
                    "interactive target is {w:.0}x{h:.0}px — below the {min:.0}px WCAG \
                     target-size floor — and target `{other_key}` sits inside its {min:.0}px \
                     clearance circle, so the invisible touch-target inflation collides and \
                     paint order picks the winner. Size interactive chrome at \
                     tokens::CONTROL_HEIGHT or larger, or add gap until neighbors clear \
                     the circle",
                    w = size.w,
                    h = size.h,
                ),
            },
        );
    }
}

/// Detect hit-target ambiguity introduced by `.hit_overflow`. Plain
/// visual overlap is not this lint's concern; it only fires when an
/// explicitly expanded hit rect reaches another keyed node's
/// visual/effective target. The comparison runs over the flattened
/// keyed set, so controls wrapped in their own layout containers (the
/// `row([label, control])`-per-field shape) are still cross-checked —
/// wrapper boundaries don't shield flush controls (issue #37). Two
/// pair classes are skipped: sibling overlay layers (overlapping hit
/// regions are normal for scrims, modals, and floating layers) and
/// ancestor/descendant pairs (hit-test resolves nested keyed nodes
/// innermost-first by construction).
fn check_hit_overflow_collisions(flat: &FlatTree, r: &mut LintReport) {
    for (left_idx, left) in flat.nodes.iter().enumerate() {
        if left.el.key.is_none() {
            continue;
        }
        let Some(left_hit) = clipped_rect(left.rect.outset(left.el.hit_overflow), &left.clip)
        else {
            continue;
        };
        for (right_idx, right) in flat.nodes.iter().enumerate().skip(left_idx + 1) {
            if right.el.key.is_none() {
                continue;
            }
            if !any_side_overflows(left.el.hit_overflow)
                && !any_side_overflows(right.el.hit_overflow)
            {
                continue;
            }
            if flat.is_descendant(left_idx, right_idx)
                || !flat.layers_comparable(left.layer, right.layer)
            {
                continue;
            }
            let Some(right_hit) =
                clipped_rect(right.rect.outset(right.el.hit_overflow), &right.clip)
            else {
                continue;
            };
            let Some(overlap) = left_hit.intersect(right_hit) else {
                continue;
            };
            if overlap.w <= 0.5 || overlap.h <= 0.5 {
                continue;
            }

            let left_visual_contains = left.rect.contains(overlap.center_x(), overlap.center_y());
            let right_visual_contains = right.rect.contains(overlap.center_x(), overlap.center_y());
            if left_visual_contains && right_visual_contains {
                // Existing visual overlap is already ambiguous by
                // construction; this lint is about invisible inflation
                // creating a new ambiguous band.
                continue;
            }

            let earlier = left.el.key.as_deref().unwrap_or("<unkeyed>");
            let later = right.el.key.as_deref().unwrap_or("<unkeyed>");
            let owner = if any_side_overflows(right.el.hit_overflow) {
                right
            } else {
                left
            };
            let Some(blame) = owner.blame else {
                continue;
            };
            push_for(
                r,
                owner.el,
                Finding {
                    kind: FindingKind::HitOverflowCollision,
                    node_id: owner.el.computed_id.clone().to_string(),
                    source: blame,
                    message: format!(
                        "expanded hit targets for keys `{earlier}` and `{later}` overlap by {w:.0}x{h:.0}px — \
                         hit-test resolves the collision by paint order, so `{later}` owns that invisible band. \
                         Reduce `.hit_overflow(...)`, add real gap/padding, or make one visible row/control own the full intended target.",
                        w = overlap.w,
                        h = overlap.h,
                    ),
                },
            );
        }
    }
}

/// Detect the corner-stackup pattern: a filled child whose rect
/// overlaps one of a rounded parent's corner-curve boxes without
/// matching that corner's radius. Mirrors the geometric test the
/// painter actually performs — the parent's rounded-rect SDF leaves
/// the `r×r` square at each rounded corner partially transparent, and
/// a child fill that overlaps that square paints sharp corners over
/// the parent's curve and stroke.
fn check_corner_stackup(
    parent: &El,
    parent_rect: Rect,
    child: &El,
    child_rect: Rect,
    r: &mut LintReport,
    blame: Source,
) {
    let pr = parent.radius;
    let cr = child.radius;
    // (parent_radius, child_radius, corner-curve box in parent space)
    let tl = (
        pr.tl,
        cr.tl,
        Rect::new(parent_rect.x, parent_rect.y, pr.tl, pr.tl),
    );
    let tr = (
        pr.tr,
        cr.tr,
        Rect::new(
            parent_rect.x + parent_rect.w - pr.tr,
            parent_rect.y,
            pr.tr,
            pr.tr,
        ),
    );
    let br = (
        pr.br,
        cr.br,
        Rect::new(
            parent_rect.x + parent_rect.w - pr.br,
            parent_rect.y + parent_rect.h - pr.br,
            pr.br,
            pr.br,
        ),
    );
    let bl = (
        pr.bl,
        cr.bl,
        Rect::new(
            parent_rect.x,
            parent_rect.y + parent_rect.h - pr.bl,
            pr.bl,
            pr.bl,
        ),
    );
    let leaks_at = |(p_r, c_r, corner_box): (f32, f32, Rect)| -> bool {
        if p_r <= 0.5 || c_r + 0.5 >= p_r {
            return false;
        }
        match child_rect.intersect(corner_box) {
            Some(overlap) => overlap.w >= 0.5 && overlap.h >= 0.5,
            None => false,
        }
    };
    let (leak_tl, leak_tr, leak_br, leak_bl) =
        (leaks_at(tl), leaks_at(tr), leaks_at(br), leaks_at(bl));
    if !(leak_tl || leak_tr || leak_br || leak_bl) {
        return;
    }
    let (descriptor, helper) = match (leak_tl, leak_tr, leak_br, leak_bl) {
        (true, true, false, false) => ("the parent's top corners", "Corners::top(...)"),
        (false, false, true, true) => ("the parent's bottom corners", "Corners::bottom(...)"),
        (true, false, false, true) => ("the parent's left corners", "Corners::left(...)"),
        (false, true, true, false) => ("the parent's right corners", "Corners::right(...)"),
        (true, true, true, true) => ("the parent's corners", "Corners::all(...)"),
        // Single corner or any L-shape: author picks the matching field set.
        _ => (
            "a parent corner",
            "Corners { tl, tr, br, bl } with the matching corner set",
        ),
    };
    push_for(
        r,
        child,
        Finding {
            kind: FindingKind::CornerStackup,
            node_id: child.computed_id.clone().to_string(),
            source: blame,
            message: format!(
                "filled child paints into {descriptor} (rounded parent, max radius={pr_max:.0}) — \
                 the flat corners obscure the parent's curve and stroke. \
                 Set `.radius({helper})` on the child so its corners follow the parent's curve, \
                 or add padding to the parent so the child is inset from the curve.",
                pr_max = pr.max(),
            ),
        },
    );
}

/// Detects [`FindingKind::UnpaddedSurfacePanel`]: a Panel surface
/// whose direct children sit flush against one or more outer edges
/// with no padding to inset them. Per-side rule: a side is "safe"
/// when either the panel itself pads on that side, or some child
/// whose rect touches that side carries inward padding on that side.
/// That keeps the canonical `card([card_header, card_content,
/// card_footer])` anatomy quiet (header pads top/left/right at
/// `SPACE_6`; footer pads bottom/left/right at `SPACE_6`) while
/// flagging `card([row(...).width(Fill(1.0)), button_row])` and
/// other bare-panel + Fill-children shapes.
fn check_unpadded_surface_panel(panel: &El, panel_rect: Rect, r: &mut LintReport, blame: Source) {
    // Match the issue spec: a child rect within `RING_WIDTH` of an
    // outer edge counts as flush against it.
    let touch_eps = crate::tokens::RING_WIDTH;
    // Half a pixel of inward padding is enough to clear `touch_eps`
    // and inset content from the edge.
    const PAD_EPS: f32 = 0.5;

    // Per-side state: (any child touches, any touching child pads inward).
    let mut top = (false, false);
    let mut right = (false, false);
    let mut bottom = (false, false);
    let mut left = (false, false);

    for c in &panel.children {
        let cr = c.computed_rect;
        if cr.w <= PAD_EPS || cr.h <= PAD_EPS {
            // Zero-area children can't be flush against anything.
            continue;
        }
        if (cr.y - panel_rect.y).abs() <= touch_eps {
            top.0 = true;
            if c.padding.top > PAD_EPS {
                top.1 = true;
            }
        }
        if (panel_rect.right() - cr.right()).abs() <= touch_eps {
            right.0 = true;
            if c.padding.right > PAD_EPS {
                right.1 = true;
            }
        }
        if (panel_rect.bottom() - cr.bottom()).abs() <= touch_eps {
            bottom.0 = true;
            if c.padding.bottom > PAD_EPS {
                bottom.1 = true;
            }
        }
        if (cr.x - panel_rect.x).abs() <= touch_eps {
            left.0 = true;
            if c.padding.left > PAD_EPS {
                left.1 = true;
            }
        }
    }

    let pad = panel.padding;
    let mut sides: Vec<&'static str> = Vec::new();
    if pad.top <= PAD_EPS && top.0 && !top.1 {
        sides.push("top");
    }
    if pad.right <= PAD_EPS && right.0 && !right.1 {
        sides.push("right");
    }
    if pad.bottom <= PAD_EPS && bottom.0 && !bottom.1 {
        sides.push("bottom");
    }
    if pad.left <= PAD_EPS && left.0 && !left.1 {
        sides.push("left");
    }
    if sides.is_empty() {
        return;
    }
    let joined = sides.join("/");
    push_for(
        r,
        panel,
        Finding {
            kind: FindingKind::UnpaddedSurfacePanel,
            node_id: panel.computed_id.clone().to_string(),
            source: blame,
            message: format!(
                "Panel-surface children sit flush against the {joined} edge — \
                 wrap content in the slot anatomy (`card_header(...)` / `card_content(...)` / `card_footer(...)` \
                 each bake `SPACE_6` padding), or pad the panel itself \
                 (e.g. `.padding(Sides::all(tokens::SPACE_4))` for dense list-row cards).",
            ),
        },
    );
}

/// Detect [`FindingKind::FocusRingObscured`]'s clipping half: the
/// focus ring's bleed band cut by the nearest clipping ancestor's
/// scissor. The occlusion half (a later-painted node covering the
/// band) lives in [`check_focus_ring_occluded`] — it needs the
/// flattened paint-order set, while this half is inherently a
/// node-vs-ancestor check and runs during `walk`.
fn check_focus_ring_clipped(
    n: &El,
    n_rect: Rect,
    nearest_clip: &ClipCtx,
    r: &mut LintReport,
    blame: Source,
) {
    let ring_overflow = focus_ring_overflow(n);
    if !any_side_overflows(ring_overflow) {
        return;
    }
    let band = n_rect.outset(ring_overflow);

    // Clipped by ancestor scissor. For scrollable clips, only the
    // cross axis is checked — the scroll axis can bring partially
    // clipped rows into view on focus.
    let (clip_rect, check_horiz, check_vert) = match nearest_clip {
        ClipCtx::None => (None, false, false),
        ClipCtx::Static(rect) => (Some(*rect), true, true),
        ClipCtx::Scrolling {
            rect, scroll_axis, ..
        } => match scroll_axis {
            Axis::Column => (Some(*rect), true, false),
            Axis::Row => (Some(*rect), false, true),
            Axis::Overlay => (Some(*rect), true, true),
        },
    };
    if let Some(clip) = clip_rect {
        let dx_left = if check_horiz {
            (clip.x - band.x).max(0.0)
        } else {
            0.0
        };
        let dx_right = if check_horiz {
            (band.right() - clip.right()).max(0.0)
        } else {
            0.0
        };
        let dy_top = if check_vert {
            (clip.y - band.y).max(0.0)
        } else {
            0.0
        };
        let dy_bottom = if check_vert {
            (band.bottom() - clip.bottom()).max(0.0)
        } else {
            0.0
        };
        if dx_left + dx_right + dy_top + dy_bottom > 0.5 {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::FocusRingObscured,
                    node_id: n.computed_id.clone().to_string(),
                    source: blame,
                    message: format!(
                        "focus ring band clipped by ancestor scissor (L={dx_left:.0} R={dx_right:.0} T={dy_top:.0} B={dy_bottom:.0}) — give a clipping ancestor padding ≥ tokens::RING_WIDTH on the clipped side",
                    ),
                },
            );
        }
    }
}

/// Detect [`FindingKind::FocusRingObscured`]'s occlusion half: a
/// focusable node with an outside ring whose bleed band is overlapped
/// by a later-painted node. Runs over the flattened paint-order set,
/// so an occluder in a sibling wrapper container is still seen —
/// wrapper boundaries don't shield flush controls (issue #37). The
/// focusable's own subtree is skipped (a control's internals paint
/// with it, not over its ring), and so are sibling overlay layers — a
/// scrim or dialog painting over a background control's band is
/// intentional stacking, not a layout bug. Occluder rects are clipped
/// by their own scissor first, so content inside a scroll viewport
/// can't "occlude" a control it never actually paints over. The
/// clipping half (ancestor scissor cutting the band) lives in
/// [`check_focus_ring_clipped`].
fn check_focus_ring_occluded(flat: &FlatTree, r: &mut LintReport) {
    for f in flat.nodes.iter() {
        if !f.el.focusable || !is_from_user(f.el.source) {
            continue;
        }
        let ring_overflow = focus_ring_overflow(f.el);
        if !any_side_overflows(ring_overflow) {
            continue;
        }
        // Everything from `subtree_end` on paints after `f` and is
        // outside its own subtree.
        for o in &flat.nodes[f.subtree_end..] {
            if !paints_pixels(o.el) || !flat.layers_comparable(f.layer, o.layer) {
                continue;
            }
            // Clip the occluder by its own scissor (it never paints
            // outside it), then by the focusable's scissor — the ring
            // band can only render inside the focusable's own clip, so
            // a band region outside it has nothing to occlude. This is
            // what keeps content-space rects honest across a scroll
            // boundary: a row scrolled past the viewport bottom has a
            // rect that overlaps window chrome below the scroll, but
            // the scissor means neither ring nor row paints there.
            let Some(o_rect) = clipped_rect(occluder_paint_rect(o.el, o.rect), &o.clip) else {
                continue;
            };
            let Some(o_rect) = clipped_rect(o_rect, &f.clip) else {
                continue;
            };
            if let Some(side) = bleed_occlusion(f.rect, ring_overflow, o_rect) {
                push_for(
                    r,
                    f.el,
                    Finding {
                        kind: FindingKind::FocusRingObscured,
                        node_id: f.el.computed_id.clone().to_string(),
                        source: f.el.source,
                        message: format!(
                            "focus ring band occluded on the {side} edge by later-painted {occluder_id} — increase gap to ≥ tokens::RING_WIDTH or restructure so the neighbor doesn't sit on the edge",
                            occluder_id = o.el.computed_id,
                        ),
                    },
                );
                // First occluder is enough — don't double-report.
                break;
            }
        }
    }
}

/// Detects `ScrollbarObscuresFocusable`: a focusable descendant of a
/// scrolling ancestor whose x-extent overlaps the visible scrollbar
/// thumb's column. The check uses the thumb's *active* width
/// (`SCROLLBAR_THUMB_WIDTH_ACTIVE`) — the wider rendering shown when
/// the user interacts with the scrollbar — so the fix that clears
/// the active thumb (a `SCROLLBAR_THUMB_WIDTH_ACTIVE +
/// SCROLLBAR_TRACK_INSET`-wide right-edge gutter on content) is also
/// what silences the lint.
///
/// The thumb's vertical position changes with scroll offset, but its
/// x-column is fixed; checking x-axis overlap (independent of the
/// thumb's current y) catches focusables that would be covered at
/// any scroll position.
///
/// Only fires when content actually overflows enough for the runtime
/// to write a `thumb_tracks` entry — non-overflowing scrolls don't
/// render a thumb, so the bug isn't user-visible.
fn check_scrollbar_overlap(
    n: &El,
    n_rect: Rect,
    nearest_clip: &ClipCtx,
    ui_state: &UiState,
    r: &mut LintReport,
    blame: Source,
) {
    let ClipCtx::Scrolling {
        node_id,
        overlay_thumb,
        ..
    } = nearest_clip
    else {
        return;
    };
    if *overlay_thumb {
        return;
    }
    let Some(track) = ui_state.scroll.thumb_tracks.get(node_id).copied() else {
        return;
    };
    // Active thumb sits flush-right inside the hitbox gutter, so its
    // right edge equals the track's right edge and its width is
    // SCROLLBAR_THUMB_WIDTH_ACTIVE. Checking against this (rather
    // than the wider hitbox) matches the conventional fix gutter of
    // SCROLLBAR_THUMB_WIDTH_ACTIVE + SCROLLBAR_TRACK_INSET.
    let active_w = crate::tokens::SCROLLBAR_THUMB_WIDTH_ACTIVE;
    let thumb_left = track.right() - active_w;
    let thumb_right = track.right();
    let overlap_x = n_rect.right().min(thumb_right) - n_rect.x.max(thumb_left);
    if overlap_x <= 0.5 {
        return;
    }
    push_for(
        r,
        n,
        Finding {
            kind: FindingKind::ScrollbarObscuresFocusable,
            node_id: n.computed_id.clone().to_string(),
            source: blame,
            message: format!(
                "scrollbar thumb overlaps this focusable on the right edge by {overlap_x:.0}px (thumb x={thumb_left:.0}..{thumb_right:.0}; control x={ctrl_x:.0}..{ctrl_right:.0}) — add `.scrollbar_gutter()` to the scroll node to reserve a thumb gutter (CSS scrollbar-gutter: stable), or move horizontal padding *inside* the scroll onto a wrapper that constrains children to a narrower content rect",
                ctrl_x = n_rect.x,
                ctrl_right = n_rect.right(),
            ),
        },
    );
}

/// True if `n` paints visible pixels (so it can occlude a neighbor's
/// focus ring band). Pure structural columns/rows with no fill/
/// stroke/text/image/shadow don't occlude.
fn paints_pixels(n: &El) -> bool {
    n.fill.is_some()
        || n.stroke.is_some()
        || n.image.is_some()
        || n.icon.is_some()
        || n.shadow > 0.0
        || n.text.is_some()
        || !matches!(n.surface_role, SurfaceRole::None)
}

/// The region where `n` actually puts ink, given its layout `rect`.
/// Fills, strokes, shadows, images, and surface roles paint the full
/// rect; a text/icon-only node paints its content *inside* its
/// padding, so `.padding(Sides::top(...))` on a caption genuinely
/// moves the ink off a neighbor's focus-ring band and must silence
/// the occlusion check.
fn occluder_paint_rect(n: &El, rect: Rect) -> Rect {
    let full_rect_paint = n.fill.is_some()
        || n.stroke.is_some()
        || n.image.is_some()
        || n.shadow > 0.0
        || !matches!(n.surface_role, SurfaceRole::None);
    if full_rect_paint {
        rect
    } else {
        rect.inset(n.padding)
    }
}

/// Whichever side of `n_rect`'s `paint_overflow` band `sib_rect`
/// intersects (above the EPS adjacency threshold). `EPS` keeps a
/// neighbor whose edge merely touches the focusable's edge (gap = 0)
/// from triggering — touching is adjacency, not yet occlusion.
fn bleed_occlusion(n_rect: Rect, overflow: Sides, sib_rect: Rect) -> Option<&'static str> {
    const EPS: f32 = 0.5;
    let bands: [(&'static str, Rect); 4] = [
        (
            "top",
            Rect::new(n_rect.x, n_rect.y - overflow.top, n_rect.w, overflow.top),
        ),
        (
            "bottom",
            Rect::new(n_rect.x, n_rect.bottom(), n_rect.w, overflow.bottom),
        ),
        (
            "left",
            Rect::new(n_rect.x - overflow.left, n_rect.y, overflow.left, n_rect.h),
        ),
        (
            "right",
            Rect::new(n_rect.right(), n_rect.y, overflow.right, n_rect.h),
        ),
    ];
    for (side, band) in bands {
        if band.w <= 0.0 || band.h <= 0.0 {
            continue;
        }
        let iw = band.right().min(sib_rect.right()) - band.x.max(sib_rect.x);
        let ih = band.bottom().min(sib_rect.bottom()) - band.y.max(sib_rect.y);
        if iw > EPS && ih > EPS {
            return Some(side);
        }
    }
    None
}

fn lint_row_alignment(n: &El, computed: Rect, r: &mut LintReport, blame: Source) {
    if !matches!(n.axis, Axis::Row) || !matches!(n.align, Align::Stretch) || n.children.len() < 2 {
        return;
    }
    if !n.children.iter().any(is_text_like_child) {
        return;
    }

    let inner = computed.inset(n.padding);
    if inner.h <= 0.0 {
        return;
    }

    for child in &n.children {
        if !is_fixed_visual_child(child) {
            continue;
        }
        let child_rect = child.computed_rect;
        let top_pinned = (child_rect.y - inner.y).abs() <= 0.5;
        let visibly_short = child_rect.h + 2.0 < inner.h;
        if top_pinned && visibly_short {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::Alignment,
                    node_id: n.computed_id.clone().to_string(),
                    source: blame,
                    message: "row has a fixed-size visual child pinned to the top beside text; add .align(Align::Center) to vertically center row content"
                        .to_string(),
                },
            );
            return;
        }
    }
}

fn lint_overlay_alignment(n: &El, computed: Rect, r: &mut LintReport, blame: Source) {
    if !matches!(n.axis, Axis::Overlay)
        || n.children.is_empty()
        || !matches!(n.align, Align::Start | Align::Stretch)
        || !matches!(n.justify, Justify::Start | Justify::SpaceBetween)
        || !has_visible_surface(n)
    {
        return;
    }

    let inner = computed.inset(n.padding);
    if inner.w <= 0.0 || inner.h <= 0.0 {
        return;
    }

    for child in &n.children {
        if !is_fixed_visual_child(child) {
            continue;
        }
        let child_rect = child.computed_rect;
        let left_pinned = (child_rect.x - inner.x).abs() <= 0.5;
        let top_pinned = (child_rect.y - inner.y).abs() <= 0.5;
        let visibly_narrow = child_rect.w + 2.0 < inner.w;
        let visibly_short = child_rect.h + 2.0 < inner.h;
        if left_pinned && top_pinned && visibly_narrow && visibly_short {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::Alignment,
                    node_id: n.computed_id.clone().to_string(),
                    source: blame,
                    message: "overlay has a smaller fixed-size visual child pinned to the top-left; add .align(Align::Center).justify(Justify::Center) to center overlay content"
                        .to_string(),
                },
            );
            return;
        }
    }
}

fn lint_row_visual_text_spacing(n: &El, r: &mut LintReport, blame: Source) {
    if !matches!(n.axis, Axis::Row) || n.children.len() < 2 {
        return;
    }

    for pair in n.children.windows(2) {
        let [visual, text] = pair else {
            continue;
        };
        if !is_visual_cluster_child(visual) || !is_text_like_child(text) {
            continue;
        }

        let visual_rect = visual.computed_rect;
        let text_rect = text.computed_rect;
        let gap = text_rect.x - visual_rect.right();
        if gap < 4.0 {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::Spacing,
                    node_id: n.computed_id.clone().to_string(),
                    source: blame,
                    message: format!(
                        "row places text {:.0}px after an icon/control slot; add .gap(tokens::SPACE_2) or use a stock menu/list row",
                        gap.max(0.0)
                    ),
                },
            );
            return;
        }
    }
}

fn is_text_like_child(c: &El) -> bool {
    c.text.is_some()
        || c.children
            .iter()
            .any(|child| child.text.is_some() || matches!(child.kind, Kind::Text | Kind::Heading))
}

fn has_visible_surface(n: &El) -> bool {
    n.fill.is_some() || n.stroke.is_some()
}

fn is_fixed_visual_child(c: &El) -> bool {
    let fixed_height = matches!(c.height, Size::Fixed(_));
    fixed_height
        && (c.icon.is_some()
            || matches!(c.kind, Kind::Badge)
            || matches!(
                c.metrics_role,
                Some(
                    MetricsRole::Button
                        | MetricsRole::IconButton
                        | MetricsRole::Input
                        | MetricsRole::Badge
                        | MetricsRole::TabTrigger
                        | MetricsRole::ChoiceControl
                        | MetricsRole::Slider
                        | MetricsRole::Progress
                )
            ))
}

fn is_visual_cluster_child(c: &El) -> bool {
    let fixed_box = matches!(c.width, Size::Fixed(_)) && matches!(c.height, Size::Fixed(_));
    fixed_box
        && (c.icon.is_some()
            || matches!(c.kind, Kind::Badge)
            || matches!(
                c.metrics_role,
                Some(MetricsRole::IconButton | MetricsRole::Badge | MetricsRole::ChoiceControl)
            )
            || (has_visible_surface(c) && c.children.iter().any(is_fixed_visual_child)))
}

fn rect_contains(parent: Rect, child: Rect, tol: f32) -> bool {
    child.x >= parent.x - tol
        && child.y >= parent.y - tol
        && child.right() <= parent.right() + tol
        && child.bottom() <= parent.bottom() + tol
}

/// True when a Row/Column parent's children, summed along the parent's
/// main axis (plus gaps), exceed the parent's padded inner extent —
/// i.e. the layout pass overran. Mirrors the `consumed > main_extent`
/// shape from `layout::layout_axis`. Overlay parents have no main-axis
/// packing, so overrun is meaningless there.
fn flex_main_axis_overflowed(parent: &El, parent_rect: Rect) -> bool {
    let n = parent.children.len();
    if n == 0 {
        return false;
    }
    let inner = parent_rect.inset(parent.padding);
    let inner_main = match parent.axis {
        Axis::Row => inner.w,
        Axis::Column => inner.h,
        Axis::Overlay => return false,
    };
    let total_gap = parent.gap * n.saturating_sub(1) as f32;
    let consumed: f32 = parent
        .children
        .iter()
        .map(|c| {
            let r = c.computed_rect;
            match parent.axis {
                Axis::Row => r.w,
                Axis::Column => r.h,
                Axis::Overlay => 0.0,
            }
        })
        .sum();
    consumed + total_gap > inner_main + 0.5
}

fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default-theme shim: the module's tests predate the `theme`
    /// parameter and don't exercise palettes, so they lint against
    /// `Theme::default()` — shadowing the real [`super::lint`].
    fn lint(root: &El, ui_state: &UiState) -> LintReport {
        super::lint(root, ui_state, &crate::theme::Theme::default())
    }

    fn lint_one(mut root: El) -> LintReport {
        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        lint(&root, &ui_state)
    }

    #[test]
    fn lane_plot_spec_conflicts_are_flagged() {
        use crate::plot::{Lane, PlotSpec, Sample, Scale, SeriesHandle, line};
        let h = SeriesHandle::new(vec![Sample::new(0.0, 0.0), Sample::new(1.0, 1.0)]);

        // Marks + lanes, and a configured y axis: both findings fire.
        let conflicted = PlotSpec::new()
            .y(Scale::log())
            .add_mark(line(&h))
            .lane(Lane::digital("ch0", &h));
        let report = lint_one(crate::tree::plot(conflicted).key("p"));
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::PlotMarksAndLanes),
            "{}",
            report.text()
        );
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::PlotLaneYAxisIgnored),
            "{}",
            report.text()
        );

        // A clean lane plot (and a plain marks plot) stay quiet.
        let clean = PlotSpec::new().lane(Lane::digital("ch0", &h));
        let report = lint_one(crate::tree::plot(clean).key("p"));
        assert!(
            !report.findings.iter().any(|f| matches!(
                f.kind,
                FindingKind::PlotMarksAndLanes | FindingKind::PlotLaneYAxisIgnored
            )),
            "{}",
            report.text()
        );
        let plain = PlotSpec::new().y(Scale::log()).add_mark(line(&h));
        let report = lint_one(crate::tree::plot(plain).key("p"));
        assert!(
            !report.findings.iter().any(|f| matches!(
                f.kind,
                FindingKind::PlotMarksAndLanes | FindingKind::PlotLaneYAxisIgnored
            )),
            "{}",
            report.text()
        );
    }

    #[test]
    fn clipped_nowrap_text_reports_text_overflow() {
        let root = crate::text("A very long dashboard label")
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(20.0));

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn ellipsis_nowrap_text_satisfies_horizontal_overflow_policy() {
        let root = crate::text("A very long dashboard label")
            .ellipsis()
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(20.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn collapsed_fill_input_in_overlay_capped_hug_row_reports_issue_120() {
        // Issue #120's shape: a Fill-width input plus trailing badge
        // in a bare Hug row, inside an Overlay wrapper (what
        // `form_control` was before the fix). Overlay caps Hug
        // children at their intrinsic, and `text_input`'s intrinsic
        // is just its padding, so the input collapses to ~20px while
        // the layout stays "valid" — the lint must say so.
        let selection = crate::Selection::default();
        let mut root = El::new(Kind::Custom("wrapper"))
            .width(Size::Fill(1.0))
            .height(Size::Fixed(48.0))
            .child(crate::row([
                crate::text_input("url", "http://127.0.0.1:8188", &selection),
                crate::text("reachable"),
            ]));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 60.0));
        let report = lint(&root, &state);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::CollapsedFillChild),
            "{}",
            report.text()
        );
    }

    #[test]
    fn rescued_fill_in_hug_wrappers_are_not_flagged() {
        // The declared Fill-inside-Hug pair alone is not a defect:
        // a Hug row under a column is stretched to the column's
        // width (cross-axis Align::Stretch), and the Fill child gets
        // real space. Spacers collapse by design when a row has no
        // slack. Neither may fire CollapsedFillChild.
        let selection = crate::Selection::default();
        let rescued = crate::column([crate::row([
            crate::text_input("url", "http://127.0.0.1:8188", &selection),
            crate::text("reachable"),
        ])]);
        let collapsed_spacer = El::new(Kind::Custom("wrapper"))
            .width(Size::Fill(1.0))
            .height(Size::Fixed(48.0))
            .child(crate::row([crate::text("left"), crate::spacer()]));

        for (shape, mut root) in [("rescued", rescued), ("spacer", collapsed_spacer)] {
            let mut state = UiState::new();
            layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 60.0));
            let report = lint(&root, &state);
            assert!(
                !report
                    .findings
                    .iter()
                    .any(|finding| finding.kind == FindingKind::CollapsedFillChild),
                "{shape}: {}",
                report.text()
            );
        }
    }

    #[test]
    fn hug_ellipsis_in_overflowing_row_reports_dead_chain_issue_19() {
        // Repro for #19: a `text(...).ellipsis()` (default Hug width)
        // inside a flex row whose children's intrinsics sum past the
        // row's allocated width. `Size::Hug` makes the layout pass
        // resolve `main_size = intrinsic`, so the rect's width equals
        // the natural text width — and that's the budget passed to
        // `ellipsize_text_with_family`. The truncation branch never
        // trims a glyph and the chain is silent dead code. The lint
        // must point at the offending text node directly.
        let row = crate::row([
            crate::text("short_label"),
            crate::text("a long descriptive body that should truncate but cannot").ellipsis(),
            crate::text("right_side_metadata"),
        ])
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(20.0));

        let report = lint_one(row);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TextOverflow && f.message.contains("Size::Hug")),
            "expected dead-ellipsis finding pointing at Hug text\n{}",
            report.text()
        );
    }

    #[test]
    fn hug_ellipsis_in_non_overflowing_row_is_quiet() {
        // The lint targets the failure mode (parent overran + dead
        // chain), not the chain itself. When the row has room for all
        // children, `text(...).ellipsis()` with default Hug is just
        // harmless extra metadata — don't lint it.
        let row = crate::row([crate::text("ok").ellipsis()])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(20.0));

        let report = lint_one(row);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn fill_ellipsis_in_overflowing_row_is_quiet() {
        // Counter-test: when the user has chosen `Size::Fill(_)` on
        // the ellipsis text, the chain is live (layout actually
        // constrains the rect), so even if other children push the
        // row over, the dead-chain lint must not fire on this node.
        let row = crate::row([
            crate::text("short_label"),
            crate::text("a long descriptive body that should truncate but cannot")
                .width(Size::Fill(1.0))
                .ellipsis(),
            crate::text("right_side_metadata"),
        ])
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(20.0));

        let report = lint_one(row);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TextOverflow && f.message.contains("Size::Hug")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn padding_eats_fixed_height_button_reports_padding_advice() {
        // `.padding(scalar)` goes through `From<f32> for Sides` as
        // `Sides::all(scalar)` — so on a 30px-tall button with
        // `.padding(SPACE_2)` the vertical padding totals 16, leaving
        // only 14px of inner height for a 20px Label cell. The
        // v-center step clamps the negative slack to 0 and the text
        // paints into the padding band (visibly bottom-leaning, in
        // this case 8px above + 2px below). Message must blame the
        // padding (or the height override), not recommend
        // `paragraph()` / `wrap_text()` / a wider box.
        let root = crate::row([crate::button("Resume")
            .height(Size::Fixed(30.0))
            .padding(crate::tokens::SPACE_2)]);

        let report = lint_one(root);

        let finding = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::Overflow)
            .unwrap_or_else(|| {
                panic!(
                    "expected an Overflow finding for the padding-eats-height shape\n{}",
                    report.text()
                )
            });
        assert!(
            finding.message.contains("vertical padding") && finding.message.contains("Sides::xy"),
            "expected padding-y advice, got:\n{}\n{}",
            finding.message,
            report.text(),
        );
        assert!(
            !finding.message.contains("paragraph()") && !finding.message.contains("wrap_text()"),
            "padding-eats-height case should not recommend paragraph/wrap_text:\n{}",
            finding.message,
        );
    }

    #[test]
    fn padding_eats_fixed_height_y_only_does_not_fire_when_height_is_hug() {
        // Counter-case: with `Size::Hug` the box grows to fit; padding
        // can't "eat" a hugged height so there's no off-center symptom.
        // Don't pin the user to a non-issue.
        let root = crate::row([crate::text("Resume").padding(crate::tokens::SPACE_2)]);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::Overflow || f.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn text_taller_than_fixed_height_without_padding_reports_height_advice() {
        // Different shape: no padding-y, but the text cell itself is
        // taller than the box (e.g. body text size in a too-short
        // chip). The fix is the height (or text size), not the
        // padding. Make sure the lint message reflects that.
        let root = crate::row([crate::text("body")
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(12.0))]);

        let report = lint_one(root);

        let finding = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::Overflow)
            .unwrap_or_else(|| {
                panic!(
                    "expected an Overflow finding for text-taller-than-box\n{}",
                    report.text()
                )
            });
        assert!(
            finding.message.contains("exceeds box height") && finding.message.contains("height"),
            "expected height-advice message, got:\n{}",
            finding.message,
        );
        assert!(
            !finding.message.contains("vertical padding"),
            "no-padding case should not blame padding:\n{}",
            finding.message,
        );
    }

    #[test]
    fn padding_aware_text_overflow_fires_when_text_spills_past_padded_region() {
        // Box is wide enough for the bare text (66 ≤ 80) but padding
        // eats so much that the text spills past the padded content
        // area (66 > 80 - 40). Centered text in this state visually
        // reads as off-center — the lint must flag it even though the
        // text would technically fit inside the outer rect.
        //
        // Wrap in a row so the inner Fixed(80) is honored; the layout
        // pass forces the root rect to the viewport regardless of its
        // own size, so a single-node test would mis-measure.
        let leaf = crate::text("dashboard")
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(28.0))
            .padding(Sides::xy(20.0, 0.0));
        let root = crate::row([leaf]);

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn stretch_row_with_top_pinned_icon_and_text_suggests_center_alignment() {
        let root = crate::row([
            crate::icon("settings").icon_size(crate::tokens::ICON_SM),
            crate::text("Settings").width(Size::Fill(1.0)),
        ])
        .height(Size::Fixed(36.0));

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Alignment
                    && finding.message.contains(".align(Align::Center)")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn centered_row_with_icon_and_text_satisfies_alignment_policy() {
        let root = crate::row([
            crate::icon("settings").icon_size(crate::tokens::ICON_SM),
            crate::text("Settings").width(Size::Fill(1.0)),
        ])
        .height(Size::Fixed(36.0))
        .align(Align::Center);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Alignment),
            "{}",
            report.text()
        );
    }

    #[test]
    fn row_with_icon_slot_touching_text_reports_spacing() {
        let icon_slot = crate::stack([crate::icon("settings").icon_size(crate::tokens::ICON_XS)])
            .align(Align::Center)
            .justify(Justify::Center)
            .fill(crate::tokens::MUTED)
            .width(Size::Fixed(26.0))
            .height(Size::Fixed(26.0));
        let root = crate::row([icon_slot, crate::text("Settings").width(Size::Fill(1.0))])
            .height(Size::Fixed(32.0))
            .align(Align::Center);

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Spacing
                    && finding.message.contains(".gap(tokens::SPACE_2)")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn row_with_icon_slot_and_text_gap_satisfies_spacing_policy() {
        let icon_slot = crate::stack([crate::icon("settings").icon_size(crate::tokens::ICON_XS)])
            .align(Align::Center)
            .justify(Justify::Center)
            .fill(crate::tokens::MUTED)
            .width(Size::Fixed(26.0))
            .height(Size::Fixed(26.0));
        let root = crate::row([icon_slot, crate::text("Settings").width(Size::Fill(1.0))])
            .height(Size::Fixed(32.0))
            .align(Align::Center)
            .gap(crate::tokens::SPACE_2);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Spacing),
            "{}",
            report.text()
        );
    }

    #[test]
    fn overlay_with_top_left_pinned_icon_suggests_center_alignment() {
        let icon_slot = crate::stack([crate::icon("settings").icon_size(crate::tokens::ICON_XS)])
            .fill(crate::tokens::MUTED)
            .width(Size::Fixed(26.0))
            .height(Size::Fixed(26.0));
        let root = crate::column([icon_slot]);

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Alignment
                    && finding.message.contains(".justify(Justify::Center)")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn centered_overlay_icon_satisfies_alignment_policy() {
        let icon_slot = crate::stack([crate::icon("settings").icon_size(crate::tokens::ICON_XS)])
            .align(Align::Center)
            .justify(Justify::Center)
            .fill(crate::tokens::MUTED)
            .width(Size::Fixed(26.0))
            .height(Size::Fixed(26.0));
        let root = crate::column([icon_slot]);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Alignment),
            "{}",
            report.text()
        );
    }

    #[test]
    fn overflow_findings_attribute_to_nearest_user_source_ancestor() {
        // Closure-built-widget shape: an Element constructed inside an
        // damascene widget closure carries `from_library: true`. Its
        // overflow finding should attribute to the nearest non-library
        // ancestor's source.
        let user_source = Source {
            file: "src/screen.rs",
            line: 42,
            from_library: false,
        };
        let widget_source = Source {
            file: "src/widgets/tabs.rs",
            line: 200,
            from_library: true,
        };

        let mut leaf = crate::text("A very long dashboard label")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0));
        leaf.source = widget_source;

        let mut root = crate::row([leaf])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(48.0));
        root.source = user_source;

        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        let report = lint(&root, &ui_state);

        let text_overflow = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::TextOverflow)
            .unwrap_or_else(|| panic!("expected TextOverflow finding\n{}", report.text()));
        assert_eq!(text_overflow.source.file, user_source.file);
        assert_eq!(text_overflow.source.line, user_source.line);
    }

    #[test]
    fn overflow_finding_self_attributes_when_node_is_already_user_source() {
        let mut node = crate::text("A very long dashboard label")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0));
        let user_source = Source {
            file: "src/screen.rs",
            line: 99,
            from_library: false,
        };
        node.source = user_source;

        let mut ui_state = UiState::new();
        layout::layout(&mut node, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        let report = lint(&node, &ui_state);

        let text_overflow = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::TextOverflow)
            .unwrap_or_else(|| panic!("expected TextOverflow finding\n{}", report.text()));
        assert_eq!(text_overflow.source.line, user_source.line);
    }

    #[test]
    fn overflow_lint_fires_for_external_app_paths_issue_13() {
        // Regression for #13: an external app's `Location::caller()`
        // file paths look like `src/sidebar.rs` (relative to its own
        // manifest), not `crates/<name>/src/...`. The old marker-
        // substring filter silently dropped every overflow finding for
        // these. With `from_library: false` (the user-code default),
        // the overflow must fire.
        let user_source = Source {
            file: "src/sidebar.rs",
            line: 17,
            from_library: false,
        };
        let mut child = crate::column(Vec::<El>::new())
            .width(Size::Fixed(32.0))
            .height(Size::Fixed(32.0));
        child.source = user_source;

        let mut row = crate::row([child])
            .width(Size::Fixed(256.0))
            .height(Size::Fixed(28.0));
        row.source = user_source;

        let mut ui_state = UiState::new();
        layout::layout(&mut row, &mut ui_state, Rect::new(0.0, 0.0, 256.0, 28.0));
        let report = lint(&row, &ui_state);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::Overflow),
            "expected an Overflow finding for the 32px child in a 28px row\n{}",
            report.text()
        );
    }

    #[test]
    fn overflow_finding_suppressed_when_no_user_ancestor_exists() {
        // Pure-library tree: every node carries `from_library: true`,
        // so there's no user code to blame and the finding is dropped.
        let widget_source = Source {
            file: "src/widgets/tabs.rs",
            line: 200,
            from_library: true,
        };
        let mut leaf = crate::text("A very long dashboard label")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0));
        leaf.source = widget_source;

        let mut wrapper = crate::row([leaf])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(48.0));
        wrapper.source = widget_source;

        let mut ui_state = UiState::new();
        layout::layout(
            &mut wrapper,
            &mut ui_state,
            Rect::new(0.0, 0.0, 160.0, 48.0),
        );
        let report = lint(&wrapper, &ui_state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TextOverflow || f.kind == FindingKind::Overflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn panel_role_without_fill_reports_missing_surface_fill() {
        let root = crate::column([crate::text("body")])
            .surface_role(SurfaceRole::Panel)
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::MissingSurfaceFill),
            "{}",
            report.text()
        );
    }

    #[test]
    fn panel_role_with_fill_satisfies_surface_policy() {
        let root = crate::column([crate::text("body")])
            .surface_role(SurfaceRole::Panel)
            .fill(crate::tokens::CARD)
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::MissingSurfaceFill),
            "{}",
            report.text()
        );
    }

    #[test]
    fn card_widget_satisfies_surface_policy() {
        let root = crate::widgets::card::card([crate::text("body")])
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::MissingSurfaceFill),
            "{}",
            report.text()
        );
    }

    #[test]
    fn handrolled_card_recipe_reports_reinvented_widget() {
        // column().fill(CARD).stroke(BORDER).radius(>0) is the canonical
        // hand-rolled card silhouette.
        let root = crate::column([crate::text("body")])
            .fill(crate::tokens::CARD)
            .stroke(crate::tokens::BORDER)
            .radius(crate::tokens::RADIUS_LG)
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(48.0));

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ReinventedWidget && f.message.contains("card(")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn real_card_widget_does_not_report_reinvented_widget() {
        // card() returns Kind::Card, so the smell signature (which
        // requires Kind::Group) excludes it by construction.
        let root = crate::widgets::card::card([crate::text("body")])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(48.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ReinventedWidget),
            "{}",
            report.text()
        );
    }

    #[test]
    fn handrolled_sidebar_recipe_reports_reinvented_widget() {
        // column().fill(CARD).stroke(BORDER).width(SIDEBAR_WIDTH) without
        // surface_role(Panel) is the volumetric_ui_v2 sidebar pattern.
        let root = crate::column([crate::text("nav")])
            .fill(crate::tokens::CARD)
            .stroke(crate::tokens::BORDER)
            .width(Size::Fixed(crate::tokens::SIDEBAR_WIDTH))
            .height(Size::Fill(1.0));

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ReinventedWidget && f.message.contains("sidebar(")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn real_sidebar_widget_does_not_report_reinvented_widget() {
        // sidebar() sets surface_role(Panel), which excludes it from the
        // smell signature even though its fill+stroke+width match.
        let root = crate::widgets::sidebar::sidebar([crate::text("nav")]);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ReinventedWidget),
            "{}",
            report.text()
        );
    }

    #[test]
    fn empty_visual_swatch_does_not_report_reinvented_widget() {
        // A childless Group styled with CARD/BORDER is a color sample,
        // not a card-mimic. Card-mimics always wrap content; pure
        // decorative boxes shouldn't trip the lint.
        let root = crate::column(Vec::<El>::new())
            .fill(crate::tokens::CARD)
            .stroke(crate::tokens::BORDER)
            .radius(crate::tokens::RADIUS_SM)
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(34.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ReinventedWidget),
            "{}",
            report.text()
        );
    }

    #[test]
    fn plain_column_does_not_report_reinvented_widget() {
        // A normal column with no surface decoration is fine.
        let root = crate::column([crate::text("a"), crate::text("b")])
            .gap(crate::tokens::SPACE_2)
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ReinventedWidget),
            "{}",
            report.text()
        );
    }

    #[test]
    fn fill_providing_roles_do_not_require_explicit_fill() {
        // Sunken paints palette MUTED.darken(0.08) by default — no
        // explicit fill needed. Same shape applies to Selected /
        // Current / Input / Danger; covering Sunken here as a
        // representative.
        let root = crate::column([crate::text("body")])
            .surface_role(SurfaceRole::Sunken)
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::MissingSurfaceFill),
            "{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_fires_when_input_clipped_on_scroll_cross_axis() {
        // The original bug: a focusable text input flush at the left
        // edge of a vertical-scroll viewport gets its ring scissored.
        let selection = crate::selection::Selection::default();
        let mut root = crate::tree::scroll([crate::tree::column([
            crate::widgets::text_input::text_input("field", "", &selection),
        ])])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::FocusRingObscured
                    && f.message.contains("clipped")
                    && (f.message.contains("L=2") || f.message.contains("R=2"))
            }),
            "expected a FocusRingObscured clipping finding (L=2 or R=2)\n{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_assumes_every_focusable_has_a_ring_band() {
        // Regression coverage for sidebar_menu_button-style widgets:
        // focusable controls may forget an explicit paint_overflow, but
        // the renderer still draws a RING_WIDTH focus halo when focused.
        // The lint should reason about that implicit band.
        let mut root = crate::tree::scroll([crate::tree::column([El::new(Kind::Custom(
            "raw_focusable",
        ))
        .key("raw")
        .focusable()
        .fill(crate::tokens::CARD)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(40.0))])])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::FocusRingObscured
                    && f.message.contains("clipped")
                    && (f.message.contains("L=2") || f.message.contains("R=2"))
            }),
            "expected a FocusRingObscured clipping finding for implicit focus ring band\n{}",
            report.text()
        );
    }

    #[test]
    fn hit_overflow_collision_lint_fires_for_sibling_target_overlap() {
        let root = crate::tree::row([
            crate::button("A")
                .key("a")
                .hit_overflow(Sides::right(8.0))
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(24.0)),
            crate::button("B")
                .key("b")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(24.0)),
        ])
        .gap(4.0);

        let report = lint_one(root);

        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::HitOverflowCollision
                    && f.message.contains("`a`")
                    && f.message.contains("`b`")
            }),
            "expected HitOverflowCollision when a hit_overflow band reaches the next sibling\n{}",
            report.text()
        );
    }

    #[test]
    fn hit_overflow_collision_lint_is_quiet_when_gap_clears_band() {
        let root = crate::tree::row([
            crate::button("A")
                .key("a")
                .hit_overflow(Sides::right(8.0))
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(24.0)),
            crate::button("B")
                .key("b")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(24.0)),
        ])
        .gap(12.0);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::HitOverflowCollision),
            "{}",
            report.text()
        );
    }

    #[test]
    fn hit_overflow_collision_lint_skips_overlay_stacks() {
        let root = crate::tree::stack([
            crate::button("A")
                .key("a")
                .hit_overflow(Sides::all(8.0))
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(24.0)),
            crate::button("B")
                .key("b")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(24.0)),
        ]);

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::HitOverflowCollision),
            "{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_silenced_when_scroll_supplies_horizontal_slack() {
        // Same shape, but the scroll's content is wrapped so the input
        // sits inset by RING_WIDTH on each horizontal edge. No finding.
        let selection = crate::selection::Selection::default();
        let mut root =
            crate::tree::scroll(
                [crate::tree::column([crate::widgets::text_input::text_input(
                    "field", "", &selection,
                )])
                .padding(Sides::xy(crate::tokens::RING_WIDTH, 0.0))],
            )
            .width(Size::Fixed(300.0))
            .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::FocusRingObscured),
            "{}",
            report.text()
        );
    }

    #[test]
    fn unknown_icon_name_is_flagged() {
        // A hallucinated icon name resolves to AlertCircle at paint, but
        // the lint surfaces it so the silent fallback shows up in review.
        let mut root = crate::tree::column([crate::icons::icon("trending-up").key("trend")]);
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let report = lint(&root, &state);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnknownIconName),
            "{}",
            report.text()
        );

        // A real built-in name is clean.
        let mut ok = crate::tree::column([crate::icons::icon("chevron-up").key("ok")]);
        let mut ok_state = UiState::new();
        layout::layout(&mut ok, &mut ok_state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ok_report = lint(&ok, &ok_state);
        assert!(
            !ok_report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnknownIconName),
            "{}",
            ok_report.text()
        );
    }

    #[test]
    fn item_group_supplies_focus_ring_slack_inside_a_scroll() {
        // The canonical `scroll([item_group([item, ...])])` pattern must
        // be focus-ring-clean with no hand-rolled padding: item_group
        // carries RING_WIDTH of horizontal inset so the focusable items'
        // outside ring bands clear the scroll scissor.
        use crate::widgets::item::{item, item_content, item_group, item_title};
        let mut root = crate::tree::scroll([item_group([
            item([item_content([item_title("Alpha")])]).key("a"),
            item([item_content([item_title("Beta")])]).key("b"),
        ])])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::FocusRingObscured),
            "{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_skips_clipping_on_scroll_axis() {
        // Tall content that runs past a vertical scroll's bottom edge
        // is fine — auto-scroll-on-focus brings the focused row into
        // view. The lint must not fire on the scroll axis.
        let selection = crate::selection::Selection::default();
        let mut root = crate::tree::scroll([crate::tree::column([
            // Big top filler so the input lands well below the viewport.
            crate::tree::column(Vec::<El>::new())
                .width(Size::Fill(1.0))
                .height(Size::Fixed(200.0)),
            crate::widgets::text_input::text_input("field", "", &selection),
        ])
        .padding(Sides::xy(crate::tokens::RING_WIDTH, 0.0))])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::FocusRingObscured),
            "expected no FocusRingObscured finding for a row clipped on the scroll axis\n{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_fires_on_static_clip_in_any_direction() {
        // A non-scrolling clipping container (an ordinary clipped card)
        // doesn't auto-reveal anything, so all four sides count.
        let selection = crate::selection::Selection::default();
        let mut root = crate::tree::column([crate::widgets::text_input::text_input(
            "field", "", &selection,
        )])
        .clip()
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::FocusRingObscured && f.message.contains("clipped")
            }),
            "expected a static-clip FocusRingObscured finding\n{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_fires_on_painted_later_sibling_overlap() {
        // Focusable on the left, a card-like sibling immediately to
        // the right at gap=0. The card paints fill+stroke, so the
        // focusable's right ring band gets occluded.
        let selection = crate::selection::Selection::default();
        let mut root = crate::tree::row([
            crate::widgets::text_input::text_input("field", "", &selection),
            crate::tree::column([crate::text("neighbor")])
                .fill(crate::tokens::CARD)
                .stroke(crate::tokens::BORDER)
                .width(Size::Fixed(80.0))
                .height(Size::Fixed(32.0)),
        ])
        .gap(0.0)
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(32.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 60.0));
        let report = lint(&root, &state);

        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::FocusRingObscured
                    && f.message.contains("occluded")
                    && f.message.contains("right")
            }),
            "expected an occlusion finding on the right edge\n{}",
            report.text()
        );
    }

    #[test]
    fn adjacency_lints_fire_across_wrapper_containers_issue_37() {
        // Issue #37 case (b): two buttons rendered visually flush —
        // identical geometry to the direct-sibling case, but each
        // wrapped in its own row (the `field(label, control)` shape).
        // Both adjacency findings must survive the extra container
        // boundary: buttons carry default `hit_overflow` and an
        // outside focus ring, so flush stacking overlaps both bands.
        let mut root = crate::tree::column([
            crate::tree::row([crate::button("Alpha").key("a")]),
            crate::tree::row([crate::button("Beta").key("b")]),
        ])
        .width(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let report = lint(&root, &state);

        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::HitOverflowCollision
                    && f.message.contains("`a`")
                    && f.message.contains("`b`")
            }),
            "expected HitOverflowCollision across wrapper rows\n{}",
            report.text()
        );
        assert!(
            report.findings.iter().any(|f| {
                f.kind == FindingKind::FocusRingObscured
                    && f.message.contains("occluded")
                    && f.message.contains("bottom")
            }),
            "expected a FocusRingObscured occlusion finding across wrapper rows\n{}",
            report.text()
        );
    }

    #[test]
    fn hit_overflow_collision_lint_skips_nested_keyed_targets() {
        // A keyed clickable row with hit_overflow containing a keyed
        // button: the expanded outer target necessarily overlaps the
        // inner one, but nested hit targets resolve innermost-first by
        // construction — not ambiguity.
        let root = crate::tree::row([crate::button("Inner")
            .key("inner")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(24.0))])
        .key("outer")
        .hit_overflow(Sides::all(8.0))
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(32.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::HitOverflowCollision),
            "{}",
            report.text()
        );
    }

    #[test]
    fn adjacency_lints_skip_sibling_overlay_layers_when_nested() {
        // Controls in *different overlay layers* stack on purpose, even
        // when each is buried in its own wrapper container: a dialog
        // layer painting over a background button's hit band and focus
        // ring is intentional layering, not a flush-layout bug.
        let mut root = crate::tree::stack([
            crate::tree::column([crate::button("Behind")
                .key("behind")
                .hit_overflow(Sides::all(8.0))]),
            crate::tree::column(Vec::<El>::new())
                .key("scrim")
                .fill(crate::tokens::CARD)
                .width(Size::Fixed(300.0))
                .height(Size::Fixed(200.0)),
        ]);
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let report = lint(&root, &state);

        assert!(
            !report.findings.iter().any(|f| {
                f.kind == FindingKind::HitOverflowCollision
                    || (f.kind == FindingKind::FocusRingObscured && f.message.contains("occluded"))
            }),
            "{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_allows_flush_inside_ring_menu_items() {
        let mut root = crate::tree::column([
            crate::menu_item("Checkout").key("checkout"),
            crate::menu_item("Merge").key("merge"),
            crate::menu_item("Delete").key("delete"),
        ])
        .gap(0.0)
        .width(Size::Fixed(180.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 220.0, 140.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::FocusRingObscured),
            "{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_ignores_unpainted_structural_sibling() {
        // A structural column with no fill/stroke/text shouldn't be
        // counted as an occluder — it draws no pixels.
        let selection = crate::selection::Selection::default();
        let mut root = crate::tree::row([
            crate::widgets::text_input::text_input("field", "", &selection),
            crate::tree::column(Vec::<El>::new())
                .width(Size::Fixed(80.0))
                .height(Size::Fixed(32.0)),
        ])
        .gap(0.0)
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(32.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 60.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::FocusRingObscured),
            "{}",
            report.text()
        );
    }

    #[test]
    fn scrollbar_overlap_lint_exempts_popover_menu_panels() {
        // A stock menu panel shrunk below its content (the phone
        // dropdown case) scrolls with the thumb over its rows — the
        // platform-menu convention, which the lint must not report.
        // The same panel without the popover surface role is an
        // ordinary scroll and still fires.
        use crate::tree::SurfaceRole;
        use crate::widgets::popover::{menu_item, popover_panel};
        let panel = || {
            popover_panel((0..30).map(|i| menu_item(format!("Item {i}")).key(format!("item-{i}"))))
                .width(Size::Fixed(200.0))
                .height(Size::Fixed(100.0))
        };
        for (popover, expected) in [(true, false), (false, true)] {
            let role = if popover {
                SurfaceRole::Popover
            } else {
                SurfaceRole::None
            };
            let mut root = crate::tree::column([panel().surface_role(role)]);
            let mut state = UiState::new();
            layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
            let report = lint(&root, &state);
            let fired = report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ScrollbarObscuresFocusable);
            assert_eq!(fired, expected, "popover = {popover}:\n{}", report.text());
        }
    }

    #[test]
    fn scrollbar_overlap_lint_fires_when_thumb_covers_fill_child() {
        // Repro from #21: padding *on* the scroll silences
        // FocusRingObscured but leaves the scrollbar thumb painting
        // on top of right-flush focusables.
        let body = crate::tree::column(
            (0..30)
                .map(|i| {
                    crate::tree::row([
                        crate::text(format!("Row {i}")),
                        crate::tree::spacer(),
                        crate::widgets::switch::switch(format!("row-{i}-toggle"), false),
                    ])
                    .gap(crate::tokens::SPACE_2)
                    .width(Size::Fill(1.0))
                })
                .collect::<Vec<_>>(),
        )
        .gap(crate::tokens::SPACE_2)
        .width(Size::Fill(1.0));

        let mut root = crate::tree::scroll([body])
            .padding(Sides::xy(crate::tokens::SPACE_3, crate::tokens::SPACE_2))
            .width(Size::Fixed(480.0))
            .height(Size::Fixed(320.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 480.0, 320.0));
        let report = lint(&root, &state);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ScrollbarObscuresFocusable),
            "expected ScrollbarObscuresFocusable for a switch that reaches the scroll's inner.right()\n{}",
            report.text()
        );
    }

    #[test]
    fn scrollbar_overlap_lint_silenced_when_padding_is_inside_scroll() {
        // The recommended fix: move horizontal padding onto a wrapper
        // *inside* the scroll. The scroll's own padding stays on the
        // y axis only; the wrapper inset clears the thumb gutter.
        let body = crate::tree::column(
            (0..30)
                .map(|i| {
                    crate::tree::row([
                        crate::text(format!("Row {i}")),
                        crate::tree::spacer(),
                        crate::widgets::switch::switch(format!("row-{i}-toggle"), false),
                    ])
                    .gap(crate::tokens::SPACE_2)
                    .width(Size::Fill(1.0))
                })
                .collect::<Vec<_>>(),
        )
        .gap(crate::tokens::SPACE_2)
        .width(Size::Fill(1.0));

        let mut root = crate::tree::scroll([crate::tree::column([body])
            .padding(Sides::xy(crate::tokens::SPACE_3, 0.0))
            .width(Size::Fill(1.0))])
        .padding(Sides::xy(0.0, crate::tokens::SPACE_2))
        .width(Size::Fixed(480.0))
        .height(Size::Fixed(320.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 480.0, 320.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ScrollbarObscuresFocusable),
            "expected no ScrollbarObscuresFocusable when padding is inside the scroll\n{}",
            report.text()
        );
    }

    #[test]
    fn scrollbar_overlap_lint_silenced_by_scrollbar_gutter() {
        // The one-call fix: `.scrollbar_gutter()` on the scroll node.
        // Same overflowing right-flush switches as the firing repro;
        // the gutter is resolved by the metrics pass (as in the real
        // prepare and bundle pipelines), so run it before layout.
        let body = crate::tree::column(
            (0..30)
                .map(|i| {
                    crate::tree::row([
                        crate::text(format!("Row {i}")),
                        crate::tree::spacer(),
                        crate::widgets::switch::switch(format!("row-{i}-toggle"), false),
                    ])
                    .gap(crate::tokens::SPACE_2)
                    .width(Size::Fill(1.0))
                })
                .collect::<Vec<_>>(),
        )
        .gap(crate::tokens::SPACE_2)
        .width(Size::Fill(1.0));

        let mut root = crate::tree::scroll([body])
            .scrollbar_gutter()
            .padding(Sides::xy(crate::tokens::SPACE_3, crate::tokens::SPACE_2))
            .width(Size::Fixed(480.0))
            .height(Size::Fixed(320.0));
        crate::Theme::default().apply_metrics(&mut root);
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 480.0, 320.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ScrollbarObscuresFocusable),
            "expected scrollbar_gutter() to clear the thumb overlap\n{}",
            report.text()
        );
        // The thumb still renders (content overflows) — the gutter
        // reserves space for it rather than hiding it.
        assert!(
            state.scroll.thumb_tracks.len() == 1,
            "thumb track should still exist with the gutter"
        );
    }

    #[test]
    fn scrollbar_overlap_lint_quiet_when_content_does_not_overflow() {
        // A `scroll` with content shorter than its viewport doesn't
        // render a thumb, so the bug isn't user-visible. The lint
        // should match — thumb_tracks has no entry for the scroll, so
        // there's nothing to collide against.
        let body = crate::tree::column([crate::tree::row([
            crate::text("only row"),
            crate::tree::spacer(),
            crate::widgets::switch::switch("only-toggle", false),
        ])
        .gap(crate::tokens::SPACE_2)
        .width(Size::Fill(1.0))])
        .gap(crate::tokens::SPACE_2)
        .width(Size::Fill(1.0));

        let mut root = crate::tree::scroll([body])
            .padding(Sides::xy(crate::tokens::SPACE_3, crate::tokens::SPACE_2))
            .width(Size::Fixed(480.0))
            .height(Size::Fixed(320.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 480.0, 320.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ScrollbarObscuresFocusable),
            "expected no ScrollbarObscuresFocusable when content fits in the viewport (no thumb rendered)\n{}",
            report.text()
        );
    }

    #[test]
    fn unkeyed_tooltip_reports_dead_tooltip() {
        // Repro: a `.tooltip()` on a text leaf with no `.key()`.
        // Hit-test only returns keyed nodes, so hover never lands on
        // this leaf and the tooltip is silently dead. The classic
        // mistake on commit-graph row chrome (sha cells, timestamps,
        // chips, identicon avatars).
        let root = crate::text("abc1234").tooltip("commit sha");

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::DeadTooltip),
            "expected DeadTooltip on unkeyed tooltipped text\n{}",
            report.text()
        );
    }

    #[test]
    fn keyed_tooltip_satisfies_dead_tooltip_policy() {
        // Counter-test: same shape, but the leaf has a key — so
        // hit-test does land here and the tooltip fires.
        let root = crate::text("abc1234").key("sha").tooltip("commit sha");

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::DeadTooltip),
            "{}",
            report.text()
        );
    }

    fn lint_windowed(mut root: El) -> LintReport {
        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 640.0, 480.0));
        lint(&root, &ui_state)
    }

    #[test]
    fn flush_toolbar_text_reports_unpadded_viewport_leaf() {
        // Repro from the damascene-gallery field report: a bare column
        // root, toolbar text flush against the window edge, clipped by
        // rounded window corners. No surface role anywhere, so
        // UnpaddedSurfacePanel can't see it.
        let root = crate::column([crate::text("Library")]);

        let report = lint_windowed(root);

        let findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::UnpaddedViewportLeaf)
            .collect();
        assert_eq!(
            findings.len(),
            1,
            "one leaf flush on several sides folds into one finding\n{}",
            report.text()
        );
        let msg = &findings[0].message;
        assert!(
            msg.contains("top/right/left") && msg.contains("page([...])"),
            "message should name the sides and the fix: {msg}"
        );
    }

    #[test]
    fn padded_page_root_satisfies_viewport_leaf_policy() {
        // The fix the lint suggests: page() bakes the window padding.
        let report = lint_windowed(crate::page([crate::text("Library")]));
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedViewportLeaf),
            "{}",
            report.text()
        );
    }

    #[test]
    fn bare_leaf_root_skips_viewport_leaf_policy() {
        // A single bare text node smoke-rendered through render_bundle
        // is a fragment, not a window — no anatomy to fix.
        let report = lint_windowed(crate::text("just a fragment"));
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedViewportLeaf),
            "{}",
            report.text()
        );
    }

    #[test]
    fn scrolled_content_skips_viewport_leaf_policy() {
        // Repro from the showcase shell: a leaf inside a scroll lands
        // flush against the window edge in content-space coordinates.
        // Scrolled rects shift with the offset and are clipped by the
        // scroll viewport, so that's coincidence, not missing window
        // padding.
        let root = crate::column([crate::scroll([crate::column([crate::text("nav item")])])
            .height(crate::Size::Fill(1.0))]);
        let report = lint_windowed(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedViewportLeaf),
            "{}",
            report.text()
        );
    }

    #[test]
    fn full_bleed_leaf_can_allow_viewport_leaf_lint() {
        let root = crate::column([crate::text("intentional full-bleed strip")
            .allow_lint(FindingKind::UnpaddedViewportLeaf)]);
        let report = lint_windowed(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedViewportLeaf),
            "{}",
            report.text()
        );
    }

    #[test]
    fn tooltip_under_non_overlay_root_reports_missing_overlay_root() {
        // Repro from the damascene-gallery field report: App::build
        // returns a bare column, a descendant carries .tooltip() —
        // runtime panics on first hover. The lint catches it at
        // render_bundle time, attributed to the root.
        let root = crate::column([
            crate::text("toolbar"),
            crate::text("cell").key("cell").tooltip("a tooltip"),
        ]);

        let report = lint_one(root);

        let f = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::TooltipWithoutOverlayRoot)
            .unwrap_or_else(|| {
                panic!(
                    "expected TooltipWithoutOverlayRoot under a column root\n{}",
                    report.text()
                )
            });
        assert!(
            f.message.contains("overlays(main, [])"),
            "message should carry the fix: {}",
            f.message
        );
    }

    #[test]
    fn tooltip_under_overlay_root_satisfies_overlay_root_policy() {
        // Counter-test: the documented fix — overlays(main, []) — and
        // any stack(...) root are Axis::Overlay containers.
        for root in [
            crate::overlays(
                crate::column([crate::text("cell").key("cell").tooltip("tip")]),
                [],
            ),
            crate::stack([crate::text("cell").key("cell").tooltip("tip")]),
        ] {
            let report = lint_one(root);
            assert!(
                !report
                    .findings
                    .iter()
                    .any(|f| f.kind == FindingKind::TooltipWithoutOverlayRoot),
                "{}",
                report.text()
            );
        }
    }

    #[test]
    fn tooltip_free_tree_satisfies_overlay_root_policy() {
        let report = lint_one(crate::column([crate::text("plain")]));
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TooltipWithoutOverlayRoot),
            "{}",
            report.text()
        );
    }

    #[test]
    fn unkeyed_tooltip_inside_keyed_ancestor_still_reports_dead_tooltip() {
        // Even when an ancestor is keyed (so hover lands on the
        // ancestor), the leaf's tooltip text is on the leaf — and
        // tooltip lookup is by the hit target's `computed_id`, not
        // by walking ancestors. So the leaf's tooltip still never
        // fires. Flag it.
        let root =
            crate::row([crate::text("inner detail").tooltip("never shown")]).key("outer-row");

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::DeadTooltip),
            "expected DeadTooltip on unkeyed leaf even with keyed ancestor\n{}",
            report.text()
        );
    }

    #[test]
    fn focus_ring_lint_is_quiet_inside_form_after_padding_fix() {
        // Regression: with form()'s default RING_WIDTH horizontal
        // padding, a text input flush inside a scroll/form chain
        // doesn't trip the clipping lint.
        let selection = crate::selection::Selection::default();
        let mut root = crate::tree::scroll([crate::widgets::form::form([
            crate::widgets::form::form_item([crate::widgets::form::form_control(
                crate::widgets::text_input::text_input("field", "", &selection),
            )]),
        ])])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(120.0));
        let mut state = UiState::new();
        layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));
        let report = lint(&root, &state);

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::FocusRingObscured),
            "{}",
            report.text()
        );
    }

    /// Like [`lint_one`] but runs the metrics pass first, so canonical
    /// recipes that depend on auto-defaults (card_header corner
    /// inheritance, control heights, etc.) reach lint in their settled
    /// shape.
    fn lint_one_with_metrics(mut root: El) -> LintReport {
        crate::metrics::ThemeMetrics::default().apply_to_tree(&mut root);
        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 200.0, 120.0));
        lint(&root, &ui_state)
    }

    #[test]
    fn handrolled_rounded_container_with_flat_filled_header_reports_corner_stackup() {
        // The hand-rolled equivalent of `card([card_header(...).fill(MUTED), ...])`.
        // Metrics-pass corner inheritance doesn't apply here (no
        // MetricsRole::Card on the parent), so the lint must fire.
        let parent = crate::column([
            crate::row([crate::text("Header")])
                .fill(crate::tokens::MUTED)
                .width(Size::Fill(1.0))
                .height(Size::Fixed(24.0)),
            crate::row([crate::text("Body")])
                .width(Size::Fill(1.0))
                .height(Size::Fixed(60.0)),
        ])
        .fill(crate::tokens::CARD)
        .stroke(crate::tokens::BORDER)
        .radius(crate::tokens::RADIUS_LG)
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(96.0));

        let report = lint_one(parent);

        let found = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::CornerStackup);
        let found =
            found.unwrap_or_else(|| panic!("expected CornerStackup, got:\n{}", report.text()));
        assert!(
            found.message.contains("Corners::top"),
            "top-strip leak should suggest Corners::top, got: {}",
            found.message
        );
    }

    #[test]
    fn handrolled_rounded_container_with_inset_child_does_not_report_corner_stackup() {
        // Parent has padding; the child is inset from the curve area.
        let parent = crate::column([crate::row([crate::text("Header")])
            .fill(crate::tokens::MUTED)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(24.0))])
        .fill(crate::tokens::CARD)
        .stroke(crate::tokens::BORDER)
        .radius(crate::tokens::RADIUS_LG)
        .padding(Sides::all(crate::tokens::RADIUS_LG))
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(96.0));

        let report = lint_one(parent);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::CornerStackup),
            "inset child should not trip the lint, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn handrolled_rounded_container_with_matching_corners_does_not_report_corner_stackup() {
        let parent = crate::column([crate::row([crate::text("Header")])
            .fill(crate::tokens::MUTED)
            .radius(Corners::top(crate::tokens::RADIUS_LG))
            .width(Size::Fill(1.0))
            .height(Size::Fixed(24.0))])
        .fill(crate::tokens::CARD)
        .stroke(crate::tokens::BORDER)
        .radius(crate::tokens::RADIUS_LG)
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(96.0));

        let report = lint_one(parent);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::CornerStackup),
            "matching corners should not trip the lint, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn canonical_card_recipe_does_not_report_corner_stackup_after_metrics() {
        // A + B together: the canonical recipe lands in lint with
        // corners already stamped, so the lint stays quiet.
        let root = crate::widgets::card::card([
            crate::widgets::card::card_header([crate::text("Header")]).fill(crate::tokens::MUTED),
            crate::widgets::card::card_content([crate::text("Body")]),
        ])
        .width(Size::Fixed(180.0))
        .height(Size::Fixed(110.0));

        let report = lint_one_with_metrics(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::CornerStackup),
            "canonical card_header(...).fill(...) recipe should be quiet after metrics pass, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn bare_card_with_flush_content_reports_unpadded_surface_panel_issue_24() {
        // Repro for #24: `card([...])` with children that carry their
        // own width/gap config and no slot wrappers and no
        // `.padding(...)` on the card. The row's rect is flush against
        // the card's top stroke (and L/R via Size::Fill(1.0)).
        let root = crate::widgets::card::card([crate::row([
            crate::text("some title").bold(),
            crate::text("description line").muted(),
        ])
        .gap(crate::tokens::SPACE_2)
        .width(Size::Fill(1.0))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(80.0));

        let report = lint_one(root);
        let f = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::UnpaddedSurfacePanel)
            .unwrap_or_else(|| {
                panic!(
                    "expected UnpaddedSurfacePanel finding, got:\n{}",
                    report.text()
                )
            });
        assert!(
            f.message.contains("top"),
            "expected the flushing-side list to call out `top`, got: {}",
            f.message
        );
    }

    #[test]
    fn card_with_explicit_padding_does_not_report_unpadded_surface_panel() {
        // The "dense list-row card" fix from the issue: pad the card
        // itself (the bare slot recipe's SPACE_6 feels too generous).
        let root = crate::widgets::card::card([
            crate::row([crate::text("title").bold()]).width(Size::Fill(1.0))
        ])
        .padding(Sides::all(crate::tokens::SPACE_4))
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(60.0));

        let report = lint_one(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedSurfacePanel),
            "{}",
            report.text()
        );
    }

    #[test]
    fn canonical_card_anatomy_does_not_report_unpadded_surface_panel() {
        // header pads top/left/right at SPACE_6; footer pads
        // bottom/left/right at SPACE_6. Every panel edge is covered
        // by a touching slot child with inward padding on that side.
        let root = crate::widgets::card::card([
            crate::widgets::card::card_header([crate::widgets::card::card_title("Header")]),
            crate::widgets::card::card_content([crate::text("Body")]),
            crate::widgets::card::card_footer([crate::text("footer")]),
        ])
        .width(Size::Fixed(220.0))
        .height(Size::Fixed(160.0));

        let report = lint_one(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedSurfacePanel),
            "canonical slot anatomy should be quiet, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn sidebar_widget_does_not_report_unpadded_surface_panel() {
        // sidebar() carries default_padding(SPACE_4), so the panel
        // itself insets content from every edge.
        let root = crate::widgets::sidebar::sidebar([crate::text("nav")]);

        let report = lint_one(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::UnpaddedSurfacePanel),
            "{}",
            report.text()
        );
    }

    #[test]
    fn raw_color_fires_without_allow_lint() {
        // Sanity check for the suppression tests below — confirms the
        // baseline finding exists when nothing is silenced. A raw rgba
        // fill on a Group is the textbook RawColor case.
        let root = crate::column(Vec::<El>::new())
            .fill(crate::Color::srgb_u8a(40, 50, 60, 255))
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::RawColor),
            "{}",
            report.text()
        );
    }

    #[test]
    fn allow_lint_silences_finding_on_same_node() {
        // The same shape as the sanity test, plus `.allow_lint(RawColor)`
        // on the offending node. The finding must not fire.
        let root = crate::column(Vec::<El>::new())
            .fill(crate::Color::srgb_u8a(40, 50, 60, 255))
            .allow_lint(FindingKind::RawColor)
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(40.0));

        let report = lint_one(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::RawColor),
            "expected RawColor silenced on the allowed node, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn allow_lint_does_not_leak_to_siblings() {
        // Sibling 1 silences RawColor on itself; sibling 2 keeps the
        // raw fill. Only sibling 1's finding should be missing.
        let row = crate::row([
            crate::column(Vec::<El>::new())
                .fill(crate::Color::srgb_u8a(40, 50, 60, 255))
                .allow_lint(FindingKind::RawColor)
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0)),
            crate::column(Vec::<El>::new())
                .fill(crate::Color::srgb_u8a(70, 80, 90, 255))
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0)),
        ])
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(40.0));

        let report = lint_one(row);
        let raw_color_count = report
            .findings
            .iter()
            .filter(|f| f.kind == FindingKind::RawColor)
            .count();
        assert_eq!(
            raw_color_count,
            1,
            "expected exactly one RawColor finding (the un-silenced sibling), got:\n{}",
            report.text()
        );
    }

    #[test]
    fn allow_lint_does_not_propagate_to_descendants() {
        // Parent silences RawColor on itself; child has its own raw
        // fill. The parent's allow_lint must not silence the child.
        let parent = crate::column([crate::column(Vec::<El>::new())
            .fill(crate::Color::srgb_u8a(70, 80, 90, 255))
            .width(Size::Fixed(20.0))
            .height(Size::Fixed(20.0))])
        .fill(crate::Color::srgb_u8a(40, 50, 60, 255))
        .allow_lint(FindingKind::RawColor)
        .width(Size::Fixed(40.0))
        .height(Size::Fixed(40.0));

        let report = lint_one(parent);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::RawColor),
            "child RawColor must still fire when only parent silenced it, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn allow_lint_silences_text_overflow_on_same_node() {
        // The clipped-nowrap text from `clipped_nowrap_text_reports_text_overflow`,
        // plus `.allow_lint(FindingKind::TextOverflow)`. The text-overflow
        // finding's attribution target is the text node itself.
        let root = crate::text("A very long dashboard label")
            .allow_lint(FindingKind::TextOverflow)
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(20.0));

        let report = lint_one(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn lint_report_retain_drops_matching_findings() {
        // The escape hatch for cases per-node allow can't reach —
        // notably DuplicateId, which is emitted post-walk and has no
        // attribution target to mark. Build a tree with two
        // explicitly-keyed siblings sharing a key (the only way to
        // collide computed_id under the path-based scheme), confirm
        // the finding fires, then retain it away.
        let root = crate::row([crate::text("a").key("dup"), crate::text("b").key("dup")])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(20.0));

        let mut report = lint_one(root);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::DuplicateId),
            "baseline DuplicateId must fire, got:\n{}",
            report.text()
        );

        report.retain(|f| f.kind != FindingKind::DuplicateId);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::DuplicateId),
            "retain should have dropped DuplicateId, got:\n{}",
            report.text()
        );
    }

    #[test]
    fn popover_composed_in_flow_reports_misplaced_overlay_layer() {
        // Issue #142's shape: select_menu rendered as a conditional
        // sibling of its trigger inside a scrolled column. The popover
        // layer lays out in-flow, its scrim doesn't cover the
        // viewport, and the menu silently never appears.
        let root = crate::stack([crate::scroll([
            crate::select_trigger("pick", "Current"),
            crate::select_menu("pick", [(1, "One"), (2, "Two")]),
        ])]);

        let report = lint_one(root);
        let finding = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::MisplacedOverlayLayer);
        let finding = finding.unwrap_or_else(|| {
            panic!(
                "MisplacedOverlayLayer must fire for an in-flow popover, got:\n{}",
                report.text()
            )
        });
        assert!(
            finding.message.contains("scrolls or clips"),
            "message should name the scrolling ancestor, got: {}",
            finding.message
        );
    }

    #[test]
    fn popover_and_modal_composed_at_root_stay_quiet() {
        // The documented composition: layers as root-stack siblings
        // via overlays(). Both the transparent popover scrim and the
        // modal's filled scrim resolve to the viewport rect.
        let root = crate::overlays(
            crate::column([crate::select_trigger("pick", "Current")]),
            [
                Some(crate::select_menu("pick", [(1, "One"), (2, "Two")])),
                Some(crate::modal("confirm", "Delete?", [crate::text("Sure?")])),
            ],
        );

        let report = lint_one(root);
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::MisplacedOverlayLayer),
            "root-composed layers must stay quiet, got:\n{}",
            report.text()
        );
    }

    // ---- Accessibility lints (arc 2b) ----------------------------------

    fn has(report: &LintReport, kind: FindingKind) -> bool {
        report.findings.iter().any(|f| f.kind == kind)
    }

    #[test]
    fn unlabeled_icon_button_reports_no_accessible_name() {
        let report = lint_one(crate::column([
            crate::icon_button(crate::IconName::Plus).key("add")
        ]));
        assert!(
            has(&report, FindingKind::NoAccessibleName),
            "icon-only focusable must fire, got:\n{}",
            report.text()
        );

        // An aria_label names it…
        let report = lint_one(crate::column([crate::icon_button(crate::IconName::Plus)
            .key("add")
            .aria_label("New tab")]));
        assert!(
            !has(&report, FindingKind::NoAccessibleName),
            "{}",
            report.text()
        );

        // …and so does a tooltip (HTML `title` fallback, mirrored by
        // the AccessKit lowering).
        let report = lint_one(crate::column([crate::icon_button(crate::IconName::Plus)
            .key("add")
            .tooltip("New tab")]));
        assert!(
            !has(&report, FindingKind::NoAccessibleName),
            "{}",
            report.text()
        );

        // Name-from-content covers text buttons.
        let report = lint_one(crate::column([crate::button("Save").key("save")]));
        assert!(
            !has(&report, FindingKind::NoAccessibleName),
            "{}",
            report.text()
        );
    }

    #[test]
    fn hidden_focusable_reports_no_accessible_name() {
        // Even a labeled control is unannounceable under aria_hidden —
        // the finding fires with the hidden-specific message.
        let report = lint_one(crate::column([crate::icon_button(crate::IconName::Plus)
            .key("add")
            .aria_label("New tab")
            .aria_hidden()]));
        let finding = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::NoAccessibleName)
            .unwrap_or_else(|| panic!("hidden focusable must fire:\n{}", report.text()));
        assert!(
            finding.message.contains("aria_hidden"),
            "message should call out the hidden state: {}",
            finding.message
        );
    }

    #[test]
    fn image_role_without_alt_fires_and_alt_or_hidden_silences() {
        let img = || El::new(Kind::Group).role(crate::a11y::Role::Img);
        let report = lint_one(crate::column([img()]));
        assert!(
            has(&report, FindingKind::ImageWithoutAlt),
            "{}",
            report.text()
        );

        let report = lint_one(crate::column([img().alt("Boarding pass QR code")]));
        assert!(
            !has(&report, FindingKind::ImageWithoutAlt),
            "{}",
            report.text()
        );

        let report = lint_one(crate::column([img().aria_hidden()]));
        assert!(
            !has(&report, FindingKind::ImageWithoutAlt),
            "{}",
            report.text()
        );
    }

    #[test]
    fn low_contrast_text_fires_and_default_foreground_passes() {
        use crate::color::Color;
        // #464646 on the dark page background: ~2.1:1.
        let report = lint_one(crate::column([
            crate::text("dim").text_color(Color::srgb_u8(70, 70, 70))
        ]));
        let finding = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::LowContrastText)
            .unwrap_or_else(|| panic!("dim text must fire:\n{}", report.text()));
        assert!(
            finding.message.contains(":1"),
            "message should show the measured ratio: {}",
            finding.message
        );

        let report = lint_one(crate::column([crate::text("body")]));
        assert!(
            !has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );
    }

    #[test]
    fn large_text_uses_the_relaxed_contrast_floor() {
        use crate::color::Color;
        // #6e6e6e on the dark page background: ~3.9:1 — fails the
        // 4.5:1 normal-text floor, passes the 3:1 large-text floor.
        let c = Color::srgb_u8(110, 110, 110);
        let report = lint_one(crate::column([crate::text("dim").text_color(c)]));
        assert!(
            has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );

        let report = lint_one(crate::column([crate::text("dim")
            .text_color(c)
            .font_size(24.0)]));
        assert!(
            !has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );
    }

    #[test]
    fn contrast_resolves_tokens_through_the_active_palette() {
        // Default FOREGROUND's compile-time rgb is the *dark* palette's
        // near-white. Under the light theme it must resolve to the
        // light palette's near-black before measuring — a missing
        // resolve would read near-white-on-white and fire.
        let mut root = crate::column([crate::text("body")]);
        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        let report = super::lint(&root, &ui_state, &crate::theme::Theme::damascene_light());
        assert!(
            !has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );
    }

    #[test]
    fn contrast_skips_disabled_reduced_opacity_and_unknown_backdrops() {
        use crate::color::Color;
        let dim = || crate::text("dim").text_color(Color::srgb_u8(70, 70, 70));

        // WCAG exempts inactive controls.
        let report = lint_one(crate::column([dim().aria_disabled(true)]));
        assert!(
            !has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );

        // Reduced opacity makes the painted result unknowable here.
        let report = lint_one(crate::column([dim().opacity(0.5)]));
        assert!(
            !has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );

        // Overlay layers past the first paint over arbitrary pixels —
        // quiet until an opaque fill re-establishes the surface.
        let report = lint_one(crate::overlays(
            crate::column(Vec::<El>::new()),
            [Some(crate::column([dim()]))],
        ));
        assert!(
            !has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );

        // An opaque panel fill re-establishes it: fires again.
        let report = lint_one(crate::overlays(
            crate::column(Vec::<El>::new()),
            [Some(crate::column([dim()]).fill(crate::tokens::CARD))],
        ));
        assert!(
            has(&report, FindingKind::LowContrastText),
            "{}",
            report.text()
        );
    }

    #[test]
    fn tinted_status_text_meets_aa_in_every_palette() {
        // Pins the `*_TINT_FOREGROUND` palette values: every status
        // treatment that puts status-colored text on an app surface —
        // tinted badges, text-only status text, the destructive form
        // message — must clear WCAG AA under every stock theme. This
        // is the regression fence for the arc-2b finding that the
        // fill-grade status tokens were doubling as text colors
        // (dark destructive text measured 1.73:1 on its own tint).
        let themes = [
            ("damascene-dark", crate::theme::Theme::damascene_dark()),
            ("damascene-light", crate::theme::Theme::damascene_light()),
            (
                "radix-slate-blue-dark",
                crate::theme::Theme::radix_slate_blue_dark(),
            ),
            (
                "radix-slate-blue-light",
                crate::theme::Theme::radix_slate_blue_light(),
            ),
            (
                "radix-sand-amber-dark",
                crate::theme::Theme::radix_sand_amber_dark(),
            ),
            (
                "radix-sand-amber-light",
                crate::theme::Theme::radix_sand_amber_light(),
            ),
            (
                "radix-mauve-violet-dark",
                crate::theme::Theme::radix_mauve_violet_dark(),
            ),
            (
                "radix-mauve-violet-light",
                crate::theme::Theme::radix_mauve_violet_light(),
            ),
        ];
        for (name, theme) in themes {
            // Status treatments on the page, and again inside a card —
            // the two surfaces status chrome actually sits on.
            let statuses = || {
                crate::column([
                    crate::badge("Info").info(),
                    crate::badge("Success").success(),
                    crate::badge("Warning").warning(),
                    crate::badge("Destructive").destructive(),
                    crate::text("failed").destructive(),
                    crate::text("behind").warning(),
                    crate::text("passed").success(),
                    crate::text("note").info(),
                    crate::widgets::form::form_message("Enter a valid email"),
                ])
            };
            let mut root = crate::column([
                statuses(),
                crate::card([statuses()]).fill(crate::tokens::CARD),
            ]);
            let mut ui_state = UiState::new();
            layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 400.0, 900.0));
            let report = super::lint(&root, &ui_state, &theme);
            let contrast: Vec<_> = report
                .findings
                .iter()
                .filter(|f| f.kind == FindingKind::LowContrastText)
                .collect();
            assert!(
                contrast.is_empty(),
                "[{name}] status text must meet AA:\n{}",
                report.text()
            );
        }
    }

    #[test]
    fn packed_small_targets_fire_and_isolated_or_spaced_ones_pass() {
        let tiny = |k: &str| {
            El::new(Kind::Group)
                .key(k)
                .focusable()
                .aria_label(k)
                .width(Size::Fixed(16.0))
                .height(Size::Fixed(16.0))
        };

        // 16px targets 4px apart: the 24px clearance circles collide.
        let report = lint_one(crate::row([tiny("a"), tiny("b")]).gap(4.0));
        assert!(
            has(&report, FindingKind::SmallHitTarget),
            "{}",
            report.text()
        );

        // Isolated: the spacing exception (and invisible touch
        // inflation) rescue it.
        let report = lint_one(crate::row([tiny("a")]));
        assert!(
            !has(&report, FindingKind::SmallHitTarget),
            "{}",
            report.text()
        );

        // Spaced beyond the clearance circle: quiet.
        let report = lint_one(crate::row([tiny("a"), tiny("b")]).gap(24.0));
        assert!(
            !has(&report, FindingKind::SmallHitTarget),
            "{}",
            report.text()
        );

        // Suppressible per node like every lint.
        let report = lint_one(
            crate::row([
                tiny("a").allow_lint(FindingKind::SmallHitTarget),
                tiny("b").allow_lint(FindingKind::SmallHitTarget),
            ])
            .gap(4.0),
        );
        assert!(
            !has(&report, FindingKind::SmallHitTarget),
            "{}",
            report.text()
        );
    }
}
