// stock::text_highlight — solid-fill background quads behind inline runs.
//
// Used by the text recorder to paint `RunStyle.bg` underlays beneath the
// glyphs of a styled span. One per-instance entry places a logical-pixel
// rect and emits a single premultiplied solid colour. No texture sampling,
// no SDF, no rounding.

struct FrameUniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:  vec4<f32>,  // xy = top-left logical px, zw = size logical px
    @location(2) color: vec4<f32>,  // rgba 0..1 (linear)
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      color: vec4<f32>,
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
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Premultiplied output — pipeline blend state is alpha-blending.
    return vec4<f32>(in.color.rgb * in.color.a, in.color.a);
}
