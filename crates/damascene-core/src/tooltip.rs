//! Hover-driven tooltips.
//!
//! Apps attach tooltips with `.tooltip(text)` on any element. The
//! runtime — not the app — decides when to show them: after the
//! pointer rests on the trigger for [`HOVER_DELAY`], the runtime
//! synthesizes a floating tooltip layer at the El root, anchored to
//! the trigger's laid-out rect. Pointer-leave or press dismisses.
//!
//! The synthesized layer is appended to the user's tree before
//! layout, so it goes through the normal layout / draw_ops / paint
//! pipeline — no separate tooltip render pass. It carries
//! `Kind::Custom("tooltip_layer")` so inspectors and the
//! popover-focus pass can recognize it (the focus stack ignores
//! tooltips since they aren't interactive).
//!
//! ## Why library-driven
//!
//! Apps could compose tooltips by hand (set hover state per node,
//! build a popover layer when hover is "settled"). That's a lot of
//! per-app plumbing for a behavior every native UI shares: the
//! library already tracks hover targets and animates per-node
//! envelopes; tooltips are a small natural extension.
//!
//! See `docs/LIBRARY_VISION.md` for why tooltips are the one
//! runtime-appended floating layer (modals and popovers stay
//! app-owned).

use std::time::Duration;

use web_time::Instant;

use crate::state::UiState;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::popover::{Anchor, anchor_rect};

/// How long the pointer must rest on a tooltipped node before its
/// tooltip appears. Matches the typical native default (~500ms).
pub const HOVER_DELAY: Duration = Duration::from_millis(500);

/// The runtime's tooltip-synthesis pass. Inspects the current hover
/// state and, when a tooltip is due, appends a tooltip layer to
/// `root`. Returns `true` when another frame is needed to keep the
/// hover-delay timer ticking (no tooltip yet, but one will be due
/// soon); the caller folds this into its `needs_redraw` signal so
/// the host doesn't idle through the delay.
///
/// Must be called after [`crate::layout::assign_ids`] (so the tree
/// has stable `computed_id`s to look up by) and before
/// [`crate::layout::layout`] (so the appended layer goes through
/// the same layout pass as everything else).
///
/// **Root precondition:** the appended layer is a sibling of the
/// app's [`crate::App::build`] return value. For it to overlay (and
/// not compete for flex space) the root must be an `Axis::Overlay`
/// container — typically `overlays(main, [])`, the same convention
/// used for user-composed popovers and modals. Debug builds panic
/// on a non-overlay root.
pub fn synthesize_tooltip(root: &mut El, ui_state: &UiState, now: Instant) -> bool {
    // Suppressed: pointer is pressed (about to click — don't pop a
    // tooltip in the user's face), or this hover already had its
    // tooltip dismissed by a press.
    if ui_state.pressed.is_some() || ui_state.tooltip.dismissed_for_hover {
        return false;
    }
    let Some(hover) = ui_state.hovered.as_ref() else {
        return false;
    };
    let Some(started_at) = ui_state.tooltip.hover_started_at else {
        return false;
    };

    // Tooltip text is snapshotted onto `UiTarget` at hit-test time
    // (`hit_test_rec`), against the previous frame's tree. Reading
    // from the cached `UiTarget` rather than walking the live tree
    // is what makes tooltips work on `virtual_list_dyn` rows: this
    // pass runs before `layout_post_assign` has called `build_row`
    // on the current frame, so the live tree's virtual-list
    // children are still empty.
    let Some(text) = hover.tooltip.as_deref() else {
        return false;
    };

    if now.duration_since(started_at) < HOVER_DELAY {
        // Hover started but delay not elapsed — caller should keep
        // the redraw loop alive so we re-enter once the delay
        // passes. After it elapses, the tooltip layer below kicks
        // in on the next frame.
        return true;
    }

    debug_assert_eq!(
        root.axis,
        Axis::Overlay,
        "synthesize_tooltip: root must be an Axis::Overlay container so the \
         tooltip layer overlays the main view. Wrap your `App::build` return \
         value in `overlays(main, [])`. Got axis = {:?}",
        root.axis,
    );
    root.children
        .push(tooltip_layer(text, hover.node_id.clone()));
    // Assign computed_ids to the pushed layer in-place so the
    // subsequent `layout_post_assign` doesn't have to re-walk the
    // whole tree just to id one new floating subtree. Pairs with
    // `RunnerCore::prepare_layout`'s skip-the-second-id-walk flow.
    let i = root.children.len() - 1;
    crate::layout::assign_id_appended(&root.computed_id, &mut root.children[i], i);
    // Tooltip is now in the tree; further redraws are driven by
    // the layer's fade-in envelope, not by us.
    false
}

/// Build a `Kind::Custom("tooltip_layer")` that fills the viewport
/// and uses [`anchor_rect`] to position its single child (the
/// styled panel) below the trigger, flipping above on viewport
/// collision. Hit-test transparent — the layer doesn't block clicks
/// on whatever is underneath.
fn tooltip_layer(text: &str, anchor_id: String) -> El {
    let panel = tooltip_panel(text);
    El::new(Kind::Custom("tooltip_layer"))
        .child(panel)
        .fill_size()
        .layout(move |ctx| {
            let (w, h) = (ctx.measure)(&ctx.children[0]);
            // Resolve the anchor by id; if the trigger has been
            // laid out (it should have — we're inside the same
            // layout pass), this returns its rect. If somehow it
            // hasn't, anchor_rect's None-fallback puts the panel at
            // the viewport origin, which is ugly but visible.
            let rect = anchor_rect(
                &Anchor::below_id(&anchor_id),
                (w, h),
                ctx.container,
                ctx.rect_of_id,
                crate::widgets::popover::ANCHOR_GAP,
            );
            vec![rect]
        })
}

/// The styled tooltip surface — small, hugs its content, soft
/// shadow, no scrim. Long strings get a single line at intrinsic
/// width; line wrapping for paragraph-length tooltips is a v2
/// concern (depends on width-aware measure).
fn tooltip_panel(text: &str) -> El {
    El::new(Kind::Custom("tooltip_panel"))
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Popover)
        .child(
            El::new(Kind::Text)
                .style_profile(StyleProfile::TextOnly)
                .text(text.to_string())
                .text_role(TextRole::Caption)
                .text_color(tokens::FOREGROUND),
        )
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .shadow(tokens::SHADOW_MD)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .gap(0.0)
        .width(Size::Hug)
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::UiTarget;
    use crate::layout::{assign_ids, layout};
    use crate::widgets::button::button;

    fn lay_out_with_button() -> (El, UiState) {
        let mut tree = button("Save").key("save").tooltip("Save changes (Ctrl+S)");
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_focus_order(&tree);
        (tree, state)
    }

    #[test]
    fn pre_delay_returns_pending_no_layer() {
        let (mut tree, mut state) = lay_out_with_button();
        let trigger = state
            .focus
            .order
            .iter()
            .find(|t| t.key == "save")
            .cloned()
            .unwrap();
        let now = Instant::now();
        state.set_hovered(Some(trigger), now);

        assign_ids(&mut tree);
        let before = tree.children.len();
        let pending = synthesize_tooltip(&mut tree, &state, now + Duration::from_millis(100));
        assert!(pending, "delay not elapsed → caller should request redraw");
        assert_eq!(tree.children.len(), before, "no tooltip layer appended yet");
    }

    #[test]
    fn post_delay_appends_tooltip_layer() {
        let (mut tree, mut state) = lay_out_with_button();
        let trigger = state
            .focus
            .order
            .iter()
            .find(|t| t.key == "save")
            .cloned()
            .unwrap();
        let now = Instant::now();
        state.set_hovered(Some(trigger), now);

        assign_ids(&mut tree);
        let before = tree.children.len();
        let pending = synthesize_tooltip(
            &mut tree,
            &state,
            now + HOVER_DELAY + Duration::from_millis(1),
        );
        assert!(!pending, "tooltip placed → redraw is now animation-driven");
        assert_eq!(
            tree.children.len(),
            before + 1,
            "tooltip layer appended to root"
        );
        assert!(matches!(
            tree.children.last().unwrap().kind,
            Kind::Custom("tooltip_layer")
        ));
    }

    #[test]
    fn no_tooltip_when_pressed() {
        let (mut tree, mut state) = lay_out_with_button();
        let trigger = state
            .focus
            .order
            .iter()
            .find(|t| t.key == "save")
            .cloned()
            .unwrap();
        let now = Instant::now();
        state.set_hovered(Some(trigger.clone()), now);
        state.pressed = Some(trigger);

        assign_ids(&mut tree);
        let before = tree.children.len();
        let pending = synthesize_tooltip(
            &mut tree,
            &state,
            now + HOVER_DELAY + Duration::from_millis(50),
        );
        assert!(!pending);
        assert_eq!(tree.children.len(), before, "press suppresses the tooltip");
    }

    #[test]
    fn dismissed_for_hover_blocks_until_re_entry() {
        let (mut tree, mut state) = lay_out_with_button();
        let trigger = state
            .focus
            .order
            .iter()
            .find(|t| t.key == "save")
            .cloned()
            .unwrap();
        let now = Instant::now();
        state.set_hovered(Some(trigger), now);
        state.tooltip.dismissed_for_hover = true;

        assign_ids(&mut tree);
        let before = tree.children.len();
        let pending = synthesize_tooltip(
            &mut tree,
            &state,
            now + HOVER_DELAY + Duration::from_millis(50),
        );
        assert!(!pending);
        assert_eq!(
            tree.children.len(),
            before,
            "dismissed flag suppresses tooltip"
        );
    }

    #[test]
    fn tooltip_fires_on_virtual_list_row_after_rebuild() {
        // Regression for issue #23: tooltip synthesis runs before
        // `layout_post_assign` realizes `virtual_list_dyn` rows, so a
        // tree walk for tooltip text on a virtual-list row child
        // always misses. The fix snapshots the text onto `UiTarget`
        // at hit-test time (which reads the *previous* frame's tree,
        // where rows are realized), and `synthesize_tooltip` reads
        // from the cached target instead of walking the live tree.
        //
        // The test models that two-frame sequence: lay out frame 1
        // (rows realized), hit-test to capture the hover, then build
        // a fresh frame 2 tree (rows not realized) and assert that
        // the tooltip layer still appends.
        use crate::hit_test::hit_test_target;
        use crate::tree::virtual_list_dyn;
        use crate::widgets::overlay::overlays;
        use crate::widgets::text::text;

        fn build_tree() -> El {
            overlays(
                virtual_list_dyn(
                    5,
                    30.0,
                    |i| format!("row:{i}"),
                    |i| {
                        text(format!("row {i}"))
                            .key(format!("row:{i}"))
                            .tooltip(format!("tip {i}"))
                            .height(crate::tree::Size::Fixed(30.0))
                    },
                ),
                std::iter::empty::<Option<El>>(),
            )
        }

        // Frame 1: lay out with virtual rows realized so we can
        // hit-test against a row child.
        let mut tree_f1 = build_tree();
        let mut state = UiState::new();
        layout(&mut tree_f1, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        // Hit-test mid-viewport — should land on one of the
        // realized rows. The exact row index isn't load-bearing.
        let target = hit_test_target(&tree_f1, &state, (50.0, 45.0))
            .expect("hit-test should find a realized virtual-list row");
        assert!(
            target.tooltip.is_some(),
            "hit-test should snapshot the row's tooltip text; got {:?}",
            target.tooltip
        );

        let now = Instant::now();
        state.set_hovered(Some(target), now);

        // Frame 2: rebuild from scratch. The virtual list's row
        // closure has not run on this fresh tree — its `children`
        // is empty. Assign ids and synthesize, which exercises the
        // path that used to walk the live tree and miss.
        let mut tree_f2 = build_tree();
        assign_ids(&mut tree_f2);
        let before = tree_f2.children.len();
        let pending = synthesize_tooltip(
            &mut tree_f2,
            &state,
            now + HOVER_DELAY + Duration::from_millis(1),
        );
        assert!(!pending);
        assert_eq!(
            tree_f2.children.len(),
            before + 1,
            "tooltip layer should append even though the virtual list \
             hasn't realized its rows on this frame"
        );
        assert!(matches!(
            tree_f2.children.last().unwrap().kind,
            Kind::Custom("tooltip_layer")
        ));
    }

    #[test]
    fn hover_change_resets_timer_via_set_hovered() {
        let mut state = UiState::new();
        let now = Instant::now();
        let target_a = UiTarget {
            key: "a".into(),
            node_id: "/a".into(),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            tooltip: None,
            scroll_offset_y: 0.0,
        };
        let target_b = UiTarget {
            key: "b".into(),
            node_id: "/b".into(),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            tooltip: None,
            scroll_offset_y: 0.0,
        };
        state.set_hovered(Some(target_a), now);
        let started = state.tooltip.hover_started_at;
        state.set_hovered(Some(target_b), now + Duration::from_millis(100));
        assert!(
            state.tooltip.hover_started_at > started,
            "timer reset on target change"
        );
    }
}
