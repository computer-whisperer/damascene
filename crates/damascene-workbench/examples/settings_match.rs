//! A full-window Preferences page reproduced from a target design
//! (`references/workbench-validation/settings/reference.png`).
//!
//! This is the *validation* twin of `examples/settings.rs`: the same
//! genre, but built to match a specific screenshot pixel-for-pixel as
//! far as the stock vocabulary reaches. Everything interactive is a
//! stock damascene widget — `select_trigger`, `numeric_input`,
//! `checkbox`, `switch`, `button`, `text_input_with`,
//! `sidebar_menu_button_with_icon` — sized and coloured through
//! [`damascene_workbench::theme`].
//!
//! # What is re-pointed, and why
//!
//! The reference is not Dark Modern: it is a slate-blue ramp
//! (`#1E2023` content, `#1A1C1F` rail, `#24272B` panels, `#4D8FF5`
//! accent). Rather than hard-code those values at the call sites, they
//! are registered *back onto the workbench key namespace*
//! (`sideBar.background`, `panel.border`, `focusBorder`, …) in
//! [`palette`], so the tree below still reads in workbench vocabulary
//! and a swap back to Dark Modern is one function call. That is exactly
//! the split `theme::register_workbench_tokens` documents.
//!
//! Two theme knobs move with it:
//!
//! - `with_radius_scale(4/7)` — the reference's controls are ~4px
//!   cornered, not the workbench's ~2px.
//! - the settings panels round at 6px, set explicitly.
//!
//! # Known gaps against the reference
//!
//! Recorded rather than hand-replicated with raw primitives:
//!
//! - **Search field affordances.** `TextInputOpts` has no leading-icon
//!   or trailing-adornment slot, so the reference's magnifier and `⌘F`
//!   hint are absent.
//! - **Numeric unit suffix.** `NumericInputOpts` has no `.suffix("px")`,
//!   so the unit rides outside the field.
//! - **Icon vocabulary — CLOSED.** The rail once ran on the nearest
//!   stand-ins because none of the 26 built-ins was a code bracket,
//!   keyboard, globe, terminal or contrast disc. All five are in the
//!   57-name vocabulary now, and the rail draws them.
//! - **Text ramp.** The palette carries two text tones (`foreground`,
//!   `descriptionForeground`); the reference uses four.
//!
//! Run: `cargo run -p damascene-workbench --example settings_match`

use damascene_core::prelude::*;
use damascene_workbench::{theme, tokens as vs};

// ---------------------------------------------------------------------
// Reference palette — sampled from the target screenshot.
// ---------------------------------------------------------------------

/// Content surface behind the setting groups.
const REF_CONTENT: Color = Color::srgb_u8(30, 32, 35);
/// Category rail; sinks one step below the content.
const REF_RAIL: Color = Color::srgb_u8(26, 28, 31);
/// Header strip and footer action bar.
const REF_BAR: Color = Color::srgb_u8(33, 35, 39);
/// The grouped-settings panel; rises one step above the content.
const REF_PANEL: Color = Color::srgb_u8(36, 39, 43);
/// Structural rules: header under-rule, rail edge, panel outline.
const REF_RULE: Color = Color::srgb_u8(48, 52, 57);
/// The dimmer 1px rule *inside* a panel, between rows.
const REF_DIVIDER: Color = Color::srgb_u8(41, 44, 49);
/// Control wells — inputs, selects, unchecked boxes.
const REF_WELL: Color = Color::srgb_u8(27, 29, 32);
/// Control outlines.
const REF_CONTROL_BORDER: Color = Color::srgb_u8(58, 63, 69);
/// The accent: primary button, focus ring, active-category bar.
const REF_ACCENT: Color = Color::srgb_u8(77, 143, 245);
/// Selected row wash in the rail.
const REF_SELECTED: Color = Color::srgb_u8(43, 46, 51);

/// Primary text.
const REF_FG: Color = Color::srgb_u8(228, 232, 236);
/// Second text tone — inactive rail rows, secondary button labels.
/// The stock palette has no slot for this; see the module gap list.
const REF_FG_2: Color = Color::srgb_u8(152, 160, 170);
/// Third text tone — row descriptions, group captions.
const REF_FG_3: Color = Color::srgb_u8(110, 118, 127);
/// Fourth text tone — the faintest annotations.
const REF_FG_4: Color = Color::srgb_u8(86, 93, 101);

/// The eight accent swatches, left to right.
const ACCENTS: &[Color] = &[
    REF_ACCENT,
    Color::srgb_u8(124, 108, 240),
    Color::srgb_u8(49, 177, 164),
    Color::srgb_u8(87, 169, 90),
    Color::srgb_u8(217, 155, 60),
    Color::srgb_u8(217, 97, 79),
    Color::srgb_u8(201, 97, 147),
    Color::srgb_u8(120, 130, 141),
];

// ---------------------------------------------------------------------
// Metrics — measured off the reference.
// ---------------------------------------------------------------------

const RAIL_WIDTH: f32 = 220.0;
const CONTENT_WIDTH: f32 = 760.0;
/// Fixed control column on the right of every setting row.
const CONTROL_WIDTH: f32 = 236.0;
/// Leading gutter that carries the "modified" dot.
const MARKER_WIDTH: f32 = 20.0;
const ROW_HEIGHT: f32 = 48.0;
const HEADER_HEIGHT: f32 = 43.0;
const FOOTER_HEIGHT: f32 = 56.0;
const RAIL_ITEM_HEIGHT: f32 = 30.0;
const PANEL_RADIUS: f32 = 6.0;
const SWATCH: f32 = 18.0;

/// Categories, paired with their built-in icon — all six glyphs the
/// reference uses are in the vocabulary by name.
const CATEGORIES: &[(&str, &str)] = &[
    ("General", "settings"),
    ("Appearance", "contrast"),
    ("Editor", "code"),
    ("Keyboard", "keyboard"),
    ("Network", "globe"),
    ("Advanced", "terminal"),
];

const THEME_KEY: &str = "color-theme";
const FONT_KEY: &str = "ui-font";
const SIZE_KEY: &str = "font-size";
const SEARCH_KEY: &str = "search";

const THEMES: &[&str] = &[
    "Nightfall Dark",
    "Nightfall Light",
    "Solarized Dark",
    "High Contrast",
];
const UI_FONTS: &[&str] = &["Inter", "SF Pro Text", "Segoe UI Variable", "System Default"];

struct Preferences {
    category: String,
    search: String,
    selection: Selection,

    color_theme: String,
    theme_open: bool,
    ui_font: String,
    ui_font_open: bool,
    font_size: String,
    accent: usize,

    match_system: bool,
    smooth_scrolling: bool,
    reduce_motion: bool,
    show_status_bar: bool,
    compact_mode: bool,
    translucent_sidebar: bool,
}

impl Preferences {
    fn new() -> Self {
        Self {
            category: "Appearance".into(),
            search: String::new(),
            selection: Selection::default(),

            color_theme: "Nightfall Dark".into(),
            theme_open: false,
            ui_font: "Inter".into(),
            ui_font_open: false,
            font_size: "13".into(),
            accent: 0,

            match_system: false,
            smooth_scrolling: true,
            reduce_motion: false,
            show_status_bar: true,
            compact_mode: true,
            translucent_sidebar: false,
        }
    }

    fn size_opts() -> NumericInputOpts<'static> {
        NumericInputOpts::default()
            .min(9.0)
            .max(24.0)
            .step(1.0)
            .stacked()
    }

    /// Title left, settings search right, on the bar level with an
    /// under-rule — the `border_b` case per-side borders exist for.
    fn header(&self) -> El {
        row([
            text("Preferences")
                .label()
                .semibold()
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            spacer(),
            text_input_with(
                SEARCH_KEY,
                &self.search,
                &self.selection,
                TextInputOpts::default().placeholder("Search settings"),
            )
            .width(Size::Fixed(268.0)),
        ])
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_4))
        .height(Size::Fixed(HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The category rail: a stock `sidebar_menu` of icon rows, with the
    /// active row's accent bar riding the rail's own left edge (which is
    /// why the menu column carries no left padding).
    fn rail(&self) -> El {
        let rows: Vec<El> = CATEGORIES
            .iter()
            .map(|(label, glyph)| self.rail_row(label, glyph))
            .collect();

        column([
            text("SETTINGS")
                .caption()
                .letter_spacing(0.5)
                .text_color(REF_FG_4)
                .pl(tokens::SPACE_4)
                .pt(tokens::SPACE_4)
                .pb(tokens::SPACE_2),
            column([sidebar_menu(rows).gap(0.0)])
                .padding(Sides {
                    left: 0.0,
                    right: tokens::SPACE_2,
                    top: 0.0,
                    bottom: tokens::SPACE_2,
                })
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .scrollable(),
            row([text("v2.4.1 · stable")
                .caption()
                .mono()
                .text_color(REF_FG_4)])
            .border_t()
            .border_color(vs::SIDE_BAR_SECTION_HEADER_BORDER)
            .padding(Sides::x(tokens::SPACE_5))
            .height(Size::Fixed(37.0))
            .width(Size::Fill(1.0))
            .align(Align::Center),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_r()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(RAIL_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn rail_row(&self, label: &str, glyph: &str) -> El {
        let current = self.category == label;
        let button = sidebar_menu_button_with_icon(glyph, label, current)
            .key(format!("cat:{label}"))
            .height(Size::Fixed(RAIL_ITEM_HEIGHT))
            .radius(tokens::RADIUS_SM)
            .width(Size::Fill(1.0));
        let button = if current {
            button
        } else {
            button.text_color(REF_FG_2)
        };

        row([
            // The 3px current-row bar. `sidebar_menu_button` has no
            // slot for it and a `.border_l()` would sit at the row's own
            // edge, 8px in — see the gap list.
            column(Vec::<El>::new())
                .width(Size::Fixed(3.0))
                .height(Size::Fixed(RAIL_ITEM_HEIGHT))
                .fill(if current {
                    vs::FOCUS_BORDER
                } else {
                    Color::srgb_u8a(0, 0, 0, 0)
                }),
            button,
        ])
        .gap(5.0)
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// Scrolling body over a pinned action bar.
    fn content(&self) -> El {
        column([
            // `width(Fill).max_width(...)` clamps but does not centre —
            // spacers either side are what put the column in the middle
            // of the content region.
            column([row([spacer(), self.body(), spacer()])
                .width(Size::Fill(1.0))
                .align(Align::Start)])
            .padding(Sides::y(tokens::SPACE_5))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .scrollable(),
            self.actions(),
        ])
        .fill(vs::EDITOR_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn body(&self) -> El {
        column([
            column([
                // The role ladder jumps `title` (16→14.9 under the
                // workbench type scale) straight to `heading`
                // (24→22.3); the reference's page title sits between
                // them, so the size is explicit.
                text("Appearance")
                    .font_size(18.0)
                    .semibold()
                    .text_color(vs::FOREGROUND),
                text("Control the theme, typography, and interface density of the workspace.")
                    .label()
                    .text_color(REF_FG_2),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0)),
            column([
                group(
                    "THEME",
                    [
                        setting_row(
                            "Color theme",
                            "Applied to the editor, panels, and terminal.",
                            true,
                            select_trigger(THEME_KEY, &self.color_theme)
                                .width(Size::Fixed(CONTROL_WIDTH)),
                        ),
                        setting_row(
                            "Accent color",
                            "Used for selection, focus rings, and primary actions.",
                            false,
                            self.swatches(),
                        ),
                        setting_row(
                            "Match system appearance",
                            "Switch between the light and dark theme with the OS setting.",
                            false,
                            checkbox("match-system", self.match_system),
                        ),
                    ],
                ),
                group(
                    "TYPOGRAPHY",
                    [
                        setting_row(
                            "UI font family",
                            "Menus, sidebars, and dialogs. Falls back to the system UI font.",
                            false,
                            select_trigger(FONT_KEY, &self.ui_font)
                                .width(Size::Fixed(CONTROL_WIDTH)),
                        ),
                        setting_row(
                            "Font size",
                            "Base size for interface text. Accepts 9–24.",
                            true,
                            row([
                                // `NumericInputOpts` has no unit slot, so
                                // "px" cannot live inside the field.
                                numeric_input(
                                    SIZE_KEY,
                                    &self.font_size,
                                    &self.selection,
                                    Self::size_opts(),
                                )
                                .width(Size::Fixed(104.0)),
                                text("Default 12").caption().text_color(REF_FG_4),
                            ])
                            .gap(tokens::SPACE_3)
                            .align(Align::Center),
                        ),
                    ],
                ),
                group(
                    "INTERFACE",
                    [
                        setting_row(
                            "Smooth scrolling",
                            "Animate scroll position instead of jumping by line.",
                            false,
                            checkbox("smooth-scrolling", self.smooth_scrolling),
                        ),
                        setting_row(
                            "Reduce motion",
                            "Disable panel transitions, easing, and decorative animation.",
                            false,
                            checkbox("reduce-motion", self.reduce_motion),
                        ),
                        setting_row(
                            "Show status bar",
                            "Display branch, diagnostics, and cursor position along the bottom edge.",
                            false,
                            checkbox("show-status-bar", self.show_status_bar),
                        ),
                        setting_row(
                            "Compact mode",
                            "Tighten row heights and padding throughout the workspace.",
                            true,
                            toggle_control("compact-mode", self.compact_mode),
                        ),
                        setting_row(
                            "Translucent sidebar",
                            "Blur the desktop behind panel surfaces. Requires compositing.",
                            false,
                            toggle_control("translucent-sidebar", self.translucent_sidebar),
                        ),
                    ],
                ),
            ])
            .gap(17.0)
            .width(Size::Fill(1.0)),
        ])
        .gap(10.0)
        .width(Size::Fixed(CONTENT_WIDTH))
    }

    fn swatches(&self) -> El {
        let cells: Vec<El> = ACCENTS
            .iter()
            .enumerate()
            .map(|(i, c)| self.swatch(i, *c))
            .collect();
        row(cells).gap(3.0).align(Align::Center).width(Size::Hug)
    }

    /// A swatch is a keyed, focusable filled box — there is no stock
    /// colour-well widget, and at 18px a `button` carries the wrong
    /// anatomy. The selection ring is a padded wrapper because there is
    /// no ring-offset modifier outside the focus treatment.
    fn swatch(&self, index: usize, color: Color) -> El {
        let selected = self.accent == index;
        let cell = column([icon("check")
            .icon_size(12.0)
            .icon_stroke_width(2.5)
            .color(Color::srgb_u8(255, 255, 255))
            .opacity(if selected { 1.0 } else { 0.0 })])
        .key(format!("accent:{index}"))
        .focusable()
        .cursor(Cursor::Pointer)
        .fill(color)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(SWATCH))
        .height(Size::Fixed(SWATCH))
        .align(Align::Center)
        .justify(Justify::Center);

        let mut ring = column([cell])
            .padding(Sides::all(2.0))
            .radius(PANEL_RADIUS)
            .width(Size::Hug)
            .height(Size::Hug);
        if selected {
            ring = ring.stroke(vs::FOCUS_BORDER).stroke_width(2.0);
        }
        ring
    }

    /// Destructive-adjacent action and a dirty-count on the left, commit
    /// pair on the right, both aligned to the content column.
    fn actions(&self) -> El {
        row([
            spacer(),
            row([
                button("Restore defaults").key("restore").ghost(),
                text("3 settings changed").caption().text_color(REF_FG_4),
                spacer(),
                button("Cancel")
                    .key("cancel")
                    .ghost()
                    .padding(Sides::x(tokens::SPACE_3)),
                button("Save")
                    .key("save")
                    .primary()
                    .padding(Sides::x(15.0)),
            ])
            .gap(tokens::SPACE_3)
            .width(Size::Fixed(CONTENT_WIDTH))
            .align(Align::Center),
            spacer(),
        ])
        .fill(vs::STATUS_BAR_BG)
        .border_t()
        .border_color(vs::STATUS_BAR_BORDER)
        .height(Size::Fixed(FOOTER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }
}

/// A titled group: a tracked-out caption over a bordered panel whose
/// rows are separated by 1px internal rules.
fn group<I, E>(title: &str, rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let rows: Vec<El> = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let r: El = r.into();
            if i == 0 {
                r
            } else {
                // The rule eats into the row's own box, so a divided row
                // is one pixel taller to keep its content on the rhythm.
                r.height(Size::Fixed(ROW_HEIGHT + vs::HAIRLINE))
                    .border_t()
                    .border_color(vs::SIDE_BAR_SECTION_HEADER_BORDER)
            }
        })
        .collect();

    column([
        text(title)
            .caption()
            .letter_spacing(0.6)
            .text_color(vs::DESCRIPTION_FG),
        column(rows)
            .fill(vs::EDITOR_WIDGET_BG)
            .stroke(vs::PANEL_BORDER)
            .radius(PANEL_RADIUS)
            .clip()
            .width(Size::Fill(1.0))
            .align(Align::Stretch),
    ])
    .gap(7.0)
    .width(Size::Fill(1.0))
}

/// One settings row: modified gutter, two-line label stack, and a fixed
/// control column pinned right.
fn setting_row(label: &str, description: &str, modified: bool, control: impl Into<El>) -> El {
    row([
        column([column(Vec::<El>::new())
            .width(Size::Fixed(5.0))
            .height(Size::Fixed(5.0))
            .radius(tokens::RADIUS_PILL)
            .fill(if modified {
                vs::FOCUS_BORDER
            } else {
                Color::srgb_u8a(0, 0, 0, 0)
            })])
        .width(Size::Fixed(MARKER_WIDTH))
        .align(Align::Center)
        .justify(Justify::Center),
        column([
            text(label).label().text_color(vs::FOREGROUND).ellipsis(),
            text(description)
                .caption()
                .text_color(vs::DESCRIPTION_FG)
                .ellipsis(),
        ])
        .gap(1.0)
        .width(Size::Fill(1.0)),
        row([control.into()])
            .width(Size::Fixed(CONTROL_WIDTH))
            .align(Align::Center),
    ])
    .pr(tokens::SPACE_4)
    .height(Size::Fixed(ROW_HEIGHT))
    .width(Size::Fill(1.0))
    .align(Align::Center)
}

/// A switch with its state spelled out beside it, as the reference does.
fn toggle_control(key: &str, value: bool) -> El {
    row([
        switch(key, value),
        text(if value { "On" } else { "Off" })
            .label()
            .text_color(REF_FG_2),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

fn options(values: &[&str]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|v| ((*v).to_string(), (*v).to_string()))
        .collect()
}

/// The reference's ramp, registered back onto the workbench key
/// namespace so the tree above can stay in workbench vocabulary.
fn palette() -> Palette {
    let p = Palette {
        background: REF_CONTENT,
        foreground: REF_FG,

        // `card` is the *control well* here, not the settings panel:
        // the stock checkbox paints its unchecked box from this slot,
        // and the reference's unchecked box is the input value.
        card: REF_WELL,
        card_foreground: REF_FG,

        popover: REF_PANEL,
        popover_foreground: REF_FG,

        primary: REF_ACCENT,
        primary_foreground: Color::srgb_u8(255, 255, 255),

        secondary: REF_SELECTED,
        secondary_foreground: REF_FG_2,

        muted: REF_WELL,
        muted_foreground: REF_FG_3,

        accent: REF_SELECTED,
        accent_foreground: REF_FG,

        border: REF_CONTROL_BORDER,
        input: REF_CONTROL_BORDER,
        ring: REF_ACCENT,

        selection_bg: REF_ACCENT,

        ..theme::dark_modern_palette()
    };

    p.with_token("editor.background", REF_CONTENT)
        .with_token("sideBar.background", REF_RAIL)
        .with_token("sideBar.border", REF_RULE)
        .with_token("sideBarSectionHeader.border", REF_DIVIDER)
        .with_token("titleBar.activeBackground", REF_BAR)
        .with_token("titleBar.activeForeground", REF_FG)
        .with_token("titleBar.border", REF_RULE)
        .with_token("statusBar.background", REF_BAR)
        .with_token("statusBar.border", REF_RULE)
        .with_token("panel.border", REF_RULE)
        .with_token("editorWidget.background", REF_PANEL)
        .with_token("input.background", REF_WELL)
        .with_token("input.border", REF_CONTROL_BORDER)
        .with_token("dropdown.background", REF_WELL)
        .with_token("dropdown.border", REF_CONTROL_BORDER)
        .with_token("checkbox.background", REF_WELL)
        .with_token("checkbox.border", REF_CONTROL_BORDER)
        .with_token("button.background", REF_ACCENT)
        .with_token("focusBorder", REF_ACCENT)
        .with_token("descriptionForeground", REF_FG_3)
        .with_token("list.activeSelectionBackground", REF_SELECTED)
        .with_token("list.activeSelectionForeground", REF_FG)
}

impl App for Preferences {
    fn build(&self, _cx: &BuildCx) -> El {
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
                    select::select_menu_selected(THEME_KEY, options(THEMES), &self.color_theme)
                }),
                self.ui_font_open.then(|| {
                    select::select_menu_selected(FONT_KEY, options(UI_FONTS), &self.ui_font)
                }),
            ],
        )
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if select::apply_event(
            &mut self.color_theme,
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

        if checkbox::apply_event(&mut self.match_system, &event, "match-system")
            || checkbox::apply_event(&mut self.smooth_scrolling, &event, "smooth-scrolling")
            || checkbox::apply_event(&mut self.reduce_motion, &event, "reduce-motion")
            || checkbox::apply_event(&mut self.show_status_bar, &event, "show-status-bar")
            || switch::apply_event(&mut self.compact_mode, &event, "compact-mode")
            || switch::apply_event(&mut self.translucent_sidebar, &event, "translucent-sidebar")
        {
            return;
        }

        for (i, _) in ACCENTS.iter().enumerate() {
            if event.is_click_or_activate(&format!("accent:{i}")) {
                self.accent = i;
                return;
            }
        }
        for (label, _) in CATEGORIES {
            if event.is_click_or_activate(&format!("cat:{label}")) {
                self.category = (*label).to_string();
                return;
            }
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn theme(&self) -> Theme {
        theme::theme()
            .with_palette(palette())
            .with_radius_scale(4.0 / 7.0)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Preferences", viewport, Preferences::new())
}
