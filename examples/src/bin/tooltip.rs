//! Tooltip — the `.tooltip(text)` modifier.
//!
//! A row of buttons; hover on any of them, and after the standard
//! delay the runtime appends a styled tooltip layer anchored below
//! the button. Move away or click — the tooltip disappears.
//!
//! Apps don't compose the floating layer themselves: the runtime
//! synthesizes it from hover state. The user's tree is unchanged
//! by `.tooltip()`; the appended layer is part of the laid-out tree
//! the same frame, so it goes through normal layout / paint.
//!
//! Run: `cargo run -p damascene-examples --bin tooltip`
//!
//! Things to try:
//! - Hover a button and wait — the tooltip fades in below.
//! - Move the pointer along the row — the tooltip retracks.
//! - Click a button — the tooltip dismisses for the rest of the
//!   hover (move pointer away and back to see it again).

use damascene_core::prelude::*;

#[derive(Default)]
struct Demo {
    last_clicked: Option<&'static str>,
}

impl App for Demo {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h2("Tooltip demo"),
            text(match self.last_clicked {
                Some(k) => format!("Clicked: {k}"),
                None => "Hover any button — tooltip appears after 500ms.".to_string(),
            })
            .muted(),
            spacer().height(Size::Fixed(tokens::SPACE_4)),
            row([
                button("Save")
                    .key("save")
                    .primary()
                    .tooltip("Save the current document (Ctrl+S)"),
                button("Open")
                    .key("open")
                    .secondary()
                    .tooltip("Open a file from disk"),
                button("Settings")
                    .key("settings")
                    .ghost()
                    .tooltip("Application preferences"),
            ])
            .gap(tokens::SPACE_3),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_7)
        .align(Align::Center)
        .justify(Justify::Center)
    }

    fn on_event(&mut self, event: UiEvent) {
        if event.is_click_or_activate("save") {
            self.last_clicked = Some("Save");
        } else if event.is_click_or_activate("open") {
            self.last_clicked = Some("Open");
        } else if event.is_click_or_activate("settings") {
            self.last_clicked = Some("Settings");
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 560.0, 320.0);
    damascene_winit_wgpu::run("Damascene — tooltip demo", viewport, Demo::default())
}
