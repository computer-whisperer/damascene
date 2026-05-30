// stock::skeleton — pulsing loading placeholder.
//
// Rounded rect filled with `slot_a`, with a cosine alpha breathe
// matching shadcn's `animate-pulse`: alpha multiplier oscillates
// 0.5 → 1.0 → 0.5 over `period` seconds. The shader uses cosine so
// t=0 lands at the peak (alpha multiplier = 1.0), giving a
// representative "fully visible" Settled-mode fixture.
//
// Slot conventions:
//   slot_a (`vec_a`)  — base color rgba
//   slot_b unused
//   slot_c (`vec_c`)  — (radius_px, period_seconds, min_alpha_mult, max_alpha_mult)
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
    @location(2) base_color:  vec4<f32>,
    @location(3) _slot_b:     vec4<f32>,
    @location(4) params:      vec4<f32>,
    @location(5) inner_rect:  vec4<f32>,
    @location(6) _slot_d:     vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) @interpolate(perspective, sample) pos_px: vec2<f32>,
    @location(1) inner_center: vec2<f32>,
    @location(2) inner_half_size: vec2<f32>,
    @location(3) base_color: vec4<f32>,
    @location(4) params: vec4<f32>,
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
    out.base_color = inst.base_color;
    out.params = inst.params;
    return out;
}

const TAU: f32 = 6.28318530718;

const DEFAULT_RADIUS_PX: f32 = 6.0;
const DEFAULT_PERIOD_SEC: f32 = 2.0;
const DEFAULT_MIN_ALPHA: f32 = 0.5;
const DEFAULT_MAX_ALPHA: f32 = 1.0;

fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius_param = select(DEFAULT_RADIUS_PX, in.params.x, in.params.x > 0.0);
    let radius = min(radius_param, min(in.inner_half_size.x, in.inner_half_size.y));
    let period = select(DEFAULT_PERIOD_SEC, in.params.y, in.params.y > 0.0);
    let min_alpha = select(DEFAULT_MIN_ALPHA, in.params.z, in.params.z > 0.0);
    let max_alpha = select(DEFAULT_MAX_ALPHA, in.params.w, in.params.w > 0.0);

    let local_px = in.pos_px - in.inner_center;
    let d = sdf_rounded_box(local_px, in.inner_half_size, radius);
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);
    let inside = 1.0 - smoothstep(-aa, 0.0, d);
    if (inside <= 0.0) {
        return vec4<f32>(0.0);
    }

    // Cosine envelope so t=0 = max alpha (Settled-mode fixtures land
    // on the brightest frame). `pulse` ranges 0..1; we mix between
    // min_alpha and max_alpha with that.
    let phase = frame.time * (TAU / period);
    let pulse = 0.5 + 0.5 * cos(phase);
    let alpha_mult = mix(min_alpha, max_alpha, pulse);

    return vec4<f32>(in.base_color.rgb, in.base_color.a * inside * alpha_mult);
}
