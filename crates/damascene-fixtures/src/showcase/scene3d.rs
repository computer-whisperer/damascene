//! 3D scene — the `chart3d` widget end-to-end.
//!
//! A representative hardware-accelerated scene: a lit sphere, a colormap-graded
//! point scatter with hover tooltips, a faint translucent shell enclosing the
//! data (material alpha < 1 routes through the two-pass translucent mesh
//! path), three orbit-guide rings, a reference grid, and labelled axes (one
//! remapping a world span to a data range). It is
//! described purely as data (a [`SceneSpec`]) and composited through the core
//! render pipeline with zero host glue — so the *same* page renders on wgpu,
//! vulkano, and ash. Default `Framing::Auto` hands the camera to the library:
//! drag to orbit, shift-drag to pan, wheel to zoom. The buttons request
//! animated focus moves declaratively.
//!
//! Geometry is built once into app-owned handles (in [`State::default`]) and
//! only *referenced* each frame; the backend caches GPU buffers and never
//! re-uploads while the camera moves.

use damascene_core::prelude::*;
use damascene_core::scene::glam::Vec3;
use damascene_core::scene::{
    Aabb, Axes, AxisRange, Colormap, Focus, GridPlanes, GridSettings, LineData, LineSegment,
    LinesHandle, Material, MeshData, MeshHandle, MeshVertex, PointData, PointLabels, PointShape,
    PointStyle, PointsHandle, SceneSpec, SceneStyle, TickFormat,
};

pub struct State {
    mesh: MeshHandle,
    /// Translucent envelope around the data — the issue-#39 "gamut shell"
    /// shape: alpha < 1 renders it see-through (depth-tested, two-sided).
    shell: MeshHandle,
    scatter: PointsHandle,
    /// Per-point hover tooltips for the scatter (built once, cloned per frame).
    scatter_labels: PointLabels,
    rings: LinesHandle,
    /// Combined data bounds — for the "Frame all" focus request.
    bounds: Aabb,
    /// Declarative focus request. Changing it animates the camera; the library
    /// owns the actual pose (default `Framing::Auto`).
    focus: Option<Focus>,
}

impl Default for State {
    fn default() -> Self {
        let mesh = MeshHandle::new(uv_sphere(0.95, 28, 36));
        let shell = MeshHandle::new(uv_sphere(1.85, 20, 28));
        let (scatter_data, labels) = fibonacci_scatter(240, 1.7);
        let scatter = PointsHandle::new(scatter_data);
        // Hover any scatter point to read its value; occluded points (behind
        // the sphere) aren't pickable.
        let scatter_labels = PointLabels::new(labels).on_hover();
        let rings = LinesHandle::new(LineData {
            segments: orbit_rings(1.7, 96),
        });
        let bounds = shell
            .bounds()
            .union(mesh.bounds())
            .union(scatter.bounds())
            .union(rings.bounds());
        Self {
            mesh,
            shell,
            scatter,
            scatter_labels,
            rings,
            bounds,
            focus: None,
        }
    }
}

pub fn view(state: &State) -> El {
    // Size the reference grid to the content (a ~1.7-radius sphere) so the
    // labelled ticks land around the data rather than far out.
    let style = SceneStyle {
        grid: GridSettings {
            planes: GridPlanes::XZ,
            spacing: 0.5,
            extent: 2.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // Rich axis config: X/Z label the raw world coordinate; Y is a titled axis
    // whose world span [-2, 2] is remapped to a 0..100 "Altitude" data range
    // and shown as integers.
    let mut axes = Axes::default();
    axes.x.title = Some("X".into());
    axes.z.title = Some("Z".into());
    axes.y.title = Some("Altitude".into());
    axes.y.range = AxisRange::Linear {
        world_span: Some((-2.0, 2.0)),
        data: (0.0, 100.0),
    };
    axes.y.format = TickFormat::Integer;

    let mut scene = SceneSpec::new()
        .mesh_with(
            state.mesh.clone(),
            Material::Glossy {
                base: Color::srgb_u8(120, 170, 235),
                specular: 0.6,
                shininess: 48.0,
            },
        )
        // Material alpha < 1 selects the translucent mesh path: depth-tested
        // against the opaque sphere but two-sided and see-through, so the
        // scatter reads inside it from any angle.
        .mesh_with(
            state.shell.clone(),
            Material::matte(Color::srgb_u8(140, 185, 255).with_alpha(0.16)),
        )
        .points_labeled(
            state.scatter.clone(),
            PointStyle {
                size: 7.0,
                shape: PointShape::Circle,
                ..Default::default()
            },
            state.scatter_labels.clone(),
        )
        .lines(state.rings.clone())
        .style(style)
        .axes(axes);
    if let Some(focus) = state.focus {
        scene = scene.focus(focus);
    }

    column([
        h1("3D scene"),
        paragraph(
            "The `chart3d` widget renders a hardware-accelerated scene — a lit \
             mesh, a translucent shell around the data, a colormap-graded \
             point scatter, orbit-guide lines, a reference grid, and labelled \
             axes — from a backend-neutral `SceneSpec`. No `surface()`, no \
             app-owned device: the same description composites identically on \
             wgpu, vulkano, and ash.",
        )
        .muted(),
        text("drag to orbit · shift-drag to pan · wheel to zoom · hover a point")
            .small()
            .muted()
            .wrap_text(),
        // Fills the remaining height of the content panel (chart3d defaults to
        // Size::Fill on both axes).
        chart3d(scene),
        row([
            button("Frame all").key("scene3d-frame").secondary(),
            button("Focus top").key("scene3d-focus-top").secondary(),
            button("Focus side").key("scene3d-focus-side").secondary(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_3)
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    // Each button sets a distinct focus request; the camera springs there from
    // wherever the user left it. Re-clicking the same button after dragging is
    // a no-op — focus animates on *change*.
    if e.is_click_or_activate("scene3d-frame") {
        state.focus = Some(Focus::Bounds(state.bounds));
    } else if e.is_click_or_activate("scene3d-focus-top") {
        state.focus = Some(Focus::Point {
            target: Vec3::new(0.0, 1.7, 0.0),
            distance: 3.0,
        });
    } else if e.is_click_or_activate("scene3d-focus-side") {
        state.focus = Some(Focus::Point {
            target: Vec3::new(1.7, 0.0, 0.0),
            distance: 3.0,
        });
    }
}

/// UV sphere, smooth (position-direction) normals. CCW outward winding —
/// validated by every backend's `uv_sphere_winds_outward` render test.
/// Crate-visible: the hero fixture's orbit view builds its planet from it.
pub(crate) fn uv_sphere(radius: f32, rings: u32, sectors: u32) -> MeshData {
    use std::f32::consts::{PI, TAU};
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=rings {
        let theta = i as f32 / rings as f32 * PI;
        let (st, ct) = theta.sin_cos();
        for j in 0..=sectors {
            let phi = j as f32 / sectors as f32 * TAU;
            let (sp, cp) = phi.sin_cos();
            let n = Vec3::new(st * cp, ct, st * sp);
            vertices.push(MeshVertex {
                position: n * radius,
                normal: n,
            });
        }
    }
    let stride = sectors + 1;
    for i in 0..rings {
        for j in 0..sectors {
            let a = i * stride + j;
            let b = a + stride;
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    MeshData {
        vertices,
        indices: Some(indices),
    }
}

/// `n` points spread evenly on a sphere (Fibonacci lattice), colour-mapped by
/// height through a perceptual colormap, plus a per-point label of that height
/// for hover tooltips.
fn fibonacci_scatter(n: usize, radius: f32) -> (PointData, Vec<String>) {
    const GOLDEN_ANGLE: f32 = 2.399_963_2;
    let positions: Vec<Vec3> = (0..n)
        .map(|i| {
            let t = (i as f32 + 0.5) / n as f32;
            let y = 1.0 - 2.0 * t;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let phi = i as f32 * GOLDEN_ANGLE;
            Vec3::new(r * phi.cos(), y, r * phi.sin()) * radius
        })
        .collect();
    let heights: Vec<f32> = positions.iter().map(|p| p.y).collect();
    let labels: Vec<String> = heights.iter().map(|h| format!("y = {h:.2}")).collect();
    let data = PointData::from_values(positions, heights, (-radius, radius), Colormap::Viridis);
    (data, labels)
}

/// Three great-circle orbit guides (one per coordinate plane), each a closed
/// loop of `segments` faint line segments.
fn orbit_rings(radius: f32, segments: usize) -> Vec<LineSegment> {
    let color = [0.75, 0.78, 0.85, 0.35];
    let mut out = Vec::new();
    for (u, v) in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
        for s in 0..segments {
            let a = s as f32 / segments as f32 * std::f32::consts::TAU;
            let b = (s + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let p = |ang: f32| (u * ang.cos() + v * ang.sin()) * radius;
            out.push(LineSegment {
                start: p(a),
                end: p(b),
                color,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_buttons_set_distinct_requests() {
        let mut s = State::default();
        assert!(s.focus.is_none());

        on_event(&mut s, UiEvent::synthetic_click("scene3d-focus-top"));
        assert!(matches!(s.focus, Some(Focus::Point { .. })));

        on_event(&mut s, UiEvent::synthetic_click("scene3d-frame"));
        assert!(matches!(s.focus, Some(Focus::Bounds(_))));
    }

    #[test]
    fn unrelated_event_leaves_focus_untouched() {
        let mut s = State::default();
        on_event(&mut s, UiEvent::synthetic_click("nav-palette"));
        assert!(s.focus.is_none());
    }
}
