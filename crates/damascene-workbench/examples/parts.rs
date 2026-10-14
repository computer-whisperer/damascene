//! Copperline — an ECAD parts-library browser on the workbench theme.
//!
//! A static mock of the shape this genre of tool always converges on: a
//! filter toolbar across the top, a dense table of parts filling the
//! work area, a properties inspector docked right, and a status bar
//! along the bottom. Everything but [`damascene_workbench::chrome`]'s
//! `status_bar` / `pane_header` / `hairline` is a stock damascene widget
//! — `table`, `text_input`, `select_trigger`, `text_area`, `button` —
//! rendered through [`damascene_workbench::theme::theme`].
//!
//! The controls are wired far enough to be honest (the selects open and
//! pick, the fields type, clicking a row moves the selection and
//! repopulates the inspector), but nothing is persisted and the table is
//! not actually filtered — this is a layout study, not an application.
//!
//! Run: `cargo run -p damascene-workbench --example parts`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

/// Toolbar strip height. Taller than [`vs::TITLE_BAR_HEIGHT`] because it
/// carries 28px controls plus their focus ring, not just a text label.
const TOOLBAR_HEIGHT: f32 = 38.0;
/// Inspector width. VS Code's side bar sits at ~256px; a properties
/// pane holding a footprint preview and labelled fields wants more.
const INSPECTOR_WIDTH: f32 = 320.0;
/// Width of the inspector's label gutter — wide enough for "Datasheet".
const FIELD_LABEL_WIDTH: f32 = 68.0;
/// Side of the square footprint-preview placeholder.
const PREVIEW_SIDE: f32 = 176.0;

const SEARCH_KEY: &str = "toolbar:search";
const LIBRARY_KEY: &str = "toolbar:library";
const PACKAGE_KEY: &str = "toolbar:package";
const VALUE_KEY: &str = "inspector:value";
const FOOTPRINT_KEY: &str = "inspector:footprint";
const DATASHEET_KEY: &str = "inspector:datasheet";
const DESCRIPTION_KEY: &str = "inspector:description";

const SEARCH_PLACEHOLDER: &str = "Search parts, values, footprints…";

const LIBRARIES: &[&str] = &[
    "All libraries",
    "Device",
    "Diode",
    "Transistor_BJT",
    "MCU_ST_STM32",
    "Regulator_Linear",
    "Connector_USB",
];

const PACKAGES: &[&str] = &[
    "All packages",
    "0402",
    "0603",
    "0805",
    "SOT-23",
    "SOIC-8",
    "LQFP-48",
];

const FOOTPRINTS: &[&str] = &[
    "0402", "0603", "0805", "1206", "SOT-23", "SOD-123", "SOIC-8", "LQFP-48",
];

/// One row of the library index. `&'static str` throughout — the mock
/// owns no data of its own beyond the inspector's editable copies.
struct Part {
    reference: &'static str,
    value: &'static str,
    mpn: &'static str,
    footprint: &'static str,
    library: &'static str,
    stock: u32,
    datasheet: &'static str,
    description: &'static str,
}

const PARTS: &[Part] = &[
    Part {
        reference: "C1",
        value: "100nF",
        mpn: "CL10B104KB8NNNC",
        footprint: "0603",
        library: "Device",
        stock: 5600,
        datasheet: "https://api.kemet.com/component-edge/download/specsheet/C0603C104K5RAC.pdf",
        description: "Ceramic capacitor, 100 nF ±10%, 50 V, X7R, 0603. Decoupling.",
    },
    Part {
        reference: "C2",
        value: "10uF",
        mpn: "GRM21BR61E106KA73L",
        footprint: "0805",
        library: "Device",
        stock: 740,
        datasheet: "https://www.murata.com/products/productdata/8796810838046/GRM21BR61E106KA73.pdf",
        description: "Ceramic capacitor, 10 µF ±10%, 25 V, X5R, 0805. Bulk rail decoupling.",
    },
    Part {
        reference: "C3",
        value: "22pF",
        mpn: "CL05C220JB5NNNC",
        footprint: "0402",
        library: "Device",
        stock: 1905,
        datasheet: "https://www.samsungsem.com/kr/support/product-search/mlcc/CL05C220JB5NNNC.jsp",
        description: "Ceramic capacitor, 22 pF ±5%, 50 V, C0G/NP0, 0402. Crystal load cap.",
    },
    Part {
        reference: "C4",
        value: "4.7uF",
        mpn: "GRM188R61A475KE15D",
        footprint: "0603",
        library: "Device",
        stock: 268,
        datasheet: "https://www.murata.com/products/productdata/8796828631070/GRM188R61A475KE15.pdf",
        description: "Ceramic capacitor, 4.7 µF ±10%, 10 V, X5R, 0603.",
    },
    Part {
        reference: "D1",
        value: "1N4148W",
        mpn: "1N4148W-7-F",
        footprint: "SOD-123",
        library: "Diode",
        stock: 640,
        datasheet: "https://www.diodes.com/assets/Datasheets/ds30086.pdf",
        description: "Small-signal switching diode, 100 V, 300 mA, SOD-123.",
    },
    Part {
        reference: "D2",
        value: "SS34",
        mpn: "SS34-E3/57T",
        footprint: "DO-214AC",
        library: "Diode",
        stock: 122,
        datasheet: "https://www.vishay.com/docs/88751/ss32.pdf",
        description: "Schottky rectifier, 40 V, 3 A, DO-214AC (SMA). Reverse-polarity guard.",
    },
    Part {
        reference: "J1",
        value: "USB-C 16P",
        mpn: "TYPE-C-31-M-12",
        footprint: "HRO_TYPE-C-31-M-12",
        library: "Connector_USB",
        stock: 310,
        datasheet: "https://datasheet.lcsc.com/lcsc/2201121800_Korean-Hroparts-Elec-TYPE-C-31-M-12.pdf",
        description: "USB Type-C receptacle, 16-pin, USB 2.0 only, through-hole shell tabs.",
    },
    Part {
        reference: "L1",
        value: "4.7uH",
        mpn: "SRN4018-4R7M",
        footprint: "1210",
        library: "Device",
        stock: 96,
        datasheet: "https://product.tdk.com/system/files/dataheet/inductor_commercial_power_vlf.pdf",
        description: "Shielded power inductor, 4.7 µH, 1.6 A sat, 1210. Buck output.",
    },
    Part {
        reference: "Q1",
        value: "BC847B",
        mpn: "BC847B,215",
        footprint: "SOT-23",
        library: "Transistor_BJT",
        stock: 980,
        datasheet: "https://www.nexperia.com/products/BC847B.pdf",
        description: "NPN general-purpose transistor, 45 V, 100 mA, hFE 200–450, SOT-23.",
    },
    Part {
        reference: "R1",
        value: "10k",
        mpn: "RC0603FR-0710KL",
        footprint: "0603",
        library: "Device",
        stock: 1420,
        datasheet: "https://www.yageo.com/upload/media/product/RC0603FR-0710KL.pdf",
        description: "Thick film chip resistor, 10 kΩ ±1%, 100 mW, 0603.\nYageo RC0603FR-0710KL. Standard pull-up value across the board.",
    },
    Part {
        reference: "R2",
        value: "4.7k",
        mpn: "RC0603FR-074K7L",
        footprint: "0603",
        library: "Device",
        stock: 2310,
        datasheet: "https://www.yageo.com/upload/media/product/RC0603FR-074K7L.pdf",
        description: "Thick film chip resistor, 4.7 kΩ ±1%, 100 mW, 0603.",
    },
    Part {
        reference: "R3",
        value: "1k",
        mpn: "RC0402FR-071KL",
        footprint: "0402",
        library: "Device",
        stock: 865,
        datasheet: "https://www.yageo.com/upload/media/product/RC0402FR-071KL.pdf",
        description: "Thick film chip resistor, 1 kΩ ±1%, 63 mW, 0402. LED series resistor.",
    },
    Part {
        reference: "R4",
        value: "100R",
        mpn: "RC0805FR-07100RL",
        footprint: "0805",
        library: "Device",
        stock: 412,
        datasheet: "https://www.yageo.com/upload/media/product/RC0805FR-07100RL.pdf",
        description: "Thick film chip resistor, 100 Ω ±1%, 125 mW, 0805.",
    },
    Part {
        reference: "R5",
        value: "22R",
        mpn: "RC0603FR-0722RL",
        footprint: "0603",
        library: "Device",
        stock: 1180,
        datasheet: "https://www.yageo.com/upload/media/product/RC0603FR-0722RL.pdf",
        description: "Thick film chip resistor, 22 Ω ±1%, 100 mW, 0603. USB series termination.",
    },
    Part {
        reference: "R6",
        value: "0R",
        mpn: "RC0603JR-070RL",
        footprint: "0603",
        library: "Device",
        stock: 3050,
        datasheet: "https://www.yageo.com/upload/media/product/RC0603JR-070RL.pdf",
        description: "Jumper resistor, 0 Ω, 1 A, 0603. Net-tie / option strap.",
    },
    Part {
        reference: "U1",
        value: "STM32F103C8T6",
        mpn: "STM32F103C8T6",
        footprint: "LQFP-48",
        library: "MCU_ST_STM32",
        stock: 36,
        datasheet: "https://www.st.com/resource/en/datasheet/stm32f103c8.pdf",
        description: "ARM Cortex-M3 MCU, 72 MHz, 64 KB flash, 20 KB SRAM, LQFP-48.",
    },
    Part {
        reference: "U2",
        value: "AMS1117-3.3",
        mpn: "AMS1117-3.3",
        footprint: "SOT-223",
        library: "Regulator_Linear",
        stock: 214,
        datasheet: "http://www.advanced-monolithic.com/pdf/ds1117.pdf",
        description: "LDO regulator, 3.3 V fixed, 1 A, 1.1 V dropout, SOT-223.",
    },
    Part {
        reference: "U3",
        value: "NE555",
        mpn: "NE555DR",
        footprint: "SOIC-8",
        library: "Timer",
        stock: 58,
        datasheet: "https://www.ti.com/lit/ds/symlink/ne555.pdf",
        description: "Precision timer, astable/monostable, 4.5–16 V, SOIC-8.",
    },
    Part {
        reference: "U4",
        value: "TPS62840",
        mpn: "TPS62840DLCR",
        footprint: "SON-6",
        library: "Regulator_Switching",
        stock: 0,
        datasheet: "https://www.ti.com/lit/ds/symlink/tps62840.pdf",
        description: "Step-down converter, 750 mA, 60 nA quiescent, 1.8–6.5 V in, SON-6.",
    },
    Part {
        reference: "Y1",
        value: "8MHz",
        mpn: "ABM8G-8.000MHZ-18-D2Y-T",
        footprint: "Crystal_SMD_3225",
        library: "Oscillator",
        stock: 84,
        datasheet: "https://abracon.com/Resonators/ABM8G.pdf",
        description: "Quartz crystal, 8.000 MHz, ±20 ppm, 18 pF load, 3.2 × 2.5 mm.",
    },
];

/// The row the mock opens on — a resistor, mid-table, so the selection
/// band reads as a selection rather than as a header.
const INITIAL_SELECTION: usize = 9;

struct Copperline {
    selected: usize,
    /// Toolbar state.
    search: String,
    library: String,
    library_open: bool,
    package: String,
    package_open: bool,
    /// Inspector state — an editable copy of the selected part, exactly
    /// as a real inspector holds a draft until Apply.
    value: String,
    footprint: String,
    footprint_open: bool,
    datasheet: String,
    description: String,
    /// The app-owned text selection every damascene text widget reads
    /// its caret and band from.
    selection: Selection,
}

impl Copperline {
    fn new() -> Self {
        let mut app = Self {
            selected: INITIAL_SELECTION,
            search: String::new(),
            library: "All libraries".into(),
            library_open: false,
            package: "All packages".into(),
            package_open: false,
            value: String::new(),
            footprint: String::new(),
            footprint_open: false,
            datasheet: String::new(),
            description: String::new(),
            selection: Selection::default(),
        };
        app.load_selected();
        app
    }

    fn part(&self) -> &'static Part {
        &PARTS[self.selected]
    }

    /// Refill the inspector draft from the selected row.
    fn load_selected(&mut self) {
        let p = self.part();
        self.value = p.value.to_string();
        self.footprint = p.footprint.to_string();
        self.datasheet = p.datasheet.to_string();
        self.description = p.description.to_string();
    }

    // ---- toolbar ----------------------------------------------------

    /// Search + two filter selects + the primary action, on the title
    /// bar's fill so the strip reads as chrome rather than as content.
    fn toolbar(&self) -> El {
        row([
            text("Copperline")
                .caption()
                .font_weight(FontWeight::Medium)
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            divider()
                .width(Size::Fixed(1.0))
                .height(Size::Fixed(18.0))
                .fill(vs::TITLE_BAR_BORDER),
            // The one flexible control in the strip: everything else is
            // Hug or Fixed, so the search field absorbs the window.
            text_input_with(
                SEARCH_KEY,
                &self.search,
                &self.selection,
                TextInputOpts::default().placeholder(SEARCH_PLACEHOLDER),
            )
            .width(Size::Fill(1.0)),
            select_trigger(LIBRARY_KEY, &self.library).width(Size::Fixed(150.0)),
            select_trigger(PACKAGE_KEY, &self.package).width(Size::Fixed(150.0)),
            button("Add Part").key("toolbar:add").primary(),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_2))
        .fill(vs::TITLE_BAR_ACTIVE_BG)
        .border_b()
        .border_color(vs::TITLE_BAR_BORDER)
        .height(Size::Fixed(TOOLBAR_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
    }

    // ---- parts table ------------------------------------------------

    fn parts_pane(&self) -> El {
        column([
            pane_header(
                "PARTS",
                [
                    text(format!("{} shown", PARTS.len()))
                        .caption()
                        .text_color(vs::DESCRIPTION_FG),
                    chip("Reference ▲"),
                ],
            ),
            column([table([
                table_header([table_row([
                    head("Reference", COL_REFERENCE),
                    head("Value", COL_VALUE),
                    head("Footprint", COL_FOOTPRINT),
                    head("Library", COL_LIBRARY),
                    head_right("Stock", COL_STOCK),
                ])]),
                table_body(
                    PARTS
                        .iter()
                        .enumerate()
                        .map(|(i, p)| self.part_row(i, p))
                        .collect::<Vec<El>>(),
                ),
            ])])
            .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .scrollable(),
        ])
        .fill(vs::EDITOR_BG)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn part_row(&self, index: usize, part: &Part) -> El {
        let selected = index == self.selected;
        let fg = if selected {
            vs::LIST_ACTIVE_SELECTION_FG
        } else {
            vs::FOREGROUND
        };
        let muted = if selected {
            vs::LIST_ACTIVE_SELECTION_FG
        } else {
            vs::DESCRIPTION_FG
        };

        let mut row = table_row([
            cell(text(part.reference).mono().text_color(fg), COL_REFERENCE),
            cell(text(part.value).mono().text_color(fg), COL_VALUE),
            cell(text(part.footprint).text_color(muted), COL_FOOTPRINT),
            cell(text(part.library).text_color(muted), COL_LIBRARY),
            cell(
                text(thousands(part.stock))
                    .mono()
                    .text_align(TextAlign::End)
                    .text_color(if selected {
                        vs::LIST_ACTIVE_SELECTION_FG
                    } else {
                        stock_color(part.stock)
                    }),
                COL_STOCK,
            ),
        ])
        // Keyed and pointer-cursored, but deliberately *not*
        // `.focusable()`: `table()` clips its content, so a focusable
        // row's focus ring is drawn outside the row rect and scissored
        // away — `lint` reports it as `FocusRingObscured` for all 20
        // rows. A keyboard-navigable table wants roving `ArrowNav` on an
        // unclipped list, which is past what a layout mock should model.
        .key(format!("row:{index}"))
        .cursor(Cursor::Pointer);

        if selected {
            row = row.fill(vs::LIST_ACTIVE_SELECTION_BG);
        }
        row
    }

    // ---- inspector --------------------------------------------------

    fn inspector(&self) -> El {
        let part = self.part();

        column([
            pane_header("PROPERTIES", [chip(part.reference)]),
            column([self.footprint_preview(), hairline(), self.property_form()])
                .gap(tokens::SPACE_3)
                .padding(tokens::SPACE_3)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch)
                .scrollable(),
            self.inspector_actions(),
        ])
        .fill(vs::SIDE_BAR_BG)
        .border_l()
        .border_color(vs::SIDE_BAR_BORDER)
        .width(Size::Fixed(INSPECTOR_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    /// Square placeholder where the pad-stack render would go. Sunk to
    /// `editor.background` so it reads as a well in the panel, not as a
    /// card floating on it.
    fn footprint_preview(&self) -> El {
        row([
            spacer(),
            column([
                spacer(),
                text(&self.footprint)
                    .mono()
                    .text_align(TextAlign::Center)
                    .width(Size::Fill(1.0)),
                text("footprint preview")
                    .caption()
                    .text_color(vs::DESCRIPTION_FG)
                    .text_align(TextAlign::Center)
                    .width(Size::Fill(1.0)),
                spacer(),
            ])
            .gap(tokens::SPACE_1)
            .fill(vs::EDITOR_BG)
            .stroke(vs::PANEL_BORDER)
            .radius(vs::RADIUS)
            .width(Size::Fixed(PREVIEW_SIDE))
            .height(Size::Fixed(PREVIEW_SIDE))
            .align(Align::Stretch),
            spacer(),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
    }

    fn property_form(&self) -> El {
        let part = self.part();

        column([
            field(
                "MPN",
                text(part.mpn)
                    .mono()
                    .caption()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                Align::Center,
            ),
            field(
                "Library",
                text(part.library)
                    .caption()
                    .text_color(vs::DESCRIPTION_FG)
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                Align::Center,
            ),
            hairline(),
            field(
                "Value",
                text_input(VALUE_KEY, &self.value, &self.selection),
                Align::Center,
            ),
            field(
                "Footprint",
                select_trigger(FOOTPRINT_KEY, &self.footprint),
                Align::Center,
            ),
            field(
                "Datasheet",
                text_input(DATASHEET_KEY, &self.datasheet, &self.selection),
                Align::Center,
            ),
            field(
                "Notes",
                text_area(DESCRIPTION_KEY, &self.description, &self.selection)
                    .height(Size::Fixed(116.0)),
                Align::Start,
            ),
        ])
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
    }

    fn inspector_actions(&self) -> El {
        row([
            text("Draft").caption().text_color(vs::DESCRIPTION_FG),
            spacer(),
            button("Revert").key("inspector:revert").secondary(),
            button("Apply").key("inspector:apply").primary(),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_2)
        .border_t()
        .border_color(vs::PANEL_BORDER)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
    }
}

// ---- column widths --------------------------------------------------
//
// `table_cell` hands every column `Fill(1.0)`; these are the weights the
// five columns actually want. Reference and Stock are narrow because
// their content is bounded ("R14", "5,600"); Library is widest because
// KiCad library names are long.
const COL_REFERENCE: f32 = 0.7;
const COL_VALUE: f32 = 1.3;
const COL_FOOTPRINT: f32 = 1.5;
const COL_LIBRARY: f32 = 1.4;
const COL_STOCK: f32 = 0.6;

/// A body cell at a given column weight.
///
/// No padding override: cell padding is on the `ComponentSize` ladder,
/// so this theme's `Xs` rung already stamps `xy(8, 4.71)` — the value
/// this helper used to hardcode as `xy(SPACE_2, 5.0)`.
fn cell(content: impl Into<El>, weight: f32) -> El {
    table_cell(content).width(Size::Fill(weight))
}

fn head(label: &str, weight: f32) -> El {
    table_head(label).width(Size::Fill(weight))
}

fn head_right(label: &str, weight: f32) -> El {
    table_head_el(text(label).text_align(TextAlign::End)).width(Size::Fill(weight))
}

/// A dense inspector row: fixed label gutter, control takes the rest.
///
/// Not `field_row` — that one is `[label, spacer, control]`, which sizes
/// for a right-aligned switch or button. An inspector wants its controls
/// left-aligned on a common gutter so the column of fields reads as a
/// table.
fn field(label: &str, control: impl Into<El>, align: Align) -> El {
    row([
        text(label)
            .caption()
            .text_color(vs::DESCRIPTION_FG)
            .ellipsis()
            .width(Size::Fixed(FIELD_LABEL_WIDTH)),
        control.into().width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
    .align(align)
}

/// Stock levels carry the only semantic color in the table: out of
/// stock is `errorForeground`, low is the theme's amber.
fn stock_color(stock: u32) -> Color {
    match stock {
        0 => vs::ERROR_FG,
        1..=99 => vs::CHAT_EDITED_FILE_FG,
        _ => vs::DESCRIPTION_FG,
    }
}

/// `5600` → `5,600`. Distributor stock figures are always grouped.
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl App for Copperline {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column root rather than `page()`: a workbench window is
        // full-bleed, and window padding would float the instrument on a
        // margin. `overlays` supplies the layer the open select menus
        // mount into.
        overlays(
            column([
                self.toolbar(),
                row([self.parts_pane(), self.inspector()])
                    .width(Size::Fill(1.0))
                    .height(Size::Fill(1.0))
                    .align(Align::Stretch),
                status_bar(
                    [
                        text(format!("{} parts · 1 selected", PARTS.len())).caption(),
                        text(format!("Filter: {} · {}", self.library, self.package))
                            .caption()
                            .text_color(vs::DESCRIPTION_FG),
                    ],
                    [
                        text("KiCad 8 · copperline.lib")
                            .caption()
                            .text_color(vs::DESCRIPTION_FG),
                        chip("Library synced · 2m ago"),
                    ],
                ),
            ])
            .fill(vs::EDITOR_BG)
            .align(Align::Stretch),
            [
                self.library_open.then(|| {
                    select::select_menu_selected(
                        LIBRARY_KEY,
                        LIBRARIES.iter().map(|l| (l.to_string(), *l)),
                        &self.library,
                    )
                }),
                self.package_open.then(|| {
                    select::select_menu_selected(
                        PACKAGE_KEY,
                        PACKAGES.iter().map(|p| (p.to_string(), *p)),
                        &self.package,
                    )
                }),
                self.footprint_open.then(|| {
                    select::select_menu_selected(
                        FOOTPRINT_KEY,
                        FOOTPRINTS.iter().map(|f| (f.to_string(), *f)),
                        &self.footprint,
                    )
                }),
            ],
        )
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if select::apply_event(
            &mut self.library,
            &mut self.library_open,
            &event,
            LIBRARY_KEY,
            Some,
        ) || select::apply_event(
            &mut self.package,
            &mut self.package_open,
            &event,
            PACKAGE_KEY,
            Some,
        ) || select::apply_event(
            &mut self.footprint,
            &mut self.footprint_open,
            &event,
            FOOTPRINT_KEY,
            Some,
        ) {
            return;
        }

        // Text widgets self-gate on route, so dispatching all of them
        // unconditionally is the documented idiom.
        if text_input::apply_event(&mut self.search, &mut self.selection, &event, SEARCH_KEY)
            || text_input::apply_event(&mut self.value, &mut self.selection, &event, VALUE_KEY)
            || text_input::apply_event(
                &mut self.datasheet,
                &mut self.selection,
                &event,
                DATASHEET_KEY,
            )
            || text_area::apply_event(
                &mut self.description,
                &mut self.selection,
                &event,
                DESCRIPTION_KEY,
            )
        {
            return;
        }

        if event.is_click_or_activate("inspector:revert") {
            self.load_selected();
            return;
        }

        for index in 0..PARTS.len() {
            if event.is_click_or_activate(&format!("row:{index}")) {
                self.selected = index;
                self.load_selected();
                return;
            }
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    // Without this, every stock control renders in shadcn zinc at 36px
    // and the layered surfaces collapse into one background.
    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Copperline", viewport, Copperline::new())
}
