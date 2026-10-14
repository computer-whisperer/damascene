//! "Copperline" — an ECAD parts-library browser on the workbench theme.
//!
//! A reproduction of `references/workbench-validation/parts/reference.png`
//! (1280×800) built from the stock damascene vocabulary re-skinned by
//! [`damascene_workbench::theme::theme`]:
//!
//! - a 40px command strip — `text_input`, `toggle_group`, `select_trigger`,
//!   `button_with_icon().primary()`,
//! - the parts table — stock `table` / `table_header` / `table_row` /
//!   `table_cell`, zebra-striped, with the selected row on
//!   `list.activeSelectionBackground`,
//! - a 320px PROPERTIES inspector — `pane_header`, a footprint preview
//!   (content-level placeholder), `text_input` / `select_trigger` /
//!   `text_area` form rows, and an Apply/Revert action bar,
//! - `chrome::status_bar` across the bottom.
//!
//! Run: `cargo run -p damascene-workbench --example parts_match`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---------------------------------------------------------------------
// Metrics measured off the reference screenshot.
// ---------------------------------------------------------------------

const TOOLBAR_H: f32 = 40.0;
const CONTROL_H: f32 = 26.0;
const INSPECTOR_W: f32 = 320.0;
const TABLE_HEADER_H: f32 = 30.0;
const ROW_H: f32 = 28.0;
const FOOTER_H: f32 = 25.0;
const ACTION_BAR_H: f32 = 40.0;
const CELL_PAD_X: f32 = 8.0;
const FIELD_H: f32 = 22.0;
const LABEL_W: f32 = 78.0;

const W_REF: f32 = 97.0;
const W_VALUE: f32 = 186.0;
const W_FOOTPRINT: f32 = 266.0;
const W_STOCK: f32 = 86.0;

// ---------------------------------------------------------------------
// Content-level colors: values the app owns, not workbench tokens.
// ---------------------------------------------------------------------

/// Table zebra stripe — one step above `editor.background`.
const ROW_ALT_BG: Color = Color::srgb_u8(33, 33, 33);
/// Column / row separator inside the table.
const GRID_LINE: Color = Color::srgb_u8(46, 46, 46);
/// The `ATTRIBUTES` section rule (`#2B2B2B`). Was brightened to 80 to
/// compensate for the inside-band AA bug that rendered 1px fills at
/// ~35% coverage; the centered AA band fixed that, so the token value
/// reads at full contrast again.
const ATTR_RULE: Color = Color::srgb_u8(43, 43, 43);
/// Footprint-name column, KiCad's "copper" green.
const FOOTPRINT_FG: Color = Color::srgb_u8(169, 198, 161);
/// Inspector action-bar ground, one step below the panel.
const ACTION_BAR_BG: Color = Color::srgb_u8(26, 26, 26);
/// Table footer strip.
const FOOTER_BG: Color = Color::srgb_u8(27, 27, 27);
/// Footprint canvas — darker than any workbench surface.
const CANVAS_BG: Color = Color::srgb_u8(20, 20, 20);
/// Copper pads.
const COPPER: Color = Color::srgb_u8(193, 121, 58);
/// Courtyard / silkscreen annotation blue.
const COURTYARD: Color = Color::srgb_u8(90, 140, 190);
/// Status-bar sync pill ground and its indicator.
const SYNC_BG: Color = Color::srgb_u8(34, 58, 42);
const SYNC_FG: Color = Color::srgb_u8(120, 190, 145);
const SYNC_DOT: Color = Color::srgb_u8(78, 169, 107);

const TRANSPARENT: Color = Color::srgb_u8a(0, 0, 0, 0);

/// Component-class dot color — the ECAD convention of coloring a row by
/// what kind of part it is.
#[derive(Clone, Copy, PartialEq)]
enum Class {
    Passive,
    Semi,
    Mechanical,
    Ic,
}

impl Class {
    fn dot(self) -> Color {
        match self {
            Class::Passive => COPPER,
            Class::Semi => Color::srgb_u8(122, 158, 111),
            Class::Mechanical => Color::srgb_u8(74, 74, 74),
            Class::Ic => Color::srgb_u8(108, 143, 196),
        }
    }
}

struct Part {
    reference: &'static str,
    value: &'static str,
    footprint: &'static str,
    library: &'static str,
    stock: &'static str,
    /// `0` = plain, `1` = low (warning), `2` = out of stock.
    level: u8,
    class: Class,
}

const fn part(
    reference: &'static str,
    value: &'static str,
    footprint: &'static str,
    library: &'static str,
    stock: &'static str,
    level: u8,
    class: Class,
) -> Part {
    Part {
        reference,
        value,
        footprint,
        library,
        stock,
        level,
        class,
    }
}

const PARTS: &[Part] = &[
    part("C1", "100nF 50V X7R", "C_0402_1005Metric", "Device", "1,284", 0, Class::Passive),
    part("C2", "10µF 16V X5R", "C_0805_2012Metric", "Device", "642", 0, Class::Passive),
    part("C3", "22pF 50V C0G", "C_0402_1005Metric", "Device", "3,010", 0, Class::Passive),
    part("D1", "PMEG3005EJ", "D_SOD-323F", "Diode", "88", 0, Class::Semi),
    part("F1", "1812L110/16", "Fuse_1812_4532Metric", "Device", "24", 1, Class::Passive),
    part("J1", "USB4110-GF-A", "GCT_USB4110-GF-A", "Connector_USB", "61", 0, Class::Mechanical),
    part("L1", "2.2µH 1.7A", "L_Murata_DFE201610P", "Device", "190", 0, Class::Passive),
    part("Q1", "DMN2075U-7", "SOT-23", "Transistor_FET", "415", 0, Class::Semi),
    part("R1", "10kΩ 1%", "R_0603_1608Metric", "Device", "5,120", 0, Class::Passive),
    part("R2", "4.7kΩ 1%", "R_0402_1005Metric", "Device", "2,860", 0, Class::Passive),
    part("R3", "0.05Ω 1W", "R_1206_3216Metric", "Device", "0", 2, Class::Passive),
    part("R4", "100kΩ 0.1%", "R_0603_1608Metric", "Device", "731", 0, Class::Passive),
    part("SW1", "EVQ-P7C01P", "SW_SPST_EVQP", "Switch", "350", 0, Class::Mechanical),
    part("U1", "ATmega328P-AU", "TQFP-32_7x7mm_P0.8mm", "MCU_Microchip_ATmega", "37", 0, Class::Ic),
    part("U2", "LM358DR", "SOIC-8_3.9x4.9mm_P1.27mm", "Amplifier_Operational", "268", 0, Class::Ic),
    part("U3", "TPS62840DLCR", "SON-8_1.5x2mm_P0.5mm", "Regulator_Switching", "156", 0, Class::Ic),
    part("U4", "SN74LVC1G14DBVR", "SOT-23-5", "Logic_LevelTranslator", "902", 0, Class::Ic),
    part("U5", "W25Q128JVSIQ", "SOIC-8_5.23x5.23mm_P1.27mm", "Memory_Flash", "43", 0, Class::Ic),
    part("U6", "MCP1700T-3302E/TT", "SOT-23-3", "Regulator_Linear", "12", 1, Class::Ic),
    part("X1", "16MHz ±10ppm", "Crystal_SMD_3225-4Pin", "Device", "214", 0, Class::Passive),
];

const ATTRIBUTES: &[(&str, &str)] = &[
    ("Symbol", "Regulator_Switching:TPS62840"),
    ("Manufacturer", "Texas Instruments"),
    ("MPN", "TPS62840DLCR"),
    ("Supplier", "Digi-Key · 296-TPS62840DLCR"),
    ("Keywords", "buck dcdc low-iq"),
    ("Updated", "2026-07-19 14:02"),
];

// ---------------------------------------------------------------------

struct Copperline {
    query: String,
    search_mode: String,
    library: String,
    package: String,
    selected: usize,
    value: String,
    datasheet: String,
    description: String,
    selection: Selection,
    open_menu: Option<&'static str>,
}

impl Copperline {
    fn new() -> Self {
        Self {
            query: "0603".into(),
            search_mode: "case".into(),
            library: "Device".into(),
            package: "Any".into(),
            selected: 15,
            value: "TPS62840DLCR".into(),
            datasheet: "https://www.ti.com/lit/ds/symlink/tps62840.pdf".into(),
            description: "750mA synchronous step-down converter, 60nA quiescent \
                          current, 1.8–6.5V input, DCS-Control, adjustable output."
                .into(),
            selection: Selection::default(),
            open_menu: None,
        }
    }

    // -----------------------------------------------------------------
    // Command strip.
    // -----------------------------------------------------------------

    fn toolbar(&self) -> El {
        row([
            icon("activity").icon_size(16.0).text_color(COPPER),
            text("Copperline")
                .label()
                .medium()
                .text_color(vs::FOREGROUND),
            self.search_field(),
            rule(),
            select_trigger("library", format!("Library   {}", self.library))
                .width(Size::Fixed(170.0))
                .height(Size::Fixed(CONTROL_H)),
            select_trigger("package", format!("Package   {}", self.package))
                .width(Size::Fixed(164.0))
                .height(Size::Fixed(CONTROL_H)),
            rule(),
            button_with_icon("plus", "Add Part")
                .key("add-part")
                .primary()
                .height(Size::Fixed(CONTROL_H)),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(TOOLBAR_H))
        .width(Size::Fill(1.0))
        .align(Align::Center)
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
    }

    /// The search box. `text_input` has no affix slots, so the field
    /// chrome lives on the wrapping row and the input's own surface is
    /// neutralised — see the gap list in the module docs.
    fn search_field(&self) -> El {
        row([
            icon("search")
                .icon_size(13.0)
                .text_color(vs::INPUT_PLACEHOLDER_FG),
            text_input_with(
                "search",
                &self.query,
                &self.selection,
                TextInputOpts::default().placeholder("Search parts, values, footprints…"),
            )
            .width(Size::Fill(1.0))
            .height(Size::Fixed(20.0))
            .fill(TRANSPARENT)
            .stroke(TRANSPARENT)
            .padding(Sides::all(0.0)),
            toggle_group(
                "search-mode",
                &self.search_mode,
                [("case", "Aa"), ("word", "ab"), ("regex", ".*")],
            )
            .height(Size::Fixed(20.0)),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_2))
        .width(Size::Fill(1.0))
        .height(Size::Fixed(CONTROL_H))
        .align(Align::Center)
        .fill(vs::INPUT_BG)
        .stroke(vs::INPUT_BORDER)
        .radius(vs::RADIUS)
    }

    // -----------------------------------------------------------------
    // Parts table.
    // -----------------------------------------------------------------

    fn table_column(&self) -> El {
        column([
            column([table([table_header([self.header_row()])])])
                .fill(vs::SIDE_BAR_BG)
                .width(Size::Fill(1.0)),
            column([table([table_body(
                PARTS
                    .iter()
                    .enumerate()
                    .map(|(i, p)| self.part_row(i, p))
                    .collect::<Vec<_>>(),
            )])])
            .scrollable()
            .key("parts-scroll")
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
            self.table_footer(),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG)
    }

    fn header_row(&self) -> El {
        let head = |label: &str, w: Option<f32>, ruled: bool| {
            let mut cell = table_head(label.to_string())
                .medium()
                .letter_spacing(0.6)
                .padding(Sides::x(CELL_PAD_X + 2.0));
            cell = match w {
                Some(w) => cell.width(Size::Fixed(w)),
                None => cell.width(Size::Fill(1.0)),
            };
            if ruled {
                cell = cell.border_r().border_color(GRID_LINE);
            }
            cell
        };

        table_row([
            table_head_el(
                row([
                    text("REFERENCE").caption(),
                    icon("chevron-up").icon_size(11.0),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
            )
            .width(Size::Fixed(W_REF))
            .padding(Sides::x(CELL_PAD_X + 2.0))
            .border_r()
            .border_color(GRID_LINE),
            head("VALUE", Some(W_VALUE), true),
            head("FOOTPRINT", Some(W_FOOTPRINT), true),
            head("LIBRARY", None, true),
            table_head("STOCK")
                .medium()
                .letter_spacing(0.6)
                .text_align(TextAlign::End)
                .width(Size::Fixed(W_STOCK))
                .padding(Sides::x(tokens::SPACE_3)),
        ])
        .height(Size::Fixed(TABLE_HEADER_H))
        .align(Align::Center)
    }

    fn part_row(&self, i: usize, p: &Part) -> El {
        let selected = i == self.selected;
        let (fg, muted, fp_fg) = if selected {
            (
                vs::LIST_ACTIVE_SELECTION_FG,
                vs::LIST_ACTIVE_SELECTION_FG,
                vs::LIST_ACTIVE_SELECTION_FG,
            )
        } else {
            (vs::FOREGROUND, vs::DESCRIPTION_FG, FOOTPRINT_FG)
        };
        let stock_fg = match (selected, p.level) {
            (true, _) => vs::LIST_ACTIVE_SELECTION_FG,
            (_, 1) => vs::CHAT_EDITED_FILE_FG,
            (_, 2) => vs::ERROR_FG,
            _ => vs::FOREGROUND,
        };

        let fill = if selected {
            vs::LIST_ACTIVE_SELECTION_BG
        } else if i % 2 == 1 {
            ROW_ALT_BG
        } else {
            vs::EDITOR_BG
        };

        table_row([
            table_cell(
                row([
                    dot(p.class.dot()),
                    text(p.reference).label().mono().text_color(fg),
                ])
                .gap(tokens::SPACE_2)
                .align(Align::Center),
            )
            .width(Size::Fixed(W_REF))
            .padding(Sides::x(tokens::SPACE_3)),
            table_cell(text(p.value).label().mono().text_color(fg))
                .width(Size::Fixed(W_VALUE))
                .padding(Sides::x(CELL_PAD_X)),
            table_cell(text(p.footprint).label().mono().text_color(fp_fg))
                .width(Size::Fixed(W_FOOTPRINT))
                .padding(Sides::x(CELL_PAD_X)),
            table_cell(text(p.library).label().text_color(muted))
                .width(Size::Fill(1.0))
                .padding(Sides::x(CELL_PAD_X)),
            table_cell(
                text(p.stock)
                    .label()
                    .mono()
                    .tabular_numerals()
                    .text_align(TextAlign::End)
                    .text_color(stock_fg),
            )
            .width(Size::Fixed(W_STOCK))
            .padding(Sides::x(tokens::SPACE_3)),
        ])
        .key(format!("part:{}", p.reference))
        .height(Size::Fixed(ROW_H))
        .align(Align::Center)
        .fill(fill)
        .border_color(GRID_LINE)
    }

    fn table_footer(&self) -> El {
        row([
            text_runs([
                text("Showing ").caption().text_color(vs::DESCRIPTION_FG),
                text("20").caption().medium().text_color(vs::FOREGROUND),
                text(" of 412 parts — filtered by ")
                    .caption()
                    .text_color(vs::DESCRIPTION_FG),
            ])
            .width(Size::Hug),
            text("\"0603\", library Device")
                .caption()
                .mono()
                .text_color(vs::FOREGROUND),
            spacer(),
            text("sort: reference ▲  ·  group: none")
                .caption()
                .mono()
                .text_color(vs::DESCRIPTION_FG),
        ])
        .gap(tokens::SPACE_1)
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(FOOTER_H))
        .width(Size::Fill(1.0))
        .align(Align::Center)
        .fill(FOOTER_BG)
        .text_color(vs::DESCRIPTION_FG)
        .border_t()
        .border_color(vs::PANEL_BORDER)
    }

    // -----------------------------------------------------------------
    // Inspector.
    // -----------------------------------------------------------------

    fn inspector(&self) -> El {
        column([
            pane_header(
                "PROPERTIES",
                [
                    ghost_icon("upload", "pop-out"),
                    ghost_icon("more-horizontal", "pane-menu"),
                ],
            )
            .height(Size::Fixed(TABLE_HEADER_H))
            .fill(vs::PANEL_BG)
            .border_color(TRANSPARENT),
            column([
                column([footprint_preview()]).align(Align::Center),
                text("SON-8_1.5x2mm_P0.5mm  ·  8 pads + EP")
                    .caption()
                    .mono()
                    .text_color(vs::DESCRIPTION_FG)
                    .text_align(TextAlign::Center)
                    .width(Size::Fill(1.0)),
                self.property_form(),
                self.attributes(),
            ])
            .gap(tokens::SPACE_3)
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_3))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .scrollable()
            .key("inspector-scroll"),
            self.action_bar(),
        ])
        .width(Size::Fixed(INSPECTOR_W))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .fill(vs::PANEL_BG)
        .border_l()
        .border_color(vs::PANEL_BORDER)
    }

    fn property_form(&self) -> El {
        form([
            field(
                "Value",
                text_input("prop-value", &self.value, &self.selection)
                    .width(Size::Fill(1.0))
                    .height(Size::Fixed(FIELD_H)),
            ),
            field(
                "Footprint",
                select_trigger("prop-footprint", "Package_SON:SON-8_1.5x2mm_P0.5mm")
                    .width(Size::Fill(1.0))
                    .height(Size::Fixed(FIELD_H)),
            ),
            field(
                "Datasheet",
                text_input("prop-datasheet", &self.datasheet, &self.selection)
                    .width(Size::Fill(1.0))
                    .height(Size::Fixed(FIELD_H)),
            ),
            field(
                "Description",
                text_area("prop-description", &self.description, &self.selection)
                    .width(Size::Fill(1.0))
                    .height(Size::Fixed(110.0)),
            ),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::all(0.0))
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn attributes(&self) -> El {
        let rows: Vec<El> = ATTRIBUTES
            .iter()
            .map(|(k, v)| {
                row([
                    text(*k)
                        .caption()
                        .text_color(vs::DESCRIPTION_FG)
                        .text_align(TextAlign::End)
                        .width(Size::Fixed(LABEL_W)),
                    text(*v)
                        .caption()
                        .mono()
                        .ellipsis()
                        .text_color(vs::FOREGROUND)
                        .width(Size::Fill(1.0)),
                ])
                .gap(tokens::SPACE_2)
                .height(Size::Fixed(20.0))
                .width(Size::Fill(1.0))
                .align(Align::Center)
            })
            .collect();

        column([
            row([
                text("ATTRIBUTES")
                    .caption()
                    .medium()
                    .letter_spacing(0.6)
                    .text_color(vs::DESCRIPTION_FG),
                separator().fill(ATTR_RULE),
            ])
            .gap(tokens::SPACE_2)
            .height(Size::Fixed(24.0))
            .width(Size::Fill(1.0))
            .align(Align::Center),
            column(rows).width(Size::Fill(1.0)).align(Align::Stretch),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn action_bar(&self) -> El {
        row([
            text("2 unsaved edits")
                .caption()
                .text_color(vs::DESCRIPTION_FG),
            dot(vs::CHAT_EDITED_FILE_FG),
            spacer(),
            button("Revert")
                .key("revert")
                .secondary()
                .height(Size::Fixed(24.0)),
            button("Apply")
                .key("apply")
                .primary()
                .height(Size::Fixed(24.0)),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_3))
        .height(Size::Fixed(ACTION_BAR_H))
        .width(Size::Fill(1.0))
        .align(Align::Center)
        .fill(ACTION_BAR_BG)
        .border_t()
        .border_color(vs::PANEL_BORDER)
    }
}

// ---------------------------------------------------------------------
// Small shared pieces.
// ---------------------------------------------------------------------

/// A short vertical rule between toolbar groups — the case
/// [`chrome::hairline`] documents per-side borders cannot express.
fn rule() -> El {
    vertical_separator()
        .height(Size::Fixed(16.0))
        .fill(vs::PANEL_BORDER)
}

/// A 7px status dot.
fn dot(color: Color) -> El {
    column(Vec::<El>::new())
        .width(Size::Fixed(7.0))
        .height(Size::Fixed(7.0))
        .radius(tokens::RADIUS_PILL)
        .fill(color)
}

/// A borderless icon action for a pane header.
fn ghost_icon(name: &str, key: &str) -> El {
    icon_button(name)
        .key(key.to_string())
        .ghost()
        .width(Size::Fixed(20.0))
        .height(Size::Fixed(20.0))
        .icon_size(13.0)
        .text_color(vs::DESCRIPTION_FG)
}

/// A right-aligned-label / fill-control inspector row. `field_row()`
/// puts the label on the left and hugs the control on the right, which
/// is the opposite geometry — see the gap list.
fn field(label: &str, control: El) -> El {
    row([
        text(label)
            .label()
            .text_color(vs::DESCRIPTION_FG)
            .text_align(TextAlign::End)
            .width(Size::Fixed(LABEL_W)),
        control,
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
    .align(Align::Start)
}

/// CONTENT: a simplified SON-8 footprint. The real thing is a
/// board-geometry canvas; this is eight copper pads, a body outline, an
/// exposed pad and a courtyard box, composed from stock layout nodes.
fn footprint_preview() -> El {
    let pad = || {
        column(Vec::<El>::new())
            .width(Size::Fixed(34.0))
            .height(Size::Fixed(12.0))
            .radius(1.0)
            .fill(COPPER)
    };
    let pads = || {
        column([pad(), pad(), pad(), pad()])
            .gap(10.0)
            .width(Size::Hug)
            .align(Align::Stretch)
    };

    let body = column([column(Vec::<El>::new())
        .width(Size::Fixed(38.0))
        .height(Size::Fixed(74.0))
        .radius(1.0)
        .fill(COPPER)])
    .width(Size::Fixed(58.0))
    .height(Size::Fixed(118.0))
    .align(Align::Center)
    .justify(Justify::Center)
    .stroke(Color::srgb_u8(220, 220, 220))
    .fill(TRANSPARENT);

    let courtyard = row([pads(), body, pads()])
        .gap(12.0)
        .padding(Sides::all(tokens::SPACE_2))
        .width(Size::Hug)
        .height(Size::Hug)
        .align(Align::Center)
        .stroke(COURTYARD);

    column([
        text("U3")
            .caption()
            .mono()
            .text_align(TextAlign::Center)
            .text_color(COURTYARD)
            .width(Size::Fill(1.0)),
        column([courtyard])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Center)
            .justify(Justify::Center),
        row([
            text("F.Cu · F.SilkS · F.CrtYd")
                .caption()
                .mono()
                .text_color(Color::srgb_u8(110, 110, 110)),
            spacer(),
            text("8 : 1")
                .caption()
                .mono()
                .text_color(Color::srgb_u8(110, 110, 110)),
        ])
        .width(Size::Fill(1.0))
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_1)
    .padding(Sides::all(tokens::SPACE_2))
    .width(Size::Fixed(240.0))
    .height(Size::Fixed(238.0))
    .align(Align::Stretch)
    .fill(CANVAS_BG)
    .stroke(vs::PANEL_BORDER)
    .radius(vs::RADIUS)
}

/// The status bar's "library sync" pill — a `chip` re-tinted green.
fn sync_pill() -> El {
    row([
        dot(SYNC_DOT),
        text("Library sync · up to date · 2 min ago")
            .caption()
            .nowrap_text()
            .text_color(SYNC_FG),
    ])
    .gap(tokens::SPACE_1)
    .padding(Sides::x(tokens::SPACE_2))
    .height(Size::Fixed(18.0))
    .width(Size::Hug)
    .align(Align::Center)
    .fill(SYNC_BG)
    .radius(tokens::RADIUS_PILL)
}

// ---------------------------------------------------------------------

impl App for Copperline {
    fn build(&self, _cx: &BuildCx) -> El {
        let selected = &PARTS[self.selected];

        let shell = column([
            self.toolbar(),
            row([self.table_column(), self.inspector()])
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    text("412 parts · 1 selected").caption().nowrap_text(),
                    text(format!("{}  —  {}", selected.reference, selected.value))
                        .caption()
                        .mono()
                        .nowrap_text(),
                ],
                [
                    text("v4.2.1  ·  schema 7")
                        .caption()
                        .mono()
                        .nowrap_text()
                        .text_color(vs::DESCRIPTION_FG),
                    sync_pill(),
                ],
            ),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG);

        let mut layers: Vec<El> = vec![shell];
        if let Some("library") = self.open_menu {
            layers.push(select_menu(
                "library",
                [
                    ("Device", "Device"),
                    ("Diode", "Diode"),
                    ("Connector_USB", "Connector_USB"),
                    ("Regulator_Switching", "Regulator_Switching"),
                ],
            ));
        }
        if let Some("package") = self.open_menu {
            layers.push(select_menu(
                "package",
                [("Any", "Any"), ("SMD", "SMD"), ("THT", "THT")],
            ));
        }
        stack(layers).width(Size::Fill(1.0)).height(Size::Fill(1.0))
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if text_input::apply_event(&mut self.value, &mut self.selection, &event, "prop-value")
            || text_input::apply_event(
                &mut self.datasheet,
                &mut self.selection,
                &event,
                "prop-datasheet",
            )
            || text_input::apply_event(&mut self.query, &mut self.selection, &event, "search")
            || text_area::apply_event(
                &mut self.description,
                &mut self.selection,
                &event,
                "prop-description",
            )
        {
            return;
        }
        if toggle::apply_event_single(&mut self.search_mode, &event, "search-mode", |raw| {
            Some(raw.to_string())
        }) {
            return;
        }
        for (key, field) in [
            ("library", 0usize),
            ("package", 1usize),
        ] {
            match select::classify_event(&event, key) {
                Some(select::SelectAction::Toggle) => {
                    self.open_menu = if self.open_menu == Some(leak(key)) {
                        None
                    } else {
                        Some(leak(key))
                    };
                    return;
                }
                Some(select::SelectAction::Pick(v)) => {
                    if field == 0 {
                        self.library = v;
                    } else {
                        self.package = v;
                    }
                    self.open_menu = None;
                    return;
                }
                Some(select::SelectAction::Dismiss) => {
                    self.open_menu = None;
                    return;
                }
                _ => {}
            }
        }
        for (i, p) in PARTS.iter().enumerate() {
            if event.is_click_or_activate(&format!("part:{}", p.reference)) {
                self.selected = i;
                self.value = p.value.to_string();
                return;
            }
        }
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn leak(key: &str) -> &'static str {
    match key {
        "library" => "library",
        _ => "package",
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Copperline — parts library", viewport, Copperline::new())
}
