//! dashboard_01_calibration — Damascene fixture paired with the shadcn
//! dashboard-01-style reference.
//!
//! Run:
//! `cargo run -p damascene-core --example dashboard_01_calibration`

use damascene_core::prelude::*;

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 1180.0, 780.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let name = "dashboard_01_calibration";
    let theme = Theme::damascene_dark();
    let mut root = dashboard_01_calibration();
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

fn dashboard_01_calibration() -> El {
    row([dashboard_sidebar(), dashboard_main()])
        .key("metric:root")
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BACKGROUND)
}

fn dashboard_sidebar() -> El {
    column([
        row([
            icon_cell("A"),
            column([
                text("Acme Inc.")
                    .semibold()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("Enterprise")
                    .caption()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_2)
        .height(Size::Fixed(44.0))
        .align(Align::Center),
        section_label("Platform"),
        side_item("layout-dashboard", "Dashboard", true),
        side_item("activity", "Lifecycle", false),
        side_item("bar-chart", "Analytics", false),
        side_item("folder", "Projects", false),
        spacer().height(Size::Fixed(tokens::SPACE_4)),
        section_label("Documents"),
        side_item("file-text", "Data library", false),
        side_item("download", "Reports", false),
        side_item("users", "Team", false),
        spacer(),
        row([
            icon_cell("AK"),
            column([
                text("Alicia Koch")
                    .semibold()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("alicia@example.com")
                    .caption()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_2)
        .height(Size::Fixed(50.0))
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_2))
    .key("metric:sidebar")
    .width(Size::Fixed(244.0))
    .height(Size::Fill(1.0))
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
}

fn section_label(label: &'static str) -> El {
    text(label)
        .caption()
        .height(Size::Fixed(22.0))
        .padding(Sides::xy(tokens::SPACE_2, 0.0))
}

fn side_item(icon_name: &'static str, label: &'static str, selected: bool) -> El {
    let mut item = row([
        icon(icon_name)
            .color(tokens::MUTED_FOREGROUND)
            .icon_size(tokens::ICON_SM)
            .width(Size::Fixed(tokens::ICON_SM)),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .key(if selected {
        "metric:sidebar.nav.row".to_string()
    } else {
        format!("side-item-{label}")
    })
    .metrics_role(MetricsRole::ListItem)
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(tokens::SPACE_2, 0.0))
    .height(Size::Fixed(32.0))
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    } else {
        item = item.color(tokens::MUTED_FOREGROUND);
    }

    item
}

fn dashboard_main() -> El {
    column([
        dashboard_header(),
        column([
            row([
                metric_card(
                    "bar-chart",
                    "Total Revenue",
                    "$1,250.00",
                    "+12.5%",
                    "Trending up this month",
                    true,
                ),
                metric_card(
                    "users",
                    "New Customers",
                    "1,234",
                    "-20%",
                    "Acquisition needs attention",
                    false,
                ),
                metric_card(
                    "folder",
                    "Active Accounts",
                    "45,678",
                    "+12.5%",
                    "Strong user retention",
                    true,
                ),
                metric_card(
                    "activity",
                    "Growth Rate",
                    "4.5%",
                    "+4.5%",
                    "Meets growth projections",
                    true,
                ),
            ])
            .gap(tokens::SPACE_4),
            row([chart_card(), sales_card()])
                .gap(tokens::SPACE_4)
                .height(Size::Fixed(306.0))
                .align(Align::Stretch),
            documents_card(),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .height(Size::Fill(1.0)),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn dashboard_header() -> El {
    row([
        icon_button("menu").ghost(),
        divider().width(Size::Fixed(1.0)).height(Size::Fixed(22.0)),
        h3("Documents").key("metric:page.title"),
        spacer(),
        text_input("dashboard-search", "Search...", &Selection::default())
            .key("metric:command.input")
            .width(Size::Fixed(260.0)),
        icon_button("plus").ghost(),
        icon_button("bell").ghost(),
    ])
    .key("metric:header")
    .gap(tokens::SPACE_3)
    .height(Size::Fixed(56.0))
    .padding(Sides::xy(tokens::SPACE_4, 0.0))
    .align(Align::Center)
    .stroke(tokens::BORDER)
}

fn metric_card(
    icon_name: &'static str,
    title: &'static str,
    value: &'static str,
    delta: &'static str,
    note: &'static str,
    positive: bool,
) -> El {
    let badge = if positive {
        badge(delta).success()
    } else {
        badge(delta).warning()
    };
    let badge = if title == "Total Revenue" {
        badge.key("metric:kpi.badge")
    } else {
        badge
    };
    let value = if title == "Total Revenue" {
        h2(value).ellipsis().key("metric:kpi.value")
    } else {
        h2(value).ellipsis()
    };
    card([card_content([
        row([
            row([
                icon(icon_name)
                    .color(tokens::MUTED_FOREGROUND)
                    .icon_size(tokens::ICON_XS),
                text(title).muted().ellipsis().width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_1)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            badge,
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        value,
        text(note).caption().ellipsis().width(Size::Fill(1.0)),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_2)])
    .key(if title == "Total Revenue" {
        "metric:kpi.card"
    } else {
        title
    })
    .width(Size::Fill(1.0))
}

fn chart_card() -> El {
    card([
        card_header([
            card_title("Visitors for the last 6 months"),
            card_description("Total visitors by channel."),
        ])
        .padding(tokens::SPACE_4),
        card_content([row(chart_bars())
            .gap(2.0)
            .height(Size::Fill(1.0))
            .align(Align::End)])
        .padding(Sides {
            left: tokens::SPACE_4,
            right: tokens::SPACE_4,
            top: 0.0,
            bottom: tokens::SPACE_4,
        })
        .height(Size::Fill(1.0)),
    ])
    .key("metric:chart.card")
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn chart_bars() -> Vec<El> {
    [
        48.0, 72.0, 56.0, 90.0, 64.0, 80.0, 108.0, 84.0, 122.0, 96.0, 136.0, 118.0,
    ]
    .into_iter()
    .flat_map(|height| {
        [
            bar(height, tokens::MUTED_FOREGROUND),
            bar((height - 28.0_f32).max(24.0), tokens::INPUT),
        ]
    })
    .collect()
}

fn bar(height: f32, color: Color) -> El {
    El::new(Kind::Custom("chart_bar"))
        .fill(color)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(height))
}

fn sales_card() -> El {
    card([
        card_header([
            card_title("Recent Sales"),
            card_description("You made 265 sales this month."),
        ])
        .padding(tokens::SPACE_4),
        card_content([
            sale_row("OM", "Olivia Martin", "olivia@example.com", "+$1,999.00"),
            sale_row("JL", "Jackson Lee", "jackson@example.com", "+$39.00"),
            sale_row("IN", "Isabella Nguyen", "isabella@example.com", "+$299.00"),
            sale_row("WK", "William Kim", "will@example.com", "+$99.00"),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides {
            left: tokens::SPACE_4,
            right: tokens::SPACE_4,
            top: 0.0,
            bottom: tokens::SPACE_4,
        }),
    ])
    .key("metric:sales.card")
    .width(Size::Fixed(330.0))
    .height(Size::Fill(1.0))
}

fn sale_row(
    initials: &'static str,
    name: &'static str,
    email: &'static str,
    amount: &'static str,
) -> El {
    row([
        icon_cell(initials),
        column([
            text(name).semibold().ellipsis().width(Size::Fill(1.0)),
            text(email).caption().ellipsis().width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .height(Size::Hug)
        .width(Size::Fill(1.0)),
        text(amount).label().small(),
    ])
    .gap(tokens::SPACE_2)
    .height(Size::Fixed(42.0))
    .align(Align::Center)
}

fn documents_card() -> El {
    card([
        card_header([card_title("Documents")]).padding(tokens::SPACE_4),
        card_content([scroll([table([
            table_header([table_row([
                table_head("").width(Size::Fixed(35.0)),
                table_head("Header").width(Size::Fill(1.8)),
                table_head("Section Type").width(Size::Fill(1.0)),
                table_head("Status").width(Size::Fixed(104.0)),
                table_head("Target").width(Size::Fixed(64.0)),
                table_head("Limit").width(Size::Fixed(64.0)),
                table_head("Reviewer").width(Size::Fixed(128.0)),
                table_head("").width(Size::Fixed(32.0)),
            ])
            .padding(Sides::xy(tokens::SPACE_4, 0.0))
            .key("metric:table.header")]),
            divider(),
            table_body([
                document_row(
                    "Cover page",
                    "Cover page",
                    "In Process",
                    "18",
                    "5",
                    "Eddie Lake",
                    "info",
                ),
                document_row(
                    "Table of contents",
                    "Table of contents",
                    "Done",
                    "29",
                    "24",
                    "Eddie Lake",
                    "success",
                ),
            ]),
        ])])
        .height(Size::Fill(1.0))])
        .gap(0.0)
        .padding(0.0)
        .height(Size::Fill(1.0)),
    ])
    .key("metric:table.card")
    .height(Size::Fill(1.0))
}

fn document_row(
    header: &'static str,
    section: &'static str,
    status: &'static str,
    target: &'static str,
    limit: &'static str,
    reviewer: &'static str,
    tone: &'static str,
) -> El {
    let status_badge = match tone {
        "success" => badge(status).success(),
        _ => badge(status).info(),
    };
    table_row([
        table_utility_cell("::"),
        table_cell(text(header).label().small()).width(Size::Fill(1.8)),
        table_cell(text(section).muted()).width(Size::Fill(1.0)),
        table_cell(status_badge).width(Size::Fixed(104.0)),
        table_cell(text(target).label().small()).width(Size::Fixed(64.0)),
        table_cell(text(limit).label().small()).width(Size::Fixed(64.0)),
        table_cell(text(reviewer).muted()).width(Size::Fixed(128.0)),
        table_action_cell(),
    ])
    .padding(Sides::xy(tokens::SPACE_4, 0.0))
    .key(if header == "Cover page" {
        "metric:table.row"
    } else {
        header
    })
}

fn table_utility_cell(label: &'static str) -> El {
    table_cell(text(label).muted().center_text()).width(Size::Fixed(35.0))
}

fn table_action_cell() -> El {
    stack([icon("more-horizontal")
        .icon_size(tokens::ICON_SM)
        .color(tokens::MUTED_FOREGROUND)])
    .align(Align::Center)
    .justify(Justify::Center)
    .width(Size::Fixed(32.0))
    .height(Size::Hug)
}

fn icon_cell(label: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .text(label)
        .text_align(TextAlign::Center)
        .caption()
        .font_weight(FontWeight::Semibold)
        .fill(tokens::MUTED)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(30.0))
        .height(Size::Fixed(30.0))
}
