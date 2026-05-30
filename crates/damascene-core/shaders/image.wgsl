// stock::image — raster image rendering.
//
// One pipeline. Each per-instance entry places an image quad in
// logical pixel space (`rect.xy/zw`) and samples a per-image RGBA8
// texture (`uv.xy/zw`). The texture is bound at `@group(1)` —
// backends rebind group(1) when the source image changes between
// runs (one bind group per cached `Image::content_hash`).
//
// `params` carries per-corner radii (tl, tr, br, bl) in logical
// pixels — non-zero values fade alpha out via a rounded-rect SDF so
// authors can drop image corner masks without separately
// compositing. Authors that want a uniform radius write the same
// value to all four lanes.
//
// `tint.rgb * tint.a` multiplies the sampled colour. When the El had
// no `image_tint`, the recorder writes `(1,1,1,1)` and sampling is
// passthrough; with a tint colour the texture acts as a luminance /
// coverage map (useful for monochrome PNGs the app wants themed).

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var image_tex: texture_2d<f32>;
@group(1) @binding(1) var image_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:   vec4<f32>,  // xy = top-left logical px, zw = size logical px
    @location(2) tint:   vec4<f32>,  // rgba 0..1 (linear). (1,1,1,1) = no tint
    @location(3) params: vec4<f32>,  // per-corner radii (tl, tr, br, bl) logical px
    @location(4) uv:     vec4<f32>,  // xy = uv top-left 0..1, zw = uv size 0..1
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      local_px:    vec2<f32>,  // pixels inside rect (top-left = 0,0)
    @location(1)      rect_size:   vec2<f32>,
    @location(2)      uv:          vec2<f32>,
    @location(3)      tint:        vec4<f32>,
    @location(4)      params:      vec4<f32>,
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
    out.local_px  = in.corner_uv * inst.rect.zw;
    out.rect_size = inst.rect.zw;
    out.uv        = inst.uv.xy + in.corner_uv * inst.uv.zw;
    out.tint      = inst.tint;
    out.params    = inst.params;
    return out;
}

// Signed distance to a centred rounded box with per-corner radii
// (`tl, tr, br, bl`). Same convention as stock::rounded_rect's
// `sdf_rounded_box`. Each corner's radius is clamped to half the
// shorter side by the caller so the SDF stays well-formed when an
// author asks for radii larger than the rect.
fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // Pick the radius for the quadrant `p` lies in — top corners on
    // y<0, right corners on x>0. (`r` is `(tl, tr, br, bl)`.)
    let r_top = select(r.x, r.y, p.x > 0.0);  // tl or tr
    let r_bot = select(r.w, r.z, p.x > 0.0);  // bl or br
    let rd    = select(r_bot, r_top, p.y < 0.0);
    let q = abs(p) - b + vec2<f32>(rd, rd);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - rd;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(image_tex, image_smp, in.uv);

    // Rounded-corner coverage — clamp every corner radius to half the
    // shorter side and let the SDF carry AA across the boundary.
    let half_size = in.rect_size * 0.5;
    let centred = in.local_px - half_size;
    let max_r = min(half_size.x, half_size.y);
    let r = clamp(in.params, vec4<f32>(0.0), vec4<f32>(max_r));
    let d = sdf_rounded_box(centred, half_size, r);
    // 1 logical-pixel-wide AA band, scaled to physical pixels at flush.
    let aa = max(fwidth(d), 1e-4);
    let cov = clamp(0.5 - d / aa, 0.0, 1.0);

    // Tint multiply — when no tint was set the recorder writes
    // (1,1,1,1) so this is identity.
    let rgb = sampled.rgb * in.tint.rgb;
    let alpha = sampled.a * in.tint.a * cov;
    // Premultiplied output for the standard alpha-blend pipeline.
    return vec4<f32>(rgb * alpha, alpha);
}
