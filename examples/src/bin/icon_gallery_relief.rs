//! Windowed wgpu fixture for the vector-icon relief material.
//!
//! Run: `cargo run -p damascene-examples --bin icon_gallery_relief`

use damascene_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 880.0, 620.0);
    damascene_winit_wgpu::run(
        "Damascene — vector icon relief",
        viewport,
        damascene_fixtures::ReliefIconGallery,
    )
}
