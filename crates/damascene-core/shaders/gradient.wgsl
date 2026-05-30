// Custom shader: vertical linear gradient with rounded corners.
//
// Demonstrates the custom-shader escape hatch — same vertex envelope as
// stock::rounded_rect (corner_uv + per-instance rect + 3 generic vec4s),
// different fragment shading. Reads:
//
//   vec_a (location 2) — top color (rgba 0..1)
//   vec_b (location 3) — bottom color (rgba 0..1)
//   vec_c (location 4) — params: x = corner radius (px), yzw reserved
//
// Demonstrates the shader model described in docs/SHADER_VISION.md. Not
// "stock" — it lives outside the shipped inventory and is registered by the
// host crate through a backend runner.

struct FrameUniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:  vec4<f32>,  // xy = top-left px, zw = size px
    @location(2) vec_a: vec4<f32>,  // top color rgba 0..1
    @location(3) vec_b: vec4<f32>,  // bottom color rgba 0..1
    @location(4) vec_c: vec4<f32>,  // params: x = radius
};

struct VertexOutput {
    @builtin(position) clip_pos:  vec4<f32>,
    // `@interpolate(perspective, sample)` triggers sample-rate shading
    // when the pipeline runs at sample_count > 1; see rounded_rect.wgsl.
    @location(0) @interpolate(perspective, sample) local_px:  vec2<f32>,
    @location(1)       half_size: vec2<f32>,
    @location(2)       top_color: vec4<f32>,
    @location(3)       bot_color: vec4<f32>,
    @location(4)       radius:    f32,
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
    out.local_px = (in.corner_uv - vec2<f32>(0.5, 0.5)) * inst.rect.zw;
    out.half_size = inst.rect.zw * 0.5;
    out.top_color = inst.vec_a;
    out.bot_color = inst.vec_b;
    out.radius = inst.vec_c.x;
    return out;
}

fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = min(in.radius, min(in.half_size.x, in.half_size.y));
    let d = sdf_rounded_box(in.local_px, in.half_size, r);
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);
    let inside = 1.0 - smoothstep(-aa, 0.0, d);

    // Vertical gradient: t = 0 at the top edge, t = 1 at the bottom.
    let t = clamp(
        (in.local_px.y + in.half_size.y) / (in.half_size.y * 2.0),
        0.0,
        1.0,
    );
    let color = mix(in.top_color, in.bot_color, t);

    return vec4<f32>(color.rgb, color.a * inside);
}
