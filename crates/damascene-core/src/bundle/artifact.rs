//! Render bundle — one call produces every artifact the agent loop needs.
//!
//! A [`Bundle`] is the textual + visual representation of a rendered tree:
//!
//! - `svg` — visual fixture (also convertible to PNG via `tools/svg_to_png.sh`).
//! - `tree_dump` — semantic walk of the laid-out tree with rects and source.
//! - `draw_ops` — flat draw-op IR (the same one a wgpu backend consumes).
//! - `shader_manifest` — every shader used by this tree, with uniform values.
//! - `lint` — findings: raw values in user code, overflows, duplicate IDs.
//!
//! [`render_bundle`] runs layout + draw-op resolution + dump + lint in one
//! call so a single `cargo run --example X` produces everything needed
//! to verify intent without further round-trips.
//!
//! The SVG output is approximate (stock shaders rendered best-effort,
//! custom shaders as placeholder rects). The wgpu renderer is the source
//! of truth for visual fidelity; SVG stays as a layout/structure
//! debugging artifact.
//!
//! # Wiring this into your app
//!
//! The bundle pipeline is also the cheapest layout-review path *during
//! app development*. It runs CPU-only, exercises the same layout +
//! draw-op stack the GPU does, and produces a diffable tree dump that
//! catches regressions long before they hit a window. The shape every
//! damascene app converges on:
//!
//! ```ignore
//! // crates/your-app/src/bin/dump_bundles.rs
//! use damascene_core::prelude::*;
//! use std::path::PathBuf;
//!
//! struct MockBackend { state: AppState }
//! impl UiBackend for MockBackend { /* return canned `state` */ }
//!
//! enum Scene { Empty, Loaded, ErrorDialog /* ... */ }
//!
//! fn main() -> std::io::Result<()> {
//!     let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
//!     let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
//!
//!     for scene in Scene::ALL {
//!         let mut app = MyApp::new(MockBackend { state: scene.canned_state() });
//!         // Local UI flags? Drive them through the real on_event path:
//!         scene.drive_setup(&mut app);
//!
//!         let theme = app.theme();
//!         let mut tree = app.build(&BuildCx::new(&theme));
//!         let bundle = render_bundle(&mut tree, viewport);
//!         write_bundle(&bundle, &out_dir, &scene.slug())?;
//!         if !bundle.lint.findings.is_empty() {
//!             eprint!("{}", bundle.lint.text());
//!         }
//!     }
//!     Ok(())
//! }
//! ```
//!
//! Three to six scenes is plenty for a typical app chrome. Output goes
//! to `crates/<app>/out/` (gitignore the directory). Worked examples in
//! the workspace: `tools/src/bin/dump_showcase_bundles.rs` (damascene's own
//! showcase), and the `render_artifacts` / `dump_bundles` bins in the
//! external `damascene-volume` and `rumble-damascene` apps.
//!
//! Driving local UI state via [`crate::event::UiEvent::synthetic_click`]
//! is preferred over fixture-only setters: the dumped scene is exactly
//! what the user sees after performing the same interaction, so the
//! fixture and production code can't drift.

use std::path::Path;

use super::inspect;
use super::lint::{LintReport, lint};
use super::manifest;
use super::svg::svg_from_ops;
use crate::draw_ops;
use crate::ir::DrawOp;
use crate::layout;
use crate::state::UiState;
use crate::theme::Theme;
use crate::tokens;
use crate::tree::{El, Rect};

/// Everything an agent loop wants from a single render.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Bundle {
    /// SVG source (approximate — see crate-level docs).
    pub svg: String,
    /// Semantic tree dump — grep-able, source-mapped.
    pub tree_dump: String,
    /// Flat draw-op list — the same IR a wgpu backend would consume.
    pub draw_ops: Vec<DrawOp>,
    /// Shader manifest — usage + resolved uniforms per draw.
    pub shader_manifest: String,
    /// Findings from the lint pass.
    pub lint: LintReport,
}

/// Lay out, resolve to draw ops, dump, lint.
///
/// Constructs a fresh [`UiState`] internally — bundle artifacts are a
/// snapshot of the tree at rest, with no hover/press/focus state. For
/// fixtures that need to demonstrate non-trivial state (a scroll
/// position, a hovered button), see [`render_bundle_with`].
pub fn render_bundle(root: &mut El, viewport: Rect) -> Bundle {
    render_bundle_with(root, &mut UiState::new(), viewport)
}

/// Same as [`render_bundle`], but resolves implicit surfaces through a
/// caller-supplied [`Theme`].
pub fn render_bundle_themed(root: &mut El, viewport: Rect, theme: &Theme) -> Bundle {
    render_bundle_with_theme(root, &mut UiState::new(), viewport, theme)
}

/// Same as [`render_bundle`], but threads a caller-built [`UiState`]
/// through the pipeline. Use this when the fixture wants to seed
/// runtime state (scroll offsets, hovered/focused trackers) before
/// snapshotting — the layout pass reads it, and the resulting bundle
/// reflects the seeded state.
///
/// Seed scroll offsets by calling [`crate::layout::assign_ids`] first
/// to populate `computed_id`, then calling [`UiState::set_scroll_offset`].
pub fn render_bundle_with(root: &mut El, ui_state: &mut UiState, viewport: Rect) -> Bundle {
    render_bundle_with_theme(root, ui_state, viewport, &Theme::default())
}

/// Same as [`render_bundle_with`], but resolves implicit surfaces through
/// a caller-supplied [`Theme`].
pub fn render_bundle_with_theme(
    root: &mut El,
    ui_state: &mut UiState,
    viewport: Rect,
    theme: &Theme,
) -> Bundle {
    theme.apply_metrics(root);
    layout::layout(root, ui_state, viewport);
    let draw_ops = draw_ops::draw_ops_with_theme(root, ui_state, theme);
    // Resolve the background through the same theme as the ops — the
    // raw token serializes as the dark palette's rgb, which put a dark
    // page rect behind light-resolved widgets (issue #135).
    let svg = svg_from_ops(
        viewport.w,
        viewport.h,
        &draw_ops,
        theme.resolve(tokens::BACKGROUND),
    );
    let tree_dump = inspect::dump_tree(root, ui_state);
    let shader_manifest = manifest::shader_manifest(&draw_ops);
    let lint = lint(root, ui_state, theme);
    Bundle {
        svg,
        tree_dump,
        draw_ops,
        shader_manifest,
        lint,
    }
}

/// Write a bundle to disk under `dir`, naming files `{name}.{ext}`.
///
/// Files written:
/// - `{name}.svg`
/// - `{name}.tree.txt`
/// - `{name}.draw_ops.txt`
/// - `{name}.shader_manifest.txt`
/// - `{name}.lint.txt`
pub fn write_bundle(
    bundle: &Bundle,
    dir: &Path,
    name: &str,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();

    let svg = dir.join(format!("{name}.svg"));
    std::fs::write(&svg, &bundle.svg)?;
    written.push(svg);

    let tree = dir.join(format!("{name}.tree.txt"));
    std::fs::write(&tree, &bundle.tree_dump)?;
    written.push(tree);

    let draw_ops_path = dir.join(format!("{name}.draw_ops.txt"));
    std::fs::write(&draw_ops_path, manifest::draw_ops_text(&bundle.draw_ops))?;
    written.push(draw_ops_path);

    let manifest_path = dir.join(format!("{name}.shader_manifest.txt"));
    std::fs::write(&manifest_path, &bundle.shader_manifest)?;
    written.push(manifest_path);

    let lint = dir.join(format!("{name}.lint.txt"));
    std::fs::write(&lint, bundle.lint.text())?;
    written.push(lint);

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::svg::color_svg;

    /// Issue #135: the SVG's page background must resolve through the
    /// caller's theme, not serialize the raw `tokens::BACKGROUND`
    /// sentinel (which carries the dark palette's rgb).
    #[test]
    fn themed_bundle_background_resolves_through_theme() {
        let theme = Theme::damascene_light();
        let mut root = El::new(crate::tree::Kind::Custom("root"));
        let bundle = render_bundle_themed(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0), &theme);

        let bg_rect = |c: crate::color::Color| {
            format!(
                r#"<rect width="100%" height="100%" fill="{}"/>"#,
                color_svg(c)
            )
        };
        assert!(
            bundle
                .svg
                .contains(&bg_rect(theme.resolve(tokens::BACKGROUND))),
            "background rect should use the light-resolved token; svg head: {}",
            &bundle.svg[..bundle.svg.len().min(400)]
        );
        assert!(
            !bundle.svg.contains(&bg_rect(tokens::BACKGROUND)),
            "background rect must not serialize the raw dark sentinel"
        );
    }
}
