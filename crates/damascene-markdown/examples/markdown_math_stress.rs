//! markdown_math_stress — focused visual coverage for native math layout.
//!
//! Run: `cargo run -p damascene-markdown --example markdown_math_stress`

use damascene_core::prelude::*;
use damascene_markdown::{MarkdownOptions, md_with_options};

const WRAPPING_SOURCE: &str = "\
## Narrow inline wrapping

This paragraph intentionally puts built-up math near line boundaries: \
$\\frac{a+b}{c+d}$ should keep its baseline, \
$\\sqrt{x_1+x_2+x_3+x_4}$ should stay atomic, and \
$\\left(\\frac{a}{b}\\right)$ should not force adjacent prose into odd gaps. \
The follow-up expression $y_{n+1}=y_n+x^2$ checks nested script spacing after a wrap.

Greek and operator fallback should remain readable inline: \
$\\alpha+\\beta\\to\\gamma$, $\\Delta\\le\\Omega$, and \
$A\\cup B\\cap C\\neq\\emptyset$.
";

const DISPLAY_SOURCE: &str = "\
## Display structures

$$
\\frac{\\sum_{i=1}^{n} x_i^2}{\\sqrt{x_1+x_2+x_3+x_4+x_5}} + \\sqrt[3]{\\frac{a+b}{c+d}}
$$

$$
\\left[\\begin{array}{lcr}x&10&\\alpha\\\\xx&2&\\beta\\\\xxx&300&\\gamma\\end{array}\\right]
$$

$$
\\left\\{\\begin{matrix}a+b\\\\\\frac{c}{d}\\\\\\sqrt{x+y}\\\\\\sum_{i=1}^{n}i\\end{matrix}\\right.
$$
";

fn tex(source: &str) -> MathExpr {
    parse_tex(source).unwrap_or_else(|err| {
        MathExpr::Error(format!("math parse error at {}: {}", err.byte, err.message))
    })
}

fn inline_size_row(label: &str, size: f32) -> El {
    text_runs([
        text(label).font_size(size),
        text("  fraction ").font_size(size),
        math_inline(tex(r"\frac{1}{2}")).font_size(size),
        text(", radical ").font_size(size),
        math_inline(tex(r"\sqrt{x_1+x_2}")).font_size(size),
        text(", scripts ").font_size(size),
        math_inline(tex(r"x_{n+1}^{2}")).font_size(size),
        text(".").font_size(size),
    ])
    .wrap_text()
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn fixture() -> El {
    column([
        h1("Native Math Stress"),
        paragraph("Focused fixtures for inline wrapping, built-up inline atoms, fallback symbols, and display structures."),
        column([
            inline_size_row("12 px", 12.0),
            inline_size_row("16 px", 16.0),
            inline_size_row("22 px", 22.0),
            inline_size_row("30 px", 30.0),
        ])
        .gap(tokens::SPACE_3)
        .width(Size::Fixed(460.0)),
        divider(),
        md_with_options(WRAPPING_SOURCE, MarkdownOptions::default().math(true))
            .width(Size::Fixed(420.0)),
        divider(),
        md_with_options(DISPLAY_SOURCE, MarkdownOptions::default().math(true)),
    ])
    .gap(tokens::SPACE_5)
    .padding(tokens::SPACE_7)
    .width(Size::Fixed(680.0))
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 680.0, 1120.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "markdown_math_stress")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
