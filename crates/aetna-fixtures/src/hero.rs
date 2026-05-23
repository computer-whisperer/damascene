//! Backend-neutral hero/demo app for README screenshots.
//!
//! Unlike the exhaustive Showcase fixture, this composes a plausible app
//! surface at production density. The same `App` drives the interactive
//! `aetna-examples` binary and the headless README renderer.

use aetna_core::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct HeroDemo;

impl App for HeroDemo {
    fn build(&self, _cx: &BuildCx) -> El {
        stack([
            column(Vec::<El>::new())
                .fill(tokens::BACKGROUND)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            row([nav_rail(), main_panel(), inspector()])
                .gap(tokens::SPACE_4)
                .align(Align::Stretch)
                .padding(tokens::SPACE_4)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
    }

    fn theme(&self) -> Theme {
        Theme::radix_slate_blue_dark()
    }
}

fn nav_rail() -> El {
    sidebar([
        row([
            column(Vec::<El>::new())
                .width(Size::Fixed(32.0))
                .height(Size::Fixed(32.0))
                .radius(tokens::RADIUS_MD)
                .fill(tokens::PRIMARY),
            column([
                text("Aetna").title(),
                text("Release Console").caption().muted(),
            ])
            .gap(1.0),
        ])
        .gap(tokens::SPACE_3)
        .align(Align::Center),
        separator(),
        sidebar_group([
            sidebar_group_label("Workspace"),
            nav_item(IconName::LayoutDashboard, "Overview", true),
            nav_item(IconName::GitBranch, "Pipelines", false),
            nav_item(IconName::BarChart, "Telemetry", false),
            nav_item(IconName::Users, "Agents", false),
        ])
        .gap(tokens::SPACE_1),
        sidebar_group([
            sidebar_group_label("System"),
            nav_item(IconName::Bell, "Incidents", false),
            nav_item(IconName::Settings, "Settings", false),
        ])
        .gap(tokens::SPACE_1),
        spacer(),
        card([
            row([
                icon(IconName::Activity).text_color(tokens::SUCCESS),
                text("Renderer idle").label(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            text("3 backends green").caption().muted(),
            progress(0.92, tokens::SUCCESS),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_3)
        .muted(),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_4)
    .width(Size::Fixed(236.0))
    .height(Size::Fill(1.0))
}

fn nav_item(icon_name: IconName, label: &'static str, current: bool) -> El {
    let item = row([
        icon(icon_name)
            .width(Size::Fixed(18.0))
            .height(Size::Fixed(18.0)),
        text(label).label(),
        spacer(),
        if current {
            badge("live").success().xsmall()
        } else {
            column(Vec::<El>::new()).width(Size::Fixed(1.0))
        },
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
    .radius(tokens::RADIUS_MD);

    if current {
        item.current()
    } else {
        item.ghost()
    }
}

fn main_panel() -> El {
    column([
        top_bar(),
        row([
            metric_card(
                "Frame budget",
                "5.8 ms",
                "p95 GPU submit",
                0.58,
                tokens::SUCCESS,
            ),
            metric_card("Glyph atlas", "41%", "2 pages resident", 0.41, tokens::INFO),
            metric_card(
                "CI matrix",
                "18/18",
                "wgpu, vulkano, wasm",
                1.0,
                tokens::SUCCESS,
            ),
        ])
        .gap(tokens::SPACE_3)
        .align(Align::Stretch),
        pipeline_card(),
        row([preview_card(), queue_card()])
            .gap(tokens::SPACE_3)
            .align(Align::Stretch),
    ])
    .gap(tokens::SPACE_3)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn top_bar() -> El {
    row([
        column([
            text("0.3.6 release gate").heading(),
            text("Backends green. Text quality and shader routing ready.")
                .muted()
                .small()
                .wrap_text(),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0)),
        spacer(),
        search_box(),
        button("Ship").primary().key("hero-promote"),
        icon_button(IconName::MoreHorizontal)
            .ghost()
            .key("hero-menu"),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Center)
}

fn search_box() -> El {
    row([
        icon(IconName::Search)
            .width(Size::Fixed(16.0))
            .height(Size::Fixed(16.0))
            .text_color(tokens::MUTED_FOREGROUND),
        text("Search traces").muted().small(),
        spacer(),
        mono("/").caption().muted(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
    .width(Size::Fixed(200.0))
    .radius(tokens::RADIUS_MD)
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
}

fn metric_card(
    label: &'static str,
    value: &'static str,
    detail: &'static str,
    amount: f32,
    color: Color,
) -> El {
    card([
        row([
            text(label).caption().muted(),
            spacer(),
            badge(if amount >= 0.9 { "green" } else { "stable" })
                .success()
                .xsmall(),
        ])
        .align(Align::Center),
        text(value).display().font_size(26.0),
        text(detail).small().muted(),
        progress(amount, color),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_4)
    .width(Size::Fill(1.0))
}

fn pipeline_card() -> El {
    card([
        card_header([row([
            column([
                card_title("Release pipeline"),
                card_description("GPU render path exercised through headless fixtures."),
            ])
            .gap(tokens::SPACE_1),
            spacer(),
            badge("ready").success(),
        ])
        .align(Align::Center)]),
        card_content([
            row([
                stage("Layout", "tree + state", "4.1k nodes", tokens::INFO),
                connector(0.94),
                stage("Paint stream", "batched ops", "812 quads", tokens::SUCCESS),
                connector(0.88),
                stage("Backend", "wgpu / vulkano", "4x MSAA", tokens::WARNING),
                connector(0.76),
                stage("Artifacts", "png + bundle", "clean lint", tokens::SUCCESS),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            stack([
                chart_grid(),
                sparkline()
                    .width(Size::Fill(1.0))
                    .height(Size::Fixed(96.0))
                    .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_2)),
            ])
            .height(Size::Fixed(118.0))
            .radius(tokens::RADIUS_MD)
            .fill(tokens::MUTED.with_alpha_u8(82))
            .stroke(tokens::BORDER),
        ])
        .gap(tokens::SPACE_4),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_1)
    .height(Size::Fixed(286.0))
}

fn stage(title: &'static str, subtitle: &'static str, value: &'static str, color: Color) -> El {
    column([
        row([
            column(Vec::<El>::new())
                .width(Size::Fixed(10.0))
                .height(Size::Fixed(10.0))
                .radius(tokens::RADIUS_PILL)
                .fill(color),
            text(title).label(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        text(subtitle).caption().muted(),
        text(value).small().semibold(),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_3)
    .width(Size::Fill(1.0))
    .radius(tokens::RADIUS_MD)
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
}

fn connector(amount: f32) -> El {
    column([progress(amount, tokens::PRIMARY)])
        .justify(Justify::Center)
        .width(Size::Fixed(42.0))
        .height(Size::Fixed(58.0))
}

fn chart_grid() -> El {
    column([
        row([
            chart_bar(0.52, tokens::INFO),
            chart_bar(0.64, tokens::SUCCESS),
            chart_bar(0.46, tokens::PRIMARY),
            chart_bar(0.74, tokens::SUCCESS),
            chart_bar(0.58, tokens::INFO),
            chart_bar(0.81, tokens::SUCCESS),
            chart_bar(0.69, tokens::PRIMARY),
            chart_bar(0.88, tokens::SUCCESS),
        ])
        .gap(tokens::SPACE_3)
        .align(Align::End)
        .height(Size::Fill(1.0)),
        row([
            text("layout").caption().muted(),
            spacer(),
            text("draw ops").caption().muted(),
            spacer(),
            text("present").caption().muted(),
        ])
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn chart_bar(amount: f32, color: Color) -> El {
    column(Vec::<El>::new())
        .width(Size::Fill(1.0))
        .height(Size::Fixed(18.0 + amount * 62.0))
        .radius(tokens::RADIUS_SM)
        .fill(color.with_alpha_u8(120))
        .stroke(color.with_alpha_u8(170))
}

fn sparkline() -> El {
    let path = PathBuilder::new()
        .move_to(4.0, 72.0)
        .cubic_to(40.0, 42.0, 62.0, 88.0, 94.0, 50.0)
        .cubic_to(126.0, 12.0, 154.0, 28.0, 184.0, 44.0)
        .cubic_to(222.0, 64.0, 260.0, 24.0, 316.0, 20.0)
        .stroke_solid(tokens::PRIMARY, 4.0)
        .stroke_line_cap(VectorLineCap::Round)
        .stroke_line_join(VectorLineJoin::Round)
        .build();
    vector(VectorAsset::from_paths([0.0, 0.0, 320.0, 96.0], vec![path])).vector_mask(tokens::INFO)
}

fn preview_card() -> El {
    card([
        card_header([row([
            column([
                card_title("Scene preview"),
                card_description("Live app tree"),
            ])
            .gap(tokens::SPACE_1),
            spacer(),
            icon(IconName::RefreshCw)
                .width(Size::Fixed(18.0))
                .height(Size::Fixed(18.0))
                .text_color(tokens::MUTED_FOREGROUND),
        ])
        .align(Align::Center)]),
        card_content([row([
            preview_tile("Cards", 0.84, tokens::SUCCESS),
            preview_tile("Text", 0.96, tokens::INFO),
            preview_tile("Vectors", 0.72, tokens::WARNING),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Stretch)]),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_1)
    .width(Size::Fill(1.2))
}

fn preview_tile(label: &'static str, amount: f32, color: Color) -> El {
    column([
        stack([
            column(Vec::<El>::new())
                .fill(tokens::ACCENT)
                .radius(tokens::RADIUS_SM)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            column(Vec::<El>::new())
                .fill(color.with_alpha_u8(120))
                .radius(tokens::RADIUS_SM)
                .width(Size::Fixed(34.0 + amount * 58.0))
                .height(Size::Fixed(38.0 + amount * 44.0)),
        ])
        .align(Align::Center)
        .justify(Justify::Center)
        .height(Size::Fixed(88.0)),
        text(label).label(),
        progress(amount, color),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .width(Size::Fill(1.0))
    .radius(tokens::RADIUS_MD)
    .fill(tokens::MUTED.with_alpha_u8(82))
    .stroke(tokens::BORDER)
}

fn queue_card() -> El {
    card([
        card_header([
            card_title("Agent queue"),
            card_description("Integration apps exercising the release branch."),
        ]),
        card_content([
            queue_row(
                "volumetric-ui",
                "dashboard port",
                "passing",
                tokens::SUCCESS,
            ),
            queue_row("aetna-volume", "PipeWire control", "review", tokens::INFO),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}

fn queue_row(app: &'static str, detail: &'static str, status: &'static str, color: Color) -> El {
    row([
        column(Vec::<El>::new())
            .width(Size::Fixed(9.0))
            .height(Size::Fixed(9.0))
            .radius(tokens::RADIUS_PILL)
            .fill(color),
        column([text(app).label(), text(detail).caption().muted()]).gap(1.0),
        spacer(),
        badge(status).info().xsmall(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(tokens::SPACE_2)
    .radius(tokens::RADIUS_MD)
    .fill(tokens::MUTED.with_alpha_u8(64))
}

fn inspector() -> El {
    column([
        card([
            row([
                icon(IconName::Command)
                    .width(Size::Fixed(18.0))
                    .height(Size::Fixed(18.0)),
                text("Command surface").label(),
                spacer(),
                badge("hot").warning().xsmall(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            command_row("Ctrl K", "Open palette"),
            command_row("Tab", "Traverse focus"),
            command_row("Esc", "Dismiss overlays"),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4),
        card([
            card_header([card_title("Quality checks")]),
            card_content([
                check_row("Text shaping", "Inter + JetBrains Mono"),
                check_row("MSDF vectors", "explicit mask routing"),
                check_row("Backdrop", "liquid glass parity"),
                check_row("Bundles", "tree, lint, manifest"),
            ])
            .gap(tokens::SPACE_3),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_1),
        card([
            row([
                icon(IconName::Bell).text_color(tokens::INFO),
                text("Deploy window").label(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            text("May 13, 10:00 UTC").title(),
            text("Lock candidate once downstream demos clear their soak pass.")
                .wrap_text()
                .small()
                .muted(),
            button("View checklist").secondary().key("hero-checklist"),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4),
    ])
    .gap(tokens::SPACE_3)
    .width(Size::Fixed(286.0))
    .height(Size::Fill(1.0))
}

fn command_row(key: &'static str, label: &'static str) -> El {
    row([
        mono(key)
            .caption()
            .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
            .radius(tokens::RADIUS_SM)
            .fill(tokens::MUTED),
        text(label).small(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

fn check_row(title: &'static str, detail: &'static str) -> El {
    row([
        icon(IconName::Check)
            .width(Size::Fixed(16.0))
            .height(Size::Fixed(16.0))
            .text_color(tokens::SUCCESS),
        column([text(title).label(), text(detail).caption().muted()]).gap(1.0),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}
