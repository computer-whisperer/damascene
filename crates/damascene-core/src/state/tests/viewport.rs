//! Layout + state integration tests for `viewport()` pan/zoom: the
//! transform baked into descendant rects, hit-testing through it,
//! programmatic requests (fit / reset), pan clamping, and cursor-anchored
//! wheel zoom.

use super::support::*;
use crate::tree::viewport;
use crate::viewport::{ViewportRequest, ViewportView};

const R: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);
const ORIGIN: (f32, f32) = (0.0, 0.0); // viewport inner top-left (padding 0)

fn approx(a: f32, b: f32) {
    assert!((a - b).abs() < 0.05, "{a} != {b}");
}

/// A viewport with one fixed-size keyed box as its content.
fn vp_tree(w: f32, h: f32) -> El {
    viewport([button("x")
        .key("box")
        .width(Size::Fixed(w))
        .height(Size::Fixed(h))])
    .key("vp")
}

fn vp_id(tree: &El) -> String {
    find_id(tree, "vp").expect("viewport id")
}

#[test]
fn transform_bakes_into_descendant_rects() {
    let mut tree = vp_tree(100.0, 80.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R); // identity pass to capture content rect
    let content = find_rect(&tree, &s, "box").expect("box rect");

    let view = ViewportView {
        pan: (40.0, 20.0),
        zoom: 2.0,
    };
    s.set_viewport_view(vp_id(&tree), view);
    layout(&mut tree, &mut s, R);
    let after = find_rect(&tree, &s, "box").expect("box rect");

    let (ex, ey) = view.project((content.x, content.y), ORIGIN);
    approx(after.x, ex);
    approx(after.y, ey);
    approx(after.w, content.w * 2.0);
    approx(after.h, content.h * 2.0);
}

#[test]
fn identity_view_leaves_rects_untouched() {
    let mut a = vp_tree(120.0, 90.0);
    let mut sa = UiState::new();
    assign_ids(&mut a);
    layout(&mut a, &mut sa, R);
    let ra = find_rect(&a, &sa, "box").expect("box");
    // A separately built identical tree must lay out to the same rect —
    // i.e. the default (reset) view is a true no-op.
    let mut b = vp_tree(120.0, 90.0);
    let mut sb = UiState::new();
    assign_ids(&mut b);
    layout(&mut b, &mut sb, R);
    let rb = find_rect(&b, &sb, "box").expect("box");
    approx(ra.x, rb.x);
    approx(ra.w, rb.w);
}

#[test]
fn hit_test_follows_the_transform() {
    let mut tree = vp_tree(100.0, 80.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (60.0, 30.0),
            zoom: 1.5,
        },
    );
    layout(&mut tree, &mut s, R);
    let box_rect = find_rect(&tree, &s, "box").expect("box");
    // A click at the transformed box center lands on the box, even though
    // its content-space rect is elsewhere — hit-test reads the baked rect.
    let hit = hit_test(&tree, &s, (box_rect.center_x(), box_rect.center_y()));
    assert_eq!(hit.as_deref(), Some("box"));
}

#[test]
fn fit_content_frames_and_centers() {
    let mut tree = vp_tree(200.0, 100.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    s.push_viewport_requests(vec![ViewportRequest::FitContent {
        key: "vp".into(),
        padding: 20.0,
    }]);
    layout(&mut tree, &mut s, R);

    // avail = 360x260; zoom = min(360/200, 260/100) = 1.8.
    let after = find_rect(&tree, &s, "box").expect("box");
    approx(after.w, 200.0 * 1.8);
    approx(after.h, 100.0 * 1.8);
    // Centered in the viewport.
    approx(after.center_x(), R.center_x());
    approx(after.center_y(), R.center_y());
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);
}

#[test]
fn reset_view_request_restores_identity() {
    let mut tree = vp_tree(100.0, 80.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    let content = find_rect(&tree, &s, "box").expect("box");

    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (90.0, 70.0),
            zoom: 3.0,
        },
    );
    s.push_viewport_requests(vec![ViewportRequest::ResetView { key: "vp".into() }]);
    layout(&mut tree, &mut s, R);

    let after = find_rect(&tree, &s, "box").expect("box");
    approx(after.x, content.x);
    approx(after.w, content.w);
    let v = s.viewport_view(&vp_id(&tree));
    approx(v.zoom, 1.0);
    approx(v.pan.0, 0.0);
}

#[test]
fn pan_is_clamped_so_oversized_content_cannot_leave_a_gutter() {
    // Content larger than the viewport on both axes.
    let mut tree = vp_tree(800.0, 600.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    // Pan way off so the content would be dragged entirely out of view.
    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (1000.0, 1000.0),
            zoom: 1.0,
        },
    );
    layout(&mut tree, &mut s, R);
    let after = find_rect(&tree, &s, "box").expect("box");
    // No gutter: content fully covers the viewport on both axes.
    assert!(after.x <= 0.05, "left edge not clamped: {}", after.x);
    assert!(
        after.right() >= R.right() - 0.05,
        "right edge not clamped: {}",
        after.right()
    );
    assert!(after.y <= 0.05, "top edge not clamped: {}", after.y);
    assert!(after.bottom() >= R.bottom() - 0.05);
}

#[test]
fn smaller_content_is_kept_inside_the_viewport() {
    let mut tree = vp_tree(100.0, 80.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (-9000.0, -9000.0),
            zoom: 1.0,
        },
    );
    layout(&mut tree, &mut s, R);
    let after = find_rect(&tree, &s, "box").expect("box");
    // Content smaller than viewport stays fully inside it.
    assert!(after.x >= -0.05, "left: {}", after.x);
    assert!(
        after.right() <= R.right() + 0.05,
        "right: {}",
        after.right()
    );
}

#[test]
fn wheel_zoom_is_cursor_anchored() {
    let mut tree = vp_tree(200.0, 150.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);

    let id = vp_id(&tree);
    let cursor = (300.0, 220.0);
    let before = s.viewport_view(&id).unproject(cursor, ORIGIN);
    // dy < 0 zooms in.
    assert!(s.viewport_wheel_zoom(cursor.0, cursor.1, -1.0));
    let v = s.viewport_view(&id);
    assert!(v.zoom > 1.0, "zoom should increase, was {}", v.zoom);
    let after = v.unproject(cursor, ORIGIN);
    // The content point under the cursor is unchanged.
    approx(before.0, after.0);
    approx(before.1, after.1);
}

#[test]
fn paint_scales_descendant_font_size_and_line_height_with_zoom() {
    use crate::ir::DrawOp;
    use crate::paint::draw_ops::draw_ops;

    // A text leaf with explicit base font size + line height. Zoom OUT
    // (0.5) is the regression direction: font_size and line_height must
    // shrink together, or the text spills past its (shrunk) box border.
    let mut tree = viewport([crate::text("zoom me")
        .font_size(20.0)
        .line_height(28.0)
        .key("label")])
    .key("vp");
    let mut s = UiState::new();
    assign_ids(&mut tree);
    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (0.0, 0.0),
            zoom: 0.5,
        },
    );
    layout(&mut tree, &mut s, R);

    let label_id = find_id(&tree, "label").expect("label id");
    let glyph = draw_ops(&tree, &s).into_iter().find_map(|op| match op {
        DrawOp::GlyphRun {
            id,
            size,
            line_height,
            ..
        } if id == label_id => Some((size, line_height)),
        _ => None,
    });
    let (size, line_height) = glyph.expect("glyph run for label");
    // Both scale by the 0.5 zoom: 20→10 and 28→14.
    approx(size, 10.0);
    approx(line_height, 14.0);
}

#[test]
fn wheel_zoom_clamps_to_max() {
    let mut tree = vp_tree(100.0, 100.0).max_zoom(2.0);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    let id = vp_id(&tree);
    // Many zoom-in notches; zoom must saturate at max_zoom.
    for _ in 0..50 {
        s.viewport_wheel_zoom(200.0, 150.0, -1.0);
    }
    approx(s.viewport_view(&id).zoom, 2.0);
}
