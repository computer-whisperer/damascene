// stock::text — unified-RGBA glyph rendering.
//
// One pipeline. Each per-instance entry places a glyph quad in logical
// pixel space (`rect.xy/zw`) and samples a single Rgba8UnormSrgb page
// texture at (`uv.xy/zw`). The fragment shader modulates the sampled
// texel by the per-glyph color and premultiplies for alpha blending.
//
// Two glyph kinds share the pipeline:
//
// - **Outline glyphs** (Roboto, etc.) are stored in the atlas as
//   `(255, 255, 255, alpha)`. The per-glyph color carries the user's
//   text color, so `texel * color` produces colored anti-aliased text.
// - **Color emoji** (CBDT/COLR/sbix) are stored as native RGBA. Backends
//   pass white as the per-glyph color so the bitmap RGB passes through
//   unmodulated; alpha edges still come from the bitmap.
//
// The page texture lives in a separate bind group so the pipeline can
// stay shared across pages — backends just rebind group(1) when the
// active atlas page changes between text runs.

struct FrameUniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:  vec4<f32>,  // xy = top-left logical px, zw = size logical px
    @location(2) uv:    vec4<f32>,  // xy = uv top-left 0..1, zw = uv size 0..1
    @location(3) color: vec4<f32>,  // rgba 0..1 (linear)
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      uv: vec2<f32>,
    @location(1)      color: vec4<f32>,
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
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_tex, atlas_smp, in.uv);
    let modulated = texel * in.color;
    // Premultiplied output — pipeline blend state is alpha-blending.
    return vec4<f32>(modulated.rgb * modulated.a, modulated.a);
}
