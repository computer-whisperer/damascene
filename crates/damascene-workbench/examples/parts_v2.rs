//! Copperline — an ECAD parts-library browser, as a static layout study.
//!
//! The shape this genre converges on: a filter strip across the top, a
//! dense index table filling the work area, a properties inspector docked
//! right, and a status bar along the bottom. Everything but
//! [`damascene_workbench::chrome`]'s `status_bar` / `pane_header` /
//! `hairline` is a stock damascene widget — `table`, `input_group`,
//! `text_input`, `select_trigger`, `text_area`, `button` — rendered
//! through [`damascene_workbench::theme::theme`].
//!
//! **Static mock.** There is no `on_event`: the controls render in their
//! resting state and nothing responds. The selected row, the inspector's
//! draft values, and the filter labels are all derived from
//! [`SELECTED`] and the [`PARTS`] table, so the screen is internally
//! consistent without any state machine behind it.
//!
//! Comment out the `theme()` impl to see the same tree on stock shadcn —
//! the four inversions in `docs/WORKBENCH_VISION.md` all move at once.
//!
//! Run: `cargo run -p damascene-workbench --example parts_v2`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---------------------------------------------------------------------
// Metrics. Only the figures this layout actually chooses; everything
// else comes from the theme's dense rungs.
// ---------------------------------------------------------------------

/// Toolbar strip height — taller than [`vs::TITLE_BAR_HEIGHT`] because it
/// carries 28px controls plus the room their focus ring paints into.
const TOOLBAR_HEIGHT: f32 = 38.0;
/// Inspector width. VS Code's side bar rests near 256px; a properties
/// pane carrying a preview and a labelled form wants the extra gutter.
const INSPECTOR_WIDTH: f32 = 320.0;
/// The inspector's label gutter — sized for "Manufacturer".
const LABEL_GUTTER: f32 = 78.0;
/// Side of the square footprint preview.
const PREVIEW_SIDE: f32 = 200.0;

// Widget keys. Static mock, so these only have to be unique — nothing
// reads them back.
const SEARCH_KEY: &str = "toolbar:search";
const LIBRARY_KEY: &str = "toolbar:library";
const PACKAGE_KEY: &str = "toolbar:package";
const VALUE_KEY: &str = "inspector:value";
const FOOTPRINT_KEY: &str = "inspector:footprint";
const DATASHEET_KEY: &str = "inspector:datasheet";
const DESCRIPTION_KEY: &str = "inspector:description";

/// Bare copper, for the footprint preview's pads. Not a theme token:
/// this is *artwork* standing in for a pad-stack render, the way a
/// waveform or a 3D viewport would be, and it should not follow a
/// palette swap.
const COPPER: Color = Color::srgb_u8(184, 122, 58);
/// The chip body over the pads — a dark ceramic slab.
const CERAMIC: Color = Color::srgb_u8(58, 58, 62);

// ---------------------------------------------------------------------
// The library index.
// ---------------------------------------------------------------------

/// One row of the index, plus the fields the inspector reads.
struct Part {
    reference: &'static str,
    value: &'static str,
    footprint: &'static str,
    library: &'static str,
    stock: u32,
    mpn: &'static str,
    manufacturer: &'static str,
    datasheet: &'static str,
    description: &'static str,
}

const PARTS: &[Part] = &[
    Part {
        reference: "C1",
        value: "100nF",
        footprint: "0402",
        library: "Device",
        stock: 8420,
        mpn: "CL05B104KO5NNNC",
        manufacturer: "Samsung",
        datasheet: "https://www.samsungsem.com/mlcc/CL05B104KO5NNNC",
        description: "Ceramic capacitor, 100 nF ±10%, 16 V, X7R, 0402.\nRail decoupling — one per supply pin.",
    },
    Part {
        reference: "C2",
        value: "22uF",
        footprint: "0805",
        library: "Device",
        stock: 613,
        mpn: "GRM21BR61A226ME51L",
        manufacturer: "Murata",
        datasheet: "https://www.murata.com/products/productdetail?partno=GRM21BR61A226ME51",
        description: "Ceramic capacitor, 22 µF ±20%, 10 V, X5R, 0805. Buck output bulk.",
    },
    Part {
        reference: "C3",
        value: "18pF",
        footprint: "0402",
        library: "Device",
        stock: 2140,
        mpn: "CL05C180JB5NNNC",
        manufacturer: "Samsung",
        datasheet: "https://www.samsungsem.com/mlcc/CL05C180JB5NNNC",
        description: "Ceramic capacitor, 18 pF ±5%, 50 V, C0G. Crystal load pair with C4.",
    },
    Part {
        reference: "D1",
        value: "1N4148W",
        footprint: "SOD-123",
        library: "Diode",
        stock: 1560,
        mpn: "1N4148W-7-F",
        manufacturer: "Diodes Inc.",
        datasheet: "https://www.diodes.com/assets/Datasheets/ds30086.pdf",
        description: "Small-signal switching diode, 100 V, 300 mA, 4 ns recovery.",
    },
    Part {
        reference: "D2",
        value: "PMEG3020",
        footprint: "SOD-123",
        library: "Diode",
        stock: 74,
        mpn: "PMEG3020EP,115",
        manufacturer: "Nexperia",
        datasheet: "https://assets.nexperia.com/documents/data-sheet/PMEG3020EP.pdf",
        description: "Schottky rectifier, 30 V, 2 A. Reverse-polarity guard on VIN.",
    },
    Part {
        reference: "J1",
        value: "USB-C 16P",
        footprint: "TYPE-C-31-M-12",
        library: "Connector_USB",
        stock: 288,
        mpn: "TYPE-C-31-M-12",
        manufacturer: "Korean Hroparts",
        datasheet: "https://datasheet.lcsc.com/lcsc/2201121800_TYPE-C-31-M-12.pdf",
        description: "USB Type-C receptacle, 16-pin, USB 2.0 only, SMT with through-hole shell tabs.",
    },
    Part {
        reference: "L1",
        value: "2.2uH",
        footprint: "1210",
        library: "Device",
        stock: 132,
        mpn: "SRN4018-2R2M",
        manufacturer: "Bourns",
        datasheet: "https://www.bourns.com/docs/product-datasheets/SRN4018.pdf",
        description: "Shielded power inductor, 2.2 µH, 2.1 A saturation, 60 mΩ DCR.",
    },
    Part {
        reference: "Q1",
        value: "BC847B",
        footprint: "SOT-23",
        library: "Transistor_BJT",
        stock: 940,
        mpn: "BC847B,215",
        manufacturer: "Nexperia",
        datasheet: "https://assets.nexperia.com/documents/data-sheet/BC847_SER.pdf",
        description: "NPN general-purpose transistor, 45 V, 100 mA, hFE 200–450.",
    },
    Part {
        reference: "Q2",
        value: "AO3401A",
        footprint: "SOT-23",
        library: "Transistor_FET",
        stock: 0,
        mpn: "AO3401A",
        manufacturer: "Alpha & Omega",
        datasheet: "https://aosmd.com/res/data_sheets/AO3401A.pdf",
        description: "P-channel MOSFET, −30 V, −4 A, 60 mΩ. High-side load switch.",
    },
    Part {
        reference: "R1",
        value: "10k",
        footprint: "0603",
        library: "Device",
        stock: 3105,
        mpn: "RC0603FR-0710KL",
        manufacturer: "Yageo",
        datasheet: "https://www.yageo.com/upload/media/product/RC0603FR-0710KL.pdf",
        description: "Thick-film chip resistor, 10 kΩ ±1%, 100 mW, 75 V.\nHouse standard pull-up; used on every I²C net on this board.",
    },
    Part {
        reference: "R2",
        value: "4.7k",
        footprint: "0603",
        library: "Device",
        stock: 1880,
        mpn: "RC0603FR-074K7L",
        manufacturer: "Yageo",
        datasheet: "https://www.yageo.com/upload/media/product/RC0603FR-074K7L.pdf",
        description: "Thick-film chip resistor, 4.7 kΩ ±1%, 100 mW.",
    },
    Part {
        reference: "R3",
        value: "5.1k",
        footprint: "0402",
        library: "Device",
        stock: 620,
        mpn: "RC0402FR-075K1L",
        manufacturer: "Yageo",
        datasheet: "https://www.yageo.com/upload/media/product/RC0402FR-075K1L.pdf",
        description: "Thick-film chip resistor, 5.1 kΩ ±1%, 63 mW. USB-C CC pulldown.",
    },
    Part {
        reference: "R4",
        value: "22R",
        footprint: "0402",
        library: "Device",
        stock: 1440,
        mpn: "RC0402FR-0722RL",
        manufacturer: "Yageo",
        datasheet: "https://www.yageo.com/upload/media/product/RC0402FR-0722RL.pdf",
        description: "Thick-film chip resistor, 22 Ω ±1%, 63 mW. USB D± series termination.",
    },
    Part {
        reference: "R5",
        value: "0R",
        footprint: "0603",
        library: "Device",
        stock: 4200,
        mpn: "RC0603JR-070RL",
        manufacturer: "Yageo",
        datasheet: "https://www.yageo.com/upload/media/product/RC0603JR-070RL.pdf",
        description: "Jumper resistor, 0 Ω, 1 A. Net tie / build-option strap.",
    },
    Part {
        reference: "R6",
        value: "1M",
        footprint: "0603",
        library: "Device",
        stock: 96,
        mpn: "RC0603FR-071ML",
        manufacturer: "Yageo",
        datasheet: "https://www.yageo.com/upload/media/product/RC0603FR-071ML.pdf",
        description: "Thick-film chip resistor, 1 MΩ ±1%, 100 mW. Bleeder across C2.",
    },
    Part {
        reference: "U1",
        value: "STM32G071CB",
        footprint: "LQFP-48",
        library: "MCU_ST_STM32",
        stock: 41,
        mpn: "STM32G071CBT6",
        manufacturer: "STMicro",
        datasheet: "https://www.st.com/resource/en/datasheet/stm32g071cb.pdf",
        description: "Cortex-M0+ MCU, 64 MHz, 128 KB flash, 36 KB SRAM, USB-PD peripheral.",
    },
    Part {
        reference: "U2",
        value: "TPS62840",
        footprint: "SON-6",
        library: "Regulator_Switching",
        stock: 18,
        mpn: "TPS62840DLCR",
        manufacturer: "Texas Instruments",
        datasheet: "https://www.ti.com/lit/ds/symlink/tps62840.pdf",
        description: "Step-down converter, 750 mA, 60 nA quiescent, 1.8–6.5 V in.",
    },
    Part {
        reference: "U3",
        value: "SN74LVC1G17",
        footprint: "SOT-23-5",
        library: "Logic_LevelTranslator",
        stock: 505,
        mpn: "SN74LVC1G17DBVR",
        manufacturer: "Texas Instruments",
        datasheet: "https://www.ti.com/lit/ds/symlink/sn74lvc1g17.pdf",
        description: "Single Schmitt-trigger buffer, 1.65–5.5 V. Debounces the enable line.",
    },
    Part {
        reference: "U4",
        value: "24LC256",
        footprint: "SOIC-8",
        library: "Memory_EEPROM",
        stock: 62,
        mpn: "24LC256-I/SN",
        manufacturer: "Microchip",
        datasheet: "https://ww1.microchip.com/downloads/en/DeviceDoc/24AA256-24LC256.pdf",
        description: "I²C EEPROM, 256 Kbit, 400 kHz, 1.7–5.5 V.",
    },
    Part {
        reference: "Y1",
        value: "16MHz",
        footprint: "Crystal_3225",
        library: "Oscillator",
        stock: 129,
        mpn: "ABM8G-16.000MHZ-18-D2Y-T",
        manufacturer: "Abracon",
        datasheet: "https://abracon.com/Resonators/ABM8G.pdf",
        description: "Quartz crystal, 16.000 MHz, ±20 ppm, 18 pF load, 3.2 × 2.5 mm.",
    },
];

/// The row the mock opens on. Mid-table on purpose: a selection band at
/// the top edge reads as a second header rather than as a selection.
const SELECTED: usize = 9;

/// The whole library, of which [`PARTS`] is the filtered page — the
/// figure the status bar reports.
const LIBRARY_TOTAL: usize = 412;

// ---------------------------------------------------------------------
// Column weights.
//
// `table_cell` hands every column `Fill(1.0)`. These are the weights the
// five columns actually want: Reference and Stock are bounded content
// ("R14", "8,420"), Library is widest because KiCad library names run
// long.
// ---------------------------------------------------------------------

const COL_REFERENCE: f32 = 0.6;
const COL_VALUE: f32 = 1.1;
const COL_FOOTPRINT: f32 = 1.3;
const COL_LIBRARY: f32 = 1.4;
const COL_STOCK: f32 = 0.6;

struct Copperline {
    /// The app-owned caret/band state every damascene text widget reads
    /// from. Static here — no `on_event` writes it — but the text
    /// widgets still need something to read.
    selection: Selection,
    /// The inspector's draft copy of the selected part, exactly as a real
    /// inspector holds edits until Apply.
    value: String,
    footprint: String,
    datasheet: String,
    description: String,
}

impl Copperline {
    fn new() -> Self {
        let part = &PARTS[SELECTED];
        Self {
            selection: Selection::default(),
            value: part.value.to_string(),
            footprint: part.footprint.to_string(),
            datasheet: part.datasheet.to_string(),
            description: part.description.to_string(),
        }
    }

    fn part(&self) -> &'static Part {
        &PARTS[SELECTED]
    }

    // ---- toolbar ----------------------------------------------------

    /// Search, two filter selects, and the primary action, on the title
    /// bar's fill so the strip reads as chrome rather than as content.
    ///
    /// Sizing is the whole trick: every control is `Hug` or `Fixed`
    /// except the search group, which is `Fill(1.0)` and therefore
    /// absorbs the entire window width.
    fn toolbar(&self) -> El {
        row([
            text("Copperline")
                .caption()
                .font_weight(FontWeight::Medium)
                .text_color(vs::TITLE_BAR_ACTIVE_FG)
                .width(Size::Hug),
            rule_v(18.0),
            self.search_field(),
            select_trigger(LIBRARY_KEY, "All libraries").width(Size::Fixed(150.0)),
            select_trigger(PACKAGE_KEY, "All packages").width(Size::Fixed(140.0)),
            rule_v(18.0),
            button_with_icon("plus", "Add Part")
                .key("toolbar:add")
                .primary()
                .width(Size::Hug),
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

    /// The search field as an `input_group`: the group is the trough,
    /// the `text_input` inside it is de-chromed, and the two cells are
    /// the leading magnifier and the trailing result count.
    fn search_field(&self) -> El {
        input_group([
            input_group_addon(icon("search")),
            text_input_with(
                SEARCH_KEY,
                "",
                &self.selection,
                TextInputOpts::default().placeholder("Search reference, value, MPN, footprint…"),
            ),
            input_group_text(format!("{} / {LIBRARY_TOTAL}", PARTS.len())),
        ])
        .width(Size::Fill(1.0))
    }

    // ---- parts table -------------------------------------------------

    fn parts_pane(&self) -> El {
        column([
            pane_header(
                "PARTS",
                [
                    text("sorted by Reference")
                        .caption()
                        .text_color(vs::DESCRIPTION_FG),
                    chip("Filtered"),
                ],
            ),
            column([table([
                table_header([table_row([
                    // The sorted column carries its caret; the rest are
                    // plain labels.
                    head_el(
                        row([
                            text("Reference"),
                            icon("chevron-up")
                                .icon_size(12.0)
                                .text_color(vs::DESCRIPTION_FG),
                        ])
                        .gap(tokens::SPACE_1)
                        .align(Align::Center),
                        COL_REFERENCE,
                    ),
                    head("Value", COL_VALUE),
                    head("Footprint", COL_FOOTPRINT),
                    head("Library", COL_LIBRARY),
                    head_el(
                        text("Stock")
                            .text_align(TextAlign::End)
                            .width(Size::Fill(1.0)),
                        COL_STOCK,
                    ),
                ])]),
                table_body(
                    PARTS
                        .iter()
                        .enumerate()
                        .map(|(i, p)| part_row(i, p))
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

    // ---- inspector ---------------------------------------------------

    fn inspector(&self) -> El {
        column([
            pane_header("PROPERTIES", [chip(self.part().reference)]),
            column([
                self.footprint_preview(),
                self.identity_block(),
                hairline(),
                self.property_form(),
            ])
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

    /// The square well the pad-stack render would land in. Sunk to
    /// `editor.background` so it reads as a recess in the panel rather
    /// than as a card floating on it, and stocked with a schematic
    /// two-pad chip artwork so the square is not simply empty.
    fn footprint_preview(&self) -> El {
        let pad = || {
            divider()
                .width(Size::Fixed(22.0))
                .height(Size::Fixed(46.0))
                .radius(1.5)
                .fill(COPPER)
        };
        // Pads first, body straddling them — a chip resistor seen from
        // above, at roughly the 1.6 × 0.8 mm proportion of an 0603.
        let land_pattern = row([pad(), body(), pad()])
            .gap(0.0)
            .width(Size::Hug)
            .height(Size::Hug)
            .align(Align::Center);

        let courtyard = column([land_pattern])
            .padding(tokens::SPACE_3)
            .stroke(vs::DESCRIPTION_FG)
            .radius(vs::RADIUS)
            .width(Size::Hug)
            .height(Size::Hug)
            .align(Align::Center)
            .justify(Justify::Center);

        row([
            spacer(),
            column([
                spacer(),
                row([spacer(), courtyard, spacer()])
                    .width(Size::Fill(1.0))
                    .height(Size::Hug)
                    .align(Align::Center),
                spacer(),
                text(format!("{} · 2 pads · 1.60 × 0.80 mm", self.footprint))
                    .caption()
                    .mono()
                    .text_color(vs::DESCRIPTION_FG)
                    .text_align(TextAlign::Center)
                    .width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_2)
            .padding(tokens::SPACE_2)
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

    /// The read-only half of the inspector: what the library says about
    /// this part, above the rule that separates it from what the user
    /// may edit.
    fn identity_block(&self) -> El {
        let part = self.part();
        column([
            prop("MPN", text(part.mpn).caption().mono().ellipsis()),
            prop("Manufacturer", text(part.manufacturer).caption().ellipsis()),
            prop(
                "Library",
                text(part.library)
                    .caption()
                    .mono()
                    .text_color(vs::DESCRIPTION_FG)
                    .ellipsis(),
            ),
            prop(
                "Stock",
                row([
                    text(thousands(part.stock))
                        .caption()
                        .mono()
                        .text_color(stock_color(part.stock))
                        .width(Size::Hug),
                    text("on hand · Warehouse B")
                        .caption()
                        .text_color(vs::DESCRIPTION_FG)
                        .ellipsis(),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Center),
            ),
        ])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
    }

    fn property_form(&self) -> El {
        column([
            prop("Value", text_input(VALUE_KEY, &self.value, &self.selection)),
            prop("Footprint", select_trigger(FOOTPRINT_KEY, &self.footprint)),
            prop(
                "Datasheet",
                text_input_with(
                    DATASHEET_KEY,
                    &self.datasheet,
                    &self.selection,
                    TextInputOpts::default().placeholder("https://"),
                ),
            ),
            prop_top(
                "Description",
                text_area_with(
                    DESCRIPTION_KEY,
                    &self.description,
                    &self.selection,
                    TextAreaOpts::default().placeholder("Notes for this library entry"),
                )
                .height(Size::Fixed(104.0)),
            ),
        ])
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
    }

    /// The panel's action rail — pinned to the bottom of the inspector,
    /// outside the scroll region, so Apply never scrolls away.
    fn inspector_actions(&self) -> El {
        row([
            text("2 unsaved edits")
                .caption()
                .text_color(vs::CHAT_EDITED_FILE_FG)
                .ellipsis(),
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

// ---------------------------------------------------------------------
// Row and cell recipes.
// ---------------------------------------------------------------------

fn part_row(index: usize, part: &Part) -> El {
    let selected = index == SELECTED;
    // Under a selection band every column collapses onto one foreground:
    // the band is the emphasis, so the muted/normal contrast inside it
    // would only fight the band.
    let strong = if selected {
        vs::LIST_ACTIVE_SELECTION_FG
    } else {
        vs::FOREGROUND
    };
    let muted = if selected {
        vs::LIST_ACTIVE_SELECTION_FG
    } else {
        vs::DESCRIPTION_FG
    };
    let stock = if selected {
        vs::LIST_ACTIVE_SELECTION_FG
    } else {
        stock_color(part.stock)
    };

    let row = table_row([
        cell(
            text(part.reference)
                .mono()
                .font_weight(FontWeight::Medium)
                .text_color(strong),
            COL_REFERENCE,
        ),
        cell(text(part.value).mono().text_color(strong), COL_VALUE),
        cell(text(part.footprint).text_color(muted), COL_FOOTPRINT),
        cell(text(part.library).mono().text_color(muted), COL_LIBRARY),
        cell(
            text(thousands(part.stock))
                .mono()
                .text_align(TextAlign::End)
                .text_color(stock),
            COL_STOCK,
        ),
    ])
    // Keyed and pointer-cursored so the rows read as targets, but
    // deliberately *not* `.focusable()`: `table()` clips its content, so
    // a focusable row's focus ring paints outside the row rect and is
    // scissored away — `lint` reports `FocusRingObscured` for every row.
    // A keyboard-navigable table wants roving `ArrowNav` on an unclipped
    // list, which is more machinery than a layout study should carry.
    .key(format!("row:{index}"))
    .cursor(Cursor::Pointer)
    .align(Align::Center);

    if selected {
        row.fill(vs::LIST_ACTIVE_SELECTION_BG)
    } else {
        row
    }
}

/// A body cell at a column weight.
///
/// No padding override: cell padding rides the `ComponentSize` ladder,
/// and this theme's `Xs` rung stamps `xy(8, 4.71)` — what this helper
/// used to hardcode as `xy(SPACE_2, ROW_PAD_Y)`.
fn cell(content: impl Into<El>, weight: f32) -> El {
    table_cell(content).width(Size::Fill(weight))
}

fn head(label: &str, weight: f32) -> El {
    table_head(label).width(Size::Fill(weight))
}

fn head_el(content: impl Into<El>, weight: f32) -> El {
    table_head_el(content).width(Size::Fill(weight))
}

/// One inspector row: fixed label gutter, control takes the rest.
///
/// Not the stock `field()` (label stacked above its control, shadcn's
/// form rhythm) and not `field_row()` (`[label, spacer, control]`, sized
/// for a right-aligned switch). An inspector wants its controls
/// left-aligned on a shared gutter, so the column of properties reads as
/// a two-column table.
fn prop(label: &str, control: impl Into<El>) -> El {
    prop_aligned(label, control, Align::Center)
}

/// [`prop`] for a control taller than one rung — the label sits at the
/// control's first line rather than at its middle.
fn prop_top(label: &str, control: impl Into<El>) -> El {
    prop_aligned(label, control, Align::Start)
}

fn prop_aligned(label: &str, control: impl Into<El>, align: Align) -> El {
    row([
        text(label)
            .caption()
            .text_color(vs::DESCRIPTION_FG)
            .ellipsis()
            .width(Size::Fixed(LABEL_GUTTER)),
        control.into().width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
    .align(align)
}

/// A vertical hairline for splitting toolbar groups — the vertical
/// counterpart of [`hairline`], which core has no name for.
fn rule_v(height: f32) -> El {
    divider()
        .width(Size::Fixed(vs::HAIRLINE))
        .height(Size::Fixed(height))
        .fill(vs::TITLE_BAR_BORDER)
}

/// The ceramic slab of the footprint preview's chip artwork.
fn body() -> El {
    divider()
        .width(Size::Fixed(40.0))
        .height(Size::Fixed(30.0))
        .radius(1.0)
        .fill(CERAMIC)
}

/// Stock levels carry the only semantic color in the table: nothing on
/// hand is `errorForeground`, a short shelf is the theme's amber.
fn stock_color(stock: u32) -> Color {
    match stock {
        0 => vs::ERROR_FG,
        1..=99 => vs::CHAT_EDITED_FILE_FG,
        _ => vs::DESCRIPTION_FG,
    }
}

/// `8420` → `8,420`. Distributor stock figures are always grouped.
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
        // full-bleed by definition, and window padding would float the
        // whole instrument on a margin — signal 4 of the diagnosis this
        // crate exists to invert.
        column([
            self.toolbar(),
            row([self.parts_pane(), self.inspector()])
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    text(format!("{LIBRARY_TOTAL} parts · 1 selected")).caption(),
                    text("copperline.kicad_sym")
                        .caption()
                        .text_color(vs::DESCRIPTION_FG),
                ],
                [
                    text("KiCad 8.0").caption().text_color(vs::DESCRIPTION_FG),
                    // The sync chip: the one saturated pixel in the bar,
                    // on `statusBarItem.remoteBackground` — the slot VS
                    // Code reserves for exactly this "connected to
                    // something" readout.
                    chip("Library synced · 4m ago")
                        .fill(vs::STATUS_BAR_ITEM_REMOTE_BG)
                        .text_color(vs::STATUS_BAR_ITEM_REMOTE_FG),
                ],
            ),
        ])
        .fill(vs::EDITOR_BG)
        .align(Align::Stretch)
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
