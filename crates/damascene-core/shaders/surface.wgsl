// stock::surface — app-owned-texture compositing.
//
// One vertex stage; three fragment entry points, one per SurfaceAlpha
// mode. The backend builds three pipelines that share everything except
// the fragment entry point and the blend state — no per-instance switch.
//
// Per-instance data carries:
//   - `rect`: post-fit destination in logical pixels (xy = top-left,
//     zw = size).
//   - `matrix` + `translation`: a 2x3 affine applied to the textured
//     quad in destination space, around the centre of `rect`. Identity
//     reduces vs_main to a plain rect cover.
//
// Sampling uses `corner_uv` directly so the texture covers the post-fit
// rect 1:1; format-dependent decode (sRGB → linear) is handled by the
// texture view. ImageFit projection is applied CPU-side before the
// instance is built.

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
@group(1) @binding(0) var surf_tex: texture_2d<f32>;
@group(1) @binding(1) var surf_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:        vec4<f32>,  // xy = top-left logical px, zw = size logical px
    @location(2) matrix:      vec4<f32>,  // a, b, c, d (Damascene Affine2 column-major: col0=[a,b], col1=[c,d])
    @location(3) translation: vec2<f32>,  // tx, ty
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      uv:        vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let center = inst.rect.xy + inst.rect.zw * 0.5;
    let local  = (in.corner_uv - vec2<f32>(0.5, 0.5)) * inst.rect.zw;
    let m_a = inst.matrix.x;
    let m_b = inst.matrix.y;
    let m_c = inst.matrix.z;
    let m_d = inst.matrix.w;
    let transformed = vec2<f32>(
        m_a * local.x + m_c * local.y + inst.translation.x,
        m_b * local.x + m_d * local.y + inst.translation.y,
    );
    let pos_px = transformed + center;
    let clip = vec4<f32>(
        pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_px.y / frame.viewport.y * 2.0,
        0.0,
        1.0,
    );
    var out: VertexOutput;
    out.clip_pos = clip;
    out.uv       = in.corner_uv;
    return out;
}

// Premultiplied input, premultiplied output. Pairs with
// (One, OneMinusSrcAlpha) blend.
@fragment
fn fs_premul(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(surf_tex, surf_smp, in.uv);
    return vec4<f32>(s.rgb * frame.white_scale, s.a);
}

// Straight (unpremultiplied) input — premultiply in the shader before
// blending. Pairs with (One, OneMinusSrcAlpha) blend.
@fragment
fn fs_straight(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(surf_tex, surf_smp, in.uv);
    return vec4<f32>(s.rgb * s.a * frame.white_scale, s.a);
}

// Opaque — alpha channel of the texture is ignored; output replaces
// the destination. Pairs with (One, Zero) blend or no blend at all.
@fragment
fn fs_opaque(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(surf_tex, surf_smp, in.uv);
    return vec4<f32>(s.rgb * frame.white_scale, 1.0);
}
