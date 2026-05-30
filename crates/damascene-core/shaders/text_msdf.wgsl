// stock::text_msdf — multi-channel signed-distance-field glyph rendering.
//
// One pipeline. Each per-instance entry places a glyph quad in logical
// pixel space (`rect.xy/zw`) and samples an MTSDF page in
// (`uv.xy/zw`) at any size. The atlas stores RGB = MSDF distance
// channels and A = a true single-channel SDF. The fragment shader
// reconstructs the signed distance from median(R,G,B); when median
// disagrees with A about inside/outside (a classic MSDF artifact at
// sharp corners), A wins. AA width is derived from the screen-space UV
// gradient so output is one screen pixel wide regardless of render
// scale.
//
// Coordinate system: vertex positions arrive as a unit quad with uv 0..1.
// `rect` (xy=topleft logical px, zw=size logical px) places the glyph in
// the viewport; the vertex shader maps logical px to clip space via
// `frame.viewport`. The atlas page is bound at `@group(1)`, so the
// pipeline can stay shared across pages — backends rebind group(1) when
// the active page changes between text runs.
//
// Per-instance `params.x` is the SDF spread in **atlas pixels** (the
// same value the atlas was built with). Multiplying by atlas-px-per-
// screen-px gives the per-screen-pixel signed-distance range, which the
// shader uses to scale the coverage smoothstep so AA stays one screen
// pixel wide at every render scale.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:   vec4<f32>,  // xy = top-left logical px, zw = size logical px
    @location(2) uv:     vec4<f32>,  // xy = uv top-left 0..1, zw = uv size 0..1
    @location(3) color:  vec4<f32>,  // rgba 0..1 (linear)
    @location(4) params: vec4<f32>,  // x = spread (atlas px), y = atlas_px_size_x of glyph,
                                     // z = reserved, w = reserved
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      uv: vec2<f32>,
    @location(1)      color: vec4<f32>,
    @location(2)      params: vec4<f32>,
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
    let uv = inst.uv.xy + in.corner_uv * inst.uv.zw;

    var out: VertexOutput;
    out.clip_pos = clip;
    out.uv = uv;
    out.color = inst.color;
    out.params = inst.params;
    return out;
}

fn median3(a: f32, b: f32, c: f32) -> f32 {
    return max(min(a, b), min(max(a, b), c));
}

fn coverage_at(uv: vec2<f32>, screen_px_range: f32) -> f32 {
    let mtsd = textureSample(atlas_tex, atlas_smp, uv);
    let median_sd = median3(mtsd.r, mtsd.g, mtsd.b);
    let true_sd = mtsd.a;
    // When MSDF and the true single-channel SDF disagree about whether
    // a sample is inside or outside the glyph, the MSDF is lying — the
    // per-channel coloring algorithm produces false-outside artifacts
    // near sharp corners. The true SDF is monotonic and never has this
    // problem, so it wins on disagreement (slightly rounding the corner
    // there). Where they agree, MSDF wins (sharp).
    let agree = (median_sd - 0.5) * (true_sd - 0.5) >= 0.0;
    let sd = select(true_sd, median_sd, agree) - 0.5;
    return clamp(sd * 2.0 * screen_px_range + 0.5, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Project the atlas-px distance gradient onto screen pixels so AA
    // stays one screen pixel wide regardless of render scale. The
    // standard MSDF formula:
    //   unit_range  = spread_atlas_px / atlas_size_px
    //   screen_per_uv = 1 / fwidth(uv)            (screen px per uv unit)
    //   screen_px_range = max(dot(unit_range, screen_per_uv), 1)
    let spread = max(in.params.x, 0.001);
    let atlas_size = vec2<f32>(textureDimensions(atlas_tex));
    let unit_range = vec2<f32>(spread, spread) / atlas_size;
    let dx = dpdx(in.uv);
    let dy = dpdy(in.uv);
    let screen_per_uv = vec2<f32>(1.0) / fwidth(in.uv);
    let screen_px_range = max(0.5 * dot(unit_range, screen_per_uv), 1.0);

    // 2×2 rotated-grid supersample inside the pixel. At small render
    // sizes (12–14 px UI text on 1× displays), MSDF sampled once per
    // pixel underestimates partial coverage at glyph boundaries — the
    // distance at the pixel centre poorly predicts the integrated edge
    // coverage. Four supersample taps approximate that integral; they
    // cost ~3 extra texture reads per fragment, which is cheap relative
    // to the quality recovery they buy at small sizes.
    let off1 =  0.125 * dx + 0.375 * dy;
    let off2 =  0.375 * dx - 0.125 * dy;
    let off3 = -0.375 * dx + 0.125 * dy;
    let off4 = -0.125 * dx - 0.375 * dy;
    let cov = 0.25 * (
        coverage_at(in.uv + off1, screen_px_range)
      + coverage_at(in.uv + off2, screen_px_range)
      + coverage_at(in.uv + off3, screen_px_range)
      + coverage_at(in.uv + off4, screen_px_range)
    );

    let out_alpha = in.color.a * cov;
    // Premultiplied colour output — pipeline uses standard alpha-blend.
    return vec4<f32>(in.color.rgb * out_alpha, out_alpha);
}
