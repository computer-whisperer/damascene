//! Per-mark styles and scene-level styling: materials, point/line styles,
//! the light rig, the reference grid, and the overall [`SceneStyle`].
//!
//! All colours here are **authoring-space** [`Color`]; the backend
//! converts them to the runner's working linear space at upload (via
//! `crate::paint::rgba_f32_in`), so the scene tracks aetna's colour
//! management and is HDR-ready. Nothing here encodes for output.

use glam::Vec3;

use crate::color::Color;
use crate::shader::{ShaderHandle, UniformBlock};

/// Material for a mesh mark.
///
/// The stock recipes ([`Material::Matte`], [`Material::Flat`]) cover V1.
/// [`Material::Custom`] is carried in the type from day one so adding it
/// is non-breaking, but it is implemented post-V1 (plan M5): an app
/// reskins the fragment via aetna's existing custom-shader path while
/// aetna keeps the vertex layout, buffers, passes, depth, and device.
/// Supplying a custom *pipeline* (not just a material) is `surface()`,
/// not this.
#[derive(Clone, Debug)]
pub enum Material {
    /// Forward-lit diffuse surface, shaded by the [`LightRig`].
    Matte { base: Color },
    /// Unlit constant colour (e.g. emissive markers, schematic fills).
    Flat { color: Color },
    /// App-supplied material shader. Post-V1; see the type docs.
    Custom {
        shader: ShaderHandle,
        uniforms: UniformBlock,
    },
}

impl Default for Material {
    fn default() -> Self {
        Material::Matte {
            base: Color::srgb_u8(214, 220, 230),
        }
    }
}

/// Whether a point size / line width is in screen pixels (constant on
/// screen regardless of zoom) or world units (scales with the scene).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SizeMode {
    #[default]
    ScreenSpace,
    World,
}

/// Marker shape for point marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PointShape {
    #[default]
    Circle,
    Square,
}

/// Style for a point/scatter mark. Per-point colour lives in the geometry
/// ([`crate::scene::ScenePoint`]); this carries size and shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointStyle {
    pub size: f32,
    pub shape: PointShape,
    pub size_mode: SizeMode,
}

impl Default for PointStyle {
    fn default() -> Self {
        Self {
            size: 5.0,
            shape: PointShape::Circle,
            size_mode: SizeMode::ScreenSpace,
        }
    }
}

/// Stroke pattern for line marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LinePattern {
    #[default]
    Solid,
    Dashed,
}

/// Style for a line mark. Per-segment colour lives in the geometry
/// ([`crate::scene::LineSegment`]); this carries width and pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineStyle {
    pub width: f32,
    pub pattern: LinePattern,
    pub size_mode: SizeMode,
}

impl Default for LineStyle {
    fn default() -> Self {
        Self {
            width: 1.5,
            pattern: LinePattern::Solid,
            size_mode: SizeMode::ScreenSpace,
        }
    }
}

/// The fixed, small lighting rig: one directional key light plus a
/// hemispheric ambient term. Closed-scope — enough to make small models
/// read as 3D without a deferred/SSAO pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightRig {
    /// World-space direction **toward** the key light (the `L` in
    /// `dot(N, L)`). Need not be normalised; the backend normalises.
    pub key_direction: Vec3,
    pub key_color: Color,
    pub key_intensity: f32,
    /// Hemispheric ambient term in `[0, 1]`, lifting shadowed faces.
    pub ambient: f32,
}

impl Default for LightRig {
    fn default() -> Self {
        Self {
            key_direction: Vec3::new(0.4, 0.7, 0.2).normalize(),
            key_color: Color::srgb_u8(255, 255, 255),
            key_intensity: 1.0,
            ambient: 0.35,
        }
    }
}

/// Which world planes carry reference grid lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPlanes {
    pub xy: bool,
    pub xz: bool,
    pub yz: bool,
}

impl GridPlanes {
    pub const NONE: GridPlanes = GridPlanes {
        xy: false,
        xz: false,
        yz: false,
    };
    /// The ground plane — the common default for data/model viewers.
    pub const XZ: GridPlanes = GridPlanes {
        xy: false,
        xz: true,
        yz: false,
    };
}

impl Default for GridPlanes {
    fn default() -> Self {
        GridPlanes::XZ
    }
}

/// Reference grid configuration. The backend generates the line geometry
/// from these settings and draws it through the line pipeline; core just
/// carries the settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSettings {
    pub planes: GridPlanes,
    /// World-space distance between major grid lines.
    pub spacing: f32,
    /// Half-size of the grid (lines span `[-extent, extent]`).
    pub extent: f32,
    /// Minor subdivisions between major lines (`1` = none).
    pub subdivisions: u32,
    pub color: Color,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            planes: GridPlanes::default(),
            spacing: 1.0,
            extent: 10.0,
            subdivisions: 1,
            color: Color::srgb_u8a(120, 120, 132, 90),
        }
    }
}

/// Scene-level styling. The working colour space is *not* stored here —
/// it is the runner's, read by the backend at render time so the scene
/// renders in whatever space the UI is in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneStyle {
    pub grid: GridSettings,
    /// Background fill for the scene viewport. `None` leaves it
    /// transparent so the UI behind shows through; `Some` fills it.
    pub background: Option<Color>,
    /// MSAA sample count for the offscreen scene target (`1` or `4`).
    /// Defaults to `4` — small graphs sit next to crisp UI text, so the
    /// scene must be antialiased and resolved before compositing.
    pub msaa_samples: u32,
    /// Draw axis lines/labels.
    pub show_axes: bool,
}

impl Default for SceneStyle {
    fn default() -> Self {
        Self {
            grid: GridSettings::default(),
            background: None,
            msaa_samples: 4,
            show_axes: true,
        }
    }
}

impl SceneStyle {
    /// Conservative world-space bounds of the reference grid + axes, for
    /// sizing the camera's near/far so they're never clipped. `None` when
    /// nothing reference-like is drawn. Returns a cube `[-e, e]³` where `e`
    /// is the largest enabled extent — overestimating the flat planes
    /// slightly, which only widens the depth range harmlessly.
    pub fn reference_extent(&self) -> Option<crate::scene::Aabb> {
        let grid_e = if self.grid.planes != GridPlanes::NONE {
            self.grid.extent.max(0.0)
        } else {
            0.0
        };
        let axis_e = if self.show_axes {
            self.grid.extent.max(self.grid.spacing).max(0.0)
        } else {
            0.0
        };
        let e = grid_e.max(axis_e);
        if e <= 0.0 {
            return None;
        }
        Some(crate::scene::Aabb::from_points([
            glam::Vec3::splat(-e),
            glam::Vec3::splat(e),
        ]))
    }
}
