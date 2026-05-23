//! custom_shader — exercises the custom-shader escape hatch.
//!
//! Produces the same bundle artifacts as `settings` (SVG, tree dump,
//! draw-ops text, shader manifest, lint) for a tree that paints three
//! buttons via a registered custom WGSL shader (`gradient.wgsl`). The
//! SVG fallback emits dashed-magenta placeholders for those buttons —
//! that's the documented behavior for `ShaderHandle::Custom`. The wgpu
//! PNG shows the actual gradient pixels.
//!
//! Inspection of `out/custom_shader.shader_manifest.txt` is the point —
//! it lists `custom::gradient` alongside the stock shaders, with the
//! per-instance uniforms each binding sets.
//!
//! Run: `cargo run -p aetna-core --example custom_shader`

use aetna_core::prelude::*;

/// Helper: a button-shaped El whose surface paint is the registered
/// `gradient` shader instead of stock::rounded_rect. The shader's vec_a
/// slot is read as the top color, vec_b as the bottom, vec_c.x as the
/// corner radius.
fn gradient_button(label: &str, top: Color, bottom: Color, radius: f32) -> El {
    button(label).text_color(tokens::PRIMARY_FOREGROUND).shader(
        ShaderBinding::custom("gradient")
            .color("vec_a", top)
            .color("vec_b", bottom)
            .f32("vec_c", radius),
    )
}

fn fixture() -> El {
    column([
        h1("Custom shader demo"),
        paragraph(
            "Three buttons below paint via a registered custom shader \
             (gradient.wgsl). The right-hand button is a stock rounded_rect \
             for contrast.",
        )
        .muted(),
        titled_card(
            "gradient.wgsl — vertical linear gradient",
            [row([
                gradient_button(
                    "Sunrise",
                    Color::srgb_u8(255, 200, 90),
                    Color::srgb_u8(245, 95, 110),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Ocean",
                    Color::srgb_u8(120, 200, 255),
                    Color::srgb_u8(40, 90, 200),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Forest",
                    Color::srgb_u8(180, 230, 140),
                    Color::srgb_u8(40, 110, 80),
                    tokens::RADIUS_MD,
                ),
                spacer(),
                button("Stock").secondary(),
            ])
            .gap(tokens::SPACE_3)],
        ),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 720.0, 360.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "custom_shader")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
