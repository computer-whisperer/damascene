//! Windowed Vulkano fixture for SVG-backed vector icons.
//!
//! Run: `cargo run -p damascene-vulkano-demo --bin icon_gallery`

use damascene_core::Rect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 840.0, 680.0);
    damascene_vulkano_demo::run(
        "Damascene — vector icons (vulkano)",
        viewport,
        damascene_fixtures::IconGallery,
    )
}
