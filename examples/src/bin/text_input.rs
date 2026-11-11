//! Text input — smoke test for the single-line text widget.
//!
//! Four fields plus a live preview of the global selection state.
//! Run interactively:
//!
//! ```text
//! cargo run -p damascene-examples --bin text_input
//! ```
//!
//! Things to try in the window:
//!
//! - Click a field to focus it (focus ring fades in around it).
//! - Type to insert characters at the caret.
//! - Drag across text to select; the selection band paints behind the
//!   selected glyphs.
//! - Shift+ArrowLeft / ArrowRight / Home / End extend the selection.
//! - Plain ArrowLeft / ArrowRight / Home / End collapse + move.
//! - Backspace / Delete remove the selection if non-empty, otherwise
//!   one grapheme.
//! - Type while a selection is active — the selection is replaced.
//! - Ctrl+A selects all in the focused field.
//! - Ctrl+C / Ctrl+X / Ctrl+V (Cmd on macOS) — copy / cut / paste via
//!   the system clipboard. On Linux, selecting text also fills the
//!   primary selection and middle-click pastes it.
//! - Tab / Shift+Tab moves focus between fields.
//! - Empty fields show a muted placeholder hint until you type.
//! - The PIN field is capped at 6 characters via `max_length`.
//! - The Password field renders bullets.
//!
//! The app has no clipboard code. The `damascene-winit-wgpu` host owns
//! the platform clipboard: it copies the runtime's selection on
//! Ctrl+C, turns a cut into a Delete key and a paste into a
//! `TextInput`, and attaches the primary selection's text to a
//! middle click — all of which `text_input::apply_event` folds like
//! any other edit.

use damascene_core::prelude::*;
use damascene_core::widgets::text_input;

#[derive(Default)]
struct Form {
    name: String,
    email: String,
    pin: String,
    password: String,
    /// Single global selection field — every input reads / writes its
    /// slice through `selection.within(key)` instead of holding its
    /// own `TextSelection`.
    selection: Selection,
}

const PIN_OPTS: TextInputOpts<'_> = TextInputOpts {
    placeholder: Some("6 digits"),
    max_length: Some(6),
    mask: MaskMode::None,
    // Digits-only field: fixed-width numerals keep entry steady.
    tabular_numerals: true,
    suffix: None,
};

const PASSWORD_OPTS: TextInputOpts<'_> = TextInputOpts {
    placeholder: Some("••••••••"),
    max_length: None,
    mask: MaskMode::Password,
    tabular_numerals: false,
    suffix: None,
};

fn opts_for(key: &str) -> TextInputOpts<'static> {
    match key {
        "name" => TextInputOpts::default().placeholder("Your name"),
        "email" => TextInputOpts::default().placeholder("you@example.com"),
        "pin" => PIN_OPTS,
        "password" => PASSWORD_OPTS,
        _ => TextInputOpts::default(),
    }
}

impl Form {
    fn value_for(&self, key: &str) -> &str {
        match key {
            "name" => &self.name,
            "email" => &self.email,
            "pin" => &self.pin,
            "password" => &self.password,
            _ => "",
        }
    }
}

impl App for Form {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h2("Form"),
            form([
                form_item([
                    form_label("Name"),
                    form_control(text_input_with(
                        "name",
                        &self.name,
                        &self.selection,
                        opts_for("name"),
                    )),
                    form_description("Shown on shared reports."),
                ]),
                form_item([
                    form_label("Email"),
                    form_control(text_input_with(
                        "email",
                        &self.email,
                        &self.selection,
                        opts_for("email"),
                    )),
                    form_description("Used for account notifications."),
                ]),
                form_item([
                    form_label("PIN"),
                    form_control(text_input_with(
                        "pin",
                        &self.pin,
                        &self.selection,
                        opts_for("pin"),
                    )),
                    form_description("Four digits for quick approval."),
                ]),
                form_item([
                    form_label("Password"),
                    form_control(text_input_with(
                        "password",
                        &self.password,
                        &self.selection,
                        opts_for("password"),
                    )),
                    form_description("Required before exporting private data."),
                ]),
            ]),
            spacer().height(Size::Fixed(tokens::SPACE_4)),
            preview_block(self),
            spacer().height(Size::Fixed(tokens::SPACE_4)),
            row([
                button("Clear").key("clear").ghost(),
                spacer(),
                button("Submit").key("submit").primary(),
            ]),
        ])
        .padding(tokens::SPACE_7)
        .gap(tokens::SPACE_3)
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        // The runtime clears a static-text selection when a press
        // lands somewhere non-selectable; an input's own selection
        // moves inside `apply_event`.
        if event.kind == UiEventKind::SelectionChanged
            && let Some(sel) = event.selection.as_ref()
        {
            self.selection = sel.clone();
            return;
        }
        if event.is_click_or_activate("clear") {
            *self = Self::default();
            return;
        }
        if event.is_click_or_activate("submit") {
            eprintln!(
                "submit: name={:?} email={:?} pin={:?} password=<{} chars>",
                self.name,
                self.email,
                self.pin,
                self.password.chars().count()
            );
            return;
        }
        // Every input's fold, unconditionally: each handles only the
        // events the runtime routed to its key.
        for (value, key) in [
            (&mut self.name, "name"),
            (&mut self.email, "email"),
            (&mut self.pin, "pin"),
            (&mut self.password, "password"),
        ] {
            text_input::apply_event_with(value, &mut self.selection, &event, key, &opts_for(key));
        }
    }
}

fn preview_block(form: &Form) -> El {
    titled_card(
        "Live state",
        [
            preview_line(form, "name"),
            preview_line(form, "email"),
            preview_line(form, "pin"),
            mono(format!("global: {:?}", form.selection)),
        ],
    )
}

fn preview_line(form: &Form, key: &str) -> El {
    let value = form.value_for(key);
    let summary = match form.selection.within(key) {
        Some(view) if view.is_collapsed() => {
            format!("{key:>8} = {:?}  caret={}", value, view.head)
        }
        Some(view) => {
            let (lo, hi) = view.ordered();
            format!(
                "{key:>8} = {:?}  selection={}..{}  ({:?})",
                value,
                lo,
                hi,
                &value[lo..hi]
            )
        }
        None => format!("{key:>8} = {:?}  (not selected)", value),
    };
    mono(summary)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 640.0, 420.0);
    damascene_winit_wgpu::run(
        "Damascene — text_input smoke test",
        viewport,
        Form::default(),
    )
}
