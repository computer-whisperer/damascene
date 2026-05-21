//! Minimal smoke app for the ash backend.
//!
//! Run: `cargo run -p aetna-ash-demo --bin hello`

use aetna_core::*;

struct HelloAsh;

impl App for HelloAsh {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            column([
                h1("Aetna ash")
                    .text_color(Color::rgb(244, 247, 255))
                    .line_height(40.0),
                paragraph("Text, surfaces, atlas uploads, and dynamic rendering.")
                    .text_color(Color::rgb(200, 209, 226))
                    .wrap_text(),
            ])
            .gap(6.0)
            .padding(16.0)
            .width(Size::Fixed(520.0))
            .fill(Color::rgba(255, 255, 255, 30))
            .radius(12.0),
            row([
                block(Color::rgb(62, 84, 172), 220.0, 180.0).radius(18.0),
                block(Color::rgb(48, 155, 125), 180.0, 180.0).radius(18.0),
                block(Color::rgb(222, 170, 76), 260.0, 180.0).radius(18.0),
            ])
            .gap(18.0),
            row([
                block(Color::rgb(218, 91, 84), 300.0, 120.0).radius(14.0),
                block(Color::rgb(77, 137, 210), 180.0, 120.0).radius(14.0),
                block(Color::rgb(116, 93, 158), 180.0, 120.0).radius(14.0),
            ])
            .gap(18.0),
        ])
        .gap(18.0)
        .padding(28.0)
        .fill(Color::rgb(22, 24, 32))
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
    aetna_ash_demo::run(
        "Aetna ash smoke",
        Rect::new(0.0, 0.0, 760.0, 520.0),
        HelloAsh,
    )
}
