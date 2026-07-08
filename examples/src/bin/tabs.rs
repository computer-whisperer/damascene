//! Tabs — the controlled `tabs_list` widget driving a settings-style
//! tabbed page.
//!
//! `tabs_list` paints a segmented row of triggers (shadcn-flavored)
//! and emits `{key}:tab:{value}` on click; `tabs::apply_event` folds
//! that back into the app's `String` field. The body of the page
//! renders by branching on the same field — no implicit "tab content"
//! sibling, just a `match`.
//!
//! Run: `cargo run -p damascene-examples --bin tabs`
//!
//! Things to try:
//! - Click a tab to switch the page body.
//! - Press `Tab` to focus the row; arrow `Tab` cycles through each
//!   trigger; `Enter` / `Space` activates the focused trigger.
//! - Drop tabs straight into a real settings layout — see how the
//!   active trigger nests visually inside the muted pill.

use damascene_core::prelude::*;

struct Demo {
    tab: String,
}

impl App for Demo {
    fn build(&self, _cx: &BuildCx) -> El {
        let body = match self.tab.as_str() {
            "account" => account_panel(),
            "appearance" => appearance_panel(),
            "advanced" => advanced_panel(),
            // Defensive default in case the value is somehow stale —
            // don't render an empty page silently.
            other => column([text(format!("Unknown tab: {other}")).muted()]),
        };

        column([
            h1("Tabs demo"),
            tabs_list(
                "settings",
                &self.tab,
                [
                    ("account", "Account"),
                    ("appearance", "Appearance"),
                    ("advanced", "Advanced"),
                ],
            ),
            body,
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        tabs::apply_event(&mut self.tab, &event, "settings", |s| Some(s.to_string()));
    }
}

// Panel bodies below are decorative filler — their buttons carry no
// `.key(...)` and route nowhere. Real buttons pair a key with an
// `on_event` arm (see `counter.rs`); the interactive surface this
// example teaches is the tab strip itself.
fn account_panel() -> El {
    titled_card(
        "Account",
        [
            row([text("Email"), text("user@example.com").muted()])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
            row([
                text("Two-factor authentication"),
                badge("Enabled").success(),
            ])
            .align(Align::Center)
            .justify(Justify::SpaceBetween),
            row([text("Recovery codes"), button("Generate").secondary()])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
        ],
    )
}

fn appearance_panel() -> El {
    titled_card(
        "Appearance",
        [
            row([text("Theme"), button("Dark").secondary()])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
            row([text("Compact mode"), badge("Off").muted()])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
            row([text("Font size"), text("14")])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
        ],
    )
}

fn advanced_panel() -> El {
    titled_card(
        "Advanced",
        [
            row([text("Telemetry"), badge("Off").muted()])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
            row([text("Beta features"), badge("Off").muted()])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
            row([
                column([
                    text("Reset to defaults").bold(),
                    text("Restore every preference to its built-in value.")
                        .muted()
                        .small(),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Start),
                button("Reset").destructive(),
            ])
            .align(Align::Center)
            .justify(Justify::SpaceBetween),
        ],
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 520.0);
    damascene_winit_wgpu::run(
        "Damascene — tabs",
        viewport,
        Demo {
            tab: "account".to_string(),
        },
    )
}
