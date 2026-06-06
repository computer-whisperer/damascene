//! Static text selection — drag-select on paragraphs and copy to the
//! system clipboard. P1a slice: single-leaf selection only (drag is
//! clamped to the paragraph the press started in).
//!
//! ```text
//! cargo run -p damascene-examples --bin text_selection
//! ```
//!
//! Things to try:
//!
//! - Drag across any of the three paragraphs to highlight a range —
//!   the selection band paints behind the glyphs.
//! - Drag across a different paragraph: the previous selection clears
//!   automatically (single-selection invariant).
//! - Click on empty space (outside any paragraph) to clear the
//!   selection.
//! - Ctrl+C / Cmd+C copies the current selection to the system
//!   clipboard. The "Last copy:" line below echoes what was sent.
//!
//! Cross-leaf drag (drag from one paragraph into the next) works:
//! the head migrates into whichever selectable leaf the pointer is
//! over, and the painter renders a band on every leaf the range
//! spans (anchor leaf from anchor.byte to end, middle leaves fully,
//! head leaf from start to head.byte).

use damascene_core::prelude::*;
use damascene_core::selection;
use damascene_core::widgets::text_input::{self, ClipboardKind};

const PARA_A: &str = "Damascene selection model: pointer-down on a selectable text leaf starts a drag, \
     pointer-move extends it, pointer-up ends it. The selection itself persists.";
const PARA_B: &str = "Single-selection invariant: at most one selection across the whole app at a time. \
     A new pointer-down on a different leaf transfers ownership.";
const PARA_C: &str = "Library-side state lives in UiState (drag in progress); the canonical Selection \
     value is owned by the application and threaded back into App::selection() per frame.";

struct Demo {
    selection: Selection,
    last_copy: String,
    clipboard: Option<arboard::Clipboard>,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            selection: Selection::default(),
            last_copy: String::new(),
            clipboard: arboard::Clipboard::new().ok(),
        }
    }
}

impl App for Demo {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h2("Static text selection"),
            paragraph(PARA_A).key("para-a").selectable(),
            paragraph(PARA_B).key("para-b").selectable(),
            paragraph(PARA_C).key("para-c").selectable(),
            spacer().height(Size::Fixed(tokens::SPACE_4)),
            titled_card(
                "Selection state",
                [
                    state_line(&self.selection),
                    mono(format!("Last copy: {:?}", self.last_copy)).muted(),
                ],
            ),
        ])
        .padding(tokens::SPACE_7)
        .gap(tokens::SPACE_3)
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.kind == UiEventKind::SelectionChanged
            && let Some(sel) = event.selection.as_ref()
        {
            self.selection = sel.clone();
            return;
        }
        if let Some(ClipboardKind::Copy) = text_input::clipboard_request(&event) {
            let theme = self.theme();
            let cx = BuildCx::new(&theme);
            let tree = self.build(&cx);
            if let Some(text) = selection::selected_text(&tree, &self.selection) {
                if let Some(cb) = self.clipboard.as_mut() {
                    let _ = cb.set_text(text.clone());
                }
                self.last_copy = text;
            }
        }
    }
}

fn state_line(sel: &Selection) -> El {
    let summary = match &sel.range {
        None => String::from("(no selection)"),
        Some(r) if r.anchor.key == r.head.key => format!(
            "{}: {}..{}",
            r.anchor.key,
            r.anchor.byte.min(r.head.byte),
            r.anchor.byte.max(r.head.byte)
        ),
        Some(r) => format!(
            "{}@{} → {}@{}",
            r.anchor.key, r.anchor.byte, r.head.key, r.head.byte
        ),
    };
    mono(summary)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 540.0);
    damascene_winit_wgpu::run("Damascene — text selection demo", viewport, Demo::default())
}
