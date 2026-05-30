// stock::vector_relief — shader-material proof for vector icons.
//
// Uses local SVG coordinates from VectorMeshVertex to shade an icon
// consistently inside its own viewBox, independent of destination size.

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
    let top_light = 1.0 - smoothstep(0.10, 1.0, uv.y);
    let lower_shade = smoothstep(0.45, 1.0, uv.y);
    let left_glint = (1.0 - smoothstep(0.15, 0.90, uv.x)) * top_light;
    let stroke_bias = select(0.04, 0.09, in.data.y > 0.5);

    var rgb = in.color.rgb;
    rgb = rgb + vec3<f32>(0.22) * top_light + vec3<f32>(0.10) * left_glint;
    rgb = rgb * (1.0 - 0.22 * lower_shade - stroke_bias * uv.y);
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let alpha = in.color.a;
    return vec4<f32>(rgb * alpha, alpha);
}
