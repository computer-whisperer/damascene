//! circular_layout — exercises the custom-layout escape hatch.
//!
//! The fixture is a `stack(...)` whose direct children are arranged on
//! the perimeter of a circle by the supplied [`LayoutFn`]. Stock paint
//! (rounded_rect for buttons, text_sdf for labels) keeps working
//! unchanged; only the rect distribution changes. Everything else —
//! intrinsic measurement of children, the recursion into each child's
//! subtree, hit-test off each node's computed rect — flows through the
//! existing library code paths.
//!
//! Inspect `out/circular_layout.tree.txt` and `.draw_ops.txt` to see
//! the paint stream the LayoutFn produced. The SVG / PNG show eight
//! buttons evenly spaced around a centered title.
//!
//! Run: `cargo run -p damascene-core --example circular_layout`

use damascene_core::prelude::*;

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
            // First child sits at the centre; others ring it.
            if i == 0 {
                let (w, h) = (ctx.measure)(child);
                return Rect::new(cx - w * 0.5, cy - h * 0.5, w, h);
            }
            let ring_count = (n - 1) as f32;
            let theta =
                (i - 1) as f32 / ring_count * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            let (w, h) = (ctx.measure)(child);
            let x = cx + radius * theta.cos() - w * 0.5;
            let y = cy + radius * theta.sin() - h * 0.5;
            Rect::new(x, y, w, h)
        })
        .collect()
}

fn fixture() -> El {
    let centre = h2("Compass").center_text();
    let dirs = [
        ("North", "n"),
        ("NE", "ne"),
        ("East", "e"),
        ("SE", "se"),
        ("South", "s"),
        ("SW", "sw"),
        ("West", "w"),
        ("NW", "nw"),
    ];

    let mut children: Vec<El> = vec![centre];
    for (label, k) in dirs {
        children.push(button(label).key(k).primary());
    }

    column([
        h1("Custom layout — circular"),
        paragraph(
            "Eight buttons positioned on a circle by an author-supplied \
             LayoutFn. Stock paint, automatic hover/press, and hit-test \
             all keep working — only the rect distribution changed.",
        )
        .muted(),
        stack(children)
            .key("compass")
            .layout(circular)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(360.0)),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 600.0, 540.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "circular_layout")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
