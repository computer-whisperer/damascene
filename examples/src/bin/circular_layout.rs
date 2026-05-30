//! Circular layout — interactive proof of the custom-layout escape hatch.
//!
//! The compass-rose tree from `damascene-core/examples/circular_layout` runs
//! through the reusable `damascene-winit-wgpu` host. Hover, press, and click route
//! through the same hit-test that the column/row path uses — proving
//! that interaction works off whatever rects a [`LayoutFn`] produces,
//! not just the rects the column/row distribution produces.
//!
//! Click any compass button to bump a counter shown in the centre.
//!
//! Run: `cargo run -p damascene-examples --bin circular_layout`

use damascene_core::prelude::*;

struct Compass {
    clicks: u32,
    last: Option<&'static str>,
}

const DIRS: &[(&str, &str)] = &[
    ("North", "n"),
    ("NE", "ne"),
    ("East", "e"),
    ("SE", "se"),
    ("South", "s"),
    ("SW", "sw"),
    ("West", "w"),
    ("NW", "nw"),
];

fn circular(ctx: LayoutCtx) -> Vec<Rect> {
    let cx = ctx.container.x + ctx.container.w * 0.5;
    let cy = ctx.container.y + ctx.container.h * 0.5;
    let radius = ctx.container.w.min(ctx.container.h) * 0.38;
    let n = ctx.children.len();
    if n == 0 {
        return Vec::new();
    }
    ctx.children
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let (w, h) = (ctx.measure)(child);
            if i == 0 {
                return Rect::new(cx - w * 0.5, cy - h * 0.5, w, h);
            }
            let ring_count = (n - 1) as f32;
            let theta =
                (i - 1) as f32 / ring_count * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            Rect::new(
                cx + radius * theta.cos() - w * 0.5,
                cy + radius * theta.sin() - h * 0.5,
                w,
                h,
            )
        })
        .collect()
}

impl App for Compass {
    fn build(&self, _cx: &BuildCx) -> El {
        let centre_label = match self.last {
            Some(name) => format!("{}\n{}", name, self.clicks),
            None => "click a\nbutton".to_string(),
        };
        let mut children: Vec<El> = vec![h2(centre_label).center_text()];
        for (label, key) in DIRS {
            children.push(button(*label).key(*key).primary());
        }
        column([
            h1("Compass").center_text(),
            stack(children)
                .key("compass")
                .layout(circular)
                .width(Size::Fill(1.0))
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
        if let Some((label, _)) = DIRS.iter().find(|(_, k)| *k == key) {
            self.last = Some(label);
            self.clicks += 1;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 600.0, 540.0);
    damascene_winit_wgpu::run(
        "Damascene — circular layout",
        viewport,
        Compass {
            clicks: 0,
            last: None,
        },
    )
}
