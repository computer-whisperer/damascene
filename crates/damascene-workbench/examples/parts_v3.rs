//! **Copperline** — an ECAD parts-library browser, as a static mock.
//!
//! Three regions, top to bottom: a filter toolbar, a
//! table / inspector split, and a status bar. Everything is a stock
//! `damascene_core` widget — `table`, `input_group`, `select_trigger`,
//! `field_with`, `text_area`, `button` — plus the four
//! [`damascene_workbench::chrome`] recipes, rendered through
//! [`damascene_workbench::theme::theme`].
//!
//! Nothing here is wired: there is no `on_event`, the selection is
//! frozen on the first row, and the controls hold literal strings. The
//! point is the *shape* of a dense tool at 1280×800.
//!
//! Run: `cargo run -p damascene-workbench --example parts_v3`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---------------------------------------------------------------------
// Geometry.
// ---------------------------------------------------------------------

/// The inspector rail. VS Code's own side bar sits around this width and
/// it is what a two-column property sheet needs at 13px type.
const INSPECTOR_WIDTH: f32 = 320.0;

/// The footprint preview is a square, so its side has to be spelled out:
/// the rail's width less its left border and the body's two paddings.
/// Core has no aspect-ratio size (`Size::Fill(1.0)` on both axes gives a
/// rectangle, not a square), so the arithmetic is the caller's.
const PREVIEW_SIDE: f32 = INSPECTOR_WIDTH - vs::HAIRLINE - 2.0 * tokens::SPACE_3;

/// Toolbar strip height — one rung above the 28px `Xs` controls it
/// holds, so they sit centered with 6px of air either side.
const TOOLBAR_HEIGHT: f32 = 40.0;

/// The inspector's label gutter, shared by every `inline_label` row so
/// the controls land on one edge.
const LABEL_GUTTER: f32 = 78.0;

/// The table's column geometry, read by the header and every body row.
const COLS: &[TableColumn] = &[
    TableColumn::fixed(168.0),           // Reference — MPNs, mono
    TableColumn::fill(1.2),              // Value
    TableColumn::fixed(120.0),           // Footprint — mono
    TableColumn::fill(1.0),              // Library
    TableColumn::fixed(84.0).align_end(), // Stock — numeric
];

// ---------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------

/// One library row. `low` flags a stock figure worth painting in the
/// error color rather than the body color.
struct Part {
    reference: &'static str,
    value: &'static str,
    footprint: &'static str,
    library: &'static str,
    stock: &'static str,
    low: bool,
}

const PARTS: &[Part] = &[
    Part { reference: "RC0603FR-0710KL",    value: "10 kΩ ±1% 1/10 W",      footprint: "0603",     library: "Passives / Resistors",  stock: "1,240", low: false },
    Part { reference: "RC0603FR-07100RL",   value: "100 Ω ±1% 1/10 W",      footprint: "0603",     library: "Passives / Resistors",  stock: "863",   low: false },
    Part { reference: "RC0402FR-074K7L",    value: "4.7 kΩ ±1% 1/16 W",     footprint: "0402",     library: "Passives / Resistors",  stock: "3,105", low: false },
    Part { reference: "ERJ-3EKF1002V",      value: "10 kΩ ±1% 1/10 W",      footprint: "0603",     library: "Passives / Resistors",  stock: "512",   low: false },
    Part { reference: "CRCW080510K0FKEA",   value: "10 kΩ ±1% 1/8 W",       footprint: "0805",     library: "Passives / Resistors",  stock: "74",    low: false },
    Part { reference: "GRM188R71H104KA93D", value: "100 nF 50 V X7R",       footprint: "0603",     library: "Passives / Capacitors", stock: "4,820", low: false },
    Part { reference: "C0805C106K8PACTU",   value: "10 µF 10 V X5R",        footprint: "0805",     library: "Passives / Capacitors", stock: "690",   low: false },
    Part { reference: "CL10A226MQ8NRNC",    value: "22 µF 6.3 V X5R",       footprint: "0603",     library: "Passives / Capacitors", stock: "1,156", low: false },
    Part { reference: "UWT1V101MCL1GS",     value: "100 µF 35 V ±20%",      footprint: "Radial-6.3", library: "Passives / Capacitors", stock: "38",  low: false },
    Part { reference: "LM358DR",            value: "Dual op-amp, 1 MHz",    footprint: "SOIC-8",   library: "Analog / Amplifiers",   stock: "214",   low: false },
    Part { reference: "NE555DR",            value: "Precision timer",       footprint: "SOIC-8",   library: "Analog / Timers",       stock: "96",    low: false },
    Part { reference: "STM32F103C8T6",      value: "Cortex-M3, 72 MHz",     footprint: "LQFP-48",  library: "MCU / ST",              stock: "41",    low: false },
    Part { reference: "ATMEGA328P-AU",      value: "AVR 8-bit, 32 kB",      footprint: "TQFP-32",  library: "MCU / Microchip",       stock: "27",    low: false },
    Part { reference: "AMS1117-3.3",        value: "LDO 3.3 V, 1 A",        footprint: "SOT-223",  library: "Power / Regulators",    stock: "305",   low: false },
    Part { reference: "MCP1700T-3302E/TT",  value: "LDO 3.3 V, 250 mA",     footprint: "SOT-23",   library: "Power / Regulators",    stock: "148",   low: false },
    Part { reference: "BC847B",             value: "NPN 45 V, 100 mA",      footprint: "SOT-23",   library: "Discrete / Transistors", stock: "2,600", low: false },
    Part { reference: "SS14",               value: "Schottky 40 V, 1 A",    footprint: "SMA",      library: "Discrete / Diodes",     stock: "780",   low: false },
    Part { reference: "1N4148WS",           value: "Switching, 75 V",       footprint: "SOD-323",  library: "Discrete / Diodes",     stock: "3,410", low: false },
    Part { reference: "SN74LVC1G14DBVR",    value: "Schmitt inverter",      footprint: "SOT-23-5", library: "Logic / 74LVC",         stock: "0",     low: true  },
];

/// The row the inspector is showing. Row 0 is the 10 kΩ 0603 resistor,
/// so the pane's values and the preview's pad geometry agree with it.
const SELECTED: usize = 0;

const DESCRIPTION: &str = "Thick film chip resistor, 1/10 W, ±1%.\n\
                           Yageo RC-L series, ±100 ppm/°C.\n\
                           AEC-Q200 qualified, RoHS compliant.";

struct Copperline {
    /// Owned by the app because `text_input` / `text_area` read the
    /// caret out of it. A static mock never writes to it.
    selection: Selection,
}

impl Copperline {
    fn new() -> Self {
        Self {
            selection: Selection::default(),
        }
    }

    // -----------------------------------------------------------------
    // Toolbar.
    // -----------------------------------------------------------------

    /// The filter strip: identity, a `Fill`-width search field, the two
    /// scope selects, and the one primary action.
    ///
    /// A plain `row` rather than core's `toolbar()` — that widget is
    /// shadcn's boxed toolbar (hugging width, `RADIUS_MD`, its own
    /// stroke), and this is a full-bleed strip flush to the window edge,
    /// the same argument `chrome::title_bar` makes against
    /// `menubar()`.
    fn toolbar(&self) -> El {
        row([
            text("Copperline")
                .caption()
                .font_weight(FontWeight::Medium)
                .text_color(vs::TITLE_BAR_ACTIVE_FG),
            vertical_hairline().height(Size::Fixed(18.0)),
            input_group([
                input_group_addon(icon("search").icon_size(tokens::ICON_XS)),
                text_input_with(
                    "search",
                    "",
                    &self.selection,
                    TextInputOpts::default()
                        .placeholder("Search reference, value, footprint…"),
                ),
            ])
            .width(Size::Fill(1.0)),
            vertical_hairline().height(Size::Fixed(18.0)),
            filter("Library", "filter:library", "All libraries", 156.0),
            filter("Package", "filter:package", "Any package", 146.0),
            button("Add Part").key("add-part").primary(),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::x(tokens::SPACE_2))
        .height(Size::Fixed(TOOLBAR_HEIGHT))
        .width(Size::Fill(1.0))
        .align(Align::Center)
        .fill(vs::SIDE_BAR_BG)
        .border_b()
        .border_color(vs::PANEL_BORDER)
    }

    // -----------------------------------------------------------------
    // Table.
    // -----------------------------------------------------------------

    /// The library table. One `TableColumn` array feeds the header and
    /// every body row, so the geometry is stated once.
    fn parts_table(&self) -> El {
        let rows: Vec<El> = PARTS
            .iter()
            .enumerate()
            .map(|(i, p)| part_row(p, i == SELECTED))
            .collect();

        column([table([
            table_header([table_header_cells(
                COLS,
                ["Reference", "Value", "Footprint", "Library", "Stock"],
            )]),
            table_body(rows),
        ])])
        .padding(0.0)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG)
        .scrollable()
    }

    // -----------------------------------------------------------------
    // Inspector.
    // -----------------------------------------------------------------

    /// The properties rail: pane header, footprint preview, the
    /// identity line, the property sheet, and the commit bar pinned to
    /// the bottom by a `spacer()`.
    fn inspector(&self) -> El {
        let part = &PARTS[SELECTED];

        column([
            pane_header(
                "PROPERTIES",
                [
                    icon_button("eye").ghost().size(ComponentSize::Xxs),
                    icon_button("more-horizontal")
                        .ghost()
                        .size(ComponentSize::Xxs),
                ],
            ),
            column([
                footprint_preview(),
                row([
                    text(part.reference).mono().label(),
                    spacer(),
                    chip("In stock").success(),
                ])
                .width(Size::Fill(1.0))
                .align(Align::Center),
                self.property_sheet(part),
                // Pushes the commit bar to the pane's bottom edge.
                spacer(),
                hairline(),
                row([
                    spacer(),
                    button("Revert").key("revert").secondary(),
                    button("Apply").key("apply").primary(),
                ])
                .gap(tokens::SPACE_2)
                .width(Size::Fill(1.0))
                .align(Align::Center),
            ])
            .gap(tokens::SPACE_3)
            .padding(tokens::SPACE_3)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
        ])
        .width(Size::Fixed(INSPECTOR_WIDTH))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .fill(vs::SIDE_BAR_BG)
        .border_l()
        .border_color(vs::PANEL_BORDER)
    }

    /// The key/value form. Three `inline_label` rows — core's inspector
    /// shape, a fixed label gutter with the control taking the rest —
    /// and one vertical field for the multiline description, which
    /// would be unreadable squeezed beside a gutter.
    fn property_sheet(&self, part: &Part) -> El {
        let inspector_row = FieldOpts::default().inline_label(LABEL_GUTTER);

        column([
            field_with(
                "Value",
                text_input_with("prop:value", part.value, &self.selection, TextInputOpts::default()),
                inspector_row,
            ),
            field_with(
                "Footprint",
                select_trigger("prop:footprint", "0603 (1608 metric)"),
                inspector_row,
            ),
            field_with(
                "Datasheet",
                text_input_with(
                    "prop:datasheet",
                    "https://www.yageo.com/rc-l.pdf",
                    &self.selection,
                    TextInputOpts::default().placeholder("https://"),
                ),
                inspector_row,
            ),
            field(
                "Description",
                text_area_with(
                    "prop:description",
                    DESCRIPTION,
                    &self.selection,
                    TextAreaOpts::default(),
                )
                // Sized to the three lines it holds: `text_area`
                // defaults to a Hug height that grows with the value,
                // which a fixed inspector pane does not want.
                .height(Size::Fixed(84.0)),
            ),
        ])
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
    }
}

// ---------------------------------------------------------------------
// Leaf recipes.
// ---------------------------------------------------------------------

/// A labelled scope select for the toolbar. The label is a caption
/// rather than a `field`, because a strip has no room for the field
/// anatomy's stacked label.
fn filter(label: &str, key: &str, value: &str, width: f32) -> El {
    row([
        text(label).caption().muted(),
        select_trigger(key, value).width(Size::Fixed(width)),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

/// One table row. The selected row takes the `list.activeSelection*`
/// pair and drops the muted treatment on its dim cells — muted
/// foreground over the selection fill is the one place this palette
/// goes unreadable.
fn part_row(p: &Part, selected: bool) -> El {
    let dim = |el: El| if selected { el } else { el.muted() };

    let stock = text(p.stock).tabular_numerals();
    let stock = match (selected, p.low) {
        (false, true) => stock.text_color(vs::ERROR_FG),
        _ => stock,
    };

    let row = table_row_cells(
        format!("part:{}", p.reference),
        COLS,
        [
            text(p.reference).mono(),
            text(p.value),
            dim(text(p.footprint).mono()),
            dim(text(p.library)),
            stock,
        ],
    );

    if selected {
        row.fill(vs::LIST_ACTIVE_SELECTION_BG)
            .text_color(vs::LIST_ACTIVE_SELECTION_FG)
            .no_hover()
    } else {
        row
    }
}

/// The footprint preview well — a square of `editor.background` inset
/// into the rail, holding a schematic 0603 chip: two copper pads
/// bridged by the component body, with the layer readout along the
/// bottom.
///
/// The pads are plain filled boxes rather than a vector asset: this is
/// a mock, and the shape a chip resistor's land pattern makes is two
/// rectangles and a body.
fn footprint_preview() -> El {
    let pad = || {
        column(Vec::<El>::new())
            .width(Size::Fixed(34.0))
            .height(Size::Fixed(58.0))
            .fill(vs::CHAT_EDITED_FILE_FG)
            .radius(1.0)
    };
    let body = column(Vec::<El>::new())
        .width(Size::Fixed(58.0))
        .height(Size::Fixed(40.0))
        .fill(vs::EDITOR_INACTIVE_SELECTION_BG)
        .stroke(vs::DESCRIPTION_FG)
        .radius(1.0);

    let caption_row = |lead: &str, trail: &str| {
        row([
            text(lead).caption().mono().muted(),
            spacer(),
            text(trail).caption().muted(),
        ])
        .width(Size::Fill(1.0))
        .align(Align::Center)
    };

    column([
        caption_row("R_0603_1608Metric", "1:1"),
        spacer(),
        // Zero gap so the pads abut the body, the way a land pattern
        // actually sits under a chip component.
        row([pad(), body, pad()])
            .gap(0.0)
            .width(Size::Fill(1.0))
            .justify(Justify::Center)
            .align(Align::Center),
        spacer(),
        caption_row("F.Cu · F.Paste · F.SilkS", "2 pads"),
    ])
    .padding(tokens::SPACE_2)
    .width(Size::Fixed(PREVIEW_SIDE))
    .height(Size::Fixed(PREVIEW_SIDE))
    .align(Align::Stretch)
    .fill(vs::EDITOR_BG)
    .stroke(vs::PANEL_BORDER)
    .radius(vs::RADIUS)
}

// ---------------------------------------------------------------------
// App.
// ---------------------------------------------------------------------

impl App for Copperline {
    fn build(&self, _cx: &BuildCx) -> El {
        // Full-bleed, like every workbench shell: no `page()` padding,
        // because a margin would float the instrument on the canvas.
        // The `overlays(.., [])` wrapper is what the status bar's
        // tooltip needs to mount on — see `examples/shell.rs`.
        let shell = column([
            self.toolbar(),
            row([self.parts_table(), self.inspector()])
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
            status_bar(
                [
                    text("412 parts · 1 selected").caption(),
                    text(PARTS[SELECTED].reference).caption().mono(),
                ],
                [
                    text("copperline.db").caption(),
                    chip("Library synced · 2m ago")
                        .success()
                        .key("sync")
                        .tooltip("Last sync 09:41 · 412 parts · 0 conflicts"),
                ],
            ),
        ])
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG);

        overlays(shell, [])
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1280.0, 800.0);
    damascene_winit_wgpu::run("Copperline", viewport, Copperline::new())
}
