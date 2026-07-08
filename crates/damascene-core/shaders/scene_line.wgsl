// Scene3D line shader.
//
// Each segment is one instance, expanded in the vertex stage into a
// screen-aligned quad with anti-aliased edges and optional dashing.
// Ported from damascene-volume's line shader (same author/license).
//
// Colours arrive in the runner's linear working space; output is
// premultiplied alpha (see scene_point.wgsl for the rationale).

struct Uniforms {
    // view_proj * model for this segment batch.
    mvp: mat4x4<f32>,
    screen_size: vec2<f32>,
    width_mode: u32,       // 0 = screen pixels, 1 = world units
    default_width: f32,    // used when an instance width is 0
    dash_length: f32,      // 0 disables dashing
    gap_length: f32,
    _pad0: f32,
    _pad1: f32,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct QuadVertex {
    // x: 0 = start, 1 = end; y: -1 = left, +1 = right.
    @location(0) corner: vec2<f32>,
};

struct LineInstance {
    @location(1) start: vec3<f32>,
    @location(2) end: vec3<f32>,
    @location(3) color: vec4<f32>, // linear, straight alpha
    @location(4) width: f32,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) edge_coord: f32,  // -1..+1 across width (AA)
    @location(2) line_coord: f32,  // distance along the segment (dashing)
};

@vertex
fn vs_main(quad: QuadVertex, line: LineInstance) -> VsOut {
    var out: VsOut;
    let t = quad.corner.x;

    let clip_start = u.mvp * vec4<f32>(line.start, 1.0);
    let clip_end = u.mvp * vec4<f32>(line.end, 1.0);

    let ndc_start = clip_start.xy / clip_start.w;
    let ndc_end = clip_end.xy / clip_end.w;

    let screen_start = (ndc_start * 0.5 + 0.5) * u.screen_size;
    let screen_end = (ndc_end * 0.5 + 0.5) * u.screen_size;
    let screen_dir = screen_end - screen_start;
    let screen_len = length(screen_dir);
    let perp = vec2<f32>(-screen_dir.y, screen_dir.x) / max(screen_len, 0.001);

    let width = select(u.default_width, line.width, line.width > 0.0);
    var half_width_px: f32;
    if u.width_mode == 0u {
        half_width_px = width * 0.5;
    } else {
        let clip_w = mix(clip_start.w, clip_end.w, t);
        half_width_px = (width / max(clip_w, 0.0001)) * u.screen_size.y * 0.5;
    }

    let offset_px = perp * half_width_px * quad.corner.y;
    let offset_ndc = offset_px * 2.0 / u.screen_size;
    let clip_pos = mix(clip_start, clip_end, t);
    out.position = vec4<f32>(clip_pos.xy + offset_ndc * clip_pos.w, clip_pos.zw);

    out.color = line.color;
    out.edge_coord = quad.corner.y;
    out.line_coord = t * length(line.end - line.start);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let edge_aa = 1.0 - smoothstep(0.85, 1.0, abs(in.edge_coord));

    var pattern = 1.0;
    if u.dash_length > 0.0 {
        let cycle = u.dash_length + u.gap_length;
        let pos = in.line_coord % cycle;
        pattern = smoothstep(0.0, 0.1, pos)
            * (1.0 - smoothstep(u.dash_length - 0.1, u.dash_length, pos));
    }

    let a = in.color.a * edge_aa * pattern;
    if a < 0.01 {
        discard;
    }
    return vec4<f32>(in.color.rgb * a, a);
}
