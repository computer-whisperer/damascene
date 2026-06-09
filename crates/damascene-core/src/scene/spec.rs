//! `SceneSpec`: the author-facing description a [`chart3d`](crate::tree::chart3d)
//! element carries until [`draw_ops`](mod@crate::draw_ops) resolves it into a
//! [`Scene3DData`](crate::scene::Scene3DData).
//!
//! A small builder so apps describe a scene declaratively:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//! use damascene_core::scene::SceneSpec;
//!
//! let scene = SceneSpec::new()
//!     .points(scatter_handle)        // PointsHandle, default style
//!     .mesh(model_handle)            // MeshHandle, default material
//!     .grid(GridPlanes::XZ);
//! let el = chart3d(scene).key("scene");
//! ```
//!
//! Marks are added with `points` / `mesh` / `lines` (default style) or the
//! `add_*` variants for a fully-specified draw (transform, material,
//! per-mark style). Per-point and per-segment colour live in the geometry
//! handles, not here.

use glam::Mat4;

use crate::color::Color;
use crate::scene::axes::{Axes, AxisKind};
use crate::scene::camera::{CameraControls, CameraState, Focus, Framing};
use crate::scene::data::{LineDraw, MeshDraw, PointDraw};
use crate::scene::geometry::{LinesHandle, MeshHandle, PointsHandle};
use crate::scene::style::{GridPlanes, LightRig, LineStyle, Material, PointStyle, SceneStyle};

/// Declarative scene description: the marks plus styling. Resolved into a
/// [`Scene3DData`](crate::scene::Scene3DData) at draw time (the camera is
/// auto-framed against the marks' combined bounds, or taken from
/// [`SceneSpec::camera`] when set).
#[derive(Clone, Debug, Default)]
pub struct SceneSpec {
    pub meshes: Vec<MeshDraw>,
    pub points: Vec<PointDraw>,
    pub lines: Vec<LineDraw>,
    pub lights: LightRig,
    pub style: SceneStyle,
    /// App-supplied camera pose. With [`Framing::Manual`] it is used
    /// verbatim; with `Auto`/`Fit` its orbit angles seed the framed view.
    /// `None` uses a default three-quarter pose.
    pub camera: Option<CameraState>,
    /// How the camera relates to the data bounds. Defaults to
    /// [`Framing::Auto`] (fit, then free).
    pub framing: Framing,
    /// Declarative focus request. Changing it animates the camera to the
    /// new viewpoint (under `Auto`/`Fit`). `None` leaves framing/gestures
    /// in charge.
    pub focus: Option<Focus>,
    /// Pointer navigation scheme (default [`CameraControls::Orbit`]).
    pub controls: CameraControls,
    /// Axis tick/title labels. `None` (the default) draws no labels;
    /// `Some` projects labels through the camera at draw time. Independent
    /// of [`SceneStyle::show_axes`], which draws the axis *lines*.
    pub axes: Option<Axes>,
}

impl SceneSpec {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a point/scatter mark with the default style.
    pub fn points(mut self, handle: PointsHandle) -> Self {
        self.points.push(PointDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            style: PointStyle::default(),
            labels: None,
        });
        self
    }

    /// Add a point mark with an explicit style.
    pub fn points_styled(mut self, handle: PointsHandle, style: PointStyle) -> Self {
        self.points.push(PointDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            style,
            labels: None,
        });
        self
    }

    /// Add a point mark with per-point labels / hover tooltips.
    pub fn points_labeled(
        mut self,
        handle: PointsHandle,
        style: PointStyle,
        labels: crate::scene::labels::PointLabels,
    ) -> Self {
        self.points.push(PointDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            style,
            labels: Some(labels),
        });
        self
    }

    /// Add a fully-specified point mark (transform + style).
    pub fn add_points(mut self, draw: PointDraw) -> Self {
        self.points.push(draw);
        self
    }

    /// Add a mesh mark with the default material.
    pub fn mesh(mut self, handle: MeshHandle) -> Self {
        self.meshes.push(MeshDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            material: Material::default(),
        });
        self
    }

    /// Add a mesh mark with an explicit material.
    pub fn mesh_with(mut self, handle: MeshHandle, material: Material) -> Self {
        self.meshes.push(MeshDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            material,
        });
        self
    }

    /// Add a fully-specified mesh mark (transform + material).
    pub fn add_mesh(mut self, draw: MeshDraw) -> Self {
        self.meshes.push(draw);
        self
    }

    /// Add a line mark with the default style.
    pub fn lines(mut self, handle: LinesHandle) -> Self {
        self.lines.push(LineDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            style: Default::default(),
        });
        self
    }

    /// Add a line mark with an explicit style.
    pub fn lines_styled(mut self, handle: LinesHandle, style: LineStyle) -> Self {
        self.lines.push(LineDraw {
            geometry: handle,
            transform: Mat4::IDENTITY,
            style,
        });
        self
    }

    /// Add a fully-specified line mark (transform + style).
    pub fn add_lines(mut self, draw: LineDraw) -> Self {
        self.lines.push(draw);
        self
    }

    /// Set which planes carry the reference grid.
    pub fn grid(mut self, planes: GridPlanes) -> Self {
        self.style.grid.planes = planes;
        self
    }

    /// Turn the reference grid off.
    pub fn no_grid(mut self) -> Self {
        self.style.grid.planes = GridPlanes::NONE;
        self
    }

    /// Clip one axis to an explicit world `[min, max]`, overriding the
    /// symmetric grid `extent` for that axis alone. The grid plane lines, the
    /// axis line, and the axis's ticks/title all honour the bound; the other
    /// axes stay symmetric. Useful for a naturally one-sided axis — e.g. CIE
    /// L\* ∈ [0, 100], which should never draw a negative half.
    pub fn axis_bounds(mut self, axis: AxisKind, min: f32, max: f32) -> Self {
        let b = &mut self.style.grid.bounds;
        match axis {
            AxisKind::X => b.x = Some((min, max)),
            AxisKind::Y => b.y = Some((min, max)),
            AxisKind::Z => b.z = Some((min, max)),
        }
        self
    }

    /// Fill the scene background (default leaves it transparent).
    pub fn background(mut self, color: Color) -> Self {
        self.style.background = Some(color);
        self
    }

    /// Replace the whole style block.
    pub fn style(mut self, style: SceneStyle) -> Self {
        self.style = style;
        self
    }

    /// Replace the light rig.
    pub fn lights(mut self, rig: LightRig) -> Self {
        self.lights = rig;
        self
    }

    /// Supply an explicit camera pose. With [`Framing::Manual`] it is the
    /// authoritative pose; otherwise its orbit angles seed the framed view.
    pub fn camera(mut self, state: CameraState) -> Self {
        self.camera = Some(state);
        self
    }

    /// Set how the camera relates to the data bounds (default
    /// [`Framing::Auto`]).
    pub fn framing(mut self, framing: Framing) -> Self {
        self.framing = framing;
        self
    }

    /// Request the camera animate to a [`Focus`]. Changing the value
    /// across rebuilds triggers a smooth move; passing the same value is a
    /// no-op.
    pub fn focus(mut self, focus: Focus) -> Self {
        self.focus = Some(focus);
        self
    }

    /// Choose the pointer navigation scheme (default
    /// [`CameraControls::Orbit`]).
    pub fn controls(mut self, controls: CameraControls) -> Self {
        self.controls = controls;
        self
    }

    /// Enable axis labels with the given configuration.
    pub fn axes(mut self, axes: Axes) -> Self {
        self.axes = Some(axes);
        self
    }

    /// Enable axis labels with default styling and the three titles set.
    pub fn axis_titles(
        mut self,
        x: impl Into<String>,
        y: impl Into<String>,
        z: impl Into<String>,
    ) -> Self {
        self.axes = Some(Axes::titles(x, y, z));
        self
    }
}
