//! Hotkeys & selection — keyboard-only navigation pattern.
//!
//! `App::hotkeys` returns a list of `(KeyChord, route_name)` pairs
//! scoped to the active section, so chords don't leak into other
//! pages. The picker below uses j/k to move, /, Enter to open,
//! Ctrl+L to clear search.

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

#[derive(Default)]
pub struct State {
    pub selected: usize,
    pub opened: Option<usize>,
    pub search: String,
    pub search_active: bool,
}

pub fn view(state: &State) -> El {
    let header = row([
        badge(if state.search_active {
            "/ active"
        } else {
            "/ search"
        })
        .info(),
        text(if state.search.is_empty() {
            "(press / to type, Ctrl+L to clear)".to_string()
        } else {
            format!("\"{}\"", state.search)
        })
        .muted(),
        spacer(),
        text(format!("{}/{}", state.selected + 1, ITEMS.len())).muted(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center);

    let rows: Vec<El> = ITEMS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let mut r = row([
                badge(format!("{}", i + 1)).muted(),
                text(*label),
                spacer(),
                text(if Some(i) == state.opened {
                    "opened"
                } else {
                    ""
                })
                .muted(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center)
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
            .height(Size::Fixed(40.0))
            .key(format!("hotkeys-row-{i}"))
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_SM);
            if i == state.selected {
                r = r.selected();
            }
            r
        })
        .collect();

    column([
        h1("Hotkeys"),
        paragraph(
            "Keyboard-driven list. `App::hotkeys` returns chords scoped \
             to this section so j/k/g/G/Enter don't leak into other \
             pages. Click any row, or use the keyboard.",
        )
        .muted(),
        header,
        scroll(rows).key("hotkeys-scroll").height(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    match (e.kind, e.route()) {
        (UiEventKind::Hotkey, Some("hotkeys-move-down")) if state.selected + 1 < ITEMS.len() => {
            state.selected += 1
        }
        (UiEventKind::Hotkey, Some("hotkeys-move-up")) => {
            state.selected = state.selected.saturating_sub(1)
        }
        (UiEventKind::Hotkey, Some("hotkeys-go-top")) => state.selected = 0,
        (UiEventKind::Hotkey, Some("hotkeys-go-bottom")) => state.selected = ITEMS.len() - 1,
        (UiEventKind::Hotkey, Some("hotkeys-open")) => state.opened = Some(state.selected),
        (UiEventKind::Hotkey, Some("hotkeys-toggle-search")) => {
            state.search_active = !state.search_active
        }
        (UiEventKind::Hotkey, Some("hotkeys-clear-search")) => {
            state.search.clear();
            state.search_active = false;
        }
        (UiEventKind::Hotkey, Some(name)) if name.starts_with("hotkeys-search-") => {
            if let Some(c) = name
                .strip_prefix("hotkeys-search-")
                .and_then(|s| s.chars().next())
            {
                state.search.push(c);
            }
        }
        (UiEventKind::Click | UiEventKind::Activate, Some(k)) => {
            if let Some(rest) = k.strip_prefix("hotkeys-row-")
                && let Ok(i) = rest.parse::<usize>()
            {
                state.selected = i;
                state.opened = Some(i);
            }
        }
        _ => {}
    }
}

pub fn hotkeys(state: &State) -> Vec<(KeyChord, String)> {
    let mut out = vec![
        (KeyChord::vim('j'), "hotkeys-move-down".into()),
        (KeyChord::vim('k'), "hotkeys-move-up".into()),
        (KeyChord::vim('g'), "hotkeys-go-top".into()),
        (KeyChord::ctrl('l'), "hotkeys-clear-search".into()),
        (KeyChord::named(UiKey::Enter), "hotkeys-open".into()),
        (
            KeyChord::named(UiKey::Character("/".into())),
            "hotkeys-toggle-search".into(),
        ),
        (
            KeyChord::named(UiKey::Character("G".into())).with_modifiers(KeyModifiers {
                shift: true,
                ..Default::default()
            }),
            "hotkeys-go-bottom".into(),
        ),
    ];
    if state.search_active {
        for c in b'a'..=b'z' {
            out.push((
                KeyChord::vim(c as char),
                format!("hotkeys-search-{}", c as char),
            ));
        }
    }
    out
}
