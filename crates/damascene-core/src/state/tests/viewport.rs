//! Layout + state integration tests for `viewport()` pan/zoom: the
//! transform baked into descendant rects, hit-testing through it,
//! programmatic requests (fit / reset), pan clamping, and cursor-anchored
//! wheel zoom.

use super::support::*;
use crate::tree::viewport;
use crate::viewport::{FitPolicy, PanBounds, ViewportRequest, ViewportView};

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
    let content = find_rect(&tree, "box").expect("box rect");

    let view = ViewportView {
        pan: (40.0, 20.0),
        zoom: 2.0,
    };
    s.set_viewport_view(vp_id(&tree), view);
    layout(&mut tree, &mut s, R);
    let after = find_rect(&tree, "box").expect("box rect");

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
    let ra = find_rect(&a, "box").expect("box");
    // A separately built identical tree must lay out to the same rect —
    // i.e. the default (reset) view is a true no-op.
    let mut b = vp_tree(120.0, 90.0);
    let mut sb = UiState::new();
    assign_ids(&mut b);
    layout(&mut b, &mut sb, R);
    let rb = find_rect(&b, "box").expect("box");
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
    let box_rect = find_rect(&tree, "box").expect("box");
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
    let after = find_rect(&tree, "box").expect("box");
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
    let content = find_rect(&tree, "box").expect("box");

    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (90.0, 70.0),
            zoom: 3.0,
        },
    );
    s.push_viewport_requests(vec![ViewportRequest::ResetView { key: "vp".into() }]);
    layout(&mut tree, &mut s, R);

    let after = find_rect(&tree, "box").expect("box");
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
    let after = find_rect(&tree, "box").expect("box");
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
fn center_bounds_let_any_node_reach_mid_frame() {
    // Content larger than the viewport, panned far past its edge. Under
    // PanBounds::Center the clamp pulls the content's near edge only to
    // the viewport center (not its edge), so the left-most node is
    // parkable mid-frame — the DAG-canvas case.
    let mut tree = vp_tree(800.0, 600.0).pan_bounds(PanBounds::Center);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (5000.0, 5000.0),
            zoom: 1.0,
        },
    );
    layout(&mut tree, &mut s, R);
    let after = find_rect(&tree, "box").expect("box");
    // The box's top-left (its content origin) lands on the viewport center.
    approx(after.x, R.center_x());
    approx(after.y, R.center_y());
}

#[test]
fn free_bounds_leave_pan_unclamped() {
    // PanBounds::Free applies no clamp at all: the content keeps a pan
    // that drags it fully out of view.
    let mut tree = vp_tree(800.0, 600.0).pan_bounds(PanBounds::Free);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    s.set_viewport_view(
        vp_id(&tree),
        ViewportView {
            pan: (5000.0, 5000.0),
            zoom: 1.0,
        },
    );
    layout(&mut tree, &mut s, R);
    let after = find_rect(&tree, "box").expect("box");
    approx(after.x, 5000.0);
    approx(after.y, 5000.0);
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
    let after = find_rect(&tree, "box").expect("box");
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
    assert!(s.viewport_wheel_zoom(&tree, cursor.0, cursor.1, -1.0));
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
        } if *id == label_id => Some((size, line_height)),
        _ => None,
    });
    let (size, line_height) = glyph.expect("glyph run for label");
    // Both scale by the 0.5 zoom: 20→10 and 28→14.
    approx(size, 10.0);
    approx(line_height, 14.0);
}

#[test]
fn paint_scales_attributed_paragraph_with_zoom() {
    use crate::ir::DrawOp;
    use crate::paint::draw_ops::draw_ops;
    use crate::tree::text_runs;

    // A wrapping multi-run paragraph (Kind::Inlines → AttributedText) in
    // the viewport. Its aggregate size/line_height must scale with zoom
    // too, or prose inside a canvas overflows on zoom-out.
    let mut tree = viewport([text_runs([
        crate::text("hello ").font_size(20.0).line_height(28.0),
        crate::text("world").font_size(20.0).line_height(28.0),
    ])
    .key("para")])
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

    let para_id = find_id(&tree, "para").expect("para id");
    let metrics = draw_ops(&tree, &s).into_iter().find_map(|op| match op {
        DrawOp::AttributedText {
            id,
            size,
            line_height,
            ..
        } if *id == para_id => Some((size, line_height)),
        _ => None,
    });
    let (size, line_height) = metrics.expect("attributed text for para");
    approx(size, 10.0);
    approx(line_height, 14.0);
}

// ---- #110: keyed-but-non-interactive nodes don't hover-brighten ----

/// Hover `key`, settle the envelopes, and read back its Hover envelope.
fn hover_envelope(tree: &mut El, state: &mut UiState, key: &str) -> Option<f32> {
    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(tree, key));
    state.apply_to_state();
    state.tick_visual_animations(tree, Instant::now(), &Palette::default());
    envelope_for(tree, state, key, EnvelopeKind::Hover)
}

#[test]
fn keyed_node_tracks_hover_envelope_by_default() {
    // Baseline: an ordinary keyed node lightens on hover (envelope → 1).
    let mut tree = column([button("x").key("normal")]);
    let mut s = UiState::new();
    layout(&mut tree, &mut s, R);
    assert_eq!(hover_envelope(&mut tree, &mut s, "normal"), Some(1.0));
}

#[test]
fn no_hover_suppresses_the_hover_envelope() {
    // The opt-out: a keyed node marked .no_hover() never tracks the
    // envelope, so its fill stays static under the cursor.
    let mut tree = column([button("x").key("quiet").no_hover()]);
    let mut s = UiState::new();
    layout(&mut tree, &mut s, R);
    assert_eq!(hover_envelope(&mut tree, &mut s, "quiet"), Some(0.0));
}

#[test]
fn viewport_does_not_track_hover_envelope() {
    // #110: a keyed viewport's background fill must not brighten on hover.
    let mut tree = viewport([button("x").key("box")]).key("vp");
    let mut s = UiState::new();
    layout(&mut tree, &mut s, R);
    assert_eq!(hover_envelope(&mut tree, &mut s, "vp"), Some(0.0));
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
        s.viewport_wheel_zoom(&tree, 200.0, 150.0, -1.0);
    }
    approx(s.viewport_view(&id).zoom, 2.0);
}

// A `block_pointer` modal floated over the viewport must take the wheel
// as scroll, not zoom: `viewport_wheel_zoom` declines (returns false) for
// points the modal panel covers, so the wheel falls through to scroll
// routing — but still zooms for points that land on the bare canvas.
// Regression for #111.
#[test]
fn wheel_over_modal_overlay_does_not_zoom_the_viewport_underneath() {
    let mut tree = crate::overlays(
        vp_tree(800.0, 600.0),
        [Some(crate::modal(
            "detail",
            "Detail",
            [crate::scroll([button("body")
                .key("modal_body")
                .width(Size::Fixed(300.0))
                .height(Size::Fixed(200.0))])],
        ))],
    );
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);

    // A point inside the modal body (which the block_pointer panel covers)
    // must NOT be taken as zoom.
    let body = find_rect(&tree, "modal_body").expect("modal body rect");
    let over_panel = (body.x + body.w * 0.5, body.y + body.h * 0.5);
    assert!(
        !s.viewport_wheel_zoom(&tree, over_panel.0, over_panel.1, -1.0),
        "wheel over the modal panel must yield to scroll routing, not zoom the canvas"
    );

    // A point on the bare canvas (top-left corner, clear of the centered
    // panel) still zooms.
    assert!(
        !s.viewport_view(&vp_id(&tree)).pan.0.is_nan(),
        "viewport view exists"
    );
    let corner = (4.0, 4.0);
    assert!(
        s.viewport_wheel_zoom(&tree, corner.0, corner.1, -1.0),
        "wheel on the bare canvas still zooms"
    );
}

// The inverse: a viewport living *inside* a modal (the overlay is its
// ancestor, not in front of it) must still zoom — occlusion only applies
// to overlays painted over the target, not around it.
#[test]
fn wheel_over_viewport_inside_a_modal_still_zooms() {
    let mut tree = crate::overlays(
        button("bg").key("bg"),
        [Some(crate::modal(
            "canvas",
            "Canvas",
            [vp_tree(800.0, 600.0)],
        ))],
    );
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);

    let vp_rect = find_rect(&tree, "vp").expect("viewport rect");
    let center = (vp_rect.x + vp_rect.w * 0.5, vp_rect.y + vp_rect.h * 0.5);
    assert!(
        s.viewport_wheel_zoom(&tree, center.0, center.1, -1.0),
        "a viewport inside the modal is not occluded by the modal — it zooms"
    );
}

/// A Hug column `count` tall (`count` × 50px boxes, no gap/padding), keyed
/// "c" — its intrinsic height is `count * 50`.
fn hug_column(count: usize) -> El {
    column(
        (0..count)
            .map(|_| {
                El::new(Kind::Group)
                    .width(Size::Fixed(100.0))
                    .height(Size::Fixed(50.0))
            })
            .collect::<Vec<_>>(),
    )
    .gap(0.0)
    .padding(0.0)
    .key("c")
}

// Issue #112: a `viewport()` lays its `Hug` content out at full intrinsic
// even past the frame — the pan/zoom transform reveals the overflow.
// Before the fix the content was clamped to the viewport rect.
#[test]
fn viewport_hug_content_is_not_clamped_to_the_frame() {
    // Frame 400×300; content is a Hug column 400px tall (8 × 50), taller
    // than the frame.
    let mut tree = viewport([hug_column(8)]).key("vp");
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    let c = find_rect(&tree, "c").expect("content rect");
    approx(c.h, 400.0); // full intrinsic, NOT clamped to the 300 frame
    assert!(
        c.h > R.h,
        "content extends past the frame so it can be panned: {} !> {}",
        c.h,
        R.h
    );
}

// The clamp still applies to non-viewport overlays — a modal shouldn't
// exceed the screen. The same Hug content inside a bare `overlay()` is
// capped at the frame. Locks the boundary the #112 fix carves out.
#[test]
fn overlay_hug_content_still_clamps_to_the_frame() {
    let mut tree = crate::overlay([hug_column(8)]);
    let mut s = UiState::new();
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    let c = find_rect(&tree, "c").expect("content rect");
    approx(c.h, R.h); // clamped to the 300 frame
}

// ---- FitPolicy: declarative fit maintained by the widget (#115) ----

/// A policy viewport with one fixed-size keyed box as its content.
fn vp_tree_fit(w: f32, h: f32, policy: FitPolicy) -> El {
    viewport([button("x")
        .key("box")
        .width(Size::Fixed(w))
        .height(Size::Fixed(h))])
    .key("vp")
    .fit_policy(policy)
}

/// `Contain` frames the content on the very first layout (no request
/// pushed), and re-frames when the container resizes.
#[test]
fn contain_fits_on_mount_and_refits_on_resize() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    // avail = 360x260; zoom = min(360/200, 260/100) = 1.8, centered.
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);
    let r = find_rect(&tree, "box").expect("box");
    approx(r.center_x(), R.center_x());
    approx(r.center_y(), R.center_y());

    // Next frame the window is larger: the fit tracks it.
    let big = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, big);
    // avail = 760x560; zoom = min(760/200, 560/100) = 3.8.
    approx(s.viewport_view(&vp_id(&tree)).zoom, 3.8);
    let r = find_rect(&tree, "box").expect("box");
    approx(r.center_x(), big.center_x());
}

/// `Contain` also tracks content-extent changes — a diagram that grows
/// stays framed until the user takes over.
#[test]
fn contain_tracks_content_growth() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);

    // Content doubles in width; same window.
    let mut tree = vp_tree_fit(400.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    // zoom = min(360/400, 260/100) = 0.9.
    approx(s.viewport_view(&vp_id(&tree)).zoom, 0.9);
}

/// The first effective user zoom releases `Contain`: the view keeps the
/// user's framing through later layouts and resizes, and the at-home
/// readback flips.
#[test]
fn contain_releases_on_user_zoom() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(true));

    // Wheel-zoom in over the center: 1.8 * 1.1.
    assert!(s.viewport_wheel_zoom(&tree, 200.0, 150.0, -1.0));
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(false));

    // A later layout at a *larger* window must not re-fit.
    let big = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, big);
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8 * 1.1);
}

/// A real pan drag (movement, not just a press) also releases `Contain`.
#[test]
fn contain_releases_on_pan_drag() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    let id = vp_id(&tree);

    s.begin_viewport_pan(id.clone(), 200.0, 150.0);
    // A press with no movement is not a takeover.
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(true));
    assert!(s.drag_viewport_to(215.0, 150.0));
    s.end_viewport_pan();
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(false));

    // The dragged pan survives the next layout (clamped, not re-fit).
    let fitted = s.viewport_view(&id);
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    approx(s.viewport_view(&id).zoom, fitted.zoom);
    assert!((s.viewport_view(&id).pan.0 - fitted.pan.0).abs() < 0.05);
}

/// `ResetView` re-arms a released `Contain`: the policy fit resumes and
/// the viewport reports at-home again.
#[test]
fn reset_request_rearms_contain() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    assert!(s.viewport_wheel_zoom(&tree, 200.0, 150.0, -1.0));
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(false));

    s.push_viewport_requests(vec![ViewportRequest::ResetView { key: "vp".into() }]);
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(true));
}

/// `CenterOn` is a deliberate steer away from the home framing: it wins
/// over `Contain` and flips the readback, exactly like a user gesture.
#[test]
fn center_on_takes_over_contain() {
    let mut s = UiState::new();
    let mut tree =
        vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 }).pan_bounds(PanBounds::Free);
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);

    s.push_viewport_requests(vec![ViewportRequest::CenterOn {
        key: "vp".into(),
        point: (0.0, 0.0),
    }]);
    let mut tree =
        vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 }).pan_bounds(PanBounds::Free);
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(false));
    // Content top-left corner parked at the viewport center (zoom kept).
    let view = s.viewport_view(&vp_id(&tree));
    let (sx, sy) = view.project((0.0, 0.0), ORIGIN);
    approx(sx, R.center_x());
    approx(sy, R.center_y());
}

/// `Lock` fits every pass and is not a gesture target: the wheel is not
/// consumed (it falls through to scroll routing) and no pan can start.
#[test]
fn lock_always_fits_and_suppresses_gestures() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Lock { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);

    // Not a wheel target...
    assert!(!s.viewport_wheel_zoom(&tree, 200.0, 150.0, -1.0));
    // ...and not a pan target either.
    assert!(s.viewport_at(200.0, 150.0).is_none());
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);

    // Locked means at-home by construction — even a seeded view (which
    // normally marks a takeover) reads as home once the lock re-fits.
    s.set_viewport_view(vp_id(&tree), ViewportView::default());
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Lock { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(true));
    approx(s.viewport_view(&vp_id(&tree)).zoom, 1.8);
}

/// An app-seeded view is a deliberate framing: `Contain` must not stomp
/// it on the next layout.
#[test]
fn seeded_view_is_not_stomped_by_contain() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    let seeded = ViewportView {
        pan: (10.0, 5.0),
        zoom: 2.5,
    };
    s.set_viewport_view(vp_id(&tree), seeded);
    layout(&mut tree, &mut s, R);
    approx(s.viewport_view(&vp_id(&tree)).zoom, 2.5);
    assert_eq!(s.viewport_at_home_by_key("vp"), Some(false));
}

/// The by-key content-bounds readback answers in content space,
/// independent of the current transform.
#[test]
fn content_bounds_by_key_reads_content_space() {
    let mut s = UiState::new();
    let mut tree = vp_tree_fit(200.0, 100.0, FitPolicy::Contain { padding: 20.0 });
    assign_ids(&mut tree);
    layout(&mut tree, &mut s, R);
    let b = s.viewport_content_bounds_by_key("vp").expect("bounds");
    approx(b.w, 200.0);
    approx(b.h, 100.0);
    assert!(s.viewport_content_bounds_by_key("nope").is_none());
}
