//! review — the headless review loop, as a copy-paste scaffold.
//!
//! Damascene apps don't need a GPU or a window to be looked at. Any
//! [`App`] builds through layout, paint, and the lint pass headlessly,
//! producing an SVG you can open and a lint report you can read. That is
//! the cheapest way to catch the layout and styling slips that otherwise
//! only show up at paint — raw colors, text overflow, missing surface
//! fills, focus rings clipped by a scroll viewport, duplicate ids.
//!
//! Treat it the way you'd treat a compiler warning: render after each
//! change, open the SVG, and keep the lint clean. Run it here with
//!
//! ```text
//! cargo run -p damascene-core --example review
//! ```
//!
//! and it writes `out/review.{svg,tree.txt,draw_ops.txt,lint.txt}` next
//! to the crate, prints where, then prints the lint report. It exits
//! non-zero if the lint finds anything, so the same `main` works as a
//! CI gate.
//!
//! ## Copy this into your own crate
//!
//! Drop a near-identical file under your crate's `examples/` (or fold
//! the body into a `#[test]` and `assert!(bundle.lint.findings
//! .is_empty(), "{}", bundle.lint.text())`). Swap `ReviewDemo` for your
//! own [`App`] and point it at the canned states worth reviewing —
//! empty states, long lists, error states. The whole loop is the three
//! calls in `main`: build the tree, [`render_bundle_themed`], then read
//! `bundle.lint`.

use damascene_core::prelude::*;

/// A small, idiomatic settings panel — enough of a real tree for the
/// lint pass to have something to say. Note the named widgets (`card`,
/// `field_row`, `switch`, `badge`) over hand-styled `column`/`row`, and
/// the explicit `gap` on the content column so stacked interactive rows
/// don't overlap their focus rings.
struct ReviewDemo {
    two_factor: bool,
    weekly_digest: bool,
}

impl App for ReviewDemo {
    fn build(&self, _cx: &BuildCx) -> El {
        let account = card([
            card_header([card_title("Account")]),
            card_content([
                field_row(
                    "Two-factor authentication",
                    row([
                        if self.two_factor {
                            badge("Enabled").success()
                        } else {
                            badge("Off").muted()
                        },
                        switch("two_factor", self.two_factor),
                    ])
                    .gap(tokens::SPACE_3)
                    .align(Align::Center)
                    .width(Size::Hug),
                ),
                separator(),
                field_row("Weekly email digest", switch("weekly_digest", self.weekly_digest)),
            ])
            .gap(tokens::SPACE_3),
        ])
        .width(Size::Fixed(420.0));

        let body = column([
            column([
                h1("Settings"),
                text("Headless review fixture — open out/review.svg to see it.").muted(),
            ])
            .gap(tokens::SPACE_1)
            .width(Size::Hug),
            account,
        ])
        .gap(tokens::SPACE_5)
        .width(Size::Hug);

        page([row([spacer(), body, spacer()]).width(Size::Fill(1.0))])
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        switch::apply_event(&mut self.two_factor, &event, "two_factor");
        switch::apply_event(&mut self.weekly_digest, &event, "weekly_digest");
    }
}

fn main() -> std::io::Result<()> {
    let app = ReviewDemo {
        two_factor: true,
        weekly_digest: false,
    };

    // The three-call loop. `theme()` defaults to `Theme::default()`;
    // override `App::theme` to review a different palette.
    let theme = app.theme();
    let viewport = Rect::new(0.0, 0.0, 720.0, 540.0);
    let mut root = app.build(&BuildCx::new(&theme).with_viewport(viewport.w, viewport.h));
    let bundle = render_bundle_themed(&mut root, viewport, &theme);

    // Write the artifacts to look at.
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "review")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    // Read the lint. Anything here is a layout/styling slip worth fixing
    // before the UI is trusted — so we surface it and fail the run.
    if bundle.lint.findings.is_empty() {
        println!("\nlint: clean");
    } else {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
        std::process::exit(1);
    }

    Ok(())
}
