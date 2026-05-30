//! Shared icon gallery fixture for windowed and headless wgpu demos.

use damascene_core::prelude::*;

pub struct IconGallery;

impl App for IconGallery {
    fn build(&self, _cx: &BuildCx) -> El {
        icon_gallery()
    }
}

pub struct ReliefIconGallery;

impl App for ReliefIconGallery {
    fn build(&self, _cx: &BuildCx) -> El {
        icon_gallery()
    }

    fn theme(&self) -> Theme {
        Theme::default().with_icon_material(IconMaterial::Relief)
    }
}

pub struct GlassIconGallery;

impl App for GlassIconGallery {
    fn build(&self, _cx: &BuildCx) -> El {
        icon_gallery()
    }

    fn theme(&self) -> Theme {
        Theme::default().with_icon_material(IconMaterial::Glass)
    }
}

pub fn icon_gallery() -> El {
    let names = all_icon_names();
    let mut rows = Vec::new();
    for chunk in names.chunks(6) {
        rows.push(
            row(chunk
                .iter()
                .map(|name| icon_tile(*name))
                .collect::<Vec<_>>())
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0)),
        );
    }

    column([
        row([
            column([
                h1("Vector icons"),
                paragraph("SVG source parsed by usvg, tessellated by lyon, drawn by the active GPU backend.")
                    .muted()
                    .max_lines(2),
            ])
            .gap(tokens::SPACE_1)
            .width(Size::Fill(1.0)),
            row([
                button_with_icon("upload", "Action").secondary(),
                icon_button("bell").ghost(),
            ])
            .gap(tokens::SPACE_2),
        ])
        .align(Align::Center)
        .width(Size::Fill(1.0)),
        titled_card("Built-ins", rows),
        row([
            button_with_icon("search", "Search").primary(),
            button_with_icon("download", "Export").secondary(),
            button_with_icon("refresh-cw", "Refresh").ghost(),
            spacer(),
            icon("settings")
                .icon_size(22.0)
                .text_color(tokens::MUTED_FOREGROUND),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn icon_tile(name: IconName) -> El {
    column([
        icon(name)
            .icon_size(28.0)
            .icon_stroke_width(2.0)
            .text_color(color_for_icon(name)),
        text(name.name())
            .small()
            .muted()
            .center_text()
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .justify(Justify::Center)
    .padding(tokens::SPACE_3)
    .width(Size::Fixed(118.0))
    .height(Size::Fixed(92.0))
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
}

fn color_for_icon(name: IconName) -> Color {
    match name {
        IconName::AlertCircle | IconName::X => tokens::DESTRUCTIVE,
        IconName::Check => tokens::SUCCESS,
        IconName::Bell | IconName::Activity => tokens::WARNING,
        IconName::Download | IconName::Upload | IconName::RefreshCw => tokens::PRIMARY,
        _ => tokens::FOREGROUND,
    }
}
