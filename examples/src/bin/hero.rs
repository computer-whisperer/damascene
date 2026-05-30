//! Polished end-to-end Damascene demo used by the root README hero shot.
//!
//! Run: `cargo run -p damascene-examples --bin hero`

use damascene_core::Rect;
use damascene_fixtures::HeroDemo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1360.0, 820.0);
    damascene_winit_wgpu::run("Damascene — hero demo", viewport, HeroDemo)
}
