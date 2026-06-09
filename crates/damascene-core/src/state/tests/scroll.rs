use super::support::*;
use crate::scroll::{ScrollAlignment, ScrollRequest};
use crate::tree::{virtual_list, virtual_list_dyn};

#[test]
fn hit_test_through_scrolled_content() {
    // Three 60px buttons in a 100px-tall scroll viewport. The
    // second button is initially below the visible area.
    // After scrolling 60px, button[1] is now at the top.
    let mut tree = scroll([
        button("zero").key("b0").height(Size::Fixed(60.0)),
        button("one").key("b1").height(Size::Fixed(60.0)),
        button("two").key("b2").height(Size::Fixed(60.0)),
    ])
    .key("list")
    .height(Size::Fixed(100.0));
    let mut state = UiState::new();
    assign_ids(&mut tree);
    state.scroll.offsets.insert(tree.computed_id.clone(), 60.0);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

    // Buttons hug their text width — click at b1's center after the
    // scroll shift to land inside its actual rect.
    let r1 = find_rect(&tree, &state, "b1").expect("b1 rect");
    let hit = hit_test(&tree, &state, (r1.center_x(), r1.center_y()));
    assert_eq!(hit.as_deref(), Some("b1"));

    // b0 has been scrolled above the viewport — clicking where it
    // would now sit (above y=0) misses it.
    let r0 = find_rect(&tree, &state, "b0").expect("b0 rect");
    assert!(
        r0.bottom() <= 0.0,
        "b0 should be above the viewport, was {:?}",
        r0
    );
}
#[test]
fn pointer_wheel_routes_to_deepest_scrollable() {
    // Outer scroll containing an inner scroll. Wheel events at the
    // inner's center should target the inner when the inner has room
    // to move in the wheel direction.
    let mut tree = scroll([
        button("above").key("above").height(Size::Fixed(80.0)),
        scroll((0..4).map(|i| {
            button(format!("inner-row-{i}"))
                .key(format!("inner-row-{i}"))
                .height(Size::Fixed(50.0))
        }))
        .key("inner")
        .height(Size::Fixed(100.0)),
        button("below").key("below").height(Size::Fixed(200.0)),
    ])
    .key("outer")
    .height(Size::Fixed(160.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 160.0));

    let inner_rect = find_rect(&tree, &state, "inner-row-0").expect("inner row rect");
    let routed = state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);
    assert!(routed, "wheel should route to a scrollable");
    let inner_id = find_id_for_kind(&tree, "inner").expect("inner id");
    assert!((state.scroll_offset(&inner_id) - 30.0).abs() < 0.5);
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
}

#[test]
fn pointer_wheel_bubbles_when_deepest_scrollable_has_no_overflow() {
    let mut tree = scroll([
        button("above").key("above").height(Size::Fixed(80.0)),
        scroll([button("inner-row")
            .key("inner-row")
            .height(Size::Fixed(60.0))])
        .key("inner")
        .height(Size::Fixed(100.0)),
        button("below").key("below").height(Size::Fixed(200.0)),
    ])
    .key("outer")
    .height(Size::Fixed(160.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 160.0));

    let inner_rect = find_rect(&tree, &state, "inner-row").expect("inner row rect");
    let routed = state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);
    assert!(
        routed,
        "wheel should bubble to the overflowing outer scroll"
    );

    let inner_id = find_id_for_kind(&tree, "inner").expect("inner id");
    assert!((state.scroll_offset(&inner_id) - 0.0).abs() < 0.5);
    assert!((state.scroll_offset(&tree.computed_id) - 30.0).abs() < 0.5);
}

#[test]
fn pointer_wheel_bubbles_when_deepest_scrollable_is_at_directional_edge() {
    let mut tree = scroll([
        button("above").key("above").height(Size::Fixed(80.0)),
        scroll((0..4).map(|i| {
            button(format!("inner-row-{i}"))
                .key(format!("inner-row-{i}"))
                .height(Size::Fixed(50.0))
        }))
        .key("inner")
        .height(Size::Fixed(100.0)),
        button("below").key("below").height(Size::Fixed(200.0)),
    ])
    .key("outer")
    .height(Size::Fixed(160.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 160.0));
    let inner_id = find_id_for_kind(&tree, "inner").expect("inner id");
    state.scroll.offsets.insert(inner_id.clone(), 100.0);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 160.0));

    let inner_rect = find_rect(&tree, &state, "inner-row-2").expect("visible inner row rect");
    let routed = state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);
    assert!(routed, "downward wheel at inner tail should bubble outward");
    assert!((state.scroll_offset(&inner_id) - 100.0).abs() < 0.5);
    assert!((state.scroll_offset(&tree.computed_id) - 30.0).abs() < 0.5);
}

#[test]
fn pointer_wheel_does_not_bubble_past_block_pointer_barrier() {
    // Page scroll behind a dialog. The dialog content carries
    // `block_pointer` (as `dialog_content` does). When the inner
    // scroll inside the dialog is exhausted, the wheel must NOT
    // bubble out to the page scroll underneath.
    let mut tree = scroll([
        button("page-row")
            .key("page-row")
            .height(Size::Fixed(200.0)),
        column([scroll([button("inner-row")
            .key("inner-row")
            .height(Size::Fixed(40.0))])
        .key("dialog-scroll")
        .height(Size::Fixed(80.0))])
        .block_pointer()
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0)),
    ])
    .key("page")
    .height(Size::Fixed(160.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 160.0));

    let inner_rect = find_rect(&tree, &state, "inner-row").expect("inner row rect");
    let routed = state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);

    let page_id = tree.computed_id.clone();
    let dialog_id = find_id_for_kind(&tree, "dialog-scroll").expect("dialog scroll id");
    assert!(
        !routed,
        "wheel must be absorbed by the block_pointer barrier when nothing inside the dialog can scroll"
    );
    assert!((state.scroll_offset(&dialog_id) - 0.0).abs() < 0.5);
    assert!(
        (state.scroll_offset(&page_id) - 0.0).abs() < 0.5,
        "page scroll behind the dialog must not move"
    );
}

fn fixed_list_root(count: usize, row_height: f32) -> El {
    virtual_list(count, row_height, |i| {
        crate::widgets::text::text(format!("r{i}"))
    })
    .key("list")
}

fn dyn_list_root(count: usize, est: f32, row_h: f32) -> El {
    virtual_list_dyn(
        count,
        est,
        |i| format!("row-{i}"),
        move |i| {
            column([crate::widgets::text::text(format!("r{i}"))])
                .key(format!("row-{i}"))
                .height(Size::Fixed(row_h))
        },
    )
    .key("dyn-list")
}

#[test]
fn scroll_request_start_aligns_row_top_to_viewport_top() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 10, ScrollAlignment::Start)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let stored = state.scroll_offset(&tree.computed_id);
    assert!(
        (stored - 500.0).abs() < 0.5,
        "expected offset 500, got {stored}"
    );
}

#[test]
fn scroll_request_end_aligns_row_bottom_to_viewport_bottom() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 10, ScrollAlignment::End)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 350.0).abs() < 0.5);
}

#[test]
fn scroll_request_center_centres_row_in_viewport() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        10,
        ScrollAlignment::Center,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 425.0).abs() < 0.5);
}

#[test]
fn scroll_request_visible_is_noop_when_row_already_in_viewport() {
    // 50 rows × 50px, viewport 200px, current offset 100 → rows 2..6
    // visible. Requesting row 3 with Visible should leave offset
    // unchanged.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    assign_ids(&mut tree);
    state.scroll.offsets.insert(tree.computed_id.clone(), 100.0);
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        3,
        ScrollAlignment::Visible,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 100.0).abs() < 0.5);
}

#[test]
fn scroll_request_visible_scrolls_min_distance_for_offscreen_row() {
    // Same 50 × 50px list; current offset 100; row 20 is below the
    // viewport (rows 2..6 visible). Visible alignment → scroll just
    // enough to put row 20's bottom at the viewport bottom: 21*50 -
    // 200 = 850.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    assign_ids(&mut tree);
    state.scroll.offsets.insert(tree.computed_id.clone(), 100.0);
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        20,
        ScrollAlignment::Visible,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 850.0).abs() < 0.5);
}

#[test]
fn scroll_request_clamps_to_max_offset() {
    // Resolved offset can land past content end (e.g. End on the last
    // row); write_virtual_scroll_state must clamp it to [0, max].
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 49, ScrollAlignment::End)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 2300.0).abs() < 0.5);
}

#[test]
fn scroll_request_unknown_list_drops_silently() {
    // Request targets a list that's not in the tree; offset must
    // remain at its initial value, and the queue must be empty after
    // layout (clean drop, not a leak).
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new(
        "no-such-list",
        10,
        ScrollAlignment::Start,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
    state.clear_pending_scroll_requests();

    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
    assert!(state.scroll.pending_requests.is_empty());
}

#[test]
fn scroll_request_out_of_range_row_drops_silently() {
    // count = 50, request row 999. The matching list takes the
    // request from the queue (so it doesn't leak), then drops it
    // because the row index is past the end.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        999,
        ScrollAlignment::Start,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
    assert!(state.scroll.pending_requests.is_empty());
}

#[test]
fn scroll_request_first_frame_works_without_prior_layout() {
    // The whole point of the deferred design: the app pushes the
    // request before any layout has run, the very first layout pass
    // resolves it correctly because viewport and row geometry are
    // available right then.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 25, ScrollAlignment::Start)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 1250.0).abs() < 0.5);
}

#[test]
fn scroll_request_resolves_against_dynamic_list_with_warm_cache() {
    // Measured rows use the cached height; unmeasured rows above the
    // target fall back to the configured estimate. The expected
    // offset is recomputed below from the actual measured_count
    // because layout decides how many rows fit in the first frame.
    let mut tree = dyn_list_root(50, 50.0, 30.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let measured_count = state
        .scroll
        .measured_row_heights
        .get(&tree.computed_id)
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(
        measured_count >= 6,
        "expected first frame to measure ≥6 rows, got {measured_count}"
    );
    let id = tree.computed_id.clone();

    let mut tree2 = dyn_list_root(50, 50.0, 30.0);
    state.push_scroll_requests(vec![ScrollRequest::new(
        "dyn-list",
        10,
        ScrollAlignment::Start,
    )]);
    layout(&mut tree2, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let expected = (measured_count.min(10)) as f32 * 30.0
        + (10_usize.saturating_sub(measured_count)) as f32 * 50.0;
    let stored = state.scroll_offset(&id);
    assert!(
        (stored - expected).abs() < 0.5,
        "expected offset {expected}, got {stored}"
    );
}

#[test]
fn scroll_request_resolves_dynamic_list_row_key() {
    let mut tree = dyn_list_root(50, 50.0, 30.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let measured_count = state
        .scroll
        .measured_row_heights
        .get(&tree.computed_id)
        .map(|m| m.len())
        .unwrap_or(0);
    let id = tree.computed_id.clone();

    let mut tree2 = dyn_list_root(50, 50.0, 30.0);
    state.push_scroll_requests(vec![ScrollRequest::to_row_key(
        "dyn-list",
        "row-10",
        ScrollAlignment::Start,
    )]);
    layout(&mut tree2, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let expected = (measured_count.min(10)) as f32 * 30.0
        + (10_usize.saturating_sub(measured_count)) as f32 * 50.0;
    let stored = state.scroll_offset(&id);
    assert!(
        (stored - expected).abs() < 0.5,
        "expected row-key scroll offset {expected}, got {stored}"
    );
}

#[test]
fn scroll_request_unknown_row_key_drops_silently() {
    let mut tree = dyn_list_root(50, 50.0, 30.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::to_row_key(
        "dyn-list",
        "missing-row",
        ScrollAlignment::Start,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
    assert!(state.scroll.pending_requests.is_empty());
}

#[test]
fn scroll_request_last_match_wins_when_multiple_target_same_list() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![
        ScrollRequest::new("list", 5, ScrollAlignment::Start),
        ScrollRequest::new("list", 30, ScrollAlignment::Start),
    ]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 1500.0).abs() < 0.5);
}

fn chat_tree(rows: usize, row_h: f32, pin: bool) -> El {
    let kids: Vec<El> = (0..rows)
        .map(|i| {
            button(format!("m{i}"))
                .key(format!("m{i}"))
                .height(Size::Fixed(row_h))
        })
        .collect();
    let mut s = scroll(kids).key("chat").height(Size::Fixed(100.0));
    if pin {
        s = s.pin_end();
    }
    s
}

#[test]
fn pin_end_starts_at_tail_on_first_layout() {
    // Four 40px rows = content_h 160; viewport 100 → max_offset 60.
    // pin_end means first frame paints with the tail visible.
    let mut tree = chat_tree(4, 40.0, true);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        (offset - 60.0).abs() < 0.5,
        "expected first-frame offset = max_offset 60, got {offset}"
    );
}

#[test]
fn without_pin_end_first_layout_starts_at_head() {
    // Same geometry, no .pin_end() — offset should default to 0.
    let mut tree = chat_tree(4, 40.0, false);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
}

#[test]
fn pin_end_follows_content_growth() {
    // Frame 1: 3 rows × 40px = 120; viewport 100 → max=20; pinned → 20.
    let mut tree = chat_tree(3, 40.0, true);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 20.0).abs() < 0.5);

    // Frame 2: a new message arrives. 4 rows × 40 = 160; max=60.
    // Pin is still engaged → offset snaps to the new max.
    let mut tree = chat_tree(4, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        (offset - 60.0).abs() < 0.5,
        "expected offset to track new tail 60, got {offset}"
    );
}

#[test]
fn pin_end_releases_when_user_scrolls_up() {
    let mut tree = chat_tree(5, 40.0, true);
    let mut state = UiState::new();
    // Frame 1: pinned at max = 200 - 100 = 100.
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 100.0).abs() < 0.5);

    // User wheels up by 40 (negative dy moves content up — but wheel
    // convention here is `dy` is added to the offset, so we send -40 to
    // scroll toward the head).
    let center = (100.0, 50.0);
    state.pointer_wheel(&tree, center, -40.0);
    assert!((state.scroll_offset(&tree.computed_id) - 60.0).abs() < 0.5);

    // Next layout: pin should release; offset stays at 60 even though
    // content grew.
    let mut tree = chat_tree(6, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        (offset - 60.0).abs() < 0.5,
        "expected pin to release and offset to stay at 60, got {offset}"
    );
}

#[test]
fn pin_end_re_engages_when_user_returns_to_tail() {
    let mut tree = chat_tree(5, 40.0, true);
    let mut state = UiState::new();
    // Frame 1: pinned at max=100.
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    // Scroll up to 60, layout to commit the release.
    state.pointer_wheel(&tree, (100.0, 50.0), -40.0);
    let mut tree = chat_tree(5, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 60.0).abs() < 0.5);

    // User scrolls back down to the tail (wheel by +40 brings stored
    // back to 100).
    state.pointer_wheel(&tree, (100.0, 50.0), 40.0);

    // Now content grows: 7 rows × 40 = 280; max = 180. Pin re-engaged
    // on the previous frame's return to tail, so the offset tracks the
    // new max.
    let mut tree = chat_tree(7, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        (offset - 180.0).abs() < 0.5,
        "expected pin to re-engage and offset to track new tail 180, got {offset}"
    );
}

#[test]
fn pin_end_survives_viewport_resize() {
    // First frame at viewport_h=100, 5 rows × 40 = 200; max=100; pinned.
    let mut tree = chat_tree(5, 40.0, true);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 100.0).abs() < 0.5);

    // Viewport grows to 150 (sidebar dragged narrower / window resize).
    // New max = 200 - 150 = 50. Pin still engaged → snap to 50.
    let mut tree = chat_tree(5, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 150.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        (offset - 50.0).abs() < 0.5,
        "expected pin to follow viewport resize to new max 50, got {offset}"
    );
}

#[test]
fn pin_end_off_does_not_follow_content_growth() {
    // No .pin_end(). Stored offset stays where the user (or default)
    // last put it; content growth does not move it.
    let mut tree = chat_tree(3, 40.0, false);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);

    let mut tree = chat_tree(6, 40.0, false);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        (offset - 0.0).abs() < 0.5,
        "expected offset to stay at head without pin_end, got {offset}"
    );
}

#[test]
fn pin_end_releases_on_ensure_visible_to_non_tail_anchor() {
    // Pinned, then an EnsureVisible request targets the first message.
    let mut tree = chat_tree(6, 40.0, true);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 140.0).abs() < 0.5);

    state.push_scroll_requests(vec![ScrollRequest::ensure_visible("chat", 0.0, 40.0)]);
    let mut tree = chat_tree(6, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        offset < 60.0,
        "EnsureVisible toward the head should release the pin and scroll up; got {offset}"
    );

    // Content grows; pin is no longer engaged, offset should not track
    // the new tail.
    let mut tree = chat_tree(10, 40.0, true);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let after = state.scroll_offset(&tree.computed_id);
    assert!(
        after < 100.0,
        "expected pin released; growth should not drag offset to new tail, got {after}"
    );
}

#[test]
fn pin_end_with_short_content_is_a_no_op_clamp() {
    // Content shorter than viewport: max_offset = 0, no scrolling.
    // pin_end should not break this case — offset stays at 0.
    let mut tree = chat_tree(1, 40.0, true);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
}

/// Mirror of [`chat_tree`] for pin_start tests. Builds the scroll with
/// a stable key so the offset persists across the per-test rebuilds,
/// and labels each child with an explicit key (`r{i}`) so the
/// anchor-preservation tests can prepend new rows while keeping the
/// old rows identifiable.
fn commit_tree(rows: usize, row_h: f32, pin: bool, label_prefix: &str) -> El {
    let kids: Vec<El> = (0..rows)
        .map(|i| {
            button(format!("{label_prefix}{i}"))
                .key(format!("{label_prefix}{i}"))
                .height(Size::Fixed(row_h))
        })
        .collect();
    let mut s = scroll(kids).key("commits").height(Size::Fixed(100.0));
    if pin {
        s = s.pin_start();
    }
    s
}

#[test]
fn pin_start_starts_at_head_on_first_layout() {
    // Four 40px rows = content_h 160; viewport 100 → max_offset 60.
    // pin_start means first frame paints with the head visible, which
    // is also the default — this test just guards the engagement state
    // so subsequent prepends are auto-snapped (see
    // pin_start_overrides_anchor_when_rows_prepended).
    let mut tree = commit_tree(4, 40.0, true, "c");
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
}

#[test]
fn pin_start_overrides_anchor_when_rows_prepended() {
    // Frame 1: rows c0..c3, pinned at head. Offset = 0.
    let mut tree = commit_tree(4, 40.0, true, "c");
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);

    // Frame 2: two new rows prepended (so children become n0, n1, c0,
    // c1, c2, c3). The scroll-anchor system would normally shift the
    // offset to keep c0 at its previous viewport position (= 80px down,
    // so the user does not visually jump). With pin_start engaged the
    // anchor preservation is overridden and the offset stays at 0,
    // exposing the freshly-prepended n0 / n1 rows.
    let prepended: Vec<El> = (0..2)
        .map(|i| {
            button(format!("n{i}"))
                .key(format!("n{i}"))
                .height(Size::Fixed(40.0))
        })
        .chain((0..4).map(|i| {
            button(format!("c{i}"))
                .key(format!("c{i}"))
                .height(Size::Fixed(40.0))
        }))
        .collect();
    let mut tree = scroll(prepended)
        .key("commits")
        .height(Size::Fixed(100.0))
        .pin_start();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        offset.abs() < 0.5,
        "pin_start should hold the offset at 0 across a prepend; got {offset}",
    );
}

#[test]
fn pin_start_releases_when_user_scrolls_down() {
    let mut tree = commit_tree(5, 40.0, true, "c");
    let mut state = UiState::new();
    // Frame 1: pinned at head (offset 0).
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);

    // User wheels down by +40, moving the offset off the head.
    state.pointer_wheel(&tree, (100.0, 50.0), 40.0);
    assert!((state.scroll_offset(&tree.computed_id) - 40.0).abs() < 0.5);

    // Next layout: pin releases; offset stays at 40 even though new
    // rows get prepended.
    let prepended: Vec<El> = (0..2)
        .map(|i| {
            button(format!("n{i}"))
                .key(format!("n{i}"))
                .height(Size::Fixed(40.0))
        })
        .chain((0..5).map(|i| {
            button(format!("c{i}"))
                .key(format!("c{i}"))
                .height(Size::Fixed(40.0))
        }))
        .collect();
    let mut tree = scroll(prepended)
        .key("commits")
        .height(Size::Fixed(100.0))
        .pin_start();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    // Anchor preservation should keep the previously-visible row at
    // the same viewport y, so after a 2-row prepend (80px of new
    // content above) the offset lands near 40 + 80 = 120.
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        offset > 60.0,
        "pin released; anchor preservation should shift offset past 60 to keep prior row stable, got {offset}",
    );
}

#[test]
fn pin_start_re_engages_when_user_returns_to_head() {
    let mut tree = commit_tree(5, 40.0, true, "c");
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    // User scrolls down to 40 to break the pin, then layout once to
    // commit the release.
    state.pointer_wheel(&tree, (100.0, 50.0), 40.0);
    let mut tree = commit_tree(5, 40.0, true, "c");
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 40.0).abs() < 0.5);

    // User scrolls back to the head.
    state.pointer_wheel(&tree, (100.0, 50.0), -40.0);

    // Next frame two rows are prepended; pin re-engaged on the
    // previous frame's return to head, so the offset stays at 0.
    let prepended: Vec<El> = (0..2)
        .map(|i| {
            button(format!("n{i}"))
                .key(format!("n{i}"))
                .height(Size::Fixed(40.0))
        })
        .chain((0..5).map(|i| {
            button(format!("c{i}"))
                .key(format!("c{i}"))
                .height(Size::Fixed(40.0))
        }))
        .collect();
    let mut tree = scroll(prepended)
        .key("commits")
        .height(Size::Fixed(100.0))
        .pin_start();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        offset.abs() < 0.5,
        "pin should re-engage and hold offset at 0, got {offset}",
    );
}

#[test]
fn pin_start_survives_viewport_resize() {
    // Frame 1 at viewport_h=100, 5 rows × 40 = 200; max=100; pinned.
    let mut tree = commit_tree(5, 40.0, true, "c");
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);

    // Viewport shrinks to 60. Pin still engaged → offset stays at 0.
    let mut tree = commit_tree(5, 40.0, true, "c");
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 60.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        offset.abs() < 0.5,
        "expected pin to hold offset at 0 across viewport resize, got {offset}",
    );
}

#[test]
fn pin_start_with_short_content_is_a_no_op_clamp() {
    // Content shorter than viewport: max_offset = 0, no scrolling.
    let mut tree = commit_tree(1, 40.0, true, "c");
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
}

#[test]
fn pin_start_off_lets_anchor_preserve_visible_row() {
    // Without pin_start the scroll-anchor system preserves the
    // previously-visible row's viewport position across a prepend,
    // shifting the offset by the height of the new rows.
    let mut tree = commit_tree(4, 40.0, false, "c");
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);

    let prepended: Vec<El> = (0..2)
        .map(|i| {
            button(format!("n{i}"))
                .key(format!("n{i}"))
                .height(Size::Fixed(40.0))
        })
        .chain((0..4).map(|i| {
            button(format!("c{i}"))
                .key(format!("c{i}"))
                .height(Size::Fixed(40.0))
        }))
        .collect();
    let mut tree = scroll(prepended).key("commits").height(Size::Fixed(100.0));
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let offset = state.scroll_offset(&tree.computed_id);
    assert!(
        offset > 40.0,
        "without pin_start the anchor should shift offset past 40 to keep the prior top row stable, got {offset}",
    );
}

// --- Persistent-map LRU GC (issue #57) -------------------------------

/// Shorthand: run the per-frame GC against `tree`'s live ids.
fn gc(state: &mut UiState, tree: &El) {
    state.gc_scroll_state(tree);
}

fn lru_cap() -> usize {
    super::super::scroll::SCROLL_LRU_CAP
}

#[test]
fn scroll_gc_preserves_absent_entries_under_cap() {
    // The load-bearing behavior: a scrollable's offset survives its
    // node leaving the tree (tab switch), so remounting restores the
    // scroll position. The GC must not touch absent entries while the
    // registry is under the cap.
    let mut tree = scroll([button("a").key("a").height(Size::Fixed(400.0))])
        .key("tab-body")
        .height(Size::Fixed(100.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let id = tree.computed_id.clone();
    state.set_scroll_offset(&id, 120.0);

    // Switch "tabs": many frames against a tree without the scrollable.
    let mut other = button("other").key("other");
    assign_ids(&mut other);
    for _ in 0..10 {
        gc(&mut state, &other);
    }
    assert_eq!(
        state.scroll_offset(&id),
        120.0,
        "absent scroll offsets persist under the cap (tab-switch restoration)"
    );
}

#[test]
fn scroll_gc_evicts_longest_unseen_when_over_cap() {
    let mut state = UiState::new();
    let mut empty = button("none").key("none");
    assign_ids(&mut empty);

    // One old identity, stamped in an early frame...
    state.set_scroll_offset("old-scrollable", 50.0);
    gc(&mut state, &empty);

    // ...then enough younger identities to overflow the cap.
    for i in 0..lru_cap() {
        state.set_scroll_offset(format!("young-{i}"), i as f32);
    }
    gc(&mut state, &empty);

    assert_eq!(
        state.scroll.offsets.len(),
        lru_cap(),
        "the registry is bounded at the cap"
    );
    assert!(
        !state.scroll.offsets.contains_key("old-scrollable"),
        "the longest-unseen identity is the one evicted"
    );
    assert_eq!(
        state.scroll_offset("young-0"),
        0.0,
        "younger identities survive"
    );
}

#[test]
fn scroll_gc_never_evicts_live_entries() {
    // A scrollable currently in the tree must survive any overflow.
    let mut tree = scroll([button("a").key("a").height(Size::Fixed(400.0))])
        .key("live-one")
        .height(Size::Fixed(100.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
    let live_id = tree.computed_id.clone();
    state.set_scroll_offset(&live_id, 33.0);
    gc(&mut state, &tree);

    for i in 0..lru_cap() {
        state.set_scroll_offset(format!("dead-{i}"), i as f32);
    }
    gc(&mut state, &tree);

    assert_eq!(
        state.scroll_offset(&live_id),
        33.0,
        "live identities are exempt from eviction"
    );
    assert_eq!(state.scroll.offsets.len(), lru_cap());
}

#[test]
fn scroll_gc_drops_virtual_list_state_as_a_unit() {
    // Virtual lists carry more than a float: eviction must clear the
    // row-measurement subtree and the anchor along with the offset.
    use super::super::types::VirtualAnchor;
    let mut state = UiState::new();
    let mut empty = button("none").key("none");
    assign_ids(&mut empty);

    let list_id = "dead-dyn-list".to_string();
    state.set_scroll_offset(&list_id, 800.0);
    let mut rows = rustc_hash::FxHashMap::default();
    for i in 0..100 {
        let mut buckets = rustc_hash::FxHashMap::default();
        buckets.insert(640u32, 24.0f32);
        rows.insert(format!("row-{i}"), buckets);
    }
    state
        .scroll
        .measured_row_heights
        .insert(list_id.clone(), rows);
    state.scroll.virtual_anchors.insert(
        list_id.clone(),
        VirtualAnchor {
            row_key: "row-3".into(),
            row_index: 3,
            row_fraction: 0.5,
            viewport_y: 10.0,
            resolved_offset: 800.0,
        },
    );
    gc(&mut state, &empty); // stamp as oldest

    for i in 0..lru_cap() {
        state.set_scroll_offset(format!("young-{i}"), 0.0);
    }
    gc(&mut state, &empty);

    assert!(
        !state.scroll.offsets.contains_key(&list_id)
            && !state.scroll.measured_row_heights.contains_key(&list_id)
            && !state.scroll.virtual_anchors.contains_key(&list_id),
        "eviction removes every per-identity map entry, including the \
         virtual-list measurement cache"
    );
}

#[test]
fn dynamic_row_width_buckets_are_capped() {
    // Resizing a window sweeps through 1-px width buckets; only the
    // ones near the current width are retained (issue #57).
    let mut state = UiState::new();
    let mut list_id = None;
    for w in 0..20 {
        let mut tree = dyn_list_root(10, 24.0, 24.0);
        let width = 300.0 + (w as f32) * 10.0;
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, width, 200.0));
        list_id = Some(tree.computed_id.clone());
    }
    let list_id = list_id.unwrap();
    let rows = state
        .scroll
        .measured_row_heights
        .get(&list_id)
        .expect("dyn list has measurements");
    assert!(!rows.is_empty());
    for (row, buckets) in rows {
        assert!(
            buckets.len() <= 8,
            "row {row} retains {} width buckets — cap is 8",
            buckets.len()
        );
    }
}
