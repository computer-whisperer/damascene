//! Palette — token swatches for the active theme.
//!
//! The hero shot. Each chip is a stock surface filled with one of the
//! semantic tokens; the resolved hex/rgba sits underneath as the
//! mono caption. Switching the sidebar's theme picker swaps every
//! chip live because the fills resolve through the active palette at
//! paint time.
//!
//! The taxonomy mirrors `crates/aetna-core/examples/palette_demo.rs`
//! so the showcase and the standalone example agree on what's a "core
//! token" vs. an "Aetna extension."

use aetna_core::prelude::*;

#[derive(Default)]
pub struct State;

#[derive(Clone, Copy)]
struct TokenDef {
    name: &'static str,
    color: Color,
}

const CORE_TOKENS: &[TokenDef] = &[
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

const EXTENSION_TOKENS: &[TokenDef] = &[
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

pub fn view(palette: &Palette, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Palette"),
        paragraph(
            "Every semantic token, resolved through the active palette. \
             Switch themes via the picker in the sidebar — every chip on \
             this page (and every widget on every other page) re-resolves \
             to the new palette on the next frame because token references \
             are stored verbatim and looked up at paint time.",
        )
        .muted(),
        preview_panel("Stock widgets", widget_preview(phone)),
        token_section(
            "Core tokens",
            "The shadcn-shaped semantic vocabulary.",
            CORE_TOKENS,
            palette,
        ),
        token_section(
            "Aetna extensions",
            "Status, link, scrollbar, and selection tokens layered on top.",
            EXTENSION_TOKENS,
            palette,
        ),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

fn token_section(
    title: &'static str,
    description: &'static str,
    defs: &'static [TokenDef],
    palette: &Palette,
) -> El {
    titled_card(
        title,
        [
            paragraph(description).muted().small(),
            swatch_grid(defs, palette),
        ],
    )
}

fn swatch_grid(defs: &'static [TokenDef], palette: &Palette) -> El {
    let rows = defs
        .chunks(2)
        .map(|chunk| {
            row(chunk.iter().map(|t| chip(*t, palette)))
                .gap(tokens::SPACE_3)
                .align(Align::Center)
                .width(Size::Fill(1.0))
        })
        .collect::<Vec<_>>();
    column(rows).gap(tokens::SPACE_2).width(Size::Fill(1.0))
}

fn chip(token: TokenDef, palette: &Palette) -> El {
    let resolved = palette.resolve(token.color);
    card([row([
        column(Vec::<El>::new())
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
            // Long rgba labels (e.g. "rgba(255, 255, 255, 1.0)") are
            // wider than a phone-shape chip; ellipsis here mirrors the
            // token-name treatment above.
            mono(rgba_label(resolved))
                .caption()
                .muted()
                .ellipsis()
                .nowrap_text()
                .width(Size::Fill(1.0)),
        ])
        .gap(0.0)
        .width(Size::Fill(1.0))
        .height(Size::Hug),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .width(Size::Fill(1.0))])
    .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_2))
    .radius(tokens::RADIUS_MD)
    .height(Size::Fixed(54.0))
}

fn preview_panel(title: &'static str, body: El) -> El {
    titled_card(
        title,
        [
            paragraph("Live components under the active palette.")
                .muted()
                .small(),
            body,
        ],
    )
}

fn widget_preview(phone: bool) -> El {
    // Phone collapses the buttons and badges rows so the palette preview
    // can show every variant without spilling past 360px content. Desktop
    // keeps both as a single line — the showcase reads better when each
    // taxonomy is a horizontal sweep.
    let buttons_row: El = if phone {
        column([
            row([
                button("Primary").primary().small(),
                button("Secondary").secondary().small(),
                button("Outline").outline().small(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                button("Ghost").ghost().small(),
                button("Destructive").destructive().small(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2)
    } else {
        row([
            button("Primary").primary(),
            button("Secondary").secondary(),
            button("Outline").outline(),
            button("Ghost").ghost(),
            button("Destructive").destructive(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    };
    column([
        buttons_row,
        row([
            badge("info").info(),
            badge("success").success(),
            badge("warning").warning(),
            badge("destructive").destructive(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        row([
            surface_sample("Card", tokens::CARD),
            surface_sample("Muted", tokens::MUTED),
            surface_sample("Popover", tokens::POPOVER),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Stretch),
        alert([
            alert_title("Heads up"),
            alert_description("Alerts re-color through the active palette."),
        ])
        .info(),
    ])
    .gap(tokens::SPACE_3)
    .width(Size::Fill(1.0))
}

fn surface_sample(title: &'static str, fill: Color) -> El {
    card([
        text(title).label().width(Size::Fill(1.0)).ellipsis(),
        text("surface sample")
            .caption()
            .muted()
            .width(Size::Fill(1.0))
            .ellipsis(),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_3)
    .fill(fill)
    .radius(tokens::RADIUS_MD)
    .height(Size::Fixed(64.0))
}

fn rgba_label(c: Color) -> String {
    let [r, g, b, a] = c.to_srgb_u8a();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}/{a:03}")
    }
}
