use super::support::*;

#[test]
fn settled_mode_snaps_hover_envelope_to_one() {
    // Headless contract: Settled mode must produce the post-hover
    // envelope on a single prepare. A windowed runner (Live mode)
    // would ease over many frames; the fixture path can't wait.
    let (mut tree, mut state) = lay_out_counter();
    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(&tree, "inc"));
    state.apply_to_state();

    let needs_redraw = state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());

    assert!(!needs_redraw, "Settled mode should never report in flight");
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::Hover),
        Some(1.0)
    );
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::Press),
        Some(0.0)
    );
    // The build fill stays untouched — the lightening happens in
    // apply_state at draw time, mixing by hover_amount.
}

#[test]
fn live_mode_eases_hover_envelope_over_multiple_ticks() {
    // After a single 8 ms tick the hover envelope should be
    // strictly between 0 and 1 — neither snapped to either end.
    let (mut tree, mut state) = lay_out_counter();
    let t0 = Instant::now();
    state.tick_visual_animations(&mut tree, t0, &Palette::default());

    state.hovered = Some(target(&tree, "inc"));
    state.apply_to_state();
    let needs_redraw = state.tick_visual_animations(
        &mut tree,
        t0 + std::time::Duration::from_millis(8),
        &Palette::default(),
    );
    let mid = envelope_for(&tree, &state, "inc", EnvelopeKind::Hover).expect("hover envelope");

    assert!(
        needs_redraw,
        "spring should still be in flight after one 8 ms tick"
    );
    assert!(
        mid > 0.0 && mid < 1.0,
        "expected envelope mid-flight, got {mid}",
    );
}

#[test]
fn build_value_change_survives_hover_envelope() {
    // The point of envelopes: when the author swaps a button's fill
    // mid-hover, n.fill must reflect the new build value
    // immediately. The envelope keeps easing independently. This is
    // what avoids the AppFill / StateFill fight of an earlier draft.
    let mut tree_a =
        column([row([button("X").key("x").fill(Color::srgb_u8(255, 0, 0))])]).padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(&tree_a, "x"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_a, Instant::now(), &Palette::default());
    assert_eq!(
        envelope_for(&tree_a, &state, "x", EnvelopeKind::Hover),
        Some(1.0)
    );

    // Rebuild: same button, fill swapped to blue.
    let mut tree_b =
        column([row([button("X").key("x").fill(Color::srgb_u8(0, 0, 255))])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_b, Instant::now(), &Palette::default());

    let observed = find_fill(&tree_b, "x").expect("x fill");
    let [or, og, ob, _] = observed.to_srgb_u8a();
    assert_eq!(
        (or, og, ob),
        (0, 0, 255),
        "build fill should pass through unchanged — envelope handles state delta separately",
    );
    assert_eq!(
        envelope_for(&tree_b, &state, "x", EnvelopeKind::Hover),
        Some(1.0)
    );
}

#[test]
fn focus_ring_alpha_eases_in_and_out() {
    let (mut tree, mut state) = lay_out_counter();
    state.set_animation_mode(AnimationMode::Settled);

    // No focus → alpha settled at 0.
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(0.0)
    );

    // Focus on inc + focus_visible (Tab semantics) → alpha settles at 1.0.
    let (mut tree, _) = lay_out_counter();
    // Re-layout against the existing state so the rect map is fresh.
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.focused = Some(target(&tree, "inc"));
    state.set_focus_visible(true);
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(1.0)
    );

    // Lose focus → alpha settles back to 0.
    let (mut tree, _) = lay_out_counter();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.focused = None;
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(0.0)
    );
}

#[test]
fn focus_ring_stays_dim_when_focus_is_not_visible() {
    // Pointer-driven focus (`focus_visible == false`) keeps the ring
    // off — clicking a button shouldn't paint the keyboard
    // affordance. Mirrors the web `:focus-visible` heuristic.
    let (mut tree, mut state) = lay_out_counter();
    state.set_animation_mode(AnimationMode::Settled);
    state.focused = Some(target(&tree, "inc"));
    // Default focus_visible is false (the runtime sets it explicitly
    // on Tab / pointer-down). Don't raise it here.
    assert!(!state.focus_visible);
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(0.0),
        "ring must stay off until focus_visible is raised",
    );
}

#[test]
fn focus_ring_lights_up_on_always_show_focus_ring_widgets_even_without_focus_visible() {
    // Text inputs / text areas opt in via `.always_show_focus_ring()`
    // because the ring on click is a meaningful "now editable"
    // affordance. Verify the per-element flag overrides the global
    // `focus_visible == false`.
    let selection = crate::selection::Selection::default();
    let mut tree = column([row([crate::widgets::text_input::text_input(
        "field", "", &selection,
    )])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.set_animation_mode(AnimationMode::Settled);
    state.focused = Some(target(&tree, "field"));
    assert!(!state.focus_visible);
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());
    assert_eq!(
        envelope_for(&tree, &state, "field", EnvelopeKind::FocusRing),
        Some(1.0),
        "always_show_focus_ring widgets ring on click too",
    );
}

#[test]
fn focus_ring_stays_on_when_focused_node_is_also_hovered() {
    // Regression: hovering a focused element used to overwrite its
    // resolved `state` from Focus to Hover (Hover wins in
    // `apply_to_state`), and `FocusRingAlpha`'s target gated on
    // `state == Focus`, so the ring fell off the moment the cursor
    // entered. The `alpha_follows_focused_ancestor` chain dragged
    // the text-input caret to invisible with it. Focus envelope must
    // be independent of hover/press.
    let selection = crate::selection::Selection::default();
    let mut tree = column([row([crate::widgets::text_input::text_input(
        "field", "", &selection,
    )])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.set_animation_mode(AnimationMode::Settled);

    let field = target(&tree, "field");
    state.focused = Some(field.clone());
    state.hovered = Some(field);
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());

    assert_eq!(
        envelope_for(&tree, &state, "field", EnvelopeKind::FocusRing),
        Some(1.0),
        "focus ring must stay on while the focused node is also the hover target",
    );
}

#[test]
fn app_fill_settles_to_new_value_in_settled_mode() {
    // .animate(SPRING_STANDARD) on a node whose fill changes
    // between rebuilds. Settled mode should produce the new fill
    // on the very first tick after the change.
    use crate::anim::Timing;
    let mut tree_a = column([
        crate::text("0"),
        row([button("X")
            .key("x")
            .fill(Color::srgb_u8(255, 0, 0))
            .animate(Timing::SPRING_STANDARD)]),
    ])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

    state.set_animation_mode(AnimationMode::Settled);
    state.tick_visual_animations(&mut tree_a, Instant::now(), &Palette::default());
    assert_eq!(
        find_fill(&tree_a, "x").map(|c| {
            let [r, g, b, _] = c.to_srgb_u8a();
            (r, g, b)
        }),
        Some((255, 0, 0))
    );

    // Rebuild with a different fill; tracker eases through.
    let mut tree_b = column([
        crate::text("0"),
        row([button("X")
            .key("x")
            .fill(Color::srgb_u8(0, 0, 255))
            .animate(Timing::SPRING_STANDARD)]),
    ])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now(), &Palette::default());

    assert_eq!(
        find_fill(&tree_b, "x").map(|c| {
            let [r, g, b, _] = c.to_srgb_u8a();
            (r, g, b)
        }),
        Some((0, 0, 255)),
        "Settled mode should snap to the new build value",
    );
}

#[test]
fn app_fill_eases_in_live_mode() {
    // Same setup as above but in Live mode: after a small dt the
    // colour should be partway between red and blue, not at either.
    use crate::anim::Timing;
    let mut tree_a = column([row([button("X")
        .key("x")
        .fill(Color::srgb_u8(255, 0, 0))
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

    let t0 = Instant::now();
    state.tick_visual_animations(&mut tree_a, t0, &Palette::default());

    let mut tree_b = column([row([button("X")
        .key("x")
        .fill(Color::srgb_u8(0, 0, 255))
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    let needs_redraw = state.tick_visual_animations(
        &mut tree_b,
        t0 + std::time::Duration::from_millis(8),
        &Palette::default(),
    );
    let mid = find_fill(&tree_b, "x").expect("mid fill");

    assert!(
        needs_redraw,
        "spring should still be in flight after one tick"
    );
    assert!(
        mid.r < 1.0 && mid.b < 1.0,
        "expected mid-flight, got {mid:?}",
    );
    assert!(
        mid.r > 0.0 || mid.b > 0.0,
        "should have moved off the start",
    );
}

#[test]
fn token_tagged_fill_eases_through_draw_ops_without_snapping() {
    // Regression: when a checkbox / switch / button toggles its
    // fill from one token to another, the animator interpolates rgba
    // between them. Before the fix, `Animation::from_channels`
    // preserved the source token on the in-flight value, and
    // `apply_state.palette.resolve(c)` snapped the rgb back to that
    // token's palette value — so the transition rendered as an
    // instant flip. Verify the eased fill survives draw_ops.
    use crate::anim::Timing;
    use crate::draw_ops::draw_ops_with_theme;
    use crate::ir::DrawOp;
    use crate::shader::UniformValue;
    use crate::theme::Theme;
    use crate::tokens;

    let mut tree_a = column([row([button("X")
        .key("x")
        .fill(tokens::PRIMARY)
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    let t0 = Instant::now();
    state.tick_visual_animations(&mut tree_a, t0, &Palette::default());

    let mut tree_b = column([row([button("X")
        .key("x")
        .fill(tokens::CARD)
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(
        &mut tree_b,
        t0 + std::time::Duration::from_millis(8),
        &Palette::default(),
    );

    // Drive the rendered output through draw_ops; the apply_state
    // path is what previously snapped the in-flight rgb.
    let theme = Theme::default();
    let ops = draw_ops_with_theme(&tree_b, &state, &theme);
    let quad_fill = ops
        .iter()
        .find_map(|op| match op {
            DrawOp::Quad { id, uniforms, .. } if id.contains("x") => {
                uniforms.get("fill").and_then(|v| match v {
                    UniformValue::Color(c) => Some(*c),
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("button quad fill");

    // Mid-flight: rgb should be strictly between the source PRIMARY
    // and target CARD, not snapped to either.
    let p = tokens::PRIMARY;
    let c = tokens::CARD;
    assert_ne!(
        (quad_fill.r, quad_fill.g, quad_fill.b),
        (p.r, p.g, p.b),
        "rendered fill must not snap back to the source token's rgb",
    );
    assert_ne!(
        (quad_fill.r, quad_fill.g, quad_fill.b),
        (c.r, c.g, c.b),
        "rendered fill must not snap forward to the target token's rgb",
    );
}

#[test]
fn spring_color_converges_to_target_token_near_equilibrium() {
    // Regression: `Animation::current` stored u8 rgba via
    // `AnimValue::Color`. The integrator read channels as f32 each
    // tick, integrated by `vel * h ≈ 0.1–0.4` per substep near the
    // target, then wrote back through `from_channels` which rounds to
    // u8 — destroying sub-integer progress every frame. Near
    // equilibrium the per-frame f32 motion was below 0.5, so the
    // rounded value froze a few rgb units past the target with the
    // token cleared, and `palette.resolve` then painted that frozen
    // intermediate rgb forever. Fix stores a lossless f32 mirror
    // (`current_precise`) for the integrator.
    //
    // This test simulates a checkbox click (false → true) and ticks
    // long enough that, prior to the fix, the spring would still be
    // stuck a few units off PRIMARY with `token: None`. After the
    // fix the spring settles and `current` snaps back to
    // `target == tokens::PRIMARY`, with the token preserved.
    use crate::tokens;
    use crate::widgets::checkbox::checkbox;
    use std::time::Duration;

    fn tree(v: bool) -> El {
        column([checkbox("agree", v)]).padding(8.0)
    }
    fn find_cb(n: &El) -> Option<&El> {
        if matches!(n.kind, Kind::Custom("checkbox")) {
            return Some(n);
        }
        n.children.iter().find_map(find_cb)
    }

    let palette = Palette::default();
    let mut state = UiState::new();
    let vp = Rect::new(0.0, 0.0, 200.0, 60.0);
    let mut t = tree(false);
    layout(&mut t, &mut state, vp);
    let t0 = Instant::now();
    state.tick_visual_animations(&mut t, t0, &palette);

    // Tick 1.5 s of 16 ms frames as the build flips to checked. That's
    // ~6× the SPRING_STANDARD settle time; if the integrator can settle
    // at all it'll have done so well before this point.
    let mut settled_to_token = false;
    for i in 1..=96 {
        let mut t = tree(true);
        layout(&mut t, &mut state, vp);
        let now_i = t0 + Duration::from_millis(16 * i);
        state.tick_visual_animations(&mut t, now_i, &palette);
        let cb = find_cb(&t).unwrap();
        let f = cb.fill.expect("fill present");
        if f.token == Some("primary") {
            settled_to_token = true;
            assert_eq!(
                (f.r, f.g, f.b),
                (tokens::PRIMARY.r, tokens::PRIMARY.g, tokens::PRIMARY.b)
            );
            break;
        }
    }
    assert!(
        settled_to_token,
        "spring fill must converge to `target` and restore the token; \
         pre-fix this froze at ~(253,253,253) with token=None forever",
    );
}

#[test]
fn anim_target_uses_active_palette_rgb_not_compile_time_constant() {
    // Regression: `compute_target` returned `n.fill` directly. Tokens'
    // compile-time rgb constants carry the default-dark palette
    // values, so on any non-default palette the animation interpolated
    // through the wrong color space. On radix slate blue dark, a
    // checkbox transition would walk the default-dark (9→250) gray
    // ramp mid-flight and only show slate blue's (0,144,255) at the
    // moment the spring settled. Fix resolves the target through the
    // active palette in `compute_target`.
    use crate::theme::Theme;
    use crate::widgets::checkbox::checkbox;
    use std::time::Duration;

    fn tree(v: bool) -> El {
        column([checkbox("agree", v)]).padding(8.0)
    }
    fn find_cb(n: &El) -> Option<&El> {
        if matches!(n.kind, Kind::Custom("checkbox")) {
            return Some(n);
        }
        n.children.iter().find_map(find_cb)
    }

    let theme = Theme::radix_slate_blue_dark();
    let palette = theme.palette();
    let mut state = UiState::new();
    let vp = Rect::new(0.0, 0.0, 200.0, 60.0);
    let mut t = tree(false);
    layout(&mut t, &mut state, vp);
    let t0 = Instant::now();
    state.tick_visual_animations(&mut t, t0, palette);

    // Mid-flight after click. The rendered fill must lie on slate
    // blue's CARD (24,25,27) → PRIMARY (0,144,255) line: R dropping
    // toward 0, G rising toward 144, B rising toward 255 — strictly
    // R < G < B. The default-dark CARD (9,9,11) → PRIMARY (250,250,250)
    // ramp would have R ≈ G ≈ B at any sample point, so the strict
    // ordering rules it out.
    let mut t = tree(true);
    layout(&mut t, &mut state, vp);
    state.tick_visual_animations(&mut t, t0 + Duration::from_millis(64), palette);
    let cb = find_cb(&t).unwrap();
    let f = cb.fill.expect("fill present");
    assert!(
        f.r < f.g && f.g < f.b,
        "mid-flight rgb must lie on slate blue's ramp (R<G<B), not on the \
         default-dark gray ramp (R≈G≈B). got fill={f:?}",
    );
    assert!(
        f.b - f.r > 30.0 / 255.0,
        "mid-flight rgb must have meaningfully diverged on the slate blue \
         line, not just one rounding step apart. got fill={f:?}",
    );
}

#[test]
fn app_translate_eases_on_rebuild() {
    use crate::anim::Timing;
    let mut tree_a = column([row([button("slide")
        .key("s")
        .translate(0.0, 0.0)
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.set_animation_mode(AnimationMode::Settled);
    state.tick_visual_animations(&mut tree_a, Instant::now(), &Palette::default());

    // Rebuild with a different translate.
    let mut tree_b = column([row([button("slide")
        .key("s")
        .translate(100.0, 50.0)
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now(), &Palette::default());

    let n = find_node(&tree_b, "s").expect("s node");
    assert!((n.translate.0 - 100.0).abs() < 0.5);
    assert!((n.translate.1 - 50.0).abs() < 0.5);
}

#[test]
fn state_envelope_composes_on_app_eased_fill() {
    // A keyed interactive node with .animate() AND being hovered.
    // After Settled tick: n.fill = (eased) build value, hover
    // envelope = 1. draw_ops in apply_state then mixes the build
    // colour toward its lightened version by the envelope amount.
    // Since the envelope is at 1, the emitted Quad's fill should
    // equal lighten(build_fill, HOVER_LIGHTEN).
    use crate::anim::Timing;
    let mut tree = column([row([button("X")
        .key("x")
        .fill(Color::srgb_u8(100, 100, 100))
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(&tree, "x"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());

    // Build fill survives untouched (envelope handles the delta).
    let n_fill = find_fill(&tree, "x").expect("x fill");
    let [nr, ng, nb, _] = n_fill.to_srgb_u8a();
    assert_eq!((nr, ng, nb), (100, 100, 100));
    assert_eq!(
        envelope_for(&tree, &state, "x", EnvelopeKind::Hover),
        Some(1.0)
    );
}

#[test]
fn app_animation_skipped_when_animate_not_set() {
    // Without .animate(), app props are not tracked — the node's
    // fill snaps to whatever the build produces, no easing.
    //
    // Note: many stock widgets (e.g. `button`) opt into animation
    // by default, so this negative test uses a plain `Kind::Custom`
    // node that genuinely has no animate field set.
    fn swatch(fill: Color) -> El {
        El::new(Kind::Custom("swatch"))
            .key("x")
            .fill(fill)
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))
    }

    let mut tree_a = column([row([swatch(Color::srgb_u8(255, 0, 0))])]).padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_a, Instant::now(), &Palette::default());

    let mut tree_b = column([row([swatch(Color::srgb_u8(0, 0, 255))])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now(), &Palette::default());

    let observed = find_fill(&tree_b, "x").expect("x fill");
    let [or, og, ob, _] = observed.to_srgb_u8a();
    assert_eq!(
        (or, og, ob),
        (0, 0, 255),
        "no .animate() — value should snap",
    );
}

#[test]
fn animation_entries_gc_when_node_leaves_tree() {
    // Build a tree with two buttons; hover one to seed an entry.
    // Then build a different tree with only one button. The orphan's
    // animation entries should be trimmed.
    let (mut tree_a, mut state) = lay_out_counter();
    state.hovered = Some(target(&tree_a, "inc"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_a, Instant::now(), &Palette::default());
    let inc_id_a = find_id(&tree_a, "inc").expect("inc id");
    assert!(
        state
            .animation
            .animations
            .keys()
            .any(|(id, _)| **id == *inc_id_a),
        "expected at least one entry for inc"
    );

    // Rebuild with only the dec button. inc entries should be gone.
    let mut tree_b = column([crate::text("0"), row([button("-").key("dec")])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.hovered = None;
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_b, Instant::now(), &Palette::default());
    assert!(
        !state
            .animation
            .animations
            .keys()
            .any(|(id, _)| **id == *inc_id_a),
        "stale entries for inc were not GC'd"
    );
}

#[test]
fn scrim_kind_opts_out_of_hover_envelope() {
    // Regression for #33: a `Kind::Scrim` is keyed so click-outside
    // routes to `{key}:dismiss`, but it must NOT receive hover/press
    // envelopes. Otherwise `apply_state` would lighten the dimmed
    // modal scrim's opaque OVERLAY_SCRIM fill under the cursor,
    // making the dim brighten when the mouse passes over it.
    use crate::widgets::overlay::scrim;
    let mut tree = stack([scrim("modal:dismiss")]);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
    state.set_animation_mode(AnimationMode::Settled);
    // Set the scrim as the hovered target and resolve interaction
    // state — without the Kind::Scrim opt-out, the tick would write
    // hover_amount = 1.0 into the envelopes map for the scrim's id.
    state.hovered = Some(target(&tree, "modal:dismiss"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now(), &Palette::default());

    assert_eq!(
        envelope_for(&tree, &state, "modal:dismiss", EnvelopeKind::Hover),
        Some(0.0),
        "scrim must not be tracked for hover envelopes; routing key only",
    );
    assert_eq!(
        envelope_for(&tree, &state, "modal:dismiss", EnvelopeKind::Press),
        Some(0.0),
        "scrim must not be tracked for press envelopes; routing key only",
    );
}
