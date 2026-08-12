//! Announce — canonical shape for screen-reader announcements.
//!
//! Demonstrates the fire-and-forget live-region pattern: event
//! handlers queue [`Announcement`]s in a `Vec` field, and
//! `App::drain_announcements` hands them to the runtime once per
//! frame. The runtime synthesizes an invisible ARIA live region at
//! the root, so nothing here paints — with a screen reader running
//! (or the AT-SPI bus monitored), each button press is spoken.
//!
//! Run interactively:
//!
//! ```text
//! cargo run -p damascene-examples --bin announce
//! ```
//!
//! Things to try (with Orca or another screen reader running):
//!
//! - "Save" simulates a background task completing: a *polite*
//!   announcement, spoken at the next graceful opportunity.
//! - "Disconnect" fires an *assertive* announcement (`role="alert"`),
//!   which interrupts current speech.
//! - "Toast" pushes a regular toast — toast cards are polite live
//!   regions themselves, so they announce with no extra code. While a
//!   screen reader is connected the toast also stays on screen until
//!   dismissed instead of auto-expiring.
//! - `--auto` announces every 2 seconds without interaction (used for
//!   headless bus verification).

use std::time::{Duration, Instant};

use damascene_core::announce::Announcement;
use damascene_core::prelude::*;

struct Demo {
    saves: u32,
    pending: Vec<Announcement>,
    pending_toasts: Vec<ToastSpec>,
    /// `--auto`: self-sustaining timed announcements. The queued
    /// announcement keeps the redraw loop alive through its retention
    /// window, so each drain can schedule the next push.
    auto: Option<Instant>,
    auto_count: u32,
}

impl App for Demo {
    fn build(&self, _cx: &BuildCx) -> El {
        let main = column([
            h2("Screen-reader announcements"),
            paragraph(
                "Buttons queue ARIA live-region messages. Nothing visible \
                 changes — run a screen reader (or monitor the AT-SPI bus) \
                 to hear them.",
            )
            .muted()
            .wrap_text(),
            row([
                button(format!("Save ({})", self.saves)).key("save"),
                button("Disconnect").destructive().key("disconnect"),
                button("Toast").secondary().key("toast"),
            ])
            .gap(tokens::SPACE_3),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .align(Align::Start);
        overlays(main, [])
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.is_click_or_activate("save") {
            self.saves += 1;
            self.pending.push(Announcement::polite(format!(
                "Saved, revision {}",
                self.saves
            )));
        } else if event.is_click_or_activate("disconnect") {
            self.pending
                .push(Announcement::assertive("Connection lost — reconnecting"));
        } else if event.is_click_or_activate("toast") {
            self.pending_toasts
                .push(ToastSpec::success("Toast cards announce themselves"));
        }
    }

    fn drain_announcements(&mut self) -> Vec<Announcement> {
        if let Some(next_at) = self.auto
            && Instant::now() >= next_at
        {
            self.auto_count += 1;
            self.auto = Some(next_at + Duration::from_secs(2));
            self.pending
                .push(Announcement::polite(format!("auto {}", self.auto_count)));
        }
        std::mem::take(&mut self.pending)
    }

    fn drain_toasts(&mut self) -> Vec<ToastSpec> {
        std::mem::take(&mut self.pending_toasts)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auto = std::env::args().any(|a| a == "--auto");
    let viewport = Rect::new(0.0, 0.0, 560.0, 240.0);
    damascene_winit_wgpu::run(
        "Damascene — announcements",
        viewport,
        Demo {
            saves: 0,
            pending: if auto {
                // Seed one announcement so the redraw loop starts
                // without interaction; the drain hook sustains it.
                vec![Announcement::polite("auto 0")]
            } else {
                Vec::new()
            },
            pending_toasts: Vec::new(),
            auto: auto.then(|| Instant::now() + Duration::from_secs(2)),
            auto_count: 0,
        },
    )
}
