// liquid_glass — backdrop-sampling custom shader.
//
// The acceptance test for the backdrop contract in `docs/SHADER_VISION.md`:
// a single .wgsl, registered by a user crate via
// `Runner::register_shader_with(name, wgsl, samples_backdrop=true)`,
// expressing a glass surface that refracts + blurs + tints whatever was
// painted under it in Pass A.
//
// Reads:
//   vec_a (location 2) — tint color (rgba 0..1). The alpha channel is
//                         tint strength (0 = no tint, 1 = full tint at
//                         the per-shader cap below).
//   vec_b (location 3) — params: x = blur_radius_px,
//                                y = refraction_strength (unitless),
//                                z = specular_strength,
//                                w = reserved.
//   vec_c (location 4) — shape:  x = corner_radius_px, yzw reserved.
//
// Bound at `@group(1)`:
//   binding=0 — backdrop_tex (the snapshot of Pass A in target-format).
//   binding=1 — backdrop_smp (filtering linear sampler).
//
// Uses `time` from FrameUniforms for the subtle specular shimmer.

struct FrameUniforms {
    viewport: vec2<f32>,
    time:     f32,
    _pad:     f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

@group(1) @binding(0) var backdrop_tex: texture_2d<f32>;
@group(1) @binding(1) var backdrop_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:  vec4<f32>,  // xy = top-left px, zw = size px
    @location(2) vec_a: vec4<f32>,  // tint color rgba
    @location(3) vec_b: vec4<f32>,  // (blur_px, refraction, specular, _)
    @location(4) vec_c: vec4<f32>,  // (corner_radius_px, _, _, _)
};

struct VertexOutput {
    @builtin(position) clip_pos:  vec4<f32>,
    // sample-rate shading on the SDF input — see rounded_rect.wgsl
    @location(0) @interpolate(perspective, sample) local_px:  vec2<f32>,
    @location(1)       half_size: vec2<f32>,
    @location(2)       tint:      vec4<f32>,
    @location(3)       params:    vec4<f32>,
    @location(4)       shape:     vec4<f32>,
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
    out.clip_pos  = clip;
    out.local_px  = (in.corner_uv - vec2<f32>(0.5, 0.5)) * inst.rect.zw;
    out.half_size = inst.rect.zw * 0.5;
    out.tint      = inst.vec_a;
    out.params    = inst.vec_b;
    out.shape     = inst.vec_c;
    return out;
}

fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Map this fragment's window-space position into the backdrop UV.
    // The snapshot is the same physical extent as the target, so the
    // ratio is fragment / snapshot_size.
    let snap_size = vec2<f32>(textureDimensions(backdrop_tex));
    let frag_uv = in.clip_pos.xy / snap_size;

    let blur_px       = max(in.params.x, 0.5);
    let refraction    = in.params.y;
    let specular_amt  = in.params.z;
    let corner_radius = in.shape.x;

    // Refraction: warp the lookup based on local position normalized
    // by half_size — strongest at edges so the glass "lenses" the
    // backdrop near its rim.
    let edge = in.local_px / in.half_size;
    let warp = edge * (refraction * 16.0) / snap_size;
    let base_uv = frag_uv + warp;

    // 9-tap separated-sample blur with Gaussian-ish weights. Single
    // pass keeps the demo simple; production glass would do two
    // passes (H + V) for cheaper wide blurs.
    let texel = vec2<f32>(1.0) / snap_size;
    let r = blur_px;
    let w_corner: f32 = 1.0;
    let w_edge:   f32 = 2.0;
    let w_center: f32 = 4.0;
    let w_sum:    f32 = w_center + 4.0 * w_edge + 4.0 * w_corner;

    var color = vec4<f32>(0.0);
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>(-r, -r) * texel) * w_corner;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>( 0.0, -r) * texel) * w_edge;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>( r, -r) * texel) * w_corner;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>(-r,  0.0) * texel) * w_edge;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv) * w_center;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>( r,  0.0) * texel) * w_edge;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>(-r,  r) * texel) * w_corner;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>( 0.0,  r) * texel) * w_edge;
    color += textureSample(backdrop_tex, backdrop_smp, base_uv + vec2<f32>( r,  r) * texel) * w_corner;
    color = color / w_sum;

    // Tint: blend toward the configured color, scaled by the alpha
    // channel of the tint vec4.
    let tint_strength = in.tint.a;
    let rgb = mix(color.rgb, in.tint.rgb, tint_strength * 0.4);

    // Specular bevel near the top edge — fades out by ~20% of the
    // glass height. `time` adds a barely-visible shimmer so the
    // animation hookup is exercised.
    let yt = (in.local_px.y + in.half_size.y) / (in.half_size.y * 2.0);
    let spec_band = smoothstep(0.02, 0.08, yt) * (1.0 - smoothstep(0.08, 0.20, yt));
    let shimmer = 0.5 + 0.5 * sin(frame.time * 1.5 + in.local_px.x * 0.05);
    let spec = spec_band * (0.7 + 0.3 * shimmer) * specular_amt;
    let final_rgb = rgb + vec3<f32>(0.32) * spec;

    // SDF mask for the rounded rect.
    let cr = min(corner_radius, min(in.half_size.x, in.half_size.y));
    let d = sdf_rounded_box(in.local_px, in.half_size, cr);
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);
    let inside = 1.0 - smoothstep(-aa, 0.0, d);

    return vec4<f32>(final_rgb, inside);
}
