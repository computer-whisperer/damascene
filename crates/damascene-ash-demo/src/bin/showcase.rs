//! Showcase — the shared `damascene-fixtures::Showcase` app routed through
//! the ash backend.
//!
//! Run: `cargo run -p damascene-ash-demo --bin showcase`

use damascene_core::Rect;
use damascene_fixtures::Showcase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    damascene_ash_demo::run("Damascene — showcase (ash)", viewport, Showcase::new())
}
