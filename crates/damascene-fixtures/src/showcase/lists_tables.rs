//! Lists, items, and tables — three flavours of row layouts.
//!
//! - **Plain rows** (the List demo): scrollable selectable rows for
//!   apps that just need an array of clickable lines.
//! - **Items**: shadcn-shaped object rows with media / title /
//!   description / actions slots.
//! - **Tables**: structured tabular data with a header and aligned
//!   cells.

use damascene_core::prelude::*;

pub struct State {
    pub list_selected: Option<usize>,
    pub item_selected: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            list_selected: Some(2),
            item_selected: "items:row:repo:damascene".into(),
        }
    }
}

pub fn view(state: &State, cx: &BuildCx) -> El {
    let phone = super::is_phone(cx);
    scroll([column([
        h1("Lists & tables"),
        paragraph(
            "Three flavours of row layouts. Plain rows for clickable \
             lines; `item` for shadcn-shaped object rows with media, \
             title, description, and actions slots; and `table` for \
             structured tabular data.",
        )
        .muted(),
        section_label("Plain rows"),
        plain_list(state),
        section_label("Items"),
        items_demo(state, phone),
        section_label("Table"),
        table_demo(),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    let Some(k) = e.route() else { return };
    if let Some(rest) = k.strip_prefix("lt-list-row-")
        && let Ok(i) = rest.parse::<usize>()
    {
        state.list_selected = Some(i);
        return;
    }
    if k.starts_with("items:row:") {
        state.item_selected = k.to_string();
    }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn plain_list(state: &State) -> El {
    let rows: Vec<El> = (0..6)
        .map(|i| {
            let selected = Some(i) == state.list_selected;
            let mut r = row([
                badge(format!("#{i}")).info(),
                text(format!("Item {i}")).bold(),
                spacer(),
                text(if selected { "selected" } else { "" }).muted(),
            ])
            .gap(tokens::SPACE_2)
            .align(Align::Center)
            .height(Size::Fixed(40.0))
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
            .key(format!("lt-list-row-{i}"))
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_SM);
            if selected {
                r = r.selected();
            }
            r
        })
        .collect();
    column(rows).gap(tokens::SPACE_2)
}

fn items_demo(state: &State, phone: bool) -> El {
    let recent = titled_card(
        "Recent repositories",
        [item_group([
            selectable_item(
                state,
                "items:row:repo:damascene",
                IconName::Folder,
                "damascene",
                "/home/christian/workspace/damascene/damascene.main",
                badge("current").info(),
            ),
            selectable_item(
                state,
                "items:row:repo:whisper",
                IconName::Folder,
                "whisper-git",
                "/home/christian/workspace/whisper-git/damascene-ui",
                icon(IconName::ChevronRight).muted(),
            ),
            selectable_item(
                state,
                "items:row:repo:dotfiles",
                IconName::Folder,
                "dotfiles",
                "/home/christian/workspace/dotfiles",
                badge("3").warning(),
            ),
        ])],
    );

    let team = titled_card(
        "Team",
        [item_group([
            current_if(
                item([
                    item_media([avatar_fallback("Alicia Koch")]),
                    item_content([
                        item_title("Alicia Koch"),
                        item_description("Reviewed the tabs interaction pass"),
                    ]),
                    item_actions([button("Assign").ghost().key("items:action:assign-alicia")]),
                ])
                .key("items:row:person:alicia"),
                state.item_selected == "items:row:person:alicia",
            ),
            item_separator(),
            current_if(
                item([
                    item_media([avatar_fallback("Max Leiter")]),
                    item_content([
                        item_title("Max Leiter"),
                        item_description("Waiting on updated screenshots"),
                    ]),
                    item_actions([badge("pending").muted()]),
                ])
                .key("items:row:person:max"),
                state.item_selected == "items:row:person:max",
            ),
        ])],
    );

    if phone {
        column([recent, team])
            .gap(tokens::SPACE_4)
            .width(Size::Fill(1.0))
    } else {
        row([recent, team]).gap(tokens::SPACE_4).align(Align::Start)
    }
}

fn selectable_item(
    state: &State,
    key: &'static str,
    source: IconName,
    title: &'static str,
    description: &'static str,
    action: El,
) -> El {
    current_if(
        item([
            item_media_icon(source),
            item_content([item_title(title), item_description(description)]),
            item_actions([action]),
        ])
        .key(key),
        state.item_selected == key,
    )
}

fn current_if(row: El, current: bool) -> El {
    if current { row.current() } else { row }
}

fn table_demo() -> El {
    let head = table_header([table_row([
        table_head("Name"),
        table_head("Status"),
        table_head("Owner"),
        table_head("Updated"),
    ])]);
    let rows = [
        ("damascene-core", "Building", "alicia", "2m ago"),
        ("damascene-wgpu", "Passing", "max", "12m ago"),
        ("damascene-fixtures", "Passing", "alicia", "1h ago"),
        ("damascene-tools", "Failing", "alicia", "3h ago"),
    ];
    let body = table_body(rows.iter().map(|(name, status, owner, updated)| {
        let status_badge = match *status {
            "Passing" => badge(*status).success(),
            "Failing" => badge(*status).destructive(),
            _ => badge(*status).warning(),
        };
        table_row([
            table_cell(text(*name).label()),
            table_cell(status_badge),
            table_cell(text(*owner).muted()),
            table_cell(text(*updated).muted().small()),
        ])
    }));
    table([head, body])
}
