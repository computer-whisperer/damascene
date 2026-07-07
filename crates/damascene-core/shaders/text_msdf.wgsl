// stock::text_msdf — multi-channel signed-distance-field glyph rendering.
//
// One pipeline. Each per-instance entry places a glyph quad in logical
// pixel space (`rect.xy/zw`) and samples an MTSDF page in
// (`uv.xy/zw`) at any size. The atlas stores RGB = MSDF distance
// channels and A = a true single-channel SDF. The fragment shader
// reconstructs the signed distance from median(R,G,B); when median
// disagrees with A about inside/outside (a classic MSDF artifact at
// sharp corners), A wins. AA width is derived from the screen-space UV
// gradient so it stays constant in screen pixels regardless of render
// scale. Each tap's coverage ramp is deliberately **half** a screen
// pixel wide (fdsm encodes ±spread/2 across 0..1, and `coverage_at`
// doubles the decoded value): the 2×2 rotated-grid supersample below
// spreads those steep taps across the pixel, integrating to ~1px of
// effective edge support with a steeper core than a single 1px ramp —
// close to a hinted rasterizer's box filter, where a single full-width
// ramp would read softer than browser text.
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
    // Output white-level scale: lifts working-space content to the output's
    // reference-white level on extended-range (scRGB) surfaces; 1.0 on SDR
    // targets. See docs/COLOR_MANAGEMENT.md.
    white_scale: f32,
    // Output luminance headroom and reference white (see image.wgsl, which
    // uses them for HDR remastering). Declared even where unused so the
    // uniform struct is 32 bytes — WebGL2 lacks
    // DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED and rejects
    // uniform types whose size is not a multiple of 16.
    headroom: f32,
    ref_nits: f32,
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

    // Gamma-aware coverage: browsers composite glyph masks in encoded
    // sRGB space, while our targets blend in linear light. At equal
    // coverage, linear blending renders dark-on-light text thinner and
    // light-on-dark text fatter than the browser reference — the
    // canonical GPU-text-vs-browser delta. The destination is
    // unknowable at fragment time, so remap coverage assuming a
    // *contrasting* background — exact for the dominant UI pairings
    // (near-black text on white surfaces, near-white text on zinc-950):
    //
    //   dark text:  a' = 1 − (1−a)^2.2   (matches black-on-white)
    //   light text: a' = a^2.2           (matches white-on-black)
    //
    // Correction strength fades to identity for mid-tone text, whose
    // background could sit on either side (e.g. muted grays), keyed by
    // *encoded* luminance so the perceptual midpoint — not linear 0.5 —
    // is the pivot (sqrt ≈ sRGB encode is plenty for a heuristic).
    let enc = sqrt(clamp(in.color.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    let enc_lum = dot(enc, vec3<f32>(0.2126, 0.7152, 0.0722));
    let t = 2.0 * enc_lum - 1.0; // -1 = black text … +1 = white text
    let cov_dark = 1.0 - pow(1.0 - cov, 2.2);
    let cov_light = pow(cov, 2.2);
    let cov_g = cov
        + clamp(-t, 0.0, 1.0) * (cov_dark - cov)
        + clamp(t, 0.0, 1.0) * (cov_light - cov);

    let out_alpha = in.color.a * cov_g;
    // Premultiplied colour output — pipeline uses standard alpha-blend.
    return vec4<f32>(in.color.rgb * out_alpha * frame.white_scale, out_alpha);
}
