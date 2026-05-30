//! toast — bundle-dump demo of the runtime-managed toast stack.
//!
//! Apps push `ToastSpec`s by accumulating them in their state and
//! returning them from `App::drain_toasts`. The runtime stamps each
//! with a monotonic id + an `expires_at` deadline, queues them on
//! `UiState::toasts`, and synthesizes a `Kind::Custom("toast_stack")`
//! floating layer at the El root each frame. The stack is bottom-right
//! anchored; each card carries a level-coloured leading bar, the
//! message, and a `toast-dismiss-{id}` button the runtime intercepts.
//!
//! This headless example seeds four toasts directly via
//! `UiState::push_toast` (skipping the App wiring), then runs the
//! same `synthesize_toasts` + `layout` + `draw_ops` path the live
//! runner uses. Inspect the bundle artifacts to see:
//!
//! - `out/toast.tree.txt` — `toast_stack` layer with one `toast_card`
//!   child per active toast.
//! - `out/toast.draw_ops.txt` — surface quad + text glyph runs for
//!   each card and the trailing `toast-dismiss-{id}` button.
//!
//! Run: `cargo run -p damascene-core --example toast`

use std::time::Duration;
use std::time::Instant;

use damascene_core::layout::assign_ids;
use damascene_core::prelude::*;
use damascene_core::state::UiState;
use damascene_core::toast::synthesize_toasts;

fn fixture() -> El {
    // Apps wrap their main view in `overlays(main, [])` so the
    // runtime can append the synthesized toast layer as an overlay
    // sibling — same convention as for popovers and modals.
    overlays(
        column([
            h2("Toasts"),
            paragraph(
                "Apps queue toasts by returning ToastSpec values from \
                 App::drain_toasts. The runtime stamps each with a TTL, \
                 stacks them at the bottom-right corner, and dismisses \
                 them on click or auto-expiry.",
            )
            .muted(),
            row([
                button("Save changes").key("save"),
                button("Trigger error").key("err"),
                button("Show info").key("info"),
            ])
            .gap(tokens::SPACE_2),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0)),
        [],
    )
}

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 360.0);
    // Seed the runtime's toast queue directly so the bundle dump
    // shows the synthesized layer. In a live app the host calls
    // `runner.push_toasts(app.drain_toasts())` once per frame.
    let mut state = UiState::new();
    let now = Instant::now();
    let long_ttl = Duration::from_secs(60);
    state.push_toast(ToastSpec::success("Settings saved").with_ttl(long_ttl), now);
    state.push_toast(
        ToastSpec::warning("Battery low — connect charger").with_ttl(long_ttl),
        now,
    );
    state.push_toast(
        ToastSpec::error("Failed to reach update server").with_ttl(long_ttl),
        now,
    );
    state.push_toast(
        ToastSpec::info("New version available").with_ttl(long_ttl),
        now,
    );

    let mut tree = fixture();
    assign_ids(&mut tree);
    let _ = synthesize_toasts(&mut tree, &mut state, now);
    let bundle = render_bundle_with(&mut tree, &mut state, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "toast")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }
    Ok(())
}
