//! Windowed liquid-glass material lab.
//!
//! Run: `cargo run -p damascene-examples --bin liquid_glass_lab`

use damascene_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    damascene_winit_wgpu::run(
        "Damascene - liquid glass lab",
        Rect::new(0.0, 0.0, 1100.0, 760.0),
        damascene_fixtures::LiquidGlassLab,
    )
}
