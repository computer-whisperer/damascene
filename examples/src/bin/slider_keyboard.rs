//! Slider keyboard — the controlled `slider::apply_input` helper.
//!
//! A focused `slider` receives both `KeyDown` and pointer events;
//! [`slider::apply_input`] folds either into a normalized value in
//! one call. Keyboard follows the standard ARIA range pattern:
//! `ArrowUp` / `ArrowRight` step up by `step`, `ArrowDown` /
//! `ArrowLeft` step down, `PageUp` / `PageDown` adjust by
//! `page_step`, `Home` / `End` jump to the ends. Pointer events
//! (`Click` / `PointerDown` / `Drag`) set the value to the pointer
//! position within the slider's track.
//!
//! Run: `cargo run -p damascene-examples --bin slider_keyboard`
//!
//! Things to try:
//! - Press `Tab` to focus the slider, then arrow keys to move.
//! - Drag the thumb with the pointer — the same `value` field
//!   updates, so keyboard and pointer stay in sync.
//! - Hold `Shift` is not used here; it's `PageUp` / `PageDown` for
//!   the coarse step.

use damascene_core::prelude::*;

struct VolumeDemo {
    value: f32,
}

impl App for VolumeDemo {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h2("Slider keyboard demo"),
            text(format!("{:.0}%", self.value * 100.0))
                .muted()
                .center_text(),
            slider(self.value, tokens::PRIMARY)
                .key("vol")
                .width(Size::Fixed(280.0)),
            text("Tab to focus · Arrows step 5% · PgUp/Dn 25% · Home/End jump")
                .muted()
                .center_text()
                .caption(),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_7)
        .align(Align::Center)
        .justify(Justify::Center)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        // One call handles both pointer drag and keyboard arrows.
        slider::apply_input(&mut self.value, &event, "vol", 0.05, 0.25);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 480.0, 280.0);
    damascene_winit_wgpu::run(
        "Damascene — slider keyboard",
        viewport,
        VolumeDemo { value: 0.5 },
    )
}
