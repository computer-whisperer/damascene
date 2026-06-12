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
use crate::event::{KeyModifiers, PointerButton};
use crate::scene::glam::Vec3;
use crate::scene::{Aabb, CameraControls, CameraState, Focus, Framing, Scene3DData};
use crate::tree::{El, Rect};

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

// Gesture sensitivities.
/// Orbit radians per pixel of drag (~180px ≈ 90°).
const ORBIT_RAD_PER_PX: f32 = 0.005;
/// Geometric zoom per pixel of wheel delta. `exp` keeps it symmetric and
/// scale-independent; scroll down (dy > 0) pulls the camera back.
const ZOOM_PER_PX: f32 = 0.0015;
/// Geometric zoom per pixel of a dolly *drag* (Maya Alt+right).
const ZOOM_DRAG_PER_PX: f32 = 0.005;

/// What a pointer drag over a scene viewport does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CameraDragMode {
    Orbit,
    Pan,
    /// Dolly — vertical drag zooms (Maya-style).
    Zoom,
}

/// Map a navigation scheme + pressed button + modifiers to the drag it
/// starts, or `None` when this combo isn't a camera gesture (so the press
/// falls through to normal handling — selection, focus, etc.). The wheel
/// zooms under every scheme and is handled separately.
pub(crate) fn scheme_drag_mode(
    controls: CameraControls,
    button: PointerButton,
    mods: KeyModifiers,
) -> Option<CameraDragMode> {
    use CameraDragMode::{Orbit, Pan, Zoom};
    use PointerButton::{Middle, Primary, Secondary};
    match controls {
        CameraControls::Orbit => match button {
            Primary if mods.shift => Some(Pan),
            Primary => Some(Orbit),
            Secondary => Some(Pan),
            _ => None,
        },
        CameraControls::Blender => match button {
            Middle if mods.shift => Some(Pan),
            Middle => Some(Orbit),
            _ => None,
        },
        CameraControls::OnShape => match button {
            Secondary => Some(Orbit),
            Middle => Some(Pan),
            _ => None,
        },
        // Maya gates everything behind Alt (so bare clicks stay free).
        CameraControls::Maya if mods.alt => match button {
            Primary => Some(Orbit),
            Middle => Some(Pan),
            Secondary => Some(Zoom),
        },
        CameraControls::Maya => None,
    }
}

/// An in-flight pointer drag captured over a scene viewport. Mirrors
/// `ScrollState::thumb_drag`: set at `pointer_down`, driven by
/// `pointer_moved`, cleared at `pointer_up`.
#[derive(Clone, Debug)]
pub(crate) struct CameraDrag {
    id: String,
    mode: CameraDragMode,
    last_x: f32,
    last_y: f32,
}

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
    /// The node's viewport rect (logical px), refreshed each tick — used
    /// to route pointer/wheel gestures by hit point.
    rect: Rect,
    /// Navigation scheme, refreshed each tick from the spec.
    controls: CameraControls,
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
    /// Active pointer-drag capture, if any.
    drag: Option<CameraDrag>,
    /// Viewport rects of scenes carrying hover-tooltip labels, refreshed
    /// each [`tick_scene_cameras`](UiState::tick_scene_cameras). The runtime
    /// redraws on pointer-move over these so the tooltip tracks the cursor —
    /// scenes aren't hover hit-targets, so a move *within* one wouldn't
    /// otherwise request a frame.
    hover_label_rects: Vec<Rect>,
    /// Per-scene geometry-identity fingerprint + churn counter, used to
    /// warn (once per scene) when an app constructs *fresh* geometry
    /// handles every frame instead of caching them — each fresh
    /// [`GeometryId`](crate::scene::GeometryId) forces a full GPU
    /// re-upload and a cold buffer-cache slot. Legitimate streaming via
    /// `GeometryHandle::set` keeps ids stable (only the revision moves)
    /// and never trips this.
    geom_churn: HashMap<String, GeomChurn>,
}

/// Consecutive fingerprint-changed frames before the fresh-handle
/// warning fires. High enough that mark add/remove transitions and
/// data-driven handle swaps never trip it; per-frame rebuilds hit it
/// within a quarter second.
const GEOM_CHURN_WARN_FRAMES: u32 = 10;

#[derive(Default)]
struct GeomChurn {
    /// Hash of the scene's mark geometry ids, in mark order. `0` =
    /// not yet observed.
    fingerprint: u64,
    /// Consecutive frames the fingerprint changed.
    consecutive: u32,
    warned: bool,
}

/// Order-sensitive hash of every geometry id a spec references.
fn spec_geometry_fingerprint(spec: &crate::scene::SceneSpec) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for m in &spec.meshes {
        m.geometry.id().hash(&mut h);
    }
    for p in &spec.points {
        p.geometry.id().hash(&mut h);
    }
    for l in &spec.lines {
        l.geometry.id().hash(&mut h);
    }
    let fp = h.finish();
    // Reserve 0 as "not yet observed".
    if fp == 0 { 1 } else { fp }
}

impl UiState {
    /// Resolved current pose for a keyed scene camera (`Auto` / `Fit`).
    /// `None` if the node hasn't been ticked yet or uses `Manual` framing
    /// (where the app owns the pose). Read by `draw_ops`, and exposed to
    /// apps (via [`BuildCx`]/[`EventCx`]) so they can build a screen→world
    /// ray for picking/placement against the live camera. `id` is the node's
    /// computed id (e.g. [`UiEvent::target`]'s `node_id`).
    ///
    /// [`BuildCx`]: crate::event::BuildCx
    /// [`EventCx`]: crate::event::EventCx
    /// [`UiEvent::target`]: crate::event::UiEvent::target
    pub fn scene_camera(&self, id: &str) -> Option<CameraState> {
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
        // Collect scene nodes first (immutable borrow of the tree), and
        // resolve each rect from the layout side-map now — both immutable
        // borrows that end before the mutable `self.cameras` loop below.
        let mut raw: Vec<(&str, &crate::scene::SceneSpec)> = Vec::new();
        collect_scene_nodes(root, &mut raw);
        let nodes: Vec<(&str, Rect, &crate::scene::SceneSpec)> = raw
            .into_iter()
            .map(|(id, spec)| (id, self.rect(id), spec))
            .collect();

        // Refresh the hover-tooltip rect list for *all* scenes (including
        // Manual, which the camera loop below skips) so pointer-move redraws
        // fire over any scene with hover labels.
        self.cameras.hover_label_rects.clear();

        let mut animating = false;
        let mut seen: Vec<&str> = Vec::with_capacity(nodes.len());
        self.cameras
            .geom_churn
            .retain(|id, _| nodes.iter().any(|(nid, _, _)| nid == id));
        for (id, _rect, spec) in &nodes {
            // Fresh-handle churn detection (all scenes, Manual included):
            // a fingerprint that changes many consecutive frames means
            // the app rebuilds handles per frame instead of caching them.
            let fp = spec_geometry_fingerprint(spec);
            let churn = self
                .cameras
                .geom_churn
                .entry((*id).to_string())
                .or_default();
            if churn.fingerprint == fp {
                churn.consecutive = 0;
            } else {
                if churn.fingerprint != 0 {
                    churn.consecutive += 1;
                    if churn.consecutive >= GEOM_CHURN_WARN_FRAMES && !churn.warned {
                        churn.warned = true;
                        log::warn!(
                            "damascene: scene {id:?} got fresh geometry handles {GEOM_CHURN_WARN_FRAMES} \
                             frames in a row — each fresh handle re-uploads the whole buffer and \
                             cold-starts the backend's geometry cache. Create PointsHandle/\
                             LinesHandle/MeshHandle once in app state and mutate with `set()` \
                             (the backend re-uploads only when the revision advances)."
                        );
                    }
                }
                churn.fingerprint = fp;
            }
        }
        for (id, rect, spec) in nodes {
            if spec_has_hover_labels(spec) {
                self.cameras.hover_label_rects.push(rect);
            }
            if spec.framing == Framing::Manual {
                continue;
            }
            seen.push(id);
            let content = Scene3DData::content_bounds(&spec.meshes, &spec.points, &spec.lines);

            let entry = self
                .cameras
                .cameras
                .entry(id.to_string())
                .or_insert_with(|| {
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
                        rect,
                        controls: spec.controls,
                    }
                });
            entry.rect = rect;
            entry.controls = spec.controls;

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

        self.cameras
            .cameras
            .retain(|k, _| seen.contains(&k.as_str()));
        animating
    }

    /// Whether `(x, y)` lies over a scene carrying hover-tooltip labels. The
    /// runtime ORs this into the pointer-move redraw signal so the tooltip
    /// follows the cursor (and clears on the move that leaves the scene).
    pub(crate) fn pointer_over_hover_scene(&self, x: f32, y: f32) -> bool {
        self.cameras
            .hover_label_rects
            .iter()
            .any(|r| r.contains(x, y))
    }

    /// Id of the keyed scene camera whose viewport contains `(x, y)`, if
    /// any. Used by the runtime to route pointer/wheel gestures. Topmost
    /// (last-walked) wins on the rare overlap.
    pub(crate) fn scene_at(&self, x: f32, y: f32) -> Option<String> {
        let mut found = None;
        for (id, cam) in &self.cameras.cameras {
            if cam.rect.contains(x, y) {
                found = Some(id.clone());
            }
        }
        found
    }

    /// The drag a press of `button`+`mods` starts over scene `id`, per the
    /// node's navigation scheme — or `None` if it isn't a camera gesture
    /// (so the press falls through) or `id` has no keyed camera (Manual).
    pub(crate) fn scene_drag_mode(
        &self,
        id: &str,
        button: PointerButton,
        mods: KeyModifiers,
    ) -> Option<CameraDragMode> {
        let controls = self.cameras.cameras.get(id)?.controls;
        scheme_drag_mode(controls, button, mods)
    }

    /// Begin a pointer-drag camera gesture over scene `id`.
    pub(crate) fn begin_camera_drag(&mut self, id: String, mode: CameraDragMode, x: f32, y: f32) {
        self.cameras.drag = Some(CameraDrag {
            id,
            mode,
            last_x: x,
            last_y: y,
        });
    }

    pub(crate) fn camera_drag_active(&self) -> bool {
        self.cameras.drag.is_some()
    }

    /// Apply pointer movement to the active camera drag (orbit or pan),
    /// writing both `current` and `goal` so the motion is crisp 1:1 (the
    /// spring has nothing to chase). Returns true if the camera moved.
    pub(crate) fn drag_camera_to(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = self.cameras.drag.as_mut() else {
            return false;
        };
        let dx = x - drag.last_x;
        let dy = y - drag.last_y;
        drag.last_x = x;
        drag.last_y = y;
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let (id, mode) = (drag.id.clone(), drag.mode);
        let Some(cam) = self.cameras.cameras.get_mut(&id) else {
            return false;
        };
        match mode {
            CameraDragMode::Orbit => {
                // Turntable: drag follows the model — drag right spins the
                // scene to the right, drag down tips the top toward you.
                cam.current
                    .orbit(-dx * ORBIT_RAD_PER_PX, dy * ORBIT_RAD_PER_PX);
            }
            CameraDragMode::Pan => {
                let (right, up) = camera_basis(&cam.current);
                // True 1:1 grab: one logical pixel maps to the world span
                // of one pixel at the focus (target) depth, so the grabbed
                // point tracks the cursor exactly. The view frustum is
                // `2·distance·tan(fov/2)` world units tall across the
                // viewport's height; the per-pixel scale is uniform (square
                // pixels), so the same factor applies to x.
                let half_h = (crate::scene::camera::DEFAULT_FOV_Y_RADIANS * 0.5).tan();
                let world_per_px = 2.0 * cam.current.distance * half_h / cam.rect.h.max(1.0);
                // Scene follows the cursor: move the target opposite drag.
                cam.current
                    .pan_by(right * (-dx * world_per_px) + up * (dy * world_per_px));
            }
            CameraDragMode::Zoom => {
                // Dolly: drag down pulls back, drag up moves in.
                cam.current.zoom_by((dy * ZOOM_DRAG_PER_PX).exp());
            }
        }
        // 1:1 manipulation: goal tracks current, spring idle.
        cam.goal = cam.current;
        cam.vel = [0.0; 6];
        true
    }

    /// End the active camera drag, if any. Returns whether one was active.
    pub(crate) fn end_camera_drag(&mut self) -> bool {
        self.cameras.drag.take().is_some()
    }

    /// Zoom the scene camera under `(x, y)` by a wheel delta (logical px).
    /// Retargets the *goal* distance so the spring animates the zoom.
    /// Returns true if a scene consumed the wheel.
    pub(crate) fn camera_wheel_zoom(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(id) = self.scene_at(x, y) else {
            return false;
        };
        let Some(cam) = self.cameras.cameras.get_mut(&id) else {
            return false;
        };
        if dy.abs() <= f32::EPSILON {
            // Over the scene but no delta — still consume so the wheel
            // doesn't fall through to scrolling an ancestor.
            return true;
        }
        cam.goal.zoom_by((dy * ZOOM_PER_PX).exp());
        true
    }
}

/// Camera-space right and up unit vectors for the current pose, for
/// screen-aligned panning.
fn camera_basis(pose: &CameraState) -> (Vec3, Vec3) {
    let forward = (pose.target - pose.eye()).normalize_or_zero();
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward).normalize_or_zero();
    (right, up)
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

/// Whether a scene has any point mark with hover-tooltip labels.
fn spec_has_hover_labels(spec: &crate::scene::SceneSpec) -> bool {
    spec.points.iter().any(|p| {
        p.labels
            .as_ref()
            .is_some_and(|l| l.display == crate::scene::LabelDisplay::Hover)
    })
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
        CameraState {
            target,
            distance,
            yaw: 0.5,
            pitch: 0.3,
        }
    }

    fn keyed(current: CameraState, goal: CameraState, now: Instant) -> KeyedCamera {
        KeyedCamera {
            current,
            goal,
            vel: [0.0; 6],
            last_bounds: Aabb::EMPTY,
            last_focus: None,
            last_step: now,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            controls: CameraControls::Orbit,
        }
    }

    #[test]
    fn spring_glides_then_settles() {
        let start = Instant::now();
        let mut cam = keyed(
            pose(Vec3::ZERO, 5.0),
            pose(Vec3::new(4.0, 0.0, 0.0), 5.0),
            start,
        );
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
        assert!(
            (cam.current.target.x - 4.0).abs() < 1e-2,
            "x={}",
            cam.current.target.x
        );
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
        assert!(
            (cam.current.distance - 100.0).abs() < 0.5,
            "settled at {}",
            cam.current.distance
        );
        // When it first reaches the geometric mean it's still well below
        // the arithmetic mean — proof the interpolation is in log space.
        let at = crossed_at_arith.expect("never reached 10");
        assert!(
            at < 50.0,
            "log interp should pass 10 long before 50, got {at}"
        );
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
        use crate::scene::{PointData, PointsHandle, ScenePoint, SceneSpec};
        use crate::tree::chart3d;

        let points = |c: f32| PointData {
            points: vec![
                ScenePoint {
                    position: Vec3::splat(c - 1.0),
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: Vec3::splat(c + 1.0),
                    color: [1.0; 4],
                },
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
        assert!(
            initial.length() < 1e-3,
            "starts framed on origin, got {initial:?}"
        );

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

    #[test]
    fn drag_orbits_and_wheel_zooms() {
        use crate::scene::{PointData, PointsHandle, ScenePoint, SceneSpec};
        use crate::tree::chart3d;

        let handle = PointsHandle::new(PointData {
            points: vec![
                ScenePoint {
                    position: Vec3::splat(-1.0),
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: Vec3::splat(1.0),
                    color: [1.0; 4],
                },
            ],
        });
        let mut tree = chart3d(SceneSpec::new().points(handle));
        let mut ui = UiState::new();
        // Lay out so the scene gets a real viewport rect for hit-routing.
        crate::layout::layout(&mut tree, &mut ui, Rect::new(0.0, 0.0, 200.0, 200.0));
        let id = tree.computed_id.clone();
        ui.tick_scene_cameras(&tree, Instant::now());
        assert_eq!(ui.scene_at(100.0, 100.0).as_deref(), Some(id.as_str()));
        assert!(ui.scene_at(-5.0, -5.0).is_none(), "outside the rect");

        // Orbit: drag right rotates yaw (current + goal move together).
        let yaw0 = ui.scene_camera(&id).unwrap().yaw;
        ui.begin_camera_drag(id.clone(), CameraDragMode::Orbit, 100.0, 100.0);
        assert!(ui.camera_drag_active());
        assert!(ui.drag_camera_to(140.0, 100.0));
        let yaw1 = ui.scene_camera(&id).unwrap().yaw;
        assert!(
            (yaw1 - yaw0).abs() > 0.1,
            "drag should orbit: {yaw0} -> {yaw1}"
        );
        assert!(ui.end_camera_drag());
        assert!(!ui.camera_drag_active());

        // Wheel: scroll down zooms out — retargets goal distance, which the
        // spring then animates `current` toward over subsequent ticks.
        let d0 = ui.scene_camera(&id).unwrap().distance;
        assert!(ui.camera_wheel_zoom(100.0, 100.0, 60.0));
        assert!(
            !ui.camera_wheel_zoom(-5.0, -5.0, 60.0),
            "wheel off-scene not consumed"
        );
        let mut t = Instant::now();
        for _ in 0..400 {
            t += Duration::from_millis(16);
            ui.tick_scene_cameras(&tree, t);
        }
        let d1 = ui.scene_camera(&id).unwrap().distance;
        assert!(
            d1 > d0 + 0.01,
            "wheel should zoom out (grow distance): {d0} -> {d1}"
        );
    }

    #[test]
    fn pan_tracks_cursor_one_to_one() {
        use crate::scene::{PointData, PointsHandle, ScenePoint, SceneSpec};
        use crate::tree::chart3d;

        let handle = PointsHandle::new(PointData {
            points: vec![
                ScenePoint {
                    position: Vec3::splat(-1.0),
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: Vec3::splat(1.0),
                    color: [1.0; 4],
                },
            ],
        });
        let mut tree = chart3d(SceneSpec::new().points(handle));
        let mut ui = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 300.0, 200.0);
        crate::layout::layout(&mut tree, &mut ui, viewport);
        let id = tree.computed_id.clone();
        ui.tick_scene_cameras(&tree, Instant::now());

        // A fixed world point at the focus (target) depth.
        let cam0 = ui.scene_camera(&id).unwrap();
        let world = cam0.target;
        let view = Aabb::from_points([Vec3::splat(-1.0), Vec3::splat(1.0)]);
        let s0 = cam0
            .resolve(view)
            .project_to_screen(world, viewport)
            .unwrap();

        // Drag right 60px, down 24px.
        ui.begin_camera_drag(id.clone(), CameraDragMode::Pan, 150.0, 100.0);
        ui.drag_camera_to(210.0, 124.0);
        let cam1 = ui.scene_camera(&id).unwrap();
        let s1 = cam1
            .resolve(view)
            .project_to_screen(world, viewport)
            .unwrap();

        // The grabbed point followed the cursor 1:1 (within sub-pixel).
        assert!(
            (s1.x - s0.x - 60.0).abs() < 0.5,
            "x moved {} (want 60)",
            s1.x - s0.x
        );
        assert!(
            (s1.y - s0.y - 24.0).abs() < 0.5,
            "y moved {} (want 24)",
            s1.y - s0.y
        );
    }

    #[test]
    fn nav_schemes_map_buttons() {
        use CameraDragMode::{Orbit, Pan, Zoom};
        use PointerButton::{Middle, Primary, Secondary};
        let none = KeyModifiers::default();
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        let alt = KeyModifiers {
            alt: true,
            ..Default::default()
        };
        let m = scheme_drag_mode;

        // Orbit (widget default): left orbits, Shift+left / right pan.
        assert_eq!(m(CameraControls::Orbit, Primary, none), Some(Orbit));
        assert_eq!(m(CameraControls::Orbit, Primary, shift), Some(Pan));
        assert_eq!(m(CameraControls::Orbit, Secondary, none), Some(Pan));
        assert_eq!(m(CameraControls::Orbit, Middle, none), None);

        // Blender: middle orbits; bare left is free (falls through).
        assert_eq!(m(CameraControls::Blender, Middle, none), Some(Orbit));
        assert_eq!(m(CameraControls::Blender, Middle, shift), Some(Pan));
        assert_eq!(m(CameraControls::Blender, Primary, none), None);

        // OnShape: right orbits, middle pans.
        assert_eq!(m(CameraControls::OnShape, Secondary, none), Some(Orbit));
        assert_eq!(m(CameraControls::OnShape, Middle, none), Some(Pan));

        // Maya: everything gated behind Alt; Alt+right dollies.
        assert_eq!(m(CameraControls::Maya, Primary, none), None);
        assert_eq!(m(CameraControls::Maya, Primary, alt), Some(Orbit));
        assert_eq!(m(CameraControls::Maya, Secondary, alt), Some(Zoom));
    }
}
