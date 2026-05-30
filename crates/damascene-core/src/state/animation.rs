//! Visual animation, caret blink, and state-summary helpers.

use std::collections::HashSet;

use web_time::Instant;

use crate::anim::AnimProp;
use crate::anim::tick::{HotTargets, is_in_flight, tick_node};
use crate::event::UiTarget;
use crate::palette::Palette;
use crate::tree::El;

use super::{AnimationMode, EnvelopeKind, UiState, caret_blink_alpha_for};

impl UiState {
    /// Current eased state envelope amount in `[0, 1]` for `(id, kind)`.
    /// Missing entries read as `0.0`.
    pub fn envelope(&self, id: &str, kind: EnvelopeKind) -> f32 {
        self.animation
            .envelopes
            .get(&(id.to_string(), kind))
            .copied()
            .unwrap_or(0.0)
    }

    /// Reset the caret-blink phase to "fully on": the painter holds
    /// the caret solid for `CARET_BLINK_GRACE` after this call before
    /// resuming the on/off cycle. Called whenever the user does
    /// something the caret should react to — focusing an input,
    /// moving the caret, replacing the selection.
    pub(crate) fn bump_caret_activity(&mut self, now: Instant) {
        self.caret.activity_at = Some(now);
        self.caret.blink_alpha = 1.0;
    }

    /// Walk the laid-out tree, retarget per-(node, prop) animations to
    /// the values implied by each node's current state, step them
    /// forward to `now`, and write back: app-driven props mutate the
    /// El's `fill` / `text_color` / `stroke` / `opacity` / `translate` /
    /// `scale` (so the next rebuild reads the eased value); state
    /// envelopes are written to the envelope side map for `draw_ops` to
    /// modulate visuals from.
    ///
    /// Returns `true` if any animation is still in flight; the host
    /// should request another redraw next frame.
    pub fn tick_visual_animations(
        &mut self,
        root: &mut El,
        now: Instant,
        palette: &Palette,
    ) -> bool {
        let mut visited: HashSet<(String, AnimProp)> = HashSet::new();
        let mut needs_redraw = false;
        let mode = self.animation.mode;
        // Snapshot the leaf hover/focus/press targets so the per-node
        // tick can derive subtree-membership without re-borrowing self.
        let hot = HotTargets {
            hovered: self.hovered.as_ref().map(|t| t.node_id.as_str()),
            focused: self.focused.as_ref().map(|t| t.node_id.as_str()),
            pressed: self.pressed.as_ref().map(|t| t.node_id.as_str()),
        };
        tick_node(
            root,
            &mut self.animation.animations,
            &mut self.animation.envelopes,
            &self.node_states.nodes,
            hot,
            self.focus_visible,
            &mut visited,
            now,
            mode,
            palette,
            &mut needs_redraw,
        );
        // GC: drop animations whose node left the tree this frame.
        self.animation
            .animations
            .retain(|key, _| visited.contains(key));
        // Build a set of live node ids once — used by both envelope and
        // widget_state GC. Cheaper than the previous per-entry linear
        // scan over `visited`, which now matters because widget_state
        // entries can outnumber envelopes.
        let live_ids: HashSet<&str> = visited.iter().map(|(id, _)| id.as_str()).collect();
        self.animation
            .envelopes
            .retain(|(id, _), _| live_ids.contains(id.as_str()));
        self.widget_states
            .entries
            .retain(|(id, _), _| live_ids.contains(id.as_str()));

        // Caret blink. Resolve the new alpha from the activity age,
        // then keep requesting redraws as long as a capture_keys node
        // is focused so the cycle keeps animating in idle frames.
        // `Settled` mode pins the caret to fully on so headless
        // single-frame snapshots don't randomly catch the off phase.
        if let Some(activity_at) = self.caret.activity_at {
            let alpha = match mode {
                AnimationMode::Settled => 1.0,
                AnimationMode::Live => {
                    caret_blink_alpha_for(now.saturating_duration_since(activity_at))
                }
            };
            self.caret.blink_alpha = alpha;
        }
        if mode == AnimationMode::Live && self.focused_node_captures_keys(root) {
            needs_redraw = true;
        }

        needs_redraw
    }

    /// Walk `root` and return whether the currently-focused node has
    /// `capture_keys` set. Used by the animation tick to keep
    /// requesting redraws while a text input is focused (so the caret
    /// blink keeps animating). Returns `false` when no node is focused
    /// or the focused node isn't in the tree.
    fn focused_node_captures_keys(&self, root: &El) -> bool {
        let Some(focused) = self.focused.as_ref() else {
            return false;
        };
        crate::runtime::find_capture_keys(root, &focused.node_id).unwrap_or(false)
    }

    /// Switch animation pacing. The default is [`AnimationMode::Live`];
    /// headless render binaries flip to [`AnimationMode::Settled`] so
    /// a single-frame snapshot reflects the post-animation visual
    /// without depending on integrator timing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.animation.mode = mode;
    }

    /// Current animation pacing. Backends read this to gate
    /// time-driven shader uniforms (e.g. `frame.time`) so headless
    /// fixtures stay byte-identical regardless of when they ran.
    pub fn animation_mode(&self) -> AnimationMode {
        self.animation.mode
    }

    /// Whether any visual animation is still moving. The host's runner
    /// uses this (via the renderer's `PrepareResult`) to keep the redraw
    /// loop ticking only while there's motion.
    pub fn has_animations_in_flight(&self) -> bool {
        self.animation.animations.values().any(is_in_flight)
    }

    /// One-line summary of interactive state for diagnostic logging.
    /// Format: `hov=<key|->|press=<key|->|focus=<key|->|env={...}|in_flight=N`.
    /// Keep terse — this is intended for per-frame `console.log`.
    pub fn debug_summary(&self) -> String {
        let key = |t: &Option<UiTarget>| {
            t.as_ref()
                .map(|t| t.key.clone())
                .unwrap_or_else(|| "-".into())
        };
        let mut env: Vec<String> = self
            .animation
            .envelopes
            .iter()
            .map(|((id, kind), v)| format!("{id}/{kind:?}={v:.3}"))
            .collect();
        env.sort();
        let in_flight = self
            .animation
            .animations
            .values()
            .filter(|a| is_in_flight(a))
            .count();
        format!(
            "hov={}|press={}|focus={}|env=[{}]|in_flight={}/{}",
            key(&self.hovered),
            key(&self.pressed),
            key(&self.focused),
            env.join(","),
            in_flight,
            self.animation.animations.len(),
        )
    }
}
