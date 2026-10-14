//! A full-window Preferences page on the workbench theme.
//!
//! The VS Code settings-tab genre: a slim header strip with a search
//! field, a category rail on `sideBar.background`, and a capped-width
//! content column of setting groups over `editor.background`. Every
//! control is **stock** damascene — `select_trigger`, `numeric_input`,
//! `checkbox`, `switch`, `button`, the `form_item` anatomy — rendered
//! through [`damascene_workbench::theme::theme`]; the only workbench
//! pieces are [`pane_header`] and [`hairline`].
//!
//! What the theme is doing here, in one screen:
//!
//! - The rail steps above the content (`sideBar.background` over
//!   `editor.background`) and the two meet on a 1px `sideBar.border` —
//!   no gap, no card, no shadow.
//! - Controls sit on the 28px `Xs` rung with ~2px corners, so a settings
//!   page with a dozen rows fits without scrolling.
//! - Setting descriptions run in `descriptionForeground`, one value step
//!   under the label — the density trick that lets a row carry two lines
//!   without reading as two rows.
//!
//! Run: `cargo run -p damascene-workbench --example settings`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// The categories in the left rail.
const CATEGORIES: &[&str] = &[
    "General",
    "Appearance",
    "Editor",
    "Keyboard",
    "Network",
    "Advanced",
];

const THEME_KEY: &str = "theme";
const FONT_KEY: &str = "ui-font";
const SIZE_KEY: &str = "font-size";
const SEARCH_KEY: &str = "search";

const THEMES: &[&str] = &[
    "Dark Modern",
    "Dark+ (default dark)",
    "Light Modern",
    "Quiet Light",
    "Monokai",
    "Solarized Dark",
];

const UI_FONTS: &[&str] = &[
    "Inter",
    "SF Pro Text",
    "Segoe UI Variable",
    "IBM Plex Sans",
    "System Default",
];

/// Accent swatches. Five come straight out of the Dark Modern palette;
/// the violet is the one hue the workbench vocabulary has no key for.
const ACCENTS: &[Color] = &[
    vs::FOCUS_BORDER,
    vs::EDITOR_GUTTER_ADDED_BG,
    vs::CHAT_EDITED_FILE_FG,
    vs::ERROR_FG,
    vs::TEXT_LINK_FG,
    Color::srgb_u8(180, 142, 255),
];

/// Rail width, and the readable cap on the content column. Settings
/// text is prose; letting it run the full 1280 would make it a page of
/// unscannable lines.
const RAIL_WIDTH: f32 = 220.0;
const CONTENT_WIDTH: f32 = 720.0;
/// Selects are wider than a value needs and narrower than the column —
/// the `<select>` proportion the genre expects.
const SELECT_WIDTH: f32 = 320.0;
const HEADER_HEIGHT: f32 = 40.0;
const FOOTER_HEIGHT: f32 = 48.0;

struct Preferences {
    category: String,
    search: String,
    selection: Selection,

    theme_name: String,
    theme_open: bool,
    ui_font: String,
    ui_font_open: bool,
    font_size: String,
    accent: usize,

    smooth_scrolling: bool,
    reduce_motion: bool,
    blink_cursor: bool,
    show_status_bar: bool,
    compact_mode: bool,
    auto_hide_activity_bar: bool,
}

impl Preferences {
    fn new() -> Self {
        Self {
            category: "Appearance".into(),
            search: String::new(),
            selection: Selection::default(),

            theme_name: "Dark Modern".into(),
            theme_open: false,
            ui_font: "Inter".into(),
            ui_font_open: false,
            font_size: "13".into(),
            accent: 0,

            smooth_scrolling: true,
            reduce_motion: false,
            blink_cursor: true,
            show_status_bar: true,
            compact_mode: false,
            auto_hide_activity_bar: false,
        }
    }

    fn size_opts() -> NumericInputOpts<'static> {
        NumericInputOpts::default()
            .min(8.0)
            .max(32.0)
            .step(1.0)
            .stacked()
    }

    /// Title on the left, settings search on the right. Same `border_b`
    /// under-rule as the shell's title strip — the header owns its own
    /// separator rather than a sibling rule owning it.
    fn header(&self) -> El {
        row([
            text("Preferences")
                .title()
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            spacer(),
            text_input_with(
                SEARCH_KEY,
                &self.search,
                &self.selection,
                TextInputOpts::default().placeholder("Search settings"),
            )
            .width(Size::Fixed(260.0)),
        ])
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_4))
        .gap(tokens::SPACE_4)
        .height(Size::Fixed(HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The category rail — a stock `sidebar_menu` of
    /// `sidebar_menu_button`s under a workbench `pane_header`.
    fn rail(&self) -> El {
        let rows: Vec<El> = CATEGORIES
            .iter()
            .map(|c| sidebar_menu_button(*c, self.category == *c).key(format!("cat:{c}")))
            .collect();

        column([
            pane_header("CATEGORIES", Vec::<El>::new()),
            column([sidebar_menu(rows)])
                .padding(tokens::SPACE_2)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .scrollable(),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_r()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(RAIL_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// Scrolling body over a pinned action bar. The hairline belongs to
    /// neither — it is laid *between* the two, which is the case
    /// per-side borders can't express and [`hairline`] exists for.
    fn content(&self) -> El {
        column([
            // Align::Stretch (the column default) is load-bearing: it is
            // what lets the `Fill` groups column claim the scroll
            // extent, so `max_width` has something to clamp. Under
            // Align::Start a Fill child collapses to its intrinsic
            // width instead.
            column([self.groups()])
                .padding(Sides::xy(tokens::SPACE_6, tokens::SPACE_5))
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .scrollable(),
            hairline(),
            self.actions(),
        ])
        .fill(vs::EDITOR_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn groups(&self) -> El {
        column([
            group(
                "Appearance",
                "How the workbench looks. Changes apply immediately.",
                [
                    form_item([
                        form_label("Theme"),
                        form_control(
                            select_trigger(THEME_KEY, &self.theme_name)
                                .width(Size::Fixed(SELECT_WIDTH)),
                        ),
                        form_description("Color theme applied to the editor and the workbench chrome."),
                    ]),
                    form_item([
                        form_label("UI font family"),
                        form_control(
                            select_trigger(FONT_KEY, &self.ui_font)
                                .width(Size::Fixed(SELECT_WIDTH)),
                        ),
                        form_description("Used for menus, panels, and dialogs. The editor keeps its own monospace face."),
                    ]),
                    form_item([
                        form_label("Font size"),
                        form_control(numeric_input(
                            SIZE_KEY,
                            &self.font_size,
                            &self.selection,
                            Self::size_opts(),
                        )),
                        form_description("Base UI size in points. Between 8 and 32."),
                    ]),
                    self.accent_row(),
                ],
            ),
            group(
                "Motion",
                "Animation and cursor behavior.",
                [
                    check_row(
                        "smooth-scrolling",
                        "Smooth scrolling",
                        "Animate the viewport when scrolling with a wheel or a keyboard page key.",
                        self.smooth_scrolling,
                    ),
                    check_row(
                        "reduce-motion",
                        "Reduce motion",
                        "Replace transitions with instant state changes. Follows the system setting when unset.",
                        self.reduce_motion,
                    ),
                    check_row(
                        "blink-cursor",
                        "Blink the text cursor",
                        "Turn off to keep the caret solid while a field is focused.",
                        self.blink_cursor,
                    ),
                ],
            ),
            group(
                "Layout",
                "Which chrome the window shows, and how tightly it packs.",
                [
                    check_row(
                        "show-status-bar",
                        "Show status bar",
                        "The 22px strip along the bottom of the window carrying branch, problems, and cursor position.",
                        self.show_status_bar,
                    ),
                    switch_row(
                        "compact-mode",
                        "Compact mode",
                        "Tighten row heights and padding across every panel.",
                        self.compact_mode,
                    ),
                    switch_row(
                        "auto-hide-activity-bar",
                        "Auto-hide activity bar",
                        "Collapse the icon rail until the pointer reaches the window edge.",
                        self.auto_hide_activity_bar,
                    ),
                ],
            ),
        ])
        .gap(tokens::SPACE_8)
        .width(Size::Fill(1.0))
        .max_width(CONTENT_WIDTH)
    }

    fn accent_row(&self) -> El {
        let swatches: Vec<El> = ACCENTS
            .iter()
            .enumerate()
            .map(|(i, c)| self.swatch(i, *c))
            .collect();

        form_item([
            form_label("Accent color"),
            form_control(
                row(swatches)
                    .gap(tokens::SPACE_2)
                    .align(Align::Center)
                    .width(Size::Hug),
            ),
            form_description("Tints the focus ring, the active tab indicator, and progress bars."),
        ])
    }

    /// A swatch is a keyed, focusable filled box — there is no stock
    /// color-well widget, and at this size a `button` would carry the
    /// wrong anatomy (label slot, control height, style profile).
    fn swatch(&self, index: usize, color: Color) -> El {
        let selected = self.accent == index;
        column(Vec::<El>::new())
            .key(format!("accent:{index}"))
            .focusable()
            .cursor(Cursor::Pointer)
            .fill(color)
            .radius(vs::RADIUS)
            .stroke(if selected {
                vs::FOREGROUND
            } else {
                vs::WIDGET_BORDER
            })
            .stroke_width(if selected { 2.0 } else { 1.0 })
            .width(Size::Fixed(22.0))
            .height(Size::Fixed(22.0))
    }

    /// Destructive-adjacent action on the left, commit pair on the
    /// right. `Restore defaults` is a ghost so it reads as available
    /// rather than offered.
    fn actions(&self) -> El {
        row([
            button("Restore defaults").key("restore").ghost(),
            spacer(),
            button("Cancel").key("cancel").ghost(),
            button("Save").key("save").primary(),
        ])
        .fill(vs::EDITOR_BG)
        .padding(Sides::xy(tokens::SPACE_4, 0.0))
        .gap(tokens::SPACE_2)
        .height(Size::Fixed(FOOTER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }
}

/// One settings group: heading, one-line summary, a rule, then rows.
fn group<I, E>(title: &str, summary: &str, items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        column([
            // `.label()` + Semibold, not `.heading()`: the role ladder
            // jumps TEXT_BASE → TEXT_2XL, and a 22px group heading in a
            // dense settings page reads as a marketing section.
            text(title).label().font_weight(FontWeight::Semibold),
            text(summary).caption().text_color(vs::DESCRIPTION_FG),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0)),
        hairline(),
        column(items)
            .gap(tokens::SPACE_5)
            .width(Size::Fill(1.0))
            .pt(tokens::SPACE_1),
    ])
    .gap(tokens::SPACE_3)
    .width(Size::Fill(1.0))
}

/// Checkbox on the left of a two-line label/description stack — the
/// settings-row shape `field_row` doesn't cover (its label is a single
/// leaf and its control sits on the right).
fn check_row(key: &str, label: &str, description: &str, value: bool) -> El {
    row([
        checkbox(key, value),
        column([
            text(label).label(),
            text(description)
                .caption()
                .text_color(vs::DESCRIPTION_FG)
                .wrap_text(),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Start)
    .width(Size::Fill(1.0))
}

/// The mirror shape: label stack on the left, switch pinned right.
fn switch_row(key: &str, label: &str, description: &str, value: bool) -> El {
    row([
        column([
            text(label).label(),
            text(description)
                .caption()
                .text_color(vs::DESCRIPTION_FG)
                .wrap_text(),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0)),
        switch(key, value),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Center)
    .width(Size::Fill(1.0))
}

fn options(values: &[&str]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|v| ((*v).to_string(), (*v).to_string()))
        .collect()
}

impl App for Preferences {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root rather than `page()`: a preferences window
        // is full-bleed chrome, and window padding would float the whole
        // page on a margin. `overlays` supplies the layer root the open
        // select menus mount into.
        let shell = column([
            self.header(),
            row([self.rail(), self.content()])
                .height(Size::Fill(1.0))
                .width(Size::Fill(1.0))
                .align(Align::Stretch),
        ])
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG);

        overlays(
            shell,
            [
                self.theme_open.then(|| {
                    select::select_menu_selected(THEME_KEY, options(THEMES), &self.theme_name)
                }),
                self.ui_font_open.then(|| {
                    select::select_menu_selected(FONT_KEY, options(UI_FONTS), &self.ui_font)
                }),
            ],
        )
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if select::apply_event(
            &mut self.theme_name,
            &mut self.theme_open,
            &event,
            THEME_KEY,
            Some,
        ) {
            return;
        }
        if select::apply_event(
            &mut self.ui_font,
            &mut self.ui_font_open,
            &event,
            FONT_KEY,
            Some,
        ) {
            return;
        }
        if numeric_input::apply_event(
            &mut self.font_size,
            &mut self.selection,
            SIZE_KEY,
            &Self::size_opts(),
            &event,
        ) {
            return;
        }
        if text_input::apply_event(&mut self.search, &mut self.selection, &event, SEARCH_KEY) {
            return;
        }

        if checkbox::apply_event(&mut self.smooth_scrolling, &event, "smooth-scrolling")
            || checkbox::apply_event(&mut self.reduce_motion, &event, "reduce-motion")
            || checkbox::apply_event(&mut self.blink_cursor, &event, "blink-cursor")
            || checkbox::apply_event(&mut self.show_status_bar, &event, "show-status-bar")
            || switch::apply_event(&mut self.compact_mode, &event, "compact-mode")
            || switch::apply_event(
                &mut self.auto_hide_activity_bar,
                &event,
                "auto-hide-activity-bar",
            )
        {
            return;
        }

        for (i, _) in ACCENTS.iter().enumerate() {
            if event.is_click_or_activate(&format!("accent:{i}")) {
                self.accent = i;
                return;
            }
        }
        for c in CATEGORIES {
            if event.is_click_or_activate(&format!("cat:{c}")) {
                self.category = (*c).to_string();
                return;
            }
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    // Without this, stock controls render in shadcn zinc at 36px.
    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Preferences", viewport, Preferences::new())
}
