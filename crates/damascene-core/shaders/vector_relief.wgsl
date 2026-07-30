// stock::vector_relief — shader-material proof for vector icons.
//
// Uses local SVG coordinates from VectorMeshVertex to shade an icon
// consistently inside its own viewBox, independent of destination size.

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

// Fragment-stage gradient evaluation — identical copy of the block in
// vector.wgsl (the canonical one, see its comment for the contract).
struct GradientParams {
    m0: vec4<f32>,
    m1: vec4<f32>,
    misc: vec4<f32>,
};

struct GradientTable {
    entries: array<GradientParams, 128>,
};

@group(1) @binding(0) var<uniform> gradients: GradientTable;
@group(1) @binding(1) var gradient_ramps: texture_2d<f32>;
@group(1) @binding(2) var gradient_sampler: sampler;

fn vector_paint(color: vec4<f32>, local: vec2<f32>, meta_z: f32) -> vec4<f32> {
    let slot = i32(meta_z + 0.5);
    if (slot <= 0) {
        return color;
    }
    let g = gradients.entries[slot - 1];
    var t: f32;
    if (g.m0.w < 0.5) {
        t = g.m0.x * local.x + g.m0.y * local.y + g.m0.z;
    } else {
        let q = vec2<f32>(
            g.m0.x * local.x + g.m0.y * local.y + g.m0.z,
            g.m1.x * local.x + g.m1.y * local.y + g.m1.z,
        );
        t = length(q);
    }
    let spread = i32(g.m1.w + 0.5);
    if (spread == 1) {
        let m = t - 2.0 * floor(t * 0.5);
        t = 1.0 - abs(m - 1.0);
    } else if (spread == 2) {
        t = fract(t);
    } else {
        t = clamp(t, 0.0, 1.0);
    }
    let u = (0.5 + t * 255.0) / 256.0;
    var c = textureSampleLevel(gradient_ramps, gradient_sampler, vec2<f32>(u, g.misc.x), 0.0);
    c.a = c.a * g.misc.y;
    return c;
}

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

    let paint = vector_paint(in.color, in.local, in.data.z);
    var rgb = paint.rgb;
    rgb = rgb + vec3<f32>(0.22) * top_light + vec3<f32>(0.10) * left_glint;
    rgb = rgb * (1.0 - 0.22 * lower_shade - stroke_bias * uv.y);
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let alpha = paint.a;
    return vec4<f32>(rgb * alpha * frame.white_scale, alpha);
}
