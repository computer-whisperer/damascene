//! Visual-state animation tick — drives state envelopes (hover / press /
//! focus ring) and app-driven prop tracks (`fill`, `text_color`, etc.).
//!
//! The animation map (`(computed_id, AnimProp) → Animation`) lives in
//! [`crate::state::UiState`]. So does the envelope side map. This module
//! owns the per-node walker that retargets, steps, and writes back —
//! state envelopes go to `UiState::envelopes` (read by `draw_ops`), app
//! props mutate the `El`'s author fields directly so the next build
//! reads the eased value.

use std::collections::HashSet;

use rustc_hash::FxHashMap;
// web_time::Instant works on wasm32 (std::time::Instant::now() panics there).
use web_time::Instant;

use crate::anim::{AnimProp, AnimValue, Animation, Timing};
use crate::palette::Palette;
use crate::state::query::target_in_subtree;
use crate::state::{AnimationMode, EnvelopeKind};
use crate::tree::{El, InteractionState, Kind};

/// Snapshot of the active hover / focus / press leaf-targets for a
/// frame. Threaded through the tick so each node can ask "is the hot
/// target equal to me, or a descendant of me?" without re-walking the
/// trackers per node.
#[derive(Copy, Clone, Default)]
pub(crate) struct HotTargets<'a> {
    pub hovered: Option<&'a str>,
    pub focused: Option<&'a str>,
    pub pressed: Option<&'a str>,
}

/// App-driven props, processed *first* on nodes with `n.animate` set.
/// They write eased build-time values back to `n.fill` etc., so the
/// state pass that follows reads the already-eased value when computing
/// hover / press deltas. State visuals therefore compose on top of
/// app-driven motion without either tracker fighting the other.
const APP_PROPS: &[AnimProp] = &[
    AnimProp::AppFill,
    AnimProp::AppStroke,
    AnimProp::AppTextColor,
    AnimProp::AppOpacity,
    AnimProp::AppScale,
    AnimProp::AppTranslateX,
    AnimProp::AppTranslateY,
];

/// Per-node state envelopes, processed *after* app props. Always tracked
/// on keyed interactive nodes — no author opt-in. Each is a 0..1 amount
/// written to `UiState::envelopes`; `apply_state` in `draw_ops` mixes
/// the build-time visual toward the state-modulated visual based on it.
/// Drives single-target visuals (hover-lighten, press-darken, focus-
/// ring fade) — exactly one node owns each at a time.
const STATE_PROPS: &[AnimProp] = &[
    AnimProp::HoverAmount,
    AnimProp::PressAmount,
    AnimProp::FocusRingAlpha,
];

/// Subtree state envelopes, processed alongside `STATE_PROPS`. Each
/// tracks "is the active hover / focus / press target this node or any
/// descendant?" — multiple nodes can be hot simultaneously (every
/// ancestor of the leaf target). Drives region-shaped affordances
/// (`hover_alpha`, future hover-driven translate / scale / tint).
///
/// Tracked on every focusable node (so the draw-time cascade can read
/// the nearest focusable ancestor's envelope) and on every node
/// carrying `hover_alpha` (so a non-focusable wrapper — the action-pill
/// case — has a self-envelope to OR-merge with the inherited one).
const SUBTREE_PROPS: &[AnimProp] = &[
    AnimProp::SubtreeHoverAmount,
    AnimProp::SubtreePressAmount,
    AnimProp::SubtreeFocusAmount,
];

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_node(
    node: &mut El,
    anims: &mut FxHashMap<(String, AnimProp), Animation>,
    envelopes: &mut FxHashMap<(String, EnvelopeKind), f32>,
    node_states: &FxHashMap<String, InteractionState>,
    hot: HotTargets<'_>,
    focus_visible: bool,
    visited: &mut HashSet<(String, AnimProp)>,
    now: Instant,
    mode: AnimationMode,
    palette: &Palette,
    needs_redraw: &mut bool,
) {
    if !node.computed_id.is_empty() {
        // App-driven props: only on nodes that opted in via .animate().
        if let Some(timing) = node.animate {
            for &prop in APP_PROPS {
                process_prop(
                    node,
                    prop,
                    timing,
                    anims,
                    envelopes,
                    node_states,
                    hot,
                    focus_visible,
                    visited,
                    now,
                    mode,
                    palette,
                    needs_redraw,
                );
            }
        }
        // Per-node state envelopes: only on keyed interactive nodes;
        // the library always tracks these, no author opt-in. `Kind::Scrim`
        // opts out — scrims are keyed purely so click-outside routes to
        // `{key}:dismiss`, never to receive hover/press visuals. Without
        // this exclusion, a dimmed modal scrim (opaque `OVERLAY_SCRIM`
        // fill) lightens under the cursor when hovered (#33).
        if node.key.is_some() && !matches!(node.kind, Kind::Scrim) {
            for &prop in STATE_PROPS {
                let timing = state_timing_for(prop);
                process_prop(
                    node,
                    prop,
                    timing,
                    anims,
                    envelopes,
                    node_states,
                    hot,
                    focus_visible,
                    visited,
                    now,
                    mode,
                    palette,
                    needs_redraw,
                );
            }
        }
        // Subtree envelopes: tracked on focusable nodes (so the
        // draw-time cascade can read the nearest focusable ancestor's
        // envelope) and on any node carrying `hover_alpha` (so
        // non-focusable wrappers — action pills, hover-revealed
        // badges — get a self-envelope to OR-merge with the inherited
        // one). Plain keyed-but-not-focusable nodes don't need them.
        if node.focusable || node.hover_alpha.is_some() {
            for &prop in SUBTREE_PROPS {
                let timing = state_timing_for(prop);
                process_prop(
                    node,
                    prop,
                    timing,
                    anims,
                    envelopes,
                    node_states,
                    hot,
                    focus_visible,
                    visited,
                    now,
                    mode,
                    palette,
                    needs_redraw,
                );
            }
        }
    }
    for child in &mut node.children {
        tick_node(
            child,
            anims,
            envelopes,
            node_states,
            hot,
            focus_visible,
            visited,
            now,
            mode,
            palette,
            needs_redraw,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn process_prop(
    node: &mut El,
    prop: AnimProp,
    timing: Timing,
    anims: &mut FxHashMap<(String, AnimProp), Animation>,
    envelopes: &mut FxHashMap<(String, EnvelopeKind), f32>,
    node_states: &FxHashMap<String, InteractionState>,
    hot: HotTargets<'_>,
    focus_visible: bool,
    visited: &mut HashSet<(String, AnimProp)>,
    now: Instant,
    mode: AnimationMode,
    palette: &Palette,
    needs_redraw: &mut bool,
) {
    let state = node_states
        .get(&node.computed_id)
        .copied()
        .unwrap_or_default();
    let Some(target) = compute_target(node, prop, state, hot, focus_visible, palette) else {
        return;
    };
    let key = (node.computed_id.clone(), prop);
    visited.insert(key.clone());
    let anim = anims
        .entry(key)
        .or_insert_with(|| Animation::new(target, target, timing, now));
    anim.retarget(target, now);
    let settled = match mode {
        AnimationMode::Live => anim.step(now),
        AnimationMode::Settled => {
            anim.settle();
            true
        }
    };
    write_prop(node, prop, anim.current, envelopes);
    if !settled {
        *needs_redraw = true;
    }
}

/// Compute the visual target for `prop` based on the node's current
/// interaction state and its build-closure-supplied original value.
/// Returns `None` if the prop doesn't apply (e.g., a node with no fill
/// has no `AppFill` to animate).
///
/// `focus_visible` is the runtime-level `:focus-visible` flag (raised
/// by Tab / arrow nav, cleared by pointer-down). The focus-ring target
/// only goes to 1.0 when the node is focused *and* the ring is allowed
/// — either the runtime says so, or the node opts in via
/// `always_show_focus_ring`.
fn compute_target(
    n: &El,
    prop: AnimProp,
    state: InteractionState,
    hot: HotTargets<'_>,
    focus_visible: bool,
    palette: &Palette,
) -> Option<AnimValue> {
    let in_subtree = |target: Option<&str>| -> bool {
        target.is_some_and(|t| target_in_subtree(&n.computed_id, t))
    };
    match prop {
        AnimProp::HoverAmount => Some(AnimValue::Float(
            if matches!(state, InteractionState::Hover) {
                1.0
            } else {
                0.0
            },
        )),
        AnimProp::PressAmount => Some(AnimValue::Float(
            if matches!(state, InteractionState::Press) {
                1.0
            } else {
                0.0
            },
        )),
        AnimProp::FocusRingAlpha => Some(AnimValue::Float(
            // Focus ring is independent of hover / press: a focused node
            // that is also hovered keeps `state = Hover` (Hover wins
            // over Focus in `apply_to_state`), but the ring should still
            // be on. Read `focused` straight from the hot targets so
            // the ring's envelope doesn't fall off when the cursor
            // enters the focused element.
            if hot.focused == Some(n.computed_id.as_str())
                && (focus_visible || n.always_show_focus_ring)
            {
                1.0
            } else {
                0.0
            },
        )),
        AnimProp::SubtreeHoverAmount => Some(AnimValue::Float(if in_subtree(hot.hovered) {
            1.0
        } else {
            0.0
        })),
        AnimProp::SubtreePressAmount => Some(AnimValue::Float(if in_subtree(hot.pressed) {
            1.0
        } else {
            0.0
        })),
        // Subtree focus reveals on any focused descendant — including
        // pointer-focused ones (no `focus_visible` gate). The pattern
        // is "show me the close × on my focused tab", which a pointer
        // focus path should satisfy too.
        AnimProp::SubtreeFocusAmount => Some(AnimValue::Float(if in_subtree(hot.focused) {
            1.0
        } else {
            0.0
        })),
        // Resolve through the active palette so the integration walks
        // the user's palette's rgb space (e.g., slate blue's PRIMARY
        // (0,144,255)), not the compile-time baked default-dark rgb on
        // the token constant. Without this the in-flight color reads
        // against default-dark values and only snaps to the user's
        // palette when the animation settles — visible as a brief
        // wrong-palette flash mid-transition. `palette.resolve`
        // preserves the token name on the returned color, so settled
        // values stay tokenized for downstream palette swaps.
        AnimProp::AppFill => n.fill.map(|c| AnimValue::Color(palette.resolve(c))),
        AnimProp::AppStroke => n.stroke.map(|c| AnimValue::Color(palette.resolve(c))),
        AnimProp::AppTextColor => n.text_color.map(|c| AnimValue::Color(palette.resolve(c))),
        AnimProp::AppOpacity => Some(AnimValue::Float(n.opacity)),
        AnimProp::AppScale => Some(AnimValue::Float(n.scale)),
        AnimProp::AppTranslateX => Some(AnimValue::Float(n.translate.0)),
        AnimProp::AppTranslateY => Some(AnimValue::Float(n.translate.1)),
    }
}

/// Library-default timing for state-driven envelopes. Hover, press,
/// focus transitions (and their subtree analogues) are uniformly
/// snappy — overshoot on a 0..1 envelope reads as jitter, so we stick
/// to a near-critical preset.
fn state_timing_for(prop: AnimProp) -> Timing {
    match prop {
        AnimProp::HoverAmount
        | AnimProp::PressAmount
        | AnimProp::FocusRingAlpha
        | AnimProp::SubtreeHoverAmount
        | AnimProp::SubtreePressAmount
        | AnimProp::SubtreeFocusAmount => Timing::SPRING_QUICK,
        // App props don't reach this function — they pull timing from
        // the per-node `animate` setting in `tick_node`.
        _ => Timing::SPRING_QUICK,
    }
}

fn write_prop(
    n: &mut El,
    prop: AnimProp,
    value: AnimValue,
    envelopes: &mut FxHashMap<(String, EnvelopeKind), f32>,
) {
    match (prop, value) {
        (AnimProp::AppFill, AnimValue::Color(c)) => n.fill = Some(c),
        (AnimProp::AppStroke, AnimValue::Color(c)) => n.stroke = Some(c),
        (AnimProp::AppTextColor, AnimValue::Color(c)) => n.text_color = Some(c),
        (AnimProp::HoverAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::Hover),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::PressAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::Press),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::FocusRingAlpha, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::FocusRing),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::SubtreeHoverAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::SubtreeHover),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::SubtreePressAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::SubtreePress),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::SubtreeFocusAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::SubtreeFocus),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::AppOpacity, AnimValue::Float(v)) => n.opacity = v.clamp(0.0, 1.0),
        (AnimProp::AppScale, AnimValue::Float(v)) => n.scale = v.max(0.0),
        (AnimProp::AppTranslateX, AnimValue::Float(v)) => n.translate.0 = v,
        (AnimProp::AppTranslateY, AnimValue::Float(v)) => n.translate.1 = v,
        _ => {}
    }
}

pub(crate) fn is_in_flight(anim: &Animation) -> bool {
    let cur = anim.current.channels();
    let tgt = anim.target.channels();
    if cur.n != tgt.n {
        return true;
    }
    for i in 0..cur.n {
        if (cur.v[i] - tgt.v[i]).abs() > f32::EPSILON {
            return true;
        }
        if anim.velocity.n == cur.n && anim.velocity.v[i].abs() > f32::EPSILON {
            return true;
        }
    }
    false
}
