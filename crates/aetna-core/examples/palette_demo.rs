//! palette_demo — token palette comparison fixture.
//!
//! Run:
//! `cargo run -p aetna-core --example palette_demo`

use aetna_core::prelude::*;

#[derive(Clone, Copy)]
struct TokenDef {
    name: &'static str,
    color: Color,
}

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 1220.0, 1040.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let variants = [
        ("palette_demo.aetna_dark", "Aetna dark", Theme::aetna_dark()),
        (
            "palette_demo.aetna_light",
            "Aetna light",
            Theme::aetna_light(),
        ),
        (
            "palette_demo.radix_slate_blue_dark",
            "Radix slate + blue dark",
            Theme::radix_slate_blue_dark(),
        ),
        (
            "palette_demo.radix_slate_blue_light",
            "Radix slate + blue light",
            Theme::radix_slate_blue_light(),
        ),
        (
            "palette_demo.radix_sand_amber_dark",
            "Radix sand + amber dark",
            Theme::radix_sand_amber_dark(),
        ),
        (
            "palette_demo.radix_sand_amber_light",
            "Radix sand + amber light",
            Theme::radix_sand_amber_light(),
        ),
        (
            "palette_demo.radix_mauve_violet_dark",
            "Radix mauve + violet dark",
            Theme::radix_mauve_violet_dark(),
        ),
        (
            "palette_demo.radix_mauve_violet_light",
            "Radix mauve + violet light",
            Theme::radix_mauve_violet_light(),
        ),
    ];

    for (file_name, label, theme) in variants {
        let mut root = palette_demo(label, theme.palette());
        let bundle = render_bundle_themed(&mut root, viewport, &theme);
        let written = write_bundle(&bundle, &out_dir, file_name)?;
        for p in &written {
            println!("wrote {}", p.display());
        }

        if !bundle.lint.findings.is_empty() {
            eprintln!(
                "\nlint findings for {file_name} ({}):",
                bundle.lint.findings.len()
            );
            eprint!("{}", bundle.lint.text());
        }
    }

    Ok(())
}

fn palette_demo(label: &'static str, palette: &Palette) -> El {
    column([
        row([
            column([
                h1("Palette demo"),
                text("Aetna and Radix palettes rendered through Aetna tokens.").muted(),
            ])
            .gap(tokens::SPACE_2)
            .height(Size::Hug),
            spacer(),
            badge(label).muted(),
        ])
        .align(Align::Start)
        .height(Size::Hug),
        row([
            token_section(
                "Core tokens",
                "The shadcn-shaped semantic vocabulary.",
                &CORE_TOKENS,
                palette,
            )
            .width(Size::Fill(1.25)),
            column([
                token_section(
                    "Aetna extensions",
                    "Component and status tokens layered on top.",
                    &EXTENSION_TOKENS,
                    palette,
                ),
                component_section(),
            ])
            .gap(tokens::SPACE_4)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Stretch)
        .height(Size::Fill(1.0)),
    ])
    .padding(tokens::SPACE_8)
    .gap(tokens::SPACE_6)
    .fill_size()
    .fill(tokens::BACKGROUND)
}

const CORE_TOKENS: [TokenDef; 19] = [
    TokenDef {
        name: "background",
        color: tokens::BACKGROUND,
    },
    TokenDef {
        name: "foreground",
        color: tokens::FOREGROUND,
    },
    TokenDef {
        name: "card",
        color: tokens::CARD,
    },
    TokenDef {
        name: "card-foreground",
        color: tokens::CARD_FOREGROUND,
    },
    TokenDef {
        name: "popover",
        color: tokens::POPOVER,
    },
    TokenDef {
        name: "popover-foreground",
        color: tokens::POPOVER_FOREGROUND,
    },
    TokenDef {
        name: "primary",
        color: tokens::PRIMARY,
    },
    TokenDef {
        name: "primary-foreground",
        color: tokens::PRIMARY_FOREGROUND,
    },
    TokenDef {
        name: "secondary",
        color: tokens::SECONDARY,
    },
    TokenDef {
        name: "secondary-foreground",
        color: tokens::SECONDARY_FOREGROUND,
    },
    TokenDef {
        name: "muted",
        color: tokens::MUTED,
    },
    TokenDef {
        name: "muted-foreground",
        color: tokens::MUTED_FOREGROUND,
    },
    TokenDef {
        name: "accent",
        color: tokens::ACCENT,
    },
    TokenDef {
        name: "accent-foreground",
        color: tokens::ACCENT_FOREGROUND,
    },
    TokenDef {
        name: "destructive",
        color: tokens::DESTRUCTIVE,
    },
    TokenDef {
        name: "destructive-foreground",
        color: tokens::DESTRUCTIVE_FOREGROUND,
    },
    TokenDef {
        name: "border",
        color: tokens::BORDER,
    },
    TokenDef {
        name: "input",
        color: tokens::INPUT,
    },
    TokenDef {
        name: "ring",
        color: tokens::RING,
    },
];

const EXTENSION_TOKENS: [TokenDef; 12] = [
    TokenDef {
        name: "success",
        color: tokens::SUCCESS,
    },
    TokenDef {
        name: "success-foreground",
        color: tokens::SUCCESS_FOREGROUND,
    },
    TokenDef {
        name: "warning",
        color: tokens::WARNING,
    },
    TokenDef {
        name: "warning-foreground",
        color: tokens::WARNING_FOREGROUND,
    },
    TokenDef {
        name: "info",
        color: tokens::INFO,
    },
    TokenDef {
        name: "info-foreground",
        color: tokens::INFO_FOREGROUND,
    },
    TokenDef {
        name: "link-foreground",
        color: tokens::LINK_FOREGROUND,
    },
    TokenDef {
        name: "overlay-scrim",
        color: tokens::OVERLAY_SCRIM,
    },
    TokenDef {
        name: "scrollbar-thumb",
        color: tokens::SCROLLBAR_THUMB_FILL,
    },
    TokenDef {
        name: "scrollbar-thumb-active",
        color: tokens::SCROLLBAR_THUMB_FILL_ACTIVE,
    },
    TokenDef {
        name: "selection-bg",
        color: tokens::SELECTION_BG,
    },
    TokenDef {
        name: "selection-bg-unfocused",
        color: tokens::SELECTION_BG_UNFOCUSED,
    },
];

fn token_section(
    title: &'static str,
    description: &'static str,
    tokens: &'static [TokenDef],
    palette: &Palette,
) -> El {
    card([
        card_header([card_title(title), card_description(description)]),
        card_content([swatch_grid(tokens, palette)]),
    ])
    .height(Size::Fill(1.0))
}

fn swatch_grid(defs: &'static [TokenDef], palette: &Palette) -> El {
    let rows = defs
        .chunks(2)
        .map(|chunk| {
            row(chunk.iter().map(|token| token_chip(*token, palette)))
                .gap(tokens::SPACE_3)
                .align(Align::Center)
                .width(Size::Fill(1.0))
        })
        .collect::<Vec<_>>();

    column(rows)
        .gap(tokens::SPACE_3)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

fn token_chip(token: TokenDef, palette: &Palette) -> El {
    let resolved = palette.resolve(token.color);
    row([
        El::new(Kind::Custom("palette-swatch"))
            .at(file!(), line!())
            .fill(token.color)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_SM)
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(34.0)),
        column([
            text(token.name)
                .label()
                .ellipsis()
                .nowrap_text()
                .width(Size::Fill(1.0)),
            mono(rgba_label(resolved)).caption().muted(),
        ])
        .gap(0.0)
        .width(Size::Fill(1.0))
        .height(Size::Hug),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_2))
    .align(Align::Center)
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fixed(54.0))
}

fn component_section() -> El {
    card([
        card_header([
            card_title("Stock widgets"),
            card_description("The same palette applied to regular component constructors."),
        ]),
        card_content([
            row([
                button("Primary").primary(),
                button("Secondary").secondary(),
                button("Outline").outline(),
                button("Ghost").ghost(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                badge("success").success(),
                badge("warning").warning(),
                badge("destructive").destructive(),
                badge("info").info(),
                badge("muted").muted(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                text_input("palette search", &Selection::default(), "palette:search")
                    .width(Size::Fill(1.0)),
                button_with_icon("settings", "Tune").secondary(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                surface_sample("Card", tokens::CARD),
                surface_sample("Muted", tokens::MUTED),
                surface_sample("Popover", tokens::POPOVER),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Stretch),
        ])
        .gap(tokens::SPACE_4),
    ])
    .height(Size::Hug)
}

fn surface_sample(title: &'static str, fill: Color) -> El {
    column([
        text(title).label(),
        text("surface sample").caption().muted(),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_3)
    .fill(fill)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fixed(76.0))
}

fn rgba_label(c: Color) -> String {
    let [r, g, b, a] = c.to_srgb_u8a();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}/{a:03}")
    }
}
