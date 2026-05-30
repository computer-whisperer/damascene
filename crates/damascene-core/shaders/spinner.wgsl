// stock::spinner — indeterminate loading spinner.
//
// Egui-shaped motion: a `start` anchor rotates continuously around the
// circle while an `end` anchor swings ±max_sweep around it via a
// cosine envelope, so the visible arc grows, shrinks, reverses, and
// rotates all at once. The "off" region of the ring is fully
// transparent unless the caller asked for a track via vec_b.a > 0.
//
// `cos(time*pulse_rate)` (rather than `sin`) keeps the t=0 frame at
// max sweep, so Settled-mode fixtures (RunnerCore pins time → 0 in
// AnimationMode::Settled) capture a representative spinner frame
// instead of a zero-width sliver.
//
// Slot conventions:
//   slot_a (`vec_a`)  — arc color rgba
//   slot_b (`vec_b`)  — track color rgba; alpha 0 ⇒ no track ring
//   slot_c (`vec_c`)  — (thickness_px, max_sweep_rad, head_rad_per_sec, pulse_rad_per_sec)
//                       any field at 0 falls back to the egui default.
//   slot_d unused.

struct FrameUniforms {
    viewport:     vec2<f32>,
    time:         f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:        vec4<f32>,
    @location(2) arc_color:   vec4<f32>,
    @location(3) track_color: vec4<f32>,
    @location(4) params:      vec4<f32>,  // x=thickness_px, y=max_sweep_rad, z=head_rad_per_sec, w=pulse_rad_per_sec
    @location(5) inner_rect:  vec4<f32>,
    @location(6) _slot_d:     vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) @interpolate(perspective, sample) pos_px: vec2<f32>,
    @location(1) inner_center: vec2<f32>,
    @location(2) inner_half_size: vec2<f32>,
    @location(3) arc_color: vec4<f32>,
    @location(4) track_color: vec4<f32>,
    @location(5) params: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let pos_px = in.corner_uv * inst.rect.zw + inst.rect.xy;
    let clip = vec4<f32>(
        pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_px.y / frame.viewport.y * 2.0,
        0.0,
        1.0,
    );

    var out: VertexOutput;
    out.clip_pos = clip;
    out.pos_px = pos_px;
    out.inner_center = inst.inner_rect.xy + inst.inner_rect.zw * 0.5;
    out.inner_half_size = inst.inner_rect.zw * 0.5;
    out.arc_color = inst.arc_color;
    out.track_color = inst.track_color;
    out.params = inst.params;
    return out;
}

const TAU: f32 = 6.28318530718;
const PI: f32 = 3.14159265358979;

// Defaults match egui's spinner so the visual rhythm reads as familiar
// loader to anyone who's seen one before.
const DEFAULT_MAX_SWEEP: f32 = 4.18879;        // 240°
const DEFAULT_HEAD_RAD_PER_SEC: f32 = 4.18879; // 2/3 of TAU per second
const DEFAULT_PULSE_RAD_PER_SEC: f32 = 1.0;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let diameter = min(in.inner_half_size.x, in.inner_half_size.y) * 2.0;
    let outer_r = diameter * 0.5;

    let thickness = select(
        max(diameter * 0.12, 1.5),
        in.params.x,
        in.params.x > 0.0,
    );
    let half_thickness = thickness * 0.5;
    // Centerline of the ring; the SDF treats the arc as a capsule
    // (tube of radius `half_thickness`) along this circular curve, so
    // the ends round naturally instead of cutting sharply at the
    // angular boundary.
    let center_r = max(outer_r - half_thickness, 0.0);

    let local = in.pos_px - in.inner_center;
    let dist = length(local);
    let theta = atan2(local.x, -local.y);

    let max_sweep = select(DEFAULT_MAX_SWEEP, in.params.y, in.params.y > 0.0);
    let head_rate = select(DEFAULT_HEAD_RAD_PER_SEC, in.params.z, in.params.z > 0.0);
    let pulse_rate = select(DEFAULT_PULSE_RAD_PER_SEC, in.params.w, in.params.w > 0.0);

    // Start anchor rotates monotonically; end anchor swings around it
    // via a cosine envelope. Cosine (not sine) so t=0 lands at max
    // sweep — both for a punchy boot frame in Live mode and for a
    // representative still in Settled-mode fixtures.
    let start_angle = frame.time * head_rate;
    let signed_span = max_sweep * cos(frame.time * pulse_rate);
    let end_angle = start_angle + signed_span;
    let midpoint = (start_angle + end_angle) * 0.5;
    let half_extent = abs(signed_span) * 0.5;

    // Shortest signed angle from midpoint to this fragment's theta,
    // wrapped into [-PI, PI] without a trig call.
    let raw = theta - midpoint;
    let wrapped = raw - TAU * floor((raw + PI) / TAU);
    let abs_delta = abs(wrapped);

    // Capsule SDF for the swept arc. Inside the angular band, the
    // closest centerline point is on the ring at the same angle as
    // the fragment. Past either end, the closest centerline point is
    // the nearer endpoint — `length(local - cap_pos)` then becomes
    // the distance to that endpoint, and subtracting half_thickness
    // gives the rounded cap.
    var arc_sdf: f32;
    if (abs_delta <= half_extent) {
        arc_sdf = abs(dist - center_r) - half_thickness;
    } else {
        let cap_angle = midpoint + sign(wrapped) * half_extent;
        let cap_pos = vec2<f32>(center_r * sin(cap_angle), -center_r * cos(cap_angle));
        arc_sdf = length(local - cap_pos) - half_thickness;
    }

    let aa = max(fwidth(arc_sdf), 0.5);
    let arc_alpha = 1.0 - smoothstep(0.0, aa, arc_sdf);

    // Track: full ring at the same centerline radius, same thickness.
    // Caps wrap around naturally because there's no angular gap.
    let track_sdf = abs(dist - center_r) - half_thickness;
    let track_alpha = 1.0 - smoothstep(0.0, aa, track_sdf);

    // "src over dst" composite with arc as the foreground layer.
    // Doing it in non-premultiplied space keeps the framebuffer blend
    // (which expects straight alpha) honest when the track is fully
    // transparent — `mix(track.rgb, arc.rgb, arc_alpha)` would otherwise
    // pull arc edges toward black.
    let track_a = in.track_color.a * track_alpha;
    let arc_a = in.arc_color.a * arc_alpha;
    let out_a = arc_a + track_a * (1.0 - arc_a);
    if (out_a <= 1e-4) {
        return vec4<f32>(0.0);
    }
    let out_rgb = (in.arc_color.rgb * arc_a + in.track_color.rgb * track_a * (1.0 - arc_a)) / out_a;
    return vec4<f32>(out_rgb, out_a);
}
