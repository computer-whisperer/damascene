//! Tree → [`DrawOp`] resolution.
//!
//! Walks the laid-out [`El`] tree and emits a flat [`Vec<DrawOp>`] in
//! paint order. Each visual fact resolves to a `Quad` (bound to a stock
//! or custom shader, with uniforms packed) or a `GlyphRun`.
//!
//! State styling lands here on the CPU side. Hover lightens / press
//! darkens / ring fade come from the eased envelopes in
//! `UiState`'s eased envelope side map, written by
//! [`UiState::tick_visual_animations`] in the prior pass. What this
//! module computes are the deltas: lerp the build-time colours toward
//! the state-modulated ones by the envelope amount, plus the non-eased
//! `Disabled` (alpha multiply) and `Loading` (text suffix) deltas.

use crate::ir::*;
use crate::palette::Palette;
use crate::shader::*;
use crate::state::{EnvelopeKind, UiState};
use crate::text::atlas::RunStyle;
use crate::text::metrics as text_metrics;
use crate::theme::Theme;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text_area::{TEXT_AREA_CARET_LAYER, TEXT_AREA_SELECTION_LAYER};

/// Walk the laid-out tree and emit draw ops in paint order.
pub fn draw_ops(root: &El, ui_state: &UiState) -> Vec<DrawOp> {
    draw_ops_with_theme(root, ui_state, &Theme::default())
}

/// Walk the laid-out tree and emit draw ops using a caller-supplied theme.
pub fn draw_ops_with_theme(root: &El, ui_state: &UiState, theme: &Theme) -> Vec<DrawOp> {
    let mut stats = DrawOpsStats::default();
    draw_ops_with_theme_and_stats(root, ui_state, theme, &mut stats)
}

#[derive(Clone, Debug, Default)]
pub struct DrawOpsStats {
    pub culled_text_ops: u64,
    /// Nearest scatter point under the cursor this pass, surfaced to the app
    /// next build via [`BuildCx::hovered_scene_point`]. `None` when no hovered
    /// scene point was picked.
    ///
    /// [`BuildCx::hovered_scene_point`]: crate::event::BuildCx::hovered_scene_point
    pub hovered_scene_point: Option<crate::scene::ScenePointPick>,
    /// Cursor distance² of `hovered_scene_point`, used to keep the global
    /// nearest across marks/scenes during the walk. Meaningless when
    /// `hovered_scene_point` is `None`.
    hovered_dist2: f32,
}

impl DrawOpsStats {
    /// Record a hovered-point candidate at cursor distance² `d2`, keeping the
    /// nearest seen this pass.
    fn consider_pick(&mut self, d2: f32, pick: crate::scene::ScenePointPick) {
        if self.hovered_scene_point.is_none() || d2 < self.hovered_dist2 {
            self.hovered_dist2 = d2;
            self.hovered_scene_point = Some(pick);
        }
    }
}

/// Walk the laid-out tree and emit draw ops, reporting cheap culling
/// decisions made before expensive text measurement.
pub fn draw_ops_with_theme_and_stats(
    root: &El,
    ui_state: &UiState,
    theme: &Theme,
    stats: &mut DrawOpsStats,
) -> Vec<DrawOp> {
    let mut out = Vec::new();
    push_node(
        root,
        ui_state,
        theme,
        &mut out,
        None,
        (0.0, 0.0),
        1.0,
        1.0,
        0.0,
        0.0,
        0.0,
        stats,
    );
    resolve_palette(&mut out, theme.palette());
    out
}

/// Replace every `Color` in `ops` with its palette-resolved version.
///
/// This is the single chokepoint where token names become rgba: the
/// per-node passes write `Color` values straight from `tokens::*`
/// (preserving the `token: Some(name)` metadata), then this pass walks
/// every emitted [`DrawOp`] and rewrites each color through
/// [`Palette::resolve`]. Token names survive resolution, so shader
/// manifest / tree-dump / lint output still see `fill=card` rather
/// than rgba bytes.
pub fn resolve_palette(ops: &mut [DrawOp], palette: &Palette) {
    for op in ops {
        match op {
            DrawOp::Quad { uniforms, .. } => {
                resolve_uniform_block(uniforms, palette);
            }
            DrawOp::GlyphRun { color, .. } => {
                *color = palette.resolve(*color);
            }
            DrawOp::AttributedText { runs, .. } => {
                for (_, style) in runs {
                    style.color = palette.resolve(style.color);
                    if let Some(bg) = &mut style.bg {
                        *bg = palette.resolve(*bg);
                    }
                }
            }
            DrawOp::Icon { color, .. } => {
                *color = palette.resolve(*color);
            }
            DrawOp::Image { tint, .. } => {
                if let Some(t) = tint {
                    *t = palette.resolve(*t);
                }
            }
            DrawOp::AppTexture { .. } => {}
            // Scene colours (materials, grid, lights) live behind the
            // scene's `Arc` and are converted to the working space by the
            // backend at render time. Palette-token resolution for themed
            // scene colours is a later refinement (plan); no-op here.
            DrawOp::Scene3D { .. } => {}
            DrawOp::Vector {
                asset, render_mode, ..
            } => {
                *render_mode = render_mode.resolved_palette(palette);
                if matches!(render_mode, crate::vector::VectorRenderMode::Painted) {
                    *asset = std::sync::Arc::new(asset.resolved_palette(palette));
                }
            }
            DrawOp::BackdropSnapshot => {}
        }
    }
}

fn resolve_uniform_block(uniforms: &mut UniformBlock, palette: &Palette) {
    let keys: Vec<&'static str> = uniforms
        .iter()
        .filter_map(|(k, v)| matches!(v, UniformValue::Color(_)).then_some(*k))
        .collect();
    for k in keys {
        if let Some(UniformValue::Color(c)) = uniforms.get(k).copied() {
            uniforms.insert(k, UniformValue::Color(palette.resolve(c)));
        }
    }
}

// Recursion threads seven "inherited from parent" paint values
// (scissor, translate, opacity, focus / hover / press envelopes from
// the nearest focusable ancestor, plus the *strict* nearest-focusable-
// ancestor's combined subtree-interaction envelope used by
// `hover_alpha`) and the four shared references (node, ui_state,
// theme, out accumulator). The explicit signature documents the
// dataflow more clearly than a bundling struct would.
#[allow(clippy::too_many_arguments)]
fn push_node(
    n: &El,
    ui_state: &UiState,
    theme: &Theme,
    out: &mut Vec<DrawOp>,
    inherited_scissor: Option<Rect>,
    inherited_translate: (f32, f32),
    inherited_opacity: f32,
    inherited_focus_envelope: f32,
    inherited_hover_envelope: f32,
    inherited_press_envelope: f32,
    inherited_interaction_envelope: f32,
    stats: &mut DrawOpsStats,
) {
    let computed = ui_state.rect(&n.computed_id);
    let state = ui_state.node_state(&n.computed_id);
    let hover_amount = ui_state.envelope(&n.computed_id, EnvelopeKind::Hover);
    let press_amount = ui_state.envelope(&n.computed_id, EnvelopeKind::Press);
    let focus_ring_alpha = ui_state.envelope(&n.computed_id, EnvelopeKind::FocusRing);

    // `state_follows_interactive_ancestor` borrows the nearest
    // focusable ancestor's hover / press envelopes for paint. The
    // hit-test only ever lands on the focusable container above, so
    // child elements (slider thumb, etc.) never receive their own
    // envelope — without this, hover / press are dead on those
    // children.
    let (effective_hover, effective_press) = if n.state_follows_interactive_ancestor {
        (inherited_hover_envelope, inherited_press_envelope)
    } else {
        (hover_amount, press_amount)
    };

    let (fill, stroke, text_color, weight, suffix) =
        apply_state(n, state, effective_hover, effective_press, theme.palette());

    // `translate` is subtree-inheriting: descendants paint at their
    // computed rect plus all ancestor `translate` accumulated through
    // the recursion. `scale` and `opacity` apply to this node only —
    // a parent fading to 0.5 multiplies through to descendants via
    // `inherited_opacity`, but `scale` doesn't propagate (descendants
    // keep their own paint metrics).
    let total_translate = (
        inherited_translate.0 + n.translate.0,
        inherited_translate.1 + n.translate.1,
    );
    // Nodes flagged with `alpha_follows_focused_ancestor` fade with
    // their nearest focusable ancestor's focus envelope. The flag is
    // layout-neutral; we just multiply the ancestor's envelope into
    // this node's paint opacity, and the existing alpha modulation in
    // `opaque(...)` propagates that to fill / stroke / text colors.
    let focus_alpha_mul = if n.alpha_follows_focused_ancestor {
        inherited_focus_envelope
    } else {
        1.0
    };
    // Caret blink: nodes flagged `blink_when_focused` are additionally
    // multiplied by the runtime's caret-blink alpha. Composes with the
    // focus envelope above so the caret bar fades in on focus, then
    // settles into the on/off cycle while focus stays.
    let blink_alpha_mul = if n.blink_when_focused {
        // No activity recorded yet → caret stays solid. This keeps
        // headless / pre-event tests deterministic without forcing
        // them to drive the animation tick.
        if ui_state.caret.activity_at.is_some() {
            ui_state.caret.blink_alpha
        } else {
            1.0
        }
    } else {
        1.0
    };
    // Subtree interaction envelope for this node: max of the hover,
    // focus, and press envelopes covering "is the active target this
    // node or any descendant?". Tracked only on nodes that consume it
    // (focusable nodes plus `hover_alpha` consumers); other nodes read
    // back as `0.0` and don't contribute. Used immediately for
    // `hover_alpha` and below to update the cascade for descendants.
    let self_interaction_envelope = ui_state
        .envelope(&n.computed_id, EnvelopeKind::SubtreeHover)
        .max(ui_state.envelope(&n.computed_id, EnvelopeKind::SubtreePress))
        .max(ui_state.envelope(&n.computed_id, EnvelopeKind::SubtreeFocus));
    // `hover_alpha` lerps the node's drawn alpha between `rest` and
    // `peak` along the **subtree interaction envelope of the
    // surrounding interaction region** — `max` of the nearest
    // focusable ancestor's subtree envelope (cascaded as
    // `inherited_interaction_envelope`) and this node's own subtree
    // envelope when the consumer is itself focusable / a hover_alpha
    // wrapper.
    //
    // The ancestor half handles the close-×-on-tab pattern: the close
    // is below a focusable tab; when the tab (or anything inside it)
    // is the hot target, the tab's subtree envelope rises and the
    // close fades in. The self half handles the action-pill pattern:
    // a non-focusable wrapper carrying `hover_alpha` whose own
    // descendants are the hot target — the wrapper's own subtree
    // envelope captures that case directly.
    //
    // Distinct from the per-node `Hover` / `Press` / `FocusRing`
    // envelopes used by `apply_state` (single-target visuals like
    // hover-lighten) and from `inherited_hover_envelope` /
    // `inherited_press_envelope` (the per-node envelope cascade for
    // `state_follows_interactive_ancestor`). Three independent
    // mechanisms, each answering a different question:
    //   - "is this node the hot target?" → per-node envelopes
    //   - "is the slider's focusable container hot?" → per-node
    //     cascade (state_follows_interactive_ancestor)
    //   - "is anything in the surrounding interaction region hot?" →
    //     subtree-interaction cascade (hover_alpha)
    let hover_alpha_mul = match n.hover_alpha {
        Some(cfg) => {
            let combined = inherited_interaction_envelope.max(self_interaction_envelope);
            cfg.rest + (cfg.peak - cfg.rest) * combined
        }
        None => 1.0,
    };
    let opacity =
        inherited_opacity * n.opacity * focus_alpha_mul * blink_alpha_mul * hover_alpha_mul;
    // Children inherit the *immediate* focusable ancestor's envelope.
    // When this node is itself focusable, its envelope replaces the
    // inherited one; otherwise the inherited value passes through.
    // Hover / press follow the same rule so opt-in descendants can
    // borrow their interactive ancestor's state envelopes (see
    // `state_follows_interactive_ancestor`).
    let child_focus_envelope = if n.focusable {
        focus_ring_alpha
    } else {
        inherited_focus_envelope
    };
    let child_hover_envelope = if n.focusable {
        hover_amount
    } else {
        inherited_hover_envelope
    };
    let child_press_envelope = if n.focusable {
        press_amount
    } else {
        inherited_press_envelope
    };
    // The interaction-envelope cascade replaces at focusable nodes:
    // descendants of a focusable container read *that* container's
    // subtree envelope, not the grandparent's. `hover_alpha` consumers
    // OR-merge with their own (via `self_interaction_envelope` above)
    // so a focusable consumer like an `icon_button` close-× still
    // sees its parent tab's envelope through this cascade.
    let child_interaction_envelope = if n.focusable {
        self_interaction_envelope
    } else {
        inherited_interaction_envelope
    };

    let translated_rect = translated(computed, total_translate);
    // The layout rect, post translate + scale, is the visual boundary the
    // SDF and clip both anchor to. `painted_rect` extends it by
    // `paint_overflow` so the quad has room to draw focus rings, drop
    // shadows, and other halos *outside* the layout box without
    // affecting sibling positions. Drop shadow auto-widens the band
    // (per-side max with explicit `paint_overflow`) so `.shadow(s)`
    // works without every shadow-using widget remembering to set
    // `paint_overflow` separately. The stock-shader branch resolves the
    // *effective* shadow (post-theme) before computing `painted_rect`,
    // since surface roles can rewrite the shadow uniform.
    let inner_painted_rect = scaled_around_center(translated_rect, n.scale);
    let painted_font_size = n.font_size * n.scale;

    // Clip uses the layout rect, not the overflowed painted rect:
    // `clip()` is about constraining descendants to the layout box, not
    // about whether this element's own paint can spill into its
    // overflow band.
    let own_scissor = if n.clip {
        intersect_scissor(inherited_scissor, inner_painted_rect)
    } else {
        inherited_scissor
    };

    if matches!(
        n.kind,
        Kind::Custom(TEXT_AREA_SELECTION_LAYER) | Kind::Custom(TEXT_AREA_CARET_LAYER)
    ) {
        push_text_area_editor_overlay(
            n,
            ui_state,
            theme,
            out,
            inner_painted_rect,
            own_scissor,
            opacity,
            inherited_focus_envelope,
            painted_font_size,
            weight,
        );
    }

    // Surface paint. Either a custom shader override, or the implicit
    // `stock::rounded_rect` driven by the El's fill/stroke/radius/shadow.
    if let Some(custom) = &n.shader_override {
        // Custom shaders manage their own paint extent; we only honor
        // explicit `paint_overflow` here. They may pack a shadow into
        // their own uniform name, which we can't introspect.
        let painted_rect = inner_painted_rect.outset(n.paint_overflow);
        let mut uniforms = custom.uniforms.clone();
        uniforms.insert("inner_rect", inner_rect_uniform(inner_painted_rect));
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: custom.handle,
            uniforms,
        });
    } else if fill.is_some() || stroke.is_some() || focus_ring_alpha > 0.0 {
        let mut uniforms = UniformBlock::new();
        if let Some(c) = fill {
            // `dim_fill` lerps the painted color toward `fill` as the
            // inherited focus envelope rises. `inherited_focus_envelope`
            // here is the nearest focusable ancestor's envelope (the
            // band's parent text_input / text_area), so the band reads
            // as muted while the input is unfocused and saturates as
            // the focus animation completes.
            //
            // Resolve `dim` through the palette before mixing — `c` is
            // already palette-resolved by `apply_state` above, but
            // `dim_fill` comes straight from the El. Without this, the
            // unfocused band reads against the compile-time dark rgb
            // of the dim token and doesn't track a runtime palette swap.
            let resolved = match n.dim_fill {
                Some(dim) => theme.resolve(dim).mix(c, inherited_focus_envelope),
                None => c,
            };
            uniforms.insert("fill", UniformValue::Color(opaque(resolved, opacity)));
        }
        if let Some(c) = stroke {
            uniforms.insert("stroke", UniformValue::Color(opaque(c, opacity)));
            uniforms.insert("stroke_width", UniformValue::F32(n.stroke_width));
        }
        // `radius` carries the max corner so custom shaders that read
        // a scalar uniform see the same shape as before. Per-corner
        // values go on `radii` (tl, tr, br, bl) — stock::rounded_rect
        // and stock::image read this for the SDF; SVG bundle output
        // emits a `<path>` when corners differ, `<rect rx>` otherwise.
        uniforms.insert("radius", UniformValue::F32(n.radius.max()));
        uniforms.insert("radii", UniformValue::Vec4(n.radius.to_array()));
        if n.shadow > 0.0 {
            uniforms.insert("shadow", UniformValue::F32(n.shadow));
        }
        uniforms.insert("inner_rect", inner_rect_uniform(inner_painted_rect));
        // Focus ring rides on the node's own quad: the library injects a
        // `focus_color` (with the eased focus alpha already multiplied
        // into its rgba) plus `focus_width`. Positive width means outside
        // the layout rect; negative means an inside ring for dense flush rows.
        // Custom shaders read the same uniforms and decide for
        // themselves what to paint — the symmetry rule.
        if n.focusable && focus_ring_alpha > 0.0 {
            let base = tokens::RING;
            let eased_alpha = (base.a * focus_ring_alpha * opacity).clamp(0.0, 1.0);
            uniforms.insert(
                "focus_color",
                UniformValue::Color(base.with_alpha(eased_alpha)),
            );
            let focus_width = match n.focus_ring_placement {
                FocusRingPlacement::Outside => tokens::RING_WIDTH,
                FocusRingPlacement::Inside => -tokens::RING_WIDTH,
            };
            uniforms.insert("focus_width", UniformValue::F32(focus_width));
        }
        theme.apply_surface_uniforms(n.surface_role, &mut uniforms);
        // Read shadow + stroke *after* theme has had its say — surface
        // roles (Panel/Popover/Sunken/...) can override either uniform,
        // and we want the painted rect to track what actually renders.
        let effective_shadow = match uniforms.get("shadow") {
            Some(UniformValue::F32(s)) => *s,
            _ => 0.0,
        };
        let effective_stroke_width = if uniforms.contains_key("stroke") {
            match uniforms.get("stroke_width") {
                Some(UniformValue::F32(w)) => *w,
                _ => 0.0,
            }
        } else {
            0.0
        };
        let focus_width = if n.focusable
            && focus_ring_alpha > 0.0
            && matches!(n.focus_ring_placement, FocusRingPlacement::Outside)
        {
            tokens::RING_WIDTH
        } else {
            0.0
        };
        let painted_rect = inner_painted_rect.outset(combined_overflow(
            n.paint_overflow,
            effective_shadow,
            effective_stroke_width,
            focus_width,
        ));
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: theme.surface_handle(n.surface_role),
            uniforms,
        });
    }

    if let Some(text) = &n.text {
        // `padding` on a text-bearing node insets the glyph rect the
        // same way it insets the children of a container node — so
        // `text("X").padding(...)` and `column([text("X")]).padding(...)`
        // produce visually identical results. Without this, padding on
        // a text node would silently inflate intrinsic measurement only
        // and disappear once `Align::Stretch` flattened the Hug width.
        let glyph_rect = inner_painted_rect.inset(n.padding);
        if !rect_visible_in_scissor(glyph_rect, own_scissor) {
            stats.culled_text_ops += 1;
        } else {
            let display = match suffix {
                Some(s) => format!("{text}{s}"),
                None => text.clone(),
            };
            let display = match (n.text_wrap, n.text_max_lines) {
                (TextWrap::Wrap, Some(max_lines)) => text_metrics::clamp_text_to_lines_with_family(
                    &display,
                    painted_font_size,
                    n.font_family,
                    weight,
                    n.font_mono,
                    glyph_rect.w,
                    max_lines,
                ),
                _ => display,
            };
            let display = match (n.text_wrap, n.text_overflow) {
                (TextWrap::NoWrap, TextOverflow::Ellipsis) => {
                    text_metrics::ellipsize_text_with_family(
                        &display,
                        painted_font_size,
                        n.font_family,
                        weight,
                        n.font_mono,
                        glyph_rect.w,
                    )
                }
                _ => display,
            };
            let anchor = match n.text_align {
                TextAlign::Start => TextAnchor::Start,
                TextAlign::Center => TextAnchor::Middle,
                TextAlign::End => TextAnchor::End,
            };
            let text_color = opaque(text_color.unwrap_or(tokens::FOREGROUND), opacity);
            let layout = text_metrics::layout_text_with_line_height_and_family(
                &display,
                painted_font_size,
                n.line_height * n.scale,
                n.font_family,
                weight,
                n.font_mono,
                n.text_wrap,
                match n.text_wrap {
                    TextWrap::NoWrap => None,
                    TextWrap::Wrap => Some(glyph_rect.w),
                },
            );

            push_selection_bands_for_text(
                n,
                ui_state,
                out,
                glyph_rect,
                own_scissor,
                opacity,
                &display,
                painted_font_size,
                effective_text_family(n),
                weight,
                n.text_wrap,
            );

            out.push(DrawOp::GlyphRun {
                id: n.computed_id.clone(),
                rect: glyph_rect,
                scissor: own_scissor,
                shader: ShaderHandle::Stock(StockShader::Text),
                color: text_color,
                text: display,
                size: painted_font_size,
                line_height: n.line_height * n.scale,
                family: n.font_family,
                mono_family: n.mono_font_family,
                weight,
                mono: n.font_mono,
                wrap: n.text_wrap,
                anchor,
                layout,
                underline: n.text_underline,
                strikethrough: n.text_strikethrough,
                link: n.text_link.clone(),
            });
        }
    }

    if let Some(source) = &n.icon {
        let color = opaque(text_color.unwrap_or(tokens::FOREGROUND), opacity);
        let inner = inner_painted_rect.inset(n.padding);
        let icon_size = painted_font_size.min(inner.w).min(inner.h).max(1.0);
        let icon_rect = Rect::new(
            inner.center_x() - icon_size * 0.5,
            inner.center_y() - icon_size * 0.5,
            icon_size,
            icon_size,
        );
        out.push(DrawOp::Icon {
            id: n.computed_id.clone(),
            rect: icon_rect,
            scissor: own_scissor,
            source: source.clone(),
            color,
            size: icon_size,
            stroke_width: n.icon_stroke_width * n.scale,
        });
    }

    if let Some(image) = &n.image {
        let inner = inner_painted_rect.inset(n.padding);
        let dest = n.image_fit.project(image.width(), image.height(), inner);
        // Always clip image draws to the El's content rect so `Cover`
        // / `None` overflow is cropped without forcing every author to
        // call `.clip()`. The clamp respects any inherited scissor —
        // when the El's `inner` is fully outside an ancestor clip,
        // `intersect_scissor` produces `Some(Rect::zero)` and the
        // renderer drops the draw (a bare `s.intersect(inner)` would
        // hand back `None`, which downstream means "no scissor" and
        // would paint the image full-bleed past the ancestor clip).
        let scissor = intersect_scissor(own_scissor, inner);
        let tint = n.image_tint.map(|c| opaque(c, opacity));
        out.push(DrawOp::Image {
            id: n.computed_id.clone(),
            rect: dest,
            scissor,
            image: image.clone(),
            tint,
            radius: n.radius,
            fit: n.image_fit,
            range_limit: n.image_range_limit,
        });
    }

    if let Some(crate::surface::SurfaceSource::Texture(tex)) = &n.surface_source {
        let inner = inner_painted_rect.inset(n.padding);
        let (tw, th) = tex.size_px();
        let dest = n.surface_fit.project(tw, th, inner);
        // Always clip surface draws to the El's content rect so
        // `Cover` / `None` overflow and any out-of-bounds
        // `surface_transform` is cropped without forcing every author
        // to call `.clip()`. The clamp respects any inherited scissor;
        // see the matching block on the image branch above for why
        // this goes through `intersect_scissor` rather than a bare
        // `s.intersect(inner)`.
        let scissor = intersect_scissor(own_scissor, inner);
        out.push(DrawOp::AppTexture {
            id: n.computed_id.clone(),
            rect: dest,
            scissor,
            texture: tex.clone(),
            alpha: n.surface_alpha,
            fit: n.surface_fit,
            transform: n.surface_transform,
        });
    }

    if let Some(spec) = &n.scene_source {
        let inner = inner_painted_rect.inset(n.padding);
        let scissor = intersect_scissor(own_scissor, inner);
        // Resolve the camera. The pose comes from the framing policy:
        // Manual uses the app's pose verbatim; Auto/Fit frame the content
        // (keeping any app-supplied orbit angles). near/far are then sized
        // from the *full* view extent — content unioned with the grid/axes
        // reference bounds — so a grid larger than the data isn't clipped.
        // (Interactive orbit + animated re-centre read keyed `ui_state`
        // once that lands; until then this is a static framed view.)
        use crate::scene::Framing;
        let content =
            crate::scene::Scene3DData::content_bounds(&spec.meshes, &spec.points, &spec.lines);
        let base = spec.camera.unwrap_or_default();
        let pose = match spec.framing {
            // App owns the absolute pose.
            Framing::Manual => base,
            // Library-owned keyed camera (spring-animated; ticked just
            // before this pass). Falls back to a static fit only if the
            // node hasn't been ticked yet (shouldn't happen in the normal
            // prepare order, but keeps draw_ops total).
            Framing::Auto | Framing::Fit => ui_state
                .scene_camera(&n.computed_id)
                .unwrap_or_else(|| base.fitted(content)),
        };
        let mut view_bounds = content;
        if let Some(refs) = spec.style.reference_extent() {
            view_bounds = view_bounds.union(refs);
        }
        let camera = pose.resolve(view_bounds);
        let scene = std::sync::Arc::new(crate::scene::Scene3DData {
            meshes: spec.meshes.clone(),
            points: spec.points.clone(),
            lines: spec.lines.clone(),
            camera,
            lights: spec.lights,
            style: spec.style,
            // Capture depth when the scene carries labels to occlude — axis
            // labels or per-point labels/tooltips.
            capture_depth: spec.axes.is_some() || spec.points.iter().any(|p| p.labels.is_some()),
        });
        out.push(DrawOp::Scene3D {
            id: n.computed_id.clone(),
            rect: inner,
            scissor,
            scene,
        });
        // Axis labels are backend-neutral text, projected through the same
        // resolved camera and emitted after the scene op so they paint on
        // top of the composited 3D content.
        if let Some(axes) = &spec.axes {
            push_axis_labels(
                axes,
                &spec.style.grid,
                &camera,
                inner,
                scissor,
                &n.computed_id,
                opacity,
                ui_state.scene_depth(&n.computed_id),
                out,
            );
        }
        // Per-point labels / hover tooltips, projected through the same
        // camera + depth map.
        for (mi, draw) in spec.points.iter().enumerate() {
            if draw.labels.is_some() {
                let pick = push_point_labels(
                    draw,
                    &camera,
                    inner,
                    scissor,
                    &n.computed_id,
                    mi,
                    opacity,
                    ui_state.scene_depth(&n.computed_id),
                    ui_state.pointer_pos,
                    out,
                );
                if let Some((d2, point)) = pick {
                    stats.consider_pick(
                        d2,
                        crate::scene::ScenePointPick {
                            scene: n.computed_id.clone(),
                            mark: mi,
                            point,
                        },
                    );
                }
            }
        }
    }

    if let Some(asset) = &n.vector_source {
        let inner = inner_painted_rect.inset(n.padding);
        // See the image branch above for the empty-intersection
        // rationale behind `intersect_scissor`.
        let scissor = intersect_scissor(own_scissor, inner);
        out.push(DrawOp::Vector {
            id: n.computed_id.clone(),
            rect: inner,
            scissor,
            asset: asset.clone(),
            render_mode: n.vector_render_mode,
        });
    }

    if matches!(n.kind, Kind::Math) {
        if let Some(source) = &n.selection_source {
            push_atomic_selection_band(
                n,
                ui_state,
                out,
                inner_painted_rect.inset(n.padding),
                own_scissor,
                opacity,
                source.visible_len(),
            );
        }
        if let Some(expr) = &n.math {
            push_math_ops(
                n,
                expr,
                inner_painted_rect.inset(n.padding),
                own_scissor,
                opacity,
                out,
            );
        }
        return;
    }

    // Attributed paragraph: aggregate child Text/HardBreak runs into one
    // DrawOp::AttributedText so cosmic-text shapes the runs together
    // (wrapping crosses run boundaries like real prose). Skip recursion
    // into children — they're encoded in the runs and don't paint
    // independently.
    if matches!(n.kind, Kind::Inlines) {
        let glyph_rect = inner_painted_rect.inset(n.padding);
        if !rect_visible_in_scissor(glyph_rect, own_scissor) {
            stats.culled_text_ops += 1;
            return;
        }
        let inline_size = inline_paragraph_font_size(n) * n.scale;
        let inline_line_height = inline_paragraph_line_height(n) * n.scale;
        if n.children.iter().any(|c| matches!(c.kind, Kind::Math)) {
            push_inline_mixed_ops(n, ui_state, glyph_rect, own_scissor, opacity, out);
            return;
        }
        if let Some(source) = &n.selection_source {
            if matches!(n.text_wrap, TextWrap::NoWrap) {
                push_selection_bands_for_inlines(
                    n,
                    ui_state,
                    out,
                    glyph_rect,
                    own_scissor,
                    opacity,
                    &source.visible,
                    inline_size,
                    inline_line_height,
                );
            } else {
                push_selection_bands_for_text(
                    n,
                    ui_state,
                    out,
                    glyph_rect,
                    own_scissor,
                    opacity,
                    &source.visible,
                    inline_size,
                    effective_text_family(n),
                    FontWeight::Regular,
                    n.text_wrap,
                );
            }
        }
        let runs = collect_inline_runs(n, opacity);
        let concat: String = runs.iter().map(|(t, _)| t.as_str()).collect();
        let anchor = match n.text_align {
            TextAlign::Start => TextAnchor::Start,
            TextAlign::Center => TextAnchor::Middle,
            TextAlign::End => TextAnchor::End,
        };
        let layout = text_metrics::layout_text_with_line_height_and_family(
            &concat,
            inline_size,
            inline_line_height,
            n.font_family,
            FontWeight::Regular,
            false,
            n.text_wrap,
            match n.text_wrap {
                TextWrap::NoWrap => None,
                TextWrap::Wrap => Some(glyph_rect.w),
            },
        );
        out.push(DrawOp::AttributedText {
            id: n.computed_id.clone(),
            rect: glyph_rect,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::Text),
            runs,
            size: inline_size,
            line_height: inline_line_height,
            wrap: n.text_wrap,
            anchor,
            layout,
        });
        return;
    }

    for c in &n.children {
        push_node(
            c,
            ui_state,
            theme,
            out,
            own_scissor,
            total_translate,
            opacity,
            child_focus_envelope,
            child_hover_envelope,
            child_press_envelope,
            child_interaction_envelope,
            stats,
        );
    }

    // Scrollbar thumb. Painted *after* children so it sits on top
    // visually, with `own_scissor` so it inherits the scrollable's
    // clip but is otherwise free of the scroll offset (the layout
    // pass shifts the children, not the thumb). `scroll.thumb_rects` is
    // populated only when the scrollable opted in and content
    // overflows, so the gating is implicit. When the pointer is
    // anywhere within the track or a drag is active, the visible
    // thumb expands to `SCROLLBAR_THUMB_WIDTH_ACTIVE` (right-anchored)
    // so the cursor sits inside the thumb instead of pinning the
    // track's right edge.
    if let Some(thumb_rect) = ui_state.scroll.thumb_rects.get(&n.computed_id) {
        let active = thumb_is_active(n, ui_state);
        let visible = if active {
            let new_w = tokens::SCROLLBAR_THUMB_WIDTH_ACTIVE.max(thumb_rect.w);
            Rect::new(
                thumb_rect.right() - new_w,
                thumb_rect.y,
                new_w,
                thumb_rect.h,
            )
        } else {
            *thumb_rect
        };
        let painted_thumb = translated(visible, total_translate);
        let base_fill = if active {
            tokens::SCROLLBAR_THUMB_FILL_ACTIVE
        } else {
            tokens::SCROLLBAR_THUMB_FILL
        };
        let mut uniforms = UniformBlock::new();
        uniforms.insert("fill", UniformValue::Color(opaque(base_fill, opacity)));
        uniforms.insert("radius", UniformValue::F32(visible.w.min(visible.h) * 0.5));
        uniforms.insert("inner_rect", inner_rect_uniform(painted_thumb));
        out.push(DrawOp::Quad {
            id: format!("{}.scrollbar-thumb", n.computed_id),
            rect: painted_thumb,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::RoundedRect),
            uniforms,
        });
    }
}

fn push_math_ops(
    n: &El,
    expr: &crate::math::MathExpr,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    out: &mut Vec<DrawOp>,
) {
    let layout = crate::math::layout_math(expr, n.font_size * n.scale, n.math_display);
    let origin_x = match n.math_display {
        crate::math::MathDisplay::Inline => rect.x,
        crate::math::MathDisplay::Block => rect.x + ((rect.w - layout.width) * 0.5).max(0.0),
    };
    let baseline_y = rect.y + layout.ascent;
    let color = opaque(crate::math::resolved_math_color(n.text_color), opacity);
    for (i, atom) in layout.atoms.iter().enumerate() {
        match atom {
            crate::math::MathAtom::Glyph {
                text,
                x,
                y_baseline,
                size,
                weight,
                ..
            } => {
                let glyph_layout = crate::math::math_glyph_layout(text, *size, *weight);
                let glyph_baseline = glyph_layout
                    .lines
                    .first()
                    .map(|line| line.baseline)
                    .unwrap_or_else(|| crate::text::metrics::line_height(*size) * 0.75);
                let glyph_rect = Rect::new(
                    origin_x + x,
                    baseline_y + y_baseline - glyph_baseline,
                    glyph_layout.width,
                    glyph_layout.height,
                );
                out.push(DrawOp::GlyphRun {
                    id: format!("{}.math-glyph.{i}", n.computed_id),
                    rect: glyph_rect,
                    scissor,
                    shader: ShaderHandle::Stock(StockShader::Text),
                    color,
                    text: text.clone(),
                    size: *size,
                    line_height: crate::text::metrics::line_height(*size),
                    family: n.font_family,
                    mono_family: n.mono_font_family,
                    weight: *weight,
                    mono: false,
                    wrap: TextWrap::NoWrap,
                    anchor: TextAnchor::Start,
                    layout: glyph_layout,
                    underline: false,
                    strikethrough: false,
                    link: None,
                });
            }
            crate::math::MathAtom::GlyphId {
                glyph_id,
                rect,
                view_box,
            } => {
                push_math_glyph_id_op(
                    n, *glyph_id, *rect, *view_box, origin_x, baseline_y, scissor, color, i, out,
                );
            }
            crate::math::MathAtom::Rule { rect: atom_rect } => {
                let rule_rect = Rect::new(
                    origin_x + atom_rect.x,
                    baseline_y + atom_rect.y,
                    atom_rect.w,
                    atom_rect.h,
                );
                let mut uniforms = UniformBlock::new();
                uniforms.insert("fill", UniformValue::Color(color));
                uniforms.insert("radius", UniformValue::F32(0.0));
                uniforms.insert("inner_rect", inner_rect_uniform(rule_rect));
                out.push(DrawOp::Quad {
                    id: format!("{}.math-rule.{i}", n.computed_id),
                    rect: rule_rect,
                    scissor,
                    shader: ShaderHandle::Stock(StockShader::RoundedRect),
                    uniforms,
                });
            }
            crate::math::MathAtom::Radical { points, thickness } => {
                push_math_radical_op(
                    n, points, *thickness, origin_x, baseline_y, scissor, color, i, out,
                );
            }
            crate::math::MathAtom::Delimiter {
                delimiter,
                rect,
                thickness,
            } => {
                push_math_delimiter_op(
                    n, delimiter, *rect, *thickness, origin_x, baseline_y, scissor, color, i, out,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_math_glyph_id_op(
    n: &El,
    glyph_id: u16,
    atom_rect: Rect,
    view_box: Rect,
    origin_x: f32,
    baseline_y: f32,
    scissor: Option<Rect>,
    color: Color,
    atom_index: usize,
    out: &mut Vec<DrawOp>,
) {
    use crate::vector::VectorRenderMode;

    let Some(asset) = math_glyph_vector_asset(glyph_id, view_box) else {
        return;
    };
    out.push(DrawOp::Vector {
        id: format!("{}.math-glyph-id.{atom_index}", n.computed_id),
        rect: Rect::new(
            origin_x + atom_rect.x,
            baseline_y + atom_rect.y,
            atom_rect.w,
            atom_rect.h,
        ),
        scissor,
        asset: std::sync::Arc::new(asset),
        render_mode: VectorRenderMode::Mask { color },
    });
}

fn math_glyph_vector_asset(glyph_id: u16, view_box: Rect) -> Option<crate::vector::VectorAsset> {
    use crate::vector::{
        VectorAsset, VectorColor, VectorFill, VectorFillRule, VectorPath, VectorSegment,
    };

    const MAX_SOURCE_DIM: f32 = 24.0;

    struct Outline {
        segments: Vec<VectorSegment>,
    }

    impl ttf_parser::OutlineBuilder for Outline {
        fn move_to(&mut self, x: f32, y: f32) {
            self.segments.push(VectorSegment::MoveTo([x, -y]));
        }

        fn line_to(&mut self, x: f32, y: f32) {
            self.segments.push(VectorSegment::LineTo([x, -y]));
        }

        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            self.segments
                .push(VectorSegment::QuadTo([x1, -y1], [x, -y]));
        }

        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.segments
                .push(VectorSegment::CubicTo([x1, -y1], [x2, -y2], [x, -y]));
        }

        fn close(&mut self) {
            self.segments.push(VectorSegment::Close);
        }
    }

    let Ok(face) = ttf_parser::Face::parse(damascene_fonts::NOTO_SANS_MATH_REGULAR, 0) else {
        return None;
    };
    let mut outline = Outline {
        segments: Vec::new(),
    };
    let _ = face.outline_glyph(ttf_parser::GlyphId(glyph_id), &mut outline)?;
    if outline.segments.is_empty() {
        return None;
    }
    if view_box.w <= 0.0 || view_box.h <= 0.0 {
        return None;
    }
    let scale = MAX_SOURCE_DIM / view_box.w.max(view_box.h);
    normalize_vector_segments(&mut outline.segments, view_box, scale);
    let normalized_view_box = [0.0, 0.0, view_box.w * scale, view_box.h * scale];
    let path = VectorPath {
        segments: outline.segments,
        fill: Some(VectorFill {
            color: VectorColor::CurrentColor,
            opacity: 1.0,
            rule: VectorFillRule::NonZero,
        }),
        stroke: None,
    };
    Some(VectorAsset::from_paths(normalized_view_box, vec![path]))
}

fn normalize_vector_segments(
    segments: &mut [crate::vector::VectorSegment],
    view_box: Rect,
    scale: f32,
) {
    use crate::vector::VectorSegment;

    let normalize = |point: &mut [f32; 2]| {
        point[0] = (point[0] - view_box.x) * scale;
        point[1] = (point[1] - view_box.y) * scale;
    };
    for segment in segments {
        match segment {
            VectorSegment::MoveTo(point) | VectorSegment::LineTo(point) => normalize(point),
            VectorSegment::QuadTo(control, point) => {
                normalize(control);
                normalize(point);
            }
            VectorSegment::CubicTo(control_a, control_b, point) => {
                normalize(control_a);
                normalize(control_b);
                normalize(point);
            }
            VectorSegment::Close => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_math_radical_op(
    n: &El,
    points: &[[f32; 2]; 5],
    thickness: f32,
    origin_x: f32,
    baseline_y: f32,
    scissor: Option<Rect>,
    color: Color,
    atom_index: usize,
    out: &mut Vec<DrawOp>,
) {
    use crate::vector::{PathBuilder, VectorAsset, VectorLineJoin, VectorRenderMode};

    let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|p| p[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = points
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let pad = thickness * 0.5;
    let local = |p: [f32; 2]| [p[0] - min_x + pad, p[1] - min_y + pad];
    let [p0, p1, p2, p3, p4] = points.map(local);
    let rect = Rect::new(
        origin_x + min_x - pad,
        baseline_y + min_y - pad,
        max_x - min_x + pad * 2.0,
        max_y - min_y + pad * 2.0,
    );
    let path = PathBuilder::new()
        .move_to(p0[0], p0[1])
        .line_to(p1[0], p1[1])
        .line_to(p2[0], p2[1])
        .line_to(p3[0], p3[1])
        .line_to(p4[0], p4[1])
        .stroke_solid(color, thickness)
        .stroke_line_join(VectorLineJoin::Miter)
        .build();
    let asset = VectorAsset::from_paths([0.0, 0.0, rect.w, rect.h], vec![path]);
    out.push(DrawOp::Vector {
        id: format!("{}.math-radical.{atom_index}", n.computed_id),
        rect,
        scissor,
        asset: std::sync::Arc::new(asset),
        render_mode: VectorRenderMode::Painted,
    });
}

#[allow(clippy::too_many_arguments)]
fn push_math_delimiter_op(
    n: &El,
    delimiter: &str,
    atom_rect: Rect,
    thickness: f32,
    origin_x: f32,
    baseline_y: f32,
    scissor: Option<Rect>,
    color: Color,
    atom_index: usize,
    out: &mut Vec<DrawOp>,
) {
    use crate::vector::{
        PathBuilder, VectorAsset, VectorLineCap, VectorLineJoin, VectorRenderMode,
    };

    let pad = thickness * 0.5;
    let rect = Rect::new(
        origin_x + atom_rect.x - pad,
        baseline_y + atom_rect.y - pad,
        atom_rect.w + pad * 2.0,
        atom_rect.h + pad * 2.0,
    );
    let w = atom_rect.w;
    let h = atom_rect.h;
    let x = |v: f32| v + pad;
    let y = |v: f32| v + pad;
    let base = PathBuilder::new();
    let path = match delimiter {
        "(" => base.move_to(x(w * 0.86), y(0.0)).cubic_to(
            x(w * 0.10),
            y(h * 0.10),
            x(w * 0.10),
            y(h * 0.90),
            x(w * 0.86),
            y(h),
        ),
        ")" => base.move_to(x(w * 0.14), y(0.0)).cubic_to(
            x(w * 0.90),
            y(h * 0.10),
            x(w * 0.90),
            y(h * 0.90),
            x(w * 0.14),
            y(h),
        ),
        "[" => base
            .move_to(x(w * 0.88), y(0.0))
            .line_to(x(w * 0.12), y(0.0))
            .line_to(x(w * 0.12), y(h))
            .line_to(x(w * 0.88), y(h)),
        "]" => base
            .move_to(x(w * 0.12), y(0.0))
            .line_to(x(w * 0.88), y(0.0))
            .line_to(x(w * 0.88), y(h))
            .line_to(x(w * 0.12), y(h)),
        "{" => base
            .move_to(x(w * 0.86), y(0.0))
            .cubic_to(
                x(w * 0.20),
                y(h * 0.04),
                x(w * 0.56),
                y(h * 0.39),
                x(w * 0.18),
                y(h * 0.48),
            )
            .quad_to(x(w * 0.04), y(h * 0.50), x(w * 0.18), y(h * 0.52))
            .cubic_to(
                x(w * 0.56),
                y(h * 0.61),
                x(w * 0.20),
                y(h * 0.96),
                x(w * 0.86),
                y(h),
            ),
        "}" => base
            .move_to(x(w * 0.14), y(0.0))
            .cubic_to(
                x(w * 0.80),
                y(h * 0.04),
                x(w * 0.44),
                y(h * 0.39),
                x(w * 0.82),
                y(h * 0.48),
            )
            .quad_to(x(w * 0.96), y(h * 0.50), x(w * 0.82), y(h * 0.52))
            .cubic_to(
                x(w * 0.44),
                y(h * 0.61),
                x(w * 0.80),
                y(h * 0.96),
                x(w * 0.14),
                y(h),
            ),
        "|" => base.move_to(x(w * 0.5), y(0.0)).line_to(x(w * 0.5), y(h)),
        "‖" => base
            .move_to(x(w * 0.34), y(0.0))
            .line_to(x(w * 0.34), y(h))
            .move_to(x(w * 0.66), y(0.0))
            .line_to(x(w * 0.66), y(h)),
        "⟨" => base
            .move_to(x(w * 0.84), y(0.0))
            .line_to(x(w * 0.18), y(h * 0.5))
            .line_to(x(w * 0.84), y(h)),
        "⟩" => base
            .move_to(x(w * 0.16), y(0.0))
            .line_to(x(w * 0.82), y(h * 0.5))
            .line_to(x(w * 0.16), y(h)),
        "⌊" => base
            .move_to(x(w * 0.18), y(0.0))
            .line_to(x(w * 0.18), y(h))
            .line_to(x(w * 0.88), y(h)),
        "⌋" => base
            .move_to(x(w * 0.82), y(0.0))
            .line_to(x(w * 0.82), y(h))
            .line_to(x(w * 0.12), y(h)),
        "⌈" => base
            .move_to(x(w * 0.88), y(0.0))
            .line_to(x(w * 0.18), y(0.0))
            .line_to(x(w * 0.18), y(h)),
        "⌉" => base
            .move_to(x(w * 0.12), y(0.0))
            .line_to(x(w * 0.82), y(0.0))
            .line_to(x(w * 0.82), y(h)),
        _ => return,
    }
    .stroke_solid(color, thickness)
    .stroke_line_cap(VectorLineCap::Round)
    .stroke_line_join(VectorLineJoin::Round)
    .build();

    let asset = VectorAsset::from_paths([0.0, 0.0, rect.w, rect.h], vec![path]);
    out.push(DrawOp::Vector {
        id: format!("{}.math-delimiter.{atom_index}", n.computed_id),
        rect,
        scissor,
        asset: std::sync::Arc::new(asset),
        render_mode: VectorRenderMode::Painted,
    });
}

fn push_inline_mixed_ops(
    n: &El,
    ui_state: &UiState,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    out: &mut Vec<DrawOp>,
) {
    let mut breaker = crate::text::inline_mixed::MixedInlineBreaker::new(
        n.text_wrap,
        Some(rect.w),
        n.font_size * 0.82,
        n.font_size * 0.22,
        n.line_height,
    );
    let mut line_items = Vec::new();
    let selected = n.selection_source.as_ref().and_then(|source| {
        selection_range_for_node(n, ui_state, source.visible_len()).map(|(lo, hi)| lo..hi)
    });
    let paint = InlineMixedLinePaint {
        parent: n,
        rect,
        scissor,
        opacity,
        selected: selected.as_ref(),
    };
    let mut visible_cursor = 0usize;

    let finish_line =
        |line_items: &mut Vec<InlineMixedItem>,
         out: &mut Vec<DrawOp>,
         breaker: &mut crate::text::inline_mixed::MixedInlineBreaker| {
            let line = breaker.finish_line();
            flush_inline_mixed_line(&paint, line.top, line.ascent, line_items, out);
        };

    for (i, child) in n.children.iter().enumerate() {
        match child.kind {
            Kind::HardBreak => {
                finish_line(&mut line_items, out, &mut breaker);
                visible_cursor += "\n".len();
                continue;
            }
            Kind::Text => {
                if let Some(text) = &child.text {
                    for (chunk_i, chunk) in inline_text_chunks(text).into_iter().enumerate() {
                        let chunk_visible = visible_cursor..(visible_cursor + chunk.len());
                        visible_cursor += chunk.len();
                        let is_space = chunk.chars().all(char::is_whitespace);
                        if breaker.skips_leading_space(is_space) {
                            continue;
                        }
                        let (w, ascent, descent) = inline_text_chunk_paint_metrics(child, chunk);
                        if breaker.wraps_before(is_space, w) {
                            finish_line(&mut line_items, out, &mut breaker);
                        }
                        if breaker.skips_overflowing_space(is_space, w) {
                            continue;
                        }
                        if is_space && !matches!(line_items.last(), Some(InlineMixedItem::Text(_)))
                        {
                            breaker.push(w, ascent, descent);
                            continue;
                        }
                        push_inline_text_item(
                            &mut line_items,
                            child,
                            i,
                            chunk_i,
                            chunk,
                            chunk_visible,
                            breaker.x(),
                        );
                        breaker.push(w, ascent, descent);
                    }
                }
                continue;
            }
            Kind::Math => {
                if let Some(expr) = &child.math {
                    let layout =
                        crate::math::layout_math(expr, child.font_size, child.math_display);
                    if breaker.wraps_before(false, layout.width) {
                        finish_line(&mut line_items, out, &mut breaker);
                    }
                    let width = layout.width;
                    let ascent = layout.ascent;
                    let descent = layout.descent;
                    let visible_len = "\u{fffc}".len();
                    let visible = visible_cursor..(visible_cursor + visible_len);
                    visible_cursor += visible_len;
                    line_items.push(InlineMixedItem::Math {
                        child: child.clone(),
                        expr: expr.clone(),
                        x: breaker.x(),
                        layout,
                        visible,
                    });
                    breaker.push(width, ascent, descent);
                }
            }
            _ => {
                let (w, ascent, descent) = inline_child_paint_metrics(child);
                if breaker.wraps_before(false, w) {
                    finish_line(&mut line_items, out, &mut breaker);
                }
                breaker.push(w, ascent, descent);
            }
        }
    }
    let line = breaker.finish_line();
    flush_inline_mixed_line(&paint, line.top, line.ascent, &mut line_items, out);
}

enum InlineMixedItem {
    Text(InlineTextItem),
    Math {
        child: El,
        expr: std::sync::Arc<crate::math::MathExpr>,
        x: f32,
        layout: crate::math::MathLayout,
        visible: std::ops::Range<usize>,
    },
}

struct InlineTextItem {
    child: El,
    text: String,
    x: f32,
    child_index: usize,
    chunk_index: usize,
    visible: std::ops::Range<usize>,
}

struct InlineMixedLinePaint<'a> {
    parent: &'a El,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    selected: Option<&'a std::ops::Range<usize>>,
}

fn push_inline_text_item(
    items: &mut Vec<InlineMixedItem>,
    child: &El,
    child_index: usize,
    chunk_index: usize,
    text: &str,
    visible: std::ops::Range<usize>,
    x: f32,
) {
    if text.is_empty() {
        return;
    }
    if let Some(InlineMixedItem::Text(prev)) = items.last_mut()
        && same_inline_text_style(&prev.child, child)
    {
        prev.text.push_str(text);
        prev.visible.end = visible.end;
        return;
    }
    items.push(InlineMixedItem::Text(InlineTextItem {
        child: child.clone(),
        text: text.to_string(),
        x,
        child_index,
        chunk_index,
        visible,
    }));
}

fn flush_inline_mixed_line(
    paint: &InlineMixedLinePaint<'_>,
    line_top: f32,
    line_ascent: f32,
    items: &mut Vec<InlineMixedItem>,
    out: &mut Vec<DrawOp>,
) {
    let baseline_y = paint.rect.y + line_top + line_ascent;
    for item in items.drain(..) {
        match item {
            InlineMixedItem::Text(item) => {
                push_inline_text_chunk(
                    paint.parent,
                    &item.child,
                    &item.text,
                    item.child_index,
                    item.chunk_index,
                    selection_overlap(paint.selected, &item.visible),
                    paint.rect,
                    paint.scissor,
                    paint.opacity,
                    item.x,
                    baseline_y,
                    out,
                );
            }
            InlineMixedItem::Math {
                child,
                expr,
                x,
                layout,
                visible,
            } => {
                let math_rect = Rect::new(
                    paint.rect.x + x,
                    baseline_y - layout.ascent,
                    layout.width,
                    layout.height(),
                );
                if selection_overlap(paint.selected, &visible).is_some() {
                    push_selection_band_rect(
                        paint.parent,
                        out,
                        math_rect,
                        paint.scissor,
                        paint.opacity,
                    );
                }
                push_math_ops(&child, &expr, math_rect, paint.scissor, paint.opacity, out);
            }
        }
    }
}

fn same_inline_text_style(a: &El, b: &El) -> bool {
    a.font_size == b.font_size
        && a.line_height == b.line_height
        && a.font_family == b.font_family
        && a.mono_font_family == b.mono_font_family
        && a.font_weight == b.font_weight
        && a.font_mono == b.font_mono
        && a.text_color == b.text_color
        && a.text_underline == b.text_underline
        && a.text_strikethrough == b.text_strikethrough
        && a.text_link == b.text_link
}

#[allow(clippy::too_many_arguments)]
fn push_inline_text_chunk(
    parent: &El,
    child: &El,
    text: &str,
    child_index: usize,
    chunk_index: usize,
    selected: Option<std::ops::Range<usize>>,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    x: f32,
    baseline_y: f32,
    out: &mut Vec<DrawOp>,
) {
    let size = child.font_size * parent.scale;
    let glyph_layout = crate::text::metrics::layout_text_with_line_height_and_family(
        text,
        size,
        child.line_height * parent.scale,
        child.font_family,
        child.font_weight,
        child.font_mono,
        TextWrap::NoWrap,
        None,
    );
    let glyph_baseline = glyph_layout
        .lines
        .first()
        .map(|line| line.baseline)
        .unwrap_or_else(|| crate::text::metrics::line_height(size) * 0.75);
    let glyph_rect = Rect::new(
        rect.x + x,
        baseline_y - glyph_baseline,
        glyph_layout.width,
        glyph_layout.height,
    );
    if let Some(selected) = selected {
        let lo = clamp_to_char_boundary(text, selected.start.min(text.len()));
        let hi = clamp_to_char_boundary(text, selected.end.min(text.len()));
        if lo < hi {
            let prefix = &text[..lo];
            let slice = &text[lo..hi];
            let band_x = glyph_rect.x
                + crate::text::metrics::line_width_with_family(
                    prefix,
                    size,
                    child.font_family,
                    child.font_weight,
                    child.font_mono,
                );
            let band_w = crate::text::metrics::line_width_with_family(
                slice,
                size,
                child.font_family,
                child.font_weight,
                child.font_mono,
            );
            push_selection_band_rect(
                parent,
                out,
                Rect::new(band_x, glyph_rect.y, band_w, glyph_rect.h),
                scissor,
                opacity,
            );
        }
    }
    let color = opaque(child.text_color.unwrap_or(tokens::FOREGROUND), opacity);
    out.push(DrawOp::GlyphRun {
        id: format!(
            "{}.inline-text.{child_index}.{chunk_index}",
            parent.computed_id
        ),
        rect: glyph_rect,
        scissor,
        shader: ShaderHandle::Stock(StockShader::Text),
        color,
        text: text.to_string(),
        size,
        line_height: child.line_height * parent.scale,
        family: child.font_family,
        mono_family: child.mono_font_family,
        weight: child.font_weight,
        mono: child.font_mono,
        wrap: TextWrap::NoWrap,
        anchor: TextAnchor::Start,
        layout: glyph_layout,
        underline: child.text_underline || child.text_link.is_some(),
        strikethrough: child.text_strikethrough,
        link: child.text_link.clone(),
    });
}

fn inline_text_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut last_space = None;
    for (i, ch) in text.char_indices() {
        let is_space = ch.is_whitespace();
        match last_space {
            None => last_space = Some(is_space),
            Some(prev) if prev != is_space => {
                chunks.push(&text[start..i]);
                start = i;
                last_space = Some(is_space);
            }
            _ => {}
        }
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

fn inline_text_chunk_paint_metrics(child: &El, text: &str) -> (f32, f32, f32) {
    let layout = crate::text::metrics::layout_text_with_line_height_and_family(
        text,
        child.font_size,
        child.line_height,
        child.font_family,
        child.font_weight,
        child.font_mono,
        TextWrap::NoWrap,
        None,
    );
    (layout.width, child.font_size * 0.82, child.font_size * 0.22)
}

fn inline_child_paint_metrics(child: &El) -> (f32, f32, f32) {
    match child.kind {
        Kind::Text => inline_text_chunk_paint_metrics(child, child.text.as_deref().unwrap_or("")),
        Kind::Math => {
            if let Some(expr) = &child.math {
                let layout = crate::math::layout_math(expr, child.font_size, child.math_display);
                (layout.width, layout.ascent, layout.descent)
            } else {
                (0.0, 0.0, 0.0)
            }
        }
        _ => (0.0, 0.0, 0.0),
    }
}

/// Active when the user is actively dragging this scrollable's thumb
/// or the pointer is hovering anywhere inside its track (the
/// generous-hitbox column on the right). Hover is computed against
/// the *un-translated* track rect since the pointer position is
/// captured pre-translate.
fn thumb_is_active(n: &El, ui_state: &UiState) -> bool {
    if let Some(drag) = ui_state.scroll.thumb_drag.as_ref()
        && drag.scroll_id == n.computed_id
    {
        return true;
    }
    if let (Some((px, py)), Some(track)) = (
        ui_state.pointer_pos,
        ui_state.scroll.thumb_tracks.get(&n.computed_id),
    ) {
        return track.contains(px, py);
    }
    false
}

/// Walk an Inlines paragraph's children and produce source-order
/// (text, RunStyle) tuples. Each `Kind::Text` child contributes one
/// run carrying its `font_weight`, `text_italic`, `font_mono`, and
/// `text_color`. `Kind::HardBreak` contributes a `\n` run with default
/// styling — cosmic-text turns the newline into a line break during
/// shaping, so style doesn't matter (no glyph is emitted).
fn collect_inline_runs(node: &El, opacity: f32) -> Vec<(String, RunStyle)> {
    let mut runs: Vec<(String, RunStyle)> = Vec::with_capacity(node.children.len());
    for c in &node.children {
        match c.kind {
            Kind::Text => {
                if let Some(text) = &c.text {
                    let color = opaque(c.text_color.unwrap_or(tokens::FOREGROUND), opacity);
                    let mut style = RunStyle::new(c.font_weight, color)
                        .family(c.font_family)
                        .mono_family(c.mono_font_family);
                    if c.text_italic {
                        style = style.italic();
                    }
                    if c.font_mono {
                        style = style.mono();
                    }
                    if let Some(bg) = c.text_bg {
                        style = style.with_bg(opaque(bg, opacity));
                    }
                    if let Some(url) = &c.text_link {
                        // .with_link sets color + underline; do it
                        // before the standalone underline / strike
                        // checks so an explicit `.underline()` on a
                        // link is a no-op rather than re-stomping.
                        style = style.with_link(url.clone());
                    }
                    if c.text_underline {
                        style = style.underline();
                    }
                    if c.text_strikethrough {
                        style = style.strikethrough();
                    }
                    runs.push((text.clone(), style));
                }
            }
            Kind::HardBreak => {
                runs.push((
                    "\n".to_string(),
                    RunStyle::new(FontWeight::Regular, tokens::FOREGROUND),
                ));
            }
            _ => {}
        }
    }
    runs
}

/// Pick the dominant font size for the paragraph's approximate
/// pre-shaping layout (used by SVG and lint). Mirrors the layout
/// pass's `inline_paragraph_size` heuristic — max across text
/// children, falling back to the parent's own `font_size`.
fn inline_paragraph_font_size(node: &El) -> f32 {
    let mut size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) {
            size = size.max(c.font_size);
        }
    }
    size
}

fn inline_paragraph_line_height(node: &El) -> f32 {
    let mut line_height: f32 = node.line_height;
    let mut max_size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) && c.font_size >= max_size {
            max_size = c.font_size;
            line_height = c.line_height;
        }
    }
    line_height
}

#[allow(clippy::too_many_arguments)]
fn push_selection_bands_for_inlines(
    n: &El,
    ui_state: &UiState,
    out: &mut Vec<DrawOp>,
    glyph_rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    visible: &str,
    font_size: f32,
    line_height: f32,
) {
    let Some((lo, hi)) = selection_range_for_node(n, ui_state, visible.len()) else {
        return;
    };

    let mut lines = vec![InlineSelectionLine::default()];
    let mut visible_cursor = 0usize;
    for child in &n.children {
        match child.kind {
            Kind::Text => {
                let Some(text) = child.text.as_deref() else {
                    continue;
                };
                for segment in text.split_inclusive('\n') {
                    let (segment, hard_break) = segment
                        .strip_suffix('\n')
                        .map(|line| (line, true))
                        .unwrap_or((segment, false));
                    if !segment.is_empty() {
                        let line_index = lines.len() - 1;
                        let x = lines[line_index].width;
                        let width = inline_selection_run_width(child, segment, font_size);
                        let end = visible_cursor + segment.len();
                        lines[line_index].runs.push(InlineSelectionRun {
                            child: child.clone(),
                            text: segment.to_string(),
                            visible: visible_cursor..end,
                            x,
                        });
                        lines[line_index].width += width;
                        visible_cursor = end;
                    }
                    if hard_break {
                        visible_cursor += "\n".len();
                        lines.push(InlineSelectionLine::default());
                    }
                }
            }
            Kind::HardBreak => {
                visible_cursor += "\n".len();
                lines.push(InlineSelectionLine::default());
            }
            _ => {}
        }
    }

    for (line_index, line) in lines.into_iter().enumerate() {
        let line_x = match n.text_align {
            TextAlign::Start => 0.0,
            TextAlign::Center => (glyph_rect.w - line.width).max(0.0) * 0.5,
            TextAlign::End => (glyph_rect.w - line.width).max(0.0),
        };
        let line_y = line_index as f32 * line_height;
        for run in line.runs {
            let Some(selected) = selection_overlap(Some(&(lo..hi)), &run.visible) else {
                continue;
            };
            let lo = clamp_to_char_boundary(&run.text, selected.start.min(run.text.len()));
            let hi = clamp_to_char_boundary(&run.text, selected.end.min(run.text.len()));
            if lo >= hi {
                continue;
            }
            let prefix = &run.text[..lo];
            let slice = &run.text[lo..hi];
            let band_x = glyph_rect.x
                + line_x
                + run.x
                + inline_selection_run_width(&run.child, prefix, font_size);
            let band_w = inline_selection_run_width(&run.child, slice, font_size);
            push_selection_band_rect(
                n,
                out,
                Rect::new(band_x, glyph_rect.y + line_y, band_w, line_height),
                scissor,
                opacity,
            );
        }
    }
}

#[derive(Default)]
struct InlineSelectionLine {
    width: f32,
    runs: Vec<InlineSelectionRun>,
}

struct InlineSelectionRun {
    child: El,
    text: String,
    visible: std::ops::Range<usize>,
    x: f32,
}

fn inline_selection_run_width(child: &El, text: &str, font_size: f32) -> f32 {
    text_metrics::line_width_with_family(
        text,
        font_size,
        child.font_family,
        child.font_weight,
        child.font_mono,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_selection_bands_for_text(
    n: &El,
    ui_state: &UiState,
    out: &mut Vec<DrawOp>,
    glyph_rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    display: &str,
    font_size: f32,
    family: FontFamily,
    weight: FontWeight,
    wrap: TextWrap,
) {
    // Selection band — emit behind the glyph run when this leaf is
    // selectable, keyed, and (part of) its bytes fall inside the active
    // selection range. Source-backed rich text passes its visible text
    // here, while copy routes through the source mapping.
    if n.selectable
        && let Some(key) = &n.key
        && let Some((lo, hi)) = crate::selection::slice_for_leaf(
            &ui_state.current_selection,
            &ui_state.selection.order,
            key,
            display.len(),
        )
    {
        let rects = text_metrics::selection_rects_with_family(
            display,
            lo,
            hi,
            font_size,
            family,
            weight,
            wrap,
            match wrap {
                TextWrap::NoWrap => None,
                TextWrap::Wrap => Some(glyph_rect.w),
            },
        );
        for (rx, ry, rw, rh) in rects {
            let band = Rect::new(glyph_rect.x + rx, glyph_rect.y + ry, rw, rh);
            let mut band_uniforms = UniformBlock::new();
            band_uniforms.insert(
                "fill",
                UniformValue::Color(opaque(tokens::SELECTION_BG, opacity)),
            );
            band_uniforms.insert("radius", UniformValue::F32(2.0));
            band_uniforms.insert("inner_rect", inner_rect_uniform(band));
            out.push(DrawOp::Quad {
                id: format!("{}.selection-band", n.computed_id),
                rect: band,
                scissor,
                shader: ShaderHandle::Stock(StockShader::RoundedRect),
                uniforms: band_uniforms,
            });
        }
    }
}

fn effective_text_family(n: &El) -> FontFamily {
    if n.font_mono {
        n.mono_font_family
    } else {
        n.font_family
    }
}

fn selection_range_for_node(
    n: &El,
    ui_state: &UiState,
    visible_len: usize,
) -> Option<(usize, usize)> {
    let key = n.key.as_ref()?;
    crate::selection::slice_for_leaf(
        &ui_state.current_selection,
        &ui_state.selection.order,
        key,
        visible_len,
    )
}

fn selection_overlap(
    selected: Option<&std::ops::Range<usize>>,
    item: &std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let selected = selected?;
    let start = selected.start.max(item.start);
    let end = selected.end.min(item.end);
    if start < end {
        Some((start - item.start)..(end - item.start))
    } else {
        None
    }
}

fn push_selection_band_rect(
    n: &El,
    out: &mut Vec<DrawOp>,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
) {
    let mut band_uniforms = UniformBlock::new();
    band_uniforms.insert(
        "fill",
        UniformValue::Color(opaque(tokens::SELECTION_BG, opacity)),
    );
    band_uniforms.insert("radius", UniformValue::F32(4.0));
    band_uniforms.insert("inner_rect", inner_rect_uniform(rect));
    out.push(DrawOp::Quad {
        id: format!("{}.selection-band", n.computed_id),
        rect,
        scissor,
        shader: ShaderHandle::Stock(StockShader::RoundedRect),
        uniforms: band_uniforms,
    });
}

fn push_atomic_selection_band(
    n: &El,
    ui_state: &UiState,
    out: &mut Vec<DrawOp>,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    visible_len: usize,
) {
    if visible_len == 0 {
        return;
    }
    if n.selectable
        && let Some(key) = &n.key
        && crate::selection::slice_for_leaf(
            &ui_state.current_selection,
            &ui_state.selection.order,
            key,
            visible_len,
        )
        .is_some()
    {
        push_selection_band_rect(n, out, rect, scissor, opacity);
    }
}

fn clamp_to_char_boundary(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while byte > 0 && !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

#[allow(clippy::too_many_arguments)]
fn push_text_area_editor_overlay(
    n: &El,
    ui_state: &UiState,
    theme: &Theme,
    out: &mut Vec<DrawOp>,
    rect: Rect,
    scissor: Option<Rect>,
    opacity: f32,
    focus_envelope: f32,
    font_size: f32,
    weight: FontWeight,
) {
    let (Some(key), Some(value)) = (n.text_link.as_deref(), n.tooltip.as_deref()) else {
        return;
    };
    let Some(view) = ui_state.current_selection.within(key) else {
        return;
    };
    match n.kind {
        Kind::Custom(TEXT_AREA_SELECTION_LAYER) => {
            if view.is_collapsed() {
                return;
            }
            let (lo, hi) = view.ordered();
            let rects = text_metrics::selection_rects(
                value,
                lo.min(value.len()),
                hi.min(value.len()),
                font_size,
                weight,
                TextWrap::Wrap,
                Some(rect.w.max(1.0)),
            );
            let fill = theme
                .resolve(tokens::SELECTION_BG_UNFOCUSED)
                .mix(theme.resolve(tokens::SELECTION_BG), focus_envelope);
            for (i, (rx, ry, rw, rh)) in rects.into_iter().enumerate() {
                let band = Rect::new(rect.x + rx, rect.y + ry, rw, rh);
                let mut uniforms = UniformBlock::new();
                uniforms.insert("fill", UniformValue::Color(opaque(fill, opacity)));
                uniforms.insert("radius", UniformValue::F32(2.0));
                uniforms.insert("inner_rect", inner_rect_uniform(band));
                out.push(DrawOp::Quad {
                    id: format!("{}.selection-band.{i}", n.computed_id),
                    rect: band,
                    scissor,
                    shader: ShaderHandle::Stock(StockShader::RoundedRect),
                    uniforms,
                });
            }
        }
        Kind::Custom(TEXT_AREA_CARET_LAYER) => {
            let head = view.head.min(value.len());
            let (x, y) = text_metrics::caret_xy(
                value,
                head,
                font_size,
                weight,
                TextWrap::Wrap,
                Some(rect.w.max(1.0)),
            );
            let caret = Rect::new(rect.x + x, rect.y + y, 2.0, tokens::TEXT_SM.line_height);
            let mut uniforms = UniformBlock::new();
            uniforms.insert(
                "fill",
                UniformValue::Color(opaque(theme.resolve(tokens::FOREGROUND), opacity)),
            );
            uniforms.insert("radius", UniformValue::F32(1.0));
            uniforms.insert("inner_rect", inner_rect_uniform(caret));
            out.push(DrawOp::Quad {
                id: format!("{}.caret", n.computed_id),
                rect: caret,
                scissor,
                shader: ShaderHandle::Stock(StockShader::RoundedRect),
                uniforms,
            });
        }
        _ => {}
    }
}

fn translated(r: Rect, offset: (f32, f32)) -> Rect {
    if offset.0 == 0.0 && offset.1 == 0.0 {
        return r;
    }
    Rect::new(r.x + offset.0, r.y + offset.1, r.w, r.h)
}

/// Combine an element's explicit `paint_overflow` with the implicit
/// halo a non-zero `shadow` and / or `stroke` needs around the layout
/// rect. The shadow's SDF in `stock::rounded_rect` softens over a
/// `blur`-wide band around an offset-down silhouette: alpha hits zero
/// at distance `blur` outside the (offset) box, so left/right need
/// `blur`, top needs `blur*0.5` (offset reduces upward extent), bottom
/// needs `blur*1.5`. Stroke straddles the boundary — its outside half
/// (`stroke_width*0.5`) plus the AA tail (≈1 px) lives just outside the
/// layout rect, so the painted quad needs that much room on every side
/// or the cardinal pixels of curved boundaries (the radio indicator's
/// circle, switch thumb, …) get clipped and the shape looks flattened
/// at top / bottom / left / right. Per-side max with the user's
/// `paint_overflow` so a ring outset + shadow + stroke on the
/// same node all fit.
fn combined_overflow(
    paint_overflow: Sides,
    shadow: f32,
    stroke_width: f32,
    focus_width: f32,
) -> Sides {
    let stroke_halo = if stroke_width > 0.0 {
        stroke_width * 0.5 + 1.0
    } else {
        0.0
    };
    let halo = stroke_halo.max(focus_width);
    let stroked = if halo > 0.0 {
        Sides {
            left: paint_overflow.left.max(halo),
            right: paint_overflow.right.max(halo),
            top: paint_overflow.top.max(halo),
            bottom: paint_overflow.bottom.max(halo),
        }
    } else {
        paint_overflow
    };
    if shadow <= 0.0 {
        return stroked;
    }
    Sides {
        left: stroked.left.max(shadow),
        right: stroked.right.max(shadow),
        top: stroked.top.max(shadow * 0.5),
        bottom: stroked.bottom.max(shadow * 1.5),
    }
}

/// Scale `r` uniformly by `s` around its centre. `s == 1.0` short-circuits
/// to identity so the common case is allocation-free of float drift.
fn scaled_around_center(r: Rect, s: f32) -> Rect {
    if (s - 1.0).abs() < f32::EPSILON {
        return r;
    }
    let cx = r.center_x();
    let cy = r.center_y();
    let w = r.w * s;
    let h = r.h * s;
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

fn opaque(c: Color, opacity: f32) -> Color {
    if (opacity - 1.0).abs() < f32::EPSILON {
        return c;
    }
    c.with_alpha(c.a * opacity.clamp(0.0, 1.0))
}

/// Project a world-space point to a centred text label inside `scene_rect`,
/// or `None` when it is behind the camera or projects outside the rect.
///
/// This is the reusable seam for **any** scene-anchored label — axis ticks
/// today, point labels / hover tooltips later all funnel through here. It
/// builds a tight, measured [`DrawOp::GlyphRun`] centred on the projected
/// point and clipped to the scene scissor, so labels render on every
/// backend through the normal text pipeline rather than as scene geometry.
/// Project `world` and place a measured text box relative to it per
/// `placement` (with `gap` px of clearance from the point), returning the
/// box rect + the measured layout. `None` for empty text, points behind the
/// camera, or anchors outside the scene rect. No occlusion — that's the
/// caller's gate (so the hover chip, which pre-picks a visible point, can
/// reuse this without the occlude-on-missing fail-safe).
fn project_label(
    camera: &crate::scene::ResolvedCamera,
    scene_rect: Rect,
    world: crate::scene::glam::Vec3,
    text: &str,
    size: f32,
    placement: crate::scene::LabelPlacement,
    gap: f32,
) -> Option<(Rect, text_metrics::TextLayout)> {
    use crate::scene::LabelPlacement as P;
    if text.is_empty() {
        return None;
    }
    let p = camera.project_to_screen(world, scene_rect)?;
    // Cull when the anchor lands outside the scene rect. The scissor would
    // clip a stray label, but culling avoids half-glyphs poking at the edge.
    if p.x < scene_rect.x
        || p.x > scene_rect.x + scene_rect.w
        || p.y < scene_rect.y
        || p.y > scene_rect.y + scene_rect.h
    {
        return None;
    }
    let layout = text_metrics::layout_text(
        text,
        size,
        FontWeight::default(),
        false,
        TextWrap::NoWrap,
        None,
    );
    let w = layout.width.max(1.0);
    let h = layout.height.max(layout.line_height);
    // Position the box per placement; anchor Middle centres text in the box
    // horizontally and the backend vertically centres NoWrap text in rect.h.
    let (x, y) = match placement {
        P::Center => (p.x - w * 0.5, p.y - h * 0.5),
        P::Above => (p.x - w * 0.5, p.y - gap - h),
        P::Below => (p.x - w * 0.5, p.y + gap),
        P::Left => (p.x - gap - w, p.y - h * 0.5),
        P::Right => (p.x + gap, p.y - h * 0.5),
    };
    Some((Rect::new(x, y, w, h), layout))
}

/// Build a centred text [`DrawOp::GlyphRun`] for a measured label box.
fn label_glyph(
    id: String,
    rect: Rect,
    scissor: Option<Rect>,
    color: Color,
    text: &str,
    size: f32,
    layout: text_metrics::TextLayout,
) -> DrawOp {
    DrawOp::GlyphRun {
        id,
        rect,
        scissor,
        shader: ShaderHandle::Stock(StockShader::Text),
        color,
        text: text.to_string(),
        size,
        line_height: layout.line_height,
        family: FontFamily::default(),
        mono_family: FontFamily::default(),
        weight: FontWeight::default(),
        mono: false,
        wrap: TextWrap::NoWrap,
        anchor: TextAnchor::Middle,
        layout,
        underline: false,
        strikethrough: false,
        link: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn scene_label(
    camera: &crate::scene::ResolvedCamera,
    scene_rect: Rect,
    scissor: Option<Rect>,
    world: crate::scene::glam::Vec3,
    text: &str,
    color: Color,
    size: f32,
    id: String,
    placement: crate::scene::LabelPlacement,
    gap: f32,
    occluder: Option<&crate::scene::SceneDepthMap>,
) -> Option<DrawOp> {
    // Depth-occlude against the (frame-late) scene depth map: hide anchors
    // behind solid geometry, and hide *every* label until a map exists — the
    // fail-safe that prevents a flash of labels punching through the scene.
    if occluder.is_none_or(|m| m.occludes(world)) {
        return None;
    }
    let (rect, layout) = project_label(camera, scene_rect, world, text, size, placement, gap)?;
    Some(label_glyph(id, rect, scissor, color, text, size, layout))
}

/// Emit axis tick + title labels for a scene, projecting each world-space
/// label through the resolved camera. Backend-neutral: pushes only text.
#[allow(clippy::too_many_arguments)]
fn push_axis_labels(
    axes: &crate::scene::Axes,
    grid: &crate::scene::style::GridSettings,
    camera: &crate::scene::ResolvedCamera,
    scene_rect: Rect,
    scissor: Option<Rect>,
    scene_id: &str,
    opacity: f32,
    occluder: Option<&crate::scene::SceneDepthMap>,
    out: &mut Vec<DrawOp>,
) {
    let color = opaque(axes.label_color, opacity);
    for (i, label) in axes.labels(grid).into_iter().enumerate() {
        if let Some(op) = scene_label(
            camera,
            scene_rect,
            scissor,
            label.world,
            &label.text,
            color,
            axes.label_size,
            format!("{scene_id}.axis-label.{i}"),
            crate::scene::LabelPlacement::Center,
            0.0,
            occluder,
        ) {
            out.push(op);
        }
    }
}

/// Marker radius in logical px, for label gap + hover-pick threshold.
/// World-sized markers project to a varying screen size, so fall back to a
/// reasonable constant rather than guessing.
fn marker_radius_px(style: &crate::scene::PointStyle) -> f32 {
    match style.size_mode {
        crate::scene::SizeMode::ScreenSpace => (style.size * 0.5).max(2.0),
        crate::scene::SizeMode::World => 8.0,
    }
}

/// Emit a scatter mark's per-point labels: persistent text for
/// [`LabelDisplay::Always`], or a single hover chip for
/// [`LabelDisplay::Hover`]. Reuses the depth map for occlusion (persistent
/// labels) / hover pickability.
/// Emit a mark's point labels. For [`LabelDisplay::Hover`], also returns the
/// picked point as `(cursor_distance², point_index)` so the caller can surface
/// it to the app; `None` when nothing is hovered or the mark isn't hover-typed.
#[allow(clippy::too_many_arguments)]
fn push_point_labels(
    draw: &crate::scene::PointDraw,
    camera: &crate::scene::ResolvedCamera,
    scene_rect: Rect,
    scissor: Option<Rect>,
    scene_id: &str,
    mark_index: usize,
    opacity: f32,
    occluder: Option<&crate::scene::SceneDepthMap>,
    pointer: Option<(f32, f32)>,
    out: &mut Vec<DrawOp>,
) -> Option<(f32, usize)> {
    let Some(labels) = &draw.labels else {
        return None;
    };
    let (data, _) = draw.geometry.snapshot();
    let marker_r = marker_radius_px(&draw.style);

    match labels.display {
        crate::scene::LabelDisplay::Always => {
            let color = opaque(labels.color.unwrap_or(tokens::FOREGROUND), opacity);
            for (i, point) in data.points.iter().enumerate() {
                let Some(text) = labels.get(i) else { continue };
                let world = draw.transform.transform_point3(point.position);
                if let Some(op) = scene_label(
                    camera,
                    scene_rect,
                    scissor,
                    world,
                    text,
                    color,
                    labels.size,
                    format!("{scene_id}.point-label.{mark_index}.{i}"),
                    labels.placement,
                    marker_r + 4.0,
                    occluder,
                ) {
                    out.push(op);
                }
            }
            None
        }
        crate::scene::LabelDisplay::Hover => {
            let (px, py) = pointer?;
            // Pick the labelled point nearest the cursor within the marker's
            // reach. Skip points the depth map says are hidden — you can't
            // hover what you can't see — but allow hover before a map exists.
            let threshold = marker_r + 6.0;
            let mut best: Option<(f32, usize, crate::scene::glam::Vec3)> = None;
            for (i, point) in data.points.iter().enumerate() {
                if labels.get(i).is_none() {
                    continue;
                }
                let world = draw.transform.transform_point3(point.position);
                if occluder.is_some_and(|m| m.occludes(world)) {
                    continue;
                }
                let Some(scr) = camera.project_to_screen(world, scene_rect) else {
                    continue;
                };
                let d2 = (scr.x - px).powi(2) + (scr.y - py).powi(2);
                if d2 <= threshold * threshold && best.is_none_or(|(bd, _, _)| d2 < bd) {
                    best = Some((d2, i, world));
                }
            }
            let (d2, i, world) = best?;
            let text = labels.get(i)?;
            push_tooltip_chip(
                camera,
                scene_rect,
                scissor,
                world,
                text,
                labels.size,
                format!("{scene_id}.point-tooltip.{mark_index}"),
                labels.placement,
                marker_r + 8.0,
                opacity,
                out,
            );
            // Surface the pick (distance² + point index) to the caller; the
            // chip is drawn either way.
            Some((d2, i))
        }
    }
}

/// Draw a hover tooltip: a rounded popover chip (fill + border) behind the
/// label text, anchored off the point. The caller has already confirmed the
/// point is visible, so no occlusion gate here.
#[allow(clippy::too_many_arguments)]
fn push_tooltip_chip(
    camera: &crate::scene::ResolvedCamera,
    scene_rect: Rect,
    scissor: Option<Rect>,
    world: crate::scene::glam::Vec3,
    text: &str,
    size: f32,
    id: String,
    placement: crate::scene::LabelPlacement,
    gap: f32,
    opacity: f32,
    out: &mut Vec<DrawOp>,
) {
    let Some((text_rect, layout)) =
        project_label(camera, scene_rect, world, text, size, placement, gap)
    else {
        return;
    };
    let (pad_x, pad_y) = (6.0, 3.0);
    let chip = Rect::new(
        text_rect.x - pad_x,
        text_rect.y - pad_y,
        text_rect.w + 2.0 * pad_x,
        text_rect.h + 2.0 * pad_y,
    );
    let mut uniforms = UniformBlock::new();
    uniforms.insert(
        "fill",
        UniformValue::Color(opaque(tokens::POPOVER, opacity)),
    );
    uniforms.insert(
        "stroke",
        UniformValue::Color(opaque(tokens::BORDER, opacity)),
    );
    uniforms.insert("stroke_width", UniformValue::F32(1.0));
    uniforms.insert("radius", UniformValue::F32(5.0));
    uniforms.insert("inner_rect", inner_rect_uniform(chip));
    out.push(DrawOp::Quad {
        id: format!("{id}.chip"),
        rect: chip,
        scissor,
        shader: ShaderHandle::Stock(StockShader::RoundedRect),
        uniforms,
    });
    out.push(label_glyph(
        format!("{id}.text"),
        text_rect,
        scissor,
        opaque(tokens::POPOVER_FOREGROUND, opacity),
        text,
        size,
        layout,
    ));
}

/// Resolve the effective `(fill, stroke, text_color, font_weight,
/// optional text suffix)` for paint.
///
/// Hover and press are applied as **envelope mixes**: the eased amounts
/// `hover` / `press` (both 0..1, written by the animation tracker into
/// [`UiState::envelope`]) lerp the build-time colour toward its
/// state-modulated form. This composition keeps state easing
/// independent of mid-flight changes to `n.fill` — the author can swap
/// a button's colour during a hover and the new colour appears with
/// the same eased lighten amount, no fighting between trackers.
///
/// Surfaces with no resting fill (`.ghost()`, `.outline()`, inactive tab
/// triggers) get a **synthesized state-only fill** instead — a faint
/// `ACCENT` whose alpha rises with hover and press. Mirrors the
/// shadcn idiom `hover:bg-accent active:bg-accent/80`: transparent at
/// rest, a soft surface fades in on interaction. Without this, the
/// envelope mix above has nothing to land on (`None.map(...)` is
/// `None`) and ghost surfaces show no feedback at all.
///
/// The synthesis only fires when the node already declares some
/// surface affordance — a non-zero radius or an explicit stroke. That
/// excludes layout-only focusable containers (the `stack(...)` outers
/// of `slider`, `switch`, `resize_handle`) where a translucent
/// rectangle behind the actual visual would compete with the widget's
/// own thumb / track / hairline.
///
/// Disabled (alpha multiply) and Loading (text suffix) aren't eased
/// and are still applied here, branching on the resolved `state`.
fn apply_state(
    n: &El,
    state: InteractionState,
    hover: f32,
    press: f32,
    palette: &Palette,
) -> (
    Option<Color>,
    Option<Color>,
    Option<Color>,
    FontWeight,
    Option<&'static str>,
) {
    // Resolve token rgb against the active palette *before* applying
    // any rgb-modifying op. lighten/darken/mix bake the result and
    // strip the token, so we have to compose the op against the
    // palette's rgb here — otherwise hover/press visuals are computed
    // off the compile-time dark fallback regardless of theme.
    let mut fill = n.fill.map(|c| palette.resolve(c));
    let mut stroke = n.stroke.map(|c| palette.resolve(c));
    let mut text_color = n.text_color.map(|c| palette.resolve(c));
    let weight = n.font_weight;
    let mut suffix = None;

    if hover > 0.0 {
        fill = fill.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN), hover));
        stroke = stroke.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN), hover));
        text_color = text_color.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN * 0.5), hover));
    }
    if press > 0.0 {
        fill = fill.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN), press));
        stroke = stroke.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN), press));
        text_color = text_color.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN * 0.5), press));
    }
    if n.fill.is_none()
        && (hover > 0.0 || press > 0.0)
        && (n.radius.any_nonzero() || n.stroke.is_some())
    {
        let alpha = (hover * tokens::STATE_FILL_HOVER_ALPHA
            + press * tokens::STATE_FILL_PRESS_ALPHA)
            .clamp(0.0, 1.0);
        // ACCENT.with_alpha keeps the token name, so the final
        // resolve_palette walk swaps the rgb to the active palette.
        fill = Some(tokens::ACCENT.with_alpha(alpha));
    }

    match state {
        InteractionState::Default
        | InteractionState::Focus
        | InteractionState::Hover
        | InteractionState::Press => {}
        InteractionState::Disabled => {
            let factor = tokens::DISABLED_ALPHA;
            fill = fill.map(|c| c.with_alpha(c.a * factor));
            stroke = stroke.map(|c| c.with_alpha(c.a * factor));
            text_color = text_color.map(|c| c.with_alpha(c.a * factor));
        }
        InteractionState::Loading => {
            text_color = text_color.map(|c| c.with_alpha(c.a * (200.0 / 255.0)));
            suffix = Some(" ⋯");
        }
    }
    (fill, stroke, text_color, weight, suffix)
}

/// Pack a rect as the `inner_rect` uniform value (vec4 of x, y, w, h).
fn inner_rect_uniform(r: Rect) -> UniformValue {
    UniformValue::Vec4([r.x, r.y, r.w, r.h])
}

fn intersect_scissor(current: Option<Rect>, next: Rect) -> Option<Rect> {
    match current {
        Some(r) => Some(r.intersect(next).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))),
        None => Some(next),
    }
}

fn rect_visible_in_scissor(rect: Rect, scissor: Option<Rect>) -> bool {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return false;
    }
    match scissor {
        Some(clip) => rect.intersect(clip).is_some(),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;
    use crate::{button, column, row};

    fn test_camera(eye: crate::scene::glam::Vec3) -> crate::scene::ResolvedCamera {
        crate::scene::ResolvedCamera {
            eye,
            target: crate::scene::glam::Vec3::ZERO,
            up: crate::scene::glam::Vec3::Y,
            fov_y: std::f32::consts::FRAC_PI_4,
            near: 0.1,
            far: 200.0,
        }
    }

    /// A 1×1 all-far depth map for `cam`/`rect` — occludes nothing in view,
    /// so labels project normally (isolates projection from occlusion).
    fn unoccluded(cam: crate::scene::ResolvedCamera, rect: Rect) -> crate::scene::SceneDepthMap {
        crate::scene::SceneDepthMap {
            camera: cam,
            rect,
            width: 1,
            height: 1,
            depth: std::sync::Arc::from(vec![1.0_f32]),
        }
    }

    #[test]
    fn scene_label_projects_in_front_and_culls_behind() {
        use crate::scene::glam::Vec3;
        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let map = unoccluded(cam, rect);
        // The origin is dead ahead → a label centred near the rect centre.
        let op = scene_label(
            &cam,
            rect,
            None,
            Vec3::ZERO,
            "0",
            Color::srgb_u8(255, 255, 255),
            11.0,
            "t.l.0".into(),
            crate::scene::LabelPlacement::Center,
            0.0,
            Some(&map),
        );
        let Some(DrawOp::GlyphRun {
            rect: r,
            anchor,
            text,
            ..
        }) = op
        else {
            panic!("expected a GlyphRun for an in-front point");
        };
        assert_eq!(text, "0");
        assert_eq!(anchor, TextAnchor::Middle);
        assert!((r.x + r.w * 0.5 - 100.0).abs() < 5.0, "centred in x");
        assert!((r.y + r.h * 0.5 - 100.0).abs() < 5.0, "centred in y");

        // A point behind the eye produces nothing.
        let behind = scene_label(
            &cam,
            rect,
            None,
            Vec3::new(0.0, 0.0, 20.0),
            "x",
            Color::srgb_u8(255, 255, 255),
            11.0,
            "t.l.1".into(),
            crate::scene::LabelPlacement::Center,
            0.0,
            Some(&map),
        );
        assert!(behind.is_none(), "points behind the camera are culled");

        // A point far off-axis but in front projects outside the rect.
        let off = scene_label(
            &cam,
            rect,
            None,
            Vec3::new(100.0, 0.0, 0.0),
            "x",
            Color::srgb_u8(255, 255, 255),
            11.0,
            "t.l.2".into(),
            crate::scene::LabelPlacement::Center,
            0.0,
            Some(&map),
        );
        assert!(off.is_none(), "off-rect labels are culled");
    }

    #[test]
    fn hover_pick_returns_nearest_labelled_point() {
        use crate::scene::glam::{Mat4, Vec3};
        use crate::scene::{
            PointData, PointDraw, PointLabels, PointStyle, PointsHandle, ScenePoint,
        };

        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let map = unoccluded(cam, rect);

        // Point 0 at the origin (projects to the rect centre); point 1 off to
        // the side. Both carry hover labels.
        let draw = PointDraw {
            geometry: PointsHandle::new(PointData {
                points: vec![
                    ScenePoint {
                        position: Vec3::ZERO,
                        color: [1.0; 4],
                    },
                    ScenePoint {
                        position: Vec3::new(2.0, 0.0, 0.0),
                        color: [1.0; 4],
                    },
                ],
            }),
            transform: Mat4::IDENTITY,
            style: PointStyle::default(),
            labels: Some(PointLabels::new(["A", "B"]).on_hover()),
        };
        let pick = |cursor| {
            let mut out = Vec::new();
            push_point_labels(
                &draw,
                &cam,
                rect,
                None,
                "scene",
                0,
                1.0,
                Some(&map),
                cursor,
                &mut out,
            )
        };

        // Cursor at the rect centre → picks the centred point (index 0).
        assert_eq!(
            pick(Some((100.0, 100.0))).map(|(_, i)| i),
            Some(0),
            "cursor over the centred point picks index 0"
        );
        // Cursor far from every point → no pick.
        assert!(
            pick(Some((10.0, 10.0))).is_none(),
            "cursor off every point picks nothing"
        );
        // No pointer at all → no pick.
        assert!(pick(None).is_none());
    }

    #[test]
    fn consider_pick_keeps_the_nearest() {
        use crate::scene::ScenePointPick;
        let p = |point| ScenePointPick {
            scene: "s".into(),
            mark: 0,
            point,
        };
        let mut s = DrawOpsStats::default();
        assert!(s.hovered_scene_point.is_none());
        s.consider_pick(100.0, p(7));
        assert_eq!(s.hovered_scene_point.as_ref().unwrap().point, 7);
        s.consider_pick(25.0, p(3)); // nearer wins
        assert_eq!(s.hovered_scene_point.as_ref().unwrap().point, 3);
        s.consider_pick(80.0, p(9)); // farther does not replace
        assert_eq!(s.hovered_scene_point.as_ref().unwrap().point, 3);
    }

    #[test]
    fn scene_label_occludes_without_map_and_behind_geometry() {
        use crate::scene::glam::Vec3;
        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let args = |occ: Option<&crate::scene::SceneDepthMap>| {
            scene_label(
                &cam,
                rect,
                None,
                Vec3::ZERO,
                "0",
                Color::srgb_u8(255, 255, 255),
                11.0,
                "t.l".into(),
                crate::scene::LabelPlacement::Center,
                0.0,
                occ,
            )
        };
        // No depth map yet → the fail-safe hides every label.
        assert!(args(None).is_none(), "no map hides labels");
        // A near surface in front of the anchor hides it.
        let near = crate::scene::SceneDepthMap {
            width: 1,
            height: 1,
            depth: std::sync::Arc::from(vec![0.0_f32]),
            ..unoccluded(cam, rect)
        };
        assert!(args(Some(&near)).is_none(), "occluded anchor is hidden");
        // An all-far map (empty background) lets it through.
        assert!(
            args(Some(&unoccluded(cam, rect))).is_some(),
            "visible anchor draws"
        );
    }

    #[test]
    fn push_axis_labels_emits_only_projected_text() {
        use crate::scene::Axes;
        use crate::scene::glam::Vec3;
        use crate::scene::style::GridSettings;
        let cam = test_camera(Vec3::new(15.0, 15.0, 15.0));
        let rect = Rect::new(0.0, 0.0, 400.0, 400.0);
        let map = unoccluded(cam, rect);
        let mut out = Vec::new();
        push_axis_labels(
            &Axes::titles("X", "Y", "Z"),
            &GridSettings::default(),
            &cam,
            rect,
            None,
            "scene",
            1.0,
            Some(&map),
            &mut out,
        );
        assert!(out.len() > 5, "default grid yields many ticks in view");
        for op in &out {
            match op {
                DrawOp::GlyphRun { id, .. } => {
                    assert!(id.starts_with("scene.axis-label."), "stable label id");
                }
                other => panic!("axis labels must be text, got {other:?}"),
            }
        }

        // With no depth map, the fail-safe suppresses all labels.
        let mut none_out = Vec::new();
        push_axis_labels(
            &Axes::titles("X", "Y", "Z"),
            &GridSettings::default(),
            &cam,
            rect,
            None,
            "scene",
            1.0,
            None,
            &mut none_out,
        );
        assert!(none_out.is_empty(), "no map → no labels");
    }

    /// One-point scatter mark labelled "A" at the origin, plus a camera/rect
    /// where the origin projects to the rect centre (100, 100).
    fn labeled_point(display: crate::scene::LabelDisplay) -> crate::scene::PointDraw {
        use crate::scene::glam::{Mat4, Vec3};
        use crate::scene::{PointData, PointLabels, PointStyle, PointsHandle, ScenePoint};
        let geometry = PointsHandle::new(PointData {
            points: vec![ScenePoint {
                position: Vec3::ZERO,
                color: [1.0; 4],
            }],
        });
        let labels = PointLabels::new(["A"]);
        let labels = match display {
            crate::scene::LabelDisplay::Always => labels.always(),
            crate::scene::LabelDisplay::Hover => labels.on_hover(),
        };
        crate::scene::PointDraw {
            geometry,
            transform: Mat4::IDENTITY,
            style: PointStyle::default(),
            labels: Some(labels),
        }
    }

    #[test]
    fn always_point_labels_emit_offset_glyphs() {
        use crate::scene::glam::Vec3;
        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let map = unoccluded(cam, rect);
        let draw = labeled_point(crate::scene::LabelDisplay::Always);
        let mut out = Vec::new();
        push_point_labels(
            &draw,
            &cam,
            rect,
            None,
            "scene",
            0,
            1.0,
            Some(&map),
            None,
            &mut out,
        );
        let glyph = out.iter().find_map(|op| match op {
            DrawOp::GlyphRun { text, rect, .. } if text == "A" => Some(*rect),
            _ => None,
        });
        let r = glyph.expect("labelled point emits a GlyphRun");
        // Default placement is Above: the box sits above the point (which
        // projects to the rect centre, y≈100), and is centred in x.
        assert!((r.x + r.w * 0.5 - 100.0).abs() < 5.0, "centred in x");
        assert!(r.y + r.h < 100.0, "label sits above the point");
    }

    #[test]
    fn always_point_labels_hide_when_occluded() {
        use crate::scene::glam::Vec3;
        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        // A surface right at the front occludes the origin point.
        let near = crate::scene::SceneDepthMap {
            width: 1,
            height: 1,
            depth: std::sync::Arc::from(vec![0.0_f32]),
            ..unoccluded(cam, rect)
        };
        let draw = labeled_point(crate::scene::LabelDisplay::Always);
        let mut out = Vec::new();
        push_point_labels(
            &draw,
            &cam,
            rect,
            None,
            "scene",
            0,
            1.0,
            Some(&near),
            None,
            &mut out,
        );
        assert!(out.is_empty(), "occluded point label is hidden");
    }

    #[test]
    fn hover_tooltip_picks_point_under_cursor_and_draws_a_chip() {
        use crate::scene::glam::Vec3;
        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let map = unoccluded(cam, rect);
        let draw = labeled_point(crate::scene::LabelDisplay::Hover);

        // Cursor on the point (which projects to centre) → chip + text.
        let mut hit = Vec::new();
        push_point_labels(
            &draw,
            &cam,
            rect,
            None,
            "scene",
            0,
            1.0,
            Some(&map),
            Some((100.0, 100.0)),
            &mut hit,
        );
        assert!(
            hit.iter().any(|op| matches!(op, DrawOp::Quad { .. })),
            "tooltip draws a chip background"
        );
        assert!(
            hit.iter()
                .any(|op| matches!(op, DrawOp::GlyphRun { text, .. } if text == "A")),
            "tooltip draws the label text"
        );

        // Cursor far from any point → nothing.
        let mut miss = Vec::new();
        push_point_labels(
            &draw,
            &cam,
            rect,
            None,
            "scene",
            0,
            1.0,
            Some(&map),
            Some((10.0, 10.0)),
            &mut miss,
        );
        assert!(miss.is_empty(), "no point near the cursor → no tooltip");

        // No pointer at all → nothing.
        let mut none = Vec::new();
        push_point_labels(
            &draw,
            &cam,
            rect,
            None,
            "scene",
            0,
            1.0,
            Some(&map),
            None,
            &mut none,
        );
        assert!(none.is_empty(), "no cursor → no tooltip");
    }

    #[test]
    fn hover_tooltip_skips_occluded_points() {
        use crate::scene::glam::Vec3;
        let cam = test_camera(Vec3::new(0.0, 0.0, 5.0));
        let rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let near = crate::scene::SceneDepthMap {
            width: 1,
            height: 1,
            depth: std::sync::Arc::from(vec![0.0_f32]),
            ..unoccluded(cam, rect)
        };
        let draw = labeled_point(crate::scene::LabelDisplay::Hover);
        let mut out = Vec::new();
        push_point_labels(
            &draw,
            &cam,
            rect,
            None,
            "scene",
            0,
            1.0,
            Some(&near),
            Some((100.0, 100.0)),
            &mut out,
        );
        assert!(out.is_empty(), "can't hover a point hidden behind geometry");
    }

    #[test]
    fn ghost_surface_synthesizes_state_fill_for_hover_and_press() {
        // Surfaces with no resting fill (`.ghost()`, inactive tab
        // triggers, `.outline()`) must still show interaction feedback.
        // The hover/press envelope mix is `fill.map(...)` which
        // collapses to `None` when there's nothing to lerp from, so
        // `apply_state` synthesizes a translucent ACCENT fill whose
        // alpha rises with hover and press.
        // `.ghost()` clears fill / stroke; a real tab trigger or
        // ghost button also carries a radius (the visual affordance
        // the synthesis gates on).
        let ghost = El::new(Kind::Custom("tab_trigger"))
            .ghost()
            .radius(tokens::RADIUS_SM);
        assert!(ghost.fill.is_none(), "ghost has no resting fill");

        let (rest_fill, ..) = apply_state(
            &ghost,
            InteractionState::Default,
            0.0,
            0.0,
            &Palette::damascene_dark(),
        );
        assert_eq!(rest_fill, None, "no envelope, no synthesized fill");

        let (hover_fill, ..) = apply_state(
            &ghost,
            InteractionState::Hover,
            1.0,
            0.0,
            &Palette::damascene_dark(),
        );
        let hover_alpha = tokens::STATE_FILL_HOVER_ALPHA;
        assert_eq!(
            hover_fill,
            Some(tokens::ACCENT.with_alpha(hover_alpha)),
            "hover at peak fades a faint ACCENT in",
        );

        let (press_fill, ..) = apply_state(
            &ghost,
            InteractionState::Press,
            1.0,
            1.0,
            &Palette::damascene_dark(),
        );
        let press_alpha =
            (tokens::STATE_FILL_HOVER_ALPHA + tokens::STATE_FILL_PRESS_ALPHA).clamp(0.0, 1.0);
        assert_eq!(
            press_fill,
            Some(tokens::ACCENT.with_alpha(press_alpha)),
            "press while hovered sums the two envelope contributions",
        );
    }

    #[test]
    fn hover_alpha_fades_child_with_focusable_ancestor_envelope() {
        // A non-interactive child flagged with `hover_alpha` sits below
        // a focusable container. With no interaction anywhere, the
        // child paints at `rest` * its declared alpha. When the
        // container picks up hover, the cascade through the focusable
        // ancestor's subtree-interaction envelope animates the child's
        // effective alpha to `peak`.
        use crate::layout::layout;

        let make_tree = || {
            column([row([crate::stack([El::new(Kind::Custom("badge"))
                .width(Size::Fixed(14.0))
                .height(Size::Fixed(14.0))
                .fill(tokens::FOREGROUND)
                .hover_alpha(0.25, 1.0)])
            .key("container")
            .focusable()
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(18.0))])])
            .padding(20.0)
        };

        // No hover: the child paints with alpha ≈ 0.25 * 255.
        {
            let mut tree = make_tree();
            let mut state = UiState::new();
            layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
            state.set_animation_mode(crate::state::AnimationMode::Settled);
            state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

            let ops = draw_ops(&tree, &state);
            let badge = find_quad(&ops, "badge").expect("badge quad");
            let DrawOp::Quad { uniforms, .. } = badge else {
                unreachable!()
            };
            let UniformValue::Color(fill) = uniforms.get("fill").expect("badge fill") else {
                panic!("expected color uniform");
            };
            // FOREGROUND is fully opaque in source; alpha after
            // composition should be ~0.25 (rest_opacity).
            assert!(
                (fill.a - 0.25).abs() <= 2.0 / 255.0,
                "rest opacity should hold the child near 0.25 alpha; got {}",
                fill.a,
            );
        }

        // Container hovered: the child's effective alpha rises to full.
        {
            let mut tree = make_tree();
            let mut state = UiState::new();
            layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
            let container_target = state
                .target_of_key(&tree, "container")
                .expect("container target");
            state.hovered = Some(container_target);
            state.apply_to_state();
            state.set_animation_mode(crate::state::AnimationMode::Settled);
            state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

            let ops = draw_ops(&tree, &state);
            let badge = find_quad(&ops, "badge").expect("badge quad");
            let DrawOp::Quad { uniforms, .. } = badge else {
                unreachable!()
            };
            let UniformValue::Color(fill) = uniforms.get("fill").expect("badge fill") else {
                panic!("expected color uniform");
            };
            assert_eq!(
                fill.a, 1.0,
                "ancestor hover should pull the child's alpha to full",
            );
        }
    }

    #[test]
    fn hover_alpha_keeps_child_visible_while_self_hovered() {
        // Even with no ancestor hover, a keyed focusable child
        // carrying `hover_alpha` stays visible while the cursor is
        // directly on it — the cascade carries the parent's subtree
        // envelope down, and `max(inherited, self)` saturates when
        // either side fires.
        use crate::layout::layout;

        let mut tree = column([row([crate::stack([El::new(Kind::Custom("close"))
            .key("close")
            .focusable()
            .width(Size::Fixed(14.0))
            .height(Size::Fixed(14.0))
            .fill(tokens::FOREGROUND)
            .hover_alpha(0.0, 1.0)])
        .key("container")
        .focusable()
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(18.0))])])
        .padding(20.0);

        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        // Hit-test only resolves to the deepest interactive target,
        // so cursor-on-close hovers the close, not the container.
        let close_target = state.target_of_key(&tree, "close").expect("close target");
        state.hovered = Some(close_target);
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        let ops = draw_ops(&tree, &state);
        let close = find_quad(&ops, "close").expect("close quad");
        let DrawOp::Quad { uniforms, .. } = close else {
            unreachable!()
        };
        let UniformValue::Color(fill) = uniforms.get("fill").expect("close fill") else {
            panic!("expected color uniform");
        };
        assert_eq!(
            fill.a, 1.0,
            "self-hover should keep a hover_alpha element fully visible \
             even when no ancestor is hovered",
        );
    }

    #[test]
    fn hover_alpha_does_not_affect_unmarked_descendants() {
        // Sibling control: a sibling without `hover_alpha` paints at
        // its declared alpha regardless of ancestor hover, so the
        // modifier is opt-in and doesn't bleed.
        use crate::layout::layout;

        let mut tree = column([row([crate::stack([
            El::new(Kind::Custom("tagged"))
                .width(Size::Fixed(8.0))
                .height(Size::Fixed(8.0))
                .fill(tokens::FOREGROUND)
                .hover_alpha(0.0, 1.0),
            El::new(Kind::Custom("plain"))
                .width(Size::Fixed(8.0))
                .height(Size::Fixed(8.0))
                .fill(tokens::FOREGROUND),
        ])
        .key("container")
        .focusable()
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(18.0))])])
        .padding(20.0);

        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        let ops = draw_ops(&tree, &state);
        let tagged = find_quad(&ops, "tagged").expect("tagged quad");
        let plain = find_quad(&ops, "plain").expect("plain quad");
        let DrawOp::Quad {
            uniforms: tagged_u, ..
        } = tagged
        else {
            unreachable!()
        };
        let DrawOp::Quad {
            uniforms: plain_u, ..
        } = plain
        else {
            unreachable!()
        };
        let UniformValue::Color(t) = tagged_u.get("fill").unwrap() else {
            panic!()
        };
        let UniformValue::Color(p) = plain_u.get("fill").unwrap() else {
            panic!()
        };
        assert_eq!(t.a, 0.0, "tagged child invisible at rest with rest=0");
        assert_eq!(p.a, 1.0, "unmarked sibling unaffected");
    }

    #[test]
    fn hover_alpha_stays_revealed_when_focusable_descendant_is_hovered() {
        // gh#11. A non-focusable wrapper carrying `hover_alpha` (the
        // action-pill pattern) sits between a focusable card and the
        // focusable buttons inside it. With the cursor on a button, the
        // pill must stay revealed — the cascade reads the *card's*
        // subtree envelope, which sees the hovered button as a
        // descendant.
        use crate::layout::layout;

        let mut tree = column([row([crate::stack([
            // Pill wrapper: not keyed, not focusable, but carries
            // hover_alpha. Wraps two focusable buttons.
            El::new(Kind::Custom("pill"))
                .width(Size::Fixed(80.0))
                .height(Size::Fixed(20.0))
                .fill(tokens::FOREGROUND)
                .hover_alpha(0.0, 1.0)
                .axis(crate::tree::Axis::Row),
        ])
        .key("card")
        .focusable()
        .width(Size::Fixed(160.0))
        .height(Size::Fixed(40.0))])])
        .padding(20.0);
        // Drop two focusable button keys directly under the pill.
        tree.children[0].children[0].children[0]
            .children
            .push(El::new(Kind::Custom("play")).key("play").focusable());
        tree.children[0].children[0].children[0]
            .children
            .push(El::new(Kind::Custom("more")).key("more").focusable());

        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        // Hover the focusable descendant (button), not the card or
        // the pill background. Pre-fix this caused the pill to fade
        // out: card lost hover, pill inherited 0.0, descendant button
        // didn't reach the pill via the focusable-ancestor cascade.
        let play = state.target_of_key(&tree, "play").expect("play target");
        state.hovered = Some(play);
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        let ops = draw_ops(&tree, &state);
        let pill = find_quad(&ops, "pill").expect("pill quad");
        let DrawOp::Quad { uniforms, .. } = pill else {
            unreachable!()
        };
        let UniformValue::Color(fill) = uniforms.get("fill").expect("pill fill") else {
            panic!("expected color uniform");
        };
        assert_eq!(
            fill.a, 1.0,
            "pill must stay fully revealed while a focusable descendant is hovered",
        );
    }

    #[test]
    fn hover_alpha_reveals_on_keyboard_focus_of_focusable_ancestor() {
        // gh#8. A close-× icon inside an inactive editor tab uses
        // `hover_alpha(0.0, 1.0)`. When the tab is keyboard-focused,
        // the close affordance must reveal so a keyboard-only user
        // sees that closing exists. Pre-fix `reveal_on_hover` only
        // read the hover envelope and the close stayed at α=0.
        use crate::layout::layout;

        let mut tree = column([row([crate::stack([El::new(Kind::Custom("close"))
            .key("close")
            .focusable()
            .width(Size::Fixed(14.0))
            .height(Size::Fixed(14.0))
            .fill(tokens::FOREGROUND)
            .hover_alpha(0.0, 1.0)])
        .key("tab")
        .focusable()
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(28.0))])])
        .padding(20.0);

        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let tab = state.target_of_key(&tree, "tab").expect("tab target");
        state.focused = Some(tab);
        state.focus_visible = true;
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        let ops = draw_ops(&tree, &state);
        let close = find_quad(&ops, "close").expect("close quad");
        let DrawOp::Quad { uniforms, .. } = close else {
            unreachable!()
        };
        let UniformValue::Color(fill) = uniforms.get("fill").expect("close fill") else {
            panic!("expected color uniform");
        };
        assert_eq!(
            fill.a, 1.0,
            "keyboard focus on the tab should reveal the close affordance",
        );
    }

    #[test]
    fn hover_alpha_returns_to_rest_when_subtree_loses_interaction() {
        // Inverse of #11 / #8: once the cursor leaves the surrounding
        // interaction region, the affordance fades back to `rest`.
        use crate::layout::layout;

        let mut tree = column([
            row([crate::stack([El::new(Kind::Custom("badge"))
                .width(Size::Fixed(14.0))
                .height(Size::Fixed(14.0))
                .fill(tokens::FOREGROUND)
                .hover_alpha(0.25, 1.0)])
            .key("container")
            .focusable()
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(18.0))]),
            // A second focusable that the cursor moves to. Its
            // subtree envelope rises but the badge's interaction
            // region (rooted at "container") does not.
            row([
                crate::stack([El::new(Kind::Custom("other_body")).width(Size::Fixed(80.0))])
                    .key("other")
                    .focusable()
                    .width(Size::Fixed(120.0))
                    .height(Size::Fixed(18.0)),
            ]),
        ])
        .padding(20.0);

        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let other = state.target_of_key(&tree, "other").expect("other target");
        state.hovered = Some(other);
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        let ops = draw_ops(&tree, &state);
        let badge = find_quad(&ops, "badge").expect("badge quad");
        let DrawOp::Quad { uniforms, .. } = badge else {
            unreachable!()
        };
        let UniformValue::Color(fill) = uniforms.get("fill").expect("badge fill") else {
            panic!("expected color uniform");
        };
        assert!(
            (fill.a - 0.25).abs() <= 2.0 / 255.0,
            "badge should be at rest opacity when interaction is on a sibling region; got {}",
            fill.a,
        );
    }

    fn find_quad<'a>(ops: &'a [DrawOp], id_substr: &str) -> Option<&'a DrawOp> {
        ops.iter().find(|op| op.id().contains(id_substr))
    }

    #[test]
    fn state_follows_interactive_ancestor_borrows_envelopes() {
        // A child flagged with `state_follows_interactive_ancestor` —
        // the slider thumb pattern — borrows hover and press
        // envelopes from its focusable container, since hit-test
        // never resolves to it directly.
        use crate::layout::layout;

        let mut tree = column([row([crate::stack([El::new(Kind::Custom("thumb"))
            .key("thumb")
            .width(Size::Fixed(14.0))
            .height(Size::Fixed(14.0))
            .fill(tokens::FOREGROUND)
            .radius(tokens::RADIUS_PILL)
            .state_follows_interactive_ancestor()])
        .key("container")
        .focusable()
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(18.0))])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        // Drive the container into Press by setting both `pressed` and
        // `hovered` (post-fix gating requires hover==pressed for press
        // to fire) and snap envelopes via Settled mode.
        let container_target = state
            .target_of_key(&tree, "container")
            .expect("container target");
        state.hovered = Some(container_target.clone());
        state.pressed = Some(container_target);
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        // The thumb's *own* envelopes stay zero — only the container
        // got the press. But via the cascade flag, the thumb's paint
        // sees the container's press envelope.
        let ops = draw_ops(&tree, &state);
        let thumb_op = ops
            .iter()
            .find(|op| op.id().contains("thumb"))
            .expect("thumb quad");
        let DrawOp::Quad { uniforms, .. } = thumb_op else {
            panic!("expected thumb quad");
        };
        let UniformValue::Color(thumb_fill) = uniforms.get("fill").expect("thumb fill") else {
            panic!("expected color uniform");
        };
        // Press darkens FOREGROUND by PRESS_DARKEN. Without the
        // cascade, the thumb would paint at FOREGROUND unchanged.
        let expected = tokens::FOREGROUND.mix(tokens::FOREGROUND.darken(tokens::PRESS_DARKEN), 1.0);
        assert_eq!(
            (thumb_fill.r, thumb_fill.g, thumb_fill.b),
            (expected.r, expected.g, expected.b),
            "flagged thumb borrows the container's press envelope",
        );
    }

    #[test]
    fn cross_leaf_selection_paints_a_band_on_each_spanned_leaf() {
        use crate::selection::{Selection, SelectionPoint, SelectionRange};

        let mut tree = column([
            crate::widgets::text::paragraph("First")
                .key("a")
                .selectable(),
            crate::widgets::text::paragraph("Second")
                .key("b")
                .selectable(),
            crate::widgets::text::paragraph("Third")
                .key("c")
                .selectable(),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        state.sync_selection_order(&tree);

        // anchor at byte 2 in "First", head at byte 3 in "Third":
        // span includes a partial of a, all of b, partial of c.
        state.current_selection = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("a", 2),
                head: SelectionPoint::new("c", 3),
            }),
        };

        let ops = draw_ops(&tree, &state);
        let band_ids: Vec<&str> = ops
            .iter()
            .filter_map(|op| {
                if let DrawOp::Quad { id, .. } = op
                    && id.contains("selection-band")
                {
                    Some(id.as_str())
                } else {
                    None
                }
            })
            .collect();
        // One band per spanned leaf (3 leaves: a, b, c).
        assert_eq!(
            band_ids.len(),
            3,
            "cross-leaf selection should emit a band on each of {{a, b, c}}; got {band_ids:?}"
        );
    }

    #[test]
    fn mixed_inline_math_selection_band_uses_math_rect() {
        use crate::selection::{Selection, SelectionPoint, SelectionRange, SelectionSource};

        let object = "\u{fffc}";
        let visible = format!("Inline {object} math");
        let mut source = SelectionSource::new("Inline $\\frac{a+b}{c+d}$ math", visible.clone());
        let math_start = "Inline ".len();
        let math_end = math_start + object.len();
        source.push_span(0..math_start, 0.."Inline ".len(), false);
        source.push_span(
            math_start..math_end,
            "Inline $".len()..(source.source.len() - " math".len()),
            true,
        );
        source.push_span(
            math_end..visible.len(),
            (source.source.len() - " math".len())..source.source.len(),
            false,
        );

        let expr = crate::math::parse_tex(r"\frac{a+b}{c+d}").expect("fixture TeX parses");
        let mut tree = crate::text_runs([
            crate::text("Inline "),
            crate::math_inline(expr),
            crate::text(" math"),
        ])
        .key("p")
        .selectable()
        .selection_source(source)
        .padding(20.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 500.0, 200.0));
        state.sync_selection_order(&tree);
        state.current_selection = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("p", math_start),
                head: SelectionPoint::new("p", math_end),
            }),
        };

        let ops = draw_ops(&tree, &state);
        let bands: Vec<Rect> = ops
            .iter()
            .filter_map(|op| {
                if let DrawOp::Quad { id, rect, .. } = op
                    && id.contains("selection-band")
                {
                    Some(*rect)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(bands.len(), 1, "expected one atomic math band");
        let placeholder_width =
            crate::text::metrics::line_width(object, 16.0, FontWeight::Regular, false);
        assert!(
            bands[0].w > placeholder_width * 1.5,
            "inline math selection band should cover the rendered fraction box instead of the placeholder glyph, got {:?}",
            bands[0],
        );
    }

    #[test]
    fn source_backed_mono_inlines_measure_selection_with_mono_family() {
        use crate::selection::{Selection, SelectionPoint, SelectionRange, SelectionSource};

        let visible = "iiii\nwwww";
        let mut tree = crate::text_runs([
            crate::text("iiii").mono(),
            crate::hard_break(),
            crate::text("wwww").mono(),
        ])
        .mono()
        .font_size(16.0)
        .nowrap_text()
        .key("code")
        .selectable()
        .selection_source(SelectionSource::identity(visible))
        .padding(20.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 500.0, 200.0));
        state.sync_selection_order(&tree);

        let selected_band_width = |state: &mut UiState, start: usize, end: usize| {
            state.current_selection = Selection {
                range: Some(SelectionRange {
                    anchor: SelectionPoint::new("code", start),
                    head: SelectionPoint::new("code", end),
                }),
            };
            let ops = draw_ops(&tree, state);
            let bands: Vec<Rect> = ops
                .iter()
                .filter_map(|op| {
                    if let DrawOp::Quad { id, rect, .. } = op
                        && id.contains("selection-band")
                    {
                        Some(*rect)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(bands.len(), 1, "expected one selected visual line");
            bands[0].w
        };

        let i_width = selected_band_width(&mut state, 0, 4);
        let w_width = selected_band_width(&mut state, 5, visible.len());
        assert!(
            (i_width - w_width).abs() <= 0.5,
            "mono code selection should measure equal-length lines equally; got iiii={i_width}, wwww={w_width}",
        );
    }

    #[test]
    fn source_backed_attributed_inlines_measure_selection_per_run_style() {
        use crate::selection::{Selection, SelectionPoint, SelectionRange, SelectionSource};

        let visible = "prefix iiii suffix";
        let code_start = "prefix ".len();
        let code_end = code_start + "iiii".len();
        let mut tree = crate::text_runs([
            crate::text("prefix "),
            crate::text("iiii").code(),
            crate::text(" suffix"),
        ])
        .font_size(16.0)
        .nowrap_text()
        .key("rich")
        .selectable()
        .selection_source(SelectionSource::identity(visible))
        .padding(20.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 500.0, 200.0));
        state.sync_selection_order(&tree);
        state.current_selection = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("rich", code_start),
                head: SelectionPoint::new("rich", code_end),
            }),
        };

        let ops = draw_ops(&tree, &state);
        let bands: Vec<Rect> = ops
            .iter()
            .filter_map(|op| {
                if let DrawOp::Quad { id, rect, .. } = op
                    && id.contains("selection-band")
                {
                    Some(*rect)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(bands.len(), 1, "expected one inline-code selection band");

        let regular_width = crate::text::metrics::line_width_with_family(
            "iiii",
            16.0,
            FontFamily::Inter,
            FontWeight::Regular,
            false,
        );
        let mono_width = crate::text::metrics::line_width_with_family(
            "iiii",
            16.0,
            FontFamily::Inter,
            FontWeight::Regular,
            true,
        );
        assert!(
            (bands[0].w - mono_width).abs() <= 0.75,
            "inline-code selection should use mono run width; band={:?}, mono={mono_width}, regular={regular_width}",
            bands[0],
        );
        assert!(
            bands[0].w > regular_width * 1.5,
            "regression guard: measuring the code run as regular text would be visibly too short"
        );
    }

    #[test]
    fn drag_select_through_runtime_paints_band_in_next_frame() {
        // End-to-end: simulate pointer_down + pointer_moved on a
        // selectable paragraph, then drive a fresh `prepare_layout`
        // and verify the band is in the resulting DrawOps. Catches
        // regressions where the runtime's per-frame updates would
        // overwrite the live selection or where the painter doesn't
        // see the manager's writes.
        use crate::event::{Pointer, PointerButton};
        use crate::runtime::{PrepareTimings, RunnerCore};

        let mut core = RunnerCore::new();
        let mut tree = column([crate::widgets::text::paragraph("Hello, world!")
            .key("p")
            .selectable()])
        .padding(20.0);
        let viewport = Rect::new(0.0, 0.0, 400.0, 200.0);
        // First prepare_layout populates the selection_order, etc.
        let mut t = PrepareTimings::default();
        let _ = core.prepare_layout(
            &mut tree,
            viewport,
            1.0,
            &mut t,
            RunnerCore::no_time_shaders,
        );
        // Snapshot so pointer events can hit-test against this frame.
        core.snapshot(&tree, &mut t);

        let p_rect = core.rect_of_key("p").expect("p rect");
        let cy = p_rect.y + p_rect.h * 0.5;
        let _ = core.pointer_down(Pointer::mouse(p_rect.x + 4.0, cy, PointerButton::Primary));
        // Drag to extend.
        let _ = core.pointer_moved(Pointer::moving(p_rect.x + p_rect.w - 8.0, cy));

        // Selection in UiState must be a non-collapsed range now.
        let sel = &core.ui_state.current_selection;
        let r = sel.range.as_ref().expect("selection set");
        assert!(
            r.anchor.byte != r.head.byte,
            "drag should extend head past anchor (anchor={}, head={})",
            r.anchor.byte,
            r.head.byte
        );

        // Re-run prepare_layout (the per-frame loop). The painter
        // should emit a selection band Quad on this frame.
        let mut t2 = PrepareTimings::default();
        let crate::runtime::LayoutPrepared { ops, .. } = core.prepare_layout(
            &mut tree,
            viewport,
            1.0,
            &mut t2,
            RunnerCore::no_time_shaders,
        );
        let bands: Vec<&DrawOp> = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("selection-band")))
            .collect();
        assert!(
            !bands.is_empty(),
            "after drag-select, prepare_layout should emit a selection band Quad"
        );
        // Verify the band's painted rect overlaps the leaf's painted
        // rect — otherwise the highlight is rendered, but off-screen.
        if let DrawOp::Quad { rect, .. } = bands[0] {
            assert!(
                rect.intersect(p_rect).is_some(),
                "band rect = {rect:?} doesn't overlap leaf rect = {p_rect:?}"
            );
        }
    }

    #[test]
    fn selectable_leaf_paints_selection_band_when_key_matches_active_selection() {
        use crate::selection::{Selection, SelectionPoint, SelectionRange};

        let mut tree = column([crate::widgets::text::paragraph("Hello, world!")
            .key("p")
            .selectable()])
        .padding(20.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        // Pre-painter sanity: no current selection → no band.
        let ops_pre = draw_ops(&tree, &state);
        let bands_pre = ops_pre
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("selection-band")))
            .count();
        assert_eq!(bands_pre, 0, "no band should paint when selection is empty");

        state.current_selection = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("p", 0),
                head: SelectionPoint::new("p", 5),
            }),
        };
        let ops = draw_ops(&tree, &state);
        let bands: Vec<&DrawOp> = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("selection-band")))
            .collect();
        assert!(
            !bands.is_empty(),
            "selection range over keyed selectable leaf should emit at least one band Quad"
        );
        if let DrawOp::Quad { rect, .. } = bands[0] {
            // Band must overlap the leaf's painted rect (positive area).
            assert!(rect.w > 0.0 && rect.h > 0.0, "band rect = {rect:?}");
        }
    }

    #[test]
    fn layout_only_focusable_container_does_not_synthesize_fill() {
        // The outer wrappers of `slider`, `switch`, and `resize_handle`
        // are `focusable` `stack(...)`s with no fill, no radius, and no
        // stroke — they exist purely to capture pointer/keyboard events
        // for the visible children below. Synthesizing a state fill
        // here would paint a translucent rectangle across the widget's
        // hit area on hover / press, competing with the actual thumb /
        // track / hairline. Gate the synthesis on the node having some
        // surface affordance of its own.
        let layout_only = El::new(Kind::Custom("slider")).focusable();
        assert!(layout_only.fill.is_none());
        assert_eq!(layout_only.radius, crate::tree::Corners::ZERO);
        assert!(layout_only.stroke.is_none());

        let (rest_fill, ..) = apply_state(
            &layout_only,
            InteractionState::Default,
            0.0,
            0.0,
            &Palette::damascene_dark(),
        );
        let (hover_fill, ..) = apply_state(
            &layout_only,
            InteractionState::Hover,
            1.0,
            0.0,
            &Palette::damascene_dark(),
        );
        let (press_fill, ..) = apply_state(
            &layout_only,
            InteractionState::Press,
            1.0,
            1.0,
            &Palette::damascene_dark(),
        );
        assert_eq!(rest_fill, None);
        assert_eq!(hover_fill, None);
        assert_eq!(press_fill, None);
    }

    #[test]
    fn solid_surface_keeps_envelope_mix_unchanged() {
        // Surfaces with a resting fill still go through the existing
        // lighten/darken envelope mix — the synthesized state fill only
        // kicks in when the resting fill is None.
        let solid = El::new(Kind::Custom("button")).fill(tokens::MUTED);
        let (rest_fill, ..) = apply_state(
            &solid,
            InteractionState::Default,
            0.0,
            0.0,
            &Palette::damascene_dark(),
        );
        assert_eq!(rest_fill, Some(tokens::MUTED));

        let (hover_fill, ..) = apply_state(
            &solid,
            InteractionState::Hover,
            1.0,
            0.0,
            &Palette::damascene_dark(),
        );
        assert_eq!(
            hover_fill,
            Some(tokens::MUTED.mix(tokens::MUTED.lighten(tokens::HOVER_LIGHTEN), 1.0)),
            "solid surfaces lighten existing fill, not synthesize a new one",
        );
    }

    #[test]
    fn state_envelope_composes_against_active_palette() {
        // Hover/press lighten/darken must compose against the active
        // palette's rgb, not the token's compile-time dark fallback —
        // otherwise hover visuals are dark-derived even in light mode.
        let solid = El::new(Kind::Custom("button")).fill(tokens::MUTED);
        let light = Palette::damascene_light();
        let (hover_fill, ..) = apply_state(&solid, InteractionState::Hover, 1.0, 0.0, &light);
        let expected = light
            .muted
            .mix(light.muted.lighten(tokens::HOVER_LIGHTEN), 1.0);
        assert_eq!(
            hover_fill,
            Some(expected),
            "hover lighten composes against the active palette",
        );
    }

    #[test]
    fn clip_sets_scissor_on_descendant_ops() {
        let mut root = column([row([
            button("Inside").key("inside"),
            button("Too wide").key("outside").width(Size::Fixed(300.0)),
        ])
        .clip()
        .width(Size::Fixed(120.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));

        let ops = draw_ops(&root, &state);
        let clipped = ops
            .iter()
            .find(|op| op.id().contains("outside"))
            .expect("outside button op");
        let DrawOp::Quad { scissor, .. } = clipped else {
            panic!("expected button surface quad");
        };
        assert_eq!(*scissor, Some(Rect::new(0.0, 0.0, 120.0, 32.0)));
    }

    #[test]
    fn draw_ops_culls_text_fully_outside_inherited_clip() {
        let clipped = column([
            crate::widgets::text::text("visible").key("visible"),
            crate::tree::spacer().height(Size::Fixed(40.0)),
            crate::widgets::text::text("offscreen").key("offscreen"),
        ])
        .clip()
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(24.0));
        let mut root = column([clipped]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 240.0, 80.0));

        let mut stats = DrawOpsStats::default();
        let ops = draw_ops_with_theme_and_stats(&root, &state, &Theme::default(), &mut stats);

        assert_eq!(stats.culled_text_ops, 1);
        assert!(
            ops.iter().any(|op| op.id().contains("visible")),
            "visible text still emits a draw op"
        );
        assert!(
            !ops.iter().any(|op| op.id().contains("offscreen")),
            "fully clipped text should not reach draw ops"
        );
    }

    #[test]
    fn inline_text_bg_propagates_to_run_style() {
        // text_runs([..text("hit").background(...)..]) flows into the
        // Inlines collector and lands on the per-run RunStyle.bg of
        // the AttributedText draw op. Other runs keep `bg: None`.
        let highlight = Color::srgb_u8(220, 200, 60);
        let mut root = crate::text_runs([
            crate::text("plain "),
            crate::text("marked").background(highlight),
            crate::text(" rest"),
        ]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 320.0, 80.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::AttributedText { runs, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::AttributedText { .. }))
            .expect("attr op")
        else {
            unreachable!()
        };
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].1.bg, None);
        assert_eq!(runs[1].1.bg, Some(highlight));
        assert_eq!(runs[2].1.bg, None);
    }

    #[test]
    fn text_align_center_emits_middle_anchor() {
        let mut root = crate::text("Centered").center_text();
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 80.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::GlyphRun { anchor, .. } = &ops[0] else {
            panic!("expected glyph run");
        };
        assert_eq!(*anchor, TextAnchor::Middle);
    }

    #[test]
    fn paragraph_emits_wrapped_glyph_run() {
        let mut root = crate::paragraph("This sentence should wrap in a narrow box.")
            .width(Size::Fixed(120.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 120.0, 120.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::GlyphRun { wrap, .. } = &ops[0] else {
            panic!("expected glyph run");
        };
        assert_eq!(*wrap, TextWrap::Wrap);
    }

    #[test]
    fn inline_math_batches_same_style_text_runs() {
        let expr = crate::math::parse_tex("x_1+x_2").expect("valid tex");
        let mut root = crate::text_runs([
            crate::text("Alpha beta gamma "),
            crate::math_inline(expr),
            crate::text(" delta epsilon"),
        ])
        .width(Size::Fixed(600.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 80.0));

        let ops = draw_ops(&root, &state);
        let inline_runs: Vec<(&str, Rect)> = ops
            .iter()
            .filter_map(|op| {
                let DrawOp::GlyphRun { id, text, rect, .. } = op else {
                    return None;
                };
                if id.contains(".inline-text.") {
                    return Some((text.as_str(), *rect));
                }
                None
            })
            .collect();

        assert_eq!(inline_runs.len(), 2);
        assert_eq!(inline_runs[0].0, "Alpha beta gamma ");
        assert_eq!(inline_runs[1].0, "delta epsilon");
        assert!(
            inline_runs[1].1.x > inline_runs[0].1.right(),
            "post-math text keeps the leading-space advance without painting a separate space run"
        );
    }

    #[test]
    fn inline_math_uses_line_ascent_for_mixed_baseline() {
        let expr = crate::math::parse_tex(r"\frac{a+b}{c+d}").expect("valid tex");
        let mut root = crate::text_runs([
            crate::text("Before "),
            crate::math_inline(expr),
            crate::text(" after"),
        ])
        .width(Size::Fixed(600.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 120.0));

        let ops = draw_ops(&root, &state);
        let min_math_y = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::GlyphRun { id, rect, .. } if id.contains(".math-glyph.") => Some(rect.y),
                DrawOp::Quad { id, rect, .. } if id.contains(".math-rule.") => Some(rect.y),
                DrawOp::Vector { id, rect, .. } if id.contains(".math-") => Some(rect.y),
                _ => None,
            })
            .fold(f32::INFINITY, f32::min);

        assert!(
            min_math_y >= -3.0,
            "built-up inline math should sit inside the line box, min y = {min_math_y}"
        );
    }

    #[test]
    fn mixed_inline_wrap_paint_stays_inside_layout_height() {
        let expr = crate::math::parse_tex(r"\frac{a+b}{c+d}").expect("valid tex");
        let mut root = crate::text_runs([
            crate::text("Alpha beta "),
            crate::math_inline(expr),
            crate::text(" after wrap"),
        ])
        .width(Size::Fixed(116.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 116.0, 200.0));

        let root_rect = state
            .layout
            .computed_rects
            .get(&root.computed_id)
            .copied()
            .expect("root rect");
        let ops = draw_ops(&root, &state);
        let paint_bounds = mixed_inline_paint_bounds(&ops).expect("mixed inline paint bounds");

        assert!(
            paint_bounds.bottom() <= root_rect.bottom() + 3.0,
            "paint bounds {paint_bounds:?} should fit layout rect {root_rect:?}"
        );
    }

    #[test]
    fn mixed_inline_hard_break_paint_stays_inside_layout_height() {
        let expr = crate::math::parse_tex(r"\frac{a+b}{c+d}").expect("valid tex");
        let mut root = crate::text_runs([
            crate::text("Before "),
            crate::math_inline(expr),
            crate::hard_break(),
            crate::text("after"),
        ])
        .width(Size::Fixed(400.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let root_rect = state
            .layout
            .computed_rects
            .get(&root.computed_id)
            .copied()
            .expect("root rect");
        let ops = draw_ops(&root, &state);
        let paint_bounds = mixed_inline_paint_bounds(&ops).expect("mixed inline paint bounds");

        assert!(
            paint_bounds.bottom() <= root_rect.bottom() + 3.0,
            "paint bounds {paint_bounds:?} should fit layout rect {root_rect:?}"
        );
    }

    fn mixed_inline_paint_bounds(ops: &[DrawOp]) -> Option<Rect> {
        let mut bounds: Option<Rect> = None;
        for op in ops {
            let candidate = match op {
                DrawOp::GlyphRun { id, rect, .. }
                    if id.contains(".inline-text.") || id.contains(".math-glyph.") =>
                {
                    Some(*rect)
                }
                DrawOp::Quad { id, rect, .. } if id.contains(".math-rule.") => Some(*rect),
                DrawOp::Vector { id, rect, .. } if id.contains(".math-") => Some(*rect),
                _ => None,
            };
            if let Some(rect) = candidate {
                bounds = Some(match bounds {
                    Some(prev) => union_rect(prev, rect),
                    None => rect,
                });
            }
        }
        bounds
    }

    fn union_rect(a: Rect, b: Rect) -> Rect {
        let left = a.x.min(b.x);
        let top = a.y.min(b.y);
        let right = a.right().max(b.right());
        let bottom = a.bottom().max(b.bottom());
        Rect::new(left, top, right - left, bottom - top)
    }

    #[test]
    fn padding_on_text_node_insets_glyph_rect() {
        // Regression: `text("X").padding(...)` used to inflate the
        // node's intrinsic size but emit the GlyphRun against the full
        // (uninset) layout rect, so glyphs anchored at the parent's
        // edge whenever Stretch flattened the Hug width. The fix is
        // for draw_ops to inset the glyph rect by `node.padding`,
        // making text padding behave the same as container padding.
        let mut root = column([crate::text("Chat").padding(Sides::xy(12.0, 8.0))])
            .width(Size::Fixed(320.0))
            .height(Size::Fill(1.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 320.0, 600.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::GlyphRun { rect, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::GlyphRun { .. }))
            .expect("text node emits a glyph run")
        else {
            unreachable!()
        };
        // Column stretched the text element to 320×(text_height + 16);
        // the glyph rect should be inset by the padding on each side.
        assert!(
            (rect.x - 12.0).abs() < 1e-3,
            "glyph rect.x = {}, expected 12 (left padding)",
            rect.x,
        );
        assert!(
            (rect.w - (320.0 - 24.0)).abs() < 1e-3,
            "glyph rect.w = {}, expected 296 (320 minus 12+12)",
            rect.w,
        );
        assert!(
            (rect.y - 8.0).abs() < 1e-3,
            "glyph rect.y = {}, expected 8 (top padding)",
            rect.y,
        );
    }

    #[test]
    fn padding_on_icon_node_insets_icon_rect() {
        // Same fix applies to icon nodes: the centered icon should
        // center in the inset rect, not the full layout rect. Override
        // the Fixed width that `icon_size(...)` sets so the padding
        // has room — without the override, padding(20) on a 16-wide
        // element would produce a negative inset.
        let mut root = column([crate::icon(IconName::Folder)
            .icon_size(crate::tokens::ICON_SM)
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))
            .padding(Sides::xy(20.0, 0.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::Icon { rect, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Icon { .. }))
            .expect("icon node emits an icon op")
        else {
            unreachable!()
        };
        // Element 80×40, inner after Sides::xy(20, 0) → (20, 0, 40, 40),
        // inner.center_x() = 40, 16px icon → x = 32.
        assert!(
            (rect.x - 32.0).abs() < 1e-3,
            "icon rect.x = {}, expected 32 (centered in inset rect)",
            rect.x,
        );
    }

    #[test]
    fn image_intrinsic_is_natural_pixel_size() {
        let pixels = vec![0u8; 80 * 40 * 4];
        let img = crate::image::Image::from_rgba8(80, 40, pixels);
        let el = crate::tree::image(img);
        let (w, h) = crate::layout::intrinsic(&el);
        assert!((w - 80.0).abs() < 1e-3, "intrinsic w = {w}");
        assert!((h - 40.0).abs() < 1e-3, "intrinsic h = {h}");
    }

    #[test]
    fn image_emits_draw_op_with_fit_projection() {
        // 100×50 image into a 400×400 box with Cover: dest = 800×400.
        let pixels = vec![0u8; 100 * 50 * 4];
        let img = crate::image::Image::from_rgba8(100, 50, pixels);
        let mut root = crate::row([crate::tree::image(img)
            .image_fit(crate::image::ImageFit::Cover)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(400.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 600.0));
        let ops = draw_ops(&root, &state);
        let img_op = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Image { .. }))
            .expect("image El emits a DrawOp::Image");
        let DrawOp::Image {
            rect, scissor, fit, ..
        } = img_op
        else {
            unreachable!()
        };
        assert_eq!(*fit, crate::image::ImageFit::Cover);
        // Cover scale = max(400/100, 400/50) = 8 → 800×400 dest.
        assert!((rect.w - 800.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 400.0).abs() < 1e-3, "rect.h = {}", rect.h);
        // Scissor clamps to content (400×400 box) so the horizontal
        // overflow is cropped without an explicit `.clip()`.
        let s = scissor.expect("image draw op carries a scissor");
        assert!((s.w - 400.0).abs() < 1e-3, "scissor.w = {}", s.w);
        assert!((s.h - 400.0).abs() < 1e-3, "scissor.h = {}", s.h);
    }

    #[test]
    fn image_fully_outside_inherited_clip_emits_zero_scissor_not_none() {
        // Regression: an image El whose computed rect falls fully
        // outside an ancestor `clip()` must not paint past the clip.
        // The previous open-coded `s.intersect(inner)` returned `None`
        // when the rects didn't overlap, and `scissor: None` is
        // interpreted downstream as "no scissor" — so the image
        // painted full-bleed against the framebuffer instead of being
        // dropped. The fix routes through `intersect_scissor`, which
        // hands back `Some(Rect::zero)` and lets the renderer skip
        // the draw via its `phys.w == 0 || phys.h == 0` guard.
        //
        // Repro: a clipped row whose first child (Fixed 150) pushes
        // the second image child entirely past the row's right edge.
        let pixels = vec![0u8; 10 * 10 * 4];
        let img = crate::image::Image::from_rgba8(10, 10, pixels);
        // Wrap the clipped row in a column so the layout entry point
        // doesn't paste the viewport rect onto the row itself —
        // `layout()` forces the root rect to the viewport regardless
        // of the El's stated width/height, which collapses the
        // overflow we want to repro.
        let mut root = crate::column([crate::row([
            crate::column(Vec::<El>::new())
                .width(Size::Fixed(150.0))
                .height(Size::Fixed(50.0)),
            crate::tree::image(img)
                .width(Size::Fixed(60.0))
                .height(Size::Fixed(50.0)),
        ])
        .width(Size::Fixed(100.0))
        .height(Size::Fixed(100.0))
        .clip()]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::Image { scissor, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Image { .. }))
            .expect("image El still emits a DrawOp::Image when fully clipped")
        else {
            unreachable!()
        };
        let s = scissor.expect(
            "scissor must be Some(_) so the renderer drops the draw — \
             None would let it paint past the ancestor clip",
        );
        assert!(
            s.w <= 0.0 || s.h <= 0.0,
            "image fully outside ancestor clip must yield a zero-sized scissor, got {s:?}",
        );
    }

    #[test]
    fn image_tint_propagates_with_opacity() {
        let pixels = vec![0u8; 4 * 4 * 4];
        let img = crate::image::Image::from_rgba8(4, 4, pixels);
        let mut root = crate::tree::image(img)
            .image_tint(Color::srgb_u8(200, 100, 50))
            .opacity(0.5);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::Image { tint, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Image { .. }))
            .expect("image emits draw op")
        else {
            unreachable!()
        };
        let tint = tint.expect("image_tint set, draw op carries tint");
        // Opacity halves the alpha channel of the tint (255 → 128).
        let [tr, tg, tb, ta] = tint.to_srgb_u8a();
        assert_eq!(ta, 128, "tint.a after 0.5 opacity = {ta}");
        assert_eq!((tr, tg, tb), (200, 100, 50));
    }

    /// Stub backend used by the surface-emission test. Nothing
    /// inspects the texture at this layer, so the impl is minimal.
    #[derive(Debug)]
    struct StubAppTextureBackend {
        id: crate::surface::AppTextureId,
        size: (u32, u32),
    }

    impl crate::surface::AppTextureBackend for StubAppTextureBackend {
        fn id(&self) -> crate::surface::AppTextureId {
            self.id
        }
        fn size_px(&self) -> (u32, u32) {
            self.size
        }
        fn format(&self) -> crate::surface::SurfaceFormat {
            crate::surface::SurfaceFormat::Rgba8UnormSrgb
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn stub_app_texture(w: u32, h: u32) -> crate::surface::AppTexture {
        crate::surface::AppTexture::from_backend(std::sync::Arc::new(StubAppTextureBackend {
            id: crate::surface::next_app_texture_id(),
            size: (w, h),
        }))
    }

    #[test]
    fn surface_emits_app_texture_op_filling_rect() {
        let tex = stub_app_texture(64, 32);
        let mut root = crate::row([crate::tree::surface(tex)
            .width(Size::Fixed(200.0))
            .height(Size::Fixed(100.0))
            .surface_alpha(crate::surface::SurfaceAlpha::Opaque)]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let ops = draw_ops(&root, &state);
        let surf_op = ops
            .iter()
            .find(|op| matches!(op, DrawOp::AppTexture { .. }))
            .expect("Kind::Surface emits a DrawOp::AppTexture");
        let DrawOp::AppTexture {
            rect,
            scissor,
            alpha,
            fit,
            transform,
            ..
        } = surf_op
        else {
            unreachable!()
        };
        // Default surface_fit is Fill — rect matches the content rect 1:1.
        assert_eq!(*fit, crate::image::ImageFit::Fill);
        assert!((rect.w - 200.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 100.0).abs() < 1e-3, "rect.h = {}", rect.h);
        // Default surface_transform is identity.
        assert!(transform.is_identity());
        // Auto-clip applies regardless of `.clip()`.
        let s = scissor.expect("surface op carries a scissor");
        assert!((s.w - 200.0).abs() < 1e-3, "scissor.w = {}", s.w);
        assert_eq!(*alpha, crate::surface::SurfaceAlpha::Opaque);
    }

    #[test]
    fn chart3d_emits_scene3d_op_with_marks_and_framed_camera() {
        use crate::scene::{PointData, PointsHandle, ScenePoint, SceneSpec};
        use glam::Vec3;

        let pts = PointsHandle::new(PointData {
            points: vec![
                ScenePoint {
                    position: Vec3::splat(-1.0),
                    color: [1.0; 4],
                },
                ScenePoint {
                    position: Vec3::splat(1.0),
                    color: [1.0; 4],
                },
            ],
        });
        let mut root = crate::row([crate::chart3d(SceneSpec::new().points(pts))
            .width(Size::Fixed(200.0))
            .height(Size::Fixed(120.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::Scene3D {
            rect,
            scissor,
            scene,
            ..
        } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Scene3D { .. }))
            .expect("Kind::Scene3D emits a DrawOp::Scene3D")
        else {
            unreachable!()
        };
        assert_eq!(scene.points.len(), 1);
        assert!(scene.meshes.is_empty() && scene.lines.is_empty());
        assert!((rect.w - 200.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 120.0).abs() < 1e-3, "rect.h = {}", rect.h);
        // Camera auto-framed the scene: eye offset from target, sane planes.
        assert!((scene.camera.eye - scene.camera.target).length() > 0.0);
        assert!(scene.camera.near > 0.0 && scene.camera.far > scene.camera.near);
        // Auto-clip scissor applies like any leaf.
        assert!(scissor.is_some());
    }

    #[test]
    fn surface_fit_contain_letterboxes_aspect_mismatch() {
        // 100×50 texture (2:1) into a 400×400 box with Contain →
        // dest = 400×200 centred vertically.
        let tex = stub_app_texture(100, 50);
        let mut root = crate::row([crate::tree::surface(tex)
            .surface_fit(crate::image::ImageFit::Contain)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(400.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 600.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::AppTexture {
            rect, scissor, fit, ..
        } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::AppTexture { .. }))
            .expect("surface emits a DrawOp::AppTexture")
        else {
            unreachable!()
        };
        assert_eq!(*fit, crate::image::ImageFit::Contain);
        assert!((rect.w - 400.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 200.0).abs() < 1e-3, "rect.h = {}", rect.h);
        // Scissor still clamps to the 400×400 content rect.
        let s = scissor.expect("surface op carries a scissor");
        assert!((s.h - 400.0).abs() < 1e-3, "scissor.h = {}", s.h);
    }

    #[test]
    fn surface_fit_cover_overflows_rect_with_scissor_clamp() {
        // 100×50 texture into a 400×400 box with Cover → dest = 800×400
        // (overflowing horizontally). Scissor clamps to 400×400.
        let tex = stub_app_texture(100, 50);
        let mut root = crate::row([crate::tree::surface(tex)
            .surface_fit(crate::image::ImageFit::Cover)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(400.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 600.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::AppTexture {
            rect, scissor, fit, ..
        } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::AppTexture { .. }))
            .expect("surface emits a DrawOp::AppTexture")
        else {
            unreachable!()
        };
        assert_eq!(*fit, crate::image::ImageFit::Cover);
        assert!((rect.w - 800.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 400.0).abs() < 1e-3, "rect.h = {}", rect.h);
        let s = scissor.expect("surface op carries a scissor");
        assert!((s.w - 400.0).abs() < 1e-3, "scissor.w = {}", s.w);
    }

    #[test]
    fn surface_transform_propagates_through_to_draw_op() {
        let tex = stub_app_texture(64, 32);
        let m = crate::affine::Affine2::rotate(0.5);
        let mut root = crate::row([crate::tree::surface(tex)
            .surface_transform(m)
            .width(Size::Fixed(200.0))
            .height(Size::Fixed(100.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::AppTexture { transform, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::AppTexture { .. }))
            .expect("surface emits a DrawOp::AppTexture")
        else {
            unreachable!()
        };
        assert_eq!(*transform, m);
    }

    #[test]
    fn vector_emits_draw_op_carrying_asset() {
        use crate::vector::{PathBuilder, VectorAsset};
        let curve = PathBuilder::new()
            .move_to(0.0, 0.0)
            .cubic_to(20.0, 0.0, 0.0, 60.0, 20.0, 60.0)
            .stroke_solid(Color::srgb_u8(80, 200, 240), 2.0)
            .build();
        let asset = VectorAsset::from_paths([0.0, 0.0, 20.0, 60.0], vec![curve]);
        let expected_hash = asset.content_hash();
        let mut root = crate::row([crate::tree::vector(asset)
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(120.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let ops = draw_ops(&root, &state);
        let op = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Vector { .. }))
            .expect("Kind::Vector emits a DrawOp::Vector");
        let DrawOp::Vector {
            rect,
            scissor,
            asset,
            render_mode,
            ..
        } = op
        else {
            unreachable!()
        };
        // Widget's resolved rect drives paint, not the asset's view box.
        assert!((rect.w - 40.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 120.0).abs() < 1e-3, "rect.h = {}", rect.h);
        // Auto-clip applies.
        let s = scissor.expect("vector op carries a scissor");
        assert!((s.w - 40.0).abs() < 1e-3, "scissor.w = {}", s.w);
        // Content hash round-trips through Arc into the op.
        assert_eq!(asset.content_hash(), expected_hash);
        assert_eq!(
            *render_mode,
            crate::vector::VectorRenderMode::Painted,
            "app vectors default to painted rendering"
        );
        // The asset's first segment is preserved (sanity-check that the
        // PathBuilder fed through correctly).
        let first_seg = asset.paths[0].segments.first().copied();
        assert_eq!(
            first_seg,
            Some(crate::vector::VectorSegment::MoveTo([0.0, 0.0]))
        );
    }

    #[test]
    fn vector_asset_colors_resolve_against_active_palette() {
        use crate::vector::{PathBuilder, VectorAsset, VectorColor};

        let path = PathBuilder::new()
            .move_to(0.0, 0.0)
            .line_to(10.0, 10.0)
            .stroke_solid(tokens::PRIMARY, 1.0)
            .build();
        let mut root =
            crate::tree::vector(VectorAsset::from_paths([0.0, 0.0, 10.0, 10.0], vec![path]));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        let ops = draw_ops_with_theme(&root, &state, &Theme::damascene_light());
        let DrawOp::Vector { asset, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Vector { .. }))
            .expect("vector op")
        else {
            unreachable!()
        };
        let stroke = asset.paths[0].stroke.expect("stroke");
        assert_eq!(
            stroke.color,
            VectorColor::Solid(crate::Palette::damascene_light().primary),
            "vector token colors should resolve through the active palette"
        );
    }

    #[test]
    fn vector_mask_mode_resolves_mask_color_against_active_palette() {
        use crate::vector::{PathBuilder, VectorAsset, VectorRenderMode};

        let path = PathBuilder::new()
            .move_to(0.0, 0.0)
            .line_to(10.0, 10.0)
            .stroke_solid(Color::srgb_u8(1, 2, 3), 1.0)
            .build();
        let mut root =
            crate::tree::vector(VectorAsset::from_paths([0.0, 0.0, 10.0, 10.0], vec![path]))
                .vector_mask(tokens::PRIMARY);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        let ops = draw_ops_with_theme(&root, &state, &Theme::damascene_light());
        let DrawOp::Vector { render_mode, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Vector { .. }))
            .expect("vector op")
        else {
            unreachable!()
        };
        assert_eq!(
            *render_mode,
            VectorRenderMode::Mask {
                color: crate::Palette::damascene_light().primary
            }
        );
    }

    #[test]
    fn math_exact_glyph_assets_are_normalized_before_msdf_rasterization() {
        let face = ttf_parser::Face::parse(damascene_fonts::NOTO_SANS_MATH_REGULAR, 0).unwrap();
        let glyph_id = face.glyph_index('√').expect("math radical glyph").0;
        let asset = math_glyph_vector_asset(glyph_id, Rect::new(-64.0, -3200.0, 1280.0, 4096.0))
            .expect("math glyph vector asset");

        assert!(
            asset.view_box[2].max(asset.view_box[3]) <= 24.001,
            "font-unit view box should be normalized before hitting the icon MSDF path: {:?}",
            asset.view_box
        );

        let mut atlas = crate::icons::msdf_atlas::IconMsdfAtlas::default();
        let slot = atlas
            .ensure_vector_asset(&asset)
            .expect("normalized glyph should rasterize");
        assert!(
            slot.rect.w <= 80 && slot.rect.h <= 80,
            "normalized math glyph should produce icon-sized MSDFs, got {:?}",
            slot.rect
        );
    }

    #[test]
    fn vector_asset_content_hash_is_stable_and_distinguishing() {
        use crate::vector::{PathBuilder, VectorAsset};
        let make = |sx: f32| {
            let p = PathBuilder::new()
                .move_to(0.0, 0.0)
                .line_to(sx, 1.0)
                .stroke_solid(Color::srgb_u8(0, 0, 0), 1.0)
                .build();
            VectorAsset::from_paths([0.0, 0.0, 10.0, 10.0], vec![p])
        };
        // Same inputs → same hash, across repeated builds.
        assert_eq!(make(1.0).content_hash(), make(1.0).content_hash());
        // Different geometry → different hash.
        assert_ne!(make(1.0).content_hash(), make(2.0).content_hash());
    }

    #[test]
    fn opacity_multiplies_alpha_on_quad_uniforms() {
        let mut root = button("X")
            .fill(Color::srgb_u8a(200, 100, 50, 200))
            .opacity(0.5);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::Quad { uniforms, .. } = &ops[0] else {
            panic!("expected quad op");
        };
        let UniformValue::Color(c) = uniforms.get("fill").expect("fill") else {
            panic!("fill should be a colour");
        };
        // 200 * 0.5 = 100
        assert_eq!(
            c.to_srgb_u8a()[3],
            100,
            "alpha should be halved by opacity 0.5"
        );
    }

    #[test]
    fn theme_can_route_implicit_surfaces_to_custom_shader() {
        let mut root = button("X").primary();
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

        let theme = Theme::default()
            .with_surface_shader("xp_surface")
            .with_surface_uniform("theme_strength", UniformValue::F32(0.75));
        let ops = draw_ops_with_theme(&root, &state, &theme);
        let DrawOp::Quad {
            shader, uniforms, ..
        } = &ops[0]
        else {
            panic!("expected themed surface quad");
        };

        assert_eq!(*shader, ShaderHandle::Custom("xp_surface"));
        assert_eq!(
            uniforms.get("theme_strength"),
            Some(&UniformValue::F32(0.75))
        );
        assert!(
            matches!(uniforms.get("fill"), Some(UniformValue::Color(_))),
            "familiar rounded-rect uniforms should stay available for manifests"
        );
        assert!(
            matches!(uniforms.get("vec_a"), Some(UniformValue::Color(_))),
            "custom surface shaders should also receive packed instance slots"
        );
        assert_eq!(
            uniforms.get("vec_c"),
            Some(&UniformValue::Vec4([
                1.0,
                tokens::RADIUS_MD,
                tokens::SHADOW_SM * 0.5,
                0.0
            ]))
        );
    }

    #[test]
    fn theme_can_route_surface_role_to_custom_shader() {
        let mut root = crate::titled_card("Panel", [crate::text("Body")])
            .surface_role(SurfaceRole::Popover)
            .key("panel");
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 240.0, 120.0));

        let theme = Theme::default()
            .with_role_shader(SurfaceRole::Popover, "popover_surface")
            .with_role_uniform(SurfaceRole::Popover, "elevation", UniformValue::F32(2.0));
        let ops = draw_ops_with_theme(&root, &state, &theme);
        let DrawOp::Quad {
            shader, uniforms, ..
        } = &ops[0]
        else {
            panic!("expected themed surface quad");
        };

        assert_eq!(*shader, ShaderHandle::Custom("popover_surface"));
        assert_eq!(uniforms.get("elevation"), Some(&UniformValue::F32(2.0)));
        assert_eq!(
            uniforms.get("surface_role"),
            Some(&UniformValue::F32(SurfaceRole::Popover.uniform_id()))
        );
        assert!(
            matches!(uniforms.get("vec_a"), Some(UniformValue::Color(_))),
            "role-routed custom shaders should receive packed rect slots"
        );
        assert_eq!(
            uniforms.get("vec_c"),
            Some(&UniformValue::Vec4([
                1.0,
                tokens::RADIUS_LG,
                tokens::SHADOW_LG,
                0.0
            ]))
        );
    }

    #[test]
    fn translate_offsets_paint_rect_and_inherits_to_children() {
        // Parent translate of (50, 30) should land child rects at
        // child.computed + (50, 30). The button widget uses
        // `paint_overflow` for its focus ring, which grows the painted
        // rect outward — so we compare against the `inner_rect` uniform
        // (the post-translate layout rect) rather than the raw quad rect.
        let mut root = column([button("X").key("x")]).translate(50.0, 30.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let inner = inner_rect_quad_for(&root, &state, "x").expect("x quad inner_rect");
        let untranslated = find_computed(&root, &state, "x").expect("x computed");

        assert!((inner.x - (untranslated.x + 50.0)).abs() < 0.5);
        assert!((inner.y - (untranslated.y + 30.0)).abs() < 0.5);
    }

    #[test]
    fn scale_scales_rect_around_center() {
        let mut root = column([button("X").key("x").scale(2.0).width(Size::Fixed(40.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let pre = find_computed(&root, &state, "x").expect("computed");
        let post = inner_rect_quad_for(&root, &state, "x").expect("painted inner_rect");

        // 2x scale around centre: w doubles, x shifts left by w/2.
        assert!((post.w - pre.w * 2.0).abs() < 0.5);
        assert!((post.h - pre.h * 2.0).abs() < 0.5);
        let pre_cx = pre.center_x();
        let post_cx = post.center_x();
        assert!(
            (pre_cx - post_cx).abs() < 0.5,
            "centre should be preserved by scale-around-centre",
        );
    }

    #[test]
    fn shadow_auto_expands_painted_rect_around_inner_rect() {
        // `.shadow(s)` should auto-widen the painted quad without the
        // widget needing to set `paint_overflow` — the shader needs the
        // halo room to draw the soft band outside the layout rect.
        // No surface_role here so the El's shadow value reaches the
        // shader unchanged and we can assert the exact halo geometry.
        let mut root = column([El::new(Kind::Group)
            .key("c")
            .fill(tokens::CARD)
            .radius(tokens::RADIUS_LG)
            .shadow(tokens::SHADOW_MD)
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ops = draw_ops(&root, &state);
        let (painted, inner) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad {
                    id, rect, uniforms, ..
                } if id.contains("c") => {
                    let UniformValue::Vec4(v) = uniforms.get("inner_rect")? else {
                        return None;
                    };
                    Some((*rect, Rect::new(v[0], v[1], v[2], v[3])))
                }
                _ => None,
            })
            .expect("shadowed quad with inner_rect");

        // SHADOW_MD (== 12) → l=12, r=12, t=6, b=18.
        let blur = tokens::SHADOW_MD;
        assert!(
            (inner.x - painted.x - blur).abs() < 0.5,
            "left halo == blur, painted.x={}, inner.x={}",
            painted.x,
            inner.x,
        );
        assert!(
            (painted.right() - inner.right() - blur).abs() < 0.5,
            "right halo == blur",
        );
        assert!(
            (inner.y - painted.y - blur * 0.5).abs() < 0.5,
            "top halo == blur * 0.5",
        );
        assert!(
            (painted.bottom() - inner.bottom() - blur * 1.5).abs() < 0.5,
            "bottom halo == blur * 1.5",
        );
    }

    #[test]
    fn shadow_overflow_takes_per_side_max_with_explicit_paint_overflow() {
        // A focus-style outset of 8 on every side combined with
        // SHADOW_MD (12) should resolve to: l=12, r=12, t=8, b=18 —
        // shadow wins on left/right/bottom, paint_overflow wins on top.
        let combined =
            super::combined_overflow(crate::tree::Sides::all(8.0), tokens::SHADOW_MD, 0.0, 0.0);
        assert!((combined.left - 12.0).abs() < f32::EPSILON);
        assert!((combined.right - 12.0).abs() < f32::EPSILON);
        assert!((combined.top - 8.0).abs() < f32::EPSILON);
        assert!((combined.bottom - 18.0).abs() < f32::EPSILON);
    }

    #[test]
    fn shadow_overflow_is_zero_when_shadow_is_zero() {
        let combined = super::combined_overflow(crate::tree::Sides::zero(), 0.0, 0.0, 0.0);
        assert_eq!(combined, crate::tree::Sides::zero());
    }

    #[test]
    fn focus_overflow_outsets_painted_rect_by_ring_width() {
        let combined =
            super::combined_overflow(crate::tree::Sides::zero(), 0.0, 0.0, tokens::RING_WIDTH);
        assert_eq!(combined, crate::tree::Sides::all(tokens::RING_WIDTH));
    }

    #[test]
    fn inside_focus_ring_does_not_outset_painted_rect() {
        use crate::layout::layout;

        let mut tree = column([crate::menu_item("Open")
            .key("item")
            .width(Size::Fixed(100.0))])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let target = state.target_of_key(&tree, "item").expect("item target");
        state.focused = Some(target);
        state.focus_visible = true;
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now(), &Palette::default());

        let item_rect = state.rect_of_key(&tree, "item").expect("item rect");
        let ops = draw_ops(&tree, &state);
        let DrawOp::Quad { rect, uniforms, .. } =
            find_quad(&ops, "menu_item[item]").expect("menu item quad")
        else {
            panic!("expected menu item quad");
        };
        assert_eq!(*rect, item_rect);
        assert_eq!(
            uniforms.get("focus_width"),
            Some(&UniformValue::F32(-tokens::RING_WIDTH))
        );
    }

    #[test]
    fn stroke_overflow_outsets_painted_rect_by_half_width_plus_aa_tail() {
        // Stroke straddles the boundary; without any outset, cardinal
        // pixels of curved boundaries (radio indicator, switch thumb)
        // get clipped because the outside half of the band falls
        // outside the layout rect. Auto-widen by stroke_width/2 + 1px
        // (AA tail) so the full band rasterises symmetrically.
        let combined = super::combined_overflow(crate::tree::Sides::zero(), 0.0, 1.0, 0.0);
        let halo = 1.0 * 0.5 + 1.0;
        assert!((combined.left - halo).abs() < f32::EPSILON);
        assert!((combined.right - halo).abs() < f32::EPSILON);
        assert!((combined.top - halo).abs() < f32::EPSILON);
        assert!((combined.bottom - halo).abs() < f32::EPSILON);
    }

    #[test]
    fn stroke_and_shadow_take_per_side_max() {
        // Shadow's bottom halo (blur*1.5 = 18) beats stroke (1.5) on
        // the bottom; stroke beats shadow on the top (blur*0.5 = 6 vs
        // 1.5? no — shadow wins there too). Use a small shadow so the
        // stroke halo wins on the top.
        let combined = super::combined_overflow(crate::tree::Sides::zero(), 1.0, 4.0, 0.0);
        // Stroke halo = 4*0.5 + 1 = 3. Shadow blur = 1 → top = 0.5,
        // bottom = 1.5, l/r = 1. Stroke wins on every side.
        assert!(
            (combined.top - 3.0).abs() < f32::EPSILON,
            "top = {}",
            combined.top
        );
        assert!((combined.left - 3.0).abs() < f32::EPSILON);
        assert!((combined.right - 3.0).abs() < f32::EPSILON);
        // Bottom: max(stroke=3, shadow*1.5=1.5) → stroke wins.
        assert!((combined.bottom - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn stroked_indicator_painted_rect_outsets_layout_rect() {
        // Regression for the radio indicator: a small stroked circle
        // looked flattened at the cardinal directions because its
        // painted quad equalled the 16×16 layout rect, clipping the
        // outside half of the stroke band on the top/bottom/left/right.
        // After the fix, the quad outsets by stroke_width/2 + 1 on each
        // side so the AA tail rasterises cleanly.
        let mut root = column([El::new(Kind::Custom("radio-indicator"))
            .key("indicator")
            .width(Size::Fixed(16.0))
            .height(Size::Fixed(16.0))
            .radius(tokens::RADIUS_PILL)
            .fill(tokens::CARD)
            .stroke(tokens::INPUT)]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        let ops = draw_ops(&root, &state);
        let (painted, inner) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad {
                    id, rect, uniforms, ..
                } if id.contains("indicator") => {
                    let UniformValue::Vec4(v) = uniforms.get("inner_rect")? else {
                        return None;
                    };
                    Some((*rect, Rect::new(v[0], v[1], v[2], v[3])))
                }
                _ => None,
            })
            .expect("stroked indicator quad with inner_rect");

        // stroke_width default = 1 → halo = 0.5 + 1 = 1.5 on each side.
        let halo = 1.5;
        assert!(
            (inner.x - painted.x - halo).abs() < 1e-3,
            "left halo, painted.x={}, inner.x={}",
            painted.x,
            inner.x,
        );
        assert!(
            (painted.right() - inner.right() - halo).abs() < 1e-3,
            "right halo",
        );
        assert!((inner.y - painted.y - halo).abs() < 1e-3, "top halo",);
        assert!(
            (painted.bottom() - inner.bottom() - halo).abs() < 1e-3,
            "bottom halo",
        );
        // Layout rect itself is unchanged — only the painted quad
        // grows; the SDF still anchors to the original 16×16 box.
        assert!((inner.w - 16.0).abs() < 1e-3);
        assert!((inner.h - 16.0).abs() < 1e-3);
    }

    #[test]
    fn shadow_uniform_is_set_when_n_shadow_is_nonzero() {
        let mut root = column([El::new(Kind::Group)
            .key("c")
            .fill(tokens::CARD)
            .radius(tokens::RADIUS_LG)
            .shadow(tokens::SHADOW_MD)
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ops = draw_ops(&root, &state);
        let uniforms = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad { id, uniforms, .. } if id.contains("c") => Some(uniforms.clone()),
                _ => None,
            })
            .expect("shadowed quad");
        assert_eq!(
            uniforms.get("shadow"),
            Some(&UniformValue::F32(tokens::SHADOW_MD)),
            ".shadow(SHADOW_MD) on a node without surface_role must reach the shader unchanged",
        );
    }

    #[test]
    fn theme_role_override_propagates_to_painted_rect() {
        // The card widget binds SurfaceRole::Panel, which forces the
        // shadow uniform to SHADOW_SM regardless of the El's own
        // `.shadow(SHADOW_MD)` setting. The painted rect should track
        // the *effective* shadow (SM = 4), not the larger MD the
        // builder requested — over-expanding wastes overdraw budget.
        let mut root = column([crate::titled_card("Card", [crate::text("Body")]).key("c")]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ops = draw_ops(&root, &state);
        let (painted, inner) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad {
                    id, rect, uniforms, ..
                } if id.contains("c") => {
                    let UniformValue::Vec4(v) = uniforms.get("inner_rect")? else {
                        return None;
                    };
                    Some((*rect, Rect::new(v[0], v[1], v[2], v[3])))
                }
                _ => None,
            })
            .expect("card quad with inner_rect");

        let blur = tokens::SHADOW_SM;
        assert!(
            (inner.x - painted.x - blur).abs() < 0.5,
            "left halo == effective (theme-resolved) shadow, painted.x={}, inner.x={}",
            painted.x,
            inner.x,
        );
        assert!(
            (painted.bottom() - inner.bottom() - blur * 1.5).abs() < 0.5,
            "bottom halo == effective shadow * 1.5",
        );
    }

    /// Read the painted layout rect (== quad's `inner_rect` uniform) for
    /// the first quad whose id contains `key`. Falls back to the quad's
    /// `rect` for shaders that don't carry an `inner_rect` uniform.
    fn inner_rect_quad_for(root: &El, ui_state: &UiState, key: &str) -> Option<Rect> {
        use crate::shader::UniformValue;
        let ops = draw_ops(root, ui_state);
        for op in ops {
            if let DrawOp::Quad {
                id, rect, uniforms, ..
            } = op
                && id.contains(key)
            {
                if let Some(UniformValue::Vec4(v)) = uniforms.get("inner_rect") {
                    return Some(Rect::new(v[0], v[1], v[2], v[3]));
                }
                return Some(rect);
            }
        }
        None
    }

    fn find_computed(node: &El, ui_state: &UiState, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(ui_state.rect(&node.computed_id));
        }
        node.children
            .iter()
            .find_map(|c| find_computed(c, ui_state, key))
    }
}
