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

use std::fmt::Write as _;

use crate::layout;
use crate::metrics::MetricsRole;
use crate::state::UiState;
use crate::tree::*;

/// A single lint finding.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Finding {
    pub kind: FindingKind,
    pub node_id: String,
    pub source: Source,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    RawColor,
    Overflow,
    TextOverflow,
    DuplicateId,
    Alignment,
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
    /// cuts it, or because a later-painted sibling's rect overlaps the
    /// bleed region and paints on top.
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
    ScrollbarObscuresFocusable,
    /// Two sibling keyed nodes have overlapping effective pointer hit
    /// targets because at least one of them opted into
    /// `.hit_overflow(...)`. Hit-test resolves by paint order, so the
    /// later-painted sibling silently owns the collision region while
    /// the earlier sibling may still visually appear nearby.
    ///
    /// Fix: reduce the hit overflow, add real layout gap/padding, or
    /// restructure so one visible row/control owns the whole intended
    /// target area.
    HitOverflowCollision,
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
}

#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct LintReport {
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
pub fn lint(root: &El, ui_state: &UiState) -> LintReport {
    let mut r = LintReport::default();
    let mut seen_ids: std::collections::BTreeMap<String, usize> = Default::default();
    walk(
        root,
        None,
        None,
        &ClipCtx::None,
        ui_state,
        &mut r,
        &mut seen_ids,
    );
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
    check_unpadded_viewport_leaves(root, ui_state, &mut r);
    r
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
fn check_unpadded_viewport_leaves<'a>(root: &'a El, ui_state: &UiState, r: &mut LintReport) {
    const PAD_EPS: f32 = 0.5;
    let touch_eps = crate::tokens::RING_WIDTH;
    let vp = ui_state.rect(&root.computed_id);
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
        ui_state: &UiState,
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
            let rect = ui_state.rect(&n.computed_id);
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
        if matches!(n.kind, Kind::Scroll | Kind::VirtualList) {
            // Scrolled content lives in content space: its rects are
            // clipped by the scroll viewport and shift with the scroll
            // offset, so a leaf landing flush against the window edge
            // is coincidence, not missing window padding.
            return;
        }
        for c in &n.children {
            rec(c, self_blame, false, vp, touch_eps, ui_state, found);
        }
    }
    rec(root, None, true, vp, touch_eps, ui_state, &mut found);

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
                node_id: leaf.computed_id.clone(),
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
            node_id: root.computed_id.clone(),
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
        finding.node_id, target.computed_id,
        "lint::push_for: target must be the finding's attribution node",
    );
    if target.allow_lint.contains(&finding.kind) {
        return;
    }
    r.findings.push(finding);
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
    },
}

fn walk(
    n: &El,
    parent_kind: Option<&Kind>,
    parent_blame: Option<Source>,
    nearest_clip: &ClipCtx,
    ui_state: &UiState,
    r: &mut LintReport,
    seen: &mut std::collections::BTreeMap<String, usize>,
) {
    *seen.entry(n.computed_id.clone()).or_default() += 1;
    let computed = ui_state.rect(&n.computed_id);

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
                    node_id: n.computed_id.clone(),
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
                    node_id: n.computed_id.clone(),
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
                    node_id: n.computed_id.clone(),
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
                    node_id: n.computed_id.clone(),
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
                    node_id: n.computed_id.clone(),
                    source: n.source,
                    message:
                        "surface_role(Panel) without a fill paints only stroke + shadow — \
                         wrap in card() / sidebar() / dialog() for the canonical recipe, or set .fill(tokens::CARD)"
                            .to_string(),
                },
            );
        }

        if matches!(n.surface_role, SurfaceRole::Panel) {
            check_unpadded_surface_panel(n, computed, ui_state, r, n.source);
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
                                node_id: n.computed_id.clone(),
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
                                node_id: n.computed_id.clone(),
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

    // Row alignment: mirror CSS flex's default `align-items: stretch`,
    // but catch the common UI-row mistake where a fixed-size visual
    // child (icon/badge/control) is pinned to the row top beside a
    // text sibling. The fix is the familiar `items-center` move:
    // `.align(Align::Center)`.
    if let Some(blame) = self_blame {
        lint_row_alignment(n, computed, ui_state, r, blame);
        lint_overlay_alignment(n, computed, ui_state, r, blame);
        lint_row_visual_text_spacing(n, ui_state, r, blame);
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
                        node_id: n.computed_id.clone(),
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
    let parent_main_overran =
        !suppress_overflow && flex_main_axis_overflowed(n, computed, ui_state);

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
                node_id: n.computed_id.clone(),
            }
        } else {
            ClipCtx::Static(computed)
        }
    } else {
        nearest_clip.clone()
    };

    if !matches!(n.axis, Axis::Overlay)
        && let Some(blame) = self_blame
    {
        lint_hit_overflow_collisions(n, &child_clip, ui_state, r, blame);
    }

    for (child_idx, c) in n.children.iter().enumerate() {
        let from_user_child = is_from_user(c.source);
        let child_blame = if from_user_child {
            Some(c.source)
        } else {
            self_blame
        };

        let c_rect = ui_state.rect(&c.computed_id);
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
                    node_id: c.computed_id.clone(),
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
                    node_id: c.computed_id.clone(),
                    source: blame,
                    message:
                        ".ellipsis() has no effect on Size::Hug text — Hug forces the rect to the intrinsic content width, so the truncation budget equals the content and no glyph is ever trimmed. Set Size::Fill(_) or Size::Fixed(_) on the text or on a wrapping container so the layout can constrain the rect."
                            .to_string(),
                },
            );
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
            check_focus_ring_obscured(
                c,
                c_rect,
                &child_clip,
                &n.children[child_idx + 1..],
                ui_state,
                r,
                blame,
            );
            // Independent of paint_overflow: the focusable's own rect
            // overlaps an ancestor scroll's thumb track (the thumb
            // paints on top of the control whenever it's visible).
            check_scrollbar_overlap(c, c_rect, &child_clip, ui_state, r, blame);
        }

        walk(
            c,
            Some(&n.kind),
            child_blame,
            &child_clip,
            ui_state,
            r,
            seen,
        );
    }
}

fn focus_ring_overflow(n: &El) -> Sides {
    match n.focus_ring_placement {
        crate::tree::FocusRingPlacement::Outside => Sides::all(crate::tokens::RING_WIDTH),
        crate::tree::FocusRingPlacement::Inside => Sides::zero(),
    }
}

fn has_hit_overflow(sides: Sides) -> bool {
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

/// Detect sibling hit-target ambiguity introduced by `.hit_overflow`.
/// Plain visual overlap is not this lint's concern; it only fires when
/// an explicitly expanded hit rect reaches another keyed sibling's
/// visual/effective target. Overlay stacks are skipped by the caller,
/// since overlapping hit regions are normal for scrims, modals, and
/// floating layers.
fn lint_hit_overflow_collisions(
    parent: &El,
    child_clip: &ClipCtx,
    ui_state: &UiState,
    r: &mut LintReport,
    blame: Source,
) {
    for (left_idx, left) in parent.children.iter().enumerate() {
        if left.key.is_none() {
            continue;
        }
        let left_rect = ui_state.rect(&left.computed_id);
        let Some(left_hit) = clipped_rect(left_rect.outset(left.hit_overflow), child_clip) else {
            continue;
        };
        for right in parent.children.iter().skip(left_idx + 1) {
            if right.key.is_none() {
                continue;
            }
            if !has_hit_overflow(left.hit_overflow) && !has_hit_overflow(right.hit_overflow) {
                continue;
            }
            let right_rect = ui_state.rect(&right.computed_id);
            let Some(right_hit) = clipped_rect(right_rect.outset(right.hit_overflow), child_clip)
            else {
                continue;
            };
            let Some(overlap) = left_hit.intersect(right_hit) else {
                continue;
            };
            if overlap.w <= 0.5 || overlap.h <= 0.5 {
                continue;
            }

            let left_visual_contains = left_rect.contains(overlap.center_x(), overlap.center_y());
            let right_visual_contains = right_rect.contains(overlap.center_x(), overlap.center_y());
            if left_visual_contains && right_visual_contains {
                // Existing visual overlap is already ambiguous by
                // construction; this lint is about invisible inflation
                // creating a new ambiguous band.
                continue;
            }

            let earlier = left.key.as_deref().unwrap_or("<unkeyed>");
            let later = right.key.as_deref().unwrap_or("<unkeyed>");
            let owner = if has_hit_overflow(right.hit_overflow) {
                right
            } else {
                left
            };
            push_for(
                r,
                owner,
                Finding {
                    kind: FindingKind::HitOverflowCollision,
                    node_id: owner.computed_id.clone(),
                    source: blame,
                    message: format!(
                        "expanded hit targets for sibling keys `{earlier}` and `{later}` overlap by {w:.0}x{h:.0}px — \
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
            node_id: child.computed_id.clone(),
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
fn check_unpadded_surface_panel(
    panel: &El,
    panel_rect: Rect,
    ui_state: &UiState,
    r: &mut LintReport,
    blame: Source,
) {
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
        let cr = ui_state.rect(&c.computed_id);
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
            node_id: panel.computed_id.clone(),
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

fn check_focus_ring_obscured(
    n: &El,
    n_rect: Rect,
    nearest_clip: &ClipCtx,
    later_siblings: &[El],
    ui_state: &UiState,
    r: &mut LintReport,
    blame: Source,
) {
    let ring_overflow = focus_ring_overflow(n);
    if ring_overflow.left <= 0.5
        && ring_overflow.right <= 0.5
        && ring_overflow.top <= 0.5
        && ring_overflow.bottom <= 0.5
    {
        return;
    }
    let band = n_rect.outset(ring_overflow);

    // 1. Clipped by ancestor scissor. For scrollable clips, only the
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
                    node_id: n.computed_id.clone(),
                    source: blame,
                    message: format!(
                        "focus ring band clipped by ancestor scissor (L={dx_left:.0} R={dx_right:.0} T={dy_top:.0} B={dy_bottom:.0}) — give a clipping ancestor padding ≥ tokens::RING_WIDTH on the clipped side",
                    ),
                },
            );
        }
    }

    // 2. Occluded by a later-painted sibling whose rect overlaps the
    // bleed band on a side where the focusable reserves overflow.
    // Skip overlay parents (siblings are intentionally stacked).
    for sib in later_siblings {
        let sib_rect = ui_state.rect(&sib.computed_id);
        if let Some(side) = bleed_occlusion(n_rect, ring_overflow, sib_rect)
            && paints_pixels(sib)
        {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::FocusRingObscured,
                    node_id: n.computed_id.clone(),
                    source: blame,
                    message: format!(
                        "focus ring band occluded on the {side} edge by later-painted sibling {sib_id} — increase gap to ≥ tokens::RING_WIDTH or restructure so the neighbor doesn't sit on the edge",
                        sib_id = sib.computed_id,
                    ),
                },
            );
            // First occluder is enough — don't double-report.
            break;
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
    let ClipCtx::Scrolling { node_id, .. } = nearest_clip else {
        return;
    };
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
            node_id: n.computed_id.clone(),
            source: blame,
            message: format!(
                "scrollbar thumb overlaps this focusable on the right edge by {overlap_x:.0}px (thumb x={thumb_left:.0}..{thumb_right:.0}; control x={ctrl_x:.0}..{ctrl_right:.0}) — move horizontal padding *inside* the scroll, onto a wrapper that constrains children to a narrower content rect, so the thumb sits in a reserved gutter to the right of content",
                ctrl_x = n_rect.x,
                ctrl_right = n_rect.right(),
            ),
        },
    );
}

/// True if `n` paints visible pixels (so it can occlude a sibling's
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

/// Whichever side of `n_rect`'s `paint_overflow` band `sib_rect`
/// intersects (above the EPS adjacency threshold). `EPS` keeps a
/// sibling whose edge merely touches the focusable's edge (gap = 0)
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

fn lint_row_alignment(
    n: &El,
    computed: Rect,
    ui_state: &UiState,
    r: &mut LintReport,
    blame: Source,
) {
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
        let child_rect = ui_state.rect(&child.computed_id);
        let top_pinned = (child_rect.y - inner.y).abs() <= 0.5;
        let visibly_short = child_rect.h + 2.0 < inner.h;
        if top_pinned && visibly_short {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::Alignment,
                    node_id: n.computed_id.clone(),
                    source: blame,
                    message: "row has a fixed-size visual child pinned to the top beside text; add .align(Align::Center) to vertically center row content"
                        .to_string(),
                },
            );
            return;
        }
    }
}

fn lint_overlay_alignment(
    n: &El,
    computed: Rect,
    ui_state: &UiState,
    r: &mut LintReport,
    blame: Source,
) {
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
        let child_rect = ui_state.rect(&child.computed_id);
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
                    node_id: n.computed_id.clone(),
                    source: blame,
                    message: "overlay has a smaller fixed-size visual child pinned to the top-left; add .align(Align::Center).justify(Justify::Center) to center overlay content"
                        .to_string(),
                },
            );
            return;
        }
    }
}

fn lint_row_visual_text_spacing(n: &El, ui_state: &UiState, r: &mut LintReport, blame: Source) {
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

        let visual_rect = ui_state.rect(&visual.computed_id);
        let text_rect = ui_state.rect(&text.computed_id);
        let gap = text_rect.x - visual_rect.right();
        if gap < 4.0 {
            push_for(
                r,
                n,
                Finding {
                    kind: FindingKind::Spacing,
                    node_id: n.computed_id.clone(),
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
fn flex_main_axis_overflowed(parent: &El, parent_rect: Rect, ui_state: &UiState) -> bool {
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
            let r = ui_state.rect(&c.computed_id);
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

    fn lint_one(mut root: El) -> LintReport {
        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        lint(&root, &ui_state)
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
            crate::widgets::text_input::text_input("", &selection, "field"),
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
                    "", &selection, "field",
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
            crate::widgets::text_input::text_input("", &selection, "field"),
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
            "", &selection, "field",
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
            crate::widgets::text_input::text_input("", &selection, "field"),
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
            crate::widgets::text_input::text_input("", &selection, "field"),
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
                        crate::widgets::switch::switch(false).key(format!("row-{i}-toggle")),
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
                        crate::widgets::switch::switch(false).key(format!("row-{i}-toggle")),
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
    fn scrollbar_overlap_lint_quiet_when_content_does_not_overflow() {
        // A `scroll` with content shorter than its viewport doesn't
        // render a thumb, so the bug isn't user-visible. The lint
        // should match — thumb_tracks has no entry for the scroll, so
        // there's nothing to collide against.
        let body = crate::tree::column([crate::tree::row([
            crate::text("only row"),
            crate::tree::spacer(),
            crate::widgets::switch::switch(false).key("only-toggle"),
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
                crate::widgets::text_input::text_input("", &selection, "field"),
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
}
