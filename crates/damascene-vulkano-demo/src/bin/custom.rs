//! Smoke fixture — register a custom WGSL shader and render through it
//! on the vulkano backend. Same gradient.wgsl that
//! `damascene-wgpu/examples/render_custom.rs` exercises on the wgpu side, so
//! you can A/B both backends against the same custom shader.
//!
//! What this proves: `Runner::register_shader` runs naga on the WGSL,
//! installs a graphics pipeline against the shared `QuadInstance`
//! layout + descriptor set, and the paint stream picks it up for any
//! `El::shader(ShaderBinding::custom(name))` node — without any
//! damascene-core or damascene-vulkano changes per shader.
//!
//! Run: `cargo run -p damascene-vulkano-demo --bin custom`

use damascene_core::*;

const GRADIENT_WGSL: &str = include_str!("../../../damascene-core/shaders/gradient.wgsl");

fn gradient_button(label: &str, top: Color, bottom: Color, radius: f32) -> El {
    button(label).text_color(tokens::PRIMARY_FOREGROUND).shader(
        ShaderBinding::custom("gradient")
            .color("vec_a", top)
            .color("vec_b", bottom)
            .f32("vec_c", radius),
    )
}

struct Custom;

impl App for Custom {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h1("Custom shader (vulkano)"),
            paragraph(
                "Three buttons paint via a registered custom shader \
                 (gradient.wgsl). The right-hand button is stock \
                 rounded_rect for contrast.",
            )
            .muted(),
            row([
                gradient_button(
                    "Sunrise",
                    Color::srgb_u8(255, 200, 90),
                    Color::srgb_u8(245, 95, 110),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Ocean",
                    Color::srgb_u8(120, 200, 255),
                    Color::srgb_u8(40, 90, 200),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Forest",
                    Color::srgb_u8(180, 230, 140),
                    Color::srgb_u8(40, 110, 80),
                    tokens::RADIUS_MD,
                ),
                spacer(),
                button("Stock").secondary(),
            ])
            .gap(tokens::SPACE_3),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
    }

    fn on_event(&mut self, _event: UiEvent, _cx: &EventCx) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 280.0);
    damascene_vulkano_demo::run_with_init(
        "Damascene — custom shader (vulkano)",
        viewport,
        Custom,
        |runner| runner.register_shader("gradient", GRADIENT_WGSL),
    )
}
