//! Polished end-to-end Damascene demo used by the root README hero shot.
//!
//! Run: `cargo run -p damascene-examples --bin hero`

use damascene_core::Rect;
use damascene_fixtures::HeroDemo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = damascene_fixtures::hero::HERO_LOGICAL_SIZE;
    let viewport = Rect::new(0.0, 0.0, w as f32, h as f32);
    damascene_winit_wgpu::run("Damascene — hero demo", viewport, HeroDemo::default())
}
