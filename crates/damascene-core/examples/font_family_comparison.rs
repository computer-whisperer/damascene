//! font_family_comparison — quick typography comparison fixture.
//!
//! Run:
//! `cargo run -p damascene-core --example font_family_comparison`

use damascene_core::prelude::*;

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 980.0, 680.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let variants = [
        (
            "font_family_comparison.roboto",
            Theme::damascene_dark().with_font_family(FontFamily::Roboto),
        ),
        (
            "font_family_comparison.inter",
            Theme::damascene_dark().with_font_family(FontFamily::Inter),
        ),
    ];

    for (name, theme) in variants {
        let mut root = comparison_screen(theme.font_family());
        let bundle = render_bundle_themed(&mut root, viewport, &theme);
        let written = write_bundle(&bundle, &out_dir, name)?;
        for p in &written {
            println!("wrote {}", p.display());
        }
        if !bundle.lint.findings.is_empty() {
            eprintln!(
                "\nlint findings for {name} ({}):",
                bundle.lint.findings.len()
            );
            eprint!("{}", bundle.lint.text());
        }
    }

    Ok(())
}

fn comparison_screen(family: FontFamily) -> El {
    column([
        row([
            column([
                h1(format!("{family:?} typography")),
                text("Same Damascene tree, same density metrics, different proportional UI face.")
                    .muted(),
            ])
            .gap(tokens::SPACE_1)
            .height(Size::Hug),
            spacer(),
            badge(format!("{family:?}")).info(),
        ])
        .height(Size::Fixed(64.0))
        .align(Align::Center),
        row([
            column([kpi_row(), command_surface()])
                .gap(tokens::SPACE_4)
                .width(Size::Fill(1.0)),
            column([copy_card(), table_card()])
                .gap(tokens::SPACE_4)
                .width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_4)
        .height(Size::Fill(1.0)),
    ])
    .key("metric:root")
    .padding(tokens::SPACE_7)
    .gap(tokens::SPACE_4)
    .fill_size()
    .fill(tokens::BACKGROUND)
}

fn kpi_row() -> El {
    row([
        kpi_card(
            "Latency",
            "42 ms",
            "+8.2%",
            "Moving in the expected direction",
        ),
        kpi_card("Revenue", "$1,250.00", "+12.5%", "Trending up this month"),
    ])
    .gap(tokens::SPACE_3)
    .height(Size::Hug)
}

fn kpi_card(
    title: &'static str,
    value: &'static str,
    delta: &'static str,
    note: &'static str,
) -> El {
    card([
        card_header([row([card_title(title), spacer(), badge(delta).success()])
            .align(Align::Center)
            .gap(tokens::SPACE_2)]),
        card_content([
            text(value)
                .key(format!("metric:kpi.value.{title}"))
                .font_size(tokens::TEXT_3XL.size)
                .line_height(tokens::TEXT_3XL.line_height)
                .font_weight(FontWeight::Bold),
            text(note).muted().ellipsis(),
        ])
        .gap(tokens::SPACE_6),
    ])
    .metrics_role(MetricsRole::Card)
    .width(Size::Fill(1.0))
}

fn command_surface() -> El {
    titled_card(
        "Command surface",
        [
            text_input("font:search", "Search commands...", &Selection::default()),
            column([
                menu_row("git-branch", "New branch", "Ctrl+B"),
                menu_row("git-commit", "Commit staged files", "Ctrl+Enter"),
                menu_row("refresh-cw", "Refresh repository", "Ctrl+R"),
            ])
            .fill(tokens::CARD)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_MD)
            .padding(tokens::SPACE_1)
            .gap(0.0),
        ],
    )
}

fn menu_row(icon_name: &'static str, label: &'static str, shortcut: &'static str) -> El {
    row([
        icon(icon_name).muted(),
        text(label).ellipsis(),
        spacer(),
        text(shortcut).mono().caption().muted(),
    ])
    .align(Align::Center)
    .metrics_role(MetricsRole::MenuItem)
}

fn copy_card() -> El {
    titled_card(
        "Body rhythm",
        [
            text("The goal is not decorative typography. It is boring operational UI that reads as deliberate at 12 to 16 pixels.")
                .wrap_text()
                .muted(),
            text("Look for value width, label crispness, and whether headings feel modern without becoming too airy.")
                .wrap_text(),
            row([
                button("Cancel").secondary(),
                button("Save changes").primary(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ],
    )
}

fn table_card() -> El {
    titled_card(
        "Table probe",
        [table([
            table_header([table_row([
                table_head("Surface").width(Size::Fill(1.0)),
                table_head("Owner").width(Size::Fixed(82.0)),
                table_head("State").width(Size::Fixed(92.0)),
            ])]),
            divider(),
            table_body([
                table_row([
                    table_cell("Dashboard").width(Size::Fill(1.0)),
                    table_cell("Alicia").muted().width(Size::Fixed(82.0)),
                    table_cell(badge("Ready").success()).width(Size::Fixed(92.0)),
                ]),
                table_row([
                    table_cell("Command menu").width(Size::Fill(1.0)),
                    table_cell("Noah").muted().width(Size::Fixed(82.0)),
                    table_cell(badge("Review").warning()).width(Size::Fixed(92.0)),
                ]),
            ]),
        ])],
    )
}
