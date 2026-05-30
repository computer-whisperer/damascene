//! Top-right host-diagnostics overlay.
//!
//! Renders the per-frame [`HostDiagnostics`] snapshot the host hands the
//! showcase via `BuildCx::diagnostics()`. The intent is that this gives
//! "always-visible" answers to the recurring questions during web/native
//! debugging: which backend is wgpu actually using, how long did the
//! previous frame take wall-clock, and what triggered the redraw we're
//! currently in. None of these are derivable from the rest of the UI —
//! `damascene-core` is host-agnostic and the host's own logs scroll past.
//!
//! The overlay is opt-in: `Showcase::build` only mounts it when the
//! host supplied a `HostDiagnostics` (so headless render bins, the
//! vulkano demo, and any future host that wants to skip it pay
//! nothing). It's a `Custom("damascene-diagnostics")` layer with a manual
//! layout function that pins the panel against the viewport's
//! top-right corner — same approach `toast_stack` uses to float
//! independently of whatever flow the page is using underneath.
//!
//! Pointer events fall through: the panel is a non-blocking layer on
//! top of the main view, so it doesn't intercept clicks against
//! whatever's beneath.

use damascene_core::prelude::*;
use damascene_core::tree::Kind;

const PANEL_PAD: f32 = tokens::SPACE_2;
const ROW_GAP: f32 = 2.0;

/// Build the diagnostic overlay layer. Returns a `Custom`-kind floating
/// layer that pins itself to the viewport's top-right corner; suitable
/// for inclusion in `overlays(main, layers)`.
pub fn layer(diag: &HostDiagnostics) -> El {
    let panel = column([
        row_kv("backend", diag.backend.to_string()),
        row_kv("trigger", diag.trigger.label().to_string()),
        row_kv("frame", format!("#{}", diag.frame_index)),
        row_kv("dt", format_dt(diag.last_frame_dt)),
        row_kv(
            "size",
            format!(
                "{}×{} @{:.1}x",
                diag.surface_size.0, diag.surface_size.1, diag.scale_factor
            ),
        ),
        row_kv("msaa", format!("{}x", diag.msaa_samples)),
    ])
    .gap(ROW_GAP)
    .padding(PANEL_PAD)
    .fill(tokens::POPOVER)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
    .width(Size::Hug)
    .height(Size::Hug);

    // Custom layer with a manual layout function that pins the panel
    // to the viewport's top-right corner. Same pattern toast_stack uses
    // — bypass the parent's flow entirely so the overlay is unaffected
    // by whatever the page laid out underneath.
    El::new(Kind::Custom("damascene-diagnostics"))
        .children([panel])
        .fill_size()
        .layout(|ctx| {
            let viewport = (ctx.rect_of_id)("root").unwrap_or(ctx.container);
            let pad = tokens::SPACE_3;
            let mut rects = Vec::with_capacity(ctx.children.len());
            for c in ctx.children.iter() {
                let (w, h) = (ctx.measure)(c);
                let x = viewport.right() - w - pad;
                let y = viewport.y + pad;
                rects.push(Rect::new(x, y, w, h));
            }
            rects
        })
}

fn row_kv(label: &str, value: String) -> El {
    row([mono(label).muted().small(), mono(value).small()]).gap(tokens::SPACE_2)
}

fn format_dt(dt: std::time::Duration) -> String {
    let ms = dt.as_secs_f32() * 1000.0;
    if ms <= 0.0 {
        return "—".to_string();
    }
    let fps = if ms > 0.0 { 1000.0 / ms } else { 0.0 };
    format!("{ms:.1}ms ({fps:.0}fps)")
}
