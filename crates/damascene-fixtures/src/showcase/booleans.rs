//! Booleans — switch, checkbox, radio.
//!
//! The three "yes/no/which-one" controls in their canonical shadcn
//! shapes. Switch and checkbox are controlled bools sharing
//! `apply_event(&mut bool, ...)`; radio_group picks one of N tokens.

use damascene_core::prelude::*;

const RADIO_OPTIONS: &[(&str, &str)] = &[
    ("system", "Match system"),
    ("light", "Light"),
    ("dark", "Dark"),
];

pub struct State {
    pub auto_lock: bool,
    pub share_usage: bool,
    pub agree: bool,
    pub email_digest: bool,
    pub theme: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            auto_lock: true,
            share_usage: false,
            agree: true,
            email_digest: true,
            theme: "light".into(),
        }
    }
}

pub fn view(state: &State) -> El {
    scroll([column([
        h1("Booleans"),
        paragraph(
            "Three small primitives for yes/no/which-one. Switch and \
             checkbox both fold through `apply_event(&mut bool, …)`; \
             `radio_group` picks one of N tokens with \
             `radio::apply_event(&mut String, …)`.",
        )
        .muted(),
        section_label("Switches"),
        switch_row(
            "booleans-auto-lock",
            state.auto_lock,
            "Auto-lock after 5 minutes",
            "Require the password again when idle.",
        ),
        switch_row(
            "booleans-share-usage",
            state.share_usage,
            "Share anonymous usage statistics",
            "Help us understand how Damascene is used in the wild.",
        ),
        section_label("Checkboxes"),
        checkbox_row(
            "booleans-agree",
            state.agree,
            "I agree to the terms",
            "Standard agreement checkbox — no value beyond on/off.",
        ),
        checkbox_row(
            "booleans-digest",
            state.email_digest,
            "Email digest",
            "A bundled daily summary, instead of per-event email.",
        ),
        section_label("Radio group"),
        paragraph("Pick the active theme — three mutually-exclusive options.")
            .small()
            .muted(),
        radio_group(
            "booleans-theme",
            &state.theme,
            RADIO_OPTIONS.iter().copied(),
        ),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Start)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if radio::apply_event(&mut state.theme, &e, "booleans-theme", |s| {
        Some(s.to_string())
    }) {
        return;
    }
    let _ = checkbox::apply_event(&mut state.agree, &e, "booleans-agree")
        || checkbox::apply_event(&mut state.email_digest, &e, "booleans-digest")
        || switch::apply_event(&mut state.auto_lock, &e, "booleans-auto-lock")
        || switch::apply_event(&mut state.share_usage, &e, "booleans-share-usage");
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn checkbox_row(key: &str, value: bool, label: &str, description: &str) -> El {
    row([
        checkbox(key.to_string(), value).aria_label(label),
        column([
            text(label).label().wrap_text().fill_width(),
            text(description).muted().small().wrap_text().fill_width(),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Center)
    .width(Size::Fill(1.0))
}

fn switch_row(key: &str, value: bool, label: &str, description: &str) -> El {
    row([
        column([
            text(label).label().wrap_text().fill_width(),
            text(description).muted().small().wrap_text().fill_width(),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0)),
        switch(key.to_string(), value).aria_label(label),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Center)
    .width(Size::Fill(1.0))
}
