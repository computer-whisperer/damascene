//! Focused liquid-glass material lab.

use damascene_core::prelude::*;

pub const LIQUID_GLASS_LAB_WGSL: &str = include_str!("../shaders/liquid_glass_lab.wgsl");
pub const LIQUID_BACKDROP_LAB_WGSL: &str = include_str!("../shaders/liquid_backdrop_lab.wgsl");

pub struct LiquidGlassLab;

impl App for LiquidGlassLab {
    fn build(&self, _cx: &BuildCx) -> El {
        liquid_glass_lab()
    }

    fn shaders(&self) -> Vec<AppShader> {
        vec![
            AppShader {
                name: "liquid_backdrop_lab",
                wgsl: LIQUID_BACKDROP_LAB_WGSL,
                samples_backdrop: false,
                samples_time: false,
            },
            AppShader {
                name: "liquid_glass_lab",
                wgsl: LIQUID_GLASS_LAB_WGSL,
                samples_backdrop: true,
                samples_time: false,
            },
        ]
    }

    fn theme(&self) -> Theme {
        Theme::default().with_icon_material(IconMaterial::Glass)
    }
}

pub fn liquid_glass_lab() -> El {
    stack([
        ambient_backdrop(),
        column([
            top_bar(),
            row([
                control_panel(),
                column([now_panel(), flow_panel()])
                    .gap(tokens::SPACE_3)
                    .width(Size::Fixed(340.0)),
            ])
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
            bottom_dock(),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0)),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn ambient_backdrop() -> El {
    stack([
        backdrop_field(),
        column([
            row([
                backdrop_chip("north", tokens::PRIMARY.with_alpha_u8(95)),
                backdrop_chip("mesh", tokens::SUCCESS.with_alpha_u8(90)),
                backdrop_chip("relay", tokens::WARNING.with_alpha_u8(95)),
            ])
            .gap(tokens::SPACE_3),
            spacer(),
            row([
                backdrop_chip("alpha", Color::srgb_u8a(255, 255, 255, 42)),
                spacer(),
                backdrop_chip("delta", Color::srgb_u8a(255, 255, 255, 32)),
            ])
            .gap(tokens::SPACE_3),
        ])
        .padding(Sides::all(46.0))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0)),
    ])
    .fill(Color::srgb_u8(7, 10, 22))
}

fn backdrop_field() -> El {
    El::new(Kind::Custom("liquid_backdrop"))
        .shader(
            ShaderBinding::custom("liquid_backdrop_lab")
                .color("vec_a", Color::srgb_u8a(8, 13, 31, 255))
                .color("vec_b", Color::srgb_u8a(37, 210, 208, 190))
                .color("vec_c", Color::srgb_u8a(164, 74, 200, 178))
                .color("vec_d", Color::srgb_u8a(255, 139, 68, 168)),
        )
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

fn backdrop_chip(label: &'static str, color: Color) -> El {
    row([text(label).caption().text_color(tokens::PRIMARY_FOREGROUND)])
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_1))
        .fill(color)
        .stroke(Color::srgb_u8a(255, 255, 255, 42))
        .radius(tokens::RADIUS_PILL)
        .width(Size::Hug)
        .height(Size::Fixed(28.0))
}

fn top_bar() -> El {
    glass_surface(
        row([
            row([
                icon("layout-dashboard").icon_size(19.0),
                text("Control Deck").title().bold(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            spacer(),
            badge("live").success(),
            icon_button("search").ghost(),
            icon_button("bell").ghost(),
            icon_button("settings").ghost(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        GlassSpec::bar(),
    )
    .height(Size::Fixed(58.0))
}

fn control_panel() -> El {
    glass_surface(
        column([
            row([
                column([
                    text("Operations").heading().bold(),
                    text("Northwest mesh").text_color(glass_muted_text()),
                ])
                .gap(tokens::SPACE_1),
                spacer(),
                icon("activity").icon_size(34.0),
            ])
            .align(Align::Center),
            row([
                metric("throughput", "94.8", "gb/s", IconName::BarChart),
                metric("latency", "12.4", "ms", IconName::RefreshCw),
                metric("health", "99.9", "%", IconName::Check),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Stretch),
            glass_table(),
            sparkline_panel(),
            row([
                button_with_icon("upload", "Publish").primary(),
                button_with_icon("download", "Export").secondary(),
                spacer(),
                badge("stable").info(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_4),
        GlassSpec::hero(),
    )
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn now_panel() -> El {
    glass_surface(
        column([
            row([
                text("Now").title().bold(),
                spacer(),
                icon("git-branch").icon_size(22.0),
            ])
            .align(Align::Center),
            signal_row("router-7", "nominal", tokens::SUCCESS, "check"),
            signal_row("cache-east", "warming", tokens::WARNING, "alert-circle"),
            signal_row("agents", "32 active", tokens::PRIMARY, "users"),
        ])
        .gap(tokens::SPACE_3),
        GlassSpec::panel(),
    )
    .height(Size::Fixed(236.0))
}

fn flow_panel() -> El {
    glass_surface(
        column([
            row([
                text("Flow").title().bold(),
                spacer(),
                badge("auto").secondary(),
            ])
            .align(Align::Center),
            row([
                icon_stat("download", "In", "18.2"),
                icon_stat("upload", "Out", "16.7"),
            ])
            .gap(tokens::SPACE_3),
            El::new(Kind::Custom("signal_bar"))
                .fill(Color::srgb_u8a(255, 255, 255, 56))
                .stroke(Color::srgb_u8a(255, 255, 255, 80))
                .radius(tokens::RADIUS_PILL)
                .height(Size::Fixed(10.0))
                .width(Size::Fill(1.0)),
            paragraph("Adaptive routing is holding the lane while background jobs drain.")
                .text_color(glass_muted_text())
                .max_lines(2),
        ])
        .gap(tokens::SPACE_3),
        GlassSpec::panel(),
    )
    .height(Size::Fill(1.0))
}

fn bottom_dock() -> El {
    glass_surface(
        row([
            dock_button("menu", "Menu"),
            dock_button("file-text", "Logs"),
            dock_button("folder", "Files"),
            dock_button("command", "Run"),
            dock_button("more-horizontal", "More"),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .justify(Justify::Center),
        GlassSpec::dock(),
    )
    .height(Size::Fixed(72.0))
}

fn glass_table() -> El {
    column([
        table_row("Shard", "Load", "Status", true),
        table_row("atlas", "71%", "sync", false),
        table_row("beacon", "64%", "ready", false),
        table_row("cursor", "82%", "busy", false),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}

fn table_row(a: &'static str, b: &'static str, c: &'static str, header: bool) -> El {
    let role = if header {
        TextRole::Caption
    } else {
        TextRole::Label
    };
    row([
        text(a).text_role(role).width(Size::Fill(1.0)),
        text(b).text_role(role).width(Size::Fixed(72.0)),
        text(c)
            .text_role(role)
            .text_align(TextAlign::End)
            .width(Size::Fixed(86.0)),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
    .fill(if header {
        Color::srgb_u8a(255, 255, 255, 24)
    } else {
        Color::srgb_u8a(255, 255, 255, 10)
    })
    .radius(tokens::RADIUS_SM)
    .width(Size::Fill(1.0))
}

fn sparkline_panel() -> El {
    let heights = [
        28.0, 46.0, 34.0, 62.0, 54.0, 76.0, 50.0, 68.0, 44.0, 58.0, 38.0, 70.0,
    ];
    let mut bars = Vec::new();
    for (i, h) in heights.iter().enumerate() {
        let alpha = 52 + (i as u8 % 4) * 18;
        bars.push(
            column([
                spacer(),
                El::new(Kind::Custom("signal_bar"))
                    .fill(Color::srgb_u8a(238, 248, 255, alpha))
                    .radius(tokens::RADIUS_PILL)
                    .height(Size::Fixed(*h))
                    .width(Size::Fixed(16.0)),
            ])
            .height(Size::Fixed(82.0))
            .width(Size::Fill(1.0)),
        );
    }

    column([
        row([
            text("Signal envelope").label(),
            spacer(),
            text("last 12m").caption().text_color(glass_muted_text()),
        ])
        .align(Align::Center),
        row(bars).gap(tokens::SPACE_2).align(Align::End),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .fill(Color::srgb_u8a(255, 255, 255, 14))
    .stroke(Color::srgb_u8a(255, 255, 255, 38))
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
}

fn metric(label: &'static str, value: &'static str, unit: &'static str, icon_name: IconName) -> El {
    column([
        row([
            icon(icon_name).icon_size(19.0),
            spacer(),
            text(unit).caption().muted(),
        ])
        .align(Align::Center),
        text(value).heading().bold(),
        text(label).caption().text_color(glass_muted_text()),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_3)
    .fill(Color::srgb_u8a(255, 255, 255, 22))
    .stroke(Color::srgb_u8a(255, 255, 255, 46))
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fixed(108.0))
}

fn signal_row(
    label: &'static str,
    value: &'static str,
    color: Color,
    icon_name: &'static str,
) -> El {
    row([
        icon(icon_name).icon_size(18.0).text_color(color),
        column([
            text(label).label(),
            text(value).caption().text_color(glass_muted_text()),
        ])
        .gap(2.0),
        spacer(),
        El::new(Kind::Custom("signal_dot"))
            .fill(color)
            .radius(tokens::RADIUS_PILL)
            .width(Size::Fixed(9.0))
            .height(Size::Fixed(9.0)),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(tokens::SPACE_2)
    .fill(Color::srgb_u8a(255, 255, 255, 16))
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
}

fn icon_stat(icon_name: &'static str, label: &'static str, value: &'static str) -> El {
    column([
        icon(icon_name).icon_size(24.0),
        text(value).title().bold(),
        text(label).caption().text_color(glass_muted_text()),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
    .justify(Justify::Center)
    .fill(Color::srgb_u8a(255, 255, 255, 18))
    .stroke(Color::srgb_u8a(255, 255, 255, 40))
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fixed(94.0))
}

fn dock_button(icon_name: &'static str, label: &'static str) -> El {
    column([
        icon(icon_name).icon_size(21.0),
        text(label).caption().center_text(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
    .justify(Justify::Center)
    .width(Size::Fixed(84.0))
    .height(Size::Fixed(54.0))
    .fill(Color::srgb_u8a(255, 255, 255, 12))
    .radius(tokens::RADIUS_MD)
}

#[derive(Clone, Copy)]
struct GlassSpec {
    tint: Color,
    accent: Color,
    blur: f32,
    refract: f32,
    specular: f32,
    opacity: f32,
    radius: f32,
    rim: f32,
    frost: f32,
}

impl GlassSpec {
    fn hero() -> Self {
        Self {
            tint: Color::srgb_u8a(225, 243, 255, 92),
            accent: Color::srgb_u8a(112, 214, 255, 150),
            blur: 7.0,
            refract: 0.52,
            specular: 1.0,
            opacity: 0.70,
            radius: 30.0,
            rim: 0.95,
            frost: 0.30,
        }
    }

    fn panel() -> Self {
        Self {
            tint: Color::srgb_u8a(250, 247, 255, 78),
            accent: Color::srgb_u8a(194, 164, 255, 130),
            blur: 5.5,
            refract: 0.38,
            specular: 0.86,
            opacity: 0.66,
            radius: 24.0,
            rim: 0.82,
            frost: 0.28,
        }
    }

    fn bar() -> Self {
        Self {
            tint: Color::srgb_u8a(230, 248, 255, 72),
            accent: Color::srgb_u8a(126, 230, 210, 130),
            blur: 5.0,
            refract: 0.30,
            specular: 0.78,
            opacity: 0.64,
            radius: 22.0,
            rim: 0.72,
            frost: 0.28,
        }
    }

    fn dock() -> Self {
        Self {
            tint: Color::srgb_u8a(255, 255, 255, 70),
            accent: Color::srgb_u8a(255, 205, 128, 120),
            blur: 7.0,
            refract: 0.50,
            specular: 0.92,
            opacity: 0.66,
            radius: 28.0,
            rim: 0.90,
            frost: 0.34,
        }
    }
}

fn glass_surface(content: El, spec: GlassSpec) -> El {
    El::new(Kind::Custom("liquid_glass_surface"))
        .child(content)
        .padding(tokens::SPACE_4)
        .shader(
            ShaderBinding::custom("liquid_glass_lab")
                .color("vec_a", spec.tint)
                .vec4(
                    "vec_b",
                    [spec.blur, spec.refract, spec.specular, spec.opacity],
                )
                .vec4("vec_c", [spec.radius, spec.rim, spec.frost, 0.0])
                .color("vec_d", spec.accent),
        )
        .stroke(Color::srgb_u8a(255, 255, 255, 82))
        .stroke_width(1.0)
        .radius(spec.radius)
        .text_color(glass_text())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn glass_text() -> Color {
    Color::srgb_u8a(238, 246, 255, 236)
}

fn glass_muted_text() -> Color {
    Color::srgb_u8a(198, 215, 226, 206)
}
