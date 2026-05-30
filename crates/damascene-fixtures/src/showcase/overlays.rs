//! Overlays — dialog, sheet, popover, dropdown_menu, context menu.
//!
//! Floating layers all compose `overlay([scrim, content])` underneath
//! and route a `{key}:dismiss` event when the user clicks outside the
//! panel. The page mounts each layer only when its open flag is set —
//! Showcase contributes them as siblings of the main view via
//! `overlays(main, [layer_a, layer_b, …])`.

use damascene_core::prelude::*;

use super::{Section, Showcase};

const DIALOG_KEY: &str = "ov-dialog";
const SHEET_KEY: &str = "ov-sheet";
const POPOVER_KEY: &str = "ov-popover";
const POPOVER_TRIGGER_KEY: &str = "ov-popover-trigger";
const DROPDOWN_KEY: &str = "ov-dropdown";
const DROPDOWN_TRIGGER_KEY: &str = "ov-dropdown-trigger";
const CONTEXT_MENU_KEY: &str = "ov-context";
const CONTEXT_TARGET_KEY: &str = "ov-context-target";

#[derive(Default)]
pub struct State {
    pub dialog_open: bool,
    pub sheet_open: bool,
    pub popover_open: bool,
    pub dropdown_open: bool,
    pub dropdown_menu_density: MenuDensity,
    pub context_menu_at: Option<(f32, f32)>,
    pub context_menu_density: MenuDensity,
}

pub fn view(state: &State) -> El {
    column([
        h1("Overlays"),
        paragraph(
            "Five floating-layer flavours all sharing the same anatomy: \
             `overlay([scrim, content])`, `{key}:dismiss` on outside \
             click, and the app mounts the layer into a root stack only \
             while open.",
        )
        .muted(),
        section_label("Dialog & sheet"),
        row([
            button("Open dialog").primary().key("ov-open-dialog"),
            button("Open sheet (left)").secondary().key("ov-open-sheet"),
        ])
        .gap(tokens::SPACE_2),
        text(format!(
            "dialog: {}, sheet: {}",
            yn(state.dialog_open),
            yn(state.sheet_open)
        ))
        .small()
        .muted(),
        section_label("Popover & dropdown menu"),
        row([
            button("Show popover").secondary().key(POPOVER_TRIGGER_KEY),
            button("Open dropdown ▾")
                .secondary()
                .key(DROPDOWN_TRIGGER_KEY),
        ])
        .gap(tokens::SPACE_2),
        section_label("Context menu"),
        column([
            text("Right-click or long-press anywhere in this panel.")
                .muted()
                .wrap_text()
                .fill_width(),
            text(match state.context_menu_at {
                Some((x, y)) => format!("last open at ({x:.0}, {y:.0})"),
                None => "no open yet".into(),
            })
            .small()
            .muted()
            .wrap_text()
            .fill_width(),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_5)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .key(CONTEXT_TARGET_KEY)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(120.0)),
    ])
    .gap(tokens::SPACE_4)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if matches!(e.kind, UiEventKind::SecondaryClick | UiEventKind::LongPress)
        && e.route() == Some(CONTEXT_TARGET_KEY)
        && let (Some(x), Some(y)) = (e.pointer_x(), e.pointer_y())
    {
        state.context_menu_at = Some((x, y));
        state.context_menu_density = if matches!(e.kind, UiEventKind::LongPress) {
            MenuDensity::Touch
        } else {
            MenuDensity::Compact
        };
        return;
    }
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    let dismiss_dialog = format!("{DIALOG_KEY}:dismiss");
    let dismiss_sheet = format!("{SHEET_KEY}:dismiss");
    let dismiss_popover = format!("{POPOVER_KEY}:dismiss");
    let dismiss_dropdown = format!("{DROPDOWN_KEY}:dismiss");
    let dismiss_context = format!("{CONTEXT_MENU_KEY}:dismiss");
    match e.route() {
        Some("ov-open-dialog") => state.dialog_open = true,
        Some("ov-open-sheet") => state.sheet_open = true,
        Some(POPOVER_TRIGGER_KEY) => state.popover_open = !state.popover_open,
        Some(DROPDOWN_TRIGGER_KEY) => {
            state.dropdown_open = !state.dropdown_open;
            state.dropdown_menu_density = if state.dropdown_open {
                MenuDensity::from_event(&e)
            } else {
                MenuDensity::Compact
            };
        }
        Some(k) if k == dismiss_dialog || k == "ov-dialog-save" => state.dialog_open = false,
        Some(k) if k == dismiss_sheet || k == "ov-sheet-reset" || k == "ov-sheet-apply" => {
            state.sheet_open = false
        }
        Some(k) if k == dismiss_popover => state.popover_open = false,
        Some(k) if k == dismiss_dropdown || k.starts_with("ov-dropdown-action:") => {
            state.dropdown_open = false;
            state.dropdown_menu_density = MenuDensity::Compact
        }
        Some(k) if k == dismiss_context || k.starts_with("ov-context-action:") => {
            state.context_menu_at = None;
            state.context_menu_density = MenuDensity::Compact
        }
        _ => {}
    }
}

pub fn dialog_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.dialog_open).then(dialog_content_factory)
}

pub fn sheet_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.sheet_open).then(sheet_content_factory)
}

pub fn popover_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.popover_open).then(|| {
        popover(
            POPOVER_KEY,
            Anchor::below_key(POPOVER_TRIGGER_KEY),
            popover_panel([column([
                text("Information").bold(),
                paragraph(
                    "Popovers are non-modal — outside clicks dismiss; \
                         interactive content survives.",
                )
                .muted(),
            ])
            .padding(tokens::SPACE_3)
            .gap(tokens::SPACE_2)]),
        )
    })
}

pub fn dropdown_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.dropdown_open).then(|| {
        dropdown_menu_with_density(
            DROPDOWN_KEY,
            DROPDOWN_TRIGGER_KEY,
            app.overlays.dropdown_menu_density,
            [
                dropdown_menu_label("Workspace"),
                dropdown_menu_item_with_icon_and_shortcut(IconName::Plus, "New project", "⌘N")
                    .key("ov-dropdown-action:new"),
                dropdown_menu_item_with_icon_and_shortcut(IconName::FileText, "Duplicate", "⌘D")
                    .key("ov-dropdown-action:duplicate"),
                dropdown_menu_item_with_icon_and_shortcut(IconName::Settings, "Settings", "⌘,")
                    .key("ov-dropdown-action:settings"),
                dropdown_menu_separator(),
                dropdown_menu_item_with_icon_and_shortcut(IconName::Search, "Find", "⌘F")
                    .key("ov-dropdown-action:find"),
                dropdown_menu_item_with_icon(IconName::X, "Sign out")
                    .key("ov-dropdown-action:signout"),
            ],
        )
    })
}

pub fn context_menu_layer(app: &Showcase) -> Option<El> {
    if app.section != Section::Overlays {
        return None;
    }
    app.overlays.context_menu_at.map(|(x, y)| {
        context_menu_with_density(
            CONTEXT_MENU_KEY,
            (x, y),
            app.overlays.context_menu_density,
            [
                menu_item("Copy").key("ov-context-action:copy"),
                menu_item("Cut").key("ov-context-action:cut"),
                menu_item("Paste").key("ov-context-action:paste"),
                menu_item("Duplicate").key("ov-context-action:duplicate"),
                menu_item("Delete").key("ov-context-action:delete"),
            ],
        )
    })
}

fn dialog_content_factory() -> El {
    dialog(
        DIALOG_KEY,
        [
            dialog_header([
                dialog_title("Confirm changes"),
                dialog_description(
                    "Settings have been edited. Save now to keep your changes, \
                     or close this dialog to keep editing.",
                ),
            ]),
            dialog_footer([
                button("Cancel")
                    .key(format!("{DIALOG_KEY}:dismiss"))
                    .ghost(),
                button("Save").key("ov-dialog-save").primary(),
            ]),
        ],
    )
}

fn sheet_content_factory() -> El {
    sheet(
        SHEET_KEY,
        SheetSide::Left,
        [
            sheet_header([
                sheet_title("Filter"),
                sheet_description(
                    "Narrow the list with a few filters. Edge-attached so \
                     it stays out of the main content's way.",
                ),
            ]),
            paragraph(
                "Sheets are useful for navigation drawers, secondary detail \
                 inspectors, and filter panes — anywhere a full modal would \
                 feel disruptive but a transient surface still wants its own \
                 dedicated space.",
            ),
            sheet_footer([
                button("Reset").key("ov-sheet-reset").ghost(),
                button("Apply").key("ov-sheet-apply").primary(),
            ]),
        ],
    )
}

fn yn(b: bool) -> &'static str {
    if b { "open" } else { "closed" }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_open_event(kind: UiEventKind) -> UiEvent {
        let mut event = UiEvent::synthetic_click(CONTEXT_TARGET_KEY);
        event.kind = kind;
        event.pointer = Some((42.0, 64.0));
        event
    }

    fn dropdown_trigger_event(pointer_kind: Option<PointerKind>) -> UiEvent {
        let mut event = UiEvent::synthetic_click(DROPDOWN_TRIGGER_KEY);
        event.pointer_kind = pointer_kind;
        event
    }

    fn collect_menu_item_heights(el: &El, heights: &mut Vec<Size>) {
        if matches!(
            el.kind,
            Kind::Custom("menu_item") | Kind::Custom("dropdown_menu_item")
        ) {
            heights.push(el.height);
        }
        for child in &el.children {
            collect_menu_item_heights(child, heights);
        }
    }

    #[test]
    fn context_menu_opens_from_secondary_click() {
        let mut state = State::default();

        on_event(&mut state, context_open_event(UiEventKind::SecondaryClick));

        assert_eq!(state.context_menu_at, Some((42.0, 64.0)));
        assert_eq!(state.context_menu_density, MenuDensity::Compact);
    }

    #[test]
    fn context_menu_opens_from_long_press() {
        let mut state = State::default();

        on_event(&mut state, context_open_event(UiEventKind::LongPress));

        assert_eq!(state.context_menu_at, Some((42.0, 64.0)));
        assert_eq!(state.context_menu_density, MenuDensity::Touch);
    }

    #[test]
    fn dropdown_menu_uses_touch_density_for_touch_trigger() {
        let mut app = Showcase::with_section(Section::Overlays);

        on_event(
            &mut app.overlays,
            dropdown_trigger_event(Some(PointerKind::Touch)),
        );
        let layer = dropdown_layer(&app).unwrap();
        let mut heights = Vec::new();

        collect_menu_item_heights(&layer, &mut heights);

        assert_eq!(app.overlays.dropdown_menu_density, MenuDensity::Touch);
        assert_eq!(heights, vec![Size::Fixed(TOUCH_MENU_ITEM_HEIGHT); 5]);
    }

    #[test]
    fn dropdown_menu_uses_stock_density_for_mouse_trigger() {
        let mut app = Showcase::with_section(Section::Overlays);

        on_event(
            &mut app.overlays,
            dropdown_trigger_event(Some(PointerKind::Mouse)),
        );
        let layer = dropdown_layer(&app).unwrap();
        let mut heights = Vec::new();

        collect_menu_item_heights(&layer, &mut heights);

        assert_eq!(app.overlays.dropdown_menu_density, MenuDensity::Compact);
        assert_eq!(heights, vec![Size::Fixed(30.0); 5]);
    }

    #[test]
    fn context_menu_uses_stock_density_for_secondary_click() {
        let app = Showcase::with_overlay_context_menu_at(42.0, 64.0);
        let layer = context_menu_layer(&app).unwrap();
        let mut heights = Vec::new();

        collect_menu_item_heights(&layer, &mut heights);

        assert_eq!(heights, vec![Size::Fixed(28.0); 5]);
    }

    #[test]
    fn context_menu_uses_touch_density_for_long_press() {
        let mut app = Showcase::with_overlay_context_menu_at(42.0, 64.0);
        app.overlays.context_menu_density = MenuDensity::Touch;
        let layer = context_menu_layer(&app).unwrap();
        let mut heights = Vec::new();

        collect_menu_item_heights(&layer, &mut heights);

        assert_eq!(heights, vec![Size::Fixed(TOUCH_MENU_ITEM_HEIGHT); 5]);
    }
}
