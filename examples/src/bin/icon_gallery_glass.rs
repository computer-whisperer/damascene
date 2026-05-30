//! Windowed wgpu fixture for the vector-icon glass material.
//!
//! Run: `cargo run -p damascene-examples --bin icon_gallery_glass`

use damascene_core::Rect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 840.0, 680.0);
    damascene_winit_wgpu::run(
        "Damascene - vector icon glass",
        viewport,
        damascene_fixtures::GlassIconGallery,
    )
}
