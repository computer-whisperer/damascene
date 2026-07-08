//! Run Damascene's `settings` fixture against the real wgpu backend in
//! a winit window.
//!
//! This is a static fixture — no buttons are keyed and `on_event` is a
//! no-op — so the library's hover/press visuals don't kick in. It exists
//! as a readable parity baseline against `out/settings.wgpu.png`; real
//! buttons carry `.key(...)` and an `on_event` arm (see `counter.rs`).
//! For an interactive settings *form* template, start from
//! `settings_modal.rs` instead.
//!
//! Run: `cargo run -p damascene-examples --bin settings`

use damascene_core::prelude::*;

struct Settings;

impl App for Settings {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h1("Settings"),
            titled_card(
                "Account",
                [
                    row([text("Email"), text("user@example.com").muted()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                    row([
                        text("Two-factor authentication"),
                        badge("Enabled").success(),
                    ])
                    .align(Align::Center)
                    .justify(Justify::SpaceBetween),
                    row([text("Recovery codes"), button("Generate").secondary()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                ],
            ),
            titled_card(
                "Appearance",
                [
                    row([text("Theme"), button("Dark").secondary()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                    row([text("Compact mode"), badge("Off").muted()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                    row([text("Font size"), text("14")])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                ],
            ),
            titled_card(
                "Danger zone",
                [row([
                    column([
                        text("Delete account").bold(),
                        text("Permanently remove your account and all data.")
                            .muted()
                            .small(),
                    ])
                    .gap(tokens::SPACE_1)
                    .align(Align::Start),
                    button("Delete").destructive(),
                ])
                .align(Align::Center)
                .justify(Justify::SpaceBetween)],
            ),
            row([button("Cancel").ghost(), button("Save").primary()])
                .gap(tokens::SPACE_2)
                .justify(Justify::End),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 760.0);
    damascene_winit_wgpu::run("Damascene — settings", viewport, Settings)
}
