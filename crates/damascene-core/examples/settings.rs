//! settings — moderately rich UI fixture.
//!
//! Demonstrates: cards, button variants, badges, status colors, ghost
//! buttons in a save-row pattern. Should look idiomatic to an LLM that
//! has seen shadcn/Tailwind in training.
//!
//! Visuals route through `stock::rounded_rect` and `stock::text_sdf`
//! shaders. SVG output is approximate; the `shader_manifest.txt`
//! artifact shows which shader paints what.
//!
//! Produces a full agent bundle: SVG, tree dump, draw ops, shader
//! manifest, lint. Run: `cargo run -p damascene-core --example settings`

use damascene_core::prelude::*;

fn settings() -> El {
    column([
        h1("Settings"),
        titled_card(
            "Account",
            [
                row([text("Email"), spacer(), text("user@example.com").muted()]),
                row([
                    text("Two-factor authentication"),
                    spacer(),
                    badge("Enabled").success(),
                ]),
                row([
                    text("Recovery codes"),
                    spacer(),
                    button("Generate").secondary(),
                ]),
            ],
        ),
        titled_card(
            "Appearance",
            [
                row([text("Theme"), spacer(), button("Dark").secondary()]),
                row([text("Compact mode"), spacer(), badge("Off").muted()]),
                row([text("Font size"), spacer(), text("14")]),
            ],
        ),
        titled_card(
            "Danger zone",
            [row([
                column([
                    text("Delete account").bold(),
                    text("Permanently remove your account and all data.")
                        .muted()
                        .small(),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Start)
                .width(Size::Hug),
                spacer(),
                button("Delete").destructive(),
            ])
            .align(Align::Center)],
        ),
        row([spacer(), button("Cancel").ghost(), button("Save").primary()]).gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn main() -> std::io::Result<()> {
    let mut root = settings();

    let viewport = Rect::new(0.0, 0.0, 720.0, 760.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "settings")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
