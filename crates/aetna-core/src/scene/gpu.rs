//! Backend-neutral GPU byte layouts and CPU-side packing for `Scene3D`.
//!
//! Every backend (wgpu, vulkano, ash) renders the same four scene shaders
//! ([`stock_wgsl`](crate::shader::stock_wgsl)) over the same vertex/uniform
//! *byte layouts*. Those `#[repr(C)]` structs and the pure functions that
//! fill them therefore live here, once, rather than being hand-copied into
//! each backend where they would silently drift out of sync with the WGSL.
//!
//! A backend uploads these by casting with `bytemuck` and binds them through
//! its own vertex-attribute / descriptor tables (the attribute *offsets*
//! mirror the field order below; the tables themselves stay per-backend
//! because they speak each backend's API types). The structs expose no
//! public fields on purpose — the layout is an implementation detail of this
//! module, and callers only ever produce them through the packing functions
//! and consume them as opaque `Pod` bytes.
//!
//! What is *not* here: GPU resource types (buffers, textures, pipelines),
//! the offscreen/MSAA/composite passes, the geometry cache, and the
//! depth-occlusion read-back — those are inherently per-backend and stay in
//! the backend crates.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use crate::color::{Color, ColorSpace};
use crate::paint::rgba_f32_in;
use crate::scene::data::{LineDraw, MeshDraw, PointDraw, Scene3DData};
use crate::scene::geometry::{LineData, MeshData, PointData};
use crate::scene::style::{GridPlanes, LinePattern, Material, PointShape, SceneStyle, SizeMode};

/// Upper bound on grid lines per axis direction, so a tiny `spacing` against
/// a large `extent` can't generate an unbounded line batch.
const MAX_GRID_LINES: i32 = 256;

// ---- GPU-side POD structs ----
//
// Field order is the byte layout the WGSL expects; explicit `_pad` fields
// keep each struct free of implicit padding (required by `Pod`) and the
// uniforms within a 256-byte dynamic-offset slot.

/// Per-draw uniform for the point shader.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct PointUniform {
    mvp: [[f32; 4]; 4],
    screen_size_px: [f32; 2],
    point_size_px: f32,
    size_mode: u32,
    shape: u32,
    _pad: [u32; 3],
}

/// Per-draw uniform for the line shader (also drives the reference grid).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LineUniform {
    mvp: [[f32; 4]; 4],
    screen_size: [f32; 2],
    width_mode: u32,
    default_width: f32,
    dash_length: f32,
    gap_length: f32,
    _pad: [f32; 2],
}

/// Per-draw uniform for the forward-lit mesh shader. The scalar light/material
/// parameters are tucked into the unused `w` lanes of the colour/direction
/// vectors so the block stays free of stray padding.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MeshUniform {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    base_color: [f32; 4],   // rgb linear, w = opacity
    light_dir: [f32; 4],    // xyz = world dir toward key, w = key intensity
    key_color: [f32; 4],    // rgb linear, w = specular strength
    sky_color: [f32; 4],    // rgb linear (hemispheric up), w = shininess
    ground_color: [f32; 4], // rgb linear (hemispheric down), w = ambient scale
    eye_pos: [f32; 4],      // xyz = world-space camera position
}

/// Per-point instance: world position + working-space colour.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct PointInstance {
    position: [f32; 3],
    color: [f32; 4],
}

/// Per-segment instance: endpoints, working-space colour, per-segment width
/// (0 = use the uniform's default width).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LineInstance {
    start: [f32; 3],
    end: [f32; 3],
    color: [f32; 4],
    width: f32,
}

/// Mesh vertex as the shader reads it: object-space position + normal.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MeshVertexGpu {
    position: [f32; 3],
    normal: [f32; 3],
}

/// Composite instance — identical layout to the stock `surface` painter's,
/// so the `surface` shader's vertex stage reads it unchanged when a resolved
/// scene texture composites into the main pass.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct CompositeInstance {
    rect: [f32; 4],
    matrix: [f32; 4],
    translation: [f32; 2],
}

impl CompositeInstance {
    /// Build a composite instance from a logical rect and the affine the
    /// stock surface vertex stage applies (a 2×2 `matrix` packed row-major
    /// plus a `translation`).
    pub fn new(rect: [f32; 4], matrix: [f32; 4], translation: [f32; 2]) -> Self {
        Self {
            rect,
            matrix,
            translation,
        }
    }
}

// ---- packing functions ----

/// Interpret an authoring-space sRGBA `[f32; 4]` (the geometry colour
/// contract) and convert it into the working linear space.
pub fn to_linear(srgba: [f32; 4], working: ColorSpace) -> [f32; 4] {
    rgba_f32_in(
        Color::in_space(ColorSpace::SRGB, srgba[0], srgba[1], srgba[2], srgba[3]),
        working,
    )
}

/// `SizeMode` as the shaders' `*_mode` discriminant (0 = screen px, 1 = world).
pub fn size_mode_code(mode: SizeMode) -> u32 {
    match mode {
        SizeMode::ScreenSpace => 0,
        SizeMode::World => 1,
    }
}

/// Pack a point mark's per-draw uniform. `mvp` is the full model-view-proj
/// (the mark transform already folded in); `screen` is the viewport size px.
pub fn point_uniform(mvp: Mat4, screen: [f32; 2], draw: &PointDraw) -> PointUniform {
    PointUniform {
        mvp: mvp.to_cols_array_2d(),
        screen_size_px: screen,
        point_size_px: draw.style.size,
        size_mode: size_mode_code(draw.style.size_mode),
        shape: match draw.style.shape {
            PointShape::Circle => 0,
            PointShape::Square => 1,
        },
        _pad: [0; 3],
    }
}

/// Pack a line mark's per-draw uniform. `mvp` is the full model-view-proj.
pub fn line_uniform(mvp: Mat4, screen: [f32; 2], draw: &LineDraw) -> LineUniform {
    let (dash, gap) = match draw.style.pattern {
        LinePattern::Solid => (0.0, 0.0),
        // Screen-pixel dash cadence; world-unit dashing would scale with
        // zoom, which reads worse for reference strokes.
        LinePattern::Dashed => (8.0, 6.0),
    };
    LineUniform {
        mvp: mvp.to_cols_array_2d(),
        screen_size: screen,
        width_mode: size_mode_code(draw.style.size_mode),
        default_width: draw.style.width,
        dash_length: dash,
        gap_length: gap,
        _pad: [0.0; 2],
    }
}

/// Per-draw uniform for the reference grid + axes line batch: screen-space
/// 1px strokes, no dashing. Pairs with [`build_grid_lines`].
pub fn grid_uniform(view_proj: Mat4, screen: [f32; 2]) -> LineUniform {
    LineUniform {
        mvp: view_proj.to_cols_array_2d(),
        screen_size: screen,
        width_mode: 0, // grid/axes are screen-space px
        default_width: 1.0,
        dash_length: 0.0,
        gap_length: 0.0,
        _pad: [0.0; 2],
    }
}

/// Pack a mesh mark's per-draw uniform from its material and the scene's
/// light rig. `view_proj` is the camera matrix (the mesh's own `model`
/// transform is read from `draw`).
pub fn mesh_uniform(
    view_proj: Mat4,
    draw: &MeshDraw,
    scene: &Scene3DData,
    working: ColorSpace,
) -> MeshUniform {
    let light = &scene.lights;
    let dir = light.key_direction.normalize_or_zero();
    let eye = scene.camera.eye;

    // Resolve the material to a base colour + specular params + whether it is
    // lit. `Flat` is unlit: fold it into the lit shader as full-strength white
    // ambient with no key/specular, so the shader returns the base verbatim.
    // Custom material shaders are post-V1 (plan M5); render as Matte so the
    // mesh stays visible rather than dropped.
    let (base, specular, shininess, unlit) = match &draw.material {
        Material::Matte { base } => (*base, 0.0, 1.0, false),
        Material::Glossy {
            base,
            specular,
            shininess,
        } => (*base, *specular, shininess.max(1.0), false),
        Material::Flat { color } => (*color, 0.0, 1.0, true),
        Material::Custom { .. } => (Color::srgb_u8(214, 220, 230), 0.0, 1.0, false),
    };

    let white = Color::srgb_u8(255, 255, 255);
    let key_intensity = if unlit { 0.0 } else { light.key_intensity };
    let ambient_scale = if unlit { 1.0 } else { light.ambient };
    let sky = rgba_f32_in(if unlit { white } else { light.sky_color }, working);
    let ground = rgba_f32_in(if unlit { white } else { light.ground_color }, working);
    let key = rgba_f32_in(light.key_color, working);
    let base = rgba_f32_in(base, working);

    MeshUniform {
        view_proj: view_proj.to_cols_array_2d(),
        model: draw.transform.to_cols_array_2d(),
        base_color: base,
        light_dir: [dir.x, dir.y, dir.z, key_intensity],
        key_color: [key[0], key[1], key[2], specular],
        sky_color: [sky[0], sky[1], sky[2], shininess],
        ground_color: [ground[0], ground[1], ground[2], ambient_scale],
        eye_pos: [eye.x, eye.y, eye.z, 0.0],
    }
}

// ---- geometry → GPU buffer conversion ----

/// Pack mesh vertices into their GPU layout. The backend uploads the result
/// as a vertex buffer; indices (if any) upload as-is from `data.indices`.
pub fn mesh_vertices(data: &MeshData) -> Vec<MeshVertexGpu> {
    data.vertices
        .iter()
        .map(|v| MeshVertexGpu {
            position: v.position.to_array(),
            normal: v.normal.to_array(),
        })
        .collect()
}

/// Pack point geometry into per-point instances, converting colours from the
/// authoring sRGBA contract into the runner's `working` space.
pub fn point_instances(data: &PointData, working: ColorSpace) -> Vec<PointInstance> {
    data.points
        .iter()
        .map(|p| PointInstance {
            position: p.position.to_array(),
            color: to_linear(p.color, working),
        })
        .collect()
}

/// Pack line geometry into per-segment instances. Per-segment width is 0 so
/// the line uniform's default width applies; colours convert to `working`.
pub fn line_instances(data: &LineData, working: ColorSpace) -> Vec<LineInstance> {
    data.segments
        .iter()
        .map(|s| LineInstance {
            start: s.start.to_array(),
            end: s.end.to_array(),
            color: to_linear(s.color, working),
            width: 0.0,
        })
        .collect()
}

// ---- reference grid + axes ----

/// Generate the reference grid + axes as line instances (colours already in
/// the working space). Grid segments carry width 0 so the line uniform's
/// default width applies; axes carry an explicit, slightly bolder width.
pub fn build_grid_lines(style: &SceneStyle, working: ColorSpace, out: &mut Vec<LineInstance>) {
    let g = &style.grid;
    // Effective [min, max] per axis (per-axis bound, else symmetric extent).
    let spans = g.axis_spans();
    let extent = g.extent.max(0.0);

    if extent > 0.0 && g.planes != GridPlanes::NONE {
        let grid_color = rgba_f32_in(g.color, working);
        let step = (g.spacing / g.subdivisions.max(1) as f32).max(1e-4);
        // (X, Y, Z) → spans[0], spans[1], spans[2].
        if g.planes.xz {
            plane_grid(out, Vec3::X, Vec3::Z, spans[0], spans[2], step, grid_color);
        }
        if g.planes.xy {
            plane_grid(out, Vec3::X, Vec3::Y, spans[0], spans[1], step, grid_color);
        }
        if g.planes.yz {
            plane_grid(out, Vec3::Y, Vec3::Z, spans[1], spans[2], step, grid_color);
        }
    }

    if style.show_axes {
        // Muted R/G/B for X/Y/Z — readable without the neon look. An explicit
        // per-axis bound governs the line span; otherwise a symmetric reach
        // that still shows unit axes when the grid is tiny/zero.
        let fallback = extent.max(g.spacing).max(1.0);
        for (i, dir, rgb) in [
            (0usize, Vec3::X, Color::srgb_u8(206, 86, 86)),
            (1, Vec3::Y, Color::srgb_u8(120, 190, 110)),
            (2, Vec3::Z, Color::srgb_u8(110, 150, 225)),
        ] {
            let (lo, hi) = match g.bounds.axis(i) {
                Some((a, b)) => (a.min(b), a.max(b)),
                None => (-fallback, fallback),
            };
            push_seg(out, dir * lo, dir * hi, rgba_f32_in(rgb, working), 1.6);
        }
    }
}

/// Grid lines for one world plane spanned by unit axes `u`, `v`, each clipped
/// to its `[min, max]` span: lines parallel to `u` at every `step` offset
/// across `v`'s span, and lines parallel to `v` across `u`'s span.
fn plane_grid(
    out: &mut Vec<LineInstance>,
    u: Vec3,
    v: Vec3,
    u_span: (f32, f32),
    v_span: (f32, f32),
    step: f32,
    color: [f32; 4],
) {
    for off in grid_offsets(v_span, step) {
        push_seg(
            out,
            u * u_span.0 + v * off,
            u * u_span.1 + v * off,
            color,
            0.0,
        );
    }
    for off in grid_offsets(u_span, step) {
        push_seg(
            out,
            v * v_span.0 + u * off,
            v * v_span.1 + u * off,
            color,
            0.0,
        );
    }
}

/// Grid-line offsets along an axis: integer multiples of `step` lying within
/// `[min, max]` (so the lines align to the origin and clip to the span),
/// capped so a tiny step can't emit a runaway count.
fn grid_offsets(span: (f32, f32), step: f32) -> Vec<f32> {
    let (lo, hi) = (span.0.min(span.1), span.0.max(span.1));
    let k0 = (lo / step).ceil() as i32;
    let k1 = (hi / step).floor() as i32;
    let mut offs = Vec::new();
    for k in k0..=k1 {
        offs.push(k as f32 * step);
        if offs.len() >= 2 * MAX_GRID_LINES as usize {
            break;
        }
    }
    offs
}

fn push_seg(out: &mut Vec<LineInstance>, a: Vec3, b: Vec3, color: [f32; 4], width: f32) {
    out.push(LineInstance {
        start: a.to_array(),
        end: b.to_array(),
        color,
        width,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    /// Pin the GPU byte sizes the scene WGSL reads. These structs are the
    /// one place all three backends share the layout, so a stray field
    /// reorder or addition here would silently desync every shader — this
    /// test (alongside the `Pod` derive, which already rejects implicit
    /// padding) makes that a loud failure instead.
    /// A per-axis bound clips both the axis line and the grid plane lines to
    /// `[min, max]` for that axis, while unbounded axes stay symmetric.
    #[test]
    fn per_axis_bounds_clip_grid_and_axis_lines() {
        use crate::scene::style::{AxisBounds, GridPlanes, GridSettings, SceneStyle};

        let style = SceneStyle {
            grid: GridSettings {
                planes: GridPlanes::XZ,
                extent: 10.0,
                spacing: 1.0,
                bounds: AxisBounds {
                    y: Some((0.0, 100.0)),
                    ..Default::default()
                },
                ..GridSettings::default()
            },
            show_axes: true,
            ..SceneStyle::default()
        };
        let mut lines = Vec::new();
        build_grid_lines(&style, ColorSpace::SRGB, &mut lines);

        // Axis lines carry the bold width (1.6); grid lines are width 0.
        let axis_span = |pick: fn(&LineInstance) -> bool, comp: usize| {
            let l = lines
                .iter()
                .find(|l| l.width > 0.0 && pick(l))
                .expect("axis line");
            (
                l.start[comp].min(l.end[comp]),
                l.start[comp].max(l.end[comp]),
            )
        };
        // Y axis line spans exactly [0, 100] — no negative dive.
        let y = axis_span(|l| l.start[0] == 0.0 && l.start[2] == 0.0, 1);
        assert_eq!(y, (0.0, 100.0));
        // X axis line stays symmetric (fallback extent.max(spacing).max(1)).
        let x = axis_span(|l| l.start[1] == 0.0 && l.start[2] == 0.0, 0);
        assert_eq!(x, (-10.0, 10.0));

        // The XZ grid plane is unaffected (uses X/Z spans, both symmetric):
        // no grid vertex strays outside ±10 on X or Z.
        for l in lines.iter().filter(|l| l.width == 0.0) {
            for v in [l.start, l.end] {
                assert!(v[0].abs() <= 10.0 + 1e-3 && v[2].abs() <= 10.0 + 1e-3);
            }
        }
    }

    #[test]
    fn gpu_struct_sizes_are_stable() {
        assert_eq!(size_of::<PointUniform>(), 96);
        assert_eq!(size_of::<LineUniform>(), 96);
        assert_eq!(size_of::<MeshUniform>(), 224);
        assert_eq!(size_of::<PointInstance>(), 28);
        assert_eq!(size_of::<LineInstance>(), 44);
        assert_eq!(size_of::<MeshVertexGpu>(), 24);
        assert_eq!(size_of::<CompositeInstance>(), 40);
    }
}
