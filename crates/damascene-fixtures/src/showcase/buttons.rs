//! Buttons & toggles — every variant in one gallery.
//!
//! All `button(...)` flavours side-by-side (primary / secondary /
//! outline / ghost / link / destructive), the icon variants
//! (`button_with_icon`, `icon_button`), and the toggle family
//! (standalone, single-select group, multi-select group).

use std::collections::HashSet;

use damascene_core::prelude::*;

const TOGGLE_VIEW_OPTIONS: &[(&str, &str)] =
    &[("list", "List"), ("grid", "Grid"), ("kanban", "Kanban")];
const TOGGLE_FILTER_OPTIONS: &[(&str, &str)] =
    &[("open", "Open"), ("draft", "Draft"), ("merged", "Merged")];

pub struct State {
    pub last_click: Option<String>,
    pub wrap: bool,
    pub view: String,
    pub filters: HashSet<String>,
}

impl Default for State {
    fn default() -> Self {
        let mut filters = HashSet::new();
        filters.insert("open".into());
        filters.insert("draft".into());
        Self {
            last_click: None,
            wrap: true,
            view: "grid".into(),
            filters,
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Buttons & toggles"),
        paragraph(
            "Every `button` flavour, the icon variants, and the toggle \
             family. Buttons emit `Click` / `Activate`; toggles fold \
             through `toggle::apply_event_*` helpers that own the \
             pressed-state semantics.",
        )
        .muted(),
        section_label("Variants"),
        variants_strip(phone),
        section_label("Sizes"),
        row([
            button("Small").secondary().small().key("buttons-small"),
            button("Default").secondary().key("buttons-default"),
            button("Large").secondary().large().key("buttons-large"),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        section_label("With icons"),
        icons_strip(phone),
        section_label("Disabled"),
        disabled_strip(phone),
        text(match &state.last_click {
            Some(k) => format!("last click: `{k}`"),
            None => "click any button to record its key.".to_string(),
        })
        .small()
        .muted(),
        separator(),
        section_label("Standalone toggle"),
        paragraph(
            "A single bool. `Click` flips it; the app folds the event \
             back with `toggle::apply_event_pressed`.",
        )
        .small()
        .muted(),
        toggle("buttons-wrap", state.wrap, "Wrap long lines"),
        section_label("Single-select group"),
        paragraph(
            "Mutually exclusive — picks a value, like a panel-less \
             `tabs_list`. Folds via `toggle::apply_event_single`.",
        )
        .small()
        .muted(),
        toggle_group(
            "buttons-view",
            &state.view,
            TOGGLE_VIEW_OPTIONS.iter().copied(),
        ),
        section_label("Multi-select group"),
        paragraph(
            "Each value flips independently — filter chips, formatting \
             toolbars (B / I / U). Folds via `toggle::apply_event_multi`.",
        )
        .small()
        .muted(),
        toggle_group_multi(
            "buttons-filters",
            &state.filters,
            TOGGLE_FILTER_OPTIONS.iter().copied(),
        ),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Start)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if toggle::apply_event_pressed(&mut state.wrap, &e, "buttons-wrap") {
        return;
    }
    if toggle::apply_event_single(&mut state.view, &e, "buttons-view", |s| Some(s.to_string())) {
        return;
    }
    if toggle::apply_event_multi(&mut state.filters, &e, "buttons-filters") {
        return;
    }
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate)
        && let Some(k) = e.route()
        && k.starts_with("buttons-")
    {
        state.last_click = Some(k.to_string());
    }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

/// Five variants — splits 3+2 on phone so "Ghost" / "Destructive" don't
/// spill past the right edge of a 360px viewport.
fn variants_strip(phone: bool) -> El {
    if phone {
        column([
            row([
                button("Primary").primary().key("buttons-primary"),
                button("Secondary").secondary().key("buttons-secondary"),
                button("Outline").outline().key("buttons-outline"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                button("Ghost").ghost().key("buttons-ghost"),
                button("Destructive")
                    .destructive()
                    .key("buttons-destructive"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2)
    } else {
        row([
            button("Primary").primary().key("buttons-primary"),
            button("Secondary").secondary().key("buttons-secondary"),
            button("Outline").outline().key("buttons-outline"),
            button("Ghost").ghost().key("buttons-ghost"),
            button("Destructive")
                .destructive()
                .key("buttons-destructive"),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }
}

/// Icon-with-label buttons plus two icon-only ghost buttons. Phone
/// splits the three labelled buttons from the icon-only pair so each
/// row fits inside the content rect.
fn icons_strip(phone: bool) -> El {
    if phone {
        column([
            row([
                button_with_icon(IconName::Plus, "New file")
                    .primary()
                    .key("buttons-icon-primary"),
                button_with_icon(IconName::Download, "Export")
                    .secondary()
                    .key("buttons-icon-secondary"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                button_with_icon(IconName::X, "Delete")
                    .destructive()
                    .key("buttons-icon-destructive"),
                icon_button(IconName::Settings)
                    .ghost()
                    .key("buttons-icon-only-settings"),
                icon_button(IconName::Bell)
                    .ghost()
                    .key("buttons-icon-only-bell"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2)
    } else {
        row([
            button_with_icon(IconName::Plus, "New file")
                .primary()
                .key("buttons-icon-primary"),
            button_with_icon(IconName::Download, "Export")
                .secondary()
                .key("buttons-icon-secondary"),
            button_with_icon(IconName::X, "Delete")
                .destructive()
                .key("buttons-icon-destructive"),
            icon_button(IconName::Settings)
                .ghost()
                .key("buttons-icon-only-settings"),
            icon_button(IconName::Bell)
                .ghost()
                .key("buttons-icon-only-bell"),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }
}

/// Four disabled variants. Phone splits 2+2 so "Destructive" doesn't
/// poke past the right edge.
fn disabled_strip(phone: bool) -> El {
    if phone {
        column([
            row([
                button("Primary")
                    .primary()
                    .disabled()
                    .key("buttons-disabled-primary"),
                button("Secondary")
                    .secondary()
                    .disabled()
                    .key("buttons-disabled-secondary"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                button("Ghost")
                    .ghost()
                    .disabled()
                    .key("buttons-disabled-ghost"),
                button("Destructive")
                    .destructive()
                    .disabled()
                    .key("buttons-disabled-destructive"),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2)
    } else {
        row([
            button("Primary")
                .primary()
                .disabled()
                .key("buttons-disabled-primary"),
            button("Secondary")
                .secondary()
                .disabled()
                .key("buttons-disabled-secondary"),
            button("Ghost")
                .ghost()
                .disabled()
                .key("buttons-disabled-ghost"),
            button("Destructive")
                .destructive()
                .disabled()
                .key("buttons-disabled-destructive"),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }
}
