// liquid_glass_lab — polished backdrop-sampling glass surface.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

@group(1) @binding(0) var backdrop_tex: texture_2d<f32>;
@group(1) @binding(1) var backdrop_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect: vec4<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) shape: vec4<f32>,
    @location(5) inner_rect: vec4<f32>,
    @location(6) accent: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) pos_px: vec2<f32>,
    // sample-rate shading on the SDF input — see rounded_rect.wgsl
    @location(1) @interpolate(perspective, sample) local_px: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) params: vec4<f32>,
    @location(5) shape: vec4<f32>,
    @location(6) accent: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let pos_px = in.corner_uv * inst.rect.zw + inst.rect.xy;
    let center = inst.inner_rect.xy + inst.inner_rect.zw * 0.5;
    let clip = vec4<f32>(
        pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_px.y / frame.viewport.y * 2.0,
        0.0,
        1.0,
    );

    var out: VertexOutput;
    out.clip_pos = clip;
    out.pos_px = pos_px;
    out.local_px = pos_px - center;
    out.half_size = inst.inner_rect.zw * 0.5;
    out.tint = inst.tint;
    out.params = inst.params;
    out.shape = inst.shape;
    out.accent = inst.accent;
    return out;
}

fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

fn sample_glass(uv: vec2<f32>, warp: vec2<f32>, texel: vec2<f32>, radius: f32, dispersion: f32) -> vec3<f32> {
    let uv_r = uv + warp * (1.0 + dispersion);
    let uv_g = uv + warp;
    let uv_b = uv + warp * (1.0 - dispersion);

    var r = textureSample(backdrop_tex, backdrop_smp, uv_r).r * 0.18;
    var g = textureSample(backdrop_tex, backdrop_smp, uv_g).g * 0.18;
    var b = textureSample(backdrop_tex, backdrop_smp, uv_b).b * 0.18;

    let offsets = array<vec2<f32>, 12>(
        vec2<f32>( 1.0,  0.0), vec2<f32>(-1.0,  0.0),
        vec2<f32>( 0.0,  1.0), vec2<f32>( 0.0, -1.0),
        vec2<f32>( 0.72,  0.72), vec2<f32>(-0.72,  0.72),
        vec2<f32>( 0.72, -0.72), vec2<f32>(-0.72, -0.72),
        vec2<f32>( 1.85,  0.32), vec2<f32>(-1.85, -0.32),
        vec2<f32>( 0.32,  1.85), vec2<f32>(-0.32, -1.85),
    );
    let weights = array<f32, 12>(
        0.080, 0.080, 0.080, 0.080,
        0.058, 0.058, 0.058, 0.058,
        0.034, 0.034, 0.034, 0.034,
    );

    for (var i = 0u; i < 12u; i = i + 1u) {
        let o = offsets[i] * radius * texel;
        r += textureSample(backdrop_tex, backdrop_smp, uv_r + o).r * weights[i];
        g += textureSample(backdrop_tex, backdrop_smp, uv_g + o).g * weights[i];
        b += textureSample(backdrop_tex, backdrop_smp, uv_b + o).b * weights[i];
    }
    return vec3<f32>(r, g, b) / 0.876;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let snap_size = vec2<f32>(textureDimensions(backdrop_tex));
    let texel = vec2<f32>(1.0) / snap_size;
    let radius = min(in.shape.x, min(in.half_size.x, in.half_size.y));
    let blur_px = max(in.params.x, 0.5);
    let refract = in.params.y;
    let specular = in.params.z;
    let opacity = clamp(in.params.w, 0.0, 1.0);
    let rim_strength = in.shape.y;
    let frost = clamp(in.shape.z, 0.0, 1.0);

    let d = sdf_rounded_box(in.local_px, in.half_size, radius);
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);
    let inside = 1.0 - smoothstep(-aa, 0.0, d);
    let rim = (1.0 - smoothstep(0.0, 16.0, abs(d))) * rim_strength;
    let outer_rim = 1.0 - smoothstep(-3.0, 18.0, d);

    let normalized = in.local_px / max(in.half_size, vec2<f32>(1.0));
    let normal = normalize(vec2<f32>(
        normalized.x * (0.72 + 0.22 * abs(normalized.y)),
        normalized.y * 1.08,
    ));
    let ripple = vec2<f32>(
        sin(in.local_px.y * 0.035 + frame.time * 0.55),
        cos(in.local_px.x * 0.028 - frame.time * 0.45),
    ) * 0.18;
    let warp_px = (normal + ripple) * (refract * (5.0 + 28.0 * rim));
    let warp = warp_px / frame.viewport;
    let base_uv = in.pos_px / frame.viewport;
    var rgb = sample_glass(base_uv, warp, texel, blur_px, 0.36 * rim + 0.05);

    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(rgb, vec3<f32>(luma), frost * 0.18);
    rgb = mix(rgb, in.tint.rgb, in.tint.a * 0.24);

    let uv = clamp(normalized * 0.5 + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(1.0));
    let top = 1.0 - smoothstep(0.03, 0.42, uv.y);
    let bottom = smoothstep(0.50, 1.0, uv.y);
    let diagonal = smoothstep(0.18, 0.64, uv.x + (1.0 - uv.y) * 0.65)
        * (1.0 - smoothstep(0.64, 1.16, uv.x + (1.0 - uv.y) * 0.65));
    let hairline = smoothstep(0.0, 0.58, rim) * (1.0 - smoothstep(0.58, 1.0, rim));
    let caustic = pow(max(0.0, sin((uv.x * 2.1 + uv.y * 1.4) * 8.0 + rim * 2.2)), 8.0);
    let accent_glow = in.accent.rgb * in.accent.a * (0.14 * top + 0.22 * diagonal + 0.24 * hairline + 0.10 * caustic);
    let white_glint = vec3<f32>(1.0) * specular * (0.24 * top + 0.18 * diagonal + 0.28 * hairline + 0.08 * caustic);
    let inner_shadow = vec3<f32>(0.018, 0.024, 0.034) * (0.20 * bottom + 0.24 * outer_rim);
    rgb = clamp(rgb + accent_glow + white_glint - inner_shadow, vec3<f32>(0.0), vec3<f32>(1.0));

    return vec4<f32>(rgb, inside * opacity);
}
