// stock::progress_indeterminate — indeterminate linear progress.
//
// A track filled with `slot_b` (typically MUTED) plus a small bar of
// `slot_a` that slides left-to-right across it on loop. For
// determinate progress, use stock::rounded_rect via the
// widgets/progress.rs builder — that one is value-driven, not
// time-driven.
//
// The bar's center traverses from `-bar_w/2` (just off-left) to
// `1 + bar_w/2` (just off-right) over `period` seconds, then jumps
// back. A half-period bias on the phase puts the bar at the middle
// of the track at t=0 so Settled-mode fixtures show "loading in
// progress" rather than "loader hasn't started yet."
//
// Slot conventions:
//   slot_a (`vec_a`)  — bar color rgba
//   slot_b (`vec_b`)  — track color rgba
//   slot_c (`vec_c`)  — (radius_px, period_seconds, bar_width_fraction, _)
//                       any field at 0 falls back to its default.
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
    @location(2) bar_color:   vec4<f32>,
    @location(3) track_color: vec4<f32>,
    @location(4) params:      vec4<f32>,
    @location(5) inner_rect:  vec4<f32>,
    @location(6) _slot_d:     vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) @interpolate(perspective, sample) pos_px: vec2<f32>,
    @location(1) inner_center: vec2<f32>,
    @location(2) inner_half_size: vec2<f32>,
    @location(3) bar_color: vec4<f32>,
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
    out.bar_color = inst.bar_color;
    out.track_color = inst.track_color;
    out.params = inst.params;
    return out;
}

const DEFAULT_RADIUS_PX: f32 = 4.0;
const DEFAULT_PERIOD_SEC: f32 = 1.6;
const DEFAULT_BAR_WIDTH: f32 = 0.35;

fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius_param = select(DEFAULT_RADIUS_PX, in.params.x, in.params.x > 0.0);
    let radius = min(radius_param, min(in.inner_half_size.x, in.inner_half_size.y));
    let period = select(DEFAULT_PERIOD_SEC, in.params.y, in.params.y > 0.0);
    let bar_w = select(DEFAULT_BAR_WIDTH, in.params.z, in.params.z > 0.0);

    let local_px = in.pos_px - in.inner_center;
    let d = sdf_rounded_box(local_px, in.inner_half_size, radius);
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);
    let inside = 1.0 - smoothstep(-aa, 0.0, d);
    if (inside <= 0.0) {
        return vec4<f32>(0.0);
    }

    // x_norm: 0 at the left edge of the track, 1 at the right edge.
    let rect_w = in.inner_half_size.x * 2.0;
    let x_norm = (local_px.x + in.inner_half_size.x) / rect_w;

    // Phase advances 0..1 each period. The +0.5 bias puts the bar's
    // center at the middle of the track at t=0 for fixture clarity.
    let raw_phase = fract(frame.time / period);
    let phase = fract(raw_phase + 0.5);
    let bar_center = mix(-bar_w * 0.5, 1.0 + bar_w * 0.5, phase);

    let dist = abs(x_norm - bar_center);
    let half_w = bar_w * 0.5;
    // Soften the bar edges over a fraction of the half-width so the
    // moving highlight reads as a smooth glow rather than a hard slab.
    let edge_soft = max(half_w * 0.4, 0.005);
    let bar_t = 1.0 - smoothstep(half_w - edge_soft, half_w, dist);

    // "src over dst" composite — bar layer over track layer. Doing it
    // in non-premultiplied space keeps the framebuffer blend honest if
    // the track is fully transparent.
    let track_a = in.track_color.a * inside;
    let bar_a = in.bar_color.a * inside * bar_t;
    let out_a = bar_a + track_a * (1.0 - bar_a);
    if (out_a <= 1e-4) {
        return vec4<f32>(0.0);
    }
    let out_rgb =
        (in.bar_color.rgb * bar_a + in.track_color.rgb * track_a * (1.0 - bar_a)) / out_a;
    return vec4<f32>(out_rgb, out_a);
}
