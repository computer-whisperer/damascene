// liquid_backdrop_lab — rich Pass-A source material for glass sampling.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect: vec4<f32>,
    @location(2) base: vec4<f32>,
    @location(3) accent_a: vec4<f32>,
    @location(4) accent_b: vec4<f32>,
    @location(5) inner_rect: vec4<f32>,
    @location(6) accent_c: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) pos_px: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) base: vec4<f32>,
    @location(3) accent_a: vec4<f32>,
    @location(4) accent_b: vec4<f32>,
    @location(5) accent_c: vec4<f32>,
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
    out.local_uv = in.corner_uv;
    out.base = inst.base;
    out.accent_a = inst.accent_a;
    out.accent_b = inst.accent_b;
    out.accent_c = inst.accent_c;
    return out;
}

fn soft_blob(uv: vec2<f32>, center: vec2<f32>, radius: vec2<f32>) -> f32 {
    let p = (uv - center) / radius;
    return exp(-dot(p, p) * 1.65);
}

fn line_field(uv: vec2<f32>) -> f32 {
    let wave = sin((uv.x * 7.0 + uv.y * 2.2) * 3.14159);
    let sweep = sin((uv.x - uv.y * 0.44) * 18.0 + frame.time * 0.12);
    return smoothstep(0.84, 1.0, wave * 0.56 + sweep * 0.22 + 0.34);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.pos_px / frame.viewport;
    var rgb = in.base.rgb;

    let a = soft_blob(uv, vec2<f32>(0.18, 0.22), vec2<f32>(0.34, 0.22));
    let b = soft_blob(uv, vec2<f32>(0.66, 0.30), vec2<f32>(0.42, 0.28));
    let c = soft_blob(uv, vec2<f32>(0.78, 0.78), vec2<f32>(0.34, 0.24));
    let d = soft_blob(uv, vec2<f32>(0.33, 0.82), vec2<f32>(0.26, 0.18));
    rgb = mix(rgb, in.accent_a.rgb, a * in.accent_a.a);
    rgb = mix(rgb, in.accent_b.rgb, b * in.accent_b.a);
    rgb = mix(rgb, in.accent_c.rgb, c * in.accent_c.a);
    rgb = mix(rgb, vec3<f32>(0.93, 0.50, 0.25), d * 0.36);

    let stripe = smoothstep(0.46, 0.50, fract(uv.x * 4.0)) * (1.0 - smoothstep(0.50, 0.54, fract(uv.x * 4.0)));
    let grid_x = 1.0 - smoothstep(0.0, 0.012, abs(fract(uv.x * 18.0) - 0.5));
    let grid_y = 1.0 - smoothstep(0.0, 0.012, abs(fract(uv.y * 12.0) - 0.5));
    let grid = (grid_x + grid_y) * 0.5;
    let lines = line_field(uv);

    rgb += vec3<f32>(0.10, 0.18, 0.22) * stripe;
    rgb += vec3<f32>(0.28, 0.42, 0.55) * grid * 0.10;
    rgb += vec3<f32>(0.18, 0.34, 0.42) * lines * 0.26;

    let vignette = smoothstep(0.88, 0.18, distance(uv, vec2<f32>(0.52, 0.48)));
    rgb *= 0.70 + 0.34 * vignette;
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, 1.0);
}
