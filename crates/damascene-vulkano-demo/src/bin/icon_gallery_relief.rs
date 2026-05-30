//! Windowed Vulkano fixture for the vector-icon relief material.
//!
//! Run: `cargo run -p damascene-vulkano-demo --bin icon_gallery_relief`

use damascene_core::Rect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 840.0, 680.0);
    damascene_vulkano_demo::run(
        "Damascene — vector icon relief (vulkano)",
        viewport,
        damascene_fixtures::ReliefIconGallery,
    )
}
