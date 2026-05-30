//! Hotkey-driven picker — proof point for the hotkey system.
//!
//! No `match e.key_press` in the app: every keyboard interaction goes
//! through `App::hotkeys()` and the library routes the named chord
//! back as a `UiEventKind::Hotkey` event. The author's `on_event` is
//! a flat dispatch on event names, the same shape as click handling.
//!
//! Try:
//! - `j` / `k` to move selection
//! - `g` to jump to top, `Shift+G` to jump to bottom
//! - Enter (or click) to "open" the selected item
//! - `/` to focus the search header (just toggles a placeholder string)
//! - Ctrl+L to clear the search
//!
//! Run: `cargo run -p damascene-examples --bin hotkey_picker`

use damascene_core::prelude::*;

const ITEMS: &[&str] = &[
    "build the renderer",
    "wire focus traversal",
    "ship scroll primitive",
    "design hotkey system",
    "polish artifact bundle",
    "write the next manifesto",
    "rest, eat, sleep, repeat",
];

struct Picker {
    selected: usize,
    opened: Option<usize>,
    search: String,
    search_active: bool,
}

impl App for Picker {
    fn build(&self, _cx: &BuildCx) -> El {
        let header = row([
            badge(if self.search_active {
                "/ active"
            } else {
                "/ search"
            })
            .info(),
            text(if self.search.is_empty() {
                "(press / to type, Ctrl+L to clear)".to_string()
            } else {
                format!("\"{}\"", self.search)
            })
            .muted(),
            spacer(),
            text(format!("{}/{}", self.selected + 1, ITEMS.len())).muted(),
        ])
        .gap(tokens::SPACE_2);

        let rows: Vec<El> = ITEMS
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let mut r = row([
                    badge(format!("{}", i + 1)).muted(),
                    text(*label),
                    spacer(),
                    text(if Some(i) == self.opened { "opened" } else { "" }).muted(),
                ])
                .gap(tokens::SPACE_2)
                .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
                .height(Size::Fixed(40.0))
                .key(format!("row-{i}"))
                .stroke(tokens::BORDER)
                .radius(tokens::RADIUS_SM);
                if i == self.selected {
                    r = r.fill(tokens::CARD);
                }
                r
            })
            .collect();

        column([
            h2("Hotkey picker"),
            text("Keyboard-only navigation — no per-key match in the app.").muted(),
            header,
            scroll(rows).key("items").height(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
    }

    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        let mut out = vec![
            (KeyChord::vim('j'), "move-down".into()),
            (KeyChord::vim('k'), "move-up".into()),
            (KeyChord::vim('g'), "go-top".into()),
            (KeyChord::ctrl('l'), "clear-search".into()),
            (KeyChord::named(UiKey::Enter), "open".into()),
            (
                KeyChord::named(UiKey::Character("/".into())),
                "toggle-search".into(),
            ),
            (
                KeyChord::named(UiKey::Character("G".into())).with_modifiers(KeyModifiers {
                    shift: true,
                    ..Default::default()
                }),
                "go-bottom".into(),
            ),
        ];
        // While search is active, intercept printable chars to append.
        if self.search_active {
            for c in b'a'..=b'z' {
                out.push((KeyChord::vim(c as char), format!("search-{}", c as char)));
            }
        }
        out
    }

    fn on_event(&mut self, event: UiEvent) {
        match (event.kind, event.route()) {
            (UiEventKind::Hotkey, Some("move-down")) if self.selected + 1 < ITEMS.len() => {
                self.selected += 1;
            }
            (UiEventKind::Hotkey, Some("move-up")) => {
                self.selected = self.selected.saturating_sub(1);
            }
            (UiEventKind::Hotkey, Some("go-top")) => self.selected = 0,
            (UiEventKind::Hotkey, Some("go-bottom")) => self.selected = ITEMS.len() - 1,
            (UiEventKind::Hotkey, Some("open")) => self.opened = Some(self.selected),
            (UiEventKind::Hotkey, Some("toggle-search")) => {
                self.search_active = !self.search_active
            }
            (UiEventKind::Hotkey, Some("clear-search")) => {
                self.search.clear();
                self.search_active = false;
            }
            (UiEventKind::Hotkey, Some(name)) if name.starts_with("search-") => {
                if let Some(c) = name.strip_prefix("search-").and_then(|s| s.chars().next()) {
                    self.search.push(c);
                }
            }
            (UiEventKind::Click | UiEventKind::Activate, Some(k)) => {
                if let Some(rest) = k.strip_prefix("row-")
                    && let Ok(i) = rest.parse::<usize>()
                {
                    self.selected = i;
                    self.opened = Some(i);
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 600.0, 460.0);
    damascene_winit_wgpu::run(
        "Damascene — hotkey_picker",
        viewport,
        Picker {
            selected: 0,
            opened: None,
            search: String::new(),
            search_active: false,
        },
    )
}
