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
//!   application. On Linux, selecting text also fills the primary
//!   selection and middle-click pastes it.
//!
//! The app has no clipboard code: the `damascene-winit-wgpu` host owns
//! the platform clipboard and delivers a cut, a paste and a
//! middle-click paste as events `text_area::apply_event` folds.

use damascene_core::prelude::*;
use damascene_core::widgets::text_area;

const PRESET: &str = "Multi-line text area.\n\
Try Enter for new lines, Up/Down to move between them.\n\
Shift+Arrow extends the selection across line breaks.";

const BODY_KEY: &str = "body";

struct Notes {
    body: String,
    selection: Selection,
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
            scroll_caret_into_view: false,
            pending_scroll_requests: Vec::new(),
        }
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
        if !event.is_route(BODY_KEY) {
            return;
        }

        // Drag-select auto-scroll: when the pointer is past the
        // visible top/bottom edge during a drag, queue a scroll
        // request that exposes the next line in that direction.
        if let Some(req) = text_area::drag_autoscroll_request_for(&event, BODY_KEY) {
            self.pending_scroll_requests.push(req);
        }

        text_area::apply_event(&mut self.body, &mut self.selection, &event, BODY_KEY);
        // Any body-routed event might have moved the caret —
        // request caret-into-view for the next drain. The runtime
        // resolver no-ops if the caret is already visible, so a
        // pointer click that doesn't move the head is harmless.
        self.scroll_caret_into_view = true;
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
