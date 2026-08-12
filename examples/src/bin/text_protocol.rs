//! Text protocol — canonical shape for screen-reader-editable text.
//!
//! A `text_input` and a `text_area` wired the standard way (app owns
//! value + `Selection`, `apply_event` folds edits, `SelectionChanged`
//! adopted globally). With the `accessibility` feature (default in
//! `damascene-winit-wgpu`) that is *all* an app does — the runtime
//! lowers each field into AccessKit `TextRun`s, reports the caret, and
//! routes AT-driven caret moves back as the same `SelectionChanged`
//! events this file already folds.
//!
//! Run interactively (with Orca or another screen reader: arrow
//! through the fields character-by-character and word-by-word; the
//! spoken caret follows the painted one):
//!
//! ```text
//! cargo run -p damascene-examples --bin text_protocol
//! ```
//!
//! `--auto` self-drives an edit script (type, caret home, select a
//! word, caret end — one step per 2 s) for headless AT-SPI bus
//! verification: with the org.a11y flags flipped on, `dbus-monitor`
//! on the a11y bus shows `object:text-changed:insert`,
//! `object:text-caret-moved`, and `object:text-selection-changed`
//! events synthesized by the adapter from our tree diffs.

use std::time::{Duration, Instant};

use damascene_core::prelude::*;
use damascene_core::widgets::{text_area, text_input};

struct Demo {
    field: String,
    notes: String,
    selection: Selection,
    /// `--auto`: when the next scripted edit fires, and which step.
    auto: Option<Instant>,
    step: u32,
    focus_requests: Vec<String>,
}

impl Demo {
    /// One scripted edit per tick: exactly the state changes a user
    /// typing and arrowing would produce, so the adapter's frame diff
    /// emits the same AT-SPI signals.
    fn auto_step(&mut self) {
        match self.step % 4 {
            0 => {
                // "Type" a word at the end of the field.
                if self.field.len() > 60 {
                    self.field.clear();
                }
                self.field.push_str("hello ");
                self.selection = Selection::caret("field", self.field.len());
            }
            1 => {
                // Caret to the start (a pure caret move).
                self.selection = Selection::caret("field", 0);
            }
            2 => {
                // Select the first word (a real selection).
                let end = self.field.find(' ').unwrap_or(self.field.len());
                self.selection = Selection {
                    range: Some(SelectionRange {
                        anchor: SelectionPoint::new("field", 0),
                        head: SelectionPoint::new("field", end),
                    }),
                };
            }
            _ => {
                // Caret back to the end.
                self.selection = Selection::caret("field", self.field.len());
            }
        }
        self.step += 1;
    }
}

impl App for Demo {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h2("AccessKit text protocol"),
            paragraph(
                "Both fields expose per-character text runs, caret, and \
                 selection to assistive technology. Screen-reader caret \
                 commands route back as SelectionChanged events.",
            )
            .muted()
            .wrap_text(),
            form_item([
                form_label("Single line"),
                form_control(text_input("field", &self.field, &self.selection)),
            ]),
            form_item([
                form_label("Notes"),
                form_control(text_area("notes", &self.notes, &self.selection)),
            ]),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .align(Align::Stretch)
    }

    fn before_build(&mut self) {
        if let Some(next_at) = self.auto
            && Instant::now() >= next_at
        {
            self.auto = Some(next_at + Duration::from_secs(2));
            self.auto_step();
        }
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        // Library-emitted selection updates — including AT-driven
        // SetTextSelection actions — arrive here; adopt them.
        if event.kind == UiEventKind::SelectionChanged
            && let Some(sel) = event.selection.as_ref()
        {
            self.selection = sel.clone();
            return;
        }
        match event.target_key() {
            Some("field") => {
                text_input::apply_event(&mut self.field, &mut self.selection, &event, "field");
            }
            Some("notes") => {
                text_area::apply_event(&mut self.notes, &mut self.selection, &event, "notes");
            }
            _ => {}
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn drain_focus_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.focus_requests)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let auto = std::env::args().any(|a| a == "--auto");
    let viewport = Rect::new(0.0, 0.0, 560.0, 360.0);
    damascene_winit_wgpu::run(
        "Damascene — text protocol",
        viewport,
        Demo {
            field: String::new(),
            notes: "First line.\nSecond line, long enough to wrap when the \
                    window is narrow."
                .to_string(),
            selection: Selection::default(),
            auto: auto.then(|| Instant::now() + Duration::from_secs(2)),
            step: 0,
            // Focus the field up front in --auto so the caret blink
            // keeps the redraw loop (and the scripted edits) alive
            // without interaction.
            focus_requests: if auto {
                vec!["field".to_string()]
            } else {
                Vec::new()
            },
        },
    )
}
