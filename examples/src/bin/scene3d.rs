//! Scene3D — a small, polished 3D widget inside an ordinary Aetna app.
//!
//! Demonstrates the `chart3d` widget: a lit mesh, a colour-graded point
//! scatter, orbit-guide lines, a reference grid + axes — composited through
//! the core render pipeline with zero host glue (no `surface()`, no manual
//! GPU). The buttons drive the camera through the public `CameraState`, the
//! same state pointer-orbit will mutate once that lands.
//!
//! Geometry is built once into app-owned handles and merely *referenced*
//! every frame; the backend caches GPU buffers and never re-uploads while
//! the camera moves.
//!
//! Run: `cargo run -p aetna-examples --bin scene3d`

use aetna_core::prelude::*;
use aetna_core::scene::glam::Vec3;
use aetna_core::scene::{
    Aabb, Focus, GridPlanes, LineData, LineSegment, LinesHandle, Material, MeshData, MeshHandle,
    MeshVertex, PointData, PointShape, PointStyle, PointsHandle, SceneSpec, ScenePoint,
};

struct Scene3DDemo {
    mesh: MeshHandle,
    scatter: PointsHandle,
    rings: LinesHandle,
    /// Combined data bounds — for the "Frame all" focus request.
    bounds: Aabb,
    /// Declarative focus request. Changing it animates the camera; the
    /// library owns the actual pose (default `Framing::Auto`).
    focus: Option<Focus>,
}

impl Default for Scene3DDemo {
    fn default() -> Self {
        let mesh = MeshHandle::new(uv_sphere(0.95, 28, 36));
        let scatter = PointsHandle::new(PointData { points: fibonacci_scatter(240, 1.7) });
        let rings = LinesHandle::new(LineData { segments: orbit_rings(1.7, 96) });
        let bounds = mesh.bounds().union(scatter.bounds()).union(rings.bounds());
        Self { mesh, scatter, rings, bounds, focus: None }
    }
}

impl App for Scene3DDemo {
    fn build(&self, _cx: &BuildCx) -> El {
        let mut scene = SceneSpec::new()
            .mesh_with(
                self.mesh.clone(),
                Material::Matte { base: Color::srgb_u8(120, 170, 235) },
            )
            .points_styled(
                self.scatter.clone(),
                PointStyle { size: 7.0, shape: PointShape::Circle, ..Default::default() },
            )
            .lines(self.rings.clone())
            .grid(GridPlanes::XZ);
        // No background: the scene composites directly over whatever Aetna
        // painted behind it. Default `Framing::Auto` means the *library*
        // owns the camera — drag to orbit, shift-drag to pan, wheel to
        // zoom, all with zero app glue. The buttons below request animated
        // focus moves declaratively.
        if let Some(focus) = self.focus {
            scene = scene.focus(focus);
        }

        column([
            row([
                h2("Scene3D"),
                spacer(),
                text("drag to orbit · shift-drag to pan · wheel to zoom").muted(),
            ])
            .align(Align::Center),
            // The 3D widget fills the remaining space.
            chart3d(scene),
            row([
                button("Frame all").key("frame").secondary(),
                button("Focus top").key("focus_top").secondary(),
                button("Focus side").key("focus_side").secondary(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4)
    }

    fn on_event(&mut self, event: UiEvent) {
        // Each sets a distinct focus request; the camera springs there
        // from wherever the user left it. (Re-clicking the same button
        // after dragging is a no-op — focus animates on *change*.)
        if event.is_click_or_activate("frame") {
            self.focus = Some(Focus::Bounds(self.bounds));
        } else if event.is_click_or_activate("focus_top") {
            self.focus = Some(Focus::Point { target: Vec3::new(0.0, 1.7, 0.0), distance: 3.0 });
        } else if event.is_click_or_activate("focus_side") {
            self.focus = Some(Focus::Point { target: Vec3::new(1.7, 0.0, 0.0), distance: 3.0 });
        }
    }
}

/// UV sphere, smooth (position-direction) normals. CCW outward winding —
/// validated by `aetna-wgpu`'s `uv_sphere_winds_outward` render test.
fn uv_sphere(radius: f32, rings: u32, sectors: u32) -> MeshData {
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
            vertices.push(MeshVertex { position: n * radius, normal: n });
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
    MeshData { vertices, indices: Some(indices) }
}

/// `n` points spread evenly on a sphere (Fibonacci lattice), colour-graded
/// by height. Colours are authoring-space sRGBA; the backend converts.
fn fibonacci_scatter(n: usize, radius: f32) -> Vec<ScenePoint> {
    const GOLDEN_ANGLE: f32 = 2.399_963_2;
    let lo = [0.25, 0.55, 0.95, 1.0]; // cool blue at the bottom
    let hi = [0.97, 0.42, 0.68, 1.0]; // warm pink at the top
    (0..n)
        .map(|i| {
            let t = (i as f32 + 0.5) / n as f32;
            let y = 1.0 - 2.0 * t;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let phi = i as f32 * GOLDEN_ANGLE;
            let position = Vec3::new(r * phi.cos(), y, r * phi.sin()) * radius;
            let k = t; // 0 at top .. 1 at bottom
            let color = std::array::from_fn(|c| hi[c] + (lo[c] - hi[c]) * k);
            ScenePoint { position, color }
        })
        .collect()
}

/// Three great-circle orbit guides (one per coordinate plane), each a
/// closed loop of `segments` faint line segments.
fn orbit_rings(radius: f32, segments: usize) -> Vec<LineSegment> {
    let color = [0.75, 0.78, 0.85, 0.35];
    let mut out = Vec::new();
    for (u, v) in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
        for s in 0..segments {
            let a = s as f32 / segments as f32 * std::f32::consts::TAU;
            let b = (s + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let p = |ang: f32| (u * ang.cos() + v * ang.sin()) * radius;
            out.push(LineSegment { start: p(a), end: p(b), color });
        }
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 680.0);
    aetna_winit_wgpu::run("Aetna — Scene3D", viewport, Scene3DDemo::default())
}
