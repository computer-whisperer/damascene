//! Interactive scroll demo.
//!
//! A list of selectable rows in a scroll viewport. The list's content
//! is taller than the viewport — roll the wheel inside it to scroll.
//! Click a row to select it; the scroll offset persists across the
//! resulting rebuilds because it's tracked by the library's `UiState`,
//! not by the app.
//!
//! Run: `cargo run -p damascene-examples --bin scroll_list`

use damascene_core::prelude::*;

struct Picker {
    selected: Option<usize>,
}

impl App for Picker {
    fn build(&self, _cx: &BuildCx) -> El {
        let rows: Vec<El> = (0..30)
            .map(|i| {
                let key = format!("row-{i}");
                let mut r = row([
                    badge(format!("#{i}")).info(),
                    text(format!("Item {i}")).bold(),
                    spacer(),
                    text(if Some(i) == self.selected {
                        "selected"
                    } else {
                        ""
                    })
                    .muted(),
                ])
                .gap(tokens::SPACE_2)
                .height(Size::Fixed(44.0))
                .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
                .key(key)
                .stroke(tokens::BORDER)
                .radius(tokens::RADIUS_SM);
                if Some(i) == self.selected {
                    r = r.fill(tokens::CARD);
                }
                r
            })
            .collect();

        column([
            h2("Scrollable list"),
            text("Wheel inside the panel. Click a row to select.").muted(),
            scroll(rows)
                .key("items")
                .height(Size::Fill(1.0))
                .padding(tokens::SPACE_2),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if let (UiEventKind::Click | UiEventKind::Activate, Some(k)) = (event.kind, event.route())
            && let Some(rest) = k.strip_prefix("row-")
            && let Ok(i) = rest.parse::<usize>()
        {
            self.selected = Some(i);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 560.0, 480.0);
    damascene_winit_wgpu::run(
        "Damascene — scroll_list",
        viewport,
        Picker { selected: None },
    )
}
