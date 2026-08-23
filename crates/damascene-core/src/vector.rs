//! Backend-agnostic SVG/vector asset IR.
//!
//! `usvg` owns SVG normalization: XML, inherited style, transforms,
//! arcs, relative commands, and basic shapes are resolved before Damascene
//! stores anything. The renderer-facing IR below is deliberately small:
//! paths plus fill/stroke style. Backends can tessellate it with lyon or
//! feed it into more specialized vector shaders later.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;

use crate::color::ColorSpace;
use crate::paint::rgba_f32_in;
use crate::tree::Color;

use bytemuck::{Pod, Zeroable};
use i_overlay::core::fill_rule::FillRule as OverlayFillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions, StrokeTessellator,
    StrokeVertex,
};
use usvg::tiny_skia_path;

/// A parsed, backend-agnostic vector asset: an SVG `viewBox` plus styled
/// paths and a gradient side-table. Produced by [`parse_svg_asset`] or
/// composed programmatically via [`VectorAsset::from_paths`] /
/// [`PathBuilder`].
#[derive(Clone, Debug, PartialEq)]
pub struct VectorAsset {
    /// SVG `viewBox` as `[min_x, min_y, width, height]`. All path
    /// coordinates are absolute within this space.
    pub view_box: [f32; 4],
    /// Styled paths in document (paint) order, with transforms and basic
    /// shapes already flattened by usvg.
    pub paths: Vec<VectorPath>,
    /// Gradient table referenced by [`VectorColor::Gradient`] indices. Kept
    /// as a side-table so [`VectorColor`] stays `Copy`.
    pub gradients: Vec<VectorGradient>,
    /// Clip-path table referenced by [`VectorPath::clip`] indices. A
    /// side-table like [`Self::gradients`], so one region can clip many
    /// paths. Resolved into plain geometry by [`VectorAsset::flatten_clips`]
    /// before rasterisation.
    pub clips: Vec<VectorClip>,
}

/// Render policy for app-supplied [`VectorAsset`]s.
///
/// `Painted` preserves authored fills, strokes, gradients, and
/// `currentColor` paint, so backends use the colour-aware vector path.
/// `Mask` treats the asset as coverage geometry and applies one caller-
/// supplied colour, which lets backends use their MSDF atlas path.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VectorRenderMode {
    /// Render authored fills, strokes, gradients, and `currentColor` paint.
    #[default]
    Painted,
    /// Treat the asset as coverage geometry painted in one colour.
    Mask {
        /// The single colour applied to the asset's coverage.
        color: Color,
    },
}

impl VectorRenderMode {
    /// Resolve the mask colour (if any) through `palette`; `Painted` is
    /// returned unchanged.
    pub fn resolved_palette(self, palette: &crate::palette::Palette) -> Self {
        match self {
            Self::Painted => Self::Painted,
            Self::Mask { color } => Self::Mask {
                color: palette.resolve(color),
            },
        }
    }
}

impl VectorAsset {
    /// Build a [`VectorAsset`] from a list of paths and an explicit view
    /// box, without going through SVG parsing. The companion to
    /// [`PathBuilder`] for apps that compose vector content
    /// programmatically (commit-graph curves, Gantt connectors, custom
    /// chart marks). Equivalent to setting the public fields directly,
    /// but documents the construction site and keeps the gradient table
    /// empty by default.
    pub fn from_paths(view_box: [f32; 4], paths: Vec<VectorPath>) -> Self {
        Self {
            view_box,
            paths,
            gradients: Vec::new(),
            clips: Vec::new(),
        }
    }

    /// Whether any path's fill or stroke uses a gradient.
    pub fn has_gradient(&self) -> bool {
        self.paths.iter().any(|p| {
            p.fill
                .map(|f| matches!(f.color, VectorColor::Gradient(_)))
                .unwrap_or(false)
                || p.stroke
                    .map(|s| matches!(s.color, VectorColor::Gradient(_)))
                    .unwrap_or(false)
        })
    }

    /// Return this asset with every solid color resolved through
    /// `palette`. Token names are preserved by palette resolution, so
    /// subsequent palette swaps can resolve the same source asset again
    /// while the resolved RGBA still participates in atlas identity.
    pub fn resolved_palette(&self, palette: &crate::palette::Palette) -> Self {
        let mut out = self.clone();
        for path in &mut out.paths {
            if let Some(fill) = &mut path.fill {
                fill.color = resolve_vector_color(fill.color, palette);
            }
            if let Some(stroke) = &mut path.stroke {
                stroke.color = resolve_vector_color(stroke.color, palette);
            }
        }
        out
    }

    /// Stable content-hash used as a cache key in MSDF / mesh atlases.
    /// Two assets with identical view box, paths, fills, strokes, and
    /// gradients hash to the same value — backends dedupe rasterised
    /// MSDF / tessellated mesh entries on this so an app that builds
    /// the same curve shape twice (e.g. two commits sharing a merge
    /// connector geometry) shares one atlas slot.
    ///
    /// Floats hash via [`f32::to_bits`] — bitwise-equal-but-arithmetically-
    /// equal cases (`-0.0` vs `0.0`, `NaN` payloads) are treated as
    /// distinct, which matches what the atlas cache should do anyway.
    pub fn content_hash(&self) -> u64 {
        use std::hash::Hasher;
        let mut h = StableHasher::new();
        hash_view_box(&mut h, self.view_box);
        write_len(&mut h, self.paths.len());
        for path in &self.paths {
            hash_path(&mut h, path);
        }
        write_len(&mut h, self.gradients.len());
        for grad in &self.gradients {
            hash_gradient(&mut h, grad);
        }
        write_len(&mut h, self.clips.len());
        for clip in &self.clips {
            hash_clip(&mut h, clip);
        }
        h.finish()
    }
}

fn resolve_vector_color(color: VectorColor, palette: &crate::palette::Palette) -> VectorColor {
    match color {
        VectorColor::Solid(c) => VectorColor::Solid(palette.resolve(c)),
        VectorColor::CurrentColor | VectorColor::Gradient(_) => color,
    }
}

/// A small fixed FNV-1a hasher for persistent-ish vector content
/// identity. `DefaultHasher` is intentionally not specified by std;
/// this keeps `VectorAsset::content_hash` deterministic across toolchain
/// runs and target architectures.
struct StableHasher {
    state: u64,
}

impl StableHasher {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self {
            state: Self::OFFSET,
        }
    }
}

impl std::hash::Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= *byte as u64;
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

fn write_len(h: &mut impl std::hash::Hasher, len: usize) {
    h.write_u64(len as u64);
}

fn hash_str(h: &mut impl std::hash::Hasher, value: &str) {
    write_len(h, value.len());
    h.write(value.as_bytes());
}

fn hash_view_box(h: &mut impl std::hash::Hasher, vb: [f32; 4]) {
    for v in vb {
        h.write_u32(v.to_bits());
    }
}

fn hash_path(h: &mut impl std::hash::Hasher, path: &VectorPath) {
    write_len(h, path.segments.len());
    for seg in &path.segments {
        hash_segment(h, seg);
    }
    match path.fill {
        Some(f) => {
            h.write_u8(1);
            hash_fill(h, f);
        }
        None => h.write_u8(0),
    }
    match path.stroke {
        Some(s) => {
            h.write_u8(1);
            hash_stroke(h, s);
        }
        None => h.write_u8(0),
    }
    match path.clip {
        Some(idx) => {
            h.write_u8(1);
            h.write_u32(idx);
        }
        None => h.write_u8(0),
    }
}

fn hash_clip(h: &mut impl std::hash::Hasher, clip: &VectorClip) {
    write_len(h, clip.shapes.len());
    for shape in &clip.shapes {
        write_len(h, shape.segments.len());
        for seg in &shape.segments {
            hash_segment(h, seg);
        }
        h.write_u8(match shape.rule {
            VectorFillRule::NonZero => 0,
            VectorFillRule::EvenOdd => 1,
        });
    }
    match clip.parent {
        Some(p) => {
            h.write_u8(1);
            h.write_u32(p);
        }
        None => h.write_u8(0),
    }
}

fn hash_segment(h: &mut impl std::hash::Hasher, seg: &VectorSegment) {
    match *seg {
        VectorSegment::MoveTo(p) => {
            h.write_u8(0);
            hash_pt(h, p);
        }
        VectorSegment::LineTo(p) => {
            h.write_u8(1);
            hash_pt(h, p);
        }
        VectorSegment::QuadTo(c, p) => {
            h.write_u8(2);
            hash_pt(h, c);
            hash_pt(h, p);
        }
        VectorSegment::CubicTo(c1, c2, p) => {
            h.write_u8(3);
            hash_pt(h, c1);
            hash_pt(h, c2);
            hash_pt(h, p);
        }
        VectorSegment::Close => h.write_u8(4),
    }
}

fn hash_pt(h: &mut impl std::hash::Hasher, p: [f32; 2]) {
    h.write_u32(p[0].to_bits());
    h.write_u32(p[1].to_bits());
}

fn hash_fill(h: &mut impl std::hash::Hasher, f: VectorFill) {
    hash_color(h, f.color);
    h.write_u32(f.opacity.to_bits());
    h.write_u8(match f.rule {
        VectorFillRule::NonZero => 0,
        VectorFillRule::EvenOdd => 1,
    });
}

fn hash_stroke(h: &mut impl std::hash::Hasher, s: VectorStroke) {
    hash_color(h, s.color);
    h.write_u32(s.opacity.to_bits());
    h.write_u32(s.width.to_bits());
    h.write_u8(match s.line_cap {
        VectorLineCap::Butt => 0,
        VectorLineCap::Round => 1,
        VectorLineCap::Square => 2,
    });
    h.write_u8(match s.line_join {
        VectorLineJoin::Miter => 0,
        VectorLineJoin::MiterClip => 1,
        VectorLineJoin::Round => 2,
        VectorLineJoin::Bevel => 3,
    });
    h.write_u32(s.miter_limit.to_bits());
}

fn hash_color(h: &mut impl std::hash::Hasher, c: VectorColor) {
    match c {
        VectorColor::CurrentColor => h.write_u8(0),
        VectorColor::Solid(col) => {
            h.write_u8(1);
            h.write_u32(col.r.to_bits());
            h.write_u32(col.g.to_bits());
            h.write_u32(col.b.to_bits());
            h.write_u32(col.a.to_bits());
            // The space participates in identity — a color authored in
            // BT.2020 vs sRGB hashes distinctly even at the same numeric
            // channel values.
            std::hash::Hash::hash(&col.space, h);
            // The token name participates in identity — the same rgba
            // resolved from different tokens (e.g. a hard-coded
            // overlay vs `tokens::ACCENT`) should still be one cache
            // entry post-resolve, but the *unresolved* asset hashes
            // distinctly so palette swaps invalidate cleanly.
            match col.token {
                Some(name) => {
                    h.write_u8(1);
                    hash_str(h, name);
                }
                None => h.write_u8(0),
            }
        }
        VectorColor::Gradient(idx) => {
            h.write_u8(2);
            h.write_u32(idx);
        }
    }
}

fn hash_gradient(h: &mut impl std::hash::Hasher, g: &VectorGradient) {
    match g {
        VectorGradient::Linear(lin) => {
            h.write_u8(0);
            hash_pt(h, lin.p1);
            hash_pt(h, lin.p2);
            hash_stops(h, &lin.stops);
            hash_spread(h, lin.spread);
            for v in lin.absolute_to_local {
                h.write_u32(v.to_bits());
            }
        }
        VectorGradient::Radial(rad) => {
            h.write_u8(1);
            hash_pt(h, rad.center);
            h.write_u32(rad.radius.to_bits());
            hash_pt(h, rad.focal);
            h.write_u32(rad.focal_radius.to_bits());
            hash_stops(h, &rad.stops);
            hash_spread(h, rad.spread);
            for v in rad.absolute_to_local {
                h.write_u32(v.to_bits());
            }
        }
    }
}

fn hash_stops(h: &mut impl std::hash::Hasher, stops: &[VectorGradientStop]) {
    write_len(h, stops.len());
    for stop in stops {
        h.write_u32(stop.offset.to_bits());
        for c in stop.color {
            h.write_u32(c.to_bits());
        }
    }
}

fn hash_spread(h: &mut impl std::hash::Hasher, s: VectorSpreadMethod) {
    h.write_u8(match s {
        VectorSpreadMethod::Pad => 0,
        VectorSpreadMethod::Reflect => 1,
        VectorSpreadMethod::Repeat => 2,
    });
}

/// Imperative builder for a single [`VectorPath`]. Mirrors a subset of
/// the SVG path command vocabulary (`M`, `L`, `C`, `Q`, `Z`) plus
/// fill/stroke style. Returns a `VectorPath`; combine multiple via
/// [`VectorAsset::from_paths`].
///
/// ```
/// use damascene_core::vector::{
///     PathBuilder, VectorAsset, VectorColor, VectorLineCap,
/// };
/// use damascene_core::tree::Color;
///
/// let curve = PathBuilder::new()
///     .move_to(0.0, 0.0)
///     .cubic_to(20.0, 0.0, 0.0, 60.0, 20.0, 60.0)
///     .stroke_solid(Color::srgb_u8(80, 200, 240), 2.0)
///     .stroke_line_cap(VectorLineCap::Round)
///     .build();
/// let asset = VectorAsset::from_paths([0.0, 0.0, 20.0, 60.0], vec![curve]);
/// // `asset.content_hash()` is stable across rebuilds with the same inputs,
/// // so backends share one atlas slot per unique geometry.
/// # let _ = asset;
/// ```
#[derive(Clone, Debug)]
pub struct PathBuilder {
    segments: Vec<VectorSegment>,
    fill: Option<VectorFill>,
    stroke: Option<VectorStroke>,
}

impl Default for PathBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PathBuilder {
    /// Create an empty builder with no segments, fill, or stroke.
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            fill: None,
            stroke: None,
        }
    }

    /// SVG `M x y`.
    pub fn move_to(mut self, x: f32, y: f32) -> Self {
        self.segments.push(VectorSegment::MoveTo([x, y]));
        self
    }

    /// SVG `L x y`.
    pub fn line_to(mut self, x: f32, y: f32) -> Self {
        self.segments.push(VectorSegment::LineTo([x, y]));
        self
    }

    /// SVG `Q cx cy x y`.
    pub fn quad_to(mut self, cx: f32, cy: f32, x: f32, y: f32) -> Self {
        self.segments.push(VectorSegment::QuadTo([cx, cy], [x, y]));
        self
    }

    /// SVG `C c1x c1y c2x c2y x y`.
    pub fn cubic_to(mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) -> Self {
        self.segments
            .push(VectorSegment::CubicTo([c1x, c1y], [c2x, c2y], [x, y]));
        self
    }

    /// SVG `Z` — close the current subpath back to its `MoveTo`.
    pub fn close(mut self) -> Self {
        self.segments.push(VectorSegment::Close);
        self
    }

    /// Fill with a solid colour at full opacity, non-zero rule. For
    /// finer control set [`Self::fill`] directly.
    pub fn fill_solid(mut self, color: crate::tree::Color) -> Self {
        self.fill = Some(VectorFill {
            color: VectorColor::Solid(color),
            opacity: 1.0,
            rule: VectorFillRule::NonZero,
        });
        self
    }

    /// Set the fill explicitly. `None` clears it.
    pub fn fill(mut self, fill: Option<VectorFill>) -> Self {
        self.fill = fill;
        self
    }

    /// Stroke with a solid colour and explicit width, with default
    /// line cap (`Butt`), line join (`Miter`), and miter limit (4.0).
    /// For finer control chain [`Self::stroke_line_cap`] /
    /// [`Self::stroke_line_join`] / [`Self::stroke_miter_limit`].
    pub fn stroke_solid(mut self, color: crate::tree::Color, width: f32) -> Self {
        self.stroke = Some(VectorStroke {
            color: VectorColor::Solid(color),
            opacity: 1.0,
            width,
            line_cap: VectorLineCap::Butt,
            line_join: VectorLineJoin::Miter,
            miter_limit: 4.0,
        });
        self
    }

    /// Set the stroke explicitly. `None` clears it.
    pub fn stroke(mut self, stroke: Option<VectorStroke>) -> Self {
        self.stroke = stroke;
        self
    }

    /// SVG `stroke-linecap`. No-op unless a stroke is already set.
    pub fn stroke_line_cap(mut self, cap: VectorLineCap) -> Self {
        if let Some(s) = self.stroke.as_mut() {
            s.line_cap = cap;
        }
        self
    }

    /// SVG `stroke-linejoin`. No-op unless a stroke is already set.
    pub fn stroke_line_join(mut self, join: VectorLineJoin) -> Self {
        if let Some(s) = self.stroke.as_mut() {
            s.line_join = join;
        }
        self
    }

    /// SVG `stroke-miterlimit`. No-op unless a stroke is already set.
    pub fn stroke_miter_limit(mut self, limit: f32) -> Self {
        if let Some(s) = self.stroke.as_mut() {
            s.miter_limit = limit;
        }
        self
    }

    /// SVG `stroke-opacity` in `0.0..=1.0`. No-op unless a stroke is
    /// already set.
    pub fn stroke_opacity(mut self, opacity: f32) -> Self {
        if let Some(s) = self.stroke.as_mut() {
            s.opacity = opacity;
        }
        self
    }

    /// Finish the builder into a [`VectorPath`].
    pub fn build(self) -> VectorPath {
        VectorPath {
            segments: self.segments,
            fill: self.fill,
            stroke: self.stroke,
            clip: None,
        }
    }
}

/// One styled path: segments plus optional fill and stroke. The
/// flattened equivalent of an SVG `<path>` element, with transforms
/// already applied so coordinates are absolute viewBox space.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    /// Path commands in order, in absolute viewBox coordinates.
    pub segments: Vec<VectorSegment>,
    /// Fill style, or `None` (SVG `fill="none"` or an unsupported paint
    /// such as a pattern).
    pub fill: Option<VectorFill>,
    /// Stroke style, or `None` when the path is not stroked.
    pub stroke: Option<VectorStroke>,
    /// Index into [`VectorAsset::clips`]: the path renders only inside
    /// that clip's region (SVG `clip-path`). `None` renders unclipped.
    pub clip: Option<u32>,
}

/// One shape inside a [`VectorClip`]: a closed outline plus the SVG
/// `clip-rule` deciding its filled region. Coordinates are absolute
/// viewBox space, like every [`VectorPath`].
#[derive(Clone, Debug, PartialEq)]
pub struct VectorClipShape {
    /// Path commands in order, in absolute viewBox coordinates. Open
    /// subpaths close implicitly, as SVG fills do.
    pub segments: Vec<VectorSegment>,
    /// SVG `clip-rule` for this shape.
    pub rule: VectorFillRule,
}

/// One SVG `clip-path` region: the union of its shapes' filled areas,
/// optionally intersected with a chained clip. Nested groups that both
/// carry `clip-path`, and `clip-path` on a `<clipPath>` element itself,
/// compose through `parent`.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorClip {
    /// Shapes whose filled regions union into this clip's area.
    pub shapes: Vec<VectorClipShape>,
    /// Chained clip in [`VectorAsset::clips`]: the effective region is
    /// this clip's union intersected with the parent's region. Must
    /// reference an *earlier* table index; self or forward references
    /// are ignored at resolution (guards hand-built cycles).
    pub parent: Option<u32>,
}

/// One absolute path command (SVG `M`/`L`/`Q`/`C`/`Z`). Points are
/// `[x, y]` in viewBox space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VectorSegment {
    /// SVG `M x y`: start a new subpath at the point.
    MoveTo([f32; 2]),
    /// SVG `L x y`: straight line to the point.
    LineTo([f32; 2]),
    /// SVG `Q cx cy x y`: quadratic Bézier (control point, endpoint).
    QuadTo([f32; 2], [f32; 2]),
    /// SVG `C c1x c1y c2x c2y x y`: cubic Bézier (two control points,
    /// endpoint).
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    /// SVG `Z`: close the current subpath back to its `MoveTo`.
    Close,
}

/// Fill style for a path (SVG `fill`, `fill-opacity`, `fill-rule`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorFill {
    /// Fill paint (SVG `fill`).
    pub color: VectorColor,
    /// SVG `fill-opacity` in `0.0..=1.0`, multiplied into the paint's
    /// alpha at tessellation.
    pub opacity: f32,
    /// SVG `fill-rule`.
    pub rule: VectorFillRule,
}

/// Stroke style for a path (SVG `stroke` and its companion properties).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorStroke {
    /// Stroke paint (SVG `stroke`).
    pub color: VectorColor,
    /// SVG `stroke-opacity` in `0.0..=1.0`, multiplied into the paint's
    /// alpha at tessellation.
    pub opacity: f32,
    /// SVG `stroke-width` in viewBox units; scaled to the destination
    /// rect at tessellation. `currentColor` strokes are instead widened
    /// by [`VectorMeshOptions::stroke_width`].
    pub width: f32,
    /// SVG `stroke-linecap`.
    pub line_cap: VectorLineCap,
    /// SVG `stroke-linejoin`.
    pub line_join: VectorLineJoin,
    /// SVG `stroke-miterlimit` (clamped to `>= 1.0` at tessellation).
    pub miter_limit: f32,
}

/// Paint for a fill or stroke. Kept `Copy` by referencing gradients
/// through an index into the asset's side-table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VectorColor {
    /// SVG `currentColor`: substituted with
    /// [`VectorMeshOptions::current_color`] at tessellation.
    CurrentColor,
    /// A solid colour. Palette tokens stay unresolved until
    /// [`VectorAsset::resolved_palette`].
    Solid(Color),
    /// Index into [`VectorAsset::gradients`].
    Gradient(u32),
}

/// A linear or radial gradient resolved to absolute SVG/viewBox space. The
/// stored axis/centre coordinates live in the gradient's own coordinate
/// system; `absolute_to_local` maps a point in absolute SVG space back into
/// that system so per-vertex evaluation is one matrix-multiply away.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorGradient {
    /// SVG `<linearGradient>`.
    Linear(VectorLinearGradient),
    /// SVG `<radialGradient>`.
    Radial(VectorRadialGradient),
}

impl VectorGradient {
    /// The gradient's colour stops, sorted by non-decreasing offset.
    pub fn stops(&self) -> &[VectorGradientStop] {
        match self {
            Self::Linear(g) => &g.stops,
            Self::Radial(g) => &g.stops,
        }
    }

    /// The gradient's SVG `spreadMethod`.
    pub fn spread(&self) -> VectorSpreadMethod {
        match self {
            Self::Linear(g) => g.spread,
            Self::Radial(g) => g.spread,
        }
    }
}

/// An SVG `<linearGradient>` resolved by usvg (`objectBoundingBox` units
/// already baked into the transform).
#[derive(Clone, Debug, PartialEq)]
pub struct VectorLinearGradient {
    /// Gradient axis start (SVG `x1`/`y1`) in the gradient's local space.
    pub p1: [f32; 2],
    /// Gradient axis end (SVG `x2`/`y2`) in the gradient's local space.
    pub p2: [f32; 2],
    /// Colour stops, sorted by non-decreasing offset.
    pub stops: Vec<VectorGradientStop>,
    /// SVG `spreadMethod`: how the gradient parameter wraps outside `0..=1`.
    pub spread: VectorSpreadMethod,
    /// Row-major 2x3 affine `[sx, kx, tx, ky, sy, ty]` mapping absolute
    /// SVG coordinates into the gradient's own coordinate system.
    pub absolute_to_local: [f32; 6],
}

/// An SVG `<radialGradient>` resolved by usvg (`objectBoundingBox` units
/// already baked into the transform).
///
/// Sampling currently treats the gradient as concentric about `center`
/// with radius `radius`; offset focal points parse but render without
/// the focal-cone projection.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorRadialGradient {
    /// Centre (SVG `cx`/`cy`) in the gradient's local space.
    pub center: [f32; 2],
    /// Radius (SVG `r`) in the gradient's local space.
    pub radius: f32,
    /// Focal point (SVG `fx`/`fy`); see the concentric-sampling note above.
    pub focal: [f32; 2],
    /// Focal radius (SVG `fr`); see the concentric-sampling note above.
    pub focal_radius: f32,
    /// Colour stops, sorted by non-decreasing offset.
    pub stops: Vec<VectorGradientStop>,
    /// SVG `spreadMethod`: how the gradient parameter wraps outside `0..=1`.
    pub spread: VectorSpreadMethod,
    /// Row-major 2x3 affine `[sx, kx, tx, ky, sy, ty]` mapping absolute
    /// SVG coordinates into the gradient's own coordinate system.
    pub absolute_to_local: [f32; 6],
}

/// A gradient stop. The colour is canonical **sRGB-encoded** floats with
/// the per-stop opacity in alpha.
///
/// Gradients interpolate between stops in sRGB space — the SVG default
/// (`color-interpolation: sRGB`), matching browsers — and the result
/// crosses into the renderer's working space only after interpolation
/// (at ramp bake and per-vertex fallback sampling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorGradientStop {
    /// Stop position along the gradient in `0.0..=1.0` (SVG stop
    /// `offset`), non-decreasing across the stop list.
    pub offset: f32,
    /// sRGB-encoded RGB plus straight alpha (per-stop opacity), baked at
    /// parse time. Assets are cached and space-independent; see the
    /// struct docs for where working-space conversion happens.
    pub color: [f32; 4],
}

/// SVG gradient `spreadMethod`: how the gradient parameter behaves
/// outside `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorSpreadMethod {
    /// SVG `pad` (the default): clamp to the edge stops.
    Pad,
    /// SVG `reflect`: mirror back and forth.
    Reflect,
    /// SVG `repeat`: wrap around.
    Repeat,
}

/// SVG `fill-rule`: how self-intersecting paths determine interior
/// coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorFillRule {
    /// SVG `nonzero` (the default): winding-number rule.
    NonZero,
    /// SVG `evenodd`: crossing-parity rule.
    EvenOdd,
}

/// SVG `stroke-linecap`: the shape drawn at the ends of open subpaths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorLineCap {
    /// SVG `butt` (the default): flat edge exactly at the endpoint.
    Butt,
    /// SVG `round`: semicircular cap.
    Round,
    /// SVG `square`: square cap extending half the stroke width.
    Square,
}

/// SVG `stroke-linejoin`: the shape drawn where path segments meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorLineJoin {
    /// SVG `miter` (the default): sharp corner, subject to the miter limit.
    Miter,
    /// SVG `miter-clip`: miter clipped at the limit instead of falling
    /// back to bevel.
    MiterClip,
    /// SVG `round`: circular-arc corner.
    Round,
    /// SVG `bevel`: flat corner.
    Bevel,
}

/// Shader-side material treatment for icon meshes, selected per theme
/// via [`crate::theme::Theme::with_icon_material`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconMaterial {
    /// Direct premultiplied color. This is the baseline material and
    /// should match ordinary flat SVG rendering.
    #[default]
    Flat,
    /// A proof material that uses local vector coordinates to add a
    /// subtle top-left highlight and lower shadow. This exists to prove
    /// the shared mesh carries enough data for shader-controlled icon
    /// treatments.
    Relief,
    /// A glossy icon material with local-coordinate glints and a soft
    /// inner shade. Pairs with translucent/glass surfaces.
    Glass,
}

/// One tessellated vertex of a [`VectorMesh`]. `#[repr(C)]` and `Pod`
/// so backends can upload vertex buffers directly.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct VectorMeshVertex {
    /// Logical-pixel position after fitting the vector asset into its
    /// destination rect.
    pub pos: [f32; 2],
    /// SVG/viewBox-space coordinate. Theme shaders can use this for
    /// gradients, highlights, bevels, and other icon-local effects.
    pub local: [f32; 2],
    /// Vertex RGBA in the mesh's working color space (see
    /// [`VectorMeshOptions::working_color_space`]), with fill/stroke
    /// opacity baked into alpha.
    pub color: [f32; 4],
    /// Material/paint metadata: x = path index, y = primitive kind
    /// (0 fill, 1 stroke), z = 1-based slot in the frame's
    /// [`VectorGradientFrame`] (0 = paint is the per-vertex `color`),
    /// w reserved.
    pub meta: [f32; 4],
}

/// A tessellated vector asset as a flat, non-indexed triangle list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorMesh {
    /// Triangle-list vertices: every three consecutive vertices form one
    /// triangle (indices are pre-expanded).
    pub vertices: Vec<VectorMeshVertex>,
}

/// The span appended to a shared vertex vector by
/// [`append_vector_asset_mesh`] — a draw range for non-indexed rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorMeshRun {
    /// Index of the run's first vertex in the destination vector.
    pub first: u32,
    /// Number of vertices in the run (a multiple of 3; 0 for a
    /// degenerate destination rect).
    pub count: u32,
}

/// Parameters for tessellating a [`VectorAsset`] into a [`VectorMesh`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorMeshOptions {
    /// Destination rectangle in logical pixels; the asset's view box is
    /// scaled (per-axis, so possibly non-uniformly) to fill it.
    pub rect: crate::tree::Rect,
    /// Colour substituted for SVG `currentColor` fills and strokes.
    pub current_color: Color,
    /// Stroke width in viewBox units applied to `currentColor` strokes,
    /// overriding their authored width; other strokes keep their own.
    pub stroke_width: f32,
    /// Curve-flattening tolerance for the lyon tessellators, in
    /// destination logical pixels (lower is smoother;
    /// [`VectorMeshOptions::icon`] uses `0.05`).
    pub tolerance: f32,
    /// Working color space vertex colors are packed in — solid fills,
    /// `currentColor`, and sampled gradient stops all cross the
    /// working-space boundary here (`rgba_f32_in` semantics). Backends
    /// pass their painter's negotiated space.
    pub working_color_space: ColorSpace,
}

impl VectorMeshOptions {
    /// Options preset for UI icons: the given rect, `currentColor`,
    /// stroke width, and working space, with the icon-tuned tolerance of
    /// `0.05` logical pixels.
    pub fn icon(
        rect: crate::tree::Rect,
        current_color: Color,
        stroke_width: f32,
        working_color_space: ColorSpace,
    ) -> Self {
        Self {
            rect,
            current_color,
            stroke_width,
            tolerance: 0.05,
            working_color_space,
        }
    }
}

/// Texel width of one baked gradient ramp row (see
/// [`VectorGradientFrame`]).
pub const GRADIENT_RAMP_WIDTH: usize = 256;

/// Gradient slots available per frame. Fixed so backends can allocate
/// the ramp texture (`GRADIENT_RAMP_WIDTH x MAX_FRAME_GRADIENTS`) and
/// the shader-side uniform array statically. Must match `MAX_GRADIENTS`
/// in the stock `vector*.wgsl` shaders.
pub const MAX_FRAME_GRADIENTS: usize = 128;

/// GPU parameters for one gradient slot, shared by every backend (like
/// `scene::gpu` packing). The local→`t` mapping is pre-folded on the CPU
/// so the fragment shader is one dot product (linear) or one 2x3 affine
/// plus `length` (radial) away from the ramp lookup:
///
/// - linear: `t = m0.x * local.x + m0.y * local.y + m0.z`
/// - radial: `t = length(M * (local, 1))` with rows `m0.xyz` / `m1.xyz`
///
/// where `local` is the interpolated SVG/viewBox-space vertex coordinate
/// ([`VectorMeshVertex::local`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct VectorGradientGpuParams {
    /// `xyz` = row 0 of the folded local→`t` transform; `w` = kind
    /// (`0` linear, `1` radial).
    pub m0: [f32; 4],
    /// `xyz` = row 1 (radial only, zero for linear); `w` = spread
    /// (`0` pad, `1` reflect, `2` repeat).
    pub m1: [f32; 4],
    /// `x` = normalized `v` coordinate of the slot's ramp row (texel
    /// centre), `y` = paint opacity, `zw` reserved.
    pub misc: [f32; 4],
}

/// Frame-scoped gradient table backing fragment-stage gradient
/// evaluation (issues #140/#141).
///
/// Backends own one per painter and drive it per frame:
///
/// 1. [`begin`](Self::begin) at frame start (passing the negotiated
///    working color space).
/// 2. Tessellation ([`append_vector_asset_mesh`]) allocates a slot per
///    distinct `(gradient, paint opacity)` pair and writes `slot + 1`
///    into [`VectorMeshVertex::meta`]`[2]` (`0` = no gradient, paint is
///    the per-vertex colour).
/// 3. At flush, upload [`params`](Self::params) into the shader's
///    uniform array (zero-padded to [`MAX_FRAME_GRADIENTS`] entries) and
///    [`ramp_data`](Self::ramp_data) into rows `0..slot_count` of a
///    `GRADIENT_RAMP_WIDTH x MAX_FRAME_GRADIENTS` `Rgba16Float` texture,
///    sampled with bilinear filtering and clamp-to-edge addressing.
///
/// Ramp rows are baked by interpolating stops in sRGB space (the SVG
/// default `color-interpolation`) and converting each texel into the
/// working space, so the shader needs no colour-space knowledge. Slot
/// overflow past [`MAX_FRAME_GRADIENTS`] falls back to per-vertex
/// gradient sampling for the overflowing paints.
#[derive(Clone, Debug, Default)]
pub struct VectorGradientFrame {
    working: Option<ColorSpace>,
    params: Vec<VectorGradientGpuParams>,
    /// f16 bit patterns, RGBA x `GRADIENT_RAMP_WIDTH` per row, rows in
    /// slot order. Straight (un-premultiplied) alpha.
    ramps: Vec<u16>,
    lookup: std::collections::HashMap<GradientKey, u32>,
}

impl VectorGradientFrame {
    /// Empty table; call [`begin`](Self::begin) before use.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset for a new frame, keeping allocations. `working` is the
    /// painter's negotiated working color space — ramp texels are baked
    /// into it.
    pub fn begin(&mut self, working: ColorSpace) {
        self.working = Some(working);
        self.params.clear();
        self.ramps.clear();
        self.lookup.clear();
    }

    /// Number of slots allocated this frame.
    pub fn slot_count(&self) -> u32 {
        self.params.len() as u32
    }

    /// Whether no gradient paints were recorded this frame (backends can
    /// skip uploads).
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// GPU parameter blocks in slot order (`slot_count` entries).
    pub fn params(&self) -> &[VectorGradientGpuParams] {
        &self.params
    }

    /// Baked ramp texels in slot order: `GRADIENT_RAMP_WIDTH` RGBA
    /// texels per row as f16 bit patterns (straight alpha), in the
    /// working space passed to [`begin`](Self::begin).
    pub fn ramp_data(&self) -> &[u16] {
        &self.ramps
    }

    /// Allocate (or reuse) a slot for `gradient` painted at `opacity`.
    /// Returns `None` when the frame's [`MAX_FRAME_GRADIENTS`] budget is
    /// exhausted or [`begin`](Self::begin) has not been called — callers
    /// then keep the per-vertex fallback paint.
    pub fn allocate(&mut self, gradient: &VectorGradient, opacity: f32) -> Option<u32> {
        let working = self.working?;
        let opacity = opacity.clamp(0.0, 1.0);
        let mut params = fold_gradient_params(gradient, opacity);
        let key = GradientKey::of(&params, gradient.stops());
        if let Some(&slot) = self.lookup.get(&key) {
            return Some(slot);
        }
        if self.params.len() >= MAX_FRAME_GRADIENTS {
            return None;
        }
        let slot = self.params.len() as u32;
        params.misc[0] = (slot as f32 + 0.5) / MAX_FRAME_GRADIENTS as f32;
        self.params.push(params);
        bake_ramp(gradient.stops(), working, &mut self.ramps);
        self.lookup.insert(key, slot);
        Some(slot)
    }
}

/// Slot dedup key: the folded transform, kind, spread, and opacity bit
/// patterns plus the stop list. Two paints that key equal produce
/// identical params and ramps, so they can share a slot.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GradientKey {
    m0: [u32; 4],
    m1: [u32; 4],
    opacity: u32,
    stops: Vec<(u32, [u32; 4])>,
}

impl GradientKey {
    fn of(params: &VectorGradientGpuParams, stops: &[VectorGradientStop]) -> Self {
        Self {
            m0: params.m0.map(f32::to_bits),
            m1: params.m1.map(f32::to_bits),
            opacity: params.misc[1].to_bits(),
            stops: stops
                .iter()
                .map(|s| (s.offset.to_bits(), s.color.map(f32::to_bits)))
                .collect(),
        }
    }
}

/// Fold a gradient's `absolute_to_local` affine and geometry into the
/// two shader rows of [`VectorGradientGpuParams`]. `misc[0]` (ramp row)
/// is filled in by the slot allocator.
fn fold_gradient_params(gradient: &VectorGradient, opacity: f32) -> VectorGradientGpuParams {
    let spread = match gradient.spread() {
        VectorSpreadMethod::Pad => 0.0,
        VectorSpreadMethod::Reflect => 1.0,
        VectorSpreadMethod::Repeat => 2.0,
    };
    let (m0, m1) = match gradient {
        VectorGradient::Linear(g) => {
            // t(p) = ((A·p + a) - p1)·d / |d|² for absolute point p,
            // A/a from `absolute_to_local`, d = p2 - p1 — affine in p,
            // so it folds to a single row.
            let m = &g.absolute_to_local;
            let dx = g.p2[0] - g.p1[0];
            let dy = g.p2[1] - g.p1[1];
            let len2 = dx * dx + dy * dy;
            if len2 <= f32::EPSILON {
                // SVG 13.2.2: degenerate axis paints the last stop —
                // constant t = 1 under pad spread, matching
                // `sample_gradient`.
                ([0.0, 0.0, 1.0, 0.0], [0.0; 4])
            } else {
                (
                    [
                        (m[0] * dx + m[3] * dy) / len2,
                        (m[1] * dx + m[4] * dy) / len2,
                        ((m[2] - g.p1[0]) * dx + (m[5] - g.p1[1]) * dy) / len2,
                        0.0,
                    ],
                    [0.0, 0.0, 0.0, spread],
                )
            }
        }
        VectorGradient::Radial(g) => {
            // t(p) = |(A·p + a - center)| / radius: fold the centre into
            // the affine translation and the radius into its scale, so
            // the shader takes `length` of the transformed point.
            // Concentric v0 semantics — matches `sample_gradient`.
            let m = &g.absolute_to_local;
            let r = g.radius;
            if r <= f32::EPSILON {
                // SVG 13.2.3: r == 0 paints the last stop — constant
                // t = |(1, 0)| = 1 under pad spread.
                ([0.0, 0.0, 1.0, 1.0], [0.0; 4])
            } else {
                (
                    [m[0] / r, m[1] / r, (m[2] - g.center[0]) / r, 1.0],
                    [m[3] / r, m[4] / r, (m[5] - g.center[1]) / r, spread],
                )
            }
        }
    };
    VectorGradientGpuParams {
        m0,
        m1,
        misc: [0.0, opacity, 0.0, 0.0],
    }
}

/// Append one baked ramp row to `out`: `GRADIENT_RAMP_WIDTH` texels,
/// stop interpolation in sRGB space, each texel converted into `working`
/// and encoded as RGBA f16 bits with straight alpha. Texel `i` samples
/// `t = i / (GRADIENT_RAMP_WIDTH - 1)`; the shader's half-texel inset
/// maps `t = 0` and `t = 1` onto the row's edge texel centres. Spread
/// wrapping is the shader's job, so the row itself is always the padded
/// `0..=1` span.
fn bake_ramp(stops: &[VectorGradientStop], working: ColorSpace, out: &mut Vec<u16>) {
    use half::f16;
    out.reserve(GRADIENT_RAMP_WIDTH * 4);
    for i in 0..GRADIENT_RAMP_WIDTH {
        let t = i as f32 / (GRADIENT_RAMP_WIDTH - 1) as f32;
        let [r, g, b, a] = sample_stops(stops, t);
        let c = rgba_f32_in(Color::in_space(ColorSpace::SRGB, r, g, b, a), working);
        out.extend(c.map(|v| f16::from_f32(v).to_bits()));
    }
}

/// Error returned by [`parse_svg_asset`]: the SVG failed to parse, or it
/// produced no renderable paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorParseError {
    message: String,
}

impl VectorParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VectorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for VectorParseError {}

/// Parse an SVG string into a [`VectorAsset`], preserving authored
/// fills, strokes, and gradients.
///
/// usvg performs the normalization: XML, style inheritance, transforms,
/// arcs, and basic shapes are resolved, and groups are flattened to
/// paths. Unsupported paint (patterns) and non-path content (text,
/// images, filters) are silently dropped. Errors if the SVG fails to
/// parse or yields no renderable paths.
pub fn parse_svg_asset(svg: &str) -> Result<VectorAsset, VectorParseError> {
    parse_svg_asset_with_color_mode(svg, false)
}

/// Tessellate `asset` into a standalone triangle-list [`VectorMesh`].
/// Convenience over [`append_vector_asset_mesh`] for callers that do not
/// batch several assets into one shared vertex vector. No gradient frame
/// is involved: gradient paints keep their per-vertex sampled colours.
pub fn tessellate_vector_asset(asset: &VectorAsset, options: VectorMeshOptions) -> VectorMesh {
    let mut mesh = VectorMesh::default();
    append_vector_asset_mesh(asset, options, &mut mesh.vertices, None);
    mesh
}

/// Tessellate `asset` and append its triangle-list vertices to `out`,
/// returning the appended span as a [`VectorMeshRun`].
///
/// Fills and strokes are flattened with lyon at `options.tolerance`,
/// scaled from the asset's view box into `options.rect`, and coloured
/// in `options.working_color_space` (solid fills, `currentColor`, and
/// gradient samples alike). Returns an empty run when the destination
/// rect has zero or negative area.
///
/// With a `gradient_frame`, each gradient paint additionally allocates a
/// slot in the frame's table and stamps `slot + 1` into the vertices'
/// [`VectorMeshVertex::meta`]`[2]` so the stock shaders resolve the
/// gradient per fragment (see [`VectorGradientFrame`]). Without one (or
/// on slot overflow) the per-vertex sampled colour is the paint.
pub fn append_vector_asset_mesh(
    asset: &VectorAsset,
    options: VectorMeshOptions,
    out: &mut Vec<VectorMeshVertex>,
    mut gradient_frame: Option<&mut VectorGradientFrame>,
) -> VectorMeshRun {
    let first = out.len() as u32;
    if options.rect.w <= 0.0 || options.rect.h <= 0.0 {
        return VectorMeshRun { first, count: 0 };
    }

    let [vx, vy, vw, vh] = asset.view_box;
    // SVG: a viewBox with a zero (or negative) dimension disables
    // rendering of the element entirely. Dividing by the real extent
    // otherwise keeps sub-unit view boxes (legal SVG, e.g. 0.5 units
    // wide) scaling correctly instead of silently rendering at the
    // wrong size through a `max(1.0)` guard.
    if vw <= 0.0 || vh <= 0.0 {
        return VectorMeshRun { first, count: 0 };
    }
    let sx = options.rect.w / vw;
    let sy = options.rect.h / vh;
    let stroke_scale = (sx + sy) * 0.5;

    // SVG `clip-path` regions resolve geometrically at this rect's
    // tolerance before tessellation; assets without clips skip the
    // whole machinery. Gradient indices survive the transform, so the
    // samplers below are oblivious.
    let flattened;
    let asset = if asset.has_clips() {
        let view_tolerance = options.tolerance / stroke_scale.max(f32::EPSILON);
        flattened = asset.flatten_clips(view_tolerance, options.stroke_width);
        &flattened
    } else {
        asset
    };

    for (path_index, vector_path) in asset.paths.iter().enumerate() {
        let path = build_lyon_path(vector_path, options.rect, [vx, vy], [sx, sy]);
        if let Some(fill) = vector_path.fill {
            let sampler = ColorSampler::build(
                fill.color,
                fill.opacity,
                options.current_color,
                &asset.gradients,
                options.working_color_space,
            );
            let slot = gradient_slot_plus_one(
                fill.color,
                fill.opacity,
                &asset.gradients,
                gradient_frame.as_deref_mut(),
            );
            let mut geometry: VertexBuffers<VectorMeshVertex, u16> = VertexBuffers::new();
            let fill_options =
                FillOptions::tolerance(options.tolerance).with_fill_rule(match fill.rule {
                    VectorFillRule::NonZero => lyon_tessellation::FillRule::NonZero,
                    VectorFillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
                });
            let _ = FillTessellator::new().tessellate_path(
                &path,
                &fill_options,
                &mut BuffersBuilder::new(&mut geometry, |v: FillVertex<'_>| {
                    make_mesh_vertex_sampled(
                        v.position(),
                        options.rect,
                        [vx, vy],
                        [sx, sy],
                        &sampler,
                        path_index,
                        VectorPrimitiveKind::Fill,
                        slot,
                    )
                }),
            );
            append_indexed(&geometry, out);
        }

        if let Some(stroke) = vector_path.stroke {
            let sampler = ColorSampler::build(
                stroke.color,
                stroke.opacity,
                options.current_color,
                &asset.gradients,
                options.working_color_space,
            );
            let slot = gradient_slot_plus_one(
                stroke.color,
                stroke.opacity,
                &asset.gradients,
                gradient_frame.as_deref_mut(),
            );
            let width = if matches!(stroke.color, VectorColor::CurrentColor) {
                options.stroke_width * stroke_scale
            } else {
                stroke.width * stroke_scale
            }
            .max(0.5);
            let mut geometry: VertexBuffers<VectorMeshVertex, u16> = VertexBuffers::new();
            let stroke_options = StrokeOptions::tolerance(options.tolerance)
                .with_line_width(width)
                .with_line_cap(match stroke.line_cap {
                    VectorLineCap::Butt => LineCap::Butt,
                    VectorLineCap::Round => LineCap::Round,
                    VectorLineCap::Square => LineCap::Square,
                })
                .with_line_join(match stroke.line_join {
                    VectorLineJoin::Miter => LineJoin::Miter,
                    VectorLineJoin::MiterClip => LineJoin::MiterClip,
                    VectorLineJoin::Round => LineJoin::Round,
                    VectorLineJoin::Bevel => LineJoin::Bevel,
                })
                .with_miter_limit(stroke.miter_limit.max(1.0));
            let _ = StrokeTessellator::new().tessellate_path(
                &path,
                &stroke_options,
                &mut BuffersBuilder::new(&mut geometry, |v: StrokeVertex<'_, '_>| {
                    make_mesh_vertex_sampled(
                        v.position(),
                        options.rect,
                        [vx, vy],
                        [sx, sy],
                        &sampler,
                        path_index,
                        VectorPrimitiveKind::Stroke,
                        slot,
                    )
                }),
            );
            append_indexed(&geometry, out);
        }
    }

    VectorMeshRun {
        first,
        count: out.len() as u32 - first,
    }
}

pub(crate) fn parse_current_color_svg_asset(svg: &str) -> Result<VectorAsset, VectorParseError> {
    parse_svg_asset_with_color_mode(svg, true)
}

fn parse_svg_asset_with_color_mode(
    svg: &str,
    force_current_color: bool,
) -> Result<VectorAsset, VectorParseError> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
        .map_err(|e| VectorParseError::new(format!("invalid SVG: {e}")))?;
    let size = tree.size();
    let mut asset = VectorAsset {
        view_box: [0.0, 0.0, size.width(), size.height()],
        paths: Vec::new(),
        gradients: Vec::new(),
        clips: Vec::new(),
    };
    let root_clip = tree
        .root()
        .clip_path()
        .map(|cp| convert_clip(cp, tree.root().abs_transform(), None, &mut asset.clips));
    collect_group(
        tree.root(),
        force_current_color,
        root_clip,
        &mut asset.paths,
        &mut asset.gradients,
        &mut asset.clips,
    );
    if asset.paths.is_empty() {
        return Err(VectorParseError::new("SVG produced no renderable paths"));
    }
    Ok(asset)
}

fn collect_group(
    group: &usvg::Group,
    force_current_color: bool,
    clip: Option<u32>,
    out: &mut Vec<VectorPath>,
    gradients: &mut Vec<VectorGradient>,
    clips: &mut Vec<VectorClip>,
) {
    for node in group.children() {
        match node {
            usvg::Node::Group(child) => {
                // SVG clips travel with the transform of the element
                // they clip, so the referencing group's absolute
                // transform is the base for the clip's geometry.
                let child_clip = match child.clip_path() {
                    Some(cp) => Some(convert_clip(cp, child.abs_transform(), clip, clips)),
                    None => clip,
                };
                collect_group(
                    child,
                    force_current_color,
                    child_clip,
                    out,
                    gradients,
                    clips,
                );
            }
            usvg::Node::Path(path) if path.is_visible() => {
                if let Some(vector_path) = convert_path(path, force_current_color, clip, gradients)
                {
                    out.push(vector_path);
                }
            }
            _ => {}
        }
    }
}

/// Convert one usvg clip path — plus its chained `clip-path` link — into
/// [`VectorClip`] entries, returning the index representing the full
/// chain. `base` is the absolute transform of the referencing element;
/// `parent` is the region already in effect from enclosing groups. A
/// linked clip applies in the referencing element's space, not inside
/// this clip's own transform, so the link recurses with `base` untouched.
fn convert_clip(
    cp: &usvg::ClipPath,
    base: tiny_skia_path::Transform,
    parent: Option<u32>,
    clips: &mut Vec<VectorClip>,
) -> u32 {
    let parent = match cp.clip_path() {
        Some(link) => Some(convert_clip(link, base, parent, clips)),
        None => parent,
    };
    let mut shapes = Vec::new();
    collect_clip_shapes(cp.root(), base.pre_concat(cp.transform()), &mut shapes);
    clips.push(VectorClip { shapes, parent });
    (clips.len() - 1) as u32
}

/// Flatten a clip subtree into [`VectorClipShape`]s. Clip roots are
/// standalone in usvg (their children's `abs_transform` is relative to
/// the clip root), so `base` carries the referencing element's absolute
/// transform composed with the clip's own. A `clip-path` on a child of
/// a `<clipPath>` is not honoured (the child contributes unclipped) —
/// per-shape sub-clipping doesn't fit the union-∩-parent region model
/// and has no real-world sightings yet.
fn collect_clip_shapes(
    group: &usvg::Group,
    base: tiny_skia_path::Transform,
    out: &mut Vec<VectorClipShape>,
) {
    for node in group.children() {
        match node {
            usvg::Node::Group(child) => collect_clip_shapes(child, base, out),
            usvg::Node::Path(path) if path.is_visible() => {
                let segments = convert_segments(path.data(), base.pre_concat(path.abs_transform()));
                if segments.is_empty() {
                    continue;
                }
                // usvg resolves `clip-rule` into the clip child's fill
                // rule; a child without a fill clips as nonzero.
                let rule = path
                    .fill()
                    .map(|f| match f.rule() {
                        usvg::FillRule::NonZero => VectorFillRule::NonZero,
                        usvg::FillRule::EvenOdd => VectorFillRule::EvenOdd,
                    })
                    .unwrap_or(VectorFillRule::NonZero);
                out.push(VectorClipShape { segments, rule });
            }
            _ => {}
        }
    }
}

fn convert_path(
    path: &usvg::Path,
    force_current_color: bool,
    clip: Option<u32>,
    gradients: &mut Vec<VectorGradient>,
) -> Option<VectorPath> {
    let transform = path.abs_transform();
    let segments = convert_segments(path.data(), transform);
    if segments.is_empty() {
        return None;
    }

    Some(VectorPath {
        segments,
        fill: path
            .fill()
            .and_then(|fill| convert_fill(fill, transform, force_current_color, gradients)),
        stroke: path
            .stroke()
            .and_then(|stroke| convert_stroke(stroke, transform, force_current_color, gradients)),
        clip,
    })
}

/// Map a tiny-skia path's segments through `transform` into absolute
/// [`VectorSegment`]s. Shared by regular paths and clip shapes.
fn convert_segments(
    data: &tiny_skia_path::Path,
    transform: tiny_skia_path::Transform,
) -> Vec<VectorSegment> {
    let mut segments = Vec::new();
    for segment in data.segments() {
        match segment {
            tiny_skia_path::PathSegment::MoveTo(p) => {
                segments.push(VectorSegment::MoveTo(map_point(transform, p)));
            }
            tiny_skia_path::PathSegment::LineTo(p) => {
                segments.push(VectorSegment::LineTo(map_point(transform, p)));
            }
            tiny_skia_path::PathSegment::QuadTo(p0, p1) => {
                segments.push(VectorSegment::QuadTo(
                    map_point(transform, p0),
                    map_point(transform, p1),
                ));
            }
            tiny_skia_path::PathSegment::CubicTo(p0, p1, p2) => {
                segments.push(VectorSegment::CubicTo(
                    map_point(transform, p0),
                    map_point(transform, p1),
                    map_point(transform, p2),
                ));
            }
            tiny_skia_path::PathSegment::Close => segments.push(VectorSegment::Close),
        }
    }
    segments
}

fn convert_fill(
    fill: &usvg::Fill,
    abs_transform: tiny_skia_path::Transform,
    force_current_color: bool,
    gradients: &mut Vec<VectorGradient>,
) -> Option<VectorFill> {
    Some(VectorFill {
        color: convert_paint(fill.paint(), abs_transform, force_current_color, gradients)?,
        opacity: fill.opacity().get(),
        rule: match fill.rule() {
            usvg::FillRule::NonZero => VectorFillRule::NonZero,
            usvg::FillRule::EvenOdd => VectorFillRule::EvenOdd,
        },
    })
}

fn convert_stroke(
    stroke: &usvg::Stroke,
    abs_transform: tiny_skia_path::Transform,
    force_current_color: bool,
    gradients: &mut Vec<VectorGradient>,
) -> Option<VectorStroke> {
    Some(VectorStroke {
        color: convert_paint(
            stroke.paint(),
            abs_transform,
            force_current_color,
            gradients,
        )?,
        opacity: stroke.opacity().get(),
        width: stroke.width().get(),
        line_cap: match stroke.linecap() {
            usvg::LineCap::Butt => VectorLineCap::Butt,
            usvg::LineCap::Round => VectorLineCap::Round,
            usvg::LineCap::Square => VectorLineCap::Square,
        },
        line_join: match stroke.linejoin() {
            usvg::LineJoin::Miter => VectorLineJoin::Miter,
            usvg::LineJoin::MiterClip => VectorLineJoin::MiterClip,
            usvg::LineJoin::Round => VectorLineJoin::Round,
            usvg::LineJoin::Bevel => VectorLineJoin::Bevel,
        },
        miter_limit: stroke.miterlimit().get(),
    })
}

fn convert_paint(
    paint: &usvg::Paint,
    abs_transform: tiny_skia_path::Transform,
    force_current_color: bool,
    gradients: &mut Vec<VectorGradient>,
) -> Option<VectorColor> {
    if force_current_color {
        return Some(VectorColor::CurrentColor);
    }
    match paint {
        usvg::Paint::Color(c) => Some(VectorColor::Solid(Color::srgb_u8a(
            c.red, c.green, c.blue, 255,
        ))),
        usvg::Paint::LinearGradient(lg) => {
            let g = convert_linear_gradient(lg, abs_transform)?;
            let idx = gradients.len() as u32;
            gradients.push(VectorGradient::Linear(g));
            Some(VectorColor::Gradient(idx))
        }
        usvg::Paint::RadialGradient(rg) => {
            let g = convert_radial_gradient(rg, abs_transform)?;
            let idx = gradients.len() as u32;
            gradients.push(VectorGradient::Radial(g));
            Some(VectorColor::Gradient(idx))
        }
        usvg::Paint::Pattern(_) => None,
    }
}

fn convert_linear_gradient(
    lg: &usvg::LinearGradient,
    abs_transform: tiny_skia_path::Transform,
) -> Option<VectorLinearGradient> {
    let stops = convert_stops(lg.stops());
    if stops.is_empty() {
        return None;
    }
    let absolute_to_local = build_absolute_to_local(abs_transform, lg.transform())?;
    Some(VectorLinearGradient {
        p1: [lg.x1(), lg.y1()],
        p2: [lg.x2(), lg.y2()],
        stops,
        spread: convert_spread(lg.spread_method()),
        absolute_to_local,
    })
}

fn convert_radial_gradient(
    rg: &usvg::RadialGradient,
    abs_transform: tiny_skia_path::Transform,
) -> Option<VectorRadialGradient> {
    let stops = convert_stops(rg.stops());
    if stops.is_empty() {
        return None;
    }
    let absolute_to_local = build_absolute_to_local(abs_transform, rg.transform())?;
    Some(VectorRadialGradient {
        center: [rg.cx(), rg.cy()],
        radius: rg.r().get(),
        focal: [rg.fx(), rg.fy()],
        focal_radius: rg.fr().get(),
        stops,
        spread: convert_spread(rg.spread_method()),
        absolute_to_local,
    })
}

fn convert_stops(stops: &[usvg::Stop]) -> Vec<VectorGradientStop> {
    let mut out = Vec::with_capacity(stops.len());
    let mut last_offset = 0.0_f32;
    for stop in stops {
        // SVG requires monotonically non-decreasing offsets; nudge so a
        // straight binary search over `out` always works.
        let offset = stop.offset().get().max(last_offset);
        last_offset = offset;
        // Canonical sRGB — the SVG default gradient interpolation space
        // (`color-interpolation: sRGB`, issue #141). Parsed assets are
        // cached across frames, so no negotiated space is baked here;
        // conversion into the working space happens after interpolation,
        // at ramp bake / vertex sampling.
        out.push(VectorGradientStop {
            offset,
            color: [
                stop.color().red as f32 / 255.0,
                stop.color().green as f32 / 255.0,
                stop.color().blue as f32 / 255.0,
                stop.opacity().get(),
            ],
        });
    }
    out
}

fn convert_spread(method: usvg::SpreadMethod) -> VectorSpreadMethod {
    match method {
        usvg::SpreadMethod::Pad => VectorSpreadMethod::Pad,
        usvg::SpreadMethod::Reflect => VectorSpreadMethod::Reflect,
        usvg::SpreadMethod::Repeat => VectorSpreadMethod::Repeat,
    }
}

/// Build the inverse transform that maps an absolute SVG coordinate (post
/// `path.abs_transform()`) into the gradient's own coordinate system.
///
/// `gradient_transform` from usvg already takes a gradient-local point into
/// the path's *local* user space (with bbox-units pre-baked). Composing
/// with `abs_transform` lifts that into absolute space; inverting gives us
/// the back-mapping the per-vertex sampler needs.
fn build_absolute_to_local(
    abs_transform: tiny_skia_path::Transform,
    gradient_transform: tiny_skia_path::Transform,
) -> Option<[f32; 6]> {
    let local_to_absolute = abs_transform.pre_concat(gradient_transform);
    let inv = local_to_absolute.invert()?;
    Some([inv.sx, inv.kx, inv.tx, inv.ky, inv.sy, inv.ty])
}

fn map_point(transform: tiny_skia_path::Transform, mut point: tiny_skia_path::Point) -> [f32; 2] {
    transform.map_point(&mut point);
    [point.x, point.y]
}

#[derive(Clone, Copy)]
enum VectorPrimitiveKind {
    Fill,
    Stroke,
}

fn build_lyon_path(
    path: &VectorPath,
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
) -> LyonPath {
    let mut builder = LyonPath::builder();
    let mut open = false;
    for segment in &path.segments {
        match *segment {
            VectorSegment::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(map_mesh_point(rect, view_origin, scale, p));
                open = true;
            }
            VectorSegment::LineTo(p) => {
                builder.line_to(map_mesh_point(rect, view_origin, scale, p));
            }
            VectorSegment::QuadTo(c, p) => {
                builder.quadratic_bezier_to(
                    map_mesh_point(rect, view_origin, scale, c),
                    map_mesh_point(rect, view_origin, scale, p),
                );
            }
            VectorSegment::CubicTo(c0, c1, p) => {
                builder.cubic_bezier_to(
                    map_mesh_point(rect, view_origin, scale, c0),
                    map_mesh_point(rect, view_origin, scale, c1),
                    map_mesh_point(rect, view_origin, scale, p),
                );
            }
            VectorSegment::Close => {
                if open {
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

fn map_mesh_point(
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
    p: [f32; 2],
) -> lyon_tessellation::math::Point {
    point(
        rect.x + (p[0] - view_origin[0]) * scale[0],
        rect.y + (p[1] - view_origin[1]) * scale[1],
    )
}

/// Resolve the paint's gradient-frame slot for stamping into
/// [`VectorMeshVertex::meta`]`[2]`: `slot + 1` when the paint is a
/// gradient and a slot is available, else `0.0` (per-vertex paint).
fn gradient_slot_plus_one(
    color: VectorColor,
    opacity: f32,
    gradients: &[VectorGradient],
    frame: Option<&mut VectorGradientFrame>,
) -> f32 {
    let (Some(frame), VectorColor::Gradient(idx)) = (frame, color) else {
        return 0.0;
    };
    let Some(gradient) = gradients.get(idx as usize) else {
        return 0.0;
    };
    match frame.allocate(gradient, opacity) {
        Some(slot) => (slot + 1) as f32,
        None => 0.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn make_mesh_vertex_sampled(
    p: lyon_tessellation::math::Point,
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
    sampler: &ColorSampler<'_>,
    path_index: usize,
    kind: VectorPrimitiveKind,
    gradient_slot_plus_one: f32,
) -> VectorMeshVertex {
    let local = [
        view_origin[0] + (p.x - rect.x) / scale[0].max(f32::EPSILON),
        view_origin[1] + (p.y - rect.y) / scale[1].max(f32::EPSILON),
    ];
    VectorMeshVertex {
        pos: [p.x, p.y],
        local,
        color: sampler.sample(local),
        meta: [
            path_index as f32,
            match kind {
                VectorPrimitiveKind::Fill => 0.0,
                VectorPrimitiveKind::Stroke => 1.0,
            },
            gradient_slot_plus_one,
            0.0,
        ],
    }
}

/// Per-vertex colour resolver. Solid/`currentColor` paths bake to a single
/// constant; gradient paths defer to per-vertex evaluation against the
/// vertex's SVG-space `local` coordinate. All variants resolve into the
/// mesh's working color space.
enum ColorSampler<'a> {
    Solid([f32; 4]),
    Gradient {
        gradient: &'a VectorGradient,
        opacity: f32,
        working: ColorSpace,
    },
}

impl<'a> ColorSampler<'a> {
    fn build(
        color: VectorColor,
        opacity: f32,
        current_color: Color,
        gradients: &'a [VectorGradient],
        working: ColorSpace,
    ) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);
        match color {
            VectorColor::CurrentColor => {
                let mut c = rgba_f32_in(current_color, working);
                c[3] *= opacity;
                Self::Solid(c)
            }
            VectorColor::Solid(c) => {
                let mut rgba = rgba_f32_in(c, working);
                rgba[3] *= opacity;
                Self::Solid(rgba)
            }
            VectorColor::Gradient(idx) => match gradients.get(idx as usize) {
                Some(gradient) => Self::Gradient {
                    gradient,
                    opacity,
                    working,
                },
                // Index out of range — should not happen for parsed assets;
                // keep the path renderable as transparent rather than crashing.
                None => Self::Solid([0.0; 4]),
            },
        }
    }

    fn sample(&self, abs_local: [f32; 2]) -> [f32; 4] {
        match self {
            Self::Solid(c) => *c,
            Self::Gradient {
                gradient,
                opacity,
                working,
            } => {
                // Stops are canonical sRGB and lerp in sRGB — the SVG
                // default `color-interpolation` (issue #141) — so the
                // working-space conversion must come after the lerp.
                // This per-vertex sample is the fallback paint for custom
                // shaders and headless meshes; the stock pipeline
                // re-evaluates the gradient per fragment against the
                // frame's [`VectorGradientFrame`] table (issue #140).
                let [r, g, b, a] = sample_gradient(gradient, abs_local);
                let mut c = rgba_f32_in(Color::in_space(ColorSpace::SRGB, r, g, b, a), *working);
                c[3] *= *opacity;
                c
            }
        }
    }
}

fn sample_gradient(gradient: &VectorGradient, abs_local: [f32; 2]) -> [f32; 4] {
    match gradient {
        VectorGradient::Linear(g) => {
            let dx = g.p2[0] - g.p1[0];
            let dy = g.p2[1] - g.p1[1];
            let len2 = dx * dx + dy * dy;
            // SVG 13.2.2: a degenerate axis (p1 == p2) paints the area
            // in the last stop's colour, spread method notwithstanding.
            if len2 <= f32::EPSILON {
                return sample_stops(&g.stops, 1.0);
            }
            let local = apply_affine(&g.absolute_to_local, abs_local);
            let t = ((local[0] - g.p1[0]) * dx + (local[1] - g.p1[1]) * dy) / len2;
            sample_stops(&g.stops, apply_spread(t, g.spread))
        }
        VectorGradient::Radial(g) => {
            // SVG 13.2.3: r == 0 likewise paints the last stop.
            if g.radius <= f32::EPSILON {
                return sample_stops(&g.stops, 1.0);
            }
            // Damascene v0: treat radial gradients as concentric about `center`
            // with radius `radius`. This matches the common authoring case
            // (focal == centre, focal_radius == 0); offset focal points are
            // accepted but rendered without the cone-projection nuance.
            let local = apply_affine(&g.absolute_to_local, abs_local);
            let dx = local[0] - g.center[0];
            let dy = local[1] - g.center[1];
            let t = (dx * dx + dy * dy).sqrt() / g.radius;
            sample_stops(&g.stops, apply_spread(t, g.spread))
        }
    }
}

fn apply_affine(m: &[f32; 6], p: [f32; 2]) -> [f32; 2] {
    [
        p[0] * m[0] + p[1] * m[1] + m[2],
        p[0] * m[3] + p[1] * m[4] + m[5],
    ]
}

fn apply_spread(t: f32, spread: VectorSpreadMethod) -> f32 {
    match spread {
        VectorSpreadMethod::Pad => t.clamp(0.0, 1.0),
        VectorSpreadMethod::Reflect => {
            let m = t.rem_euclid(2.0);
            if m > 1.0 { 2.0 - m } else { m }
        }
        VectorSpreadMethod::Repeat => t.rem_euclid(1.0),
    }
}

fn sample_stops(stops: &[VectorGradientStop], t: f32) -> [f32; 4] {
    if stops.is_empty() {
        return [0.0; 4];
    }
    if t <= stops[0].offset {
        return stops[0].color;
    }
    let last = stops.len() - 1;
    if t >= stops[last].offset {
        return stops[last].color;
    }
    for i in 1..stops.len() {
        if t <= stops[i].offset {
            let prev = &stops[i - 1];
            let next = &stops[i];
            let span = (next.offset - prev.offset).max(f32::EPSILON);
            let frac = ((t - prev.offset) / span).clamp(0.0, 1.0);
            return [
                prev.color[0] + (next.color[0] - prev.color[0]) * frac,
                prev.color[1] + (next.color[1] - prev.color[1]) * frac,
                prev.color[2] + (next.color[2] - prev.color[2]) * frac,
                prev.color[3] + (next.color[3] - prev.color[3]) * frac,
            ];
        }
    }
    stops[last].color
}

// ---------------------------------------------------------------------
// Clip resolution (SVG `clip-path`)
// ---------------------------------------------------------------------

/// Boolean-op geometry from `i_overlay`: shapes, each a list of
/// contours whose first entry is the outer boundary and the rest holes
/// (opposite winding).
type ClipPolyShapes = Vec<Vec<Vec<[f32; 2]>>>;

impl VectorAsset {
    /// Whether any path references a clip region.
    pub fn has_clips(&self) -> bool {
        self.paths.iter().any(|p| p.clip.is_some())
    }

    /// Resolve every clip into plain, clip-free geometry: a clipped
    /// fill becomes the boolean intersection of its filled region with
    /// the clip region; a clipped stroke is first expanded to its
    /// outline (Skia's stroker via `tiny-skia-path`) and then
    /// intersected, so the stroke *body* is cut at the clip boundary
    /// exactly as SVG specifies. Unclipped paths pass through
    /// untouched; the returned asset has an empty clip table. Paints —
    /// solid, `currentColor`, gradients — survive unchanged, since
    /// consumers sample them per vertex from the geometry's position.
    ///
    /// `tolerance` is the curve-flattening tolerance in viewBox units;
    /// consumers pass their rasterisation tolerance mapped back
    /// through their scale, so the polygonised result stays exact at
    /// the size being rendered. `current_color_stroke_width`
    /// substitutes for the width of `currentColor` strokes, mirroring
    /// [`VectorMeshOptions::stroke_width`].
    pub fn flatten_clips(&self, tolerance: f32, current_color_stroke_width: f32) -> VectorAsset {
        let tolerance = tolerance.max(1e-4);
        let mut memo: Vec<Option<ClipPolyShapes>> = vec![None; self.clips.len()];
        let mut paths = Vec::with_capacity(self.paths.len());
        for path in &self.paths {
            let region_idx = match path.clip {
                Some(idx) if (idx as usize) < self.clips.len() => idx as usize,
                // An out-of-range clip index (hand-built asset) has no
                // defined region; dropping the path beats guessing.
                Some(_) => continue,
                None => {
                    paths.push(path.clone());
                    continue;
                }
            };
            let region = resolve_clip_region(&self.clips, region_idx, tolerance, &mut memo);
            if region.is_empty() {
                continue;
            }
            if let Some(fill) = path.fill {
                let contours = flatten_segments(&path.segments, tolerance);
                let subj = contours.simplify_shape(overlay_fill_rule(fill.rule));
                let clipped =
                    subj.overlay(&region, OverlayRule::Intersect, OverlayFillRule::NonZero);
                if let Some(segments) = poly_shapes_to_segments(&clipped) {
                    paths.push(VectorPath {
                        segments,
                        fill: Some(VectorFill {
                            rule: VectorFillRule::NonZero,
                            ..fill
                        }),
                        stroke: None,
                        clip: None,
                    });
                }
            }
            if let Some(stroke) = path.stroke {
                let width = if matches!(stroke.color, VectorColor::CurrentColor) {
                    current_color_stroke_width
                } else {
                    stroke.width
                };
                if width > 0.0 {
                    let outline =
                        stroke_outline_contours(&path.segments, &stroke, width, tolerance);
                    let subj = outline.simplify_shape(OverlayFillRule::NonZero);
                    let clipped =
                        subj.overlay(&region, OverlayRule::Intersect, OverlayFillRule::NonZero);
                    if let Some(segments) = poly_shapes_to_segments(&clipped) {
                        paths.push(VectorPath {
                            segments,
                            // The expanded outline renders as a fill
                            // carrying the stroke's paint.
                            fill: Some(VectorFill {
                                color: stroke.color,
                                opacity: stroke.opacity,
                                rule: VectorFillRule::NonZero,
                            }),
                            stroke: None,
                            clip: None,
                        });
                    }
                }
            }
        }
        VectorAsset {
            view_box: self.view_box,
            paths,
            gradients: self.gradients.clone(),
            clips: Vec::new(),
        }
    }
}

/// Resolve one clip's effective region: the union of its shapes (each
/// by its own rule), intersected down the parent chain. Memoised per
/// asset resolution; only strictly-earlier parent indices are followed,
/// which parse order guarantees and hand-built cycles cannot satisfy.
fn resolve_clip_region(
    clips: &[VectorClip],
    idx: usize,
    tolerance: f32,
    memo: &mut Vec<Option<ClipPolyShapes>>,
) -> ClipPolyShapes {
    if let Some(cached) = &memo[idx] {
        return cached.clone();
    }
    let clip = &clips[idx];
    let mut region: Option<ClipPolyShapes> = None;
    for shape in &clip.shapes {
        let contours = flatten_segments(&shape.segments, tolerance);
        if contours.is_empty() {
            continue;
        }
        let normalized = contours.simplify_shape(overlay_fill_rule(shape.rule));
        region = Some(match region {
            None => normalized,
            Some(acc) => acc.overlay(&normalized, OverlayRule::Union, OverlayFillRule::NonZero),
        });
    }
    let mut region = region.unwrap_or_default();
    if let Some(p) = clip.parent
        && (p as usize) < idx
    {
        let parent = resolve_clip_region(clips, p as usize, tolerance, memo);
        region = region.overlay(&parent, OverlayRule::Intersect, OverlayFillRule::NonZero);
    }
    memo[idx] = Some(region.clone());
    region
}

/// Flatten path segments into closed polygon contours at `tolerance`
/// (viewBox units). Open subpaths close implicitly — fill semantics —
/// and contours with fewer than three points are dropped.
fn flatten_segments(segments: &[VectorSegment], tolerance: f32) -> Vec<Vec<[f32; 2]>> {
    use lyon_tessellation::geom::{CubicBezierSegment, QuadraticBezierSegment};
    fn flush(contours: &mut Vec<Vec<[f32; 2]>>, current: &mut Vec<[f32; 2]>) {
        if current.len() >= 3 {
            contours.push(std::mem::take(current));
        } else {
            current.clear();
        }
    }
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut current: Vec<[f32; 2]> = Vec::new();
    let mut cursor = [0.0_f32, 0.0];
    for seg in segments {
        match *seg {
            VectorSegment::MoveTo(p) => {
                flush(&mut contours, &mut current);
                current.push(p);
                cursor = p;
            }
            VectorSegment::LineTo(p) => {
                if current.is_empty() {
                    current.push(cursor);
                }
                current.push(p);
                cursor = p;
            }
            VectorSegment::QuadTo(c, p) => {
                if current.is_empty() {
                    current.push(cursor);
                }
                let bez = QuadraticBezierSegment {
                    from: point(cursor[0], cursor[1]),
                    ctrl: point(c[0], c[1]),
                    to: point(p[0], p[1]),
                };
                bez.for_each_flattened(tolerance, &mut |line| {
                    current.push([line.to.x, line.to.y]);
                });
                cursor = p;
            }
            VectorSegment::CubicTo(c0, c1, p) => {
                if current.is_empty() {
                    current.push(cursor);
                }
                let bez = CubicBezierSegment {
                    from: point(cursor[0], cursor[1]),
                    ctrl1: point(c0[0], c0[1]),
                    ctrl2: point(c1[0], c1[1]),
                    to: point(p[0], p[1]),
                };
                bez.for_each_flattened(tolerance, &mut |line| {
                    current.push([line.to.x, line.to.y]);
                });
                cursor = p;
            }
            VectorSegment::Close => flush(&mut contours, &mut current),
        }
    }
    flush(&mut contours, &mut current);
    contours
}

/// Expand a stroked path to its outline via tiny-skia-path's stroker
/// (Skia's stroke expansion: caps, joins, miter limit), then flatten
/// the outline into polygon contours at `tolerance`.
fn stroke_outline_contours(
    segments: &[VectorSegment],
    stroke: &VectorStroke,
    width: f32,
    tolerance: f32,
) -> Vec<Vec<[f32; 2]>> {
    let mut pb = tiny_skia_path::PathBuilder::new();
    for seg in segments {
        match *seg {
            VectorSegment::MoveTo(p) => pb.move_to(p[0], p[1]),
            VectorSegment::LineTo(p) => pb.line_to(p[0], p[1]),
            VectorSegment::QuadTo(c, p) => pb.quad_to(c[0], c[1], p[0], p[1]),
            VectorSegment::CubicTo(c0, c1, p) => {
                pb.cubic_to(c0[0], c0[1], c1[0], c1[1], p[0], p[1])
            }
            VectorSegment::Close => pb.close(),
        }
    }
    let Some(path) = pb.finish() else {
        return Vec::new();
    };
    let props = tiny_skia_path::Stroke {
        width,
        miter_limit: stroke.miter_limit.max(1.0),
        line_cap: match stroke.line_cap {
            VectorLineCap::Butt => tiny_skia_path::LineCap::Butt,
            VectorLineCap::Round => tiny_skia_path::LineCap::Round,
            VectorLineCap::Square => tiny_skia_path::LineCap::Square,
        },
        line_join: match stroke.line_join {
            VectorLineJoin::Miter => tiny_skia_path::LineJoin::Miter,
            VectorLineJoin::MiterClip => tiny_skia_path::LineJoin::MiterClip,
            VectorLineJoin::Round => tiny_skia_path::LineJoin::Round,
            VectorLineJoin::Bevel => tiny_skia_path::LineJoin::Bevel,
        },
        dash: None,
    };
    // The stroker's internal curve error scales inversely with
    // `resolution_scale` (≈ a quarter unit at scale 1); matching it to
    // the caller's tolerance keeps the expansion as precise as the
    // flattening that follows without over-tessellating.
    let resolution_scale = (0.25 / tolerance).clamp(1.0, 1024.0);
    let Some(outline) = tiny_skia_path::PathStroker::new().stroke(&path, &props, resolution_scale)
    else {
        return Vec::new();
    };
    let outline = convert_segments(&outline, tiny_skia_path::Transform::identity());
    flatten_segments(&outline, tolerance)
}

/// Serialise boolean-op output back into path segments: each contour
/// becomes `MoveTo` + `LineTo`s + `Close`. Outer contours and holes
/// arrive in opposite windings, so nonzero filling is exact.
fn poly_shapes_to_segments(shapes: &ClipPolyShapes) -> Option<Vec<VectorSegment>> {
    let mut segments = Vec::new();
    for shape in shapes {
        for contour in shape {
            if contour.len() < 3 {
                continue;
            }
            segments.push(VectorSegment::MoveTo(contour[0]));
            for p in &contour[1..] {
                segments.push(VectorSegment::LineTo(*p));
            }
            segments.push(VectorSegment::Close);
        }
    }
    (!segments.is_empty()).then_some(segments)
}

fn overlay_fill_rule(rule: VectorFillRule) -> OverlayFillRule {
    match rule {
        VectorFillRule::NonZero => OverlayFillRule::NonZero,
        VectorFillRule::EvenOdd => OverlayFillRule::EvenOdd,
    }
}

fn append_indexed(
    geometry: &VertexBuffers<VectorMeshVertex, u16>,
    out: &mut Vec<VectorMeshVertex>,
) {
    for index in &geometry.indices {
        if let Some(vertex) = geometry.vertices.get(*index as usize) {
            out.push(*vertex);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::{all_icon_names, icon_vector_asset};

    #[test]
    fn parses_basic_svg_shapes_into_paths() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4" fill="none" stroke="#000" stroke-width="2"/></svg>"##,
        )
        .unwrap();
        assert_eq!(asset.view_box, [0.0, 0.0, 24.0, 24.0]);
        assert_eq!(asset.paths.len(), 1);
        assert!(asset.paths[0].stroke.is_some());
        assert!(asset.paths[0].segments.len() > 4);
    }

    #[test]
    fn sub_unit_view_box_scales_to_fill_the_destination_rect() {
        // A 0.5×0.5 viewBox is legal SVG; the old `vw.max(1.0)`
        // div-by-zero guard silently rendered it at half size.
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 0.5 0.5"><rect width="0.5" height="0.5" fill="#f00"/></svg>"##,
        )
        .unwrap();
        let mesh = tessellate_vector_asset(
            &asset,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, 100.0, 100.0),
                Color::srgb_u8(0, 0, 0),
                2.0,
                ColorSpace::SRGB_LINEAR,
            ),
        );
        let max_x = mesh
            .vertices
            .iter()
            .map(|v| v.pos[0])
            .fold(f32::MIN, f32::max);
        let max_y = mesh
            .vertices
            .iter()
            .map(|v| v.pos[1])
            .fold(f32::MIN, f32::max);
        assert!(
            (max_x - 100.0).abs() < 0.5 && (max_y - 100.0).abs() < 0.5,
            "0.5-unit square should fill the 100px rect, got extent ({max_x}, {max_y})"
        );
    }

    #[test]
    fn zero_dimension_view_box_renders_nothing() {
        // SVG: `viewBox` with a zero width or height disables rendering
        // of the element.
        let asset = VectorAsset::from_paths([0.0, 0.0, 0.0, 24.0], Vec::new());
        let mesh = tessellate_vector_asset(
            &asset,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, 16.0, 16.0),
                Color::srgb_u8(0, 0, 0),
                2.0,
                ColorSpace::SRGB_LINEAR,
            ),
        );
        assert!(mesh.vertices.is_empty());
    }

    #[test]
    fn tessellates_every_builtin_icon() {
        for name in all_icon_names() {
            let mesh = tessellate_vector_asset(
                icon_vector_asset(*name),
                VectorMeshOptions::icon(
                    crate::tree::Rect::new(0.0, 0.0, 16.0, 16.0),
                    Color::srgb_u8(15, 23, 42),
                    2.0,
                    ColorSpace::SRGB_LINEAR,
                ),
            );
            assert!(
                !mesh.vertices.is_empty(),
                "{} produced no tessellated vertices",
                name.name()
            );
        }
    }

    #[test]
    fn parses_linear_gradient_paint() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <defs>
                    <linearGradient id="g" x1="0" y1="0" x2="100" y2="0" gradientUnits="userSpaceOnUse">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#0000ff"/>
                    </linearGradient>
                </defs>
                <rect width="100" height="100" fill="url(#g)"/>
            </svg>"##,
        )
        .unwrap();
        assert_eq!(asset.gradients.len(), 1);
        assert!(matches!(
            asset.paths[0].fill.unwrap().color,
            VectorColor::Gradient(_)
        ));
        match &asset.gradients[0] {
            VectorGradient::Linear(g) => {
                assert_eq!(g.stops.len(), 2);
                assert_eq!(g.spread, VectorSpreadMethod::Pad);
                assert_eq!(g.p1, [0.0, 0.0]);
                assert_eq!(g.p2, [100.0, 0.0]);
            }
            other => panic!("expected linear gradient, got {other:?}"),
        }
    }

    #[test]
    fn bakes_gradient_into_per_vertex_colors() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <defs>
                    <linearGradient id="g" x1="0" y1="0" x2="100" y2="0" gradientUnits="userSpaceOnUse">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#0000ff"/>
                    </linearGradient>
                </defs>
                <rect width="100" height="100" fill="url(#g)"/>
            </svg>"##,
        )
        .unwrap();
        let mesh = tessellate_vector_asset(
            &asset,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, 200.0, 200.0),
                Color::srgb_u8(0, 0, 0),
                2.0,
                ColorSpace::SRGB_LINEAR,
            ),
        );
        assert!(!mesh.vertices.is_empty());

        // Vertices on the left side of the rect should be reddish; on the
        // right side, bluish.
        let mut min_x_vert = mesh.vertices[0];
        let mut max_x_vert = mesh.vertices[0];
        for v in &mesh.vertices {
            if v.local[0] < min_x_vert.local[0] {
                min_x_vert = *v;
            }
            if v.local[0] > max_x_vert.local[0] {
                max_x_vert = *v;
            }
        }
        assert!(
            min_x_vert.color[0] > min_x_vert.color[2],
            "left edge should be redder: {:?}",
            min_x_vert.color
        );
        assert!(
            max_x_vert.color[2] > max_x_vert.color[0],
            "right edge should be bluer: {:?}",
            max_x_vert.color
        );
    }

    fn clip_mesh(asset: &VectorAsset, size: f32) -> VectorMesh {
        tessellate_vector_asset(
            asset,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, size, size),
                Color::srgb_u8(255, 255, 255),
                1.0,
                ColorSpace::SRGB_LINEAR,
            ),
        )
    }

    fn local_bounds(mesh: &VectorMesh) -> ([f32; 2], [f32; 2]) {
        let mut min = [f32::MAX, f32::MAX];
        let mut max = [f32::MIN, f32::MIN];
        for v in &mesh.vertices {
            for a in 0..2 {
                min[a] = min[a].min(v.local[a]);
                max[a] = max[a].max(v.local[a]);
            }
        }
        (min, max)
    }

    #[test]
    fn clip_confines_a_fill_to_the_region() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs><clipPath id="c"><rect width="5" height="10"/></clipPath></defs>
                <g clip-path="url(#c)"><rect width="10" height="10" fill="#fff"/></g>
            </svg>"##,
        )
        .unwrap();
        assert!(asset.has_clips());
        assert_eq!(asset.clips.len(), 1);
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        let (min, max) = local_bounds(&mesh);
        assert!(min[0] >= -0.2 && max[0] <= 5.2, "x bounds {min:?} {max:?}");
        assert!(max[0] >= 4.8, "clip edge should be reached: {max:?}");
        assert!(min[1] >= -0.2 && max[1] <= 10.2, "y bounds {min:?} {max:?}");
    }

    #[test]
    fn clip_travels_with_the_referencing_groups_transform() {
        // The clip region is authored at x 0..2; the referencing group
        // translates by 4, so both its rect (drawn 0..6) and the clip
        // (4..6) move — SVG clips travel with the element's transform.
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs><clipPath id="c"><rect width="2" height="10"/></clipPath></defs>
                <g transform="translate(4 0)" clip-path="url(#c)">
                    <rect x="-4" width="10" height="10" fill="#fff"/>
                </g>
            </svg>"##,
        )
        .unwrap();
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        let (min, max) = local_bounds(&mesh);
        assert!(
            min[0] >= 3.8,
            "clip should move with the transform: {min:?}"
        );
        assert!(max[0] <= 6.2 && max[0] >= 5.8, "x max {max:?}");
    }

    #[test]
    fn even_odd_clip_rule_cuts_a_hole() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs><clipPath id="c">
                    <path d="M0 0H10V10H0Z M3 3H7V7H3Z" clip-rule="evenodd"/>
                </clipPath></defs>
                <g clip-path="url(#c)"><rect width="10" height="10" fill="#fff"/></g>
            </svg>"##,
        )
        .unwrap();
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        // The ring's vertices all sit on the outer or inner boundary;
        // none may land strictly inside the evenodd hole.
        for v in &mesh.vertices {
            let inside_hole =
                v.local[0] > 3.2 && v.local[0] < 6.8 && v.local[1] > 3.2 && v.local[1] < 6.8;
            assert!(!inside_hole, "vertex inside the clip hole: {:?}", v.local);
        }
    }

    #[test]
    fn nested_group_clips_intersect() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs>
                    <clipPath id="a"><rect width="6" height="10"/></clipPath>
                    <clipPath id="b"><rect width="10" height="6"/></clipPath>
                </defs>
                <g clip-path="url(#a)"><g clip-path="url(#b)">
                    <rect width="10" height="10" fill="#fff"/>
                </g></g>
            </svg>"##,
        )
        .unwrap();
        assert_eq!(asset.clips.len(), 2);
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        let (_, max) = local_bounds(&mesh);
        assert!(max[0] <= 6.2 && max[0] >= 5.8, "x max {max:?}");
        assert!(max[1] <= 6.2 && max[1] >= 5.8, "y max {max:?}");
    }

    #[test]
    fn a_clip_path_linked_from_a_clip_path_intersects() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs>
                    <clipPath id="a"><rect width="6" height="10"/></clipPath>
                    <clipPath id="b" clip-path="url(#a)"><rect width="10" height="6"/></clipPath>
                </defs>
                <g clip-path="url(#b)"><rect width="10" height="10" fill="#fff"/></g>
            </svg>"##,
        )
        .unwrap();
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        let (_, max) = local_bounds(&mesh);
        assert!(max[0] <= 6.2 && max[0] >= 5.8, "x max {max:?}");
        assert!(max[1] <= 6.2 && max[1] >= 5.8, "y max {max:?}");
    }

    #[test]
    fn a_clipped_stroke_is_cut_at_the_boundary() {
        // A vertical stroke overshooting the viewBox on both ends,
        // clipped to the top half: the stroke body (not just its
        // centerline) must stop at y = 5.
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs><clipPath id="c"><rect width="10" height="5"/></clipPath></defs>
                <g clip-path="url(#c)">
                    <path d="M5 -2 L5 12" fill="none" stroke="#fff" stroke-width="2"/>
                </g>
            </svg>"##,
        )
        .unwrap();
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        let (min, max) = local_bounds(&mesh);
        assert!(min[1] >= -0.2 && max[1] <= 5.2, "y bounds {min:?} {max:?}");
        assert!(max[1] >= 4.8, "stroke should reach the clip edge: {max:?}");
        assert!(
            min[0] >= 3.8 && max[0] <= 6.2,
            "stroke body x {min:?} {max:?}"
        );
    }

    #[test]
    fn a_clip_region_off_the_subject_drops_the_geometry() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs><clipPath id="c"><rect x="20" width="5" height="10"/></clipPath></defs>
                <g clip-path="url(#c)"><rect width="10" height="10" fill="#fff"/></g>
            </svg>"##,
        )
        .unwrap();
        assert!(clip_mesh(&asset, 10.0).vertices.is_empty());
    }

    #[test]
    fn a_hand_built_empty_clip_region_drops_the_path() {
        let mut asset = VectorAsset::from_paths(
            [0.0, 0.0, 10.0, 10.0],
            vec![
                PathBuilder::new()
                    .move_to(0.0, 0.0)
                    .line_to(10.0, 0.0)
                    .line_to(10.0, 10.0)
                    .close()
                    .fill_solid(Color::srgb_u8(255, 255, 255))
                    .build(),
            ],
        );
        asset.clips.push(VectorClip {
            shapes: Vec::new(),
            parent: None,
        });
        asset.paths[0].clip = Some(0);
        let flat = asset.flatten_clips(0.05, 1.0);
        assert!(flat.paths.is_empty());
        assert!(clip_mesh(&asset, 10.0).vertices.is_empty());
    }

    #[test]
    fn a_clipped_gradient_samples_at_the_new_vertices() {
        // Black-to-red across the full viewBox, clipped to the left
        // half: the reddest surviving vertex sits at the cut, sampling
        // the gradient midpoint — not the paint's far endpoint.
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs>
                    <clipPath id="c"><rect width="5" height="10"/></clipPath>
                    <linearGradient id="g" x1="0" y1="0" x2="10" y2="0" gradientUnits="userSpaceOnUse">
                        <stop offset="0" stop-color="#000000"/>
                        <stop offset="1" stop-color="#ff0000"/>
                    </linearGradient>
                </defs>
                <g clip-path="url(#c)"><rect width="10" height="10" fill="url(#g)"/></g>
            </svg>"##,
        )
        .unwrap();
        let mesh = clip_mesh(&asset, 10.0);
        assert!(!mesh.vertices.is_empty());
        let max_red = mesh
            .vertices
            .iter()
            .map(|v| v.color[0])
            .fold(f32::MIN, f32::max);
        assert!(
            max_red > 0.05 && max_red < 0.8,
            "reddest vertex should sample the midpoint, got {max_red}"
        );
    }

    #[test]
    fn clips_participate_in_the_content_hash() {
        let clipped = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <defs><clipPath id="c"><rect width="5" height="10"/></clipPath></defs>
                <g clip-path="url(#c)"><rect width="10" height="10" fill="#fff"/></g>
            </svg>"##,
        )
        .unwrap();
        let plain = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <rect width="10" height="10" fill="#fff"/>
            </svg>"##,
        )
        .unwrap();
        assert_eq!(clipped.paths.len(), plain.paths.len());
        assert_ne!(clipped.content_hash(), plain.content_hash());
    }

    #[test]
    fn has_gradient_distinguishes_flat_from_gradient_assets() {
        let flat = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4" fill="#fff"/></svg>"##,
        )
        .unwrap();
        assert!(!flat.has_gradient());

        let gradient = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <defs><linearGradient id="g" x1="0" y1="0" x2="100" y2="0" gradientUnits="userSpaceOnUse">
                    <stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff"/>
                </linearGradient></defs>
                <rect width="100" height="100" fill="url(#g)"/>
            </svg>"##,
        )
        .unwrap();
        assert!(gradient.has_gradient());
    }

    #[test]
    fn parses_pipewire_volume_icon_with_all_gradients() {
        // Sanity-check end-to-end on a real-world authored SVG: five
        // linear/radial gradients plus an unsupported drop-shadow filter
        // (which is silently dropped, not an error).
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">
  <defs>
    <linearGradient id="arcGradient" x1="210" y1="720" x2="805" y2="260" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#0667ff"/>
      <stop offset="0.52" stop-color="#139cff"/>
      <stop offset="1" stop-color="#11e4dc"/>
    </linearGradient>
    <linearGradient id="dotGradient" x1="585" y1="780" x2="805" y2="455" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#065eff"/>
      <stop offset="0.55" stop-color="#0d9fff"/>
      <stop offset="1" stop-color="#10e5dc"/>
    </linearGradient>
    <radialGradient id="knobFace" cx="42%" cy="36%" r="72%">
      <stop offset="0" stop-color="#12366c"/>
      <stop offset="0.42" stop-color="#0b2554"/>
      <stop offset="1" stop-color="#071736"/>
    </radialGradient>
    <linearGradient id="knobRim" x1="320" y1="310" x2="735" y2="740" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#214f9b"/>
      <stop offset="0.48" stop-color="#17386f"/>
      <stop offset="1" stop-color="#285aa7"/>
    </linearGradient>
    <linearGradient id="needleGradient" x1="565" y1="425" x2="670" y2="320" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#0872ff"/>
      <stop offset="1" stop-color="#168aff"/>
    </linearGradient>
  </defs>
  <path d="M 296 720 A 300 300 0 1 1 794 409" fill="none" stroke="url(#arcGradient)" stroke-width="36" stroke-linecap="round"/>
  <circle cx="512" cy="512" r="210" fill="url(#knobRim)"/>
  <circle cx="512" cy="512" r="192" fill="url(#knobFace)"/>
  <line x1="569" y1="433" x2="663" y2="339" stroke="url(#needleGradient)" stroke-width="30" stroke-linecap="round"/>
  <circle cx="612" cy="787" r="13" fill="url(#dotGradient)"/>
  <circle cx="664" cy="764" r="14" fill="url(#dotGradient)"/>
</svg>"##;
        let asset = parse_svg_asset(svg).unwrap();
        // 1 arc stroke + 2 knob fills + 1 needle stroke + 2 dot fills = 6 paths.
        assert_eq!(asset.paths.len(), 6);
        // At least one gradient per distinct paint server (5). usvg may
        // duplicate when the same gradient is referenced by multiple
        // paths after bbox resolution; we don't pin the exact count
        // because that's a usvg-internal detail.
        assert!(asset.gradients.len() >= 5);

        let mesh = tessellate_vector_asset(
            &asset,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, 256.0, 256.0),
                Color::srgb_u8(0, 0, 0),
                2.0,
                ColorSpace::SRGB_LINEAR,
            ),
        );
        assert!(!mesh.vertices.is_empty());
        // Some vertices must carry non-zero colour — if gradients silently
        // dropped to transparent, every channel would be 0.
        let any_lit = mesh
            .vertices
            .iter()
            .any(|v| v.color[0] + v.color[1] + v.color[2] > 0.01);
        assert!(any_lit, "no lit vertices — gradients did not render");
    }

    /// Reference sRGB-space lerp of two sRGB u8 colors, straight alpha —
    /// what browsers produce for the SVG default `color-interpolation:
    /// sRGB` (issue #141's verification data is this formula).
    fn srgb_lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [f32; 4] {
        let c = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) / 255.0;
        [c(a[0], b[0]), c(a[1], b[1]), c(a[2], b[2]), 1.0]
    }

    /// The #141 asset: `#754A75 → #F7A983`, vertical, plain rect.
    fn two_stop_asset() -> VectorAsset {
        parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 200">
                <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="200" gradientUnits="userSpaceOnUse">
                    <stop offset="0" stop-color="#754A75"/>
                    <stop offset="1" stop-color="#F7A983"/>
                </linearGradient></defs>
                <rect width="100" height="200" fill="url(#g)"/></svg>"##,
        )
        .unwrap()
    }

    /// The #140 asset: five stops at 0/0.25/0.5/0.75/1.
    fn five_stop_asset() -> VectorAsset {
        parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 200">
                <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="200" gradientUnits="userSpaceOnUse">
                    <stop offset="0" stop-color="#754A75"/>
                    <stop offset="0.25" stop-color="#372960"/>
                    <stop offset="0.5" stop-color="#A33861"/>
                    <stop offset="0.75" stop-color="#D1956C"/>
                    <stop offset="1" stop-color="#F7A983"/>
                </linearGradient></defs>
                <rect width="100" height="200" fill="url(#g)"/></svg>"##,
        )
        .unwrap()
    }

    /// Issue #141: stops are canonical sRGB and interpolate in sRGB —
    /// the midpoint of `#754A75 → #F7A983` must match a browser's
    /// `rgb(182, 122, 124)`, not the linear-space `rgb(196, 132, 124)`.
    #[test]
    fn stops_interpolate_in_srgb_space() {
        let asset = two_stop_asset();
        let stops = asset.gradients[0].stops();
        assert_eq!(stops.len(), 2);

        for &t in &[0.25, 0.5, 0.75] {
            let got = sample_stops(stops, t);
            let want = srgb_lerp([0x75, 0x4A, 0x75], [0xF7, 0xA9, 0x83], t);
            for (g, w) in got.iter().zip(&want) {
                assert!(
                    (g - w).abs() < 1e-6,
                    "sRGB lerp mismatch at t={t}: got {got:?} want {want:?}"
                );
            }
        }
    }

    /// Ramp texels are the sRGB-interpolated stop colours converted into
    /// the working space and encoded as f16 — checked against the same
    /// reference formula end to end, including #140's interior stops
    /// (texels near each authored stop must hit the stop colour).
    #[test]
    fn ramp_bake_matches_srgb_reference() {
        use half::f16;
        let asset = five_stop_asset();
        let gradient = &asset.gradients[0];

        let mut frame = VectorGradientFrame::new();
        frame.begin(ColorSpace::SRGB_LINEAR);
        let slot = frame.allocate(gradient, 1.0).unwrap();
        assert_eq!(slot, 0);
        let ramp = frame.ramp_data();
        assert_eq!(ramp.len(), GRADIENT_RAMP_WIDTH * 4);

        // Every texel must equal the reference pipeline (sRGB lerp →
        // working space) within f16 quantization.
        for i in 0..GRADIENT_RAMP_WIDTH {
            let t = i as f32 / (GRADIENT_RAMP_WIDTH - 1) as f32;
            let [r, g, b, a] = sample_stops(gradient.stops(), t);
            let want = rgba_f32_in(
                Color::in_space(ColorSpace::SRGB, r, g, b, a),
                ColorSpace::SRGB_LINEAR,
            );
            let got: Vec<f32> = ramp[i * 4..i * 4 + 4]
                .iter()
                .map(|&bits| f16::from_bits(bits).to_f32())
                .collect();
            for (g, w) in got.iter().zip(&want) {
                assert!((g - w).abs() < 2e-3, "texel {i}: got {got:?} want {want:?}");
            }
        }

        // Interior stops land on their authored colours: re-encode the
        // texel nearest each stop offset back to sRGB u8 and compare.
        let expected: [(f32, [u8; 3]); 5] = [
            (0.0, [0x75, 0x4A, 0x75]),
            (0.25, [0x37, 0x29, 0x60]),
            (0.5, [0xA3, 0x38, 0x61]),
            (0.75, [0xD1, 0x95, 0x6C]),
            (1.0, [0xF7, 0xA9, 0x83]),
        ];
        for (offset, rgb) in expected {
            let i = (offset * (GRADIENT_RAMP_WIDTH - 1) as f32).round() as usize;
            let texel: Vec<f32> = ramp[i * 4..i * 4 + 4]
                .iter()
                .map(|&bits| f16::from_bits(bits).to_f32())
                .collect();
            let back = Color::srgb_linear(texel[0], texel[1], texel[2], texel[3]).to_srgb_u8a();
            for (got, want) in back[..3].iter().zip(&rgb) {
                assert!(
                    (*got as i16 - *want as i16).abs() <= 2,
                    "stop at {offset}: ramp texel {back:?} vs authored {rgb:?}"
                );
            }
        }
    }

    /// The folded linear params must reproduce `sample_gradient`'s `t`
    /// for arbitrary points, including through a gradient transform.
    #[test]
    fn folded_linear_params_match_cpu_sampling() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <defs><linearGradient id="g" x1="10" y1="0" x2="110" y2="0"
                    gradientUnits="userSpaceOnUse" gradientTransform="rotate(30 50 50)">
                    <stop offset="0" stop-color="#000000"/>
                    <stop offset="1" stop-color="#ffffff"/>
                </linearGradient></defs>
                <rect width="100" height="100" fill="url(#g)"/></svg>"##,
        )
        .unwrap();
        let VectorGradient::Linear(g) = &asset.gradients[0] else {
            panic!("expected linear gradient");
        };
        let params = fold_gradient_params(&asset.gradients[0], 1.0);
        assert_eq!(params.m0[3], 0.0, "kind = linear");

        for p in [[0.0, 0.0], [50.0, 50.0], [100.0, 13.0], [-20.0, 260.0]] {
            let local = apply_affine(&g.absolute_to_local, p);
            let dx = g.p2[0] - g.p1[0];
            let dy = g.p2[1] - g.p1[1];
            let len2 = (dx * dx + dy * dy).max(f32::EPSILON);
            let want = ((local[0] - g.p1[0]) * dx + (local[1] - g.p1[1]) * dy) / len2;
            let got = params.m0[0] * p[0] + params.m0[1] * p[1] + params.m0[2];
            assert!(
                (got - want).abs() < 1e-4,
                "t mismatch at {p:?}: folded {got} vs sampled {want}"
            );
        }
    }

    /// The folded radial params must reproduce the concentric-distance
    /// `t` of `sample_gradient`.
    #[test]
    fn folded_radial_params_match_cpu_sampling() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <defs><radialGradient id="g" cx="50" cy="40" r="25" gradientUnits="userSpaceOnUse">
                    <stop offset="0" stop-color="#ffffff"/>
                    <stop offset="1" stop-color="#000000"/>
                </radialGradient></defs>
                <rect width="100" height="100" fill="url(#g)"/></svg>"##,
        )
        .unwrap();
        let VectorGradient::Radial(g) = &asset.gradients[0] else {
            panic!("expected radial gradient");
        };
        let params = fold_gradient_params(&asset.gradients[0], 1.0);
        assert_eq!(params.m0[3], 1.0, "kind = radial");

        for p in [[50.0, 40.0], [75.0, 40.0], [0.0, 0.0], [50.0, 90.0]] {
            let local = apply_affine(&g.absolute_to_local, p);
            let want = ((local[0] - g.center[0]).powi(2) + (local[1] - g.center[1]).powi(2)).sqrt()
                / g.radius.max(f32::EPSILON);
            let qx = params.m0[0] * p[0] + params.m0[1] * p[1] + params.m0[2];
            let qy = params.m1[0] * p[0] + params.m1[1] * p[1] + params.m1[2];
            let got = (qx * qx + qy * qy).sqrt();
            assert!(
                (got - want).abs() < 1e-4,
                "t mismatch at {p:?}: folded {got} vs sampled {want}"
            );
        }
    }

    /// The WGSL gradient blocks must stay in sync with the Rust-side
    /// constants: uniform array length = [`MAX_FRAME_GRADIENTS`], ramp
    /// row addressing = [`GRADIENT_RAMP_WIDTH`].
    #[test]
    fn stock_vector_shaders_match_gradient_contract() {
        use crate::shader::stock_wgsl;
        let array_decl = format!("array<GradientParams, {MAX_FRAME_GRADIENTS}>");
        let inset = format!(
            "(0.5 + t * {}.0) / {}.0",
            GRADIENT_RAMP_WIDTH - 1,
            GRADIENT_RAMP_WIDTH
        );
        for (name, source) in [
            ("vector", stock_wgsl::VECTOR),
            ("vector_relief", stock_wgsl::VECTOR_RELIEF),
            ("vector_glass", stock_wgsl::VECTOR_GLASS),
        ] {
            assert!(
                source.contains(&array_decl),
                "{name}.wgsl gradient table length must be MAX_FRAME_GRADIENTS"
            );
            assert!(
                source.contains(&inset),
                "{name}.wgsl ramp inset must match GRADIENT_RAMP_WIDTH"
            );
            assert!(
                source.contains("fn vector_paint"),
                "{name}.wgsl must resolve paint via vector_paint"
            );
        }
    }

    /// SVG 13.2.2/13.2.3: a linear gradient with `p1 == p2` (or a
    /// radial with `r == 0`) paints the last stop's colour regardless
    /// of spread method — on both the CPU sampler and the folded GPU
    /// params.
    #[test]
    fn degenerate_gradients_paint_the_last_stop() {
        let stops = vec![
            VectorGradientStop {
                offset: 0.0,
                color: [1.0, 0.0, 0.0, 1.0],
            },
            VectorGradientStop {
                offset: 1.0,
                color: [0.0, 0.0, 1.0, 1.0],
            },
        ];
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let linear = VectorGradient::Linear(VectorLinearGradient {
            p1: [10.0, 10.0],
            p2: [10.0, 10.0],
            stops: stops.clone(),
            // Repeat is the adversarial spread: fract(t) would wrap a
            // naive t = 1 back to the first stop.
            spread: VectorSpreadMethod::Repeat,
            absolute_to_local: identity,
        });
        let radial = VectorGradient::Radial(VectorRadialGradient {
            center: [10.0, 10.0],
            radius: 0.0,
            focal: [10.0, 10.0],
            focal_radius: 0.0,
            stops: stops.clone(),
            spread: VectorSpreadMethod::Repeat,
            absolute_to_local: identity,
        });

        for gradient in [&linear, &radial] {
            for p in [[0.0, 0.0], [10.0, 10.0], [55.0, -3.0]] {
                assert_eq!(
                    sample_gradient(gradient, p),
                    stops[1].color,
                    "CPU sample must be the last stop"
                );
            }
            let params = fold_gradient_params(gradient, 1.0);
            // Constant t = 1 under pad spread, for any fragment position.
            let t = match gradient {
                VectorGradient::Linear(_) => params.m0[2],
                VectorGradient::Radial(_) => {
                    (params.m0[2] * params.m0[2] + params.m1[2] * params.m1[2]).sqrt()
                }
            };
            assert_eq!(params.m0[0], 0.0);
            assert_eq!(params.m0[1], 0.0);
            assert!((t - 1.0).abs() < 1e-6, "folded t must be constant 1");
            assert_eq!(params.m1[3], 0.0, "spread forced to pad");
        }
    }

    /// Slot allocation dedupes identical `(gradient, opacity)` paints,
    /// separates distinct opacities, and refuses past the frame budget.
    #[test]
    fn gradient_frame_dedupes_and_caps() {
        let asset = two_stop_asset();
        let gradient = &asset.gradients[0];

        let mut frame = VectorGradientFrame::new();
        frame.begin(ColorSpace::SRGB_LINEAR);
        assert_eq!(frame.allocate(gradient, 1.0), Some(0));
        assert_eq!(frame.allocate(gradient, 1.0), Some(0), "dedup");
        assert_eq!(frame.allocate(gradient, 0.5), Some(1), "opacity is keyed");
        assert_eq!(frame.slot_count(), 2);
        assert_eq!(frame.params()[1].misc[1], 0.5);

        // Distinct synthetic gradients exhaust the budget; overflow
        // returns None rather than aliasing a slot.
        let mut g = match gradient {
            VectorGradient::Linear(g) => g.clone(),
            _ => unreachable!(),
        };
        for i in 2..MAX_FRAME_GRADIENTS {
            g.p2[0] += 1.0;
            assert_eq!(
                frame.allocate(&VectorGradient::Linear(g.clone()), 1.0),
                Some(i as u32)
            );
        }
        g.p2[0] += 1.0;
        assert_eq!(
            frame.allocate(&VectorGradient::Linear(g.clone()), 1.0),
            None
        );
        // Existing slots still dedupe after the cap.
        assert_eq!(frame.allocate(gradient, 1.0), Some(0));
    }

    /// Tessellating with a gradient frame stamps `slot + 1` into
    /// `meta[2]`; without one, `meta[2]` stays 0.
    #[test]
    fn tessellation_stamps_gradient_slots_into_meta() {
        let asset = five_stop_asset();
        let options = VectorMeshOptions::icon(
            crate::tree::Rect::new(0.0, 0.0, 100.0, 200.0),
            Color::srgb_u8(0, 0, 0),
            2.0,
            ColorSpace::SRGB_LINEAR,
        );

        let mut frame = VectorGradientFrame::new();
        frame.begin(ColorSpace::SRGB_LINEAR);
        let mut vertices = Vec::new();
        let run = append_vector_asset_mesh(&asset, options, &mut vertices, Some(&mut frame));
        assert!(run.count > 0);
        assert_eq!(frame.slot_count(), 1);
        for v in &vertices {
            assert_eq!(v.meta[2], 1.0, "gradient fill carries slot+1");
        }

        let plain = tessellate_vector_asset(&asset, options);
        for v in &plain.vertices {
            assert_eq!(v.meta[2], 0.0, "no frame → per-vertex paint only");
        }
    }

    /// Regression for #77: tess vertex colors must land in the mesh's
    /// working color space — solid fills, `currentColor`, and gradient
    /// samples alike. In Display-P3-linear, sRGB red keeps a non-zero
    /// green component; the old default-space packing left it at 0.
    #[test]
    fn vertex_colors_convert_into_the_working_space() {
        let p3 = ColorSpace::DISPLAY_P3_LINEAR;
        let red = Color::srgb_u8(255, 0, 0);
        let expected = rgba_f32_in(red, p3);
        assert!(expected[1] > 0.01, "P3 red must have green > 0");

        // Solid authored fill.
        let solid = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect width="100" height="100" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let mesh = tessellate_vector_asset(
            &solid,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, 100.0, 100.0),
                Color::srgb_u8(0, 0, 0),
                2.0,
                p3,
            ),
        );
        for v in &mesh.vertices {
            for (got, want) in v.color.iter().zip(&expected) {
                assert!(
                    (got - want).abs() < 1e-5,
                    "solid fill should pack P3-converted red, got {:?} want {expected:?}",
                    v.color
                );
            }
        }

        // Gradient with identical red endpoints: the lerp is constant, so
        // every sampled vertex must also be the P3-converted red.
        let grad = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                <defs>
                    <linearGradient id="g" x1="0" y1="0" x2="100" y2="0" gradientUnits="userSpaceOnUse">
                        <stop offset="0" stop-color="#ff0000"/>
                        <stop offset="1" stop-color="#ff0000"/>
                    </linearGradient>
                </defs>
                <rect width="100" height="100" fill="url(#g)"/>
            </svg>"##,
        )
        .unwrap();
        let mesh = tessellate_vector_asset(
            &grad,
            VectorMeshOptions::icon(
                crate::tree::Rect::new(0.0, 0.0, 100.0, 100.0),
                Color::srgb_u8(0, 0, 0),
                2.0,
                p3,
            ),
        );
        assert!(!mesh.vertices.is_empty());
        for v in &mesh.vertices {
            for (got, want) in v.color.iter().zip(&expected) {
                assert!(
                    (got - want).abs() < 1e-4,
                    "gradient sample should be P3-converted red, got {:?} want {expected:?}",
                    v.color
                );
            }
        }
    }
}
