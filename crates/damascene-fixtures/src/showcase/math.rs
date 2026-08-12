//! Math — editable markdown math playground.
//!
//! The Typography page carries a compact markdown math sample. This page is
//! the live workbench: edit markdown/TeX on the left and the preview re-renders
//! through the same native math path on the right.

use damascene_core::prelude::*;
use damascene_core::selection::SelectionSource;
use damascene_markdown::{MarkdownOptions, md_with_options};

const SOURCE_KEY: &str = "math-source";

const EULER_KEY: &str = "math-preset-euler";
const FRACTION_KEY: &str = "math-preset-fraction";
const MATRIX_KEY: &str = "math-preset-matrix";
const LIMITS_KEY: &str = "math-preset-limits";
const LOGIC_KEY: &str = "math-preset-logic";
const ERRORS_KEY: &str = "math-preset-errors";
const STRESS_KEY: &str = "math-preset-stress";

const DEFAULT_SOURCE: &str = "\
# Native math

Inline math rides the same baseline as prose: $e^{i\\pi}+1=0$ and \
$x_1+x_2$.

$$
\\frac{a^2+b^2}{\\sqrt{x_1+x_2}} + \\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}
$$
";

const EULER_SOURCE: &str = "\
# Euler identity

Inline: $e^{i\\pi}+1=0$.

$$
e^{i\\pi}+1=0
$$
";

const FRACTION_SOURCE: &str = "\
# Fractions and radicals

Inline: $\\frac{1}{2}$, $\\sqrt{x_1+x_2}$, and $\\sqrt[3]{x+1}$.

$$
\\frac{\\sum_{i=1}^{n}x_i^2}{\\sqrt{x_1+x_2+x_3+x_4}}
$$
";

const MATRIX_SOURCE: &str = "\
# Matrices and fences

$$
\\left[\\begin{array}{lcr}x&10&\\alpha\\\\xx&2&\\beta\\\\xxx&300&\\gamma\\end{array}\\right]
$$

$$
\\left\\{\\begin{matrix}a+b\\\\\\frac{c}{d}\\\\\\sqrt{x+y}\\\\\\sum_{i=1}^{n}i\\end{matrix}\\right.
$$
";

const LIMITS_SOURCE: &str = "\
# Large operator limits

Inline large operators stay compact beside prose: $\\sum_{i=1}^{n}x_i$.

$$
\\sum_{i=1}^{n}x_i + \\prod_{k=1}^{m}(1+x_k) + \\bigcup_{j=1}^{r}A_j
$$

$$
\\int_0^1 f(x)dx
$$
";

const LOGIC_SOURCE: &str = "\
# Logic, sets, and relations

Quantifiers and implications: $\\forall x \\in \\mathbb{R},\\ \\exists n \\in \\mathbb{N}$ \
with $n > x \\implies n + 1 > x$.

$$
\\forall \\epsilon > 0,\\ \\exists \\delta > 0\\ :\\
|x - a| < \\delta \\implies |f(x) - f(a)| < \\epsilon
$$

$$
A \\subseteq B \\iff \\forall x\\ (x \\in A \\implies x \\in B)
$$

$$
\\gcd(a, b) \\cdot \\mathrm{lcm}(a, b) = a \\cdot b,
\\qquad \\Pr(A \\cup B) \\leq \\Pr(A) + \\Pr(B)
$$

$$
\\liminf_{n \\to \\infty} a_n
\\leq \\limsup_{n \\to \\infty} a_n
$$

$$
\\bigoplus_{i=1}^{n} V_i \\cong \\bigotimes_{j=1}^{m} W_j,
\\qquad \\oint_{\\partial \\Omega} \\omega = \\iint_{\\Omega} d\\omega
$$
";

const ERRORS_SOURCE: &str = "\
# Parser errors

Malformed inline source becomes an explicit math error: $\\frac{a}{$.

Malformed display source does the same:

$$
\\begin{bmatrix}a&b\\\\c\\end{bmatrix}
$$
";

const STRESS_SOURCE: &str = "\
# Stress cases

This paragraph puts built-up math near line boundaries: \
$\\frac{a+b}{c+d}$ should keep its baseline, \
$\\sqrt{x_1+x_2+x_3+x_4}$ should stay atomic, and \
$\\left(\\frac{a}{b}\\right)$ should not create odd prose gaps.

Symbols: $\\alpha+\\beta\\to\\gamma$, $\\Delta\\le\\Omega$, and \
$A\\cup B\\cap C\\ne\\emptyset$.

$$
\\begin{aligned}
S &= \\sum_{k=0}^{n} r^k = 1 + r + r^2 + \\cdots + r^n \\\\
rS &= r + r^2 + \\cdots + r^{n+1} \\\\
S-rS &= 1-r^{n+1}
\\end{aligned}
$$

$$
\\nabla \\times \\mathbf{B} =
\\mu_0 \\mathbf{J} + \\mu_0 \\varepsilon_0
\\frac{\\partial \\mathbf{E}}{\\partial t}
$$

$$
\\mathbb{E}[X] = \\int_{-\\infty}^{\\infty} x\\, f(x)\\, dx,
\\qquad \\operatorname{Var}(X) = \\mathbb{E}[X^2] - (\\mathbb{E}[X])^2
$$

$$
R(\\theta) = \\begin{pmatrix}
\\cos\\theta & -\\sin\\theta \\\\
\\sin\\theta & \\cos\\theta
\\end{pmatrix}
$$
";

#[derive(Clone, Copy)]
struct Preset {
    key: &'static str,
    label: &'static str,
    source: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        key: EULER_KEY,
        label: "Euler",
        source: EULER_SOURCE,
    },
    Preset {
        key: FRACTION_KEY,
        label: "Fractions",
        source: FRACTION_SOURCE,
    },
    Preset {
        key: MATRIX_KEY,
        label: "Matrices",
        source: MATRIX_SOURCE,
    },
    Preset {
        key: LIMITS_KEY,
        label: "Limits",
        source: LIMITS_SOURCE,
    },
    Preset {
        key: LOGIC_KEY,
        label: "Logic",
        source: LOGIC_SOURCE,
    },
    Preset {
        key: ERRORS_KEY,
        label: "Errors",
        source: ERRORS_SOURCE,
    },
    Preset {
        key: STRESS_KEY,
        label: "Wrapping",
        source: STRESS_SOURCE,
    },
];

pub struct State {
    pub source: String,
    pub selection: Selection,
    pub scroll_caret_into_view: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            source: DEFAULT_SOURCE.into(),
            selection: Selection::default(),
            scroll_caret_into_view: false,
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    // Phone stacks the editor + preview cards vertically so each gets
    // the full content width; horizontal split leaves both too narrow
    // for the markdown preview headings to render without overflow.
    let editor_preview: El = if phone {
        column([editor_card(state), preview_card(state)])
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
    } else {
        row([editor_card(state), preview_card(state)])
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .width(Size::Fill(1.0))
    };
    scroll([column([
        h1("Math"),
        paragraph(
            "Native math starts in markdown but renders through Damascene's \
             own presentation IR, box layout, and draw ops. Edit the \
             source to exercise inline math, display math, fences, \
             matrices, scripts, and parser errors.",
        )
        .muted(),
        preset_bar(state, phone),
        editor_preview,
        h2("Coverage"),
        coverage_grid(phone),
    ])
    .gap(tokens::SPACE_4)
    .width(Size::Fill(1.0))
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if e.kind == UiEventKind::SelectionChanged
        && let Some(sel) = e.selection.as_ref()
    {
        state.selection = sel.clone();
        return;
    }

    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate)
        && let Some(route) = e.route()
        && let Some(preset) = PRESETS.iter().find(|preset| preset.key == route)
    {
        state.source = preset.source.into();
        state.selection = Selection::default();
        return;
    }

    if e.target_key() == Some(SOURCE_KEY)
        && text_area::apply_event(&mut state.source, &mut state.selection, &e, SOURCE_KEY)
    {
        state.scroll_caret_into_view = true;
    }
}

pub fn drain_scroll_requests(state: &mut State) -> Vec<damascene_core::scroll::ScrollRequest> {
    if std::mem::take(&mut state.scroll_caret_into_view)
        && let Some(req) =
            text_area::caret_scroll_request_for(&state.source, &state.selection, SOURCE_KEY)
    {
        vec![req]
    } else {
        Vec::new()
    }
}

fn preset_bar(state: &State, phone: bool) -> El {
    let buttons = row(PRESETS.iter().map(|preset| {
        let active = state.source == preset.source;
        let button = button(preset.label).xsmall().key(preset.key);
        if active {
            button.primary()
        } else {
            button.secondary()
        }
    }))
    .gap(tokens::SPACE_2);
    // Seven preset buttons can't all fit a 360px phone viewport. Wrap
    // the strip in a horizontal scroll so users can swipe through them
    // without losing access to any preset. The inner row carries
    // RING_WIDTH vertical padding so the buttons' focus ring band sits
    // inside the scroll's clip rect instead of being scissored.
    let strip: El = if phone {
        scroll([buttons
            .width(Size::Hug)
            .padding(Sides::xy(0.0, tokens::RING_WIDTH))])
        .axis(Axis::Row)
        .height(Size::Hug)
        .width(Size::Fill(1.0))
    } else {
        buttons.width(Size::Fill(1.0))
    };
    row([text("Presets").label().muted(), strip])
        .gap(tokens::SPACE_3)
        .align(Align::Center)
        .width(Size::Fill(1.0))
}

fn editor_card(state: &State) -> El {
    card([
        card_header([
            card_title("Source"),
            card_description("Markdown with TeX math enabled."),
        ]),
        card_content([text_area(SOURCE_KEY, &state.source, &state.selection)
            .aria_label("Math source")
            .height(Size::Fixed(330.0))]),
    ])
    .width(Size::Fill(1.0))
}

fn preview_card(state: &State) -> El {
    card([
        card_header([
            card_title("Preview"),
            card_description("Rendered with MarkdownOptions::math(true)."),
        ]),
        card_content([scroll([md_with_options(
            &state.source,
            MarkdownOptions::default().math(true),
        )])
        .key("math-preview")
        .height(Size::Fixed(330.0))]),
    ])
    .width(Size::Fill(1.0))
}

fn coverage_grid(phone: bool) -> El {
    // Each coverage card holds a math equation rendered at a fixed font
    // size — at half-viewport on phone, several of those equations
    // would exceed their card width. Stack one card per row on phone so
    // each gets full content width.
    if phone {
        column([
            nested_roots_card(),
            large_operator_card(),
            mathml_import_card(),
            malformed_source_card(),
        ])
        .gap(tokens::SPACE_3)
        .width(Size::Fill(1.0))
    } else {
        column([
            row([nested_roots_card(), large_operator_card()])
                .gap(tokens::SPACE_4)
                .align(Align::Stretch)
                .width(Size::Fill(1.0)),
            row([mathml_import_card(), malformed_source_card()])
                .gap(tokens::SPACE_4)
                .align(Align::Stretch)
                .width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_4)
        .width(Size::Fill(1.0))
    }
}

fn nested_roots_card() -> El {
    demo_card(
        "Nested roots",
        "Radical variants, root indices, and overbars through nested TeX roots.",
        [
            selectable_tex_block("math-nested-root-a", r"\sqrt{1+\sqrt{x+\sqrt[3]{y+1}}}"),
            selectable_tex_block(
                "math-nested-root-b",
                r"\sqrt[12]{\frac{1+\sqrt{x}}{1+\sqrt[3]{y+\sqrt{z}}}}",
            )
            .font_size(18.0),
        ],
    )
}

fn large_operator_card() -> El {
    demo_card(
        "Limit sizing",
        "Display operators use font variants; integrals keep side scripts.",
        [
            selectable_tex_block(
                "math-large-operator-small",
                r"\sum_{i=1}^{n}x_i+\prod_{k=1}^{m}(1+x_k)",
            )
            .font_size(18.0),
            selectable_tex_block(
                "math-large-operator-large",
                r"\sum_{i=1}^{n}x_i+\prod_{k=1}^{m}(1+x_k)",
            )
            .font_size(26.0),
            selectable_tex_block("math-integral", r"\int_0^1 f(x)dx").font_size(26.0),
        ],
    )
}

fn mathml_import_card() -> El {
    let source = r#"
<math display="block">
  <mfenced open="[" close="]">
    <mtable columnalign="left right" columnspacing="0.6em" rowspacing="0.2em">
      <mtr><mtd><mi>x</mi></mtd><mtd><mn>10</mn></mtd></mtr>
      <mtr><mtd><msup><mi>x</mi><mn>2</mn></msup></mtd><mtd><mn>200</mn></mtd></mtr>
    </mtable>
  </mfenced>
</math>
"#;
    demo_card(
        "MathML import",
        "Presentation MathML table, spacing, alignment, and fence.",
        [selectable_math_block(
            "math-mathml-import",
            source.trim(),
            mathml_or_error(source),
        )],
    )
}

fn malformed_source_card() -> El {
    demo_card(
        "Malformed source",
        "Parser failures render as math error expressions.",
        [selectable_tex_block(
            "math-malformed-source",
            r"\sqrt{1+\frac{x}{",
        )],
    )
}

fn demo_card<const N: usize>(title: &'static str, description: &'static str, body: [El; N]) -> El {
    card([
        card_header([
            card_title(title).wrap_text().fill_width(),
            card_description(description),
        ]),
        card_content([column(body)
            .gap(tokens::SPACE_3)
            .align(Align::Center)
            .width(Size::Fill(1.0))]),
    ])
    .width(Size::Fill(1.0))
}

fn tex_or_error(source: &str) -> MathExpr {
    parse_tex(source)
        .unwrap_or_else(|err| MathExpr::Error(format!("math parse error: {}", err.message)))
}

fn selectable_tex_block(key: &'static str, source: &'static str) -> El {
    selectable_math_block(key, source, tex_or_error(source))
}

fn selectable_math_block(key: &'static str, source: &str, expr: MathExpr) -> El {
    let visible = "\u{fffc}";
    let mut selection_source = SelectionSource::new(source.to_string(), visible);
    selection_source.push_span(0..visible.len(), 0..source.len(), true);
    math_block(expr)
        .key(key)
        .selectable()
        .selection_source(selection_source)
}

fn mathml_or_error(source: &str) -> MathExpr {
    parse_mathml(source)
        .unwrap_or_else(|err| MathExpr::Error(format!("mathml parse error: {}", err.message)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    fn collect_math_errors(el: &El, errors: &mut Vec<String>) {
        if let Some(expr) = el.math.as_deref() {
            collect_expr_errors(expr, errors);
        }
        for child in &el.children {
            collect_math_errors(child, errors);
        }
    }

    fn collect_expr_errors(expr: &MathExpr, errors: &mut Vec<String>) {
        match expr {
            MathExpr::Error(error) => errors.push(error.clone()),
            MathExpr::Row(children) => {
                for child in children {
                    collect_expr_errors(child, errors);
                }
            }
            MathExpr::Fraction {
                numerator,
                denominator,
            } => {
                collect_expr_errors(numerator, errors);
                collect_expr_errors(denominator, errors);
            }
            MathExpr::Sqrt(child) => collect_expr_errors(child, errors),
            MathExpr::Root { base, index } => {
                collect_expr_errors(base, errors);
                collect_expr_errors(index, errors);
            }
            MathExpr::Scripts { base, sub, sup } => {
                collect_expr_errors(base, errors);
                if let Some(sub) = sub {
                    collect_expr_errors(sub, errors);
                }
                if let Some(sup) = sup {
                    collect_expr_errors(sup, errors);
                }
            }
            MathExpr::UnderOver { base, under, over } => {
                collect_expr_errors(base, errors);
                if let Some(under) = under {
                    collect_expr_errors(under, errors);
                }
                if let Some(over) = over {
                    collect_expr_errors(over, errors);
                }
            }
            MathExpr::Accent { base, accent, .. } => {
                collect_expr_errors(base, errors);
                collect_expr_errors(accent, errors);
            }
            MathExpr::Fenced { body, .. } => collect_expr_errors(body, errors),
            MathExpr::Table { rows, .. } => {
                for row in rows {
                    for cell in row {
                        collect_expr_errors(cell, errors);
                    }
                }
            }
            MathExpr::Source { body, .. } => collect_expr_errors(body, errors),
            MathExpr::Identifier(_)
            | MathExpr::Number(_)
            | MathExpr::Operator(_)
            | MathExpr::OperatorWithMetadata { .. }
            | MathExpr::Text(_)
            | MathExpr::Space(_) => {}
            _ => {}
        }
    }

    #[test]
    fn preset_click_replaces_source() {
        let mut state = State::default();

        on_event(&mut state, click(MATRIX_KEY));

        assert_eq!(state.source, MATRIX_SOURCE);
        assert_eq!(state.selection, Selection::default());
    }

    #[test]
    fn error_preset_is_available_from_the_preset_bar() {
        let mut state = State::default();

        on_event(&mut state, click(ERRORS_KEY));

        assert_eq!(state.source, ERRORS_SOURCE);
    }

    #[test]
    fn mathml_showcase_sample_imports_successfully() {
        let expr = mathml_or_error(
            r#"<math><mfenced open="[" close="]"><mtable><mtr><mtd><mi>x</mi></mtd></mtr></mtable></mfenced></math>"#,
        );

        assert!(!matches!(expr, MathExpr::Error(_)));
    }

    #[test]
    fn stress_preset_markdown_has_no_math_parse_errors() {
        let doc = md_with_options(STRESS_SOURCE, MarkdownOptions::default().math(true));
        let mut errors = Vec::new();
        collect_math_errors(&doc, &mut errors);

        assert!(errors.is_empty(), "unexpected math errors: {errors:?}");
    }
}
