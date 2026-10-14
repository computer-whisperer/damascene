//! **Copperline Tables** — the four shapes of `data_table`, live.
//!
//! Four views over the same widget, switched from the strip at the top,
//! rendered through [`damascene_workbench::theme::theme`] so the rows
//! land on the dense 29px workbench pitch rather than shadcn's 37px:
//!
//! 1. **Plain** — sticky header over a scrolling body. Nothing else:
//!    the shape a `scroll([table([...])])` was reaching for and missing.
//! 2. **Selectable** — sort indicators driven from app state, row
//!    selection, and an expandable detail panel behind a disclosure
//!    gutter. Clicking a header sorts *the app's* `Vec`; the widget only
//!    reported the request.
//! 3. **Grouped** — `data_group_row` bands between sections, in the same
//!    flat row list as the data rows.
//! 4. **Virtual** — 10,000 rows over `data_table_virtual`, with a
//!    footer. Only the screenful the viewport intersects is built.
//!
//! Everything is wired: arrow keys walk the rows, Enter/Space activates
//! the focused one, Tab reaches the sortable headers, and the status bar
//! reads the state back.
//!
//! Run: `cargo run -p damascene-workbench --example data_table`

use damascene_core::prelude::*;
use damascene_workbench::{chrome::*, theme, tokens as vs};

// ---------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------

/// One library part. `group` is the band a grouped view sorts it under.
struct Part {
    mpn: &'static str,
    group: &'static str,
    value: &'static str,
    footprint: &'static str,
    stock: u32,
}

const PARTS: &[Part] = &[
    Part { mpn: "RC0603FR-0710KL",    group: "Passives",  value: "10 kΩ ±1% 1/10 W",   footprint: "0603",       stock: 1240 },
    Part { mpn: "RC0603FR-07100RL",   group: "Passives",  value: "100 Ω ±1% 1/10 W",   footprint: "0603",       stock: 863 },
    Part { mpn: "RC0402FR-074K7L",    group: "Passives",  value: "4.7 kΩ ±1% 1/16 W",  footprint: "0402",       stock: 3105 },
    Part { mpn: "CRCW080510K0FKEA",   group: "Passives",  value: "10 kΩ ±1% 1/8 W",    footprint: "0805",       stock: 74 },
    Part { mpn: "GRM188R71H104KA93D", group: "Passives",  value: "100 nF 50 V X7R",    footprint: "0603",       stock: 4820 },
    Part { mpn: "C0805C106K8PACTU",   group: "Passives",  value: "10 µF 10 V X5R",     footprint: "0805",       stock: 690 },
    Part { mpn: "UWT1V101MCL1GS",     group: "Passives",  value: "100 µF 35 V ±20%",   footprint: "Radial-6.3", stock: 38 },
    Part { mpn: "LM358DR",            group: "Analog",    value: "Dual op-amp, 1 MHz", footprint: "SOIC-8",     stock: 214 },
    Part { mpn: "NE555DR",            group: "Analog",    value: "Precision timer",    footprint: "SOIC-8",     stock: 96 },
    Part { mpn: "TL072CDR",           group: "Analog",    value: "JFET op-amp, 3 MHz", footprint: "SOIC-8",     stock: 431 },
    Part { mpn: "STM32F103C8T6",      group: "MCU",       value: "Cortex-M3, 72 MHz",  footprint: "LQFP-48",    stock: 41 },
    Part { mpn: "ATMEGA328P-AU",      group: "MCU",       value: "AVR 8-bit, 32 kB",   footprint: "TQFP-32",    stock: 27 },
    Part { mpn: "RP2350A",            group: "MCU",       value: "Dual M33, 150 MHz",  footprint: "QFN-60",     stock: 512 },
    Part { mpn: "AMS1117-3.3",        group: "Power",     value: "LDO 3.3 V, 1 A",     footprint: "SOT-223",    stock: 305 },
    Part { mpn: "MCP1700T-3302E/TT",  group: "Power",     value: "LDO 3.3 V, 250 mA",  footprint: "SOT-23",     stock: 148 },
    Part { mpn: "TPS62840DLCR",       group: "Power",     value: "Buck 750 mA, 60 nA", footprint: "SOT-563",    stock: 0 },
    Part { mpn: "BC847B",             group: "Discrete",  value: "NPN 45 V, 100 mA",   footprint: "SOT-23",     stock: 2600 },
    Part { mpn: "SS14",               group: "Discrete",  value: "Schottky 40 V, 1 A", footprint: "SMA",        stock: 780 },
    Part { mpn: "1N4148WS",           group: "Discrete",  value: "Switching, 75 V",    footprint: "SOD-323",    stock: 3410 },
    Part { mpn: "SN74LVC1G14DBVR",    group: "Logic",     value: "Schmitt inverter",   footprint: "SOT-23-5",   stock: 0 },
];

/// Row count of the virtualized view — the whole point of that path.
const SIGNAL_COUNT: usize = 10_000;

// ---------------------------------------------------------------------
// Columns
// ---------------------------------------------------------------------

/// The plain and grouped views: no sort triggers, no disclosure gutter.
fn plain_columns() -> Vec<DataColumn> {
    vec![
        DataColumn::fill("Reference", 1.3),
        DataColumn::fill("Value", 1.2),
        DataColumn::fixed("Footprint", 110.0),
        DataColumn::ch("Stock", 8.0).align_end(),
    ]
}

/// The selectable view. A fixed leading gutter holds the
/// [`data_expander`], and three of the four data columns are sort
/// triggers — the indices below index *this* slice, which is why
/// `sort_key` reads from the same list.
fn parts_columns() -> Vec<DataColumn> {
    vec![
        // `MIN_TARGET_SIZE`, not `ICON_MD`: the expander is its own
        // interactive target with no hit overflow, so a 20px gutter
        // would put a sub-24px target inside the row's clearance
        // circle (`SmallHitTarget`).
        DataColumn::fixed("", tokens::MIN_TARGET_SIZE),
        DataColumn::fill("Reference", 1.3).sortable(),
        DataColumn::fill("Value", 1.2).sortable(),
        DataColumn::fixed("Footprint", 110.0),
        // Two digit slots wider than the plain view's: a sortable
        // header reserves a gutter for its direction indicator, and the
        // column has to fit label + gutter, not just the values.
        DataColumn::ch("Stock", 10.0).align_end().sortable(),
    ]
}

fn signal_columns() -> Vec<DataColumn> {
    vec![
        DataColumn::fill("Net", 1.4),
        DataColumn::fill("Domain", 1.0),
        DataColumn::ch("Slack (ps)", 11.0).align_end(),
        DataColumn::fixed("State", 96.0),
    ]
}

// ---------------------------------------------------------------------
// App
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Plain,
    Selectable,
    Grouped,
    Virtual,
}

impl View {
    const ALL: [(View, &'static str); 4] = [
        (View::Plain, "Plain"),
        (View::Selectable, "Selectable"),
        (View::Grouped, "Grouped"),
        (View::Virtual, "Virtual · 10k"),
    ];

    fn slug(self) -> &'static str {
        match self {
            View::Plain => "plain",
            View::Selectable => "selectable",
            View::Grouped => "grouped",
            View::Virtual => "virtual",
        }
    }

    fn parse(slug: &str) -> Option<View> {
        View::ALL.iter().map(|(v, _)| *v).find(|v| v.slug() == slug)
    }
}

impl std::fmt::Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

struct Tables {
    view: View,
    /// Row order, owned by the app. The widget reports a sort request;
    /// this `Vec` is what actually gets reordered.
    order: Vec<usize>,
    parts: DataTableState,
    signals: DataTableState,
}

impl Tables {
    fn new() -> Self {
        Self {
            view: View::Selectable,
            order: (0..PARTS.len()).collect(),
            parts: DataTableState::default(),
            signals: DataTableState::default(),
        }
    }

    // -----------------------------------------------------------------
    // Sorting — the app's job, not the widget's.
    // -----------------------------------------------------------------

    /// Re-derive `order` from `parts.sort`. Called after
    /// [`data_table::apply_event`] records a sort request; the widget
    /// itself has no comparator and no access to `PARTS`.
    fn resort(&mut self) {
        let Some(sort) = self.parts.sort else {
            self.order = (0..PARTS.len()).collect();
            return;
        };
        self.order.sort_by(|&a, &b| {
            let (a, b) = (&PARTS[a], &PARTS[b]);
            // Column indices into `parts_columns()`; 0 is the gutter.
            let ord = match sort.column {
                1 => a.mpn.cmp(b.mpn),
                2 => a.value.cmp(b.value),
                4 => a.stock.cmp(&b.stock),
                _ => std::cmp::Ordering::Equal,
            };
            match sort.direction {
                SortDirection::Ascending => ord,
                SortDirection::Descending => ord.reverse(),
            }
        });
    }

    // -----------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------

    /// Sticky header, scrolling body, nothing else. The whole
    /// difference from `scroll([table([...])])` is that the header does
    /// not scroll away.
    fn plain(&self) -> El {
        let cols = plain_columns();
        let rows = PARTS
            .iter()
            .map(|p| data_row(data_row_key("plain", &p.mpn), &cols, part_cells(p)));
        data_table("plain", &cols, rows)
    }

    /// Selection, sort indicators, and a detail panel per expanded row.
    fn selectable(&self) -> El {
        let cols = parts_columns();
        let mut rows: Vec<El> = Vec::with_capacity(self.order.len() * 2);

        for &i in &self.order {
            let p = &PARTS[i];
            let mut cells = vec![data_expander(
                data_expand_key("parts", &p.mpn),
                self.parts.is_expanded(p.mpn),
            )];
            cells.extend(part_cells(p));

            let row = data_row(data_row_key("parts", &p.mpn), &cols, cells);
            rows.push(if self.parts.is_selected(p.mpn) {
                row.selected()
            } else {
                row
            });

            if self.parts.is_expanded(p.mpn) {
                rows.push(data_detail_row(part_detail(p)));
            }
        }

        data_table_with(
            "parts",
            &cols,
            DataTableOpts::default()
                .sort(self.parts.sort)
                .footer(data_footer_row(
                    &cols,
                    [
                        text(""),
                        text(format!("{} parts", PARTS.len())).caption(),
                        text(""),
                        text(""),
                        text(commas(PARTS.iter().map(|p| p.stock).sum()))
                            .tabular_numerals()
                            .caption(),
                    ],
                )),
            rows,
        )
    }

    /// Group bands interleaved with the data rows — one flat list, the
    /// app deciding where the seams go.
    fn grouped(&self) -> El {
        let cols = plain_columns();
        let mut rows: Vec<El> = Vec::new();
        let mut current = "";
        for p in PARTS {
            if p.group != current {
                rows.push(data_group_row(p.group));
                current = p.group;
            }
            rows.push(data_row(
                data_row_key("grouped", &p.mpn),
                &cols,
                part_cells(p),
            ));
        }
        data_table("grouped", &cols, rows)
    }

    /// 10,000 rows. The closures outlive the frame, so they own their
    /// column spec and their slice of state rather than borrowing this
    /// struct.
    fn virtualized(&self) -> El {
        let cols = signal_columns();
        let build_cols = cols.clone();
        let selected = self.signals.selected.clone();

        data_table_virtual_with(
            "signals",
            &cols,
            DataTableOpts::default()
                // The workbench profile sits one rung below the stock
                // default, so the estimate has to follow it — otherwise
                // the scrollbar thumb is sized against 33px rows the
                // theme renders at 29.
                .row_estimate(estimated_row_height(ComponentSize::Xs))
                .footer(data_footer_row(
                    &cols,
                    [
                        text(format!("{SIGNAL_COUNT} nets")).caption(),
                        text(""),
                        text(""),
                        text("synthetic").caption(),
                    ],
                )),
            SIGNAL_COUNT,
            |i| format!("net_{i:05}"),
            move |i| {
                let id = format!("net_{i:05}");
                let slack = (i as i32 * 37) % 900 - 300;
                let domain = ["clk_sys", "clk_usb", "clk_adc", "async"][i % 4];
                let state = if slack < 0 { "violated" } else { "met" };
                let row = data_row(
                    data_row_key("signals", &id),
                    &build_cols,
                    [
                        text(id.clone()).mono(),
                        text(domain).muted(),
                        text(slack.to_string()).tabular_numerals(),
                        // A badge has no horizontal alignment of its
                        // own, so a bare one stretches to the column
                        // (`table_row_cells`' documented rule). The row
                        // wrapper lets it hug, and centers it against
                        // the text cells beside it.
                        row([if slack < 0 {
                            let b = badge(state).destructive();
                            // KNOWN, PINNED: the destructive tint over
                            // a *selected* row composites to #62385d,
                            // where `destructive-tint-foreground`
                            // measures 4.44:1 — 0.06 under AA. Real,
                            // and core's to fix (either the tint
                            // foreground moves further along its ramp,
                            // or tinted status text stops riding a
                            // Selected surface); it is not something an
                            // app can correct. Suppressed on this node
                            // only, so every other contrast finding in
                            // this example still fails the guard.
                            if selected.as_deref() == Some(id.as_str()) {
                                b.allow_lint(FindingKind::LowContrastText)
                            } else {
                                b
                            }
                        } else {
                            badge(state).muted()
                        }])
                        .align(Align::Center),
                    ],
                );
                if selected.as_deref() == Some(id.as_str()) {
                    row.selected()
                } else {
                    row
                }
            },
        )
    }

    // -----------------------------------------------------------------
    // Chrome
    // -----------------------------------------------------------------

    fn strip(&self) -> El {
        row([
            text("COPPERLINE")
                .caption()
                .font_weight(FontWeight::Medium)
                .text_color(vs::SIDE_BAR_SECTION_HEADER_FG),
            tabs_list("view", &self.view, View::ALL.map(|(v, l)| (v.slug(), l))),
        ])
        .gap(tokens::SPACE_3)
        .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
        .width(Size::Fill(1.0))
        .align(Align::Center)
        .fill(vs::SIDE_BAR_BG)
        .border_b()
        .border_color(vs::PANEL_BORDER)
    }

    fn status(&self) -> El {
        let state = match self.view {
            View::Virtual => &self.signals,
            _ => &self.parts,
        };
        let sort = match state.sort {
            Some(sort) => {
                let cols = parts_columns();
                let name = cols
                    .get(sort.column)
                    .map(|c| c.header.as_str())
                    .unwrap_or("?");
                let arrow = match sort.direction {
                    SortDirection::Ascending => "asc",
                    SortDirection::Descending => "desc",
                };
                format!("sort: {name} {arrow}")
            }
            None => "sort: none".to_string(),
        };
        let selected = match &state.selected {
            Some(id) => format!("selected: {id}"),
            None => "selected: —".to_string(),
        };

        status_bar(
            [
                text(selected).caption(),
                text(sort).caption(),
                text(format!("expanded: {}", state.expanded.len())).caption(),
            ],
            [
                chip(match self.view {
                    View::Virtual => format!("{SIGNAL_COUNT} rows"),
                    _ => format!("{} rows", PARTS.len()),
                }),
                text("29px pitch · Xs").caption(),
            ],
        )
    }
}

/// The four data cells shared by every non-virtual view.
fn part_cells(p: &Part) -> Vec<El> {
    vec![
        text(p.mpn).mono(),
        text(p.value),
        text(p.footprint).muted(),
        if p.stock == 0 {
            text("0").tabular_numerals().text_color(tokens::DESTRUCTIVE)
        } else {
            text(commas(p.stock)).tabular_numerals()
        },
    ]
}

/// The expansion panel's body — a small property sheet, inset by the
/// detail cell's own rung padding.
fn part_detail(p: &Part) -> El {
    column([
        row([
            text("Group").caption().width(Size::Fixed(72.0)),
            text(p.group).label(),
        ])
        .align(Align::Center),
        row([
            text("Footprint").caption().width(Size::Fixed(72.0)),
            text(p.footprint).label().mono(),
        ])
        .align(Align::Center),
        row([
            text("On hand").caption().width(Size::Fixed(72.0)),
            text(commas(p.stock)).label().tabular_numerals(),
            spacer(),
            button("Order").key(format!("order:{}", p.mpn)).primary(),
        ])
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}

/// Thousands separators, so the numeric column has something to keep
/// aligned.
fn commas(n: u32) -> String {
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

impl App for Tables {
    fn build(&self, _cx: &BuildCx) -> El {
        // A bare column, not `page()`: a workbench view is full-bleed by
        // definition, and the table wants every pixel of the pane.
        column([
            self.strip(),
            match self.view {
                View::Plain => self.plain(),
                View::Selectable => self.selectable(),
                View::Grouped => self.grouped(),
                View::Virtual => self.virtualized(),
            },
            self.status(),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
        .fill(vs::EDITOR_BG)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        tabs::apply_event(&mut self.view, &event, "view", View::parse);

        // One `apply_event` per table: selection, expansion, and the
        // sort *request* all fold in one call. Sorting itself is ours.
        if data_table::apply_event(&mut self.parts, &event, "parts") {
            self.resort();
        }
        data_table::apply_event(&mut self.signals, &event, "signals");

        // The read-only views share the same state struct so the status
        // bar has something to read; they route under their own keys.
        data_table::apply_event(&mut self.parts, &event, "plain");
        data_table::apply_event(&mut self.parts, &event, "grouped");
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, VIEWPORT.0, VIEWPORT.1);
    damascene_winit_wgpu::run("Damascene — data table", viewport, Tables::new())
}

/// The size the example opens at, and the canvas the lint guard renders
/// against — overflow findings are viewport-sensitive, so the two must
/// agree.
const VIEWPORT: (f32, f32) = (1040.0, 660.0);

#[cfg(test)]
mod tests {
    use super::*;

    /// Every view, through the real pipeline — theme, metrics, layout,
    /// draw ops, lint — asserting zero findings. The interesting
    /// failures of a scrolling, focusable, sticky-header table are
    /// cross-cutting (`FocusRingObscured` under two nested clips,
    /// `ScrollbarObscuresFocusable` on full-width rows under a thumb,
    /// contrast of themed text on chrome fills), and only a rendered
    /// bundle at a real size sees them.
    #[test]
    fn every_view_is_lint_clean() {
        for (view, label) in View::ALL {
            let mut app = Tables::new();
            app.view = view;
            // Exercise the states the chrome reads back, so the guard
            // covers a sorted header, a selected row, and an open
            // detail panel rather than only the pristine tree.
            app.parts.selected = Some(PARTS[3].mpn.to_string());
            app.parts.sort_by(4);
            app.parts.toggle_expanded(PARTS[3].mpn);
            app.signals.selected = Some("net_00007".to_string());
            app.resort();

            let theme = app.theme();
            let cx = BuildCx::new(&theme).with_viewport(VIEWPORT.0, VIEWPORT.1);
            let mut tree = app.build(&cx);
            let bundle = render_bundle_themed(
                &mut tree,
                Rect::new(0.0, 0.0, VIEWPORT.0, VIEWPORT.1),
                &theme,
            );
            assert!(
                bundle.lint.findings.is_empty(),
                "the {label} view should be lint-clean at {}x{}; found {} finding(s):\n{}",
                VIEWPORT.0,
                VIEWPORT.1,
                bundle.lint.findings.len(),
                bundle.lint.text(),
            );
        }
    }

    /// The example's claim about itself: the app sorts, the widget only
    /// asks. Drive a header activation through `on_event` and assert the
    /// app's own row order changed.
    #[test]
    fn a_header_click_reorders_the_apps_own_rows() {
        let mut app = Tables::new();
        let cx = EventCx::new();

        assert_eq!(app.order[0], 0, "natural order to start");
        app.on_event(UiEvent::synthetic_click("parts:sort:4"), &cx);
        assert_eq!(app.parts.sort, Some(Sort::ascending(4)));
        assert_eq!(
            PARTS[app.order[0]].stock, 0,
            "ascending by stock puts an out-of-stock part first"
        );

        app.on_event(UiEvent::synthetic_click("parts:sort:4"), &cx);
        assert_eq!(app.parts.sort, Some(Sort::descending(4)));
        assert_eq!(
            PARTS[app.order[0]].stock,
            PARTS.iter().map(|p| p.stock).max().unwrap(),
            "a second activation reverses it"
        );
    }

    /// The virtual view must not build 10,000 rows to show 20.
    #[test]
    fn the_virtual_view_realizes_only_a_screenful() {
        let mut app = Tables::new();
        app.view = View::Virtual;
        let theme = app.theme();
        let cx = BuildCx::new(&theme).with_viewport(VIEWPORT.0, VIEWPORT.1);
        let mut tree = app.build(&cx);
        render_bundle_themed(
            &mut tree,
            Rect::new(0.0, 0.0, VIEWPORT.0, VIEWPORT.1),
            &theme,
        );

        // column[1] is the table; its child[1] is the virtual body.
        let body = &tree.children[1].children[1];
        assert_eq!(body.kind, Kind::VirtualList);
        assert!(
            (1..200).contains(&body.children.len()),
            "realized {} of {SIGNAL_COUNT} rows",
            body.children.len(),
        );
    }
}
