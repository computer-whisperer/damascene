//! Showcase — the shared `aetna-fixtures::Showcase` app routed through
//! the ash backend.
//!
//! Run: `cargo run -p aetna-ash-demo --bin showcase`

use aetna_core::Rect;
use aetna_fixtures::Showcase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    aetna_ash_demo::run("Aetna — showcase (ash)", viewport, Showcase::new())
}
