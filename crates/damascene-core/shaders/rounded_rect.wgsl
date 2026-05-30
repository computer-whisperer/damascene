// stock::rounded_rect — the workhorse surface shader.
//
// One pipeline. Per-instance uniforms drive fill, stroke, radius,
// shadow, focus ring. The quad is sized to `rect` (the painted rect,
// possibly outset for `paint_overflow`); SDF + outline use `inner_rect`
// so the rect stays anchored to the layout bounds even when the
// painted area extends outward. Drop shadow and outside focus rings render
// in the band between inner_rect and rect when their respective uniforms are
// non-zero; negative focus_width draws the focus ring inside inner_rect.
//
// Coordinate system: vertex positions arrive as a unit quad with uv 0..1.
// Per-instance `rect` (xy, wh) places it in pixel space; the vertex
// shader maps to clip space via the frame's `viewport` (width, height).
// The fragment shader computes the rounded-box SDF in centered pixel
// coordinates relative to `inner_rect` and antialiases at the boundary
// using the SDF gradient magnitude (length of dpdx/dpdy).

struct FrameUniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:        vec4<f32>,  // painted rect: xy = top-left px, zw = size px
    @location(2) fill:        vec4<f32>,  // rgba 0..1
    @location(3) stroke:      vec4<f32>,  // rgba 0..1
    // params.y carries the *max* corner radius for back-compat with
    // custom shaders that read scalar `params.y` as the radius. The
    // actual SDF reads per-corner radii from `radii` below; for
    // shadow + focus-ring SDF the scalar max is good enough.
    @location(4) params:      vec4<f32>,  // x=stroke_width, y=max_radius, z=shadow, w=focus_width
    @location(5) inner_rect:  vec4<f32>,  // layout rect (== rect when no paint_overflow)
    @location(6) focus_color: vec4<f32>,  // rgba 0..1, alpha already eased
    @location(7) radii:       vec4<f32>,  // per-corner radii (tl, tr, br, bl) in logical px
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    // `@interpolate(perspective, sample)` on `pos_px` is what asks the
    // rasterizer to run fs_main once per MSAA sample (sample-rate
    // shading) instead of once per pixel — see the comment on fs_main.
    // The other varyings are constant across the quad so they can stay
    // at the default centroid interpolation.
    @location(0) @interpolate(perspective, sample) pos_px: vec2<f32>,
    @location(1)      inner_center: vec2<f32>,
    @location(2)      inner_half_size: vec2<f32>,
    @location(3)      fill: vec4<f32>,
    @location(4)      stroke: vec4<f32>,
    @location(5)      params: vec4<f32>,
    @location(6)      focus_color: vec4<f32>,
    @location(7)      radii: vec4<f32>,
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
    out.pos_px = pos_px;
    out.inner_center = inst.inner_rect.xy + inst.inner_rect.zw * 0.5;
    out.inner_half_size = inst.inner_rect.zw * 0.5;
    out.fill = inst.fill;
    out.stroke = inst.stroke;
    out.params = inst.params;
    out.focus_color = inst.focus_color;
    out.radii = inst.radii;
    return out;
}

// SDF for a centered rounded box with per-corner radii
// (`tl, tr, br, bl`). Picks the radius for the quadrant `p` lies in:
// top corners on y<0, right corners on x>0. The caller clamps each
// corner radius to half the shorter side so the SDF stays well-formed
// when an author asks for radii larger than the rect.
//
// Same convention as `image.wgsl` so the two stock shaders agree.
fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let r_top = select(r.x, r.y, p.x > 0.0);  // tl or tr
    let r_bot = select(r.w, r.z, p.x > 0.0);  // bl or br
    let rd    = select(r_bot, r_top, p.y < 0.0);
    let q = abs(p) - b + vec2<f32>(rd, rd);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - rd;
}

// Sample-rate shading is requested via `@interpolate(perspective,
// sample)` on `pos_px` (see VertexOutput). When the pipeline runs at
// sample_count > 1, fs_main is invoked once per MSAA sample with
// `pos_px` interpolated to that sub-sample's location; smoothstep AA
// then evaluates at sub-pixel resolution and the four samples are
// averaged on resolve, smoothing the brightness pop where a curved
// boundary lands near a pixel center.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let stroke_width = in.params.x;
    // Per-corner radii clamped to half the shorter side so the SDF
    // stays well-formed for arbitrary author input.
    let max_r = min(in.inner_half_size.x, in.inner_half_size.y);
    let radii = clamp(in.radii, vec4<f32>(0.0), vec4<f32>(max_r));
    let shadow_blur = in.params.z;
    let focus_width = in.params.w;

    // Pixel position relative to the inner (layout) rect's centre.
    let local_px = in.pos_px - in.inner_center;
    let d = sdf_rounded_box(local_px, in.inner_half_size, radii);

    // Anti-aliasing width derived from screen-space derivatives. Use the
    // L2 norm of the SDF gradient (not fwidth's L1) so the AA band is the
    // same width on diagonals as on axis-aligned edges — fwidth would be
    // sqrt(2)x larger at 45° and visibly fatten the rounded corners.
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);

    // Inside coverage of the layout rect.
    let inside = 1.0 - smoothstep(-aa, 0.0, d);

    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);

    // Drop shadow — rendered first so fill/stroke composite over it. The
    // shadow is the layout-rect's rounded silhouette dropped down by
    // `blur*0.5` and softened over a `blur`-wide band. Painted rect is
    // outset in `draw_ops` to give the halo room outside `inner_rect`;
    // we cap inside-the-box alpha at the layout boundary so opaque fills
    // sit cleanly on top, while alpha-fill nodes (popovers with a tint)
    // still get a darkened ground.
    if (shadow_blur > 0.0) {
        let shadow_dy = shadow_blur * 0.5;
        let shadow_local = local_px - vec2<f32>(0.0, shadow_dy);
        let shadow_d = sdf_rounded_box(shadow_local, in.inner_half_size, radii);
        let shadow_alpha = (1.0 - smoothstep(-shadow_blur, shadow_blur, shadow_d)) * 0.30;
        color = vec4<f32>(0.0, 0.0, 0.0, shadow_alpha);
    }

    // Fill — mix-and-max over the (possibly shadowed) background so a
    // partly-transparent fill blends with the shadow instead of
    // clobbering it.
    if (in.fill.a > 0.0) {
        let fill_alpha = in.fill.a * inside;
        color = vec4<f32>(
            mix(color.rgb, in.fill.rgb, fill_alpha),
            max(color.a, fill_alpha),
        );
    }

    // Stroke: a band of width stroke_width centered on the boundary of
    // the layout rect.
    if (stroke_width > 0.0 && in.stroke.a > 0.0) {
        let stroke_d = abs(d) - stroke_width * 0.5;
        let stroke_alpha = (1.0 - smoothstep(-aa, aa, stroke_d)) * in.stroke.a;
        color = vec4<f32>(
            mix(color.rgb, in.stroke.rgb, stroke_alpha),
            max(color.a, stroke_alpha),
        );
    }

    // Focus ring: positive `focus_width` draws just outside the layout
    // rect's boundary, in the paint_overflow halo. Negative `focus_width`
    // draws the same-width band just inside the boundary for dense flush
    // rows. Alpha is the eased per-frame focus envelope (already baked into
    // focus_color.a by draw_ops).
    if (focus_width != 0.0 && in.focus_color.a > 0.0) {
        let ring_width = abs(focus_width);
        let ring_center = select(-ring_width * 0.5, ring_width * 0.5, focus_width > 0.0);
        let ring_d = abs(d - ring_center) - ring_width * 0.5;
        let ring_alpha = (1.0 - smoothstep(-aa, aa, ring_d)) * in.focus_color.a;
        color = vec4<f32>(
            mix(color.rgb, in.focus_color.rgb, ring_alpha),
            max(color.a, ring_alpha),
        );
    }

    return color;
}
