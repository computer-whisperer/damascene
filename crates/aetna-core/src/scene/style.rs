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
/// The stock recipes ([`Material::Matte`], [`Material::Glossy`],
/// [`Material::Flat`]) cover V1. [`Material::Custom`] is carried in the type
/// from day one so adding it is non-breaking, but it is implemented post-V1
/// (plan M5): an app reskins the fragment via aetna's existing custom-shader
/// path while aetna keeps the vertex layout, buffers, passes, depth, and
/// device. Supplying a custom *pipeline* (not just a material) is `surface()`,
/// not this.
#[derive(Clone, Debug)]
pub enum Material {
    /// Forward-lit diffuse surface, shaded by the [`LightRig`].
    Matte { base: Color },
    /// Forward-lit diffuse surface with a Blinn-Phong specular highlight, for
    /// a glossier read. The highlight takes the key light's colour.
    Glossy {
        base: Color,
        /// Highlight strength, `[0, 1]`. `0` is matte.
        specular: f32,
        /// Phong exponent: higher is a tighter, glassier highlight (clamped
        /// to `>= 1`).
        shininess: f32,
    },
    /// Unlit constant colour (e.g. emissive markers, schematic fills).
    Flat { color: Color },
    /// App-supplied material shader. Post-V1; see the type docs.
    Custom {
        shader: ShaderHandle,
        uniforms: UniformBlock,
    },
}

impl Material {
    /// Forward-lit diffuse surface.
    pub fn matte(base: Color) -> Self {
        Material::Matte { base }
    }

    /// Unlit constant colour.
    pub fn flat(color: Color) -> Self {
        Material::Flat { color }
    }

    /// Forward-lit with a moderate specular highlight (`specular = 0.5`,
    /// `shininess = 32`). Tune the fields directly for a sharper or softer
    /// gloss.
    pub fn glossy(base: Color) -> Self {
        Material::Glossy {
            base,
            specular: 0.5,
            shininess: 32.0,
        }
    }
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
/// hemispheric ambient fill. Closed-scope — enough to make small models read
/// as 3D without a deferred/SSAO pass.
///
/// The ambient term is **hemispheric**: upward-facing surfaces pick up
/// [`sky_color`](Self::sky_color), downward-facing ones
/// [`ground_color`](Self::ground_color), blended by the surface normal's
/// vertical component and scaled by [`ambient`](Self::ambient). Set sky and
/// ground equal for a flat ambient.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightRig {
    /// World-space direction **toward** the key light (the `L` in
    /// `dot(N, L)`). Need not be normalised; the backend normalises.
    pub key_direction: Vec3,
    pub key_color: Color,
    pub key_intensity: f32,
    /// Hemispheric ambient seen by upward-facing surfaces (the "sky").
    pub sky_color: Color,
    /// Hemispheric ambient seen by downward-facing surfaces (the "ground").
    pub ground_color: Color,
    /// Overall scale of the hemispheric ambient fill, `[0, 1]`, lifting
    /// shadowed faces.
    pub ambient: f32,
}

impl Default for LightRig {
    fn default() -> Self {
        Self {
            key_direction: Vec3::new(0.4, 0.7, 0.2).normalize(),
            key_color: Color::srgb_u8(255, 255, 255),
            key_intensity: 1.0,
            sky_color: Color::srgb_u8(236, 242, 255),
            ground_color: Color::srgb_u8(140, 144, 150),
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

/// Optional per-axis world bounds for the reference grid, axis lines, and
/// ticks. When an axis is `Some((min, max))`, the grid plane lines spanning
/// it, that axis's line, and its ticks/title are clipped to `[min, max]`
/// instead of the symmetric `[-extent, extent]`; `None` falls back to the
/// symmetric span. Lets a naturally one-sided axis (e.g. CIE L\* ∈ [0, 100])
/// bound the drawn space to where data can actually live.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct AxisBounds {
    pub x: Option<(f32, f32)>,
    pub y: Option<(f32, f32)>,
    pub z: Option<(f32, f32)>,
}

impl AxisBounds {
    /// The bound for axis `i` (0 = X, 1 = Y, 2 = Z), if set.
    pub(crate) fn axis(&self, i: usize) -> Option<(f32, f32)> {
        match i {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => None,
        }
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
    /// Half-size of the grid: the symmetric `[-extent, extent]` span used by
    /// any axis without an explicit [`bounds`](Self::bounds) entry.
    pub extent: f32,
    /// Minor subdivisions between major lines (`1` = none).
    pub subdivisions: u32,
    pub color: Color,
    /// Optional per-axis world bounds overriding the symmetric `extent`.
    pub bounds: AxisBounds,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            planes: GridPlanes::default(),
            spacing: 1.0,
            extent: 10.0,
            subdivisions: 1,
            color: Color::srgb_u8a(120, 120, 132, 90),
            bounds: AxisBounds::default(),
        }
    }
}

impl GridSettings {
    /// Effective world `[min, max]` for each axis (X, Y, Z): the per-axis
    /// [`bounds`](Self::bounds) entry when set, else the symmetric
    /// `[-extent, extent]`. Each span is normalised so `min <= max`. This is
    /// the single source of truth read by both the GPU grid/axis-line
    /// generation and the CPU-side tick/title labelling.
    pub(crate) fn axis_spans(&self) -> [(f32, f32); 3] {
        let e = self.extent.max(0.0);
        let fb = (-e, e);
        std::array::from_fn(|i| {
            let (a, b) = self.bounds.axis(i).unwrap_or(fb);
            (a.min(b), a.max(b))
        })
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
    /// World-space bounds of the reference grid + axes, for sizing the
    /// camera's near/far so they're never clipped. `None` when nothing
    /// reference-like is drawn. Builds the actual (possibly asymmetric) box
    /// from the per-axis spans, so a bounded tall axis (e.g. L\* ∈ [0, 100])
    /// stays enclosed; slight overestimation of the flat planes only widens
    /// the depth range harmlessly.
    pub fn reference_extent(&self) -> Option<crate::scene::Aabb> {
        let draws_grid = self.grid.planes != GridPlanes::NONE && self.grid.extent.max(0.0) > 0.0;
        let draws_axes = self.show_axes;
        if !draws_grid && !draws_axes {
            return None;
        }
        let spans = self.grid.axis_spans();
        // Axis lines fall back to a slightly larger reach than the grid
        // (`extent.max(spacing).max(1)`) so a tiny/zero grid still shows
        // unit axes; an explicit bound governs both.
        let axis_fallback = self.grid.extent.max(self.grid.spacing).max(1.0);
        let mut lo = [0.0f32; 3];
        let mut hi = [0.0f32; 3];
        for i in 0..3 {
            let (mut amin, mut amax) = (0.0f32, 0.0f32);
            if let Some((bmin, bmax)) = self.grid.bounds.axis(i) {
                amin = amin.min(bmin.min(bmax));
                amax = amax.max(bmin.max(bmax));
            } else {
                if draws_grid {
                    amin = amin.min(spans[i].0);
                    amax = amax.max(spans[i].1);
                }
                if draws_axes {
                    amin = amin.min(-axis_fallback);
                    amax = amax.max(axis_fallback);
                }
            }
            lo[i] = amin;
            hi[i] = amax;
        }
        let aabb = crate::scene::Aabb::from_points([
            glam::Vec3::from_array(lo),
            glam::Vec3::from_array(hi),
        ]);
        aabb.is_valid().then_some(aabb)
    }
}
