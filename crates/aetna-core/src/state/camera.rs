//! Per-scene camera state for [`UiState`](super::UiState).
//!
//! Mirrors the scroll keyed-state pattern: the library owns one persistent
//! camera per `Scene3D` node (keyed by `computed_id`), so `chart3d(...)` is
//! navigable with zero app state. Each keyed camera holds a `current` pose
//! and a `goal` pose; a spring integrates `current → goal` every frame.
//! *Everything that changes the viewpoint sets `goal`* — data re-centre,
//! a [`Focus`] request, (and, in the gesture slice) wheel zoom — and the
//! spring animates. Active drag writes `current` and `goal` together for a
//! crisp 1:1 feel (gesture slice).
//!
//! The spring mirrors `anim::Animation`'s scheme (semi-implicit Euler,
//! substepped for stability) but runs per channel on the 6-DOF pose —
//! `target.{x,y,z}`, `ln(distance)` (so animated zoom is geometric, like
//! the multiplicative manual zoom), `yaw`, `pitch` — which the node
//! `AnimProp` path can't express. It is kept self-contained rather than
//! refactoring the battle-tested visual-animation integrator.

use std::collections::HashMap;

use web_time::Instant;

use crate::anim::SpringConfig;
use crate::scene::glam::Vec3;
use crate::scene::{Aabb, CameraState, Focus, Framing, Scene3DData};
use crate::tree::El;

use super::UiState;

/// Per-substep cap for integrator stability (see `anim::SPRING_MAX_SUBSTEP`).
const MAX_SUBSTEP: f32 = 1.0 / 250.0;
/// Clamp on a single tick's dt so a stalled frame can't blow up the spring.
const DT_CAP: f32 = 0.064;
/// Settle thresholds. Channels are world units / log-distance / radians —
/// all small-magnitude, so tight epsilons keep motion smooth to a stop
/// without spinning the redraw loop forever.
const EPS_DISP: f32 = 1.0e-3;
const EPS_VEL: f32 = 1.0e-2;
/// Bounds are "changed" when centre or radius moves more than this.
const BOUNDS_EPSILON: f32 = 1.0e-3;
/// Soft, no-overshoot glide for viewpoint moves (refocus / re-centre).
const POSE_SPRING: SpringConfig = SpringConfig::GENTLE;

/// One node's persistent camera: the animating `current` pose, the `goal`
/// it springs toward, per-channel velocity, and the inputs we diff against
/// to decide when to retarget.
#[derive(Clone, Debug)]
pub(crate) struct KeyedCamera {
    pub current: CameraState,
    pub goal: CameraState,
    /// Velocity per channel: target.x, .y, .z, ln(distance), yaw, pitch.
    vel: [f32; 6],
    /// Content bounds the goal was last framed against (data-change detect).
    last_bounds: Aabb,
    /// Focus request last applied (change detect).
    last_focus: Option<Focus>,
    last_step: Instant,
}

impl KeyedCamera {
    fn channels(pose: CameraState) -> [f32; 6] {
        [
            pose.target.x,
            pose.target.y,
            pose.target.z,
            pose.distance.max(1.0e-4).ln(),
            pose.yaw,
            pose.pitch,
        ]
    }

    fn from_channels(c: [f32; 6]) -> CameraState {
        CameraState {
            target: Vec3::new(c[0], c[1], c[2]),
            distance: c[3].exp(),
            yaw: c[4],
            pitch: c[5],
        }
    }

    /// Step `current` toward `goal`. Returns true once settled.
    fn step(&mut self, now: Instant) -> bool {
        let dt = now
            .saturating_duration_since(self.last_step)
            .as_secs_f32()
            .min(DT_CAP);
        self.last_step = now;
        if dt <= 0.0 {
            return self.settled();
        }
        let cur = Self::channels(self.current);
        let mut goal = Self::channels(self.goal);
        // Shortest-path yaw: rotate the short way round, never the long way.
        goal[4] = cur[4] + shortest_angle(goal[4] - cur[4]);

        let mut next = cur;
        let mut settled = true;
        for i in 0..6 {
            let (c, v, s) = spring1(cur[i], self.vel[i], goal[i], POSE_SPRING, dt);
            next[i] = c;
            self.vel[i] = v;
            settled &= s;
        }
        self.current = Self::from_channels(next);
        settled
    }

    fn settled(&self) -> bool {
        self.vel.iter().all(|v| v.abs() <= EPS_VEL)
            && Self::channels(self.current)
                .iter()
                .zip(Self::channels(self.goal))
                .all(|(c, g)| (c - g).abs() <= EPS_DISP)
    }
}

/// Keyed camera storage on [`UiState`].
#[derive(Default)]
pub(crate) struct CameraStore {
    cameras: HashMap<String, KeyedCamera>,
}

impl UiState {
    /// Resolved current pose for a keyed scene camera (`Auto` / `Fit`).
    /// `None` if the node hasn't been ticked yet or uses `Manual` framing
    /// (where the app owns the pose). Read by `draw_ops`.
    pub(crate) fn scene_camera(&self, id: &str) -> Option<CameraState> {
        self.cameras.cameras.get(id).map(|c| c.current)
    }

    /// Advance every keyed scene camera toward its goal. Walks `root` for
    /// `Scene3D` nodes, retargets the goal from framing policy / data
    /// bounds / focus request, and springs `current → goal`. Returns true
    /// if any camera is still animating (so the frame re-requests a redraw,
    /// like a settling visual animation). `Manual` nodes are skipped — the
    /// app owns those poses. Cameras for nodes absent this frame are
    /// pruned.
    pub(crate) fn tick_scene_cameras(&mut self, root: &El, now: Instant) -> bool {
        // Collect scene nodes first (immutable borrow of the tree), then
        // mutate `self.cameras` — two distinct objects, no borrow clash.
        let mut nodes: Vec<(&str, &crate::scene::SceneSpec)> = Vec::new();
        collect_scene_nodes(root, &mut nodes);

        let mut animating = false;
        let mut seen: Vec<&str> = Vec::with_capacity(nodes.len());
        for (id, spec) in nodes {
            if spec.framing == Framing::Manual {
                continue;
            }
            seen.push(id);
            let content =
                Scene3DData::content_bounds(&spec.meshes, &spec.points, &spec.lines);

            let entry = self.cameras.cameras.entry(id.to_string()).or_insert_with(|| {
                // First sight: start *at* the framed/focused pose (no
                // animation from nowhere on frame one).
                let base = spec.camera.unwrap_or_default();
                let init = match spec.focus {
                    Some(f) => base.focused(f),
                    None => base.fitted(content),
                };
                KeyedCamera {
                    current: init,
                    goal: init,
                    vel: [0.0; 6],
                    last_bounds: content,
                    last_focus: spec.focus,
                    last_step: now,
                }
            });

            // Retarget the goal.
            if spec.focus != entry.last_focus {
                // App refocused: animate to the request.
                if let Some(f) = spec.focus {
                    entry.goal = entry.current.focused(f);
                }
                entry.last_focus = spec.focus;
            } else if spec.framing == Framing::Fit {
                // Always frame the data (animates whenever it changes).
                entry.goal = entry.goal.fitted(content);
            } else if bounds_changed(entry.last_bounds, content) {
                // Auto: data re-centred — glide the look-at point, keeping
                // the user's distance and orbit angles.
                entry.goal.target = sphere_center(content);
                entry.last_bounds = content;
            }

            if !entry.step(now) {
                animating = true;
            }
        }

        self.cameras.cameras.retain(|k, _| seen.contains(&k.as_str()));
        animating
    }
}

/// Recursively gather `(computed_id, spec)` for every `Scene3D` node.
fn collect_scene_nodes<'a>(n: &'a El, out: &mut Vec<(&'a str, &'a crate::scene::SceneSpec)>) {
    if let Some(spec) = &n.scene_source {
        out.push((n.computed_id.as_str(), spec));
    }
    for child in &n.children {
        collect_scene_nodes(child, out);
    }
}

fn sphere_center(b: Aabb) -> Vec3 {
    if b.is_valid() { b.center() } else { Vec3::ZERO }
}

fn bounds_changed(a: Aabb, b: Aabb) -> bool {
    match (a.is_valid(), b.is_valid()) {
        (false, false) => false,
        (true, true) => {
            (a.center() - b.center()).length() > BOUNDS_EPSILON
                || (a.bounding_radius() - b.bounding_radius()).abs() > BOUNDS_EPSILON
        }
        _ => true,
    }
}

/// Smallest signed angle (radians) congruent to `delta`, in `(-π, π]`.
fn shortest_angle(delta: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let d = delta.rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

/// One scalar mass-spring-damper step. Mirrors `anim::Animation::step_spring`
/// (semi-implicit Euler, substepped under `MAX_SUBSTEP` for stability);
/// snaps to target + zero velocity once within the settle thresholds.
fn spring1(cur: f32, vel: f32, target: f32, cfg: SpringConfig, dt: f32) -> (f32, f32, bool) {
    let n = (dt / MAX_SUBSTEP).ceil().max(1.0) as usize;
    let h = dt / n as f32;
    let (mut c, mut v) = (cur, vel);
    for _ in 0..n {
        let disp = c - target;
        let force = -cfg.stiffness * disp - cfg.damping * v;
        v += (force / cfg.mass) * h;
        c += v * h;
    }
    if (c - target).abs() <= EPS_DISP && v.abs() <= EPS_VEL {
        (target, 0.0, true)
    } else {
        (c, v, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn pose(target: Vec3, distance: f32) -> CameraState {
        CameraState { target, distance, yaw: 0.5, pitch: 0.3 }
    }

    fn keyed(current: CameraState, goal: CameraState, now: Instant) -> KeyedCamera {
        KeyedCamera {
            current,
            goal,
            vel: [0.0; 6],
            last_bounds: Aabb::EMPTY,
            last_focus: None,
            last_step: now,
        }
    }

    #[test]
    fn spring_glides_then_settles() {
        let start = Instant::now();
        let mut cam = keyed(pose(Vec3::ZERO, 5.0), pose(Vec3::new(4.0, 0.0, 0.0), 5.0), start);
        let mut t = start;
        // One step in: partway, not snapped.
        t += Duration::from_millis(16);
        cam.step(t);
        let mid = cam.current.target.x;
        assert!(mid > 0.0 && mid < 4.0, "should be gliding, got x={mid}");

        // Run to settle.
        let mut settled = false;
        for _ in 0..600 {
            t += Duration::from_millis(16);
            if cam.step(t) {
                settled = true;
                break;
            }
        }
        assert!(settled, "spring never settled");
        assert!((cam.current.target.x - 4.0).abs() < 1e-2, "x={}", cam.current.target.x);
    }

    #[test]
    fn log_distance_interpolates_geometrically() {
        // Halfway (in settle time) between distance 1 and 100 should be
        // near the geometric mean (10), not the arithmetic mean (50.5).
        let start = Instant::now();
        let mut cam = keyed(pose(Vec3::ZERO, 1.0), pose(Vec3::ZERO, 100.0), start);
        let mut t = start;
        // Step until distance crosses the geometric mean, capture the
        // arithmetic position at that moment.
        let mut crossed_at_arith = None;
        for _ in 0..600 {
            t += Duration::from_millis(16);
            let settled = cam.step(t);
            if crossed_at_arith.is_none() && cam.current.distance >= 10.0 {
                crossed_at_arith = Some(cam.current.distance);
            }
            if settled {
                break;
            }
        }
        assert!((cam.current.distance - 100.0).abs() < 0.5, "settled at {}", cam.current.distance);
        // When it first reaches the geometric mean it's still well below
        // the arithmetic mean — proof the interpolation is in log space.
        let at = crossed_at_arith.expect("never reached 10");
        assert!(at < 50.0, "log interp should pass 10 long before 50, got {at}");
    }

    #[test]
    fn shortest_angle_takes_the_short_way() {
        use std::f32::consts::PI;
        // +350° is really −10°.
        let d = shortest_angle(350.0_f32.to_radians());
        assert!((d - (-10.0_f32).to_radians()).abs() < 1e-4, "got {d}");
        assert!(shortest_angle(0.5).abs() <= PI);
    }

    #[test]
    fn auto_recenter_animates_on_data_change() {
        use crate::scene::{PointData, PointsHandle, SceneSpec, ScenePoint};
        use crate::tree::chart3d;

        let points = |c: f32| PointData {
            points: vec![
                ScenePoint { position: Vec3::splat(c - 1.0), color: [1.0; 4] },
                ScenePoint { position: Vec3::splat(c + 1.0), color: [1.0; 4] },
            ],
        };
        let handle = PointsHandle::new(points(0.0)); // centred at origin
        let mut tree = chart3d(SceneSpec::new().points(handle.clone())); // Auto default
        crate::layout::assign_ids(&mut tree);
        let id = tree.computed_id.clone();

        let mut ui = UiState::new();
        let start = Instant::now();
        ui.tick_scene_cameras(&tree, start);
        let initial = ui.scene_camera(&id).expect("camera created").target;
        assert!(initial.length() < 1e-3, "starts framed on origin, got {initial:?}");

        // Data jumps to centre (10,10,10) — the same tree references the
        // handle, so content bounds move under Auto framing.
        handle.set(points(10.0));
        let t1 = start + Duration::from_millis(16);
        ui.tick_scene_cameras(&tree, t1);
        let mid = ui.scene_camera(&id).unwrap().target;
        assert!(mid.length() > 0.05, "target began gliding, got {mid:?}");
        assert!(mid.x < 9.0, "must animate, not snap, got {mid:?}");

        // Settle.
        let mut t = t1;
        for _ in 0..800 {
            t += Duration::from_millis(16);
            ui.tick_scene_cameras(&tree, t);
        }
        let end = ui.scene_camera(&id).unwrap().target;
        assert!(
            (end - Vec3::splat(10.0)).length() < 0.05,
            "settled on the new centre, got {end:?}"
        );
    }
}
