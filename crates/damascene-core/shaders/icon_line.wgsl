// stock::icon_line — antialiased vector-icon stroke segment.
//
// CPU-side icon paths are flattened into line-segment instances. The
// shader owns stroke coverage, which gives theme shaders a clean place
// to vary joins, softness, embossing, or retro treatments later without
// changing widget code.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect: vec4<f32>,   // xy = top-left logical px, zw = size logical px
    @location(2) line: vec4<f32>,   // x0, y0, x1, y1 in logical px
    @location(3) color: vec4<f32>,  // linear rgba
    @location(4) params: vec4<f32>, // x = stroke width
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    // sample-rate shading on the SDF input — see rounded_rect.wgsl
    @location(0) @interpolate(perspective, sample) pos_px: vec2<f32>,
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) stroke_width: f32,
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
    out.p0 = inst.line.xy;
    out.p1 = inst.line.zw;
    out.color = inst.color;
    out.stroke_width = inst.params.x;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pa = in.pos_px - in.p0;
    let ba = in.p1 - in.p0;
    let denom = max(dot(ba, ba), 0.0001);
    let h = clamp(dot(pa, ba) / denom, 0.0, 1.0);
    let d = length(pa - ba * h);
    let half_width = in.stroke_width * 0.5;
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.75);
    let coverage = 1.0 - smoothstep(half_width - aa, half_width + aa, d);
    let alpha = coverage * in.color.a;
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
