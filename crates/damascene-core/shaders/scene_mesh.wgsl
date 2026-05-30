// Scene3D mesh shader: single-pass forward lighting.
//
// Rewritten from the volumetric renderer's deferred g-buffer mesh shader —
// the deferred normal/depth outputs and the SSAO/composite passes are
// dropped for the closed-scope widget. One directional key light, a
// hemispheric ambient fill (sky above / ground below, by the surface normal),
// and an optional Blinn-Phong specular highlight, all evaluated in the
// runner's linear working space.
//
// All colours arrive already converted to linear; output is premultiplied
// alpha so the resolved scene texture composites with the stock surface
// pipeline. Normals are transformed by the model matrix's linear part —
// correct for rotation and uniform scale (the V1 contract); strongly
// non-uniform scale will skew shading slightly.

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    base_color: vec4<f32>,    // rgb linear, a = opacity
    light_dir: vec4<f32>,     // xyz = world-space direction toward the key, w = key intensity
    key_color: vec4<f32>,     // rgb linear, w = specular strength
    sky_color: vec4<f32>,     // rgb linear (hemispheric up), w = shininess
    ground_color: vec4<f32>,  // rgb linear (hemispheric down), w = ambient scale
    eye_pos: vec4<f32>,       // xyz = world-space camera position
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
    @location(1) world_pos: vec3<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let world = u.model * vec4<f32>(in.position, 1.0);
    out.position = u.view_proj * world;
    out.normal_world = (u.model * vec4<f32>(in.normal, 0.0)).xyz;
    out.world_pos = world.xyz;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal_world);
    let l = normalize(u.light_dir.xyz);
    let key_intensity = u.light_dir.w;
    let specular_strength = u.key_color.w;
    let shininess = u.sky_color.w;
    let ambient_scale = u.ground_color.w;

    // Hemispheric ambient: blend ground→sky by the normal's vertical component.
    let hemi_t = clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    let ambient = mix(u.ground_color.rgb, u.sky_color.rgb, hemi_t) * ambient_scale;

    // Directional key (Lambert).
    let ndotl = max(dot(n, l), 0.0);
    let diffuse = u.key_color.rgb * key_intensity * ndotl;

    // Blinn-Phong specular in the key-light colour, gated on the face being
    // lit so back faces get no rim.
    let view_dir = normalize(u.eye_pos.xyz - in.world_pos);
    let half_v = normalize(l + view_dir);
    let spec_mask = select(0.0, 1.0, ndotl > 0.0);
    let spec = pow(max(dot(n, half_v), 0.0), shininess) * specular_strength * spec_mask;
    let specular = u.key_color.rgb * key_intensity * spec;

    let lit = u.base_color.rgb * (ambient + diffuse) + specular;
    let a = u.base_color.a;
    return vec4<f32>(lit * a, a);
}
