// stock::vector_glass — glossy local-coordinate vector icon material.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) pos_px: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) data: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
    @location(2) data: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let clip = vec4<f32>(
        in.pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - in.pos_px.y / frame.viewport.y * 2.0,
        0.0,
        1.0,
    );

    var out: VertexOutput;
    out.clip_pos = clip;
    out.color = in.color;
    out.local = in.local;
    out.data = in.data;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(in.local / vec2<f32>(24.0, 24.0), vec2<f32>(0.0), vec2<f32>(1.0));
    let diagonal = clamp((uv.x + (1.0 - uv.y)) * 0.5, 0.0, 1.0);
    let top = 1.0 - smoothstep(0.08, 0.72, uv.y);
    let bottom = smoothstep(0.35, 1.0, uv.y);
    let edge = max(abs(uv.x - 0.5), abs(uv.y - 0.5)) * 2.0;
    let glint = smoothstep(0.48, 0.82, diagonal) * (1.0 - smoothstep(0.82, 1.0, diagonal));
    let stroke_boost = select(0.08, 0.16, in.data.y > 0.5);

    var rgb = in.color.rgb;
    rgb = mix(rgb, vec3<f32>(1.0), 0.30 * top + 0.20 * glint + stroke_boost);
    rgb = rgb * (1.0 - 0.22 * bottom);
    rgb = rgb + vec3<f32>(0.06, 0.08, 0.10) * smoothstep(0.62, 1.0, edge);
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let alpha = in.color.a * (0.82 + 0.18 * top);
    return vec4<f32>(rgb * alpha, alpha);
}
