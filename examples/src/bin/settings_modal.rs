//! Settings modal — the canonical "modal + tabs + scroll body +
//! sticky footer" form layout.
//!
//! A 4-tab settings dialog at a non-default panel size, exercising
//! the form primitives a real port reaches for:
//!
//! - [`field_row`] for the [label … control] rhythm
//! - [`slider::apply_input`] folding pointer + key into one call
//! - `modal_panel` sized explicitly via the standard `.width()` /
//!   `.height()` builders, composed with [`overlay`] + [`scrim`]
//! - `tabs_list` driving the page body, body wrapped in [`scroll`]
//!   so long forms still let the sticky footer stay visible
//!
//! Run: `cargo run -p damascene-examples --bin settings_modal`
//!
//! Things to try:
//! - Click "Open settings" to launch the modal.
//! - Tab through the controls; arrow keys move sliders.
//! - Click the scrim or "Cancel" to dismiss.
//! - Switch tabs via click or focus + Enter.

use damascene_core::prelude::*;

struct SettingsModalApp {
    open: bool,
    tab: String,
    // General
    autoconnect: bool,
    notifications: bool,
    // Audio
    volume: f32,
    mute: bool,
    // Voice
    push_to_talk: bool,
    voice_gain: f32,
    // Advanced
    telemetry: bool,
    beta: bool,
}

impl SettingsModalApp {
    fn new() -> Self {
        Self {
            open: false,
            tab: "general".into(),
            autoconnect: true,
            notifications: true,
            volume: 0.6,
            mute: false,
            push_to_talk: false,
            voice_gain: 0.4,
            telemetry: false,
            beta: false,
        }
    }
}

impl App for SettingsModalApp {
    fn build(&self, _cx: &BuildCx) -> El {
        let main = column([
            h1("Settings modal"),
            text(
                "A 720×620 modal with tabs at the top, a scrollable body, \
                and a sticky save/cancel footer. Open it to feel the layout.",
            )
            .muted(),
            button("Open settings").primary().key("open"),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .align(Align::Start);

        overlays(main, [self.open.then(|| self.settings_modal())])
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.is_click_or_activate("open") {
            self.open = true;
            return;
        }
        if event.is_click_or_activate("settings:dismiss")
            || event.is_click_or_activate("cancel")
            || event.is_click_or_activate("save")
        {
            self.open = false;
            return;
        }
        tabs::apply_event(&mut self.tab, &event, "settings", |s| Some(s.to_string()));
        switch::apply_event(&mut self.autoconnect, &event, "autoconnect");
        switch::apply_event(&mut self.notifications, &event, "notifications");
        switch::apply_event(&mut self.mute, &event, "mute");
        switch::apply_event(&mut self.push_to_talk, &event, "push_to_talk");
        switch::apply_event(&mut self.telemetry, &event, "telemetry");
        switch::apply_event(&mut self.beta, &event, "beta");
        slider::apply_input(&mut self.volume, &event, "volume", 0.05, 0.25);
        slider::apply_input(&mut self.voice_gain, &event, "voice_gain", 0.05, 0.25);
    }
}

impl SettingsModalApp {
    fn settings_modal(&self) -> El {
        let body = match self.tab.as_str() {
            "general" => self.general_tab(),
            "audio" => self.audio_tab(),
            "voice" => self.voice_tab(),
            "advanced" => self.advanced_tab(),
            _ => column([text("Unknown tab").muted()]),
        };

        let panel = modal_panel(
            "Settings",
            [
                tabs_list(
                    "settings",
                    &self.tab,
                    [
                        ("general", "General"),
                        ("audio", "Audio"),
                        ("voice", "Voice"),
                        ("advanced", "Advanced"),
                    ],
                ),
                // The scroll body claims the remaining height between
                // the tabs and the footer, so long forms scroll while
                // the footer stays pinned to the bottom of the panel.
                scroll([body]).key("settings:body"),
                row([
                    spacer(),
                    button("Cancel").ghost().key("cancel"),
                    button("Save").primary().key("save"),
                ])
                .gap(tokens::SPACE_2),
            ],
        )
        .width(Size::Fixed(720.0))
        .height(Size::Fixed(620.0))
        .block_pointer();

        overlay([scrim("settings:dismiss"), panel])
    }

    fn general_tab(&self) -> El {
        column([
            field_row(
                "Autoconnect on launch",
                switch(self.autoconnect).key("autoconnect"),
            ),
            field_row(
                "Desktop notifications",
                switch(self.notifications).key("notifications"),
            ),
        ])
        .gap(tokens::SPACE_3)
        .padding(Sides::xy(0.0, tokens::SPACE_3))
    }

    fn audio_tab(&self) -> El {
        column([
            field_row(
                format!("Volume ({:.0}%)", self.volume * 100.0),
                slider(self.volume, tokens::PRIMARY)
                    .key("volume")
                    .width(Size::Fixed(220.0)),
            ),
            field_row("Mute output", switch(self.mute).key("mute")),
        ])
        .gap(tokens::SPACE_3)
        .padding(Sides::xy(0.0, tokens::SPACE_3))
    }

    fn voice_tab(&self) -> El {
        column([
            field_row(
                "Push to talk",
                switch(self.push_to_talk).key("push_to_talk"),
            ),
            field_row(
                format!("Mic gain ({:.0}%)", self.voice_gain * 100.0),
                slider(self.voice_gain, tokens::PRIMARY)
                    .key("voice_gain")
                    .width(Size::Fixed(220.0)),
            ),
        ])
        .gap(tokens::SPACE_3)
        .padding(Sides::xy(0.0, tokens::SPACE_3))
    }

    fn advanced_tab(&self) -> El {
        column([
            field_row(
                "Anonymous telemetry",
                switch(self.telemetry).key("telemetry"),
            ),
            field_row("Beta features", switch(self.beta).key("beta")),
        ])
        .gap(tokens::SPACE_3)
        .padding(Sides::xy(0.0, tokens::SPACE_3))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 960.0, 720.0);
    damascene_winit_wgpu::run(
        "Damascene — settings modal",
        viewport,
        SettingsModalApp::new(),
    )
}
