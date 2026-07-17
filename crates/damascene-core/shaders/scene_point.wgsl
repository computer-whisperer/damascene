// Scene3D point/scatter shader.
//
// Each point is one instance, expanded in the vertex stage into a
// screen-aligned billboard quad with an anti-aliased shape (circle /
// square / diamond). Ported from the volumetric CAD project's
// `volumetric_renderer` point shader
// (github.com/computer-whisperer/volumetric; same author).
//
// Colours arrive already converted to the runner's linear working space
// (the backend converts authoring-space sRGBA at upload), so the fragment
// stage applies no transfer function. Output is premultiplied alpha so the
// resolved scene texture composites with the stock surface pipeline.

struct Uniforms {
    // view_proj * model for this point batch.
    mvp: mat4x4<f32>,
    // Offscreen target size in physical pixels.
    screen_size_px: vec2<f32>,
    point_size_px: f32,
    size_mode: u32,        // 0 = screen pixels (constant), 1 = world units
    shape: u32,            // 0 = circle, 1 = square, 2 = diamond
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct VsIn {
    // Per-vertex quad corner / uv.
    @location(0) corner: vec2<f32>, // -1..+1
    @location(1) uv: vec2<f32>,     //  0..+1
    // Per-instance point.
    @location(2) position: vec3<f32>,
    @location(3) color: vec4<f32>,  // linear, straight alpha
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let clip = u.mvp * vec4<f32>(in.position, 1.0);

    var half_size_px: f32;
    if u.size_mode == 0u {
        half_size_px = u.point_size_px * 0.5;
    } else {
        // World-space: size shrinks with distance (clip.w).
        half_size_px = (u.point_size_px / max(clip.w, 0.0001)) * u.screen_size_px.y * 0.5;
    }

    let offset_ndc = in.corner * (half_size_px * 2.0 / u.screen_size_px);
    out.position = vec4<f32>(clip.xy + offset_ndc * clip.w, clip.zw);
    out.color = in.color;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv * 2.0 - vec2<f32>(1.0, 1.0);

    var alpha: f32;
    switch u.shape {
        case 1u: {
            // Square.
            let m = max(abs(uv.x), abs(uv.y));
            alpha = 1.0 - smoothstep(0.9, 1.0, m);
        }
        case 2u: {
            // Diamond (L1 norm).
            let d = abs(uv.x) + abs(uv.y);
            alpha = 1.0 - smoothstep(0.9, 1.0, d);
        }
        default: {
            // Circle.
            let d = length(uv);
            alpha = 1.0 - smoothstep(0.9, 1.0, d);
        }
    }

    let a = in.color.a * alpha;
    if a < 0.01 {
        discard;
    }
    return vec4<f32>(in.color.rgb * a, a);
}
