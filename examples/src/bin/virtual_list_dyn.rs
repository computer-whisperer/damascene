//! Variable-height virtual list — proof of `virtual_list_dyn`.
//!
//! 5,000 rows whose heights vary by index pattern: short single-line
//! rows, taller multi-line rows, and "expanded" rows with extra
//! content. The estimate is set deliberately wrong (a flat 60px) so
//! the cache-warming behavior is visible: scrolling into a never-seen
//! region momentarily reflows, then stabilizes.
//!
//! Click any row to bump a counter — proves the realized rects route
//! hit-tests despite varying heights.
//!
//! Run: `cargo run -p damascene-examples --bin virtual_list_dyn`

use damascene_core::prelude::*;

const ROW_COUNT: usize = 5_000;
const ESTIMATED_ROW_HEIGHT: f32 = 60.0;

struct VariableVirtualListApp {
    last_clicked: Option<usize>,
    clicks: u32,
}

fn build_row(i: usize) -> El {
    let body = match i % 7 {
        0 => column([
            text(format!("#{i:05} — expanded entry")).mono(),
            text("Two extra lines of context. Pretend this is a diff hunk header,")
                .muted()
                .text_wrap(TextWrap::Wrap),
            text("a comment thread, or a commit message wrapping over a couple of lines.")
                .muted()
                .text_wrap(TextWrap::Wrap),
        ])
        .gap(tokens::SPACE_1),
        3 => column([
            text(format!("#{i:05} — taller row")).mono(),
            text("One extra line of detail.").muted(),
        ])
        .gap(tokens::SPACE_1),
        _ => row([
            text(format!("#{i:05}")).mono(),
            spacer(),
            text(format!("entry {i}")),
        ])
        .gap(tokens::SPACE_3),
    };

    column([body])
        .key(format!("row-{i}"))
        .focusable()
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

impl App for VariableVirtualListApp {
    fn build(&self, _cx: &BuildCx) -> El {
        let header = match self.last_clicked {
            Some(i) => format!("clicked row {i} ({} times total)", self.clicks),
            None => format!("{ROW_COUNT} variable-height rows · scroll · click any row"),
        };
        column([
            h1("Variable-height virtual list"),
            text(header).muted(),
            virtual_list_dyn(
                ROW_COUNT,
                ESTIMATED_ROW_HEIGHT,
                |i| format!("row-{i}"),
                build_row,
            )
            .key("entries")
            .height(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
    }

    fn on_event(&mut self, event: UiEvent) {
        if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
            return;
        }
        let key = match event.route() {
            Some(k) => k,
            None => return,
        };
        if let Some(idx_str) = key.strip_prefix("row-")
            && let Ok(idx) = idx_str.parse::<usize>()
        {
            self.last_clicked = Some(idx);
            self.clicks += 1;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 600.0, 540.0);
    damascene_winit_wgpu::run(
        "Damascene — variable-height virtual list",
        viewport,
        VariableVirtualListApp {
            last_clicked: None,
            clicks: 0,
        },
    )
}
