//! Animated palette — proof point for `.animate()`.
//!
//! A row of three colour swatches. Click one to select it: the picked
//! swatch scales up (1.0 → 1.15), tints toward white (`fill` change),
//! and any previously-selected swatch fades + slides back. The status
//! line below cross-fades when the selection changes.
//!
//! Every prop here is animated by attaching `.animate(timing)` to the
//! El. The library compares the previous frame's value to the new one
//! and eases between them. State visuals (hover lighten, press darken,
//! focus ring) keep their own timings and compose on top — no fight.
//!
//! Try:
//! - Hover any swatch — buttons lighten via `SPRING_QUICK` envelopes.
//! - Click — selection state transition is `SPRING_BOUNCY` (visible
//!   overshoot on the scale-up).
//! - Press repeatedly — momentum carries through interrupted motion.
//!
//! Run: `cargo run -p damascene-examples --bin animated_palette`

use damascene_core::prelude::*;

#[derive(Clone, Copy)]
struct Swatch {
    name: &'static str,
    fill: Color,
}

const SWATCHES: &[Swatch] = &[
    Swatch {
        name: "warm",
        fill: Color::srgb_u8(255, 138, 76),
    },
    Swatch {
        name: "cool",
        fill: Color::srgb_u8(76, 158, 255),
    },
    Swatch {
        name: "lime",
        fill: Color::srgb_u8(140, 220, 110),
    },
];

struct Palette {
    selected: Option<usize>,
}

impl App for Palette {
    fn build(&self, _cx: &BuildCx) -> El {
        let swatches: Vec<El> = SWATCHES
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let is_selected = Some(i) == self.selected;
                // `fill` cross-fades from token to white-tinted version
                // when selected; `scale` overshoots up; selected
                // swatches lift slightly via translate.
                let fill = if is_selected {
                    s.fill.mix(Color::srgb_u8(255, 255, 255), 0.35)
                } else {
                    s.fill
                };
                let scale = if is_selected { 1.15 } else { 1.0 };
                let lift = if is_selected { -8.0 } else { 0.0 };

                titled_card(
                    s.name,
                    [text(if is_selected { "picked" } else { "tap" })
                        .center_text()
                        .muted()],
                )
                .key(format!("swatch-{i}"))
                .fill(fill)
                .width(Size::Fixed(120.0))
                .height(Size::Fixed(120.0))
                .scale(scale)
                .translate(0.0, lift)
                .animate(Timing::SPRING_BOUNCY)
            })
            .collect();

        let status = if let Some(i) = self.selected {
            format!("{} is picked.", SWATCHES[i].name)
        } else {
            "tap a card to pick.".to_string()
        };

        column([
            h2("Animated palette"),
            text("Cards spring up on tap; status fades on change.").muted(),
            row(swatches).gap(tokens::SPACE_3),
            // The status line cross-fades by easing its opacity to 1
            // each rebuild — when `self.selected` changes, the build
            // produces a different text node; opacity 0 → 1 eases in.
            text(status)
                .key("status")
                .center_text()
                .opacity(1.0)
                .animate(Timing::SPRING_GENTLE),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .align(Align::Center)
        .justify(Justify::Center)
    }

    fn on_event(&mut self, event: UiEvent) {
        if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(k) = event.route()
            && let Some(rest) = k.strip_prefix("swatch-")
            && let Ok(i) = rest.parse::<usize>()
        {
            self.selected = if Some(i) == self.selected {
                None
            } else {
                Some(i)
            };
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 560.0, 360.0);
    damascene_winit_wgpu::run(
        "Damascene — animated_palette",
        viewport,
        Palette { selected: None },
    )
}
