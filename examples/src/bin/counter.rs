//! Counter — the smallest interactive App-trait proof point.
//!
//! Validates the load-bearing claim from `docs/LIBRARY_VISION.md`: a real
//! interactive native app fits in an [`App`] impl with a pure `build`
//! and a plain `&mut self` event handler. Hover and press visuals are
//! applied automatically by the library; the author never writes
//! `.hovered()` or `.pressed()`.
//!
//! Mouse over a button — it lightens. Press it — it darkens. Release or
//! press Enter/Space while focused — the count updates. That's the
//! entire round-trip:
//!
//! ```text
//! pointer event ─▶ winit ─▶ host runner ─▶ ui.pointer_*()
//!                                              ▼
//!                                         hit-test against
//!                                         last laid-out tree
//!                                              ▼
//! UiEvent ◀── ui.pointer_up() ◀────── click matched
//!     │
//!     ▼
//! app.on_event() ─▶ self.value += 1 ─▶ request_redraw
//!                                              │
//!                                              ▼
//!                                     app.build() returns
//!                                     a fresh tree from
//!                                     the new state
//! ```
//!
//! Run: `cargo run -p damascene-examples --bin counter`

use damascene_core::prelude::*;

struct Counter {
    value: i32,
}

impl App for Counter {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("−").key("dec").secondary(),
                button("Reset").key("reset").ghost(),
                button("+").key("inc").primary(),
            ])
            .gap(tokens::SPACE_3),
            text(if self.value == 0 {
                "Click + or − to change the count.".to_string()
            } else {
                format!("You have clicked +/− a net {} times.", self.value)
            })
            .center_text()
            .muted(),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .align(Align::Center)
        .justify(Justify::Center)
    }

    fn on_event(&mut self, event: UiEvent) {
        if event.is_click_or_activate("inc") {
            self.value += 1;
        } else if event.is_click_or_activate("dec") {
            self.value -= 1;
        } else if event.is_click_or_activate("reset") {
            self.value = 0;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 480.0, 280.0);
    damascene_winit_wgpu::run("Damascene — counter", viewport, Counter { value: 0 })
}
