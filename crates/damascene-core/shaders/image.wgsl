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
//
// `range.xy` = (content peak, luminance limit), both in working-space
// units (1.0 = reference white). When the peak exceeds the limit the
// fragment shader remasters HDR content through the BT.2390 EETF —
// knee + Hermite roll-off in PQ space, applied to maxRGB so hue is
// preserved. The limit is the El's `DynamicRangeLimit` policy resolved
// against the output's headroom at record time; content that already
// fits (every SDR image) takes the early-out and pays nothing. See
// docs/COLOR_MANAGEMENT.md.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
    // Output white-level scale: lifts working-space content to the output's
    // reference-white level on extended-range (scRGB) surfaces; 1.0 on SDR
    // targets. See docs/COLOR_MANAGEMENT.md.
    white_scale: f32,
    // Output headroom (target_max / reference luminance; 1.0 on SDR) and
    // the output's reference white in cd/m² (BT.2408 203 fallback). The
    // remaster needs absolute luminance because the BT.2390 curve shape
    // lives in PQ space.
    headroom: f32,
    ref_nits: f32,
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
    @location(5) range:  vec2<f32>,  // x = content peak, y = luminance limit
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      local_px:    vec2<f32>,  // pixels inside rect (top-left = 0,0)
    @location(1)      rect_size:   vec2<f32>,
    @location(2)      uv:          vec2<f32>,
    @location(3)      tint:        vec4<f32>,
    @location(4)      params:      vec4<f32>,
    @location(5) @interpolate(flat) range: vec2<f32>,
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
    out.range     = inst.range;
    return out;
}

// ST 2084 PQ inverse EOTF: display luminance normalized to 10000 cd/m²
// → electrical signal in [0, 1].
fn pq_oetf(y: f32) -> f32 {
    let m1 = 2610.0 / 16384.0;
    let m2 = 2523.0 / 4096.0 * 128.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 2413.0 / 4096.0 * 32.0;
    let c3 = 2392.0 / 4096.0 * 32.0;
    let ym = pow(clamp(y, 0.0, 1.0), m1);
    return pow((c1 + c2 * ym) / (1.0 + c3 * ym), m2);
}

// ST 2084 PQ EOTF (inverse of pq_oetf).
fn pq_eotf(e: f32) -> f32 {
    let m1 = 2610.0 / 16384.0;
    let m2 = 2523.0 / 4096.0 * 128.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 2413.0 / 4096.0 * 32.0;
    let c3 = 2392.0 / 4096.0 * 32.0;
    let em = pow(clamp(e, 0.0, 1.0), 1.0 / m2);
    return pow(max(em - c1, 0.0) / (c2 - c3 * em), 1.0 / m1);
}

// BT.2390-8 §5.4.1 EETF: map luminance mastered up to `src_peak` nits
// onto a display peaking at `dst_peak` nits. Identity below the knee
// (KS = 1.5·maxLum − 0.5 in source-normalized PQ), Hermite-spline
// roll-off above it. nits → nits. The caller guarantees
// `dst_peak < src_peak` (otherwise the curve is identity and skipped).
fn bt2390_eetf(nits: f32, src_peak: f32, dst_peak: f32) -> f32 {
    let src_max_pq = pq_oetf(src_peak / 10000.0);
    let max_lum = min(pq_oetf(dst_peak / 10000.0) / src_max_pq, 1.0);
    let ks = 1.5 * max_lum - 0.5;
    let e1 = min(pq_oetf(max(nits, 0.0) / 10000.0) / src_max_pq, 1.0);
    var e2 = e1;
    if (e1 >= ks) {
        let t = (e1 - ks) / (1.0 - ks);
        let t2 = t * t;
        let t3 = t2 * t;
        e2 = (2.0 * t3 - 3.0 * t2 + 1.0) * ks
            + (t3 - 2.0 * t2 + t) * (1.0 - ks)
            + (-2.0 * t3 + 3.0 * t2) * max_lum;
    }
    return pq_eotf(e2 * src_max_pq) * 10000.0;
}

// Remaster working-space rgb whose content peak exceeds the resolved
// luminance limit: BT.2390 roll-off on maxRGB (hue-preserving), in
// absolute nits anchored at the output's reference white.
fn remaster(rgb: vec3<f32>, peak: f32, limit: f32) -> vec3<f32> {
    let m = max(rgb.r, max(rgb.g, rgb.b));
    if (m <= 1e-4) {
        return rgb;
    }
    let nits = m * frame.ref_nits;
    let mapped = bt2390_eetf(nits, peak * frame.ref_nits, limit * frame.ref_nits);
    return rgb * (mapped / nits);
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
    var rgb = sampled.rgb * in.tint.rgb;
    // HDR remaster: only when the image's measured peak exceeds the
    // El's resolved luminance limit (SDR content never does).
    if (in.range.x > in.range.y) {
        rgb = remaster(rgb, in.range.x, in.range.y);
    }
    let alpha = sampled.a * in.tint.a * cov;
    // Premultiplied output for the standard alpha-blend pipeline.
    return vec4<f32>(rgb * alpha * frame.white_scale, alpha);
}
