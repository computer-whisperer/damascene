//! Animation primitives.
//!
//! Two motion models ship: spring physics (semi-implicit Euler) and
//! cubic-bezier tweens. Springs are the default — they continue from
//! current+velocity when retargeted mid-flight, which is what makes
//! interrupted motion feel right (mouse-out-mid-fade eases back from
//! where it is, not from rest). Tweens cover the explicit-duration
//! cases where the curve matters more than the physics.
//!
//! ## Animatable values
//!
//! [`AnimValue`] holds the per-prop state the integrator works on.
//! `Float` (1 channel) covers opacity / scale / translation; `Color`
//! (4 channels) covers fills / strokes / text colors. The integrator
//! treats each channel as an independent 1-D mass-spring-damper.
//!
//! ## Spring config
//!
//! Mass-spring-damper: `m·a = -k·x - c·v` where `x = current - target`,
//! integrated semi-implicitly. `dt` is clamped to 64 ms so a stalled
//! frame can't blow up the integrator. Settles when both displacement
//! and velocity drop below epsilon for *all* channels.
//!
//! ## Headless determinism
//!
//! The bundle path calls [`Animation::settle`] on every in-flight
//! animation before snapshotting, so SVG/PNG fixtures are byte-identical
//! run-to-run regardless of how many frames were sampled.

use std::time::Duration;
// web_time::Instant works on wasm32 (std::time::Instant::now() panics there).
use web_time::Instant;

use crate::tree::Color;

pub mod tick;

/// A value the animator can interpolate. Each variant fans out to a
/// fixed number of f32 channels that the integrator steps independently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimValue {
    Float(f32),
    Color(Color),
}

impl AnimValue {
    pub fn channels(self) -> AnimChannels {
        match self {
            AnimValue::Float(v) => AnimChannels {
                n: 1,
                v: [v, 0.0, 0.0, 0.0],
            },
            AnimValue::Color(c) => AnimChannels {
                n: 4,
                v: [c.r, c.g, c.b, c.a],
            },
        }
    }

    /// Reconstruct an `AnimValue` of the same variant from sampled
    /// channels. The token name is dropped — an in-flight interpolated
    /// rgba doesn't equal any palette token's rgb, so carrying a name
    /// on it would mislead palette resolution. When the animation
    /// settles, `step_spring` / `step_tween` assign
    /// `self.current = self.target` directly, restoring the target's
    /// token on the final value. Channel space is recovered from the
    /// previous-frame value (`self`) so spring overshoot stays in the
    /// space the author authored in.
    pub fn from_channels(self, ch: AnimChannels) -> AnimValue {
        match self {
            AnimValue::Float(_) => AnimValue::Float(ch.v[0]),
            AnimValue::Color(prev) => AnimValue::Color(Color {
                r: ch.v[0],
                g: ch.v[1],
                b: ch.v[2],
                a: ch.v[3],
                space: prev.space,
                token: None,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AnimChannels {
    pub n: usize,
    pub v: [f32; 4],
}

impl AnimChannels {
    pub fn zero(n: usize) -> Self {
        Self { n, v: [0.0; 4] }
    }
}

/// Spring physics configuration: mass-spring-damper.
///
/// The four preset constants are calibrated to feel competitive with
/// modern native motion (UIKit defaults, Material 3 motion). Authors
/// pick a preset; ad-hoc tuning is intentionally not exposed to keep
/// the surface area small.
#[derive(Clone, Copy, Debug)]
pub struct SpringConfig {
    pub mass: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringConfig {
    /// High stiffness, near-critical damping. ~150 ms settle, no
    /// overshoot. Use for hover / focus where overshoot reads as jitter.
    pub const QUICK: Self = Self {
        mass: 1.0,
        stiffness: 380.0,
        damping: 30.0,
    };
    /// Balanced. ~250 ms settle, mild overshoot. Default state changes.
    pub const STANDARD: Self = Self {
        mass: 1.0,
        stiffness: 200.0,
        damping: 22.0,
    };
    /// Visible overshoot. Press-release rebound, playful interactions.
    pub const BOUNCY: Self = Self {
        mass: 1.0,
        stiffness: 240.0,
        damping: 14.0,
    };
    /// Soft, large displacements. Modal appearance, panel transitions.
    pub const GENTLE: Self = Self {
        mass: 1.0,
        stiffness: 80.0,
        damping: 18.0,
    };
}

/// Cubic-bezier tween: P0=(0,0), P3=(1,1), with two control points.
#[derive(Clone, Copy, Debug)]
pub struct TweenConfig {
    pub duration: Duration,
    pub p1: (f32, f32),
    pub p2: (f32, f32),
}

impl TweenConfig {
    /// 100 ms ease-out. For micro-interactions where physics is overkill.
    pub const EASE_QUICK: Self = Self {
        duration: Duration::from_millis(100),
        p1: (0.0, 0.0),
        p2: (0.2, 1.0),
    };
    /// 200 ms ease-in-out. Symmetric default tween.
    pub const EASE_STANDARD: Self = Self {
        duration: Duration::from_millis(200),
        p1: (0.4, 0.0),
        p2: (0.2, 1.0),
    };
    /// 350 ms slow-out, fast-end. For larger displacements where the
    /// final settle should feel decisive.
    pub const EASE_EMPHASIZED: Self = Self {
        duration: Duration::from_millis(350),
        p1: (0.05, 0.7),
        p2: (0.1, 1.0),
    };
}

/// Choice of motion model for an animated property. Springs feel
/// physical (continue from current+velocity on retarget); tweens feel
/// curated (fixed curve, fixed duration).
#[derive(Clone, Copy, Debug)]
pub enum Timing {
    Spring(SpringConfig),
    Tween(TweenConfig),
}

impl Timing {
    pub const SPRING_QUICK: Self = Timing::Spring(SpringConfig::QUICK);
    pub const SPRING_STANDARD: Self = Timing::Spring(SpringConfig::STANDARD);
    pub const SPRING_BOUNCY: Self = Timing::Spring(SpringConfig::BOUNCY);
    pub const SPRING_GENTLE: Self = Timing::Spring(SpringConfig::GENTLE);
    pub const EASE_QUICK: Self = Timing::Tween(TweenConfig::EASE_QUICK);
    pub const EASE_STANDARD: Self = Timing::Tween(TweenConfig::EASE_STANDARD);
    pub const EASE_EMPHASIZED: Self = Timing::Tween(TweenConfig::EASE_EMPHASIZED);
}

/// Identifies a specific animatable property on a node. Used as part
/// of the per-(node, prop) tracker key.
///
/// Two families:
///
/// - **State envelopes** (`HoverAmount`, `PressAmount`, `FocusRingAlpha`)
///   are 0..1 floats tracking *how much* of the corresponding state's
///   visual delta is currently applied. The library updates these on
///   every keyed interactive node automatically; no author opt-in. Why
///   envelopes and not absolute colours: `apply_state` in `draw_ops`
///   computes the display colour by lerping between `n.fill` and
///   `state_color(n.fill)` based on the envelope. That keeps state
///   easing completely independent of build-value changes — when the
///   author swaps a button's fill mid-hover, the new fill takes effect
///   instantly with the same hover envelope, no fighting between
///   trackers.
/// - **App-driven absolute values** (`App*`) are author-opted-in via
///   [`crate::tree::El::animate`]. The tracker eases the value the build
///   closure produces from the previous frame's value to the new one.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimProp {
    /// 0..1 amount of the hover-state visual delta currently applied.
    /// Eases 0→1 on pointer enter, 1→0 on pointer leave.
    HoverAmount,
    /// 0..1 amount of the press-state visual delta currently applied.
    /// Eases 0→1 on press, 1→0 on release.
    PressAmount,
    /// Focus-ring alpha — eases 0→1 on focus enter, 1→0 on focus leave.
    /// Lets the ring fade out after focus moves elsewhere.
    FocusRingAlpha,
    /// 0..1 amount tracking "is the hover target this node or any
    /// descendant?". Eases 0→1 when the cursor enters the subtree, 1→0
    /// when it leaves. Drives region-shaped hover affordances
    /// (`hover_alpha`, future hover-driven translate / scale / tint).
    SubtreeHoverAmount,
    /// 0..1 amount tracking "is the press target this node or any
    /// descendant?". Subtree analogue of `PressAmount`.
    SubtreePressAmount,
    /// 0..1 amount tracking "is the focus target this node or any
    /// descendant?". Subtree analogue of `FocusRingAlpha`. Composed
    /// with `SubtreeHoverAmount` by `hover_alpha` so keyboard focus
    /// reveals the same affordance hover does.
    SubtreeFocusAmount,
    /// App-driven fill colour — eases between the values the build
    /// closure produces across rebuilds.
    AppFill,
    /// App-driven stroke colour.
    AppStroke,
    /// App-driven text colour.
    AppTextColor,
    /// App-driven paint-time alpha multiplier in `[0, 1]`.
    AppOpacity,
    /// App-driven uniform scale around the rect centre.
    AppScale,
    /// App-driven translate offset in logical pixels — X channel.
    AppTranslateX,
    /// App-driven translate offset in logical pixels — Y channel.
    AppTranslateY,
}

const SPRING_EPSILON_DISP: f32 = 0.5;
const SPRING_EPSILON_VEL: f32 = 0.5;
const DT_CAP: f32 = 0.064;
/// Hard upper bound on the per-substep timestep used inside `step_spring`.
/// The semi-implicit Euler scheme with explicit damping is stable for
/// `dt < 2·sqrt(m/k) + small damping correction`; the stiffest preset
/// (`SpringConfig::QUICK`, k=380, c=30) has a stability bound near 58 ms.
/// `DT_CAP` (64 ms) sits above that, so without substepping the integrator
/// can blow up after long idle pauses or on slow frames — `current`
/// overshoots into ±values and the 0..1 envelope `clamp` rounds to a
/// binary flicker. 4 ms keeps every preset comfortably stable.
const SPRING_MAX_SUBSTEP: f32 = 1.0 / 250.0;

/// In-flight animation state for one (node, prop) pair. Stored on
/// [`crate::state::UiState`] keyed by `(ComputedId, AnimProp)`.
///
/// `current` is the read-back view consumed by `write_prop` — for
/// `AnimValue::Color` that's u8 rgba. The integrator's per-frame
/// motion near equilibrium is sub-integer in rgb units (typical
/// `vel * dt ≈ 0.1–0.4` once the spring is close to target), so
/// integrating against the rounded view loses fractional progress
/// every frame and the integrator freezes a few rgb units off
/// target. `current_precise` is the lossless f32 mirror integrators
/// actually read and write across ticks.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Animation {
    pub current: AnimValue,
    pub target: AnimValue,
    pub velocity: AnimChannels,
    pub timing: Timing,
    pub started_at: Instant,
    pub last_step: Instant,
    /// For tweens, the value at `started_at`. Springs are fully
    /// determined by current+velocity, so `from` stays `None`.
    pub from: Option<AnimValue>,
    /// Lossless f32 mirror of `current` for the integrator. See struct
    /// doc — `AnimValue::Color` stores u8, which silently freezes the
    /// spring once per-frame motion drops below 0.5 rgb units.
    current_precise: AnimChannels,
}

impl Animation {
    pub fn new(current: AnimValue, target: AnimValue, timing: Timing, now: Instant) -> Self {
        let channels = current.channels();
        let n = channels.n;
        let from = match timing {
            Timing::Tween(_) => Some(current),
            Timing::Spring(_) => None,
        };
        Self {
            current,
            target,
            velocity: AnimChannels::zero(n),
            timing,
            started_at: now,
            last_step: now,
            from,
            current_precise: channels,
        }
    }

    /// Re-target a running animation. Current value and velocity carry
    /// over so interrupted motion eases from where it is, not from rest.
    /// For tweens, `from` snaps to the current sample so the new curve
    /// starts there; the tween clock resets.
    pub fn retarget(&mut self, target: AnimValue, now: Instant) {
        if same_value(self.target, target) {
            return;
        }
        self.target = target;
        if matches!(self.timing, Timing::Tween(_)) {
            self.from = Some(self.current);
            self.started_at = now;
        }
        // Springs: keep current+velocity untouched. The integrator now
        // sees a different `target` and forces will steer toward it.
    }

    /// Snap to target and zero velocity. Used by the headless bundle
    /// path so SVG/PNG fixtures don't depend on integrator timing.
    pub fn settle(&mut self) {
        self.current = self.target;
        self.current_precise = self.target.channels();
        let n = self.current_precise.n;
        self.velocity = AnimChannels::zero(n);
        self.from = None;
    }

    /// Step the animation forward to `now`. Returns `true` if settled.
    pub fn step(&mut self, now: Instant) -> bool {
        let dt = now
            .saturating_duration_since(self.last_step)
            .as_secs_f32()
            .min(DT_CAP);
        self.last_step = now;
        match self.timing {
            Timing::Spring(cfg) => self.step_spring(cfg, dt),
            Timing::Tween(cfg) => self.step_tween(cfg, now),
        }
    }

    fn step_spring(&mut self, cfg: SpringConfig, dt: f32) -> bool {
        if dt <= 0.0 {
            return self.is_settled();
        }
        let mut cur = if self.current_precise.n == self.current.channels().n {
            self.current_precise
        } else {
            self.current.channels()
        };
        let tgt = self.target.channels();
        let mut vel = if self.velocity.n == cur.n {
            self.velocity
        } else {
            AnimChannels::zero(cur.n)
        };
        // Substep so each integrator step is well within the stability
        // bound for every SpringConfig preset. A single h = `dt` step
        // would diverge for stiff presets when frames stall or the host
        // resumes after a long idle (dt clamped to DT_CAP > stability
        // bound for QUICK), producing binary 0/1 flicker once `current`
        // overshoots into ±range and write_prop's clamp rounds it.
        let n_steps = (dt / SPRING_MAX_SUBSTEP).ceil().max(1.0) as usize;
        let h = dt / n_steps as f32;
        let mut all_settled = false;
        for _ in 0..n_steps {
            all_settled = true;
            for i in 0..cur.n {
                let displacement = cur.v[i] - tgt.v[i];
                let force = -cfg.stiffness * displacement - cfg.damping * vel.v[i];
                // Semi-implicit Euler: update velocity first, then position
                // using the new velocity. More stable than fully explicit
                // for stiff systems within UI's typical stiffness range.
                vel.v[i] += (force / cfg.mass) * h;
                cur.v[i] += vel.v[i] * h;
                if displacement.abs() > SPRING_EPSILON_DISP || vel.v[i].abs() > SPRING_EPSILON_VEL {
                    all_settled = false;
                }
            }
            if all_settled {
                break;
            }
        }
        if all_settled {
            self.current = self.target;
            self.current_precise = tgt;
            self.velocity = AnimChannels::zero(cur.n);
            return true;
        }
        self.current_precise = cur;
        self.current = self.current.from_channels(cur);
        self.velocity = vel;
        false
    }

    fn step_tween(&mut self, cfg: TweenConfig, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.started_at);
        if elapsed >= cfg.duration {
            self.current = self.target;
            self.current_precise = self.target.channels();
            return true;
        }
        let from = self.from.unwrap_or(self.current).channels();
        let tgt = self.target.channels();
        let t = elapsed.as_secs_f32() / cfg.duration.as_secs_f32();
        let eased = cubic_bezier_y_at_x(t, cfg.p1, cfg.p2);
        let mut next = AnimChannels {
            n: from.n,
            v: [0.0; 4],
        };
        for i in 0..from.n {
            next.v[i] = from.v[i] + (tgt.v[i] - from.v[i]) * eased;
        }
        self.current_precise = next;
        self.current = self.current.from_channels(next);
        false
    }

    fn is_settled(&self) -> bool {
        same_value(self.current, self.target)
            && (0..self.velocity.n).all(|i| self.velocity.v[i].abs() <= SPRING_EPSILON_VEL)
    }
}

fn same_value(a: AnimValue, b: AnimValue) -> bool {
    let ca = a.channels();
    let cb = b.channels();
    if ca.n != cb.n {
        return false;
    }
    (0..ca.n).all(|i| (ca.v[i] - cb.v[i]).abs() < f32::EPSILON)
}

/// Solve `cubic_bezier(t).x == x` for `t`, then return `cubic_bezier(t).y`.
/// P0=(0,0), P3=(1,1). Newton-Raphson with binary-search fallback.
fn cubic_bezier_y_at_x(x: f32, p1: (f32, f32), p2: (f32, f32)) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Newton-Raphson on x(t) — converges in 4-6 iterations for typical
    // ease curves. Fall back to bisection if the derivative collapses.
    let mut t = x;
    for _ in 0..8 {
        let xt = bezier_axis(t, p1.0, p2.0);
        let dx = bezier_axis_derivative(t, p1.0, p2.0);
        if dx.abs() < 1e-6 {
            break;
        }
        let next = t - (xt - x) / dx;
        if (next - t).abs() < 1e-5 {
            t = next.clamp(0.0, 1.0);
            break;
        }
        t = next.clamp(0.0, 1.0);
    }
    bezier_axis(t, p1.1, p2.1)
}

/// Cubic Bezier polynomial: B(t) = 3·(1-t)²·t·c1 + 3·(1-t)·t²·c2 + t³.
/// P0 and P3 are pinned at 0 and 1 (no contribution beyond the t³ term).
fn bezier_axis(t: f32, c1: f32, c2: f32) -> f32 {
    let one_minus_t = 1.0 - t;
    3.0 * one_minus_t * one_minus_t * t * c1 + 3.0 * one_minus_t * t * t * c2 + t * t * t
}

fn bezier_axis_derivative(t: f32, c1: f32, c2: f32) -> f32 {
    let one_minus_t = 1.0 - t;
    3.0 * one_minus_t * one_minus_t * c1
        + 6.0 * one_minus_t * t * (c2 - c1)
        + 3.0 * t * t * (1.0 - c2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_plus(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn spring_settles_to_target() {
        let start = Instant::now();
        let mut a = Animation::new(
            AnimValue::Float(0.0),
            AnimValue::Float(1.0),
            Timing::SPRING_QUICK,
            start,
        );
        let mut t = start;
        for _ in 0..200 {
            t += Duration::from_millis(8);
            if a.step(t) {
                break;
            }
        }
        let AnimValue::Float(v) = a.current else {
            panic!("expected float")
        };
        assert!((v - 1.0).abs() < 1e-3, "spring did not settle: v={v}");
    }

    #[test]
    fn spring_retarget_preserves_velocity() {
        // Start moving 0 → 1; mid-flight retarget back to 0 should
        // briefly continue past the new target before reversing —
        // momentum carries.
        let start = Instant::now();
        let mut a = Animation::new(
            AnimValue::Float(0.0),
            AnimValue::Float(1.0),
            Timing::SPRING_STANDARD,
            start,
        );
        let mut t = start;
        for _ in 0..15 {
            t += Duration::from_millis(8);
            a.step(t);
        }
        let mid = match a.current {
            AnimValue::Float(v) => v,
            _ => unreachable!(),
        };
        assert!(mid > 0.0 && mid < 1.0, "expected mid-flight, got {mid}");
        let velocity_before = a.velocity.v[0];
        assert!(velocity_before > 0.0);
        a.retarget(AnimValue::Float(0.0), t);
        // Velocity is preserved — the spring will continue forward briefly.
        assert_eq!(a.velocity.v[0], velocity_before);
    }

    #[test]
    fn tween_samples_endpoints() {
        let start = Instant::now();
        let mut a = Animation::new(
            AnimValue::Float(10.0),
            AnimValue::Float(20.0),
            Timing::EASE_STANDARD,
            start,
        );
        a.step(start);
        let AnimValue::Float(v0) = a.current else {
            panic!()
        };
        assert!(
            (v0 - 10.0).abs() < 1e-3,
            "tween at t=0 should equal `from`, got {v0}"
        );

        a.step(now_plus(start, 1000));
        let AnimValue::Float(vend) = a.current else {
            panic!()
        };
        assert!(
            (vend - 20.0).abs() < 1e-3,
            "tween past duration should equal target, got {vend}"
        );
    }

    #[test]
    fn tween_retarget_snaps_from_to_current() {
        let start = Instant::now();
        let mut a = Animation::new(
            AnimValue::Float(0.0),
            AnimValue::Float(100.0),
            Timing::EASE_STANDARD,
            start,
        );
        a.step(now_plus(start, 100));
        let AnimValue::Float(mid) = a.current else {
            panic!()
        };
        a.retarget(AnimValue::Float(0.0), now_plus(start, 100));
        assert_eq!(a.from, Some(AnimValue::Float(mid)));
    }

    #[test]
    fn settle_snaps_to_target() {
        let start = Instant::now();
        let mut a = Animation::new(
            AnimValue::Color(Color::srgb_u8a(0, 0, 0, 255)),
            AnimValue::Color(Color::srgb_u8a(255, 128, 0, 255)),
            Timing::SPRING_STANDARD,
            start,
        );
        a.step(now_plus(start, 5));
        a.settle();
        match a.current {
            AnimValue::Color(c) => {
                assert_eq!(c.to_srgb_u8a(), [255, 128, 0, 255]);
            }
            _ => panic!("expected color"),
        }
        assert!(a.velocity.v.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn cubic_bezier_endpoints_pin() {
        // Any curve must satisfy P(0)=0 and P(1)=1.
        let p1 = (0.4, 0.0);
        let p2 = (0.2, 1.0);
        assert!((cubic_bezier_y_at_x(0.0, p1, p2) - 0.0).abs() < 1e-3);
        assert!((cubic_bezier_y_at_x(1.0, p1, p2) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn color_channels_round_trip() {
        let c = Color::srgb_u8a(42, 17, 200, 255);
        let v = AnimValue::Color(c);
        let ch = v.channels();
        assert_eq!(ch.n, 4);
        assert_eq!(ch.v, [42.0 / 255.0, 17.0 / 255.0, 200.0 / 255.0, 1.0]);
        let back = v.from_channels(ch);
        assert_eq!(back, AnimValue::Color(c));
    }

    #[test]
    fn from_channels_drops_token_on_in_flight_eased_value() {
        // An in-flight eased rgba is not the same color as the source
        // token — keeping the token name on it would let palette
        // resolution snap the rgb back to the source token's palette
        // value, killing the transition. Spring/tween settled paths
        // bypass `from_channels` and assign `self.current = self.target`
        // directly, so settled values still carry the target's token.
        let v = AnimValue::Color(Color::srgb_token("primary", 92, 170, 255, 255));
        let mid = AnimChannels {
            n: 4,
            v: [128.0 / 255.0, 100.0 / 255.0, 80.0 / 255.0, 1.0],
        };
        let eased = v.from_channels(mid);
        match eased {
            AnimValue::Color(c) => {
                assert_eq!(c.token, None, "in-flight eased color must drop the token");
                let [r, g, b, _] = c.to_srgb_u8a();
                assert_eq!((r, g, b), (128, 100, 80));
            }
            _ => panic!("expected color"),
        }
    }
}
