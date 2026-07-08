//! polish_calibration — design-system tuning fixture.
//!
//! This is not a product screen. It is the surface where Damascene's default
//! tokens, stock widgets, typography, menus, rows, and state treatments
//! are calibrated against a realistic app screen.
//!
//! Run:
//! `cargo run -p damascene-core --example polish_calibration`

use damascene_core::prelude::*;

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 1180.0, 876.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let name = "polish_calibration";
    let theme = Theme::damascene_dark();
    let mut root = polish_calibration();
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

    Ok(())
}

fn polish_calibration() -> El {
    row([sidebar(), main_panel()])
        .key("metric:root")
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BACKGROUND)
}

fn sidebar() -> El {
    column([
        column([h2("Damascene"), text("calibration").muted()])
            .key("metric:sidebar.brand")
            .gap(tokens::SPACE_1)
            .height(Size::Hug),
        spacer().height(Size::Fixed(tokens::SPACE_4)),
        nav_item("01", "Overview", true),
        nav_item("02", "Commands", false),
        nav_item("03", "Tables", false),
        nav_item("04", "Forms", false),
        spacer(),
        badge("dark theme").muted(),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_5)
    .key("metric:sidebar")
    .width(Size::Fixed(220.0))
    .height(Size::Fill(1.0))
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
}

fn nav_item(icon: &'static str, label: &'static str, selected: bool) -> El {
    let mut item = row([
        icon_cell(icon),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .key(if selected {
        "metric:sidebar.nav.row".to_string()
    } else {
        format!("nav-{label}")
    })
    .metrics_role(MetricsRole::ListItem)
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(tokens::SPACE_2, 0.0))
    .height(Size::Fixed(40.0))
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    }

    item
}

fn main_panel() -> El {
    column([
        toolbar(),
        column([
            row([
                kpi_card("Latency", "42 ms", "-18%", true),
                kpi_card("Runs", "1,284", "+12%", true),
                kpi_card("Errors", "7", "+2", false),
            ])
            .gap(tokens::SPACE_4),
            row([table_card(), command_card()])
                .gap(tokens::SPACE_4)
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
        ])
        .gap(tokens::SPACE_4)
        .height(Size::Fill(1.0))
        .align(Align::Stretch),
    ])
    .padding(tokens::SPACE_7)
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn toolbar() -> El {
    row([
        column([
            h1("Polish calibration").key("metric:page.title"),
            text("A representative app surface for default tuning.")
                .muted()
                .key("metric:page.subtitle"),
        ])
        .gap(tokens::SPACE_2)
        .height(Size::Hug),
        spacer(),
        button_with_icon("search", "Preview")
            .secondary()
            .key("metric:action.secondary"),
        button_with_icon("upload", "Publish")
            .primary()
            .key("metric:action.primary"),
    ])
    .key("metric:header")
    .gap(tokens::SPACE_4)
    .height(Size::Hug)
    .align(Align::Start)
}

fn kpi_card(label: &'static str, value: &'static str, delta: &'static str, positive: bool) -> El {
    let delta_badge = if positive {
        badge(delta).success()
    } else {
        badge(delta).destructive()
    };
    let delta_badge = if label == "Latency" {
        delta_badge.key("metric:kpi.badge")
    } else {
        delta_badge
    };
    let value_text = h2(value).display();
    let value_text = if label == "Latency" {
        value_text.key("metric:kpi.value")
    } else {
        value_text
    };
    card([
        card_header([card_title(label)]),
        card_content([
            row([value_text, spacer(), delta_badge]).align(Align::Center),
            text(if positive {
                "Moving in the expected direction"
            } else {
                "Needs visual attention"
            })
            .muted(),
        ])
        .gap(tokens::SPACE_6),
    ])
    .key(if label == "Latency" {
        "metric:kpi.card"
    } else {
        label
    })
    .width(Size::Fill(1.0))
}

fn table_card() -> El {
    card([
        card_header([card_title("Reference rows")]),
        card_content([table([
            table_header([table_row([
                table_head("Status").width(Size::Fixed(86.0)),
                table_head("Surface").width(Size::Fill(1.0)),
                table_head("Owner").width(Size::Fixed(110.0)),
                table_head("State").width(Size::Fixed(86.0)),
            ])
            .key("metric:table.header")]),
            divider(),
            table_body([
                data_row("OK", "Settings card", "core", "selected", true, "success"),
                data_row(
                    "WARN",
                    "Command palette density",
                    "widgets",
                    "needs work",
                    false,
                    "warning",
                ),
                data_row(
                    "ERR",
                    "Disabled and invalid states",
                    "style",
                    "missing",
                    false,
                    "destructive",
                ),
                data_row(
                    "INFO",
                    "Token resolution",
                    "theme",
                    "planned",
                    false,
                    "info",
                ),
                data_row(
                    "OK",
                    "Popover elevation",
                    "shader",
                    "queued",
                    false,
                    "success",
                ),
            ])
            .gap(tokens::SPACE_1)
            .width(Size::Fill(1.0)),
        ])]),
    ])
    .key("metric:table.card")
    .width(Size::Fill(1.2))
    .height(Size::Fill(1.0))
}

fn data_row(
    status: &'static str,
    title: &'static str,
    owner: &'static str,
    state: &'static str,
    selected: bool,
    tone: &'static str,
) -> El {
    let status_badge = match tone {
        "success" => badge(status).success(),
        "warning" => badge(status).warning(),
        "destructive" => badge(status).destructive(),
        _ => badge(status).info(),
    };
    let status_badge = if selected {
        status_badge.key("metric:table.badge")
    } else {
        status_badge
    };

    let mut row = table_row([
        table_cell(status_badge).width(Size::Fixed(70.0)),
        column([
            text(title)
                .font_weight(FontWeight::Medium)
                .ellipsis()
                .width(Size::Fill(1.0)),
            text("Default styling probe.")
                .caption()
                .ellipsis()
                .width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0)),
        table_cell(text(owner).muted()).width(Size::Fixed(110.0)),
        table_cell(text(state).label().small()).width(Size::Fixed(86.0)),
    ])
    .key(if selected {
        "metric:table.row".to_string()
    } else {
        format!("row-{title}")
    })
    .focusable();

    if selected {
        row = row.selected();
    }

    row
}

fn command_card() -> El {
    card([
        card_header([card_title("Command surface")]),
        card_content([
            text_input(
                "command-search",
                "Search commands...",
                &Selection::default(),
            )
            .key("metric:command.input")
            .width(Size::Fill(1.0)),
            popover_panel([
                command_row("git-branch", "New branch", "Ctrl+B").key("metric:command.row"),
                command_row("git-commit", "Commit staged files", "Ctrl+Enter")
                    .key("command-row-commit"),
                command_row("refresh-cw", "Refresh repository", "Ctrl+R")
                    .key("command-row-refresh"),
                command_row("alert-circle", "Force push", "Danger").key("command-row-force"),
            ])
            .width(Size::Fill(1.0)),
            scroll([form_probe()]).key("form-probe-scroll"),
        ])
        .height(Size::Fill(1.0)),
    ])
    .key("metric:command.card")
    .width(Size::Fill(0.8))
    .height(Size::Fill(1.0))
}

fn form_probe() -> El {
    form([
        form_item([
            form_label("Valid input"),
            form_control(
                text_input(
                    "valid-input",
                    "Valid input",
                    &Selection::caret("valid-input", 11),
                )
                .key("metric:form.input"),
            ),
            form_description("Default field spacing and helper text."),
        ]),
        form_item([
            form_label("Invalid input"),
            form_control(
                text_input(
                    "invalid-input",
                    "Invalid input",
                    &Selection::caret("invalid-input", 13),
                )
                .invalid(),
            ),
            form_message("This field needs attention."),
        ]),
        row([
            button("Disabled").secondary().disabled(),
            button("Loading").primary().loading(),
            spacer(),
        ]),
    ])
    .padding(tokens::SPACE_3)
    .fill(tokens::MUTED)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
}

fn icon_cell(label: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .text(label)
        .text_align(TextAlign::Center)
        .caption()
        .font_weight(FontWeight::Semibold)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(26.0))
        .height(Size::Fixed(26.0))
}
