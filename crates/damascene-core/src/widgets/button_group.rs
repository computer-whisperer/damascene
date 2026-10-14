//! ButtonGroup — adjacent controls joined into one segmented control.
//!
//! Oracle: shadcn/ui `ButtonGroup`
//! (`references/workbench-validation/shadcn-refs/src/components/ui/button-group.tsx`,
//! the pinned late-2025 registry snapshot named in
//! `docs/NAMING_ORACLE.md`). shadcn's horizontal recipe is
//!
//! ```text
//! [&>*:not(:first-child)]:rounded-l-none
//! [&>*:not(:first-child)]:border-l-0
//! [&>*:not(:last-child)]:rounded-r-none
//! ```
//!
//! i.e. *collapse the interior corners, and never draw the interior
//! edge twice.* This module is that recipe: `button_group` is a
//! zero-gap row whose children keep their own variants, events, and
//! keys while the group rewrites the two things that make N controls
//! read as one — corner silhouette and edge seams.
//!
//! The canonical use is the split button:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! button_group([
//!     button("Slice").primary().key("slice"),
//!     icon_button("chevron-down").primary().key("slice:menu"),
//! ])
//! ```
//!
//! # Seams: why not `border_l()` everywhere
//!
//! On the web a CSS border sits *inside* the box, so two flush
//! neighbours paint 2px of border and shadcn has to delete one with
//! `border-l-0`. Damascene's `.stroke(...)`
//! straddles the boundary (see `paint::draw_ops::combined_overflow`),
//! so two flush stroked neighbours land their 1px strokes on the *same*
//! pixel column — joined at `gap = 0` they already read as a single
//! hairline, and deleting one would leave the seam blank.
//!
//! What does need help is the case a stroke can't express: a control
//! whose stroke color equals its fill (`.primary()`, `.destructive()`
//! — the solid-tint recipe sets both), or a
//! `.ghost()` control with no stroke at all. There the boundary is
//! invisible, so the trailing item gets a per-side
//! `.border_l()` — the HTML/CSS-oracle
//! primitive, exact here because corner collapse has already zeroed
//! that item's left corner radii. Per-side borders join
//! `content_inset`, so the seam costs the item 1px of content box
//! exactly like CSS `border-box`.
//!
//! Each boundary therefore carries exactly one hairline, whatever mix
//! of variants the author passes.
//!
//! # Seam color: the seam follows the frame
//!
//! The divider is not a constant. It is painted in the group's own
//! **effective frame stroke** — whatever `el.stroke` the group carries
//! when the seams are resolved, falling back to `tokens::BORDER` for a
//! group with no frame of its own (`button_group`, which is chromeless
//! by design). So `toggle_group(...)` seams the trough in
//! `tokens::BORDER` exactly as before, and
//!
//! ```ignore
//! button_group(segments).fill(TROUGH).stroke(INPUT_BORDER)
//! ```
//!
//! seams in `INPUT_BORDER` — one authored color for the frame *and* the
//! dividers, which is how every design in the reference corpus draws a
//! bordered segmented control. The re-point happens inside
//! [`El::stroke`](crate::El::stroke), so the author's chained
//! `.stroke(...)` reaches seams that were resolved when the group was
//! built.
//!
//! The suppression rule follows from the same color: a neighbour's own
//! edge stands in for the seam only when it would *coincide* with it —
//! same pixels **and** same color. A `.current()` segment strokes
//! `tokens::BORDER`, so it swallows the seam of a default trough but
//! not the seam of a trough the author stroked some other color; there
//! the divider is drawn and the segment's own outline reads as a
//! separate selection edge, which is what the references show.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom`, the
//! per-side border chainables, `.hit_overflow()` /
//! `.focus_ring_inside()`, and the `RadiusOrigin::LibraryShape` corner
//! stamp that `tabs_list` uses. An app crate can fork this file and
//! produce an equivalent widget against the same public API.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::tokens;
use crate::tree::*;

/// Join controls into one segmented control — shadcn's `ButtonGroup`.
///
/// A `Size::Hug` row (shadcn's `w-fit`) with no gap. The children keep
/// their own fills, variants, keys, and event contracts; the group only
/// rewrites what makes them read as one control:
///
/// - **Corner collapse.** The first child keeps its leading corners,
///   the last its trailing corners, everything between is squared. A
///   lone child is left alone. The stamp is
///   [`RadiusOrigin::LibraryShape`], so the silhouette survives the
///   metrics pass (`apply_control` only restamps
///   [`RadiusOrigin::ThemeDefault`]) while
///   [`Theme::with_radius_scale`](crate::Theme::with_radius_scale)
///   still scales it — same contract as `tabs_list`'s edge triggers. A
///   child with an explicit `.radius(...)`
///   ([`RadiusOrigin::Fixed`]) keeps it; the author asked for that
///   shape.
/// - **Seams.** Exactly one hairline per boundary, painted in the
///   group's own frame stroke — `tokens::BORDER` for a bare
///   `button_group`, the authored color once the author chains
///   `.stroke(...)`. See the module docs for the seam rules.
/// - **Flush-neighbour hygiene.** Joined children drop their
///   `hit_overflow` (an expanded target would steal the neighbour's
///   edge band — `FindingKind::HitOverflowCollision`) and switch to an
///   inside focus ring (an outward ring is painted over by the next
///   sibling — `FindingKind::FocusRingObscured`). Same idiom as
///   `numeric_input`'s stacked chevron column.
///
/// Unlike [`crate::widgets::toggle::toggle_group`] the group draws no
/// frame of its own: it has no fill and no stroke, so a group of
/// `.ghost()` buttons stays chromeless, exactly as shadcn's
/// `<ButtonGroup>` (a bare `role="group"` flex box) does. Give it one —
/// `.fill(trough).stroke(edge)` — and it becomes a bordered segmented
/// control whose seams paint in `edge` too.
///
/// ```ignore
/// use damascene_core::prelude::*;
///
/// // Split button: primary action + its menu affordance.
/// button_group([
///     button("Save").primary().key("save"),
///     icon_button("chevron-down").primary().key("save:menu"),
/// ])
/// ```
#[track_caller]
pub fn button_group(children: impl IntoIterator<Item = El>) -> El {
    let caller = Location::caller();
    let mut items: Vec<El> = children.into_iter().collect();
    join_row(&mut items);
    El::new(Kind::Custom("button_group"))
        .at_loc(caller)
        .axis(Axis::Row)
        // shadcn's `items-stretch`: a group of mixed-height children
        // still reads as one control.
        .align(Align::Stretch)
        .gap(tokens::SPACE_0)
        .children(items)
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Turn a row of independently-styled controls into one joined
/// segmented control, in place.
///
/// Shared by [`button_group`] and
/// [`crate::widgets::toggle::toggle_group`] so both anatomies collapse
/// corners and resolve seams by the same rule. See [`button_group`] for
/// the contract and the module docs for the seam reasoning.
///
/// Seams are resolved against `tokens::BORDER` here — the color a group
/// with no frame of its own draws. A group that carries (or later
/// receives) a frame stroke re-points them through [`restroke_seams`].
pub(crate) fn join_row(items: &mut [El]) {
    let last = items.len().saturating_sub(1);
    for (i, item) in items.iter_mut().enumerate() {
        collapse_corners(item, i == 0, i == last);
        // Flush neighbours: an expanded hit target reaches into the
        // next item's rect (`HitOverflowCollision`) and an outward
        // focus ring is painted over by it (`FocusRingObscured`).
        // Inside the joined trough there is no whitespace for either to
        // live in, so both are traded for the dense-stack idiom.
        *item = std::mem::take(item)
            .hit_overflow(Sides::zero())
            .focus_ring_inside();
    }
    resolve_seams(items, tokens::BORDER);
}

/// The color a joined group paints into its seams: its own effective
/// frame stroke, or `tokens::BORDER` when it has no frame.
fn seam_color(group: &El) -> Color {
    match group.stroke {
        Some(c) if group.stroke_width > 0.0 => c,
        _ => tokens::BORDER,
    }
}

/// Re-resolve an already-joined group's seams against its current frame
/// stroke. Called from [`El::stroke`](crate::El::stroke), which is how
/// an author's chained `.stroke(...)` reaches seams that
/// [`join_row`] resolved when the group was built — the frame and the
/// dividers are one authored color (see the module docs).
///
/// A no-op on everything that is not a joined group, so the cost on the
/// general `.stroke(...)` path is one `Kind` discriminant test.
pub(crate) fn restroke_seams(group: &mut El) {
    if !matches!(
        group.kind,
        Kind::Custom("button_group") | Kind::Custom("toggle_group")
    ) {
        return;
    }
    let seam = seam_color(group);
    resolve_seams(&mut group.children, seam);
}

/// Give every interior boundary exactly one `seam`-colored hairline.
///
/// The left-border slot of a joined child is **library-owned**: the
/// group installs the seam there and clears it again when a neighbour's
/// own edge takes the boundary over, so the pass is idempotent and can
/// be re-run when the seam color changes. (Corners and `hit_overflow`
/// are taken over the same way — the group owns what makes N controls
/// read as one.)
fn resolve_seams(items: &mut [El], seam: Color) {
    for i in 1..items.len() {
        if coincides_with_seam(&items[i - 1], seam) || coincides_with_seam(&items[i], seam) {
            // One of the two strokes already lands this exact hairline
            // on the shared boundary (strokes straddle it, so
            // neighbouring strokes coincide instead of doubling).
            // Adding a border here is what *would* double it.
            clear_seam(&mut items[i]);
            continue;
        }
        items[i] = std::mem::take(&mut items[i])
            .border_l()
            .border_color(seam);
    }
}

/// Would this element's own edge land on the shared boundary *in the
/// seam color* — i.e. is the seam already drawn?
///
/// Two conditions, both required:
///
/// - **Visible.** False for `.ghost()` (no stroke) and for the solid
///   tints (`.primary()`, `.destructive()` — `tint` sets stroke = fill,
///   so the stroke is invisible against the fill it sits on).
/// - **The same color.** A `.secondary()` / `.outline()` / `.current()`
///   segment strokes a token of its own; only when that resolves to the
///   seam color do the two hairlines become one. Otherwise the seam is
///   a distinct line the design asked for and the segment's outline is
///   its own edge — suppressing it was the bug this predicate replaces.
///
/// Colors compare token-and-all, so `tokens::BORDER` never stands in for
/// a hand-picked color that merely happens to share the default
/// palette's rgb: the two would diverge under
/// [`Theme::with_palette`](crate::Theme::with_palette).
fn coincides_with_seam(el: &El, seam: Color) -> bool {
    let Some(stroke) = el.stroke else {
        return false;
    };
    if el.stroke_width <= 0.0 || Some(stroke) == el.fill {
        return false;
    }
    stroke == seam
}

/// Release the library-owned seam slot, dropping the whole
/// [`BorderSpec`] if nothing else was using it — so an unseamed child
/// keeps `border == None`, the state it would have had if the seam had
/// never been installed.
fn clear_seam(item: &mut El) {
    let Some(spec) = item.border.as_deref_mut() else {
        return;
    };
    if spec.widths.left == 0.0 {
        return;
    }
    spec.widths.left = 0.0;
    if spec.widths == Sides::zero() {
        item.border = None;
    }
}

/// Keep the leading corners on the first item, the trailing corners on
/// the last, and square everything else — shadcn's
/// `rounded-l-none` / `rounded-r-none` pair. Magnitudes come from the
/// item's own radius, so a group of buttons keeps the button radius and
/// a group of tab triggers keeps the trigger radius.
fn collapse_corners(item: &mut El, first: bool, last: bool) {
    if first && last {
        // A lone child is not joined to anything.
        return;
    }
    if item.radius_origin == RadiusOrigin::Fixed {
        // The author set `.radius(...)` explicitly — their shape wins,
        // the same carve-out `tabs_list` makes for customized triggers.
        return;
    }
    let r = item.radius;
    item.radius = Corners {
        tl: if first { r.tl } else { 0.0 },
        bl: if first { r.bl } else { 0.0 },
        tr: if last { r.tr } else { 0.0 },
        br: if last { r.br } else { 0.0 },
    };
    // `LibraryShape`, not `Fixed`: the segment silhouette is ours (the
    // metrics pass must not restamp it) but its magnitude is still the
    // theme's, so `Theme::with_radius_scale(0.0)` squares a joined
    // group along with everything else.
    item.radius_origin = RadiusOrigin::LibraryShape;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::widgets::button::{button, icon_button};

    fn radii(el: &El) -> (f32, f32, f32, f32) {
        (el.radius.tl, el.radius.tr, el.radius.br, el.radius.bl)
    }

    #[test]
    fn group_collapses_interior_corners() {
        let g = button_group([button("One"), button("Two"), button("Three")]);
        let r = tokens::RADIUS_MD;
        assert_eq!(
            radii(&g.children[0]),
            (r, 0.0, 0.0, r),
            "first item keeps only its leading corners",
        );
        assert_eq!(
            radii(&g.children[1]),
            (0.0, 0.0, 0.0, 0.0),
            "middle items are squared on both ends",
        );
        assert_eq!(
            radii(&g.children[2]),
            (0.0, r, r, 0.0),
            "last item keeps only its trailing corners",
        );
    }

    #[test]
    fn collapsed_corners_are_library_shape() {
        // `LibraryShape` is the load-bearing origin: `apply_control` in
        // the metrics pass restamps `ThemeDefault` radii (which would
        // re-round the interior corners), while `with_radius_scale`
        // still scales `LibraryShape`.
        let g = button_group([button("One"), button("Two")]);
        for child in &g.children {
            assert_eq!(child.radius_origin, RadiusOrigin::LibraryShape);
        }
    }

    #[test]
    fn collapsed_silhouette_survives_the_metrics_pass() {
        let mut g = button_group([button("One"), button("Two")]);
        crate::Theme::default().apply_metrics(&mut g);
        assert_eq!(g.children[0].radius.tr, 0.0, "interior corner stays square");
        assert_eq!(g.children[1].radius.tl, 0.0, "interior corner stays square");
        assert!(
            g.children[0].radius.tl > 0.0,
            "outer corner keeps the silhouette",
        );
    }

    #[test]
    fn collapsed_silhouette_still_scales_with_the_theme() {
        let mut g = button_group([button("One"), button("Two")]);
        crate::Theme::default()
            .with_radius_scale(0.0)
            .apply_metrics(&mut g);
        assert_eq!(
            radii(&g.children[0]),
            (0.0, 0.0, 0.0, 0.0),
            "radius_scale(0.0) squares the joined silhouette too",
        );
    }

    #[test]
    fn lone_child_keeps_its_whole_silhouette() {
        let g = button_group([button("Only")]);
        let r = tokens::RADIUS_MD;
        assert_eq!(radii(&g.children[0]), (r, r, r, r));
    }

    #[test]
    fn author_set_radius_wins() {
        let g = button_group([button("One").radius(2.0), button("Two")]);
        assert_eq!(
            radii(&g.children[0]),
            (2.0, 2.0, 2.0, 2.0),
            "an explicit `.radius(...)` is the author's shape",
        );
        assert_eq!(g.children[0].radius_origin, RadiusOrigin::Fixed);
    }

    #[test]
    fn stroked_neighbours_do_not_get_a_doubled_seam() {
        // `button()` is `.secondary()`: stroke BORDER over fill
        // SECONDARY. Its stroke straddles the shared boundary, so the
        // neighbour's stroke coincides with it — one hairline. A
        // `border_l()` on top would be the second line.
        let g = button_group([button("One"), button("Two"), button("Three")]);
        for (i, child) in g.children.iter().enumerate() {
            assert!(
                child.border.is_none(),
                "child {i} must not add a border on top of coincident strokes",
            );
        }
    }

    #[test]
    fn invisible_seam_gets_exactly_one_left_border() {
        // `.primary()` sets stroke == fill, so nothing marks the
        // boundary; the trailing item owns the divider.
        let g = button_group([
            button("Save").primary(),
            icon_button("chevron-down").primary(),
        ]);
        assert!(
            g.children[0].border.is_none(),
            "the leading item owns no seam",
        );
        let border = g.children[1]
            .border
            .as_deref()
            .expect("the trailing item carries the seam");
        assert_eq!(border.widths.left, 1.0, "one 1px divider");
        assert_eq!(
            (border.widths.right, border.widths.top, border.widths.bottom),
            (0.0, 0.0, 0.0),
            "only the shared edge is bordered",
        );
        assert_eq!(border.color, Some(tokens::BORDER));
    }

    /// The seam color of the boundary before item `i`, or `None` when
    /// that boundary carries no library seam.
    fn seam(g: &El, i: usize) -> Option<Option<Color>> {
        let b = g.children[i].border.as_deref()?;
        (b.widths.left > 0.0).then_some(b.color)
    }

    /// The slicer_match target's `input.border`.
    const EDGE: Color = Color::srgb_u8(58, 65, 77);
    /// Any second hand-picked edge color.
    const EDGE2: Color = Color::srgb_u8(120, 40, 40);

    #[test]
    fn seams_follow_the_authored_group_stroke() {
        // The blocked case (slicer_match's local `segmented()`): a
        // bordered trough whose seams must sit on the frame color, not
        // on the region-rule token.
        let g = button_group([button("One").ghost(), button("Two").ghost()]).stroke(EDGE);
        assert_eq!(g.stroke, Some(EDGE), "the frame is the authored color");
        assert_eq!(seam(&g, 1), Some(Some(EDGE)), "and so is the seam");
    }

    #[test]
    fn an_unstroked_group_keeps_the_default_seam() {
        // `button_group` is chromeless by design; with no frame of its
        // own the seam stays on the region-rule token.
        let g = button_group([button("One").ghost(), button("Two").ghost()]);
        assert!(g.stroke.is_none(), "no frame of its own");
        assert_eq!(seam(&g, 1), Some(Some(tokens::BORDER)));
    }

    #[test]
    fn a_coincident_neighbour_edge_still_swallows_the_seam() {
        // `.secondary()` strokes BORDER; stroke an unadorned group in
        // BORDER too and the two hairlines are the same line, so the
        // seam must stay suppressed.
        let g = button_group([button("One"), button("Two")]).stroke(tokens::BORDER);
        assert_eq!(g.children[1].stroke, Some(tokens::BORDER));
        assert!(
            g.children[1].border.is_none(),
            "a coincident edge is the seam; a border would double it",
        );
    }

    #[test]
    fn a_differently_colored_neighbour_edge_no_longer_swallows_the_seam() {
        // The measured slicer_match regression: the selected segment
        // strokes `tokens::BORDER` while the trough is stroked
        // `EDGE`, so the two lines are *different* lines — the divider
        // is the design's, and the segment outline is its own.
        let g = button_group([button("One").current(), button("Two").ghost()]).stroke(EDGE);
        assert_eq!(g.children[0].stroke, Some(tokens::BORDER));
        assert_eq!(
            seam(&g, 1),
            Some(Some(EDGE)),
            "a neighbour edge in another color is not this seam",
        );
    }

    #[test]
    fn restroking_releases_a_seam_that_became_redundant() {
        // The re-point runs in both directions: stroke the group in the
        // segments' own edge color and the seam it had installed must
        // go away again, back to `border == None`.
        let g = button_group([button("One").ghost(), button("Two").ghost()]);
        assert!(g.children[1].border.is_some(), "ghost pair starts seamed");
        let g = g.stroke(EDGE2);
        assert_eq!(seam(&g, 1), Some(Some(EDGE2)));
        // Now a variant whose own stroke coincides with a re-pointed
        // seam: both segments stroke BORDER, group stroked BORDER.
        let g = button_group([button("One"), button("Two")])
            .stroke(EDGE)
            .stroke(tokens::BORDER);
        assert!(
            g.children[1].border.is_none(),
            "the seam slot is released, not left as a zero-width spec",
        );
    }

    #[test]
    fn a_solid_tint_seam_follows_the_frame_too() {
        // `.primary()` sets stroke == fill, so nothing marks the
        // boundary and the seam is always drawn — in the frame color.
        let g = button_group([
            button("Save").primary(),
            icon_button("chevron-down").primary(),
        ])
        .stroke(EDGE);
        assert_eq!(seam(&g, 1), Some(Some(EDGE)));
    }

    #[test]
    fn ghost_neighbours_get_a_divider() {
        let g = button_group([button("One").ghost(), button("Two").ghost()]);
        assert_eq!(
            g.children[1]
                .border
                .as_deref()
                .map(|b| b.widths.left)
                .unwrap_or(0.0),
            1.0,
            "two chromeless items still need a visible division",
        );
    }

    #[test]
    fn split_button_keeps_its_children_interactive() {
        let g = button_group([
            button("Save").primary().key("save"),
            icon_button("chevron-down").primary().key("save:menu"),
        ]);
        assert_eq!(g.children.len(), 2, "no divider nodes are injected");
        assert_eq!(g.children[0].key.as_deref(), Some("save"));
        assert_eq!(g.children[1].key.as_deref(), Some("save:menu"));
        assert!(g.children.iter().all(|c| c.focusable));
        assert_eq!(
            g.children[1].width,
            Size::Fixed(tokens::CONTROL_HEIGHT),
            "the chevron half stays a square icon button",
        );
        assert!(g.key.is_none(), "the group is chrome, not a target");
        assert!(g.fill.is_none() && g.stroke.is_none(), "no frame of its own");
    }

    #[test]
    fn joined_children_are_flush() {
        let mut g = button_group([button("One"), button("Two"), button("Three")]);
        let mut state = UiState::new();
        layout(&mut g, &mut state, Rect::new(0.0, 0.0, 400.0, 60.0));
        for pair in g.children.windows(2) {
            let (a, b) = (pair[0].computed_rect, pair[1].computed_rect);
            assert!(
                (b.x - (a.x + a.w)).abs() < 0.01,
                "items must touch: {a:?} then {b:?}",
            );
        }
    }

    #[test]
    fn joined_children_drop_flush_neighbour_hazards() {
        let g = button_group([button("One"), button("Two")]);
        for child in &g.children {
            assert_eq!(
                child.hit_overflow,
                Sides::zero(),
                "an expanded target would reach into the neighbour",
            );
            assert_eq!(child.focus_ring_placement, FocusRingPlacement::Inside);
        }
    }

    #[test]
    fn joined_group_is_lint_clean() {
        use crate::bundle::lint::FindingKind;
        let mut root = crate::tree::column([button_group([
            button("One"),
            button("Two"),
            button("Three"),
        ])])
        .padding(Sides::all(tokens::SPACE_4));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 120.0));
        let report = crate::bundle::lint::lint(&root, &state, &crate::theme::Theme::default());
        let relevant: Vec<_> = report
            .findings
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FindingKind::HitOverflowCollision
                        | FindingKind::FocusRingObscured
                        | FindingKind::CornerStackup
                )
            })
            .collect();
        assert!(
            relevant.is_empty(),
            "joined groups must not trip flush-neighbour lints:\n{}",
            report.text(),
        );
    }
}
