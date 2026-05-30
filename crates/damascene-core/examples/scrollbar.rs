//! scrollbar — demonstrates the default-on scrollbar thumb.
//!
//! Three side-by-side scrollables in a row:
//!
//! - `scroll(...)` — overflowing list, default-on thumb visible.
//! - `virtual_list(...)` — 200 rows in a 240px viewport; thumb scales
//!   to viewport/content ratio.
//! - `scroll(...).no_scrollbar()` — same content as the first, but
//!   author opted out: no thumb.
//!
//! Inspect `out/scrollbar.draw_ops.txt` — the first two scrollables
//! emit a `Quad shader=stock::rounded_rect ... id=root....scrollbar-thumb`
//! after their children; the third does not. The SVG fallback paints
//! the same rounded thumb rect.
//!
//! Run: `cargo run -p damascene-core --example scrollbar`

use damascene_core::prelude::*;

fn list_rows() -> Vec<El> {
    (0..40)
        .map(|i| {
            row([
                text(format!("{i:02}.")).mono().muted(),
                text(format!("scrollable list item {i}")),
            ])
            .gap(tokens::SPACE_2)
            .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
            .height(Size::Fixed(28.0))
            .align(Align::Center)
        })
        .collect()
}

fn fixture() -> El {
    column([
        h2("Scrollbar"),
        text("scroll() and virtual_list() show a draggable thumb by default.").muted(),
        row([
            // 1) scroll() — default-on scrollbar.
            column([
                text("scroll() — default").bold(),
                scroll(list_rows())
                    .height(Size::Fixed(240.0))
                    .padding(tokens::SPACE_2)
                    .stroke(tokens::BORDER)
                    .stroke_width(1.0)
                    .radius(tokens::RADIUS_MD),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
            // 2) virtual_list — thumb scales to content size.
            column([
                text("virtual_list(200, 28)").bold(),
                virtual_list(200, 28.0, |i| {
                    row([
                        text(format!("{i:03}")).mono().muted(),
                        text(format!("row {i}")),
                    ])
                    .gap(tokens::SPACE_2)
                    .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
                    .height(Size::Fixed(28.0))
                    .align(Align::Center)
                })
                .height(Size::Fixed(240.0))
                .padding(tokens::SPACE_2)
                .stroke(tokens::BORDER)
                .stroke_width(1.0)
                .radius(tokens::RADIUS_MD),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
            // 3) Opt-out: same content, no thumb.
            column([
                text("scroll().no_scrollbar()").bold(),
                scroll(list_rows())
                    .no_scrollbar()
                    .height(Size::Fixed(240.0))
                    .padding(tokens::SPACE_2)
                    .stroke(tokens::BORDER)
                    .stroke_width(1.0)
                    .radius(tokens::RADIUS_MD),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_4)
        .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 960.0, 360.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "scrollbar")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
