//! A full-window Preferences page on the workbench theme.
//!
//! The VS Code settings-tab genre: a slim header strip with a search
//! field, a 220px category rail, and a single readable content column of
//! settings rows. Everything but [`damascene_workbench::chrome`]'s
//! `hairline` is a **stock** damascene widget — `sidebar_menu_button`,
//! `select_trigger`, `numeric_input`, `checkbox`, `switch`, `field_with`,
//! `button` — rendered through [`damascene_workbench::theme::theme`].
//!
//! The settings row itself is `field_with(label, control,
//! FieldOpts::default().description(..).horizontal())`: label + dim
//! one-liner stacked left, control trailing right. That is the shape this
//! whole page is made of, repeated at one rhythm.
//!
//! Run: `cargo run -p damascene-workbench --example settings_v2`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// Width of the category rail.
const RAIL_WIDTH: f32 = 220.0;
/// The readable measure of the settings column. Wide enough for a
/// description line to breathe, narrow enough that the eye does not have
/// to track 1000px back to the next label.
const CONTENT_WIDTH: f32 = 720.0;
/// Trailing control width, so every select/field on the page flushes to
/// one right edge instead of hugging its own label.
const CONTROL_WIDTH: f32 = 240.0;
/// The header strip. Taller than the shell's 30px title bar because it
/// carries a control.
const HEADER_HEIGHT: f32 = 44.0;
/// Accent swatch edge.
const SWATCH: f32 = 20.0;

/// The six preference categories in the rail, with their icons.
const CATEGORIES: &[(&str, &str)] = &[
    ("General", "settings"),
    ("Appearance", "contrast"),
    ("Editor", "code"),
    ("Keyboard", "keyboard"),
    ("Network", "wifi"),
    ("Advanced", "terminal"),
];

/// The accent palette offered by the "Accent color" row.
const ACCENTS: &[(&str, Color)] = &[
    ("Blue", Color::srgb_u8(0, 120, 212)),
    ("Teal", Color::srgb_u8(20, 148, 140)),
    ("Violet", Color::srgb_u8(137, 108, 224)),
    ("Amber", Color::srgb_u8(214, 152, 58)),
    ("Rose", Color::srgb_u8(206, 84, 100)),
    ("Slate", Color::srgb_u8(120, 130, 145)),
];

struct Preferences {
    category: String,
    query: String,
    selection: Selection,
    theme_name: &'static str,
    ui_font: &'static str,
    font_size: String,
    accent: usize,
    smooth_scrolling: bool,
    reduce_motion: bool,
    show_status_bar: bool,
    render_whitespace: bool,
    compact_mode: bool,
    dim_unfocused: bool,
}

impl Preferences {
    fn new() -> Self {
        Self {
            category: "Appearance".into(),
            query: String::new(),
            selection: Selection::default(),
            theme_name: "Dark Modern",
            ui_font: "Inter",
            font_size: "13".into(),
            accent: 0,
            smooth_scrolling: true,
            reduce_motion: false,
            show_status_bar: true,
            render_whitespace: false,
            compact_mode: false,
            dim_unfocused: true,
        }
    }

    // -----------------------------------------------------------------
    // Regions.
    // -----------------------------------------------------------------

    /// Title on the left, settings search on the right. `titleBar.*`
    /// rather than `editor.*` so the strip steps down from the content
    /// well and its under-rule reads as a region boundary.
    fn header(&self) -> El {
        row([
            text("Preferences")
                .label()
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            spacer(),
            input_group([
                input_group_addon(icon("search").text_color(vs::INPUT_PLACEHOLDER_FG)),
                text_input_with(
                    "search",
                    &self.query,
                    &self.selection,
                    TextInputOpts::default().placeholder("Search settings"),
                ),
            ])
            .width(Size::Fixed(280.0)),
        ])
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(HEADER_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    /// The category rail — a plain `sidebar_menu` of stock
    /// `sidebar_menu_button_with_icon`s on `sideBar.background`.
    fn rail(&self) -> El {
        let items: Vec<El> = CATEGORIES
            .iter()
            .map(|(name, glyph)| {
                sidebar_menu_button_with_icon(*glyph, *name, self.category == *name)
                    .key(format!("cat:{name}"))
            })
            .collect();

        column([
            pane_header("SETTINGS", [chip("6")]),
            column([sidebar_menu(items)])
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

    /// The content well: one scrolling column, left-aligned at
    /// [`CONTENT_WIDTH`] rather than full-bleed.
    fn content(&self) -> El {
        column([column([self.appearance_section(), self.footer()])
            .gap(tokens::SPACE_6)
            .width(Size::Fixed(CONTENT_WIDTH))
            .height(Size::Hug)])
        .align(Align::Start)
        .padding(Sides::xy(tokens::SPACE_8, tokens::SPACE_5))
        .fill(vs::EDITOR_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .scrollable()
    }

    // -----------------------------------------------------------------
    // Sections.
    // -----------------------------------------------------------------

    fn appearance_section(&self) -> El {
        column([
            section_heading(
                "Appearance",
                "How the workbench looks. Changes apply to every window.",
            ),
            column([
                settings_row(
                    "Theme",
                    "Color theme used for the workbench and the editor.",
                    select_trigger("theme", self.theme_name).width(Size::Fixed(CONTROL_WIDTH)),
                ),
                separator(),
                settings_row(
                    "UI font family",
                    "Applies to menus, panels, and dialogs — not to the editor.",
                    select_trigger("ui_font", self.ui_font).width(Size::Fixed(CONTROL_WIDTH)),
                ),
                separator(),
                settings_row(
                    "Font size",
                    "Base interface size in points. The editor scales separately.",
                    numeric_input(
                        "font_size",
                        &self.font_size,
                        &self.selection,
                        NumericInputOpts::default().min(9.0).max(24.0).stacked(),
                    )
                    .width(Size::Fixed(96.0)),
                ),
                separator(),
                settings_row(
                    "Accent color",
                    "Tints selection, focus rings, and the active tab rule.",
                    self.accent_picker(),
                ),
                separator(),
                settings_row(
                    "Smooth scrolling",
                    "Animate the scroll position instead of jumping by line.",
                    checkbox("smooth_scrolling", self.smooth_scrolling),
                ),
                separator(),
                settings_row(
                    "Reduce motion",
                    "Disable panel slide, tab crossfade, and list reordering.",
                    checkbox("reduce_motion", self.reduce_motion),
                ),
                separator(),
                settings_row(
                    "Show status bar",
                    "Pin the branch, problem count, and cursor readout to the bottom edge.",
                    checkbox("show_status_bar", self.show_status_bar),
                ),
                separator(),
                settings_row(
                    "Render whitespace",
                    "Draw dots for spaces and arrows for tabs inside the editor.",
                    checkbox("render_whitespace", self.render_whitespace),
                ),
                separator(),
                settings_row(
                    "Compact mode",
                    "Tighten row heights and control padding across the workbench.",
                    switch("compact_mode", self.compact_mode),
                ),
                separator(),
                settings_row(
                    "Dim unfocused panels",
                    "Lower the contrast of panels that do not hold keyboard focus.",
                    switch("dim_unfocused", self.dim_unfocused),
                ),
            ])
            .gap(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_4)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
    }

    /// The action row, capped by a `hairline`. The rule is laid *between*
    /// two siblings that neither owns — the case `hairline` exists for,
    /// since per-side borders cover every other separator on this page.
    fn footer(&self) -> El {
        column([
            hairline(),
            row([
                button("Restore defaults").key("restore").ghost(),
                spacer(),
                button("Cancel").key("cancel").ghost(),
                button("Save").key("save").primary(),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_4)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
    }

    /// Six swatches; the selected one carries a 1px ring in the
    /// workbench foreground so the choice reads without a checkmark.
    fn accent_picker(&self) -> El {
        let swatches: Vec<El> = ACCENTS
            .iter()
            .enumerate()
            .map(|(i, (name, color))| {
                let mut sw = row(Vec::<El>::new())
                    .key(format!("accent:{name}"))
                    .tooltip(*name)
                    .focusable()
                    .cursor(Cursor::Pointer)
                    .fill(*color)
                    .radius(vs::RADIUS)
                    .width(Size::Fixed(SWATCH))
                    .height(Size::Fixed(SWATCH));
                if i == self.accent {
                    sw = sw.stroke(vs::FOREGROUND).stroke_width(1.0);
                }
                sw
            })
            .collect();

        row(swatches)
            .gap(tokens::SPACE_2)
            .width(Size::Hug)
            .height(Size::Hug)
            .align(Align::Center)
    }
}

// ---------------------------------------------------------------------
// Page-local recipes.
// ---------------------------------------------------------------------

/// A group heading: title over one dim line of orientation.
fn section_heading(title: &str, blurb: &str) -> El {
    column([
        text(title).title(),
        text(blurb).caption().muted().wrap_text().fill_width(),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

/// The one row shape this page is built from — stock `field_with` in its
/// horizontal orientation, which already stacks the description under
/// the label and trails the control right.
fn settings_row(label: &str, description: &str, control: impl Into<El>) -> El {
    field_with(
        label,
        control,
        FieldOpts::default().description(description).horizontal(),
    )
}

impl App for Preferences {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root rather than `page()`: a preferences *page*
        // in this genre is a full-window region stack, and window
        // padding would float it on a margin.
        column([
            self.header(),
            row([self.rail(), self.content()])
                .height(Size::Fill(1.0))
                .width(Size::Fill(1.0))
                .align(Align::Stretch),
        ])
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG)
    }

    /// A static mock, but the controls are real widgets, so the stock
    /// `apply_event` helpers are all the "logic" the page needs.
    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if text_input::apply_event(&mut self.query, &mut self.selection, &event, "search") {
            return;
        }
        for (name, _) in CATEGORIES {
            if event.is_click_or_activate(&format!("cat:{name}")) {
                self.category = (*name).to_string();
                return;
            }
        }
        for (i, (name, _)) in ACCENTS.iter().enumerate() {
            if event.is_click_or_activate(&format!("accent:{name}")) {
                self.accent = i;
                return;
            }
        }
        let flags: [(&str, &mut bool); 6] = [
            ("smooth_scrolling", &mut self.smooth_scrolling),
            ("reduce_motion", &mut self.reduce_motion),
            ("show_status_bar", &mut self.show_status_bar),
            ("render_whitespace", &mut self.render_whitespace),
            ("compact_mode", &mut self.compact_mode),
            ("dim_unfocused", &mut self.dim_unfocused),
        ];
        for (key, flag) in flags {
            if checkbox::apply_event(flag, &event, key) || switch::apply_event(flag, &event, key) {
                return;
            }
        }
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Preferences", viewport, Preferences::new())
}
