//! A full-window Preferences page on the workbench theme.
//!
//! The VS Code settings-tab genre: a slim header strip carrying the page
//! title and a settings search, a 220px category rail, and one readable
//! content column of settings rows broken into labelled groups.
//!
//! Everything is a **stock** damascene widget or a
//! [`damascene_workbench::chrome`] recipe — `title_bar`, `section_header`,
//! `hairline`, `sidebar_menu_button_with_icon`, `field_with`,
//! `select_trigger`, `numeric_input`, `color_swatch`, `checkbox`,
//! `switch`, `button` — rendered through
//! [`damascene_workbench::theme::theme`].
//!
//! Three shapes carry the whole page:
//!
//! - the **row**: `field_with(label, control, FieldOpts::horizontal()
//!   .description(..).control_width(..))`. Label over a dim one-liner on
//!   the left, control trailing right. `control_width` is set once per
//!   group instead of per control, so every select on the page lands on
//!   one right edge and a checkbox in the same stack still keeps its own
//!   16px self (`field_with`'s "Control sizing" rules 2 and 3).
//! - the **group**: a small dim label over a `field_group` at
//!   [`tokens::SPACE_4`] — one rung tighter than the stock `SPACE_7`,
//!   which is a page-of-forms rhythm rather than a settings-tab one.
//! - the **page section**: `chrome::section_header`, title over one dim
//!   line of orientation, then the groups.
//!
//! Run: `cargo run -p damascene-workbench --example settings_v3`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// Width of the category rail.
const RAIL_WIDTH: f32 = 220.0;

/// The readable measure of the settings column.
///
/// Left-aligned in the well rather than centered or full-bleed: a
/// settings tab is a document that starts at the region's leading edge,
/// and a 1060px-wide row would make the eye track the full window back
/// to the next label.
const CONTENT_WIDTH: f32 = 740.0;

/// The shared right edge every trailing control in a group flushes to.
const CONTROL_WIDTH: f32 = 240.0;

/// Header strip height.
///
/// [`vs::TITLE_BAR_HEIGHT`] is 30px, which is the height of a strip that
/// holds *text*. This one holds a control: at the workbench default rung
/// the search input is 28px, so a 30px strip would leave no room at all
/// for its focus-ring band. 40px is the shortest strip that clears it.
const HEADER_HEIGHT: f32 = 40.0;

/// Width of the search field in the header.
const SEARCH_WIDTH: f32 = 260.0;

/// The six preference categories, with the rail glyph for each.
const CATEGORIES: &[(&str, &str)] = &[
    ("General", "settings"),
    ("Appearance", "contrast"),
    ("Editor", "code"),
    ("Keyboard", "keyboard"),
    ("Network", "wifi"),
    ("Advanced", "terminal"),
];

/// The accent candidates offered by the "Accent color" row.
const ACCENTS: &[(&str, Color)] = &[
    ("Azure", Color::srgb_u8(0, 120, 212)),
    ("Teal", Color::srgb_u8(20, 148, 140)),
    ("Iris", Color::srgb_u8(137, 108, 224)),
    ("Amber", Color::srgb_u8(214, 152, 58)),
    ("Rose", Color::srgb_u8(206, 84, 100)),
    ("Graphite", Color::srgb_u8(120, 130, 145)),
];

struct Preferences {
    category: &'static str,
    query: String,
    selection: Selection,
    theme_name: &'static str,
    ui_font: &'static str,
    font_size: String,
    accent: usize,
    smooth_scrolling: bool,
    reduce_motion: bool,
    show_status_bar: bool,
    compact_mode: bool,
    dim_inactive: bool,
}

impl Preferences {
    fn new() -> Self {
        Self {
            category: "Appearance",
            query: String::new(),
            selection: Selection::default(),
            theme_name: "Dark Modern",
            ui_font: "Inter",
            font_size: "13".into(),
            accent: 0,
            smooth_scrolling: true,
            reduce_motion: false,
            show_status_bar: true,
            compact_mode: false,
            dim_inactive: true,
        }
    }

    // -----------------------------------------------------------------
    // Regions.
    // -----------------------------------------------------------------

    /// The header strip — the stock [`title_bar`] recipe, raised to
    /// [`HEADER_HEIGHT`] so it can carry a control (see that constant).
    ///
    /// Nothing else about the recipe changes: same `titleBar.*` ground,
    /// same under-rule, same leading/spacer/trailing split, so the strip
    /// still reads as the same object as the shell's title bar.
    fn header(&self) -> El {
        title_bar(
            [text("Preferences").label().font_weight(FontWeight::Medium)],
            [input_group([
                input_group_addon(
                    icon("search")
                        .icon_size(tokens::ICON_SM)
                        .text_color(vs::INPUT_PLACEHOLDER_FG),
                ),
                text_input_with(
                    "search",
                    &self.query,
                    &self.selection,
                    TextInputOpts::default().placeholder("Search settings"),
                ),
            ])
            .width(Size::Fixed(SEARCH_WIDTH))],
        )
        // Two stamps on the recipe, and only two: the extra height a
        // control-bearing strip needs, and the matching inset so the
        // title does not sit tighter than the rail's items below it.
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(HEADER_HEIGHT))
    }

    /// The category rail: a stock `sidebar_menu` on `sideBar.background`,
    /// separated from the content well by a right border rather than by a
    /// gap.
    ///
    /// A plain bordered column rather than core's `sidebar()`, which is
    /// shadcn's *floating* rail — a boxed panel with a stroke on all four
    /// sides. Against a window edge only the inboard edge is a boundary.
    fn rail(&self) -> El {
        let items: Vec<El> = CATEGORIES
            .iter()
            .map(|(name, glyph)| {
                sidebar_menu_button_with_icon(*glyph, *name, self.category == *name)
                    .key(format!("cat:{name}"))
            })
            .collect();

        column([sidebar_group([
            sidebar_group_label("CATEGORIES"),
            sidebar_menu(items),
        ])])
        .fill(vs::SIDE_BAR_BG)
        .border_r()
        .border_color(vs::SIDE_BAR_BORDER)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_3))
        .width(Size::Fixed(RAIL_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .scrollable()
    }

    /// The content well — a scrolling region holding one fixed-measure
    /// column, aligned to the well's leading edge.
    fn content(&self) -> El {
        let column_el = column([
            section_header(
                "Appearance",
                "How the workbench looks. Changes apply to every window immediately.",
            ),
            group("THEME & COLOR", [self.theme_row(), self.accent_row()]),
            group("TYPOGRAPHY", [self.ui_font_row(), self.font_size_row()]),
            group("INTERFACE", self.interface_rows()),
            self.footer(),
        ])
        .gap(tokens::SPACE_6)
        .width(Size::Fixed(CONTENT_WIDTH))
        .height(Size::Hug);

        column([column_el])
            .align(Align::Start)
            .padding(Sides::xy(tokens::SPACE_8, tokens::SPACE_6))
            .fill(vs::EDITOR_BG)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .scrollable()
    }

    // -----------------------------------------------------------------
    // Rows.
    // -----------------------------------------------------------------

    fn theme_row(&self) -> El {
        settings_row(
            "Color theme",
            "Applies to the workbench chrome and to editor syntax together.",
            select_trigger("theme", self.theme_name),
        )
    }

    /// Six stock `color_swatch`es; the selected one carries the widget's
    /// own `foreground` ring, so the choice reads without a checkmark and
    /// without the row reflowing as it moves.
    ///
    /// The swatch row is the one control on this page that is *not*
    /// pinned to [`CONTROL_WIDTH`] — it hugs, and the horizontal field
    /// shape trails it right anyway, so it still lands on the shared
    /// edge.
    fn accent_row(&self) -> El {
        let swatches: Vec<El> = ACCENTS
            .iter()
            .enumerate()
            .map(|(i, (name, color))| {
                color_swatch(&format!("accent:{name}"), *color, i == self.accent)
            })
            .collect();

        let picker = row(swatches)
            .gap(tokens::SPACE_2)
            .width(Size::Hug)
            .height(Size::Hug)
            .align(Align::Center);

        // Built here rather than baked into the constant: the row says
        // which accent is live, which is what makes a swatch strip
        // readable without hovering every well.
        let description = format!(
            "{} — tints selection, focus rings, and the active tab rule.",
            ACCENTS[self.accent].0
        );
        settings_row("Accent color", &description, picker)
    }

    fn ui_font_row(&self) -> El {
        settings_row(
            "UI font family",
            "Menus, panels, and dialogs. The editor keeps its own font.",
            select_trigger("ui_font", self.ui_font),
        )
    }

    /// The one control on the page that wants to be *narrower* than the
    /// shared edge: three digits and a unit. An explicit `.width(...)`
    /// beats the group's `control_width` outright (`field_with`'s
    /// "Control sizing" rule 1) while the horizontal shape still trails
    /// it right, so it flushes with the selects above it.
    fn font_size_row(&self) -> El {
        settings_row(
            "Font size",
            "Base interface size. Editor text scales from this separately.",
            numeric_input(
                "font_size",
                &self.font_size,
                &self.selection,
                NumericInputOpts::default()
                    .min(9.0)
                    .max(24.0)
                    .suffix("pt")
                    .stacked(),
            )
            .width(Size::Fixed(132.0)),
        )
    }

    /// Checkbox settings first, then the two mode switches.
    ///
    /// The split is deliberate and is the stock reading of the two
    /// controls: a checkbox states a fact about the next render, a switch
    /// puts the workbench into a mode. Mixing them in one stack at random
    /// is what makes a settings page look generated.
    fn interface_rows(&self) -> Vec<El> {
        vec![
            settings_row(
                "Smooth scrolling",
                "Animate the scroll position instead of jumping by line.",
                checkbox("smooth_scrolling", self.smooth_scrolling),
            ),
            settings_row(
                "Reduce motion",
                "Disable panel slide, tab crossfade, and list reordering.",
                checkbox("reduce_motion", self.reduce_motion),
            ),
            settings_row(
                "Show status bar",
                "Pin the branch, problem count, and cursor readout to the bottom edge.",
                checkbox("show_status_bar", self.show_status_bar),
            ),
            settings_row(
                "Compact mode",
                "Tighten row heights and control padding across the whole workbench.",
                switch("compact_mode", self.compact_mode),
            ),
            settings_row(
                "Dim inactive panels",
                "Lower the contrast of panels that do not hold keyboard focus.",
                switch("dim_inactive", self.dim_inactive),
            ),
        ]
    }

    /// The action row, capped by a [`hairline`].
    ///
    /// The rule is laid *between* two siblings that neither of them owns
    /// — the one case per-side borders cannot express, and the case
    /// `hairline` exists for. It paints because the content column aligns
    /// `Stretch` (the default), which is the condition `hairline`'s
    /// rustdoc spells out.
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
}

// ---------------------------------------------------------------------
// Page-local recipes.
// ---------------------------------------------------------------------

/// A settings row: label over a dim one-liner, control trailing right,
/// pinned to the page's shared control edge.
fn settings_row(label: &str, description: &str, control: impl Into<El>) -> El {
    field_with(
        label,
        control,
        FieldOpts::default()
            .description(description)
            .horizontal()
            .control_width(CONTROL_WIDTH),
    )
}

/// A labelled group of settings rows.
///
/// The label is a dim caption rather than a stock `field_set` legend:
/// the legend is set in the Title role, the *same* rung
/// `chrome::section_header` puts the page heading on, so nesting one
/// inside the other flattens the two levels this page needs. A caption
/// under a title is the step the hierarchy is missing.
///
/// The rows themselves are a stock `field_group`, re-geared from its
/// `SPACE_7` page-of-forms rhythm to `SPACE_4` — dense enough that a
/// group reads as one block, open enough that a description line does
/// not touch the label below it.
fn group<I, E>(label: &str, rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        text(label)
            .caption()
            .semibold()
            .muted()
            .width(Size::Fill(1.0)),
        field_group(rows).gap(tokens::SPACE_4),
    ])
    .gap(tokens::SPACE_3)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

impl App for Preferences {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root rather than `page()`: this is a full-window
        // region stack, and `page()`'s window padding would float the
        // whole thing on a margin — signal 4 of the diagnosis the
        // workbench crate exists to invert.
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

    /// A static mock — but every control on it is a real widget, so the
    /// stock `apply_event` helpers are all the wiring the page needs to
    /// stop feeling like a screenshot.
    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if text_input::apply_event(&mut self.query, &mut self.selection, &event, "search") {
            return;
        }
        if numeric_input::apply_event(
            &mut self.font_size,
            &mut self.selection,
            "font_size",
            &NumericInputOpts::default().min(9.0).max(24.0).suffix("pt"),
            &event,
        ) {
            return;
        }
        for (name, _) in CATEGORIES {
            if event.is_click_or_activate(&format!("cat:{name}")) {
                self.category = name;
                return;
            }
        }
        for (i, (name, _)) in ACCENTS.iter().enumerate() {
            if event.is_click_or_activate(&format!("accent:{name}")) {
                self.accent = i;
                return;
            }
        }
        let flags: [(&str, &mut bool); 5] = [
            ("smooth_scrolling", &mut self.smooth_scrolling),
            ("reduce_motion", &mut self.reduce_motion),
            ("show_status_bar", &mut self.show_status_bar),
            ("compact_mode", &mut self.compact_mode),
            ("dim_inactive", &mut self.dim_inactive),
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
