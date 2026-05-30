//! Counter — vulkano backend acceptance fixture.
//!
//! Same App impl as `examples/src/bin/counter.rs`, driven through
//! `damascene-vulkano` instead. Side-by-side with the wgpu version this is
//! the visual A/B test for whether the `damascene-core` ↔ backend boundary
//! actually holds across two GPU APIs.
//!
//! Duplicated rather than `pub use`-imported so this backend acceptance
//! fixture remains self-contained.

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
    damascene_vulkano_demo::run("Damascene — counter (vulkano)", viewport, Counter { value: 0 })
}
