//! SVG fallback renderer — approximate visual output for the agent loop.
//!
//! The production renderer is wgpu; SVG is an explicitly-approximate
//! fixture renderer. It exists so the agent feedback loop
//! (edit Rust → render → look at PNG + lint) keeps working without
//! standing up a GPU surface.
//!
//! Stock shaders are interpreted best-effort:
//!
//! - `stock::rounded_rect` → `<rect>` with `fill`, `stroke`, `rx` from
//!   uniforms and an SVG drop-shadow filter. When `focus_color` and
//!   `focus_width` uniforms are present (set by `draw_ops` for any
//!   focusable node whose focus envelope is active), an additional
//!   stroke-only `<rect>` is emitted just outside `inner_rect` to
//!   approximate the focus ring drawn by the GPU shader.
//! - `stock::text_sdf` → `<text>` element with font + color from the op.
//! - `Scene3D` (incl. the 2D-plot data layer) → point/line geometry
//!   projected through the scene camera into `<polyline>`/`<circle>`;
//!   meshes keep a labelled placeholder footprint.
//! - Custom shaders → labeled placeholder rect with metadata in
//!   `<title>` and a translucent fill so they're visible during fixture
//!   rendering. The wgpu renderer is the source of truth for custom
//!   shaders; SVG just lets you see the layout.

use std::fmt::Write as _;

use crate::icons;
use crate::icons::svg::IconSource;
use crate::ir::*;
use crate::shader::*;
use crate::text::metrics as text_metrics;
use crate::tokens;
use crate::tree::*;
use crate::vector::{VectorAsset, VectorSegment};

/// Render a `Vec<DrawOp>` to an SVG string.
pub fn svg_from_ops(width: f32, height: f32, ops: &[DrawOp], bg: Color) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    s.push_str(SHADOW_DEFS);
    let _ = writeln!(
        s,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        color_svg(bg)
    );

    for (i, op) in ops.iter().enumerate() {
        if let Some(scissor) = op.scissor() {
            let clip_id = format!("clip-{i}");
            let _ = writeln!(
                s,
                r#"<clipPath id="{clip_id}"><rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}"/></clipPath><g clip-path="url(#{clip_id})">"#,
                scissor.x, scissor.y, scissor.w, scissor.h,
            );
            emit_op(&mut s, op);
            s.push_str("</g>\n");
        } else {
            emit_op(&mut s, op);
        }
    }
    s.push_str("</svg>\n");
    s
}

fn emit_op(s: &mut String, op: &DrawOp) {
    match op {
        DrawOp::Quad {
            id,
            rect,
            shader,
            uniforms,
            ..
        } => emit_quad(s, id, *rect, shader, uniforms),
        DrawOp::GlyphRun {
            id,
            rect,
            color,
            size,
            family,
            mono_family,
            weight,
            mono,
            wrap,
            anchor,
            layout,
            underline,
            strikethrough,
            link,
            ..
        } => {
            // Links override color with the link-foreground token and
            // imply an underline; this mirrors `RunStyle::with_link`
            // so the SVG fallback paints what the GPU paths paint.
            let (eff_color, eff_underline) = if link.is_some() {
                (crate::tokens::LINK_FOREGROUND, true)
            } else {
                (*color, *underline)
            };
            emit_glyph_run(
                s,
                id,
                *rect,
                eff_color,
                *size,
                *family,
                *mono_family,
                *weight,
                *mono,
                *wrap,
                *anchor,
                layout,
                eff_underline,
                *strikethrough,
            );
        }
        DrawOp::AttributedText {
            id,
            rect,
            runs,
            size,
            wrap,
            anchor,
            layout,
            ..
        } => {
            emit_attributed_text(s, id, *rect, *size, *wrap, *anchor, runs, layout);
        }
        DrawOp::Icon {
            id,
            rect,
            source,
            color,
            stroke_width,
            ..
        } => emit_icon(s, id, *rect, source, *color, *stroke_width),
        DrawOp::Image {
            id, rect, image, ..
        } => emit_image_placeholder(s, id, *rect, image),
        DrawOp::AppTexture {
            id, rect, texture, ..
        } => emit_surface_placeholder(s, id, *rect, texture),
        DrawOp::Vector {
            id,
            rect,
            asset,
            render_mode,
            ..
        } => emit_vector(s, id, *rect, asset, *render_mode),
        // Point and line marks project through the scene camera (an affine
        // map — no GPU needed) into real `<polyline>`/`<circle>` geometry,
        // so the headless review loop sees a plot's actual data layer.
        // Meshes can't be rasterised here; scenes that carry them keep the
        // labelled placeholder footprint alongside any projected geometry.
        DrawOp::Scene3D {
            id, rect, scene, ..
        } => emit_scene(s, id, *rect, scene),
        DrawOp::BackdropSnapshot => {} // v2 — no SVG analogue.
    }
}

/// Project a [`DrawOp::Scene3D`]'s point/line geometry through its resolved
/// camera into SVG primitives. The projection is the same
/// [`project_to_screen`](crate::scene::ResolvedCamera::project_to_screen)
/// math the GPU pipelines encode, so for the 2D-plot data layer (an
/// orthographic camera over `z = 0` geometry) the output is exact; for a
/// true 3D scene it is a wireframe/point-cloud preview. Meshes have no SVG
/// analogue — scenes containing them also draw the dashed placeholder
/// footprint. The wrapping `<g>` carries per-mark evidence
/// (`data-lines`/`data-points`/`data-meshes`) for headless reviewers.
fn emit_scene(s: &mut String, id: &str, rect: Rect, scene: &crate::scene::Scene3DData) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let n_segments: usize = scene
        .lines
        .iter()
        .map(|l| l.geometry.snapshot().0.segments.len())
        .sum();
    let n_points: usize = scene
        .points
        .iter()
        .filter(|p| !p.line_joins)
        .map(|p| p.geometry.snapshot().0.points.len())
        .sum();
    let _ = writeln!(
        s,
        r#"<g data-node="{}" data-kind="scene3d" data-lines="{}" data-points="{}" data-meshes="{}">"#,
        esc(id),
        n_segments,
        n_points,
        scene.meshes.len(),
    );
    // Meshes (or a scene with nothing projectable) keep the placeholder so
    // the footprint stays visible in the fixture render.
    if !scene.meshes.is_empty() || (n_segments == 0 && n_points == 0) {
        let _ = writeln!(
            s,
            r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="none" stroke="gray" stroke-dasharray="4 3"/>"#,
            rect.x, rect.y, rect.w, rect.h,
        );
        let label = if scene.meshes.is_empty() {
            "3D scene".to_string()
        } else {
            format!("3D scene ({} meshes)", scene.meshes.len())
        };
        let _ = writeln!(
            s,
            r#"<text x="{:.2}" y="{:.2}" font-size="11" fill="gray">{label}</text>"#,
            rect.x + 6.0,
            rect.y + 16.0,
        );
    }

    // Line marks: chain contiguous same-colour segments back into
    // `<polyline>`s (the plot lowering emits per-series consecutive
    // segments, so a whole series round-trips to one polyline).
    for draw in &scene.lines {
        let (data, _rev) = draw.geometry.snapshot();
        let width = draw.style.width.max(0.5);
        let mut chain: Vec<(f32, f32)> = Vec::new();
        let mut chain_color = [0.0f32; 4];
        let mut prev_end: Option<glam::Vec3> = None;
        for seg in &data.segments {
            let joined =
                !chain.is_empty() && prev_end == Some(seg.start) && chain_color == seg.color;
            if !joined {
                emit_scene_polyline(s, &chain, chain_color, width);
                chain.clear();
            }
            let a = scene
                .camera
                .project_to_screen(draw.transform.transform_point3(seg.start), rect);
            let b = scene
                .camera
                .project_to_screen(draw.transform.transform_point3(seg.end), rect);
            match (a, b) {
                (Some(a), Some(b)) => {
                    if chain.is_empty() {
                        chain.push((a.x, a.y));
                        chain_color = seg.color;
                    }
                    chain.push((b.x, b.y));
                    prev_end = Some(seg.end);
                }
                // An endpoint behind the camera breaks the chain (only
                // reachable in true-3D scenes; the plot ortho camera
                // projects everything).
                _ => {
                    emit_scene_polyline(s, &chain, chain_color, width);
                    chain.clear();
                    prev_end = None;
                }
            }
        }
        emit_scene_polyline(s, &chain, chain_color, width);
    }

    // Point marks as circles. Join discs are skipped: the polylines above
    // already carry round joins/caps, and one disc per vertex would
    // dominate the file (~80% of bytes on a decimated series).
    for draw in scene.points.iter().filter(|p| !p.line_joins) {
        let (data, _rev) = draw.geometry.snapshot();
        let r = (draw.style.size * 0.5).max(0.5);
        for p in &data.points {
            if let Some(sp) = scene
                .camera
                .project_to_screen(draw.transform.transform_point3(p.position), rect)
            {
                let _ = writeln!(
                    s,
                    r#"<circle cx="{:.2}" cy="{:.2}" r="{:.2}" fill="{}"{}/>"#,
                    sp.x,
                    sp.y,
                    r,
                    scene_rgb(p.color),
                    scene_opacity_attr(" fill-opacity", p.color[3]),
                );
            }
        }
    }
    s.push_str("</g>\n");
}

/// Emit one chained polyline, skipping degenerate chains (< 2 points).
fn emit_scene_polyline(s: &mut String, chain: &[(f32, f32)], color: [f32; 4], width: f32) {
    if chain.len() < 2 {
        return;
    }
    let mut pts = String::with_capacity(chain.len() * 14);
    for (x, y) in chain {
        let _ = write!(pts, "{x:.2},{y:.2} ");
    }
    let _ = writeln!(
        s,
        r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="{:.2}" stroke-linejoin="round" stroke-linecap="round"{}/>"#,
        pts.trim_end(),
        scene_rgb(color),
        width,
        scene_opacity_attr(" stroke-opacity", color[3]),
    );
}

/// Scene geometry colours are authoring-space sRGBA `[f32; 4]`.
fn scene_rgb(c: [f32; 4]) -> String {
    format!(
        "rgb({},{},{})",
        (c[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// ` fill-opacity="0.5"`-style attribute, empty when fully opaque.
fn scene_opacity_attr(name: &str, alpha: f32) -> String {
    if alpha >= 1.0 {
        String::new()
    } else {
        format!(r#"{name}="{:.3}""#, alpha.clamp(0.0, 1.0))
    }
}

fn emit_vector(
    s: &mut String,
    id: &str,
    rect: Rect,
    asset: &crate::vector::VectorAsset,
    render_mode: crate::vector::VectorRenderMode,
) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let [vx, vy, vw, vh] = asset.view_box;
    let _ = writeln!(
        s,
        r#"<svg data-node="{}" data-shader="stock::vector" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" viewBox="{:.2} {:.2} {:.2} {:.2}">"#,
        esc(id),
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        vx,
        vy,
        vw,
        vh,
    );
    match render_mode {
        crate::vector::VectorRenderMode::Painted => {
            emit_custom_paths(s, asset, tokens::FOREGROUND, 1.5);
        }
        crate::vector::VectorRenderMode::Mask { color } => {
            emit_mask_paths(s, asset, color);
        }
    }
    s.push_str("</svg>\n");
}

/// Placeholder rect labelled with the image's content hash. The real
/// raster bytes never reach the SVG bundle (kept lean — bundles ship
/// over the wire and are inspected as text), so the dump shows where
/// an image would be and which one without embedding pixel data.
fn emit_image_placeholder(s: &mut String, id: &str, rect: Rect, image: &crate::image::Image) {
    let label = image.label();
    let _ = writeln!(
        s,
        r##"<rect data-node="{}" data-shader="stock::image" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="#444" stroke="#888" stroke-width="1" stroke-dasharray="4 2" />"##,
        esc(id),
        rect.x,
        rect.y,
        rect.w,
        rect.h,
    );
    // Centred label so artifacts stay self-describing.
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let _ = writeln!(
        s,
        r##"<text x="{:.2}" y="{:.2}" text-anchor="middle" dominant-baseline="middle" font-family="monospace" font-size="10" fill="#bbb">{}</text>"##,
        cx,
        cy,
        esc(&label),
    );
}

/// Placeholder rect labelled with the app texture's id and dimensions.
/// SVG bundles can't sample a live GPU texture; the placeholder lets
/// inspection tooling see *where* a surface widget composites without
/// pretending to render its contents.
fn emit_surface_placeholder(
    s: &mut String,
    id: &str,
    rect: Rect,
    texture: &crate::surface::AppTexture,
) {
    let (w, h) = texture.size_px();
    let label = format!("AppTexture#{} {}x{}", texture.id().0, w, h);
    let _ = writeln!(
        s,
        r##"<rect data-node="{}" data-shader="stock::surface" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="#222" stroke="#aaa" stroke-width="1" stroke-dasharray="6 3" />"##,
        esc(id),
        rect.x,
        rect.y,
        rect.w,
        rect.h,
    );
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let _ = writeln!(
        s,
        r##"<text x="{:.2}" y="{:.2}" text-anchor="middle" dominant-baseline="middle" font-family="monospace" font-size="10" fill="#bbb">{}</text>"##,
        cx,
        cy,
        esc(&label),
    );
}

fn emit_quad(s: &mut String, id: &str, rect: Rect, shader: &ShaderHandle, uniforms: &UniformBlock) {
    match shader {
        ShaderHandle::Stock(StockShader::RoundedRect) => {
            let fill = uniforms.get("fill").and_then(as_color);
            let stroke = uniforms.get("stroke").and_then(as_color);
            let stroke_w = uniforms.get("stroke_width").and_then(as_f32).unwrap_or(0.0);
            // Per-corner radii take precedence over the scalar `radius`
            // uniform; if neither is set, every corner is 0.
            let scalar_radius = uniforms.get("radius").and_then(as_f32).unwrap_or(0.0);
            let radii = uniforms
                .get("radii")
                .and_then(as_vec4)
                .unwrap_or([scalar_radius; 4]);
            let shadow = uniforms.get("shadow").and_then(as_f32).unwrap_or(0.0);
            let focus_color = uniforms.get("focus_color").and_then(as_color);
            let focus_width = uniforms.get("focus_width").and_then(as_f32).unwrap_or(0.0);
            // The painted quad's `rect` may be outset for paint_overflow;
            // SVG draws the rounded-rect border at `inner_rect` so the
            // outline stays anchored to the layout bounds.
            let inner = uniforms
                .get("inner_rect")
                .and_then(as_vec4)
                .map(|v| Rect::new(v[0], v[1], v[2], v[3]))
                .unwrap_or(rect);
            let max_r_clamp = (inner.w * 0.5).min(inner.h * 0.5).max(0.0);
            let radii = [
                radii[0].clamp(0.0, max_r_clamp),
                radii[1].clamp(0.0, max_r_clamp),
                radii[2].clamp(0.0, max_r_clamp),
                radii[3].clamp(0.0, max_r_clamp),
            ];
            let uniform_corners =
                radii[0] == radii[1] && radii[1] == radii[2] && radii[2] == radii[3];
            let fill_attr = match fill {
                Some(c) => format!(r#" fill="{}""#, color_svg(c)),
                None => r#" fill="none""#.to_string(),
            };
            let stroke_attr = match stroke {
                Some(c) => format!(
                    r#" stroke="{}" stroke-width="{:.2}""#,
                    color_svg(c),
                    stroke_w
                ),
                None => String::new(),
            };
            let filter = shadow_filter(shadow);
            if uniform_corners {
                let _ = writeln!(
                    s,
                    r#"<rect data-node="{}" data-shader="stock::rounded_rect" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}"{}{}{} />"#,
                    esc(id),
                    inner.x,
                    inner.y,
                    inner.w,
                    inner.h,
                    radii[0],
                    fill_attr,
                    stroke_attr,
                    filter
                );
            } else {
                let _ = writeln!(
                    s,
                    r#"<path data-node="{}" data-shader="stock::rounded_rect" d="{}"{}{}{} />"#,
                    esc(id),
                    rounded_rect_path(inner, radii),
                    fill_attr,
                    stroke_attr,
                    filter
                );
            }
            // Focus ring rides on the same quad: positive focus_width emits
            // a stroke-only overlay outside the inner border; negative emits
            // it inside the border for dense flush rows.
            if focus_width != 0.0
                && let Some(fc) = focus_color
                && fc.a > 0.0
            {
                let ring_width = focus_width.abs();
                let ring_offset = focus_width.signum() * ring_width * 0.5;
                let ring_inner = Rect::new(
                    inner.x - ring_offset,
                    inner.y - ring_offset,
                    inner.w + ring_offset * 2.0,
                    inner.h + ring_offset * 2.0,
                );
                let ring_radii = [
                    (radii[0] + ring_offset).max(0.0),
                    (radii[1] + ring_offset).max(0.0),
                    (radii[2] + ring_offset).max(0.0),
                    (radii[3] + ring_offset).max(0.0),
                ];
                let ring_uniform = ring_radii[0] == ring_radii[1]
                    && ring_radii[1] == ring_radii[2]
                    && ring_radii[2] == ring_radii[3];
                if ring_uniform {
                    let _ = writeln!(
                        s,
                        r#"<rect data-node="{}.ring" data-shader="stock::rounded_rect" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="none" stroke="{}" stroke-width="{:.2}" />"#,
                        esc(id),
                        ring_inner.x,
                        ring_inner.y,
                        ring_inner.w,
                        ring_inner.h,
                        ring_radii[0],
                        color_svg(fc),
                        ring_width
                    );
                } else {
                    let _ = writeln!(
                        s,
                        r#"<path data-node="{}.ring" data-shader="stock::rounded_rect" d="{}" fill="none" stroke="{}" stroke-width="{:.2}" />"#,
                        esc(id),
                        rounded_rect_path(ring_inner, ring_radii),
                        color_svg(fc),
                        ring_width
                    );
                }
            }
        }
        ShaderHandle::Stock(StockShader::SolidQuad) => {
            let fill = uniforms
                .get("fill")
                .and_then(as_color)
                .unwrap_or(tokens::MUTED);
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::solid_quad" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" />"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                color_svg(fill)
            );
        }
        ShaderHandle::Stock(StockShader::DividerLine) => {
            let fill = uniforms
                .get("fill")
                .and_then(as_color)
                .unwrap_or(tokens::BORDER);
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::divider_line" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" />"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                color_svg(fill)
            );
        }
        ShaderHandle::Stock(StockShader::Text) => {
            // text shouldn't appear as a Quad — skip silently.
        }
        ShaderHandle::Stock(StockShader::Image) => {
            // image shouldn't appear as a Quad — `DrawOp::Image`
            // dispatches through `emit_image_placeholder`. Skip
            // silently in case a custom op binds to this shader name.
        }
        ShaderHandle::Stock(StockShader::Skeleton) => {
            // Time-driven; pin to the t=0 (max-alpha) frame so SVG
            // fixtures stay deterministic and show the skeleton at
            // its brightest, most-readable phase.
            let inner = uniforms
                .get("inner_rect")
                .and_then(as_vec4)
                .map(|v| Rect::new(v[0], v[1], v[2], v[3]))
                .unwrap_or(rect);
            let base = uniforms
                .get("vec_a")
                .and_then(as_color)
                .unwrap_or(tokens::MUTED);
            let params = uniforms.get("vec_c").and_then(as_vec4).unwrap_or([0.0; 4]);
            let radius = if params[0] > 0.0 {
                params[0].min(inner.w * 0.5).min(inner.h * 0.5)
            } else {
                tokens::RADIUS_MD.min(inner.w * 0.5).min(inner.h * 0.5)
            };
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::skeleton" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="{}" />"#,
                esc(id),
                inner.x,
                inner.y,
                inner.w,
                inner.h,
                radius,
                color_svg(base),
            );
        }
        ShaderHandle::Stock(StockShader::ProgressIndeterminate) => {
            // Time-driven; pin to the t=0 frame, where the bias puts
            // the bar's center at the middle of the track.
            let inner = uniforms
                .get("inner_rect")
                .and_then(as_vec4)
                .map(|v| Rect::new(v[0], v[1], v[2], v[3]))
                .unwrap_or(rect);
            let bar_color = uniforms
                .get("vec_a")
                .and_then(as_color)
                .unwrap_or(tokens::PRIMARY);
            let track_color = uniforms
                .get("vec_b")
                .and_then(as_color)
                .unwrap_or(tokens::MUTED);
            let params = uniforms.get("vec_c").and_then(as_vec4).unwrap_or([0.0; 4]);
            let radius = if params[0] > 0.0 {
                params[0].min(inner.w * 0.5).min(inner.h * 0.5)
            } else {
                tokens::RADIUS_PILL.min(inner.w * 0.5).min(inner.h * 0.5)
            };
            let bar_w_frac = if params[2] > 0.0 { params[2] } else { 0.35 };
            let bar_w_px = inner.w * bar_w_frac;
            // At t=0 the phase bias places the bar's center at x_norm=0.5.
            let bar_left = inner.x + (inner.w - bar_w_px) * 0.5;
            // Track first, bar over.
            let _ = writeln!(
                s,
                r#"<rect data-node="{}.track" data-shader="stock::progress_indeterminate" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="{}" />"#,
                esc(id),
                inner.x,
                inner.y,
                inner.w,
                inner.h,
                radius,
                color_svg(track_color),
            );
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::progress_indeterminate" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="{}" />"#,
                esc(id),
                bar_left,
                inner.y,
                bar_w_px,
                inner.h,
                radius,
                color_svg(bar_color),
            );
        }
        ShaderHandle::Stock(StockShader::Spinner) => {
            // Time-driven shader; SVG bundles need a deterministic
            // snapshot. The shader's cosine envelope makes t=0 the
            // max-sweep frame (start anchor at 12 o'clock, end anchor
            // 240° clockwise around it), so we mirror that exact
            // geometry here — SVG fallback and Settled-mode PNG agree
            // on what "this is a loader" looks like.
            let inner = uniforms
                .get("inner_rect")
                .and_then(as_vec4)
                .map(|v| Rect::new(v[0], v[1], v[2], v[3]))
                .unwrap_or(rect);
            let arc_color = uniforms
                .get("vec_a")
                .and_then(as_color)
                .unwrap_or(tokens::FOREGROUND);
            let track_color = uniforms.get("vec_b").and_then(as_color);
            let params = uniforms.get("vec_c").and_then(as_vec4).unwrap_or([0.0; 4]);
            let thickness = if params[0] > 0.0 {
                params[0]
            } else {
                (inner.w.min(inner.h) * 0.12).max(1.5)
            };
            let max_sweep = if params[1] > 0.0 {
                params[1]
            } else {
                std::f32::consts::PI * 4.0 / 3.0 // 240°
            };
            let cx = inner.x + inner.w * 0.5;
            let cy = inner.y + inner.h * 0.5;
            let outer_r = inner.w.min(inner.h) * 0.5;
            let center_r = (outer_r - thickness * 0.5).max(0.0);

            // Track only renders when the caller asked for one. Skip
            // the <circle> entirely when fully transparent so the SVG
            // matches the shader's "off region is invisible" default.
            if let Some(track) = track_color
                && track.a > 0.0
            {
                let _ = writeln!(
                    s,
                    r#"<circle data-node="{}.track" data-shader="stock::spinner" cx="{:.2}" cy="{:.2}" r="{:.2}" fill="none" stroke="{}" stroke-width="{:.2}" />"#,
                    esc(id),
                    cx,
                    cy,
                    center_r,
                    color_svg(track),
                    thickness,
                );
            }

            // Arc: starting at 12 o'clock, sweeping clockwise by
            // `max_sweep` radians. SVG arc large-arc flag is 1 when
            // sweep > 180°, sweep flag is 1 for clockwise.
            let large_arc = if max_sweep > std::f32::consts::PI {
                1
            } else {
                0
            };
            let start_x = cx;
            let start_y = cy - center_r;
            let end_x = cx + (max_sweep.sin()) * center_r;
            let end_y = cy - (max_sweep.cos()) * center_r;
            let _ = writeln!(
                s,
                r#"<path data-node="{}" data-shader="stock::spinner" d="M {:.2} {:.2} A {:.2} {:.2} 0 {} 1 {:.2} {:.2}" fill="none" stroke="{}" stroke-width="{:.2}" stroke-linecap="round" />"#,
                esc(id),
                start_x,
                start_y,
                center_r,
                center_r,
                large_arc,
                end_x,
                end_y,
                color_svg(arc_color),
                thickness,
            );
        }
        ShaderHandle::Custom(name) => {
            // Placeholder rect so layout is visible. Real paint requires
            // wgpu + the registered shader.
            let mut title = format!("custom shader: {name}");
            for (k, v) in uniforms {
                let _ = write!(title, " {k}={}", v.debug_short());
            }
            let _ = writeln!(
                s,
                r#"<g data-node="{}" data-shader="custom::{}"><title>{}</title><rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="rgba(255,0,255,0.18)" stroke="rgba(255,0,255,0.5)" stroke-width="1" stroke-dasharray="3 2" /></g>"#,
                esc(id),
                esc(name),
                esc(&title),
                rect.x,
                rect.y,
                rect.w,
                rect.h
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_glyph_run(
    s: &mut String,
    id: &str,
    rect: Rect,
    color: Color,
    size: f32,
    family: crate::tree::FontFamily,
    mono_family: crate::tree::FontFamily,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    anchor: TextAnchor,
    layout: &text_metrics::TextLayout,
    underline: bool,
    strikethrough: bool,
) {
    let (x, anchor_attr) = match anchor {
        TextAnchor::Start => (rect.x, "start"),
        TextAnchor::Middle => (rect.center_x(), "middle"),
        TextAnchor::End => (rect.right(), "end"),
    };
    // For NoWrap text we vertically center the laid-out block within
    // its rect — buttons and badges hand us a control-height rect with
    // a single-line label, and centering the block is what reads as
    // "right". Using the layout's full height (rather than one
    // line-height) keeps multi-line NoWrap text (a code block, a
    // label that contains an embedded `\n`) flush to the top of its
    // hugged rect instead of being shoved down by `(N-1) *
    // line_height / 2`.
    let line_top = match wrap {
        TextWrap::NoWrap => rect.y + ((rect.h - layout.height) * 0.5).max(0.0),
        TextWrap::Wrap => rect.y,
    };
    let family = if mono {
        mono_family.family_name()
    } else {
        family.family_name()
    };
    let weight_str = match weight {
        FontWeight::Regular => "400",
        FontWeight::Medium => "500",
        FontWeight::Semibold => "600",
        FontWeight::Bold => "700",
    };
    let decoration_attr = decoration_attr(underline, strikethrough);
    for line in &layout.lines {
        // SVG uses a baseline; glyphon positions by line top. The core
        // text layout carries Roboto's baseline offset so artifacts stay
        // close to the wgpu text path.
        let y = line_top + line.baseline;
        let _ = writeln!(
            s,
            r#"<text data-node="{}" data-shader="stock::text" x="{:.2}" y="{:.2}" font-family="{}" font-size="{:.2}" font-weight="{}" fill="{}" text-anchor="{}"{}>{}</text>"#,
            esc(id),
            x,
            y,
            family,
            size,
            weight_str,
            color_svg(color),
            anchor_attr,
            decoration_attr,
            esc(&line.text)
        );
    }
}

/// Build the SVG `text-decoration` attribute string for a span. Returns
/// an empty string when neither flag is set (so the attribute is
/// omitted entirely rather than emitted as `text-decoration=""`).
fn decoration_attr(underline: bool, strikethrough: bool) -> &'static str {
    match (underline, strikethrough) {
        (true, true) => r#" text-decoration="underline line-through""#,
        (true, false) => r#" text-decoration="underline""#,
        (false, true) => r#" text-decoration="line-through""#,
        (false, false) => "",
    }
}

/// Degraded SVG emission for attributed text. The wgpu/vulkano paths
/// shape runs through cosmic-text and produce per-glyph color +
/// run_index, so each run can paint its own fill. SVG can't easily
/// reproduce cosmic-text's wrapping decisions, so we collapse the
/// paragraph onto one `<text>` element with one `<tspan>` per run,
/// carrying that run's color / weight / style attributes. This is
/// honest about the rendering layer's limits — the SVG fallback
/// captures *which* runs flowed where in source order, even if it
/// can't replicate the exact line breaks.
///
/// Inline-run backgrounds (`RunStyle.bg`) emit `<rect>` underlays
/// using per-run measured widths accumulated left-to-right. This is
/// approximate: it ignores wrapping (highlights ride on the first
/// line only) and skips `Middle`/`End` anchors where browser-driven
/// alignment can't be predicted without the rendered font's metrics.
#[allow(clippy::too_many_arguments)]
fn emit_attributed_text(
    s: &mut String,
    id: &str,
    rect: Rect,
    size: f32,
    wrap: TextWrap,
    anchor: TextAnchor,
    runs: &[(String, crate::text::atlas::RunStyle)],
    layout: &text_metrics::TextLayout,
) {
    let (x, anchor_attr) = match anchor {
        TextAnchor::Start => (rect.x, "start"),
        TextAnchor::Middle => (rect.center_x(), "middle"),
        TextAnchor::End => (rect.right(), "end"),
    };
    // Same centering shape as `emit_glyph_run`: use the full laid-out
    // height so multi-line NoWrap attributed text stays anchored to
    // the top of its hugged rect.
    let line_top = match wrap {
        TextWrap::NoWrap => rect.y + ((rect.h - layout.height) * 0.5).max(0.0),
        TextWrap::Wrap => rect.y,
    };
    let baseline = layout
        .lines
        .first()
        .map(|l| l.baseline)
        .unwrap_or(layout.line_height * 0.8);
    let y = line_top + baseline;

    // Highlight underlays — emitted before the text so they paint
    // behind the glyphs. Only Start-anchored paragraphs get accurate
    // x positions; we skip the rects for Middle/End rather than guess.
    if matches!(anchor, TextAnchor::Start) {
        let mut cursor_x = rect.x;
        for (text, style) in runs {
            let run_w = text_metrics::line_width(text, size, style.weight, style.mono);
            if let Some(bg) = style.bg {
                let _ = writeln!(
                    s,
                    r#"<rect data-node="{}.run-bg" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" />"#,
                    esc(id),
                    cursor_x,
                    line_top,
                    run_w.max(0.0),
                    layout.line_height,
                    color_svg(bg),
                );
            }
            cursor_x += run_w;
        }
    }

    let _ = write!(
        s,
        r#"<text data-node="{}" data-shader="stock::text" x="{:.2}" y="{:.2}" font-size="{:.2}" text-anchor="{}">"#,
        esc(id),
        x,
        y,
        size,
        anchor_attr,
    );
    for (text, style) in runs {
        let family = if style.mono {
            style.mono_family.family_name()
        } else {
            style.family.family_name()
        };
        let weight_str = match style.weight {
            FontWeight::Regular => "400",
            FontWeight::Medium => "500",
            FontWeight::Semibold => "600",
            FontWeight::Bold => "700",
        };
        let style_attr = if style.italic { "italic" } else { "normal" };
        let decoration_attr = decoration_attr(style.underline, style.strikethrough);
        let _ = write!(
            s,
            r#"<tspan font-family="{}" font-weight="{}" font-style="{}" fill="{}"{}>{}</tspan>"#,
            family,
            weight_str,
            style_attr,
            color_svg(style.color),
            decoration_attr,
            esc(text),
        );
    }
    s.push_str("</text>\n");
}

fn emit_icon(
    s: &mut String,
    id: &str,
    rect: Rect,
    source: &IconSource,
    color: Color,
    stroke_width: f32,
) {
    match source {
        // An unknown icon name paints the AlertCircle fallback, matching
        // the GPU path.
        IconSource::Builtin(_) | IconSource::UnknownName(_) => {
            let name = match source {
                IconSource::Builtin(n) => *n,
                _ => crate::tree::IconName::AlertCircle,
            };
            let path = icons::icon_path(name);
            let stroke = (stroke_width * 24.0 / rect.w.max(rect.h).max(1.0)).max(0.5);
            let _ = writeln!(
                s,
                r#"<svg data-node="{}" data-icon="{}" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" viewBox="0 0 24 24" fill="none" stroke="{}" stroke-width="{:.2}" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
                esc(id),
                name.name(),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                color_svg(color),
                stroke,
                path
            );
        }
        IconSource::Custom(svg) => {
            // Re-serialize the parsed asset back to SVG path commands.
            // We don't keep the original source bytes, so this is a
            // best-effort visualisation matching what the GPU pipeline
            // sees (post-usvg normalisation). currentColor strokes
            // resolve to the runtime color/stroke_width.
            let asset = svg.vector_asset();
            let [vx, vy, vw, vh] = asset.view_box;
            let _ = writeln!(
                s,
                r#"<svg data-node="{}" data-icon="custom" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" viewBox="{} {} {} {}" stroke-linecap="round" stroke-linejoin="round">"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                vx,
                vy,
                vw,
                vh,
            );
            emit_custom_paths(s, asset, color, stroke_width);
            s.push_str("</svg>\n");
        }
    }
}

fn emit_custom_paths(s: &mut String, asset: &VectorAsset, current_color: Color, stroke_width: f32) {
    use crate::vector::VectorColor;
    emit_gradient_defs(s, asset);
    let hash = asset.content_hash();
    for path in &asset.paths {
        let d = serialize_segments(&path.segments);
        if d.is_empty() {
            continue;
        }
        let fill_attr = match path.fill {
            Some(f) => match f.color {
                VectorColor::Solid(c) => format!(r#"fill="{}""#, color_svg(c)),
                VectorColor::CurrentColor => format!(r#"fill="{}""#, color_svg(current_color)),
                VectorColor::Gradient(idx) => {
                    format!(r#"fill="{}""#, gradient_paint(asset, hash, idx, current_color))
                }
            },
            None => r#"fill="none""#.to_string(),
        };
        let stroke_attr = match path.stroke {
            Some(st) => {
                let color = match st.color {
                    VectorColor::Solid(c) => color_svg(c),
                    VectorColor::CurrentColor => color_svg(current_color),
                    VectorColor::Gradient(idx) => gradient_paint(asset, hash, idx, current_color),
                };
                let width = if matches!(st.color, VectorColor::CurrentColor) {
                    stroke_width
                } else {
                    st.width
                };
                format!(r#"stroke="{}" stroke-width="{:.2}""#, color, width)
            }
            None => String::new(),
        };
        let _ = writeln!(s, r#"<path d="{}" {} {}/>"#, d, fill_attr, stroke_attr);
    }
}

/// Emit `<defs>` with one `<linearGradient>` / `<radialGradient>` per
/// entry in the asset's gradient table, in the asset's viewBox space
/// (`userSpaceOnUse` + the inverse of the stored `absolute_to_local`
/// affine as `gradientTransform`). Ids derive from the asset's content
/// hash, so duplicate emits collide only with identical content.
fn emit_gradient_defs(s: &mut String, asset: &VectorAsset) {
    use crate::vector::VectorGradient;
    if asset.gradients.is_empty() {
        return;
    }
    let hash = asset.content_hash();
    s.push_str("<defs>");
    for (idx, gradient) in asset.gradients.iter().enumerate() {
        let id = gradient_def_id(hash, idx as u32);
        let transform = gradient_transform_attr(gradient_affine(gradient));
        match gradient {
            VectorGradient::Linear(g) => {
                let _ = write!(
                    s,
                    r#"<linearGradient id="{}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}"{}{}>"#,
                    id,
                    g.p1[0],
                    g.p1[1],
                    g.p2[0],
                    g.p2[1],
                    spread_attr(g.spread),
                    transform,
                );
                emit_gradient_stops(s, &g.stops);
                s.push_str("</linearGradient>");
            }
            VectorGradient::Radial(g) => {
                let _ = write!(
                    s,
                    r#"<radialGradient id="{}" gradientUnits="userSpaceOnUse" cx="{}" cy="{}" r="{}"{}{}>"#,
                    id,
                    g.center[0],
                    g.center[1],
                    g.radius,
                    spread_attr(g.spread),
                    transform,
                );
                emit_gradient_stops(s, &g.stops);
                s.push_str("</radialGradient>");
            }
        }
    }
    s.push_str("</defs>\n");
}

fn emit_gradient_stops(s: &mut String, stops: &[crate::vector::VectorGradientStop]) {
    for stop in stops {
        // Stop colours are canonical sRGB-encoded floats, straight alpha.
        let [r, g, b, _] = stop.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let _ = write!(
            s,
            r##"<stop offset="{}" stop-color="#{:02x}{:02x}{:02x}""##,
            stop.offset, r, g, b
        );
        if stop.color[3] < 1.0 {
            let _ = write!(s, r#" stop-opacity="{}""#, stop.color[3].clamp(0.0, 1.0));
        }
        s.push_str("/>");
    }
}

fn gradient_def_id(asset_hash: u64, idx: u32) -> String {
    format!("dg-{asset_hash:016x}-{idx}")
}

fn gradient_affine(gradient: &crate::vector::VectorGradient) -> &[f32; 6] {
    use crate::vector::VectorGradient;
    match gradient {
        VectorGradient::Linear(g) => &g.absolute_to_local,
        VectorGradient::Radial(g) => &g.absolute_to_local,
    }
}

fn spread_attr(spread: crate::vector::VectorSpreadMethod) -> &'static str {
    use crate::vector::VectorSpreadMethod;
    match spread {
        VectorSpreadMethod::Pad => "",
        VectorSpreadMethod::Reflect => r#" spreadMethod="reflect""#,
        VectorSpreadMethod::Repeat => r#" spreadMethod="repeat""#,
    }
}

/// `gradientTransform` for a stored absolute→local affine: SVG wants the
/// local→absolute direction, i.e. the inverse. Row-major
/// `[a b tx; c d ty]` maps to SVG `matrix(a c b d tx ty)` (column
/// order). Identity (the common no-transform case) emits nothing; a
/// degenerate affine emits nothing rather than an invalid matrix.
fn gradient_transform_attr(m: &[f32; 6]) -> String {
    let det = m[0] * m[4] - m[1] * m[3];
    if det.abs() < 1e-12 {
        return String::new();
    }
    let inv_det = 1.0 / det;
    let a = m[4] * inv_det;
    let b = -m[1] * inv_det;
    let c = -m[3] * inv_det;
    let d = m[0] * inv_det;
    let tx = -(a * m[2] + b * m[5]);
    let ty = -(c * m[2] + d * m[5]);
    let identity = (a - 1.0).abs() < 1e-6
        && b.abs() < 1e-6
        && c.abs() < 1e-6
        && (d - 1.0).abs() < 1e-6
        && tx.abs() < 1e-6
        && ty.abs() < 1e-6;
    if identity {
        return String::new();
    }
    format!(r#" gradientTransform="matrix({a} {c} {b} {d} {tx} {ty})""#)
}

/// Paint value for a gradient fill/stroke: a `url(#...)` reference to
/// the def emitted by [`emit_gradient_defs`], or the flat fallback
/// colour if the index is out of range.
fn gradient_paint(asset: &VectorAsset, asset_hash: u64, idx: u32, current_color: Color) -> String {
    if (idx as usize) < asset.gradients.len() {
        format!("url(#{})", gradient_def_id(asset_hash, idx))
    } else {
        color_svg(gradient_fallback_color(asset, idx, current_color))
    }
}

fn emit_mask_paths(s: &mut String, asset: &VectorAsset, color: Color) {
    for path in &asset.paths {
        let d = serialize_segments(&path.segments);
        if d.is_empty() {
            continue;
        }
        let fill_attr = if path.fill.is_some() {
            format!(r#"fill="{}""#, color_svg(color))
        } else {
            r#"fill="none""#.to_string()
        };
        let stroke_attr = path
            .stroke
            .map(|st| {
                format!(
                    r#"stroke="{}" stroke-width="{:.2}""#,
                    color_svg(color),
                    st.width
                )
            })
            .unwrap_or_default();
        let _ = writeln!(s, r#"<path d="{}" {} {}/>"#, d, fill_attr, stroke_attr);
    }
}

/// Flat fallback for a gradient paint whose index is out of range (a
/// malformed hand-built asset): the first stop's colour, or
/// `current_color` when there are no stops. In-range gradients emit
/// real defs via [`emit_gradient_defs`].
fn gradient_fallback_color(asset: &VectorAsset, idx: u32, current_color: Color) -> Color {
    use crate::vector::VectorGradient;
    let stops = asset.gradients.get(idx as usize).map(|g| match g {
        VectorGradient::Linear(g) => g.stops.as_slice(),
        VectorGradient::Radial(g) => g.stops.as_slice(),
    });
    let Some(stops) = stops else {
        return current_color;
    };
    let Some(stop) = stops.first() else {
        return current_color;
    };
    // Stop colours are canonical sRGB-encoded floats (straight alpha).
    let [r, g, b, a] = stop.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color::srgb_u8a(r, g, b, a)
}

fn serialize_segments(segments: &[VectorSegment]) -> String {
    let mut out = String::new();
    for seg in segments {
        if !out.is_empty() {
            out.push(' ');
        }
        match *seg {
            VectorSegment::MoveTo([x, y]) => {
                let _ = write!(out, "M{:.3} {:.3}", x, y);
            }
            VectorSegment::LineTo([x, y]) => {
                let _ = write!(out, "L{:.3} {:.3}", x, y);
            }
            VectorSegment::QuadTo([cx, cy], [x, y]) => {
                let _ = write!(out, "Q{:.3} {:.3} {:.3} {:.3}", cx, cy, x, y);
            }
            VectorSegment::CubicTo([c1x, c1y], [c2x, c2y], [x, y]) => {
                let _ = write!(
                    out,
                    "C{:.3} {:.3} {:.3} {:.3} {:.3} {:.3}",
                    c1x, c1y, c2x, c2y, x, y
                );
            }
            VectorSegment::Close => out.push('Z'),
        }
    }
    out
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
fn as_vec4(v: &UniformValue) -> Option<[f32; 4]> {
    match v {
        UniformValue::Vec4(a) => Some(*a),
        _ => None,
    }
}

// The Tailwind v4 elevation recipes from `paint::shadow::SHADOW_ANCHORS`,
// bucketed to the nearest anchor: two blur/offset chains per filter
// (one per recipe layer), σ = blur/2 per the CSS blur-radius contract,
// raw CSS alphas (SVG rasterizers composite in sRGB like browsers, so
// no gamma compensation — that lives in rounded_rect.wgsl for the
// linear-light GPU path). Negative spread has no feGaussianBlur
// equivalent and is dropped; at these sizes the visible difference is
// a hair of extra penumbra width in dev-facing artifacts.
//
// Explicit feMerge form instead of the convenient `feDropShadow`. Both
// render the same shadow visually, but rsvg-convert's feDropShadow
// silently round-trips SourceGraphic through linearRGB even with
// color-interpolation-filters="sRGB" on the filter, drifting the
// shape's own interior by 1-2 channel codes (CARD #171a21 → #161c22).
// Routing SourceGraphic through feMergeNode untouched keeps the card
// color exact while the shadow path (SourceAlpha → blur → offset →
// alpha-scale) produces the same dark falloff.
const SHADOW_DEFS: &str = r##"<defs>
  <filter id="shadow-xs" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="1"/>
    <feOffset dx="0" dy="1"/>
    <feComponentTransfer result="l1"><feFuncA type="linear" slope="0.05"/></feComponentTransfer>
    <feMerge><feMergeNode in="l1"/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="shadow-sm" x="-25%" y="-25%" width="150%" height="150%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="1.5"/>
    <feOffset dx="0" dy="1"/>
    <feComponentTransfer result="l1"><feFuncA type="linear" slope="0.10"/></feComponentTransfer>
    <feGaussianBlur in="SourceAlpha" stdDeviation="1"/>
    <feOffset dx="0" dy="1"/>
    <feComponentTransfer result="l2"><feFuncA type="linear" slope="0.10"/></feComponentTransfer>
    <feMerge><feMergeNode in="l1"/><feMergeNode in="l2"/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="shadow-md" x="-40%" y="-40%" width="180%" height="200%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="3"/>
    <feOffset dx="0" dy="4"/>
    <feComponentTransfer result="l1"><feFuncA type="linear" slope="0.10"/></feComponentTransfer>
    <feGaussianBlur in="SourceAlpha" stdDeviation="2"/>
    <feOffset dx="0" dy="2"/>
    <feComponentTransfer result="l2"><feFuncA type="linear" slope="0.10"/></feComponentTransfer>
    <feMerge><feMergeNode in="l1"/><feMergeNode in="l2"/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="shadow-lg" x="-60%" y="-60%" width="220%" height="260%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="7.5"/>
    <feOffset dx="0" dy="10"/>
    <feComponentTransfer result="l1"><feFuncA type="linear" slope="0.10"/></feComponentTransfer>
    <feGaussianBlur in="SourceAlpha" stdDeviation="3"/>
    <feOffset dx="0" dy="4"/>
    <feComponentTransfer result="l2"><feFuncA type="linear" slope="0.10"/></feComponentTransfer>
    <feMerge><feMergeNode in="l1"/><feMergeNode in="l2"/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
</defs>
"##;

/// Build an SVG `path` `d` attribute for a rectangle with per-corner
/// radii in `(tl, tr, br, bl)` order. SVG's `<rect rx>` only models a
/// uniform corner radius; per-corner shapes go through this path.
/// Each corner radius is assumed to already be clamped to half the
/// shorter side.
fn rounded_rect_path(rect: Rect, radii: [f32; 4]) -> String {
    let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
    let (tl, tr, br, bl) = (radii[0], radii[1], radii[2], radii[3]);
    let mut d = String::new();
    use std::fmt::Write;
    let _ = write!(&mut d, "M {:.2} {:.2}", x + tl, y);
    let _ = write!(&mut d, " H {:.2}", x + w - tr);
    if tr > 0.0 {
        let _ = write!(
            &mut d,
            " A {:.2} {:.2} 0 0 1 {:.2} {:.2}",
            tr,
            tr,
            x + w,
            y + tr
        );
    }
    let _ = write!(&mut d, " V {:.2}", y + h - br);
    if br > 0.0 {
        let _ = write!(
            &mut d,
            " A {:.2} {:.2} 0 0 1 {:.2} {:.2}",
            br,
            br,
            x + w - br,
            y + h
        );
    }
    let _ = write!(&mut d, " H {:.2}", x + bl);
    if bl > 0.0 {
        let _ = write!(
            &mut d,
            " A {:.2} {:.2} 0 0 1 {:.2} {:.2}",
            bl,
            bl,
            x,
            y + h - bl
        );
    }
    let _ = write!(&mut d, " V {:.2}", y + tl);
    if tl > 0.0 {
        let _ = write!(
            &mut d,
            " A {:.2} {:.2} 0 0 1 {:.2} {:.2}",
            tl,
            tl,
            x + tl,
            y
        );
    }
    d.push_str(" Z");
    d
}

fn shadow_filter(shadow: f32) -> &'static str {
    if shadow >= 18.0 {
        r#" filter="url(#shadow-lg)""#
    } else if shadow >= 8.0 {
        r#" filter="url(#shadow-md)""#
    } else if shadow >= 3.0 {
        r#" filter="url(#shadow-sm)""#
    } else if shadow > 0.0 {
        r#" filter="url(#shadow-xs)""#
    } else {
        ""
    }
}

pub(crate) fn color_svg(c: Color) -> String {
    // Wide-gamut sources emit CSS Color 4 `color(display-p3 r g b)` /
    // `color(rec2020 r g b)` so modern browsers / viewers paint the SVG
    // in the authored space. sRGB sources keep the compact hex / rgba()
    // form for diff-friendliness.
    use crate::color::TransferFunction;
    if let Some(name) = c.space.primaries.css_color_space() {
        // CSS Color 4 takes channel values in the encoded form of the
        // named space (the spec inverts the OETF to linearize before
        // compositing). Convert into the encoded form of the same
        // primaries.
        let encoded = c.convert_to(crate::color::ColorSpace {
            primaries: c.space.primaries,
            transfer: match c.space.primaries {
                crate::color::Primaries::DisplayP3 => TransferFunction::Srgb,
                _ => TransferFunction::Srgb,
            },
            reference_luminance_nits: 100.0,
        });
        if (c.a - 1.0).abs() <= f32::EPSILON {
            format!(
                "color({name} {:.4} {:.4} {:.4})",
                encoded.r, encoded.g, encoded.b
            )
        } else {
            format!(
                "color({name} {:.4} {:.4} {:.4} / {:.3})",
                encoded.r, encoded.g, encoded.b, c.a
            )
        }
    } else {
        let [r, g, b, a] = c.to_srgb_u8a();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("rgba({r},{g},{b},{:.3})", a as f32 / 255.0)
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SvgIcon;
    use crate::draw_ops::draw_ops;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::tree::IconName;

    const RED_CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="#ff0000"/></svg>"##;

    fn render(el: crate::tree::El) -> String {
        let mut tree = el;
        let viewport = Rect::new(0.0, 0.0, 64.0, 64.0);
        let mut state = UiState::default();
        layout(&mut tree, &mut state, viewport);
        let ops = draw_ops(&tree, &state);
        svg_from_ops(viewport.w, viewport.h, &ops, Color::srgb_u8(255, 255, 255))
    }

    /// Issue #140's related finding: painted gradients must serialize
    /// as real `<linearGradient>` defs with every authored stop, not a
    /// flat fallback colour that masks gradient bugs in artifact review.
    #[test]
    fn painted_vector_gradient_emits_defs_with_all_stops() {
        let asset = crate::vector::parse_svg_asset(
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
        .unwrap();
        let svg = render(crate::tree::vector(asset));
        assert!(
            svg.contains("<linearGradient id=\"dg-"),
            "expected a linearGradient def, got:\n{svg}"
        );
        for stop in ["#754a75", "#372960", "#a33861", "#d1956c", "#f7a983"] {
            assert!(
                svg.contains(&format!("stop-color=\"{stop}\"")),
                "missing stop {stop}:\n{svg}"
            );
        }
        assert!(
            svg.contains("fill=\"url(#dg-"),
            "gradient fill should reference the def, got:\n{svg}"
        );
    }

    #[test]
    fn builtin_icon_renders_via_named_path() {
        let svg = render(crate::icons::icon(IconName::Check));
        assert!(
            svg.contains(r#"data-icon="check""#),
            "expected built-in `data-icon=\"check\"`, got:\n{svg}"
        );
    }

    #[test]
    fn text_emits_resolved_font_family_for_fixture_renderers() {
        let svg = render(crate::text("Hello"));
        assert!(
            svg.contains(r#"font-family="Inter Variable""#),
            "SVG fixture text should emit the resolved Damascene family for fontconfig renderers, got:\n{svg}"
        );
        assert!(
            !svg.contains("ui-sans-serif"),
            "browser-style font stacks make rsvg/resvg fall back to different metrics:\n{svg}"
        );
    }

    #[test]
    fn spinner_renders_static_arc_only_by_default() {
        // Bundle output must be deterministic; the live shader is
        // time-driven, so the SVG fallback emits a frozen 240° arc
        // matching the t=0 frame of the cosine envelope. The default
        // spinner has no track, so only the arc <path> appears.
        let svg = render(crate::widgets::spinner::spinner());
        assert!(
            svg.contains(r#"data-shader="stock::spinner""#),
            "spinner must mark its draws with the stock shader name, got:\n{svg}"
        );
        assert!(
            svg.contains(r#"<path"#),
            "spinner SVG fallback should emit the arc as a <path>, got:\n{svg}"
        );
        assert!(
            !svg.contains(r#"<circle"#),
            "default spinner has no track, so no <circle> should appear:\n{svg}"
        );
    }

    #[test]
    fn spinner_with_track_emits_both_circle_and_arc() {
        // Opt-in track via spinner_with_track adds a full-ring
        // <circle> behind the arc <path>. Pin both markers.
        let svg = render(crate::widgets::spinner::spinner_with_track(
            crate::tokens::PRIMARY,
            crate::tokens::MUTED,
        ));
        assert!(
            svg.contains(r#"<circle"#),
            "spinner_with_track should emit a track <circle>, got:\n{svg}"
        );
        assert!(
            svg.contains(r#"<path"#),
            "spinner_with_track should emit an arc <path>, got:\n{svg}"
        );
    }

    #[test]
    fn rounded_rect_with_uniform_corners_emits_rect_with_rx() {
        use crate::tree::Corners;
        let el = crate::tree::column::<_, crate::tree::El>([])
            .width(crate::tree::Size::Fixed(40.0))
            .height(crate::tree::Size::Fixed(40.0))
            .fill(Color::srgb_u8(255, 0, 0))
            .radius(Corners::all(8.0));
        let svg = render(el);
        assert!(
            svg.contains(r#"<rect"#) && svg.contains(r#"rx="8.00""#),
            "uniform corners should emit `<rect rx>`, got:\n{svg}"
        );
    }

    #[test]
    fn rounded_rect_with_non_uniform_corners_emits_path_with_per_corner_arcs() {
        use crate::tree::Corners;
        // Top-rounded card-header strip: the artifact-fixing shape.
        let el = crate::tree::column::<_, crate::tree::El>([])
            .width(crate::tree::Size::Fixed(40.0))
            .height(crate::tree::Size::Fixed(40.0))
            .fill(Color::srgb_u8(255, 0, 0))
            .radius(Corners::top(8.0));
        let svg = render(el);
        assert!(
            svg.contains(r#"<path"#) && svg.contains(r#"data-shader="stock::rounded_rect""#),
            "non-uniform corners should emit `<path>` for the rounded-rect node, got:\n{svg}"
        );
        // Two arcs (the two non-zero top corners), no arc on bottom corners.
        let arc_count = svg.matches(" A 8.00 8.00 ").count();
        assert_eq!(
            arc_count, 2,
            "top-rounded shape should emit exactly two 8px arcs, got:\n{svg}"
        );
    }

    #[test]
    fn custom_svg_icon_renders_via_re_serialised_paths() {
        let custom = SvgIcon::parse(RED_CIRCLE).unwrap();
        let svg = render(crate::icons::icon(custom));
        assert!(
            svg.contains(r#"data-icon="custom""#),
            "expected `data-icon=\"custom\"` for app-supplied SVG, got:\n{svg}"
        );
        // The fixture uses `fill="#ff0000"` which usvg normalises to
        // a `Solid` color in the IR; the re-serialiser must emit that
        // colour through (not the runtime `current_color`).
        assert!(
            svg.contains("fill=\"rgb(255,0,0)\"") || svg.contains("fill=\"#ff0000\""),
            "expected the SVG fill to round-trip in the bundle, got:\n{svg}"
        );
    }

    /// #118: a plot's data layer renders as real geometry in the headless
    /// SVG — series polylines and scatter circles, not a placeholder box.
    #[test]
    fn plot_data_layer_renders_polylines_and_circles() {
        use crate::plot::{PlotSpec, Sample, SeriesHandle, line, scatter};

        let lh = SeriesHandle::new(vec![
            Sample::new(0.0, 0.0),
            Sample::new(1.0, 2.0),
            Sample::new(2.0, 1.0),
        ]);
        let sh = SeriesHandle::new(vec![Sample::new(0.5, 0.5), Sample::new(1.5, 1.5)]);
        let spec = PlotSpec::new().add_mark(line(&lh)).add_mark(scatter(&sh));
        let svg = render(
            crate::tree::plot(spec)
                .width(Size::Fixed(64.0))
                .height(Size::Fixed(64.0)),
        );

        assert!(svg.contains("<polyline"), "line mark lowers to a polyline");
        assert!(svg.contains("<circle"), "scatter mark lowers to circles");
        assert!(
            !svg.contains(">3D scene<"),
            "no placeholder label for a mesh-free plot scene"
        );
        // Reviewer-facing evidence on the group wrapper.
        assert!(svg.contains(r#"data-kind="scene3d""#));
        assert!(svg.contains(r#"data-lines="2""#), "two segments: {svg}");
    }

    /// A geometry-free scene keeps the labelled placeholder footprint.
    #[test]
    fn empty_scene_keeps_placeholder() {
        let svg = render(
            crate::tree::chart3d(crate::scene::SceneSpec::new())
                .width(Size::Fixed(64.0))
                .height(Size::Fixed(64.0)),
        );
        assert!(svg.contains(">3D scene<"), "placeholder label: {svg}");
        assert!(svg.contains("stroke-dasharray"), "dashed footprint");
    }

    /// Non-finite samples must not leak literal `NaN` coordinates into the
    /// SVG (invalid numbers; strict consumers reject the document). The
    /// camera projection culls them like behind-camera points.
    #[test]
    fn nan_samples_do_not_leak_into_svg() {
        use crate::plot::{PlotSpec, Sample, SeriesHandle, line};

        let h = SeriesHandle::new(vec![
            Sample::new(0.0, 0.0),
            Sample::new(1.0, 2.0),
            Sample::new(2.0, f64::NAN),
            Sample::new(3.0, 1.0),
        ]);
        let svg = render(
            crate::tree::plot(PlotSpec::new().add_mark(line(&h)))
                .width(Size::Fixed(64.0))
                .height(Size::Fixed(64.0)),
        );
        assert!(!svg.contains("NaN"), "no NaN coordinates: {svg}");
        assert!(svg.contains("<polyline"), "finite spans still draw");
    }

    /// Line join discs are redundant under SVG round joins and are skipped
    /// — a pure line mark emits polylines but no circles.
    #[test]
    fn line_join_discs_are_skipped() {
        use crate::plot::{PlotSpec, Sample, SeriesHandle, line};

        let h = SeriesHandle::new(vec![
            Sample::new(0.0, 0.0),
            Sample::new(1.0, 2.0),
            Sample::new(2.0, 1.0),
        ]);
        let svg = render(
            crate::tree::plot(PlotSpec::new().add_mark(line(&h)))
                .width(Size::Fixed(64.0))
                .height(Size::Fixed(64.0)),
        );
        assert!(svg.contains("<polyline"));
        assert!(
            !svg.contains("<circle"),
            "join discs skipped for a line-only plot: {svg}"
        );
        assert!(svg.contains(r#"stroke-linejoin="round""#));
    }
}
