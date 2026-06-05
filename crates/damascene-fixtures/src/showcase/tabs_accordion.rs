//! Tabs, editor tabs, accordion — the disclosure family.
//!
//! Three controlled disclosures grouped onto one page. `tabs_list`
//! drives a panel swap; `editor_tabs` handles the closeable / addable
//! VS Code idiom; `accordion` is the FAQ-style collapsible stack.

use damascene_core::prelude::*;

use super::{Section, Showcase};

const TABS_KEY: &str = "ta-tabs";
const TABS_TRIGGER_KEY: &str = "ta-actions-trigger";
const ACCORDION_KEY: &str = "ta-accordion";
const TABS_ACTIONS: &[&str] = &["Reset to defaults", "Export config", "Import config"];

pub struct State {
    pub tab: String,
    pub actions_open: bool,
    pub last_action: Option<String>,
    pub editor_docs: Vec<String>,
    pub editor_active: String,
    pub editor_next_id: u32,
    pub editor_active_style: ActiveTabStyle,
    pub editor_close_visibility: CloseVisibility,
    pub accordion_open: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tab: "account".into(),
            actions_open: false,
            last_action: None,
            editor_docs: vec!["README.md".into(), "main.rs".into(), "Cargo.toml".into()],
            editor_active: "main.rs".into(),
            editor_next_id: 1,
            editor_active_style: ActiveTabStyle::Lifted,
            editor_close_visibility: CloseVisibility::ActiveOrHover,
            accordion_open: Some("billing".into()),
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Tabs & accordion"),
        paragraph(
            "Three controlled disclosures: `tabs_list` for a panel swap, \
             `editor_tabs` for the VS Code-style strip, and `accordion` \
             for FAQ-style collapsibles.",
        )
        .muted(),
        section_label("Tabs"),
        tabs_demo(state),
        section_label("Editor tabs"),
        editor_tabs_demo(state, phone),
        section_label("Accordion"),
        accordion_demo(state),
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

pub fn on_event(state: &mut State, e: UiEvent) {
    if tabs::apply_event(&mut state.tab, &e, TABS_KEY, |s| Some(s.to_string())) {
        return;
    }
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        match e.route() {
            Some(k) if k == TABS_TRIGGER_KEY => {
                state.actions_open = !state.actions_open;
                return;
            }
            Some("ta-actions-menu:dismiss") => {
                state.actions_open = false;
                return;
            }
            Some(k) if k.starts_with("ta-action:") => {
                state.last_action = Some(k["ta-action:".len()..].to_string());
                state.actions_open = false;
                return;
            }
            _ => {}
        }
    }
    let mut counter = state.editor_next_id;
    let did_strip = editor_tabs::apply_event(
        &mut state.editor_docs,
        &mut state.editor_active,
        &e,
        "ta-strip",
        |s| Some(s.to_string()),
        || {
            let id = counter;
            counter += 1;
            format!("untitled-{id}")
        },
    );
    state.editor_next_id = counter;
    if did_strip {
        return;
    }
    if tabs::apply_event(
        &mut state.editor_active_style,
        &e,
        "ta-style",
        parse_active_style,
    ) {
        return;
    }
    if tabs::apply_event(
        &mut state.editor_close_visibility,
        &e,
        "ta-close",
        parse_close_visibility,
    ) {
        return;
    }
    let _ = accordion::apply_event(&mut state.accordion_open, &e, ACCORDION_KEY, |s| {
        Some(s.to_string())
    });
}

/// Floating layer for the actions dropdown on the Account tab.
pub fn actions_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::TabsAccordion && app.tabs_accordion.actions_open).then(|| {
        dropdown(
            "ta-actions-menu",
            TABS_TRIGGER_KEY,
            TABS_ACTIONS
                .iter()
                .map(|a| menu_item(*a).key(format!("ta-action:{a}"))),
        )
    })
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn tabs_demo(state: &State) -> El {
    let body = match state.tab.as_str() {
        "account" => account_panel(state),
        "appearance" => appearance_panel(),
        "advanced" => advanced_panel(),
        other => column([text(format!("Unknown tab: {other}")).muted()]),
    };
    column([
        tabs_list(
            TABS_KEY,
            &state.tab,
            [
                ("account", "Account"),
                ("appearance", "Appearance"),
                ("advanced", "Advanced"),
            ],
        ),
        body,
    ])
    .gap(tokens::SPACE_3)
}

fn account_panel(state: &State) -> El {
    let trailing_caption = match &state.last_action {
        Some(a) => format!("last action: {a}"),
        None => "Click \"Actions ▾\" to open a dropdown menu.".into(),
    };
    titled_card(
        "Account",
        [
            kv("Email", "user@example.com"),
            kv_row("Two-factor authentication", badge("Enabled").success()),
            kv_row(
                "Bulk actions",
                button("Actions ▾").key(TABS_TRIGGER_KEY).secondary(),
            ),
            // Top padding clears the button's focus-ring band above —
            // card_content stacks children flush, and a caption painted
            // band-to-band with the trigger would occlude its ring.
            text(trailing_caption)
                .small()
                .muted()
                .wrap_text()
                .fill_width()
                .padding(Sides::top(tokens::SPACE_2)),
        ],
    )
}

fn appearance_panel() -> El {
    titled_card(
        "Appearance",
        [
            kv_row("Theme", badge("Dark").info()),
            kv_row("Compact mode", badge("Off").muted()),
            kv("Font size", "14"),
        ],
    )
}

fn advanced_panel() -> El {
    titled_card(
        "Advanced",
        [
            kv_row("Telemetry", badge("Off").muted()),
            kv_row("Beta features", badge("Off").muted()),
        ],
    )
}

fn kv(label: &str, value: &str) -> El {
    row([text(label), text(value).muted()])
        .align(Align::Center)
        .justify(Justify::SpaceBetween)
}

fn kv_row(label: &str, trailing: El) -> El {
    row([text(label), trailing])
        .align(Align::Center)
        .justify(Justify::SpaceBetween)
}

fn editor_tabs_demo(state: &State, phone: bool) -> El {
    let style_picker = column([
        text("Active tab treatment").caption().muted(),
        tabs_list(
            "ta-style",
            &active_style_token(state.editor_active_style),
            [
                ("lifted", "Lifted"),
                ("top-accent", "Top"),
                ("bottom-rule", "Bottom"),
            ],
        ),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0));

    let close_picker = column([
        text("Close icon visibility").caption().muted(),
        tabs_list(
            "ta-close",
            &close_visibility_token(state.editor_close_visibility),
            [
                ("hover", "Hover"),
                ("dimmed", "Dimmed"),
                ("always", "Always"),
            ],
        ),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0));

    let raw_strip = editor_tabs_with(
        "ta-strip",
        &state.editor_active,
        state.editor_docs.iter().map(|d| (d.clone(), d.clone())),
        EditorTabsConfig {
            active_style: state.editor_active_style,
            close_visibility: state.editor_close_visibility,
        },
    );
    // Phone wraps the strip in a horizontal scroll so a couple of long
    // filenames + the trailing `+` button can extend past the viewport
    // without overflowing the page column. The strip itself defaults to
    // `Size::Fill(1.0)`; inside a horizontal scroll we override to
    // `Size::Hug` so the strip's intrinsic width drives the scroll's
    // content size rather than the (narrower) viewport.
    let strip: El = if phone {
        scroll([raw_strip.width(Size::Hug)])
            .axis(Axis::Row)
            .height(Size::Hug)
            .width(Size::Fill(1.0))
    } else {
        raw_strip
    };

    let panel = card([
        h2(state.editor_active.clone()),
        text(format!(
            "{} open tab{} — click any tab to switch, × to close, + to open a new one.",
            state.editor_docs.len(),
            if state.editor_docs.len() == 1 {
                ""
            } else {
                "s"
            },
        ))
        .muted()
        .wrap_text()
        .fill_width(),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_4)
    .height(Size::Fixed(140.0));

    let strip_and_panel = column([strip, panel]).gap(0.0);
    // Phone stacks the two pickers in a column so each gets the full
    // content width — at half-width on a 360px viewport, three tabs in
    // each picker can't fit their labels ("Lifted"/"Bottom"/...).
    let pickers: El = if phone {
        column([style_picker, close_picker])
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
    } else {
        row([style_picker, close_picker])
            .gap(tokens::SPACE_4)
            .align(Align::Start)
    };
    column([pickers, strip_and_panel]).gap(tokens::SPACE_3)
}

fn accordion_demo(state: &State) -> El {
    let item = |value: &str, label: &str, body: El| -> El {
        let open = state.accordion_open.as_deref() == Some(value);
        accordion_item(ACCORDION_KEY, value, label, open, [body])
    };

    accordion([
        item(
            "shipping",
            "Shipping & delivery",
            paragraph(
                "Orders ship within 1–2 business days. Tracking emails arrive \
                 once the carrier scans the package.",
            ),
        ),
        accordion_separator(),
        item(
            "billing",
            "Billing",
            paragraph(
                "Invoices are billed monthly on the anniversary of the account \
                 creation date. Annual plans are charged once up front and renew \
                 automatically.",
            ),
        ),
        accordion_separator(),
        item(
            "support",
            "Support",
            paragraph(
                "Email support@example.com or open a ticket from the Settings → Support tab.",
            ),
        ),
    ])
}

fn active_style_token(s: ActiveTabStyle) -> &'static str {
    match s {
        ActiveTabStyle::Lifted => "lifted",
        ActiveTabStyle::TopAccent => "top-accent",
        ActiveTabStyle::BottomRule => "bottom-rule",
        _ => "lifted",
    }
}

fn parse_active_style(raw: &str) -> Option<ActiveTabStyle> {
    match raw {
        "lifted" => Some(ActiveTabStyle::Lifted),
        "top-accent" => Some(ActiveTabStyle::TopAccent),
        "bottom-rule" => Some(ActiveTabStyle::BottomRule),
        _ => None,
    }
}

fn close_visibility_token(c: CloseVisibility) -> &'static str {
    match c {
        CloseVisibility::ActiveOrHover => "hover",
        CloseVisibility::Dimmed => "dimmed",
        CloseVisibility::Always => "always",
        _ => "hover",
    }
}

fn parse_close_visibility(raw: &str) -> Option<CloseVisibility> {
    match raw {
        "hover" => Some(CloseVisibility::ActiveOrHover),
        "dimmed" => Some(CloseVisibility::Dimmed),
        "always" => Some(CloseVisibility::Always),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn tabs_swap_active() {
        let mut s = State::default();
        assert_eq!(s.tab, "account");
        on_event(&mut s, click("ta-tabs:tab:appearance"));
        assert_eq!(s.tab, "appearance");
        on_event(&mut s, click("ta-tabs:tab:advanced"));
        assert_eq!(s.tab, "advanced");
    }

    #[test]
    fn editor_tabs_select_close_add_round_trip() {
        let mut s = State::default();
        assert_eq!(s.editor_active, "main.rs");
        on_event(&mut s, click("ta-strip:tab:README.md"));
        assert_eq!(s.editor_active, "README.md");
        on_event(&mut s, click("ta-strip:close:README.md"));
        assert_eq!(s.editor_active, "main.rs");
        assert!(!s.editor_docs.iter().any(|d| d == "README.md"));
        on_event(&mut s, click("ta-strip:add"));
        assert_eq!(s.editor_active, "untitled-1");
    }

    #[test]
    fn actions_dropdown_open_close_cycle() {
        let mut s = State::default();
        assert!(!s.actions_open);
        on_event(&mut s, click(TABS_TRIGGER_KEY));
        assert!(s.actions_open);
        on_event(&mut s, click("ta-action:Reset to defaults"));
        assert_eq!(s.last_action.as_deref(), Some("Reset to defaults"));
        assert!(!s.actions_open);
    }
}
