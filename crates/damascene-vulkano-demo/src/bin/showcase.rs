//! Showcase — the shared `damascene-fixtures::Showcase` app routed through
//! the vulkano backend. Broad-coverage A/B fixture: every Damascene primitive
//! (sidebar nav, scroll, animation, hotkeys, cards) must produce
//! visually-equivalent output through `damascene-vulkano` as it does through
//! `damascene-wgpu`.
//!
//! Run: `cargo run -p damascene-vulkano-demo --bin showcase`

use damascene_core::Rect;
use damascene_fixtures::Showcase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    damascene_vulkano_demo::run("Damascene — showcase (vulkano)", viewport, Showcase::new())
}
