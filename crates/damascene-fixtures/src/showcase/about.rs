//! About — landing page for the Showcase.
//!
//! First-touch page for visitors hitting the published wasm build. Two
//! short framing paragraphs, a `Notifications` dispatcher card that
//! wires several widgets together (severity tabs → button colour, text
//! input → toast message, switch → toast TTL, click → drain to host),
//! a row of animation-profile swatches that promote on click and fade
//! their siblings, and a link out to the repo. The two cards
//! deliberately use widgets with strong motion (animated tab
//! indicator, switch thumb, toast slide-in, spring-driven swatch
//! transitions) so a visitor's first interaction shows off the
//! library's animation envelope.

use std::sync::LazyLock;
use std::time::Duration;

use damascene_core::prelude::*;

/// Damascene badge icon — embedded SVG with its own gradient stops, so the
/// brand colors stay constant across every theme rather than tinting
/// through `currentColor`.
const DAMASCENE_BADGE_ICON_SVG: &str = include_str!("../../../../assets/damascene_badge_icon.svg");
static DAMASCENE_BADGE_ICON: LazyLock<SvgIcon> = LazyLock::new(|| {
    SvgIcon::parse(DAMASCENE_BADGE_ICON_SVG).expect("damascene_badge_icon.svg parses")
});

const SEVERITY_KEY: &str = "about-severity";
const MESSAGE_KEY: &str = "about-message";
const AUTO_DISMISS_KEY: &str = "about-auto-dismiss";
const SEND_KEY: &str = "about-send";
const ANNOUNCE_KEY: &str = "about-announce";
const RESET_KEY: &str = "about-reset";
/// Routed-key prefix for the animation-profile swatches; the suffix
/// is the index into [`ACCENTS`].
const ACCENT_PREFIX: &str = "about-accent-";

const REPO_URL: &str = "https://github.com/computer-whisperer/damascene";

/// TTL applied to dispatched toasts when the auto-dismiss switch is
/// off. Long enough to read comfortably without being effectively
/// permanent — the user can still dismiss manually with the toast's
/// own × button.
const PERSISTENT_TTL: Duration = Duration::from_secs(60);

/// One animation-profile swatch row entry. Each swatch animates with
/// its own `Timing`, so clicking through the row shows the four
/// motion shapes side-by-side: critically-damped vs. snappy vs.
/// overshoot vs. fixed-duration ease.
#[derive(Clone, Copy)]
struct Accent {
    label: &'static str,
    blurb: &'static str,
    color: Color,
    timing: Timing,
}

const ACCENTS: &[Accent] = &[
    Accent {
        label: "Spring · gentle",
        blurb: "UI defaults — settle without overshoot.",
        color: tokens::INFO,
        timing: Timing::SPRING_GENTLE,
    },
    Accent {
        label: "Spring · quick",
        blurb: "Snappy — buttons, tooltips, toggles.",
        color: tokens::PRIMARY,
        timing: Timing::SPRING_QUICK,
    },
    Accent {
        label: "Spring · bouncy",
        blurb: "Overshoots — drawers, picker pops.",
        color: tokens::SUCCESS,
        timing: Timing::SPRING_BOUNCY,
    },
    Accent {
        label: "Tween · ease",
        blurb: "Fixed-duration ease for incidental fades.",
        color: tokens::WARNING,
        timing: Timing::EASE_STANDARD,
    },
];

pub struct State {
    /// Selected severity tab — one of `info` / `success` / `warning`
    /// / `error`. Drives both the toast variant and the Send button's
    /// style.
    pub severity: String,
    /// Body text for the next dispatched toast.
    pub message: String,
    /// Folded selection for the message text input.
    pub message_selection: Selection,
    /// When true, dispatched toasts use the runtime's default TTL;
    /// when false they get a long persistent TTL so the user can
    /// inspect them at leisure.
    pub auto_dismiss: bool,
    /// Counter shown in the dispatcher's badge.
    pub sent_count: u32,
    /// Toasts the next frame should drain. Showcase merges this with
    /// the Status section's queue in `App::drain_toasts`.
    pub pending_toasts: Vec<ToastSpec>,
    /// Screen-reader announcements queued by the Announce button,
    /// drained by `App::drain_announcements`. Audible only while
    /// assistive technology is connected — the live-region layer the
    /// runtime synthesizes from these paints nothing.
    pub pending_announcements: Vec<Announcement>,
    /// Index of the currently-promoted accent swatch, when one is
    /// active. `None` means every swatch sits at rest.
    pub accent_active: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            severity: "success".into(),
            message: "Showcase deployed".into(),
            message_selection: Selection::default(),
            auto_dismiss: true,
            sent_count: 0,
            pending_toasts: Vec::new(),
            pending_announcements: Vec::new(),
            accent_active: None,
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let runtime_note = if cx
        .diagnostics()
        .is_some_and(|diag| matches!(diag.backend, "WebGPU" | "WebGL2"))
    {
        "The page you're looking at is wasm-compiled Rust running through a \
         browser GPU canvas; native shells use the same widget vocabulary."
    } else {
        "The page you're looking at is native Rust running through a wgpu \
         surface; the browser shell uses the same widget vocabulary through \
         WebGPU."
    };
    scroll([column([
        row([
            icon((*DAMASCENE_BADGE_ICON).clone()).icon_size(64.0),
            h1("Damascene"),
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Center)
        .height(Size::Hug),
        paragraph(
            "Damascene is a GPU UI rendering library that mounts inside an \
             existing Vulkan or wgpu host rather than owning the device, \
             queue, or swapchain.",
        )
        .muted(),
        paragraph(runtime_note).muted(),
        paragraph(
            "Browse the sidebar for every widget category and system \
             feature. Switch themes from the picker above the nav and \
             watch each page re-resolve live.",
        )
        .muted(),
        section_label("Try it"),
        dispatch_card(state, cx),
        section_label("Animation profiles"),
        accents_card(state),
        section_label("Source"),
        text_runs([
            text("Code, docs, and the open work list at "),
            text("github.com/computer-whisperer/damascene").link(REPO_URL),
            text("."),
        ])
        .wrap_text(),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if tabs::apply_event(&mut state.severity, &e, SEVERITY_KEY, |s| {
        Some(s.to_string())
    }) {
        return;
    }
    if switch::apply_event(&mut state.auto_dismiss, &e, AUTO_DISMISS_KEY) {
        return;
    }
    if text_input::apply_event(
        &mut state.message,
        &mut state.message_selection,
        &e,
        MESSAGE_KEY,
    ) {
        return;
    }
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        match e.route() {
            Some(SEND_KEY) => {
                let body = if state.message.trim().is_empty() {
                    placeholder_message(&state.severity).to_string()
                } else {
                    state.message.clone()
                };
                let mut spec = match state.severity.as_str() {
                    "success" => ToastSpec::success(body),
                    "warning" => ToastSpec::warning(body),
                    "error" => ToastSpec::error(body),
                    _ => ToastSpec::info(body),
                };
                if !state.auto_dismiss {
                    spec = spec.with_ttl(PERSISTENT_TTL);
                }
                state.pending_toasts.push(spec);
                state.sent_count = state.sent_count.saturating_add(1);
            }
            Some(ANNOUNCE_KEY) => {
                let body = if state.message.trim().is_empty() {
                    placeholder_message(&state.severity).to_string()
                } else {
                    state.message.clone()
                };
                // Severity maps onto ARIA politeness: errors interrupt
                // (`role="alert"`), everything else waits its turn
                // (`role="status"`).
                state
                    .pending_announcements
                    .push(match state.severity.as_str() {
                        "error" => Announcement::assertive(body),
                        _ => Announcement::polite(body),
                    });
            }
            Some(RESET_KEY) => {
                state.sent_count = 0;
            }
            Some(k) if k.starts_with(ACCENT_PREFIX) => {
                if let Ok(i) = k[ACCENT_PREFIX.len()..].parse::<usize>()
                    && i < ACCENTS.len()
                {
                    state.accent_active = if state.accent_active == Some(i) {
                        None
                    } else {
                        Some(i)
                    };
                }
            }
            _ => {}
        }
    }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn dispatch_card(state: &State, cx: &BuildCx) -> El {
    // On a 360px phone the four severity labels can't all fit at default
    // weight inside the equal-share tabs_list — "Success"/"Warning"
    // overflow by ~10px. Switch to compact labels below the showcase
    // phone breakpoint.
    let severity_labels: &[(&str, &str)] = if super::is_phone(cx) {
        &[
            ("info", "Info"),
            ("success", "OK"),
            ("warning", "Warn"),
            ("error", "Error"),
        ]
    } else {
        &[
            ("info", "Info"),
            ("success", "Success"),
            ("warning", "Warning"),
            ("error", "Error"),
        ]
    };
    // `card_content` defaults to no inter-child gap — slot-of-slots
    // surfaces leave that decision to the caller. Wrap the body in a
    // single column so each row breathes from the next.
    titled_card(
        "Notifications",
        [column([
            paragraph(
                "Pick a severity, edit the message, fire a toast. The Send \
                 button restyles itself to match the chosen variant; turning \
                 Auto-dismiss off makes the toast stick around longer so you \
                 can read it.",
            )
            .small()
            .muted(),
            tabs_list(
                SEVERITY_KEY,
                &state.severity,
                severity_labels.iter().copied(),
            ),
            text_input(MESSAGE_KEY, &state.message, &state.message_selection)
                .aria_label("Message")
                .width(Size::Fill(1.0)),
            row([
                switch(AUTO_DISMISS_KEY, state.auto_dismiss).aria_label("Auto-dismiss"),
                paragraph(if state.auto_dismiss {
                    "Auto-dismiss after a few seconds"
                } else {
                    "Stay until manually dismissed"
                })
                .small()
                .muted()
                .width(Size::Fill(1.0)),
                badge(format!("{} sent", state.sent_count)).muted(),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center),
            row([
                send_button(&state.severity, super::is_phone(cx)).key(SEND_KEY),
                button("Announce")
                    .outline()
                    .small()
                    .key(ANNOUNCE_KEY)
                    .tooltip(
                        "Queue a screen-reader announcement (ARIA live region) — \
                         audible only with assistive technology running",
                    ),
                spacer(),
                button(if super::is_phone(cx) {
                    "Reset"
                } else {
                    "Reset counter"
                })
                .ghost()
                .small()
                .key(RESET_KEY),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Stretch)],
    )
}

fn send_button(severity: &str, phone: bool) -> El {
    // The full label plus the Announce/Reset buttons doesn't fit a
    // 360px phone row — compact label below the showcase breakpoint,
    // same move the severity tabs make above.
    let base = button_with_icon(
        IconName::Bell,
        if phone { "Send" } else { "Send notification" },
    );
    match severity {
        "success" => base.success(),
        "warning" => base.warning(),
        "error" => base.destructive(),
        _ => base.info(),
    }
}

fn placeholder_message(severity: &str) -> &'static str {
    match severity {
        "success" => "Showcase deployed",
        "warning" => "Disk almost full",
        "error" => "Failed to reach the update server",
        _ => "New version available",
    }
}

fn accents_card(state: &State) -> El {
    let any_active = state.accent_active.is_some();
    let swatches: Vec<El> = ACCENTS
        .iter()
        .enumerate()
        .map(|(i, a)| accent_swatch(i, a, state.accent_active == Some(i), any_active))
        .collect();
    titled_card(
        "Animation profiles",
        [column([
            paragraph(
                "Hover for the press/hover envelope; click any swatch to \
                 promote it — the others fade. Each card animates with its \
                 own timing, so you can compare the four motion shapes side \
                 by side.",
            )
            .small()
            .muted(),
            row(swatches).gap(tokens::SPACE_3).align(Align::Stretch),
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Stretch)],
    )
}

fn accent_swatch(i: usize, accent: &Accent, active: bool, any_active: bool) -> El {
    // Three visual states: at rest (no swatch active), promoted (this
    // swatch active — scaled and lifted), and demoted (some other
    // swatch active — fade to background). Each transition rides the
    // accent's own `Timing`, so the four cards demonstrate the four
    // motion shapes when toggled.
    let (scale, translate_y, opacity) = if active {
        (1.08, -6.0, 1.0)
    } else if any_active {
        (1.0, 0.0, 0.4)
    } else {
        (1.0, 0.0, 1.0)
    };
    column([
        column(Vec::<El>::new())
            .key(format!("{ACCENT_PREFIX}{i}"))
            .aria_label(format!("Accent color {i}"))
            .focusable()
            .cursor(Cursor::Pointer)
            .fill(accent.color)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_LG)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(80.0))
            .scale(scale)
            .translate(0.0, translate_y)
            .opacity(opacity)
            .animate(accent.timing),
        // Wrap text so long labels / blurbs fit when the swatch
        // column is narrower than the text — tab-trigger labels stay
        // ellipsised but these read like prose, so wrap is right.
        paragraph(accent.label).label(),
        paragraph(accent.blurb).caption().muted(),
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn send_appends_a_toast_with_the_typed_message_and_severity() {
        let mut s = State {
            severity: "warning".into(),
            message: "Disk almost full".into(),
            ..State::default()
        };
        on_event(&mut s, click(SEND_KEY));
        assert_eq!(s.sent_count, 1);
        assert_eq!(s.pending_toasts.len(), 1);
        assert_eq!(s.pending_toasts[0].level, ToastLevel::Warning);
        assert_eq!(s.pending_toasts[0].message, "Disk almost full");
    }

    #[test]
    fn auto_dismiss_off_extends_the_toast_ttl() {
        let mut s = State::default();
        on_event(&mut s, click(SEND_KEY));
        let default_ttl = s.pending_toasts[0].ttl;
        s.pending_toasts.clear();
        s.auto_dismiss = false;
        on_event(&mut s, click(SEND_KEY));
        assert!(
            s.pending_toasts[0].ttl > default_ttl,
            "auto-dismiss off should extend the toast TTL beyond the default",
        );
    }

    #[test]
    fn empty_message_falls_back_to_a_severity_appropriate_placeholder() {
        let mut s = State {
            message: "   ".into(),
            severity: "error".into(),
            ..State::default()
        };
        on_event(&mut s, click(SEND_KEY));
        let body = &s.pending_toasts[0].message;
        assert!(
            !body.trim().is_empty(),
            "blank message should not produce a blank toast; got {body:?}",
        );
    }

    #[test]
    fn accent_clicks_toggle_the_promoted_index() {
        let mut s = State::default();
        on_event(&mut s, click("about-accent-2"));
        assert_eq!(s.accent_active, Some(2));
        on_event(&mut s, click("about-accent-0"));
        assert_eq!(s.accent_active, Some(0));
        on_event(&mut s, click("about-accent-0"));
        assert_eq!(
            s.accent_active, None,
            "click on the promoted swatch demotes it"
        );
    }

    #[test]
    fn reset_zeros_the_send_counter_without_clearing_the_message() {
        let mut s = State::default();
        on_event(&mut s, click(SEND_KEY));
        on_event(&mut s, click(SEND_KEY));
        s.pending_toasts.clear();
        let original = s.message.clone();
        on_event(&mut s, click(RESET_KEY));
        assert_eq!(s.sent_count, 0);
        assert_eq!(s.message, original, "reset should leave the message intact");
    }
}
