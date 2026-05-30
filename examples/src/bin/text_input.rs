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
//!   the system clipboard.
//! - Tab / Shift+Tab moves focus between fields.
//! - Empty fields show a muted placeholder hint until you type.
//! - The PIN field is capped at 6 characters via `max_length`.
//! - The Password field renders bullets, and Ctrl+C / Ctrl+X are
//!   suppressed (Ctrl+V still works — pasting *into* a password field
//!   is fine).

use damascene_core::prelude::*;
use damascene_core::widgets::text_input;

struct Form {
    name: String,
    email: String,
    pin: String,
    password: String,
    /// Single global selection field — every input reads / writes its
    /// slice through `selection.within(key)` instead of holding its
    /// own `TextSelection`.
    selection: Selection,
    clipboard: Option<arboard::Clipboard>,
    /// Last text written to the Linux primary selection. Compared
    /// against the current selection's text after every event so the
    /// auto-copy only fires on real changes (no redundant clipboard
    /// writes during shift-arrow extension or arrow-key navigation).
    last_primary: String,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            pin: String::new(),
            password: String::new(),
            selection: Selection::default(),
            // arboard fails to initialize on headless / display-less
            // environments. Treat clipboard as best-effort.
            clipboard: arboard::Clipboard::new().ok(),
            last_primary: String::new(),
        }
    }
}

/// Linux primary-selection helpers. On X11 / Wayland, highlighting
/// text writes it to a "primary" buffer separate from the system
/// clipboard, and middle-click pastes from that buffer at the click
/// position. On other platforms these are no-ops — primary selection
/// isn't a concept on macOS / Windows / web.
mod primary {
    #[cfg(target_os = "linux")]
    pub fn set(clipboard: Option<&mut arboard::Clipboard>, text: &str) {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        if let Some(cb) = clipboard {
            let _ = cb.set().clipboard(LinuxClipboardKind::Primary).text(text);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn get(clipboard: Option<&mut arboard::Clipboard>) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        let cb = clipboard?;
        cb.get().clipboard(LinuxClipboardKind::Primary).text().ok()
    }

    #[cfg(not(target_os = "linux"))]
    pub fn set(_clipboard: Option<&mut arboard::Clipboard>, _text: &str) {}

    #[cfg(not(target_os = "linux"))]
    pub fn get(_clipboard: Option<&mut arboard::Clipboard>) -> Option<String> {
        None
    }
}

const PIN_OPTS: TextInputOpts<'_> = TextInputOpts {
    placeholder: Some("6 digits"),
    max_length: Some(6),
    mask: MaskMode::None,
};

const PASSWORD_OPTS: TextInputOpts<'_> = TextInputOpts {
    placeholder: Some("••••••••"),
    max_length: None,
    mask: MaskMode::Password,
};

impl Form {
    fn opts_for(&self, key: &str) -> TextInputOpts<'static> {
        match key {
            "name" => TextInputOpts::default().placeholder("Your name"),
            "email" => TextInputOpts::default().placeholder("you@example.com"),
            "pin" => PIN_OPTS,
            "password" => PASSWORD_OPTS,
            _ => TextInputOpts::default(),
        }
    }

    fn value_for(&self, key: &str) -> Option<&str> {
        match key {
            "name" => Some(&self.name),
            "email" => Some(&self.email),
            "pin" => Some(&self.pin),
            "password" => Some(&self.password),
            _ => None,
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
                        &self.name,
                        &self.selection,
                        "name",
                        self.opts_for("name"),
                    )),
                    form_description("Shown on shared reports."),
                ]),
                form_item([
                    form_label("Email"),
                    form_control(text_input_with(
                        &self.email,
                        &self.selection,
                        "email",
                        self.opts_for("email"),
                    )),
                    form_description("Used for account notifications."),
                ]),
                form_item([
                    form_label("PIN"),
                    form_control(text_input_with(
                        &self.pin,
                        &self.selection,
                        "pin",
                        self.opts_for("pin"),
                    )),
                    form_description("Four digits for quick approval."),
                ]),
                form_item([
                    form_label("Password"),
                    form_control(text_input_with(
                        &self.password,
                        &self.selection,
                        "password",
                        self.opts_for("password"),
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

    fn on_event(&mut self, event: UiEvent) {
        // Library-emitted selection updates: the runtime doesn't
        // touch text_input's own selection (text_input handles it
        // inside apply_event), but `SelectionChanged` fires when a
        // press lands somewhere non-selectable / non-focusable to
        // clear the active static-text selection. Honor that.
        if event.kind == UiEventKind::SelectionChanged
            && let Some(sel) = event.selection.as_ref()
        {
            self.selection = sel.clone();
            self.sync_primary();
            return;
        }
        match (event.kind, event.route()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("clear")) => {
                self.name.clear();
                self.email.clear();
                self.pin.clear();
                self.password.clear();
                self.selection = Selection::default();
                return;
            }
            (UiEventKind::Click | UiEventKind::Activate, Some("submit")) => {
                eprintln!(
                    "submit: name={:?} email={:?} pin={:?} password=<{} chars>",
                    self.name,
                    self.email,
                    self.pin,
                    self.password.chars().count()
                );
                return;
            }
            _ => {}
        }
        let key = match event.target_key() {
            Some(k) => k.to_owned(),
            None => return,
        };
        let opts = self.opts_for(&key);
        let (value, key_str): (&mut String, &str) = match key.as_str() {
            "name" => (&mut self.name, "name"),
            "email" => (&mut self.email, "email"),
            "pin" => (&mut self.pin, "pin"),
            "password" => (&mut self.password, "password"),
            _ => return,
        };

        // Linux middle-click paste: insert primary-clipboard text at
        // the click position, leaving the system clipboard untouched.
        // No-op on platforms without primary selection.
        if event.kind == UiEventKind::MiddleClick {
            if let Some(byte) = text_input::caret_byte_at(value, &event, &opts) {
                let mut local = TextSelection::caret(byte);
                if let Some(text) = primary::get(self.clipboard.as_mut()) {
                    text_input::replace_selection_with(value, &mut local, &text, &opts);
                }
                self.selection.set_within(key_str, local);
                if self.selection.within(key_str).is_none() {
                    self.selection.range = Some(SelectionRange {
                        anchor: SelectionPoint::new(key_str, local.head),
                        head: SelectionPoint::new(key_str, local.head),
                    });
                }
            }
            return;
        }

        apply_with_clipboard(
            value,
            &mut self.selection,
            key_str,
            &event,
            &opts,
            self.clipboard.as_mut(),
        );
        self.sync_primary();
    }
}

impl Form {
    /// Mirror the current selection's text into the Linux primary
    /// buffer. Browsers and native Linux apps do this on every
    /// selection change; middle-click then pastes the most recently
    /// highlighted text. We dedupe against `last_primary` so a long
    /// shift-arrow extension or arrow-key drift inside a stable
    /// selection doesn't churn the clipboard.
    fn sync_primary(&mut self) {
        let text = self
            .selected_text()
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        if text == self.last_primary {
            return;
        }
        if !text.is_empty() {
            primary::set(self.clipboard.as_mut(), &text);
        }
        self.last_primary = text;
    }

    fn selected_text(&self) -> Option<String> {
        let r = self.selection.range.as_ref()?;
        if r.anchor.key != r.head.key {
            return None; // cross-leaf selections can't live across these inputs
        }
        let value = self.value_for(&r.anchor.key)?;
        let lo = r.anchor.byte.min(r.head.byte).min(value.len());
        let hi = r.anchor.byte.max(r.head.byte).min(value.len());
        if lo >= hi {
            return None;
        }
        Some(value[lo..hi].to_string())
    }
}

fn apply_with_clipboard(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    event: &UiEvent,
    opts: &TextInputOpts<'_>,
    clipboard: Option<&mut arboard::Clipboard>,
) {
    match text_input::clipboard_request_for(event, opts) {
        Some(ClipboardKind::Copy) => {
            if let (Some(cb), Some(view)) = (clipboard, selection.within(key)) {
                let _ = cb.set_text(text_input::selected_text(value, view).to_string());
            }
        }
        Some(ClipboardKind::Cut) => {
            if let Some(view) = selection.within(key) {
                if let Some(cb) = clipboard {
                    let _ = cb.set_text(text_input::selected_text(value, view).to_string());
                }
                let mut local = view;
                text_input::replace_selection(value, &mut local, "");
                selection.set_within(key, local);
            }
        }
        Some(ClipboardKind::Paste) => {
            if let Some(cb) = clipboard
                && let Ok(text) = cb.get_text()
            {
                let mut local = selection.within(key).unwrap_or_default();
                text_input::replace_selection_with(value, &mut local, &text, opts);
                selection.set_within(key, local);
                // If selection wasn't in our key, claim it now.
                if selection.within(key).is_none() {
                    selection.range = Some(SelectionRange {
                        anchor: SelectionPoint::new(key, local.head),
                        head: SelectionPoint::new(key, local.head),
                    });
                }
            }
        }
        None => {
            text_input::apply_event_with(value, selection, key, event, opts);
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
    let value = form.value_for(key).unwrap_or("");
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
    damascene_winit_wgpu::run("Damascene — text_input smoke test", viewport, Form::default())
}
