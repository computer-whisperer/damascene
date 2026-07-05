//! Pan/zoom viewport demo.
//!
//! A grid of cards larger than the window, wrapped in a `viewport()`.
//! Drag the empty canvas to pan; roll the wheel to zoom toward the
//! cursor. The card chrome and text scale with the zoom automatically —
//! the app never multiplies anything by the zoom factor. The canvas
//! opens *fit* and tracks window resizes by itself
//! (`FitPolicy::Contain`) until the first pan/zoom hands control over;
//! the readout shows "Fit" at the home framing and a live percentage
//! once you've taken the view somewhere (`BuildCx::viewport_at_home`).
//! "Fit" re-frames all the content and re-arms the policy — under
//! `Contain` a `ResetView` request would do the same (the home framing
//! *is* the fit), so one button covers both. The pan/zoom lives in the
//! library's `UiState` (keyed by `"canvas"`), so it persists across the
//! rebuilds these button clicks trigger.
//!
//! Run: `cargo run -p damascene-examples --bin viewport`

use damascene_core::prelude::*;

#[derive(Default)]
struct Demo {
    /// Programmatic viewport commands queued from toolbar clicks, drained
    /// once per frame by the host.
    pending: Vec<ViewportRequest>,
}

const COLS: usize = 7;
const ROWS: usize = 6;

fn node_card(i: usize) -> El {
    let titles = ["parse", "lex", "lower", "typeck", "emit", "link", "run"];
    let title = titles[i % titles.len()];
    card([column([
        text(format!("{title}_{i}")).bold(),
        text(format!("node #{i}")).muted().font_size(12.0),
    ])
    .gap(tokens::SPACE_1)])
    .key(format!("node-{i}"))
    .width(Size::Fixed(150.0))
    .height(Size::Fixed(86.0))
    .padding(tokens::SPACE_3)
}

impl App for Demo {
    fn build(&self, cx: &BuildCx) -> El {
        // The content layer: a grid that's wider/taller than the window.
        let grid = column(
            (0..ROWS)
                .map(|r| {
                    row((0..COLS)
                        .map(|c| node_card(r * COLS + c))
                        .collect::<Vec<_>>())
                    .gap(tokens::SPACE_4)
                })
                .collect::<Vec<_>>(),
        )
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_5);

        // "Fit" at the home framing, a concrete percentage once the user
        // has steered the view — the widget tracks which one applies.
        let zoom = cx.viewport_view("canvas").map_or(1.0, |v| v.zoom);
        let readout = if cx.viewport_at_home("canvas").unwrap_or(true) {
            "Fit".to_string()
        } else {
            format!("{:.0}%", zoom * 100.0)
        };

        column([
            row([
                h2("Pan / zoom canvas"),
                spacer(),
                text(readout).muted(),
                button("Fit").key("fit"),
            ])
            .gap(tokens::SPACE_3)
            .padding(Sides::xy(tokens::SPACE_5, tokens::SPACE_3)),
            text("Drag the background to pan · wheel to zoom toward the cursor")
                .muted()
                .padding(Sides::x(tokens::SPACE_5)),
            // The whole feature: a clipped, pannable, zoomable window onto
            // `grid`. No `* zoom` anywhere in this file.
            viewport([grid])
                .key("canvas")
                .min_zoom(0.3)
                .max_zoom(4.0)
                .fit_policy(FitPolicy::Contain { padding: 24.0 })
                .height(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
            return;
        }
        if event.route() == Some("fit") {
            self.pending.push(ViewportRequest::FitContent {
                key: "canvas".into(),
                padding: 24.0,
            });
        }
    }

    fn drain_viewport_requests(&mut self) -> Vec<ViewportRequest> {
        std::mem::take(&mut self.pending)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport_rect = Rect::new(0.0, 0.0, 760.0, 560.0);
    damascene_winit_wgpu::run("Damascene — viewport", viewport_rect, Demo::default())
}
