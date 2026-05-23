//! Paint-stream types and helpers shared by every backend.
//!
//! The `QuadInstance` ABI is the cross-backend contract: every
//! rect-shaped pipeline (stock or custom) reads the same 4 × `vec4<f32>`
//! layout, so the layout pass's logical-pixel rects compose with each
//! backend's GPU pipelines without per-backend tweaking. `aetna-wgpu`
//! and `aetna-vulkano` build different pipelines around it; the bytes
//! the vertex shader sees are identical.
//!
//! Color values in uniform slots are converted into the renderer's
//! *working color space* exactly once at pack time. See
//! [`shader_contract`](mod@shader_contract) for what custom shaders
//! receive in those slots and what they're expected to write.
//!
//! `PaintItem` + `InstanceRun` + [`close_run`] are the paint-stream
//! batching shape: walk the [`crate::DrawOp`] list, pack `Quad`s into
//! the instance buffer in groups of consecutive same-pipeline +
//! same-scissor runs, intersperse text layers in their original
//! z-order. Both backends consume this exactly the same way.
//!
//! The one paint concern this module *doesn't* own is `set_scissor` —
//! that one needs the backend-specific encoder type, so each backend
//! keeps a thin `set_scissor` of its own.
//!
//! Sibling modules:
//! - [`ir`] — the backend-neutral [`DrawOp`] enum the El tree resolves into.
//! - [`draw_ops`] — the producer pass that walks the tree + state and emits `DrawOp`s.
//! - [`shader`] — `ShaderHandle`, `StockShader`, uniform-block types.
//! - [`surface`] — `AppTexture` / surface-format types for host-owned textures.

pub mod draw_ops;
pub mod ir;
pub mod shader;
pub mod surface;

// Documentation-only module so the `shader_contract.md` reference in
// the module-level doc resolves via rustdoc.
#[doc = include_str!("shader_contract.md")]
pub mod shader_contract {}

use bytemuck::{Pod, Zeroable};

use crate::tree::{Color, Rect};
use crate::vector::IconMaterial;
use shader::{ShaderHandle, StockShader, UniformBlock, UniformValue};

/// One instance of a rect-shaped shader. Layout is shared between
/// `stock::rounded_rect` and any custom shader registered via the host's
/// `register_shader`. The fragment shader interprets the slots however
/// it wants; the vertex shader uses `rect` to place the unit quad in
/// pixel space.
///
/// `inner_rect` is the original layout rect — equal to `rect` when
/// `paint_overflow` is zero, smaller (set inside `rect`) when the
/// element has opted into painting outside its bounds. SDF shaders
/// anchor their geometry to `inner_rect` so the rounded outline stays
/// where layout placed it; the overflow band is where focus rings,
/// drop shadows, and other halos render.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct QuadInstance {
    /// Painted rect — xy = top-left px, zw = size px. Equal to
    /// `inner_rect` when no `paint_overflow`. Vertex shader reads at
    /// `@location(1)`.
    pub rect: [f32; 4],
    /// `vec_a` slot — for stock::rounded_rect, this is `fill`. Vertex
    /// shader reads at `@location(2)`.
    pub slot_a: [f32; 4],
    /// `vec_b` slot — for stock::rounded_rect, this is `stroke`.
    /// Vertex shader reads at `@location(3)`.
    pub slot_b: [f32; 4],
    /// `vec_c` slot — for stock::rounded_rect, this is
    /// `(stroke_width, max_radius, shadow, focus_width)`. Positive
    /// `focus_width` draws outside the layout rect; negative draws inside.
    /// `max_radius`
    /// is the largest of the four per-corner radii (in `slot_e`); it
    /// stays here so custom shaders that read scalar `slot_c.y` as
    /// the radius keep working when corners are uniform. Vertex
    /// shader reads at `@location(4)`.
    pub slot_c: [f32; 4],
    /// Layout rect (xy = top-left px, zw = size px). SDF shaders use
    /// this so the rect outline stays anchored to layout bounds even
    /// when `rect` has been outset for `paint_overflow`. Vertex shader
    /// reads at `@location(5)` — declared *after* the legacy slots so
    /// custom shaders that only consume locations 1..=4 keep working
    /// unchanged.
    pub inner_rect: [f32; 4],
    /// `vec_d` slot — for stock::rounded_rect, this is the ring
    /// color (rgba) with eased alpha already multiplied in. Zero when
    /// the node isn't focused or isn't focusable. Vertex shader reads
    /// at `@location(6)`.
    pub slot_d: [f32; 4],
    /// `vec_e` slot — for stock::rounded_rect, this is per-corner
    /// radii in `(tl, tr, br, bl)` order (logical px). Custom shaders
    /// that don't care about per-corner shapes can ignore this slot.
    /// Vertex shader reads at `@location(7)`.
    pub slot_e: [f32; 4],
}

/// One line-segment primitive in a vector icon. The instance renders a
/// single antialiased stroke into `rect`; higher-level icon paths are
/// flattened into runs of these records by the backend recorder.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct IconInstance {
    /// Painted bounds for the segment, outset for stroke width and AA.
    /// Vertex shader reads at `@location(1)`.
    pub rect: [f32; 4],
    /// Segment endpoints in logical px: `(x0, y0, x1, y1)`.
    /// Fragment shader reads at `@location(2)`.
    pub line: [f32; 4],
    /// Linear rgba color. Fragment shader reads at `@location(3)`.
    pub color: [f32; 4],
    /// `(stroke_width, reserved, reserved, reserved)`.
    /// Fragment shader reads at `@location(4)`.
    pub params: [f32; 4],
}

/// A contiguous run of instances drawn with the same pipeline + scissor.
/// Built in tree order so a custom shader sandwiched between two stock
/// surfaces is drawn at the right z-position.
#[derive(Clone, Copy)]
pub struct InstanceRun {
    pub handle: ShaderHandle,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

/// Which icon-draw path a backend uses for this run.
///
/// `Tess` runs index into the backend's tessellated vector mesh
/// (vertex range, expanded triangles). `Msdf` runs index into the
/// backend's per-instance MSDF buffer (one entry = one icon quad) and
/// must bind the atlas page identified by `IconRun::page`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconRunKind {
    Tess,
    Msdf,
}

/// A contiguous run of backend-owned icon draws sharing a scissor.
///
/// For `Tess` runs, `first..first+count` is a vertex range in the
/// backend's vector-mesh buffer and `material` selects the fragment
/// shader (flat / relief / glass). For `Msdf` runs, `first..first+count`
/// is an instance range in the backend's MSDF instance buffer; `page`
/// names the atlas page to bind. `material` is always `Flat` for MSDF
/// runs — non-flat materials need the per-fragment local view-box
/// coordinate that the tessellated path provides, so they stay on the
/// `Tess` route.
#[derive(Clone, Copy)]
pub struct IconRun {
    pub kind: IconRunKind,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
    pub page: u32,
    pub material: IconMaterial,
}

/// Scissor in **physical pixels** (host swapchain extent), already
/// clamped to the surface and snapped to integer pixel boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalScissor {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Sequencing entry for the recorded paint stream.
///
/// - `QuadRun(idx)` — a contiguous instance run (indexed into `runs`).
/// - `IconRun(idx)` — a vector icon run (backend-owned storage,
///   indexed by the wgpu icon painter; other backends may keep using
///   text fallback and never emit this item).
/// - `Text(idx)` — a glyph layer (indexed into the backend's
///   `TextLayer` vector).
/// - `BackdropSnapshot` — a pass boundary. The backend ends the
///   current render pass, copies the current target into its managed
///   snapshot texture, and begins a new pass with `LoadOp::Load` so
///   subsequent quads can sample the snapshot via the `backdrop` bind
///   group. At most one of these is emitted per frame, inserted by
///   [`crate::runtime::RunnerCore::prepare_paint`] immediately before
///   the first quad bound to a `samples_backdrop` shader.
#[derive(Clone, Copy)]
pub enum PaintItem {
    QuadRun(usize),
    IconRun(usize),
    Text(usize),
    /// One raster image draw. Indexes into the backend's
    /// `ImagePaint`-equivalent storage. Produced by
    /// [`crate::runtime::TextRecorder::record_image`] from a
    /// [`crate::ir::DrawOp::Image`].
    Image(usize),
    /// One app-owned-texture composite. Indexes into the backend's
    /// `SurfacePaint`-equivalent storage. Produced by the backend's
    /// surface recorder from a [`crate::ir::DrawOp::AppTexture`].
    AppTexture(usize),
    /// One app-supplied vector draw. Indexes into the backend's vector
    /// storage; explicit render mode determines whether that storage is
    /// tessellated geometry or an MSDF atlas entry. Produced from a
    /// [`crate::ir::DrawOp::Vector`].
    Vector(usize),
    BackdropSnapshot,
}

/// Close the current run and append it to `runs` + `paint_items`. No-op
/// when `run_key` is `None` or the run is empty.
pub fn close_run(
    runs: &mut Vec<InstanceRun>,
    paint_items: &mut Vec<PaintItem>,
    run_key: Option<(ShaderHandle, Option<PhysicalScissor>)>,
    first: u32,
    end: u32,
) {
    if let Some((handle, scissor)) = run_key {
        let count = end - first;
        if count > 0 {
            let index = runs.len();
            runs.push(InstanceRun {
                handle,
                scissor,
                first,
                count,
            });
            paint_items.push(PaintItem::QuadRun(index));
        }
    }
}

/// Convert a logical-pixel scissor to physical pixels, clamping to the
/// physical viewport. Returns `None` when the input is `None`.
pub fn physical_scissor(
    scissor: Option<Rect>,
    scale: f32,
    viewport_px: (u32, u32),
) -> Option<PhysicalScissor> {
    let r = scissor?;
    let x1 = (r.x * scale).floor().clamp(0.0, viewport_px.0 as f32) as u32;
    let y1 = (r.y * scale).floor().clamp(0.0, viewport_px.1 as f32) as u32;
    let x2 = (r.right() * scale).ceil().clamp(0.0, viewport_px.0 as f32) as u32;
    let y2 = (r.bottom() * scale).ceil().clamp(0.0, viewport_px.1 as f32) as u32;
    Some(PhysicalScissor {
        x: x1,
        y: y1,
        w: x2.saturating_sub(x1),
        h: y2.saturating_sub(y1),
    })
}

/// Pack a quad's uniforms into the shared `QuadInstance` layout. Stock
/// `rounded_rect` reads its named uniforms; everything else reads the
/// generic `vec_a`/`vec_b`/`vec_c`/`vec_d` slots. `inner_rect` falls
/// back to `rect` when the uniform isn't supplied — i.e. when the node
/// has no `paint_overflow`.
///
/// Defaults to the [`DEFAULT_WORKING_COLOR_SPACE`]; use
/// [`pack_instance_in`] to target a wide-gamut working space.
pub fn pack_instance(rect: Rect, shader: ShaderHandle, uniforms: &UniformBlock) -> QuadInstance {
    pack_instance_in(rect, shader, uniforms, DEFAULT_WORKING_COLOR_SPACE)
}

/// Like [`pack_instance`], but with an explicit working color space.
/// Hosts wiring up an HDR / wide-gamut surface pass their renderer's
/// chosen working space (e.g. `SCRGB_LINEAR` for an `Rgba16Float`
/// surface) so paint slots land already in that space.
pub fn pack_instance_in(
    rect: Rect,
    shader: ShaderHandle,
    uniforms: &UniformBlock,
    working: crate::color::ColorSpace,
) -> QuadInstance {
    let rect_arr = [rect.x, rect.y, rect.w, rect.h];
    let inner_rect = uniforms
        .get("inner_rect")
        .map(|v| value_to_vec4_in(v, working))
        .unwrap_or(rect_arr);
    let to_lin = |c: Color| rgba_f32_in(c, working);

    match shader {
        ShaderHandle::Stock(StockShader::RoundedRect) => {
            let radii = uniforms.get("radii").map(|v| value_to_vec4_in(v, working));
            // Fall back to the scalar `radius` uniform when no
            // per-corner block was inserted (custom callers, focus
            // ring band, etc.). Either path produces a valid
            // four-corner instance — callers that only set scalar
            // `radius` get uniform corners.
            let scalar_radius = uniforms.get("radius").and_then(as_f32).unwrap_or(0.0);
            let radii = radii.unwrap_or([scalar_radius; 4]);
            let max_radius = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);
            QuadInstance {
                rect: rect_arr,
                inner_rect,
                slot_a: uniforms
                    .get("fill")
                    .and_then(as_color)
                    .map(to_lin)
                    .unwrap_or([0.0; 4]),
                slot_b: uniforms
                    .get("stroke")
                    .and_then(as_color)
                    .map(to_lin)
                    .unwrap_or([0.0; 4]),
                slot_c: [
                    uniforms.get("stroke_width").and_then(as_f32).unwrap_or(0.0),
                    max_radius,
                    uniforms.get("shadow").and_then(as_f32).unwrap_or(0.0),
                    uniforms.get("focus_width").and_then(as_f32).unwrap_or(0.0),
                ],
                slot_d: uniforms
                    .get("focus_color")
                    .and_then(as_color)
                    .map(to_lin)
                    .unwrap_or([0.0; 4]),
                slot_e: radii,
            }
        }
        _ => QuadInstance {
            rect: rect_arr,
            inner_rect,
            slot_a: uniforms
                .get("vec_a")
                .map(|v| value_to_vec4_in(v, working))
                .unwrap_or([0.0; 4]),
            slot_b: uniforms
                .get("vec_b")
                .map(|v| value_to_vec4_in(v, working))
                .unwrap_or([0.0; 4]),
            slot_c: uniforms
                .get("vec_c")
                .map(|v| value_to_vec4_in(v, working))
                .unwrap_or([0.0; 4]),
            slot_d: uniforms
                .get("vec_d")
                .map(|v| value_to_vec4_in(v, working))
                .unwrap_or([0.0; 4]),
            slot_e: uniforms
                .get("vec_e")
                .map(|v| value_to_vec4_in(v, working))
                .unwrap_or([0.0; 4]),
        },
    }
}

fn as_color(v: &UniformValue) -> Option<Color> {
    match v {
        UniformValue::Color(c) => Some(*c),
        _ => None,
    }
}
fn as_f32(v: &UniformValue) -> Option<f32> {
    match v {
        UniformValue::F32(f) => Some(*f),
        _ => None,
    }
}

/// Coerce any `UniformValue` into the four floats of a vec4 slot.
/// Custom-shader authors typically pass `Color` (rgba) or `Vec4`
/// (arbitrary semantics); `F32` packs into `.x` so a single scalar like
/// `radius` doesn't need a Vec4 wrapper.
fn value_to_vec4_in(v: &UniformValue, working: crate::color::ColorSpace) -> [f32; 4] {
    match v {
        UniformValue::Color(c) => rgba_f32_in(*c, working),
        UniformValue::Vec4(a) => *a,
        UniformValue::Vec2([x, y]) => [*x, *y, 0.0, 0.0],
        UniformValue::F32(f) => [*f, 0.0, 0.0, 0.0],
        UniformValue::Bool(b) => [if *b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    }
}

/// The default renderer working color space — sRGB primaries, linear
/// transfer. Existing backends (`aetna-wgpu`, `aetna-vulkano`, …) all
/// composite in this space and write to an `…UnormSrgb` swapchain.
///
/// Hosts targeting wide-gamut / HDR surfaces (Wayland color-management,
/// macOS Display-P3) construct paint values via
/// [`pack_instance_in`] / [`rgba_f32_in`] with a different
/// [`crate::color::ColorSpace`] — for example
/// [`crate::color::ColorSpace::SCRGB_LINEAR`] for fp16 surfaces.
pub const DEFAULT_WORKING_COLOR_SPACE: crate::color::ColorSpace =
    crate::color::ColorSpace::SRGB_LINEAR;

/// Convert a [`Color`] (in any [`crate::color::ColorSpace`]) to the four
/// linear floats the shader reads. Defaults to the
/// [`DEFAULT_WORKING_COLOR_SPACE`]; use [`rgba_f32_in`] to target a
/// different working space.
///
/// Alpha is left straight (not premultiplied) — historical semantics the
/// stock shader pipeline relies on.
pub fn rgba_f32(c: Color) -> [f32; 4] {
    rgba_f32_in(c, DEFAULT_WORKING_COLOR_SPACE)
}

/// Like [`rgba_f32`], but converts into an explicit working color space.
/// Hosts that opt their backend into a wide-gamut working space (e.g.
/// `SCRGB_LINEAR` for an fp16 Rgba16Float surface) call this from their
/// `draw_pass_to_quads` equivalent so paint values land pre-converted.
pub fn rgba_f32_in(c: Color, working: crate::color::ColorSpace) -> [f32; 4] {
    let lin = c.convert_to(working);
    [lin.r, lin.g, lin.b, lin.a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::UniformBlock;
    use crate::tokens;

    #[test]
    fn focus_uniforms_pack_into_rounded_rect_slots() {
        // Focus ring rides on the node's own RoundedRect quad: focus_color
        // packs into slot_d (rgba) and focus_width into slot_c.w (the
        // params slot's previously-padding lane).
        let mut uniforms = UniformBlock::new();
        uniforms.insert("fill", UniformValue::Color(Color::srgb_u8a(40, 40, 40, 255)));
        uniforms.insert("radius", UniformValue::F32(8.0));
        uniforms.insert("focus_color", UniformValue::Color(tokens::RING));
        uniforms.insert("focus_width", UniformValue::F32(tokens::RING_WIDTH));

        let inst = pack_instance(
            Rect::new(1.0, 2.0, 30.0, 40.0),
            ShaderHandle::Stock(StockShader::RoundedRect),
            &uniforms,
        );

        assert_eq!(inst.rect, [1.0, 2.0, 30.0, 40.0]);
        assert_eq!(
            inst.inner_rect, inst.rect,
            "no inner_rect uniform → fall back to painted rect"
        );
        assert_eq!(
            inst.slot_c[1], 8.0,
            "max corner radius in slot_c.y (uniform corners derived from scalar `radius` uniform)"
        );
        assert_eq!(
            inst.slot_e,
            [8.0, 8.0, 8.0, 8.0],
            "scalar `radius` uniform fills all four corners on slot_e"
        );
        assert_eq!(
            inst.slot_c[3],
            tokens::RING_WIDTH,
            "focus_width in slot_c.w"
        );
        assert!(inst.slot_d[3] > 0.0, "focus_color alpha should be visible");
    }

    #[test]
    fn per_corner_radii_uniform_routes_to_slot_e() {
        // The `radii` uniform overrides the scalar `radius` for the
        // SDF, while `slot_c.y` carries the max corner so custom
        // shaders that read scalar `slot_c.y` still see the right
        // shape silhouette.
        let mut uniforms = UniformBlock::new();
        uniforms.insert("fill", UniformValue::Color(Color::srgb_u8a(40, 40, 40, 255)));
        // Top-rounded only — the strip-on-card shape.
        uniforms.insert("radii", UniformValue::Vec4([12.0, 12.0, 0.0, 0.0]));
        uniforms.insert("radius", UniformValue::F32(12.0));

        let inst = pack_instance(
            Rect::new(0.0, 0.0, 100.0, 40.0),
            ShaderHandle::Stock(StockShader::RoundedRect),
            &uniforms,
        );

        assert_eq!(inst.slot_e, [12.0, 12.0, 0.0, 0.0]);
        assert_eq!(inst.slot_c[1], 12.0, "max corner radius -> slot_c.y");
    }

    #[test]
    fn physical_scissor_converts_logical_to_physical_pixels() {
        let scissor = physical_scissor(Some(Rect::new(10.2, 20.2, 30.2, 40.2)), 2.0, (200, 200))
            .expect("scissor");

        assert_eq!(
            scissor,
            PhysicalScissor {
                x: 20,
                y: 40,
                w: 61,
                h: 81
            }
        );
    }
}
