// Scene3D mesh shader: single-pass forward lighting.
//
// Rewritten from the volumetric renderer's deferred g-buffer mesh shader —
// the deferred normal/depth outputs and the SSAO/composite passes are
// dropped for the closed-scope widget. One directional key light plus a
// flat ambient term, evaluated in the runner's linear working space.
//
// `base_color` and `key_color` arrive already converted to linear; output
// is premultiplied alpha so the resolved scene texture composites with the
// stock surface pipeline. Normals are transformed by the model matrix's
// linear part — correct for rotation and uniform scale (the V1 contract);
// strongly non-uniform scale will skew shading slightly.

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    base_color: vec4<f32>,  // rgb linear, a = opacity
    light_dir: vec4<f32>,   // xyz = world-space direction toward the light
    key_color: vec4<f32>,   // rgb linear, w = key intensity
    params: vec4<f32>,      // x = ambient term
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal_world: vec3<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let world = u.model * vec4<f32>(in.position, 1.0);
    out.position = u.view_proj * world;
    out.normal_world = (u.model * vec4<f32>(in.normal, 0.0)).xyz;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal_world);
    let l = normalize(u.light_dir.xyz);
    let ndotl = max(dot(n, l), 0.0);

    let lit = u.base_color.rgb * (u.params.x + u.key_color.rgb * u.key_color.w * ndotl);
    let a = u.base_color.a;
    return vec4<f32>(lit * a, a);
}
