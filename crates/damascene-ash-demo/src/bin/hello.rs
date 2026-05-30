//! Minimal smoke app for the ash backend.
//!
//! Run: `cargo run -p damascene-ash-demo --bin hello`

use damascene_core::*;

struct HelloAsh;

impl App for HelloAsh {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            column([
                h1("Damascene ash")
                    .text_color(Color::srgb_u8(244, 247, 255))
                    .line_height(40.0),
                paragraph("Text, surfaces, atlas uploads, and dynamic rendering.")
                    .text_color(Color::srgb_u8(200, 209, 226))
                    .wrap_text(),
            ])
            .gap(6.0)
            .padding(16.0)
            .width(Size::Fixed(520.0))
            .fill(Color::srgb_u8a(255, 255, 255, 30))
            .radius(12.0),
            row([
                block(Color::srgb_u8(62, 84, 172), 220.0, 180.0).radius(18.0),
                block(Color::srgb_u8(48, 155, 125), 180.0, 180.0).radius(18.0),
                block(Color::srgb_u8(222, 170, 76), 260.0, 180.0).radius(18.0),
            ])
            .gap(18.0),
            row([
                block(Color::srgb_u8(218, 91, 84), 300.0, 120.0).radius(14.0),
                block(Color::srgb_u8(77, 137, 210), 180.0, 120.0).radius(14.0),
                block(Color::srgb_u8(116, 93, 158), 180.0, 120.0).radius(14.0),
            ])
            .gap(18.0),
        ])
        .gap(18.0)
        .padding(28.0)
        .fill(Color::srgb_u8(22, 24, 32))
        .fill_width()
        .fill_height()
    }

    fn on_event(&mut self, _event: UiEvent) {}
}

fn block(color: Color, width: f32, height: f32) -> El {
    column(std::iter::empty::<El>())
        .width(Size::Fixed(width))
        .height(Size::Fixed(height))
        .fill(color)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    damascene_ash_demo::run(
        "Damascene ash smoke",
        Rect::new(0.0, 0.0, 760.0, 520.0),
        HelloAsh,
    )
}
