//! Text area — smoke test for the multi-line text widget.
//!
//! Single multi-line `text_area` plus a live preview of the global
//! selection state. Run interactively:
//!
//! ```text
//! cargo run -p damascene-examples --bin text_area
//! ```
//!
//! Things to try in the window:
//!
//! - Click anywhere in the field to focus it. The focus ring fades in.
//! - Type to insert characters; `Enter` inserts a newline.
//! - Backspace / Delete remove the selection (or one grapheme when
//!   collapsed).
//! - Arrow keys navigate; Up/Down move between lines preserving the
//!   visual column.
//! - Shift+arrows extend the selection (including across line breaks).
//! - PageUp / PageDown move by one visible editor page; Shift variants
//!   extend the selection.
//! - Escape collapses an active selection without editing the text.
//! - Drag across text to select. The selection band paints behind the
//!   selected glyphs, one rectangle per visual line.
//! - Home / End go to the start / end of the current line; Shift
//!   variants extend the selection.
//! - Ctrl+A selects all.
//! - Ctrl+C / Ctrl+X / Ctrl+V (Cmd on macOS) — copy / cut / paste via
//!   the system clipboard, including multi-line text from any other
//!   application.

use damascene_core::prelude::*;
use damascene_core::widgets::{text_area, text_input};

const PRESET: &str = "Multi-line text area.\n\
Try Enter for new lines, Up/Down to move between them.\n\
Shift+Arrow extends the selection across line breaks.";

const BODY_KEY: &str = "body";

struct Notes {
    body: String,
    selection: Selection,
    clipboard: Option<arboard::Clipboard>,
    /// Last text written to the Linux primary selection — see the
    /// matching field on the `text_input` example.
    last_primary: String,
    /// Set when an event moved the caret; consumed by
    /// `drain_scroll_requests` to push a single
    /// `ScrollRequest::EnsureVisible` so keyboard navigation past
    /// the visible region scrolls the body back to the caret.
    scroll_caret_into_view: bool,
    /// Drag-select auto-scroll requests collected during pointer
    /// drags past the viewport edges. Drained next frame.
    pending_scroll_requests: Vec<damascene_core::scroll::ScrollRequest>,
}

impl Default for Notes {
    fn default() -> Self {
        Self {
            body: PRESET.to_string(),
            selection: Selection::default(),
            clipboard: arboard::Clipboard::new().ok(),
            last_primary: String::new(),
            scroll_caret_into_view: false,
            pending_scroll_requests: Vec::new(),
        }
    }
}

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

impl App for Notes {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h2("Notes"),
            form([form_item([
                form_label("Body"),
                form_control(
                    text_area(BODY_KEY, &self.body, &self.selection).height(Size::Fixed(180.0)),
                ),
                form_description("Saved with the incident timeline."),
            ])]),
            spacer().height(Size::Fixed(tokens::SPACE_4)),
            preview_block(self),
            spacer().height(Size::Fixed(tokens::SPACE_4)),
            row([
                button("Clear").key("clear").ghost(),
                spacer(),
                button("Reset").key("reset").secondary(),
            ]),
        ])
        .padding(tokens::SPACE_7)
        .gap(tokens::SPACE_3)
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn drain_scroll_requests(&mut self) -> Vec<damascene_core::scroll::ScrollRequest> {
        let mut out: Vec<damascene_core::scroll::ScrollRequest> =
            std::mem::take(&mut self.pending_scroll_requests);
        if std::mem::take(&mut self.scroll_caret_into_view)
            && let Some(req) =
                text_area::caret_scroll_request_for(&self.body, &self.selection, BODY_KEY)
        {
            out.push(req);
        }
        out
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.kind == UiEventKind::SelectionChanged
            && let Some(sel) = event.selection.as_ref()
        {
            self.selection = sel.clone();
            self.sync_primary();
            return;
        }
        match (event.kind, event.route()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("clear")) => {
                self.body.clear();
                self.selection = Selection::default();
                return;
            }
            (UiEventKind::Click | UiEventKind::Activate, Some("reset")) => {
                self.body = PRESET.to_string();
                self.selection = Selection::default();
                return;
            }
            _ => {}
        }
        if event.target_key() != Some(BODY_KEY) {
            return;
        }

        // Linux middle-click paste: insert primary-clipboard text at
        // the click position. No-op on platforms without primary
        // selection.
        if event.kind == UiEventKind::MiddleClick {
            if let Some(byte) = text_area::caret_byte_at(&self.body, &event) {
                let mut local = TextSelection::caret(byte);
                if let Some(text) = primary::get(self.clipboard.as_mut()) {
                    text_input::replace_selection(&mut self.body, &mut local, &text);
                }
                self.selection.set_within(BODY_KEY, local);
                if self.selection.within(BODY_KEY).is_none() {
                    self.selection.range = Some(SelectionRange {
                        anchor: SelectionPoint::new(BODY_KEY, local.head),
                        head: SelectionPoint::new(BODY_KEY, local.head),
                    });
                }
                self.scroll_caret_into_view = true;
            }
            return;
        }

        // Drag-select auto-scroll: when the pointer is past the
        // visible top/bottom edge during a drag, queue a scroll
        // request that exposes the next line in that direction.
        if let Some(req) = text_area::drag_autoscroll_request_for(&event, BODY_KEY) {
            self.pending_scroll_requests.push(req);
        }

        apply_with_clipboard(
            &mut self.body,
            &mut self.selection,
            &event,
            self.clipboard.as_mut(),
        );
        // Any body-targeted event might have moved the caret —
        // request caret-into-view for the next drain. The runtime
        // resolver no-ops if the caret is already visible, so a
        // pointer click that doesn't move the head is harmless.
        self.scroll_caret_into_view = true;
        self.sync_primary();
    }
}

impl Notes {
    /// Mirror the current selection's text into the Linux primary
    /// buffer. Same shape as the matching helper in the text_input
    /// example — see that file for the rationale.
    fn sync_primary(&mut self) {
        let text = self
            .selection
            .within(BODY_KEY)
            .filter(|view| !view.is_collapsed())
            .map(|view| {
                let (lo, hi) = view.ordered();
                self.body[lo..hi].to_string()
            })
            .unwrap_or_default();
        if text == self.last_primary {
            return;
        }
        if !text.is_empty() {
            primary::set(self.clipboard.as_mut(), &text);
        }
        self.last_primary = text;
    }
}

fn apply_with_clipboard(
    value: &mut String,
    selection: &mut Selection,
    event: &UiEvent,
    clipboard: Option<&mut arboard::Clipboard>,
) {
    // The clipboard keystroke detector is shared with text_input — it
    // identifies Ctrl/Cmd+C/X/V independent of which widget handles the
    // body of the event.
    match text_input::clipboard_request(event) {
        Some(ClipboardKind::Copy) => {
            if let (Some(cb), Some(view)) = (clipboard, selection.within(BODY_KEY)) {
                let _ = cb.set_text(text_input::selected_text(value, view).to_string());
            }
        }
        Some(ClipboardKind::Cut) => {
            if let Some(view) = selection.within(BODY_KEY) {
                if let Some(cb) = clipboard {
                    let _ = cb.set_text(text_input::selected_text(value, view).to_string());
                }
                let mut local = view;
                text_input::replace_selection(value, &mut local, "");
                selection.set_within(BODY_KEY, local);
            }
        }
        Some(ClipboardKind::Paste) => {
            if let Some(cb) = clipboard
                && let Ok(text) = cb.get_text()
            {
                let mut local = selection.within(BODY_KEY).unwrap_or_default();
                text_input::replace_selection(value, &mut local, &text);
                if selection.within(BODY_KEY).is_some() {
                    selection.set_within(BODY_KEY, local);
                } else {
                    selection.range = Some(SelectionRange {
                        anchor: SelectionPoint::new(BODY_KEY, local.head),
                        head: SelectionPoint::new(BODY_KEY, local.head),
                    });
                }
            }
        }
        None => {
            text_area::apply_event(value, selection, BODY_KEY, event);
        }
    }
}

fn preview_block(notes: &Notes) -> El {
    let summary = match notes.selection.within(BODY_KEY) {
        Some(view) if view.is_collapsed() => format!(
            "len={}  caret={}  lines={}",
            notes.body.len(),
            view.head,
            notes.body.lines().count().max(1)
        ),
        Some(view) => {
            let (lo, hi) = view.ordered();
            format!(
                "len={}  selection={}..{}  selected={:?}",
                notes.body.len(),
                lo,
                hi,
                &notes.body[lo..hi]
            )
        }
        None => format!(
            "len={}  (selection elsewhere or empty)  lines={}",
            notes.body.len(),
            notes.body.lines().count().max(1)
        ),
    };
    titled_card("Live state", [mono(summary)])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 520.0);
    damascene_winit_wgpu::run(
        "Damascene — text_area smoke test",
        viewport,
        Notes::default(),
    )
}
