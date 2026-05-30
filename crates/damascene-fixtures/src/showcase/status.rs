//! Status & feedback — alert, badge, progress, spinner, skeleton,
//! toast, tooltip.
//!
//! All seven feedback primitives in one panel, scrollable so the long
//! list reads top-to-bottom without crowding. Toasts are mounted into
//! the runtime's floating layer via `App::drain_toasts`; tooltips show
//! up on hover.

use damascene_core::prelude::*;

#[derive(Default)]
pub struct State {
    /// Toasts that the runtime should drain at the start of the next
    /// frame. Push to this Vec; `Showcase::drain_toasts` hands it off.
    pub pending_toasts: Vec<ToastSpec>,
    pub toast_fires: u32,
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Status & feedback"),
        paragraph(
            "Seven primitives apps reach for to communicate state — \
             alerts, badges, progress bars, spinners, skeletons, toasts, \
             and tooltips. Each is small alone; together they cover most \
             of the feedback surface.",
        )
        .muted(),
        section_label("Alert variants"),
        column([
            alert([
                alert_title("Heads up"),
                alert_description("Default alert: card surface, neutral border."),
            ]),
            alert([
                alert_title("New comment"),
                alert_description("Info: friendly nudge, non-destructive."),
            ])
            .info(),
            alert([
                alert_title("Saved"),
                alert_description("Success: a positive confirmation."),
            ])
            .success(),
            alert([
                alert_title("Disk almost full"),
                alert_description("Warning: action recommended but not required."),
            ])
            .warning(),
            alert([
                alert_title("Could not delete repository"),
                alert_description("Destructive: a failure or destructive consequence."),
            ])
            .destructive(),
            alert([
                alert_title("Background task running"),
                alert_description("Muted: low-emphasis status, deprioritized chrome."),
            ])
            .muted(),
        ])
        .gap(tokens::SPACE_2),
        section_label("Badges"),
        badge_strip(phone),
        section_label("Progress"),
        column([
            row([
                text("Determinate (35%)").label().width(Size::Fixed(180.0)),
                progress(0.35, tokens::PRIMARY).width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center),
            row([
                text("Determinate (78%)").label().width(Size::Fixed(180.0)),
                progress(0.78, tokens::SUCCESS).width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center),
            row([
                text("Indeterminate").label().width(Size::Fixed(180.0)),
                progress_indeterminate(tokens::PRIMARY).width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2),
        section_label("Spinners"),
        spinner_strip(phone),
        section_label("Skeletons"),
        skeleton_strip(phone),
        section_label("Toasts"),
        paragraph(
            "Each button queues a `ToastSpec`; the runtime drains them \
             on the next frame and synthesizes the floating stack at \
             bottom-right. Click the X on any card to dismiss.",
        )
        .small()
        .muted(),
        row([
            button("Success").key("toast-success").primary(),
            button("Warning").key("toast-warning"),
            button("Error").key("toast-error").destructive(),
            button("Info").key("toast-info").ghost(),
        ])
        .gap(tokens::SPACE_2),
        text(format!(
            "fired {} toast{} this session",
            state.toast_fires,
            if state.toast_fires == 1 { "" } else { "s" }
        ))
        .small()
        .muted(),
        section_label("Tooltips"),
        paragraph(
            "`.tooltip(text)` on any element shows a runtime-managed \
             tooltip after a short hover. Hover any of these:",
        )
        .small()
        .muted(),
        row([
            button("Save")
                .primary()
                .tooltip("Saves your changes (Ctrl+S)")
                .key("status-save"),
            button("Discard")
                .secondary()
                .tooltip("Discard local edits")
                .key("status-discard"),
            icon_button(IconName::Settings)
                .ghost()
                .tooltip("Open settings")
                .key("status-settings"),
            badge("3")
                .info()
                .tooltip("3 unread notifications")
                .key("status-unread-badge"),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)
    .padding(Sides::xy(tokens::RING_WIDTH, 0.0))])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    let spec = match e.route() {
        Some("toast-success") => ToastSpec::success("Settings saved"),
        Some("toast-warning") => ToastSpec::warning("Battery low — connect charger"),
        Some("toast-error") => ToastSpec::error("Failed to reach update server"),
        Some("toast-info") => ToastSpec::info("New version available"),
        _ => return,
    };
    state.pending_toasts.push(spec);
    state.toast_fires += 1;
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn spinner_tile(label: &str, content: El) -> El {
    card([
        stack([content])
            .align(Align::Center)
            .justify(Justify::Center)
            .height(Size::Fixed(48.0)),
        text(label)
            .muted()
            .small()
            .center_text()
            .width(Size::Fill(1.0))
            .ellipsis(),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

fn skeleton_tile(label: &str, content: El) -> El {
    card([
        content,
        text(label)
            .muted()
            .small()
            .width(Size::Fill(1.0))
            .ellipsis(),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_4)
}

/// Six badges split into two rows of three on phone, which keeps each
/// label readable instead of squeezing them under their pill padding.
fn badge_strip(phone: bool) -> El {
    if phone {
        column([
            row([
                badge("default"),
                badge("info").info(),
                badge("success").success(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
            row([
                badge("warning").warning(),
                badge("destructive").destructive(),
                badge("muted").muted(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_2)
    } else {
        row([
            badge("default"),
            badge("info").info(),
            badge("success").success(),
            badge("warning").warning(),
            badge("destructive").destructive(),
            badge("muted").muted(),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }
}

/// Four spinner variants share one phone row only if we drop the
/// "Inline label" tile to its own row underneath — the spinner+text
/// composition needs more horizontal space than a quarter-viewport tile
/// can give it.
fn spinner_strip(phone: bool) -> El {
    let default_tile = || spinner_tile("Default", spinner());
    let primary_tile = || {
        spinner_tile(
            "Primary",
            spinner_with_color(tokens::PRIMARY)
                .width(Size::Fixed(28.0))
                .height(Size::Fixed(28.0)),
        )
    };
    let destructive_tile = || {
        spinner_tile(
            "Destructive",
            spinner_with_color(tokens::DESTRUCTIVE)
                .width(Size::Fixed(36.0))
                .height(Size::Fixed(36.0)),
        )
    };
    let inline_tile = || {
        spinner_tile(
            "Inline label",
            row([spinner(), text("Loading…").muted()])
                .gap(tokens::SPACE_2)
                .align(Align::Center),
        )
    };
    if phone {
        column([
            row([default_tile(), primary_tile(), destructive_tile()]).gap(tokens::SPACE_3),
            inline_tile(),
        ])
        .gap(tokens::SPACE_3)
    } else {
        row([
            default_tile(),
            primary_tile(),
            destructive_tile(),
            inline_tile(),
        ])
        .gap(tokens::SPACE_3)
    }
}

/// Two skeleton tiles side by side. On phone the "Lines" placeholder
/// shrinks its fixed-width skeletons so they fit under one half-tile.
fn skeleton_strip(phone: bool) -> El {
    let (line_a, line_b, line_c) = if phone {
        (96.0, 76.0, 60.0)
    } else {
        (180.0, 140.0, 110.0)
    };
    row([
        skeleton_tile(
            "Lines",
            column([
                skeleton().width(Size::Fixed(line_a)),
                skeleton().width(Size::Fixed(line_b)),
                skeleton().width(Size::Fixed(line_c)),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Start),
        ),
        skeleton_tile(
            "Avatar placeholder",
            row([
                skeleton_circle(40.0),
                skeleton_circle(32.0),
                skeleton_circle(24.0),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ),
    ])
    .gap(tokens::SPACE_3)
}
