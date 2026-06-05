//! Shader manifest — a textual artifact listing every shader used by a
//! frame's [`DrawOp`] list, with usage counts and resolved uniform values.
//!
//! Part of the agent loop's feedback bundle. The LLM uses this to:
//!
//! - See which stock shader is painting a node it cares about.
//! - See the resolved uniform values per draw (e.g. `fill=primary`,
//!   `radius=12.00`).
//! - Notice when a custom shader is requested but not registered.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::ir::*;

/// Render the shader manifest for an ops list to a string.
pub fn shader_manifest(ops: &[DrawOp]) -> String {
    // group ops by shader handle for the summary; preserve original
    // order under each shader.
    let mut by_shader: BTreeMap<String, Vec<&DrawOp>> = BTreeMap::new();
    for op in ops {
        if let Some(handle) = op.shader() {
            by_shader.entry(handle.name()).or_default().push(op);
        }
    }

    let mut s = String::new();
    if by_shader.is_empty() {
        s.push_str("no shaders\n");
        return s;
    }
    for (name, group) in &by_shader {
        let _ = writeln!(s, "{name}  used {} times", group.len());
        for op in group {
            match op {
                DrawOp::Quad { id, uniforms, .. } => {
                    let _ = write!(s, "  {id}");
                    for (k, v) in uniforms {
                        let _ = write!(s, " {k}={}", v.debug_short());
                    }
                    s.push('\n');
                }
                DrawOp::GlyphRun {
                    id,
                    color,
                    text,
                    size,
                    family,
                    weight,
                    mono,
                    wrap,
                    anchor,
                    ..
                } => {
                    let preview: String = text.chars().take(28).collect();
                    let suffix = if text.chars().count() > 28 { "…" } else { "" };
                    let _ = write!(
                        s,
                        "  {id} text=\"{preview}{suffix}\" color={} size={size:.1} family={family:?} weight={weight:?} mono={mono} wrap={wrap:?} anchor={anchor:?}",
                        color_label(*color),
                    );
                    s.push('\n');
                }
                DrawOp::AttributedText {
                    id,
                    runs,
                    size,
                    wrap,
                    anchor,
                    ..
                } => {
                    let concat: String = runs.iter().map(|(t, _)| t.as_str()).collect();
                    let preview: String = concat.chars().take(28).collect();
                    let suffix = if concat.chars().count() > 28 {
                        "…"
                    } else {
                        ""
                    };
                    let _ = write!(
                        s,
                        "  {id} attr=\"{preview}{suffix}\" runs={} size={size:.1} wrap={wrap:?} anchor={anchor:?}",
                        runs.len(),
                    );
                    s.push('\n');
                }
                DrawOp::Icon { .. } => {}
                DrawOp::Image { .. } => {} // bound to a per-image texture, not a stock shader
                DrawOp::AppTexture { .. } => {} // bound to an app-owned texture, not a stock shader
                DrawOp::Vector { .. } => {} // backend vector path, not a stock shader
                DrawOp::Scene3D { .. } => {} // backend scene pipelines, not a stock shader
                DrawOp::BackdropSnapshot => {}
            }
        }
        s.push('\n');
    }
    s
}

/// Render the raw draw-op list for inspection. The wgpu backend consumes
/// the same `Vec<DrawOp>`; this is the textual form for the agent loop.
pub fn draw_ops_text(ops: &[DrawOp]) -> String {
    let mut s = String::new();
    for op in ops {
        match op {
            DrawOp::Quad {
                id,
                rect,
                scissor,
                shader,
                uniforms,
            } => {
                let _ = write!(
                    s,
                    "Quad   shader={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id}",
                    shader.name(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                for (k, v) in uniforms {
                    let _ = write!(s, " {k}={}", v.debug_short());
                }
                s.push('\n');
            }
            DrawOp::GlyphRun {
                id,
                rect,
                scissor,
                shader,
                color,
                text,
                size,
                line_height,
                family,
                mono_family,
                weight,
                mono,
                wrap,
                anchor,
                layout: _,
                underline: _,
                strikethrough: _,
                link: _,
            } => {
                let preview: String = text.chars().take(40).collect();
                let suffix = if text.chars().count() > 40 { "…" } else { "" };
                let face = if *mono { *mono_family } else { *family };
                let _ = write!(
                    s,
                    "Glyph  shader={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id} text=\"{preview}{suffix}\" color={} size={size:.1} line_height={line_height:.1} family={face:?} weight={weight:?} mono={mono} wrap={wrap:?} anchor={anchor:?}",
                    shader.name(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    color_label(*color),
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::AttributedText {
                id,
                rect,
                scissor,
                shader,
                runs,
                size,
                line_height,
                wrap,
                anchor,
                layout: _,
            } => {
                let concat: String = runs.iter().map(|(t, _)| t.as_str()).collect();
                let preview: String = concat.chars().take(40).collect();
                let suffix = if concat.chars().count() > 40 {
                    "…"
                } else {
                    ""
                };
                let bg_runs = runs.iter().filter(|(_, st)| st.bg.is_some()).count();
                let _ = write!(
                    s,
                    "Attr   shader={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id} attr=\"{preview}{suffix}\" runs={} bg_runs={bg_runs} size={size:.1} line_height={line_height:.1} wrap={wrap:?} anchor={anchor:?}",
                    shader.name(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    runs.len(),
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::Icon {
                id,
                rect,
                scissor,
                source,
                color,
                size,
                stroke_width,
            } => {
                let _ = write!(
                    s,
                    "Icon   name={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id} color={} size={size:.1} stroke_width={stroke_width:.1}",
                    source.label(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    color_label(*color),
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::Image {
                id,
                rect,
                scissor,
                image,
                tint,
                radius,
                fit,
                range_limit,
            } => {
                let tint_str = match tint {
                    Some(c) => color_label(*c),
                    None => "none".to_string(),
                };
                let _ = write!(
                    s,
                    "Image  src={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id} natural=({}x{}) fit={fit:?} tint={tint_str} radius=({:.1},{:.1},{:.1},{:.1})",
                    image.label(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    image.width(),
                    image.height(),
                    radius.tl,
                    radius.tr,
                    radius.br,
                    radius.bl,
                );
                // Only worth a column when it deviates from the default
                // (keeps existing manifest output stable).
                if *range_limit != crate::image::DynamicRangeLimit::NoLimit {
                    let _ = write!(s, " range={range_limit:?}");
                }
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::AppTexture {
                id,
                rect,
                scissor,
                texture,
                alpha,
                fit,
                transform,
            } => {
                let (tw, th) = texture.size_px();
                let format = texture.format();
                let _ = write!(
                    s,
                    "Surface tex_id={:<10} rect=({:.0},{:.0},{:.0},{:.0}) id={id} natural=({tw}x{th}) format={format:?} fit={fit:?} alpha={alpha:?}",
                    texture.id().0,
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                );
                if !transform.is_identity() {
                    let _ = write!(
                        s,
                        " transform=({:.3},{:.3},{:.3},{:.3},{:.3},{:.3})",
                        transform.a,
                        transform.b,
                        transform.c,
                        transform.d,
                        transform.tx,
                        transform.ty,
                    );
                }
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::Vector {
                id,
                rect,
                scissor,
                asset,
                render_mode,
            } => {
                let [_, _, vw, vh] = asset.view_box;
                let _ = write!(
                    s,
                    "Vector hash={:016x} mode={render_mode:?} rect=({:.0},{:.0},{:.0},{:.0}) id={id} view_box=({vw}x{vh}) paths={}",
                    asset.content_hash(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    asset.paths.len(),
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::Scene3D {
                id,
                rect,
                scissor,
                scene,
            } => {
                let _ = write!(
                    s,
                    "Scene3D rect=({:.0},{:.0},{:.0},{:.0}) id={id} meshes={} points={} lines={}",
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    scene.meshes.len(),
                    scene.points.len(),
                    scene.lines.len(),
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::BackdropSnapshot => {
                let _ = writeln!(s, "BackdropSnapshot");
            }
        }
    }
    s
}

fn color_label(c: crate::tree::Color) -> String {
    match c.token {
        Some(name) => name.to_string(),
        None => {
            let [r, g, b, a] = c.to_srgb_u8a();
            match c.space.primaries.css_color_space() {
                None => format!("rgba({r},{g},{b},{a})"),
                Some(space) => format!("rgba({r},{g},{b},{a}) in {space}"),
            }
        }
    }
}

fn write_scissor(s: &mut String, scissor: crate::tree::Rect) {
    let _ = write!(
        s,
        " scissor=({:.0},{:.0},{:.0},{:.0})",
        scissor.x, scissor.y, scissor.w, scissor.h,
    );
}
