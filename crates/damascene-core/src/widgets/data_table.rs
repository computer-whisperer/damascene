//! Data table — [`table`](crate::widgets::table)'s bigger sibling: the
//! one that owns its scroll, so the header stays put.
//!
//! [`table`](crate::widgets::table::table) is anatomy and geometry —
//! `<table>`, `<thead>`, `<tbody>`, and a [`TableColumn`] spec the
//! header and the body rows both render from. It has no behaviour and
//! no scroll of its own: putting one inside `scroll([...])` scrolls the
//! header away with the rows, and a data view of any size needs the
//! header pinned.
//!
//! `data_table` closes that gap **by composition, with no core layout
//! changes**: the header row (and an optional footer row) are laid out
//! *outside* an internal scroll region, and only the body rows go
//! inside it. A column of `[header, scroll(body), footer]` gives sticky
//! header and sticky footer for free, because the scroll is a sibling
//! rather than an ancestor.
//!
//! ```text
//! data_table                     Kind::Custom("data_table"), clipped
//! ├─ table_header                sticky — outside the scroll
//! │  └─ table_row                one head cell per DataColumn
//! ├─ scroll  key "{key}:body"    Fill height, arrow-nav over the rows
//! │  └─ table_body
//! │     ├─ data_row              keyed, focusable, full-row target
//! │     ├─ data_detail_row       expansion panel, spans all columns
//! │     ├─ data_group_row        full-width group label
//! │     └─ …
//! └─ data_table_footer           sticky — outside the scroll
//! ```
//!
//! # Height
//!
//! **The widget must be given a bounded height.** It owns a scroll, and
//! a scroll needs something to scroll inside; the root therefore
//! defaults to `Size::Fill(1.0)`. Dropped into a `Hug`-height parent it
//! collapses (core's `CollapsedFill` lint says so). Either let it fill a
//! `Fill`-height region — the normal case, a table filling a pane — or
//! set `.height(Size::Fixed(…))`. A short table that should simply hug
//! its rows with no scrolling at all is still plain
//! [`table`](crate::widgets::table::table); that is the widget this one
//! does not replace.
//!
//! # Usage
//!
//! State is controlled, as everywhere else: the app owns a
//! [`DataTableState`], projects it into the build, and folds routed
//! events back with [`apply_event`]. The widget **never sorts** — it
//! renders the caller's sort indicator and emits a routed key when a
//! header cell is activated; ordering the rows is the app's job, on the
//! app's data.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Parts {
//!     parts: Vec<Part>,
//!     table: DataTableState,
//! }
//!
//! fn columns() -> Vec<DataColumn> {
//!     vec![
//!         DataColumn::fill("Reference", 1.2).sortable(),
//!         DataColumn::fill("Value", 1.0),
//!         DataColumn::fixed("Footprint", 120.0),
//!         DataColumn::ch("Stock", 7.0).align_end().sortable(),
//!     ]
//! }
//!
//! impl App for Parts {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         let cols = columns();
//!         let rows = self.parts.iter().map(|p| {
//!             let row = data_row(
//!                 data_row_key("parts", &p.mpn),
//!                 &cols,
//!                 [
//!                     text(&p.mpn).mono(),
//!                     text(&p.value),
//!                     text(&p.footprint),
//!                     text(&p.stock).tabular_numerals(),
//!                 ],
//!             );
//!             if self.table.is_selected(&p.mpn) { row.selected() } else { row }
//!         });
//!         data_table_with(
//!             "parts",
//!             &cols,
//!             DataTableOpts::default().sort(self.table.sort),
//!             rows,
//!         )
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
//!         if data_table::apply_event(&mut self.table, &event, "parts") {
//!             self.resort();   // the app sorts; the widget only asked.
//!         }
//!     }
//! }
//! ```
//!
//! # Row anatomy
//!
//! The body is a **flat list**, the same shape [`tree`] uses: the app
//! emits one El per *visible* row, in order, and the row kind says what
//! it is. That keeps expansion and grouping where the library
//! philosophy puts all state — in the app.
//!
//! - [`data_row`] — the data row. Keyed, focusable, full-row hit
//!   target, hover response, [`FocusRingPlacement::Inside`] so the
//!   internal scroll's scissor cannot clip its ring. Selection and
//!   disablement compose from the stock chainables (`.selected()`,
//!   `.disabled()`) exactly as they do on [`tree_item`] and
//!   [`sidebar_menu_button`] — the widget has no `selected: bool`
//!   parameter because the house style already has one spelling for it.
//! - [`data_detail_row`] — an expansion panel spanning all columns,
//!   emitted directly after the row it belongs to. The app decides
//!   which rows are expanded ([`DataTableState::is_expanded`]);
//!   [`data_expander`] is the optional disclosure cell that routes the
//!   toggle.
//! - [`data_group_row`] — a full-width label row between groups.
//! - [`data_footer_row`] — a column-aligned totals row, passed through
//!   [`DataTableOpts::footer`] so it lands *outside* the scroll.
//!
//! # Virtualization
//!
//! [`data_table_virtual`] is the same widget over
//! [`virtual_list_dyn`](crate::tree::virtual_list_dyn): the body
//! realizes only the rows the viewport intersects, so a 10k-row table
//! costs a screenful. `row_key(i)` must be a stable identity — the
//! dynamic list uses it to keep an in-viewport anchor across inserts,
//! removals, and remeasures. The flat-list model is unchanged: index
//! `i` is a *visible row*, so an app with group headers and expanded
//! detail panels flattens them into the same index space (see
//! [`data_table_virtual`]'s docs for the projection).
//!
//! **Known limitation, shared with every virtualized surface in the
//! library:** arrow-key navigation covers the rows that are currently
//! realized. Stepping focus past the edge of the realization window
//! stops rather than scrolling. Small tables (a few hundred rows) do
//! not pay for this — use the plain [`data_table`] path.
//!
//! # Oracle (`docs/NAMING_ORACLE.md`)
//!
//! shadcn's `DataTable` recipe over TanStack Table, whose `columns`
//! array is the same one positional spec the header and every row
//! render from — [`TableColumn`] is already its geometry half, and
//! [`DataColumn`] is that plus the two things a *behaving* table needs
//! per column: the header label, and whether the header is a sort
//! trigger. Sticky header / footer, row selection, expandable detail
//! rows, and grouping are TanStack's own feature names.
//!
//! Deliberately **not** in v1, each because the shape is genuinely
//! undecided rather than merely unwritten: column resize dragging, cell
//! editing, horizontal scrolling, and multi-select ranges.
//!
//! [`tree`]: crate::widgets::tree::tree
//! [`tree_item`]: crate::widgets::tree::tree_item
//! [`sidebar_menu_button`]: crate::widgets::sidebar::sidebar_menu_button

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::collections::BTreeSet;
use std::fmt::Display;
use std::panic::Location;

use crate::a11y::Role;
use crate::cursor::Cursor;
use crate::event::UiEvent;
use crate::icon;
use crate::metrics::ComponentSize;
use crate::tokens;
use crate::tree::*;
use crate::widgets::table::{
    TableColumn, stamp_column, table_body, table_cell, table_head_el, table_header, table_row,
    table_row_cells,
};
use crate::widgets::text::text;

// ---------------------------------------------------------------------
// Column model
// ---------------------------------------------------------------------

/// One column of a [`data_table`]: its geometry, its header label, and
/// whether the header cell is a sort trigger.
///
/// The geometry is a [`TableColumn`] — the *same* type plain
/// [`table`](crate::widgets::table::table) rows are stamped from, held
/// in a field rather than re-minted, so a table that grows into a
/// `data_table` keeps one description of its columns and a
/// `data_table`'s columns can be handed straight to
/// [`table_row_cells`](crate::widgets::table::table_row_cells).
///
/// ```ignore
/// let cols = vec![
///     DataColumn::fill("Reference", 1.2).sortable(),
///     DataColumn::fixed("Footprint", 120.0),
///     DataColumn::ch("Stock", 7.0).align_end().sortable(),
/// ];
/// ```
///
/// The label is a `String`, so column sets are built per frame like any
/// other part of the tree (a unit conversion in a header — `"Pressure
/// (kPa)"` — is a normal thing to want). The allocation is a handful of
/// short strings per build, against a body that allocates per cell.
#[derive(Clone, Debug, PartialEq)]
pub struct DataColumn {
    /// Width and horizontal alignment, stamped onto the header cell and
    /// the same-position cell of every body row.
    pub column: TableColumn,
    /// The header label. Rendered through
    /// [`table_head_el`](crate::widgets::table::table_head_el)'s muted
    /// caption treatment, ellipsized to the column width.
    pub header: String,
    /// Whether the header cell is activatable. A sortable header is
    /// focusable, takes a pointer cursor, reserves a gutter for the
    /// direction indicator, and emits [`data_sort_key`] on click or
    /// keyboard activation. **It does not sort** — see the module docs.
    pub sortable: bool,
}

impl DataColumn {
    /// A column with explicit [`TableColumn`] geometry — the escape
    /// hatch for a `Size` the shorthands below do not name.
    pub fn new(header: impl Into<String>, column: TableColumn) -> Self {
        Self {
            column,
            header: header.into(),
            sortable: false,
        }
    }

    /// A column claiming `weight` of the leftover width
    /// ([`TableColumn::fill`]).
    pub fn fill(header: impl Into<String>, weight: f32) -> Self {
        Self::new(header, TableColumn::fill(weight))
    }

    /// A column pinned to `px` logical pixels ([`TableColumn::fixed`]).
    pub fn fixed(header: impl Into<String>, px: f32) -> Self {
        Self::new(header, TableColumn::fixed(px))
    }

    /// A numeric gutter `n` tabular digits wide ([`TableColumn::ch`]).
    /// Pair with `.align_end()` and `.tabular_numerals()` on the cell
    /// text so the digits stack and the field stops twitching as values
    /// change length.
    pub fn ch(header: impl Into<String>, n: f32) -> Self {
        Self::new(header, TableColumn::ch(n))
    }

    /// Align this column's content to the leading edge (the default).
    pub fn align_start(mut self) -> Self {
        self.column = self.column.align_start();
        self
    }

    /// Center this column's content.
    pub fn align_center(mut self) -> Self {
        self.column = self.column.align_center();
        self
    }

    /// Align this column's content to the trailing edge — the numeric
    /// column treatment.
    pub fn align_end(mut self) -> Self {
        self.column = self.column.align_end();
        self
    }

    /// Make this column's header cell a sort trigger (see
    /// [`DataColumn::sortable`]).
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }
}

/// The geometry half of a column set — the `&[TableColumn]` slice the
/// plain [`table`](crate::widgets::table::table) constructors take.
///
/// Use it to build a row by hand when [`data_row`]'s uniform cell
/// chrome isn't enough:
///
/// ```ignore
/// let geom = column_geometry(&cols);
/// table_row_cells(data_row_key("parts", &id), &geom, cells)
/// ```
pub fn column_geometry(cols: &[DataColumn]) -> Vec<TableColumn> {
    cols.iter().map(|c| c.column).collect()
}

// ---------------------------------------------------------------------
// Sort
// ---------------------------------------------------------------------

/// Which way a sorted column is ordered. The `aria-sort` values, and
/// TanStack's `desc: false | true`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortDirection {
    /// Smallest / earliest / A–Z first. The direction a fresh sort
    /// starts in, matching every stock data grid.
    #[default]
    Ascending,
    /// Largest / latest / Z–A first.
    Descending,
}

impl SortDirection {
    /// The other direction — what a second activation of an already
    /// sorted header means.
    pub fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// The sort the app has applied: which column, and which way.
///
/// Held by the app (usually inside [`DataTableState`]) and passed back
/// in through [`DataTableOpts::sort`] so the header can draw the
/// indicator. The widget reads it; it never writes it, and it never
/// reorders rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    /// Index into the `&[DataColumn]` slice.
    pub column: usize,
    /// Which way that column is ordered.
    pub direction: SortDirection,
}

impl Sort {
    /// Sort `column` ascending.
    pub fn ascending(column: usize) -> Self {
        Self {
            column,
            direction: SortDirection::Ascending,
        }
    }

    /// Sort `column` descending.
    pub fn descending(column: usize) -> Self {
        Self {
            column,
            direction: SortDirection::Descending,
        }
    }

    /// The same column, the other way round.
    pub fn reversed(self) -> Self {
        Self {
            column: self.column,
            direction: self.direction.reversed(),
        }
    }
}

// ---------------------------------------------------------------------
// State
// ---------------------------------------------------------------------

/// The app-owned state a [`data_table`] projects: which row is
/// selected, which rows are expanded, and the current sort.
///
/// Controlled state, like every other Damascene widget: the app holds
/// this struct, the builders read it, and [`apply_event`] folds routed
/// events back into it. Rows are identified by the **id** — the part of
/// the routed key after `{key}:row:` — not by index, so the selection
/// survives a re-sort.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DataTableState {
    /// The selected row's id, if any. Single selection; multi-select
    /// ranges are deliberately out of v1 (see the module docs).
    pub selected: Option<String>,
    /// Ids of the rows whose [`data_detail_row`] panels are showing.
    pub expanded: BTreeSet<String>,
    /// The sort the app has applied, or `None` for the natural order.
    pub sort: Option<Sort>,
}

impl DataTableState {
    /// Whether `id` is the selected row — the predicate that gates the
    /// stock `.selected()` chainable on the row:
    ///
    /// ```ignore
    /// let row = data_row(data_row_key("parts", &p.mpn), &cols, cells);
    /// if state.is_selected(&p.mpn) { row.selected() } else { row }
    /// ```
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.as_deref() == Some(id)
    }

    /// Whether `id`'s detail panel is showing — the predicate that
    /// decides whether the app emits a [`data_detail_row`] after that
    /// row.
    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    /// Show or hide `id`'s detail panel.
    pub fn toggle_expanded(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_string());
        }
    }

    /// Apply the standard header-click rule: activating the sorted
    /// column reverses it, activating any other column sorts it
    /// ascending. What [`apply_event`] calls for
    /// [`DataTableAction::Sort`].
    pub fn sort_by(&mut self, column: usize) {
        self.sort = Some(match self.sort {
            Some(sort) if sort.column == column => sort.reversed(),
            _ => Sort::ascending(column),
        });
    }
}

// ---------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------

/// Row pitch (content box plus the row's 1px rule) at each rung of the
/// [`ComponentSize`] ladder — the value
/// [`data_table_virtual`] wants for its unmeasured rows.
///
/// A table row's content box is exactly as tall as a control of the
/// same rung (that is the invariant
/// [`ComponentSize`]-keyed cell padding is derived from), and
/// [`table_body`](crate::widgets::table::table_body) puts a 1px
/// `border_b` under every row but the last. So the pitch is
/// `control_height + 1`, independent of the theme's type scale:
/// 23 px at `Xxs`, 29 at `Xs`, 33 at `Sm`, 37 at `Md`, 41 at `Lg`.
pub const fn estimated_row_height(size: ComponentSize) -> f32 {
    let control = match size {
        ComponentSize::Xxs => 22.0,
        ComponentSize::Xs => 28.0,
        ComponentSize::Sm => 32.0,
        ComponentSize::Md => 36.0,
        ComponentSize::Lg => 40.0,
    };
    control + 1.0
}

/// The default row-height estimate for [`data_table_virtual`] — the
/// pitch at Damascene's default [`ComponentSize::Sm`]. Override with
/// [`DataTableOpts::row_estimate`] when the theme moves the rung (a
/// workbench-profile app wants `estimated_row_height(ComponentSize::Xs)`).
pub const DEFAULT_ROW_ESTIMATE: f32 = estimated_row_height(ComponentSize::Sm);

/// The optional half of a [`data_table`]: sort indicator, sticky
/// footer, header suppression, and the virtual row estimate.
///
/// ```ignore
/// DataTableOpts::default()
///     .sort(self.table.sort)
///     .footer(data_footer_row(&cols, [text("Total"), text("18,204")]))
/// ```
#[derive(Clone, Debug, Default)]
pub struct DataTableOpts {
    /// The caller's current sort, drawn as an indicator on that
    /// column's header cell. `None` draws no indicator anywhere.
    pub sort: Option<Sort>,
    /// A row pinned below the scroll region — totals, counts, a status
    /// line. Build it with [`data_footer_row`] to keep it aligned with
    /// the columns.
    pub footer: Option<El>,
    /// Set to hide the header row entirely (a table embedded under a
    /// pane header that already names the columns). Defaults to
    /// showing it.
    pub headerless: bool,
    /// Height estimate for the not-yet-measured rows of
    /// [`data_table_virtual`]; ignored by the plain path. Defaults to
    /// [`DEFAULT_ROW_ESTIMATE`].
    pub row_estimate: Option<f32>,
}

impl DataTableOpts {
    /// Set the sort indicator (see [`DataTableOpts::sort`]). Takes an
    /// `Option` because that is the shape [`DataTableState::sort`]
    /// already has.
    pub fn sort(mut self, sort: Option<Sort>) -> Self {
        self.sort = sort;
        self
    }

    /// Set the sticky footer row (see [`DataTableOpts::footer`]).
    pub fn footer(mut self, footer: impl Into<El>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Hide the header row (see [`DataTableOpts::headerless`]).
    pub fn headerless(mut self) -> Self {
        self.headerless = true;
        self
    }

    /// Set the virtual row-height estimate (see
    /// [`DataTableOpts::row_estimate`]).
    pub fn row_estimate(mut self, px: f32) -> Self {
        self.row_estimate = Some(px);
        self
    }
}

// ---------------------------------------------------------------------
// Routed keys
// ---------------------------------------------------------------------

/// The routed key of the body row for `id` in the table keyed `key` —
/// `{key}:row:{id}`.
///
/// `id` is the row's *data* identity (an MPN, a uuid, a serial), not
/// its index: a keyed row survives sorting and filtering, an indexed
/// one does not.
pub fn data_row_key(key: &str, id: &impl Display) -> String {
    format!("{key}:row:{id}")
}

/// The routed key of the header cell for column `index` —
/// `{key}:sort:{index}`.
pub fn data_sort_key(key: &str, index: usize) -> String {
    format!("{key}:sort:{index}")
}

/// The routed key of the [`data_expander`] disclosure cell for `id` —
/// `{key}:expand:{id}`.
pub fn data_expand_key(key: &str, id: &impl Display) -> String {
    format!("{key}:expand:{id}")
}

// ---------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------

/// What a routed event on a [`data_table`] meant.
///
/// The borrowed `&str`s point into the event's routed key, so keep one
/// past the match arm with `.to_string()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataTableAction<'a> {
    /// A body row was clicked or keyboard-activated. The `&str` is the
    /// row id (the part after `{key}:row:`).
    Activate(&'a str),
    /// A [`data_expander`] was activated. Show or hide that row's
    /// detail panel.
    ToggleExpanded(&'a str),
    /// A sortable header cell was activated. The `usize` indexes the
    /// `&[DataColumn]` slice; **the app sorts**, the widget only
    /// reports the request.
    Sort(usize),
}

/// Classify a routed [`UiEvent`] against the table keyed `key`.
/// Returns `None` for events that are not for this table.
///
/// Only `Click` / `Activate` qualify — the standard click-or-activate
/// path every stock control routes through, so a row responds to the
/// mouse and to Enter/Space on the focused row identically.
pub fn classify_event<'a>(event: &'a UiEvent, key: &str) -> Option<DataTableAction<'a>> {
    if !matches!(
        event.kind,
        crate::event::UiEventKind::Click | crate::event::UiEventKind::Activate
    ) {
        return None;
    }
    let route = event.route()?;
    if let Some(id) = crate::key::suffix(route, &format!("{key}:row")) {
        return Some(DataTableAction::Activate(id));
    }
    if let Some(id) = crate::key::suffix(route, &format!("{key}:expand")) {
        return Some(DataTableAction::ToggleExpanded(id));
    }
    if let Some(index) = crate::key::index::<usize>(route, &format!("{key}:sort")) {
        return Some(DataTableAction::Sort(index));
    }
    None
}

/// Fold a routed [`UiEvent`] into a [`DataTableState`]. Returns `true`
/// when the event was for this table and the state changed shape —
/// which is also the app's cue to re-derive anything downstream (a
/// re-sort, a detail fetch).
///
/// ```ignore
/// fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
///     if data_table::apply_event(&mut self.table, &event, "parts") {
///         self.resort();
///     }
/// }
/// ```
///
/// Sorting is deliberately not done here: the widget has no access to
/// the app's rows, no comparator, and no opinion about stability or
/// locale. [`DataTableState::sort`] records *what was asked for* and
/// the app applies it.
pub fn apply_event(state: &mut DataTableState, event: &UiEvent, key: &str) -> bool {
    match classify_event(event, key) {
        Some(DataTableAction::Activate(id)) => {
            state.selected = Some(id.to_string());
            true
        }
        Some(DataTableAction::ToggleExpanded(id)) => {
            state.toggle_expanded(id);
            true
        }
        Some(DataTableAction::Sort(column)) => {
            state.sort_by(column);
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------

/// A body row (a `<tr>` of `<td>`) stamped from the column spec, keyed,
/// focusable, and clickable across its whole width.
///
/// The `key` is the row's **routed key** — mint it with
/// [`data_row_key`] so [`apply_event`] can route it back. This mirrors
/// [`table_row_cells`](crate::widgets::table::table_row_cells), whose
/// key argument means the same thing; a `data_row` is that row plus
/// focus and a pointer cursor.
///
/// Pass *content*, not pre-built cells: the padded, ellipsizing cell
/// chrome and the column's width / alignment are applied here.
///
/// Selection, disablement, and validity compose from the stock
/// chainables rather than from parameters:
///
/// ```ignore
/// let row = data_row(data_row_key("parts", &p.mpn), &cols, cells);
/// if state.is_selected(&p.mpn) { row.selected() } else { row }
///
/// data_row(…).disabled()      // dropped from focus order, pointer-inert
/// ```
///
/// The row draws its focus ring [`FocusRingPlacement::Inside`] (from
/// [`table_row`](crate::widgets::table::table_row)), which is what lets
/// the internal scroll clip the body without scissoring the ring off a
/// focused row.
#[track_caller]
pub fn data_row<I, E>(key: impl Into<String>, cols: &[DataColumn], cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    table_row_cells(key, &column_geometry(cols), cells)
        .at_loc(Location::caller())
        .focusable()
        .cursor(Cursor::Pointer)
}

/// A full-width label row separating groups of data rows — the
/// "Passives", "Analog", "Power" band a grouped table puts between its
/// sections.
///
/// Emitted inline in the flat row list, before the group's rows. Not
/// focusable and not hoverable: it is a landmark, not a control. Its
/// label cell takes the same rung padding as every other cell, so the
/// text lines up with the first column.
#[track_caller]
pub fn data_group_row(label: impl Into<String>) -> El {
    table_row([table_cell(text(label).label())])
        .at_loc(Location::caller())
        .fill(tokens::MUTED)
        .no_hover()
}

/// An expansion panel attached to the row above it — spans every
/// column, sits between that row and the next.
///
/// The app decides when to emit one (`if state.is_expanded(id)`), which
/// is what makes "expanded" app state rather than hidden widget state.
/// The panel's single cell takes the rung's cell padding, so a plain
/// `column([...])` of content is already inset in line with the table;
/// content that wants more air sets its own `.padding(...)`.
///
/// ```ignore
/// let mut rows = Vec::new();
/// for p in &self.parts {
///     rows.push(data_row(data_row_key("parts", &p.mpn), &cols, cells(p)));
///     if self.table.is_expanded(&p.mpn) {
///         rows.push(data_detail_row(part_detail(p)));
///     }
/// }
/// ```
#[track_caller]
pub fn data_detail_row(content: impl Into<El>) -> El {
    table_row([El::new(Kind::Custom("data_detail"))
        .children([content.into()])
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .radius(0.0)])
    .at_loc(Location::caller())
    .no_hover()
}

/// The disclosure cell for a row with a [`data_detail_row`] — a
/// chevron that routes [`DataTableAction::ToggleExpanded`].
///
/// Goes in the row's first (gutter) column, as the cell content:
///
/// ```ignore
/// let cols = vec![
///     DataColumn::fixed("", tokens::MIN_TARGET_SIZE),  // disclosure gutter
///     DataColumn::fill("Reference", 1.0),
///     // …
/// ];
/// data_row(data_row_key("parts", &p.mpn), &cols, [
///     data_expander(data_expand_key("parts", &p.mpn), state.is_expanded(&p.mpn)),
///     text(&p.mpn).mono(),
///     // …
/// ])
/// ```
///
/// It is focusable in its own right, nested inside the focusable row —
/// pointer hits resolve innermost-first, so clicking the chevron
/// toggles while clicking anywhere else in the row activates it. It
/// carries no `hit_overflow`: an expanded target here would reach into
/// the neighbouring cell and collide with the row's own. That is also
/// why the gutter column has to be at least
/// [`tokens::MIN_TARGET_SIZE`] wide — with no outset to rescue it, a
/// narrower gutter is a sub-24px interactive target sitting inside the
/// row's own clearance circle (`SmallHitTarget`).
///
/// It announces itself as an ARIA disclosure button: a stable name
/// plus `aria-expanded`, so a screen reader reads the state change
/// rather than a bare chevron glyph whose name would flip under it.
#[track_caller]
pub fn data_expander(key: impl Into<String>, expanded: bool) -> El {
    icon(if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .at_loc(Location::caller())
    .icon_size(tokens::ICON_XS)
    .color(tokens::MUTED_FOREGROUND)
    .key(key)
    .role(Role::Button)
    .aria_label("Row details")
    .aria_expanded(expanded)
    .focusable()
    .cursor(Cursor::Pointer)
    .focus_ring_inside()
}

/// A column-aligned footer row — totals, counts, a summary line.
///
/// Pass it to [`DataTableOpts::footer`], which places it *outside* the
/// scroll region so it stays pinned to the table's bottom edge while
/// the body scrolls under it. Cells are stamped from the same column
/// spec as the body, so a total sits under its column. The row carries
/// a top rule and no hover response.
#[track_caller]
pub fn data_footer_row<I, E>(cols: &[DataColumn], cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let geometry = column_geometry(cols);
    let cells: Vec<El> = cells
        .into_iter()
        .enumerate()
        .map(|(i, content)| stamp_column(table_cell(content), geometry.get(i)))
        .collect();
    table_row(cells)
        .at_loc(Location::caller())
        .border_t()
        .no_hover()
}

// ---------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------

/// A data table over an already-built row list — sticky header, an
/// internal scroll over the body, vertical arrow-key navigation across
/// the rows.
///
/// `rows` is the flat visible-row list: [`data_row`]s, with
/// [`data_group_row`]s and [`data_detail_row`]s interleaved wherever
/// the app wants them.
///
/// The widget needs a bounded height — see the module docs. For sort
/// indicators, a sticky footer, or a headerless table, use
/// [`data_table_with`]; for row sets too large to build every frame,
/// [`data_table_virtual`].
#[track_caller]
pub fn data_table<I, E>(key: impl Into<String>, cols: &[DataColumn], rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    data_table_with(key, cols, DataTableOpts::default(), rows).at_loc(Location::caller())
}

/// [`data_table`] with its optional parts — sort indicator, sticky
/// footer, headerless mode. See [`DataTableOpts`].
#[track_caller]
pub fn data_table_with<I, E>(
    key: impl Into<String>,
    cols: &[DataColumn],
    opts: DataTableOpts,
    rows: I,
) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let key = key.into();
    // The rows are the arrow-nav group's members, so the flag goes on
    // their direct parent — the same "flat list is load-bearing" rule
    // `tree` follows.
    let body = scroll([table_body(rows).arrow_nav_siblings()])
        .key(data_body_key(&key))
        .scrollbar_gutter();
    assemble(&key, cols, opts, body).at_loc(Location::caller())
}

/// A virtualized data table over `count` visible rows — the same
/// widget, with the body realized lazily through
/// [`virtual_list_dyn`](crate::tree::virtual_list_dyn).
///
/// - `row_key(i)` — a **stable** identity for the row at `i`. The
///   dynamic list keys its measurement cache and its scroll anchor off
///   these, so they must follow the data, not the index (the same rule
///   as [`data_row_key`]'s id). Reusing [`data_row_key`] for both is
///   the easy path.
/// - `build_row(i)` — builds visible row `i`: a [`data_row`], a
///   [`data_group_row`], or a [`data_detail_row`]. Called only for rows
///   the viewport intersects, every frame they are visible, so it must
///   be cheap and free of side effects.
///
/// Both closures are `Send + Sync + 'static` (the list holds them
/// across frames), so they cannot borrow the column slice — clone it
/// in:
///
/// ```ignore
/// let cols = columns();
/// let rows = self.rows.clone();          // or an Arc
/// let build_cols = cols.clone();
/// data_table_virtual(
///     "signals",
///     &cols,
///     rows.len(),
///     |i| format!("signals:row:{i}"),
///     move |i| {
///         let r = &rows[i];
///         data_row(data_row_key("signals", &r.id), &build_cols, [
///             text(&r.name).mono(),
///             text(&r.value).tabular_numerals(),
///         ])
///     },
/// )
/// ```
///
/// Flattening: index `i` counts *visible rows*, so an app with group
/// headers or expanded detail panels builds the projection itself —
/// typically a `Vec<VisibleRow>` enum computed once per frame and
/// indexed by the closure. That is the same trade
/// [`tree`](crate::widgets::tree::tree) makes, and it is what keeps
/// expansion state in the app.
///
/// Arrow-key navigation covers the realized window only; see the module
/// docs.
#[track_caller]
pub fn data_table_virtual<K, F>(
    key: impl Into<String>,
    cols: &[DataColumn],
    count: usize,
    row_key: K,
    build_row: F,
) -> El
where
    K: Fn(usize) -> String + Send + Sync + 'static,
    F: Fn(usize) -> El + Send + Sync + 'static,
{
    data_table_virtual_with(
        key,
        cols,
        DataTableOpts::default(),
        count,
        row_key,
        build_row,
    )
    .at_loc(Location::caller())
}

/// [`data_table_virtual`] with its optional parts. See
/// [`DataTableOpts`] — and in particular
/// [`DataTableOpts::row_estimate`], which the virtual path uses to
/// position the window and size the scrollbar thumb before a row has
/// been measured.
#[track_caller]
pub fn data_table_virtual_with<K, F>(
    key: impl Into<String>,
    cols: &[DataColumn],
    opts: DataTableOpts,
    count: usize,
    row_key: K,
    build_row: F,
) -> El
where
    K: Fn(usize) -> String + Send + Sync + 'static,
    F: Fn(usize) -> El + Send + Sync + 'static,
{
    let key = key.into();
    let estimate = opts.row_estimate.unwrap_or(DEFAULT_ROW_ESTIMATE);
    // `table_body` puts the rules on for the plain path; the virtual
    // body has no list to walk, so the wrapper stamps each realized
    // row instead — last row unruled, exactly as `tbody tr:last-child`.
    let body = virtual_list_dyn(count, estimate, row_key, move |i| {
        let row = build_row(i);
        if i + 1 < count { row.border_b() } else { row }
    })
    .key(data_body_key(&key))
    .arrow_nav(ArrowNav::Vertical)
    .scrollbar_gutter();
    assemble(&key, cols, opts, body).at_loc(Location::caller())
}

/// The routed key of a table's internal scroll region —
/// `{key}:body`. Exposed because that is the node a
/// [`ScrollRequest`](crate::scroll::ScrollRequest) has to name to
/// scroll a row into view.
pub fn data_body_key(key: &str) -> String {
    format!("{key}:body")
}

// ---------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------

/// Column of `[header?, body, footer?]`. The header and footer are
/// siblings of the scroll rather than children of it — that, and
/// nothing else, is what makes them sticky.
fn assemble(key: &str, cols: &[DataColumn], opts: DataTableOpts, body: El) -> El {
    let mut children: Vec<El> = Vec::with_capacity(3);

    if !opts.headerless {
        children.push(
            table_header([header_row(key, cols, opts.sort)])
                // The body reserves a stable scrollbar gutter (rows are
                // focusable, and a thumb over a focused row is the
                // `ScrollbarObscuresFocusable` finding). The header is
                // outside that scroll, so it has to reserve the same
                // strip or every column would sit a thumb-width left of
                // its data.
                .pr(tokens::SCROLLBAR_GUTTER),
        );
    }

    children.push(body);

    if let Some(footer) = opts.footer {
        children.push(
            El::new(Kind::Custom("data_table_footer"))
                .children([footer])
                .axis(Axis::Column)
                .align(Align::Stretch)
                .width(Size::Fill(1.0))
                .height(Size::Hug)
                // Same gutter argument as the header.
                .pr(tokens::SCROLLBAR_GUTTER),
        );
    }

    El::new(Kind::Custom("data_table"))
        .children(children)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        // The widget owns a scroll, so it needs a bounded height; a
        // caller who wants a different one says so and this defers.
        .default_height(Size::Fill(1.0))
        // Like `table()`: a too-wide cell must not bleed past the
        // table's edge. Rows and header cells all draw their focus ring
        // inside, so the scissor never cuts one.
        .clip()
}

/// The header row: one head cell per column, sortable ones keyed,
/// focusable, and carrying the direction indicator.
fn header_row(key: &str, cols: &[DataColumn], sort: Option<Sort>) -> El {
    let cells: Vec<El> = cols
        .iter()
        .enumerate()
        .map(|(i, col)| header_cell(key, i, col, sort))
        .collect();
    table_row(cells)
}

fn header_cell(key: &str, index: usize, col: &DataColumn, sort: Option<Sort>) -> El {
    let label = text(col.header.clone())
        .ellipsis()
        .width(Size::Fill(1.0))
        .text_align(col.column.align);

    if !col.sortable {
        return table_head_el(label).width(col.column.width);
    }

    // The indicator's box is reserved whether or not this column is the
    // sorted one, so activating a header doesn't shift its label — the
    // same argument `tree_item`'s empty disclosure gutter makes.
    let indicator = match sort {
        Some(sort) if sort.column == index => icon(match sort.direction {
            SortDirection::Ascending => IconName::ChevronUp,
            SortDirection::Descending => IconName::ChevronDown,
        })
        .icon_size(tokens::ICON_XS)
        .color(tokens::FOREGROUND),
        _ => El::new(Kind::Custom("data_table_sort_gutter"))
            .width(Size::Fixed(tokens::ICON_XS))
            .height(Size::Fixed(tokens::ICON_XS)),
    };

    table_head_el(
        row([label, indicator])
            .align(Align::Center)
            .gap(tokens::SPACE_1),
    )
    .width(col.column.width)
    .key(data_sort_key(key, index))
    .focusable()
    .cursor(Cursor::Pointer)
    // Header cells stack flush in a zero-gap row inside the table's
    // clip; an outward ring would be both scissored and overpainted by
    // the neighbouring cell.
    .focus_ring_inside()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;
    use crate::event::{UiEvent, UiEventKind};
    use crate::layout::layout;
    use crate::state::UiState;

    fn cols() -> Vec<DataColumn> {
        vec![
            DataColumn::fill("Reference", 1.2).sortable(),
            DataColumn::fill("Value", 1.0),
            DataColumn::fixed("Footprint", 120.0),
            DataColumn::ch("Stock", 7.0).align_end().sortable(),
        ]
    }

    fn body_rows(cols: &[DataColumn], n: usize) -> Vec<El> {
        (0..n)
            .map(|i| {
                data_row(
                    data_row_key("parts", &format!("R{i}")),
                    cols,
                    [
                        text(format!("R{i}")),
                        text("10k"),
                        text("0603"),
                        text("1,240").tabular_numerals(),
                    ],
                )
            })
            .collect()
    }

    // ---- column model ------------------------------------------------

    #[test]
    fn data_column_carries_table_column_geometry_verbatim() {
        // The point of holding a `TableColumn` rather than re-minting
        // width/align: the geometry a `data_table` renders from is
        // *the same value* a plain `table` renders from, so the two
        // paths can share one spec.
        let c = DataColumn::fill("Value", 1.4).align_end().sortable();
        assert_eq!(c.column, TableColumn::fill(1.4).align_end());
        assert_eq!(c.header, "Value");
        assert!(c.sortable);

        assert_eq!(
            column_geometry(&cols()),
            vec![
                TableColumn::fill(1.2),
                TableColumn::fill(1.0),
                TableColumn::fixed(120.0),
                TableColumn::ch(7.0).align_end(),
            ],
        );
    }

    #[test]
    fn header_and_body_cells_share_one_column_spec() {
        let cols = cols();
        let table = data_table("parts", &cols, body_rows(&cols, 3));
        let header = &table.children[0].children[0];
        let body = &table.children[1].children[0];

        assert_eq!(header.children.len(), cols.len());
        for (i, col) in cols.iter().enumerate() {
            assert_eq!(
                header.children[i].width, col.column.width,
                "header cell {i}"
            );
            for (r, row) in body.children.iter().enumerate() {
                assert_eq!(row.children[i].width, col.column.width, "row {r} cell {i}");
            }
        }
    }

    // ---- sticky anatomy ----------------------------------------------

    #[test]
    fn header_and_footer_are_siblings_of_the_scroll_not_children() {
        // The whole design: sticky-by-composition. If the header ever
        // moves inside the scroll it scrolls away, and the widget is
        // just `scroll([table(...)])` again.
        let cols = cols();
        let table = data_table_with(
            "parts",
            &cols,
            DataTableOpts::default()
                .footer(data_footer_row(&cols, [text("Total"), text("18,204")])),
            body_rows(&cols, 3),
        );

        assert_eq!(table.kind, Kind::Custom("data_table"));
        assert_eq!(table.axis, Axis::Column);
        assert_eq!(table.children.len(), 3);
        assert_eq!(table.children[0].kind, Kind::Custom("table_header"));
        assert_eq!(table.children[1].kind, Kind::Scroll);
        assert_eq!(table.children[2].kind, Kind::Custom("data_table_footer"));

        // The scroll holds the body and nothing else.
        let scroll = &table.children[1];
        assert_eq!(scroll.children.len(), 1);
        assert_eq!(scroll.children[0].kind, Kind::Custom("table_body"));
        assert_eq!(scroll.key.as_deref(), Some("parts:body"));
        assert_eq!(data_body_key("parts"), "parts:body");
    }

    #[test]
    fn headerless_drops_the_header_row() {
        let cols = cols();
        let table = data_table_with(
            "parts",
            &cols,
            DataTableOpts::default().headerless(),
            body_rows(&cols, 2),
        );
        assert_eq!(table.children.len(), 1);
        assert_eq!(table.children[0].kind, Kind::Scroll);
    }

    #[test]
    fn header_and_footer_reserve_the_bodys_scrollbar_gutter() {
        // Column alignment across the sticky seam. The body reserves a
        // stable gutter so the thumb never covers a focusable row; the
        // header and footer are outside that scroll and must reserve
        // the same strip or every column drifts by a thumb width.
        let cols = cols();
        let table = data_table_with(
            "parts",
            &cols,
            DataTableOpts::default().footer(data_footer_row(&cols, [text("Total")])),
            body_rows(&cols, 3),
        );
        assert!(table.children[1].scrollbar_gutter, "body reserves a gutter");
        assert_eq!(table.children[0].padding.right, tokens::SCROLLBAR_GUTTER);
        assert_eq!(table.children[2].padding.right, tokens::SCROLLBAR_GUTTER);
    }

    #[test]
    fn a_sticky_header_stays_put_while_the_body_scrolls() {
        // The behavioural claim, measured: lay the table out, scroll the
        // body to its end, and the header's rect must not have moved
        // while the first body row's has.
        let cols = cols();
        let mut table = data_table("parts", &cols, body_rows(&cols, 60));
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 720.0, 300.0);
        crate::layout::assign_ids(&mut table);
        layout(&mut table, &mut state, viewport);

        let header_y = table.children[0].computed_rect.y;
        let first_row_y = table.children[1].children[0].children[0].computed_rect.y;

        let scroll_id = table.children[1].computed_id.clone();
        state.set_scroll_offset(&*scroll_id, 400.0);
        layout(&mut table, &mut state, viewport);

        assert_eq!(
            table.children[0].computed_rect.y, header_y,
            "the header is outside the scroll, so it cannot move"
        );
        assert!(
            table.children[1].children[0].children[0].computed_rect.y < first_row_y,
            "the body rows scrolled under it"
        );
    }

    // ---- rows ---------------------------------------------------------

    #[test]
    fn rows_are_keyed_focusable_and_ring_inside() {
        let cols = cols();
        let r = data_row(data_row_key("parts", &"R14"), &cols, [text("R14")]);
        assert_eq!(r.key.as_deref(), Some("parts:row:R14"));
        assert!(r.focusable);
        assert_eq!(r.cursor, Some(Cursor::Pointer));
        assert_eq!(r.focus_ring_placement, FocusRingPlacement::Inside);
        assert_eq!(r.metrics_role, Some(crate::metrics::MetricsRole::TableRow));
    }

    #[test]
    fn selection_and_disablement_compose_from_the_stock_chainables() {
        // No `selected: bool` parameter — the house style already has
        // one spelling for this, and a widget that mints a second one
        // is a vocabulary fork.
        let cols = cols();
        let selected = data_row("parts:row:a", &cols, [text("a")]).selected();
        assert_eq!(selected.surface_role, SurfaceRole::Selected);
        assert!(selected.fill.is_some());
        assert!(selected.focusable, "a selected row is still focusable");

        let disabled = data_row("parts:row:b", &cols, [text("b")]).disabled();
        assert!(!disabled.focusable, "disabled leaves the focus order");
        assert!(disabled.block_pointer);
    }

    #[test]
    fn group_rows_are_landmarks_not_controls() {
        let g = data_group_row("Passives");
        assert!(!g.focusable);
        assert!(g.no_hover);
        assert_eq!(g.fill, Some(tokens::MUTED));
        assert_eq!(g.metrics_role, Some(crate::metrics::MetricsRole::TableRow));
        assert_eq!(g.children.len(), 1, "one full-width cell");
        assert_eq!(g.children[0].text.as_deref(), Some("Passives"));
    }

    #[test]
    fn detail_rows_span_every_column() {
        let d = data_detail_row(column([text("Datasheet"), text("Yageo RC-L")]));
        assert!(!d.focusable);
        assert!(d.no_hover);
        assert_eq!(d.children.len(), 1, "one cell, not one per column");
        assert_eq!(d.children[0].width, Size::Fill(1.0));
        assert_eq!(d.children[0].kind, Kind::Custom("data_detail"));
    }

    #[test]
    fn detail_panels_take_the_rungs_cell_padding() {
        // The panel is inset in line with the cells above it, because
        // the metrics pass stamps every child of a `TableRow` row.
        let mut d = data_detail_row(text("body"));
        Theme::default()
            .with_default_component_size(ComponentSize::Xs)
            .apply_metrics(&mut d);
        assert_eq!(d.children[0].padding, Sides::xy(8.0, 4.0));
    }

    #[test]
    fn expander_is_a_nested_focusable_with_no_hit_overflow() {
        // Nested hit targets resolve innermost-first, so the chevron
        // toggles and the rest of the row activates. An expanded hit
        // target here would reach into the next cell and collide with
        // the row's own (`HitOverflowCollision`).
        let open = data_expander(data_expand_key("parts", &"R14"), true);
        let shut = data_expander(data_expand_key("parts", &"R14"), false);
        assert_eq!(open.key.as_deref(), Some("parts:expand:R14"));
        assert!(open.focusable);
        assert_eq!(open.focus_ring_placement, FocusRingPlacement::Inside);
        assert_eq!(open.hit_overflow, Sides::zero());
        assert_eq!(
            open.icon,
            Some(crate::icons::svg::IconSource::Builtin(
                IconName::ChevronDown
            ))
        );
        assert_eq!(
            shut.icon,
            Some(crate::icons::svg::IconSource::Builtin(
                IconName::ChevronRight
            ))
        );
    }

    #[test]
    fn footer_cells_are_stamped_from_the_same_column_spec() {
        let cols = cols();
        let f = data_footer_row(&cols, [text("Total"), text(""), text(""), text("18,204")]);
        assert!(f.no_hover);
        assert_eq!(f.border.as_deref().unwrap().widths, Sides::top(1.0));
        for (i, col) in cols.iter().enumerate() {
            assert_eq!(f.children[i].width, col.column.width, "footer cell {i}");
        }
        assert_eq!(
            f.children[3].text_align,
            TextAlign::End,
            "the numeric column's alignment carries to the total"
        );
    }

    // ---- sort ---------------------------------------------------------

    #[test]
    fn sortable_headers_are_keyed_and_activatable_others_are_not() {
        let cols = cols();
        let table = data_table("parts", &cols, body_rows(&cols, 1));
        let header = &table.children[0].children[0];

        assert_eq!(header.children[0].key.as_deref(), Some("parts:sort:0"));
        assert!(header.children[0].focusable);
        assert_eq!(header.children[0].cursor, Some(Cursor::Pointer));
        assert_eq!(
            header.children[0].focus_ring_placement,
            FocusRingPlacement::Inside
        );

        assert_eq!(
            header.children[1].key, None,
            "plain columns aren't triggers"
        );
        assert!(!header.children[1].focusable);

        assert_eq!(header.children[3].key.as_deref(), Some("parts:sort:3"));
        assert_eq!(data_sort_key("parts", 3), "parts:sort:3");
    }

    #[test]
    fn the_sort_indicator_reads_the_callers_state_and_reserves_its_box() {
        fn indicator(sort: Option<Sort>, column: usize) -> El {
            let cols = cols();
            let table = data_table_with(
                "parts",
                &cols,
                DataTableOpts::default().sort(sort),
                Vec::<El>::new(),
            );
            table.children[0].children[0].children[column].children[1].clone()
        }

        // Unsorted: an empty gutter of the icon's box, so activating a
        // header never shifts its label.
        let none = indicator(None, 0);
        assert_eq!(none.kind, Kind::Custom("data_table_sort_gutter"));
        assert_eq!(none.width, Size::Fixed(tokens::ICON_XS));

        let asc = indicator(Some(Sort::ascending(0)), 0);
        assert_eq!(
            asc.icon,
            Some(crate::icons::svg::IconSource::Builtin(IconName::ChevronUp))
        );
        let desc = indicator(Some(Sort::descending(0)), 0);
        assert_eq!(
            desc.icon,
            Some(crate::icons::svg::IconSource::Builtin(
                IconName::ChevronDown
            ))
        );

        // Only the sorted column carries a glyph.
        let other = indicator(Some(Sort::ascending(0)), 3);
        assert_eq!(other.kind, Kind::Custom("data_table_sort_gutter"));
    }

    // ---- events -------------------------------------------------------

    #[test]
    fn classify_routes_rows_expanders_and_headers() {
        assert_eq!(
            classify_event(&UiEvent::synthetic_click("parts:row:R14"), "parts"),
            Some(DataTableAction::Activate("R14")),
        );
        assert_eq!(
            classify_event(&UiEvent::synthetic_click("parts:expand:R14"), "parts"),
            Some(DataTableAction::ToggleExpanded("R14")),
        );
        assert_eq!(
            classify_event(&UiEvent::synthetic_click("parts:sort:2"), "parts"),
            Some(DataTableAction::Sort(2)),
        );
        // Another table's rows, and this table's own scroll, are not ours.
        assert_eq!(
            classify_event(&UiEvent::synthetic_click("nets:row:R14"), "parts"),
            None,
        );
        assert_eq!(
            classify_event(&UiEvent::synthetic_click("parts:body"), "parts"),
            None,
        );
    }

    #[test]
    fn classify_ignores_non_activation_events() {
        let mut hover = UiEvent::synthetic_click("parts:row:R14");
        hover.kind = UiEventKind::PointerEnter;
        assert_eq!(classify_event(&hover, "parts"), None);
    }

    #[test]
    fn apply_event_selects_toggles_and_records_the_sort_request() {
        let mut state = DataTableState::default();

        assert!(apply_event(
            &mut state,
            &UiEvent::synthetic_click("parts:row:R14"),
            "parts"
        ));
        assert!(state.is_selected("R14"));
        assert!(!state.is_selected("R15"));

        assert!(apply_event(
            &mut state,
            &UiEvent::synthetic_click("parts:expand:R14"),
            "parts"
        ));
        assert!(state.is_expanded("R14"));
        assert!(apply_event(
            &mut state,
            &UiEvent::synthetic_click("parts:expand:R14"),
            "parts"
        ));
        assert!(!state.is_expanded("R14"), "a second activation collapses");

        // First activation of a column sorts it ascending; the next
        // reverses it; a different column starts over ascending.
        assert!(apply_event(
            &mut state,
            &UiEvent::synthetic_click("parts:sort:3"),
            "parts"
        ));
        assert_eq!(state.sort, Some(Sort::ascending(3)));
        apply_event(
            &mut state,
            &UiEvent::synthetic_click("parts:sort:3"),
            "parts",
        );
        assert_eq!(state.sort, Some(Sort::descending(3)));
        apply_event(
            &mut state,
            &UiEvent::synthetic_click("parts:sort:0"),
            "parts",
        );
        assert_eq!(state.sort, Some(Sort::ascending(0)));

        // Selection survives the re-sort: rows are identified by id.
        assert!(state.is_selected("R14"));
    }

    #[test]
    fn apply_event_ignores_unrelated_keys() {
        let mut state = DataTableState::default();
        assert!(!apply_event(
            &mut state,
            &UiEvent::synthetic_click("save"),
            "parts"
        ));
        assert_eq!(state, DataTableState::default());
    }

    // ---- navigation ---------------------------------------------------

    #[test]
    fn body_rows_form_a_vertical_arrow_nav_group() {
        let cols = cols();
        let mut table = data_table("parts", &cols, body_rows(&cols, 5));
        let mut state = UiState::new();
        crate::layout::assign_ids(&mut table);
        layout(&mut table, &mut state, Rect::new(0.0, 0.0, 720.0, 600.0));

        let order = crate::focus::focus_order(&table);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "parts:sort:0",
                "parts:sort:3",
                "parts:row:R0",
                "parts:row:R1",
                "parts:row:R2",
                "parts:row:R3",
                "parts:row:R4",
            ],
            "tab order walks the sortable headers, then the rows",
        );

        let row0 = order
            .iter()
            .find(|t| t.key == "parts:row:R0")
            .expect("row 0 focusable");
        let (mode, members) =
            crate::focus::arrow_nav_group(&table, &row0.node_id).expect("rows form a nav group");
        assert_eq!(mode, ArrowNav::Vertical);
        assert_eq!(members.len(), 5);
        assert_eq!(members[1].key, "parts:row:R1");
    }

    // ---- virtualization ------------------------------------------------

    #[test]
    fn the_virtual_body_is_a_virtual_list_with_the_same_chrome() {
        let cols = cols();
        let build_cols = cols.clone();
        let table = data_table_virtual(
            "signals",
            &cols,
            10_000,
            |i| format!("signals:row:{i}"),
            move |i| {
                data_row(
                    data_row_key("signals", &i),
                    &build_cols,
                    [text(format!("net{i}")), text("10k")],
                )
            },
        );

        assert_eq!(table.children.len(), 2, "header + virtual body");
        assert_eq!(table.children[0].kind, Kind::Custom("table_header"));
        let body = &table.children[1];
        assert_eq!(body.kind, Kind::VirtualList);
        assert_eq!(body.key.as_deref(), Some("signals:body"));
        assert_eq!(body.arrow_nav, Some(ArrowNav::Vertical));
        assert!(body.scrollbar_gutter);
    }

    #[test]
    fn a_ten_thousand_row_table_realizes_only_a_screenful() {
        let cols = cols();
        let build_cols = cols.clone();
        let mut table = data_table_virtual(
            "signals",
            &cols,
            10_000,
            |i| format!("signals:row:{i}"),
            move |i| {
                data_row(
                    data_row_key("signals", &i),
                    &build_cols,
                    [text(format!("net{i}")), text("10k"), text("—"), text("0")],
                )
            },
        );
        let mut state = UiState::new();
        crate::layout::assign_ids(&mut table);
        layout(&mut table, &mut state, Rect::new(0.0, 0.0, 720.0, 400.0));

        let realized = table.children[1].children.len();
        assert!(
            (1..200).contains(&realized),
            "expected a viewport's worth of rows, realized {realized}",
        );
        // Realized rows carry the body rules the plain path gets from
        // `table_body`.
        assert!(
            table.children[1].children[0].border.is_some(),
            "realized rows carry the row rule",
        );
    }

    // ---- density -------------------------------------------------------

    #[test]
    fn the_row_estimate_matches_a_real_rows_pitch_at_every_rung() {
        // `estimated_row_height` is a hardcoded mirror of the control
        // ladder, so measure a laid-out table against it rather than
        // trusting the table twice.
        for size in [
            ComponentSize::Xxs,
            ComponentSize::Xs,
            ComponentSize::Sm,
            ComponentSize::Md,
            ComponentSize::Lg,
        ] {
            let cols = cols();
            let mut table = data_table("parts", &cols, body_rows(&cols, 4));
            let theme = Theme::default().with_default_component_size(size);
            theme.apply_metrics(&mut table);
            let mut state = UiState::new();
            crate::layout::assign_ids(&mut table);
            layout(&mut table, &mut state, Rect::new(0.0, 0.0, 720.0, 600.0));

            let rows = &table.children[1].children[0].children;
            let pitch = rows[1].computed_rect.y - rows[0].computed_rect.y;
            assert!(
                (pitch - estimated_row_height(size)).abs() < 1e-3,
                "{size:?}: measured pitch {pitch}, estimate {}",
                estimated_row_height(size),
            );
        }
        assert_eq!(
            DEFAULT_ROW_ESTIMATE,
            estimated_row_height(ComponentSize::Sm)
        );
    }

    #[test]
    fn an_xs_table_lands_on_the_workbench_row_pitch() {
        // The density claim from `docs/WORKBENCH_VISION.md`: the dense
        // profile's rows are 29px, against the reference corpus's
        // `tbody tr { height: 28px }` plus its rule.
        assert_eq!(estimated_row_height(ComponentSize::Xs), 29.0);
    }

    // ---- lint ----------------------------------------------------------

    /// The composite claim: a scrolling table of focusable rows, a
    /// sortable header, a group row, a detail panel and a sticky footer
    /// must all be lint-clean together. Two findings this specifically
    /// guards: `FocusRingObscured` (rows and header cells live inside
    /// two nested clips) and `ScrollbarObscuresFocusable` (full-width
    /// focusable rows under a rendered thumb — the reason the body
    /// reserves a gutter and the header pads to match).
    #[test]
    fn a_full_data_table_is_lint_clean() {
        let cols = cols();
        let mut rows = vec![data_group_row("Passives")];
        rows.extend(body_rows(&cols, 40));
        rows.insert(
            2,
            data_detail_row(
                column([text("Thick film chip resistor, 1/10 W, ±1%.").label()])
                    .width(Size::Fill(1.0)),
            ),
        );

        let mut root = crate::tree::column([data_table_with(
            "parts",
            &cols,
            DataTableOpts::default()
                .sort(Some(Sort::descending(3)))
                .footer(data_footer_row(
                    &cols,
                    [text("40 parts"), text(""), text(""), text("18,204")],
                )),
            rows,
        )])
        .padding(Sides::all(tokens::SPACE_4))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));

        let bundle = crate::bundle::artifact::render_bundle_themed(
            &mut root,
            Rect::new(0.0, 0.0, 900.0, 420.0),
            &Theme::default().with_default_component_size(ComponentSize::Xs),
        );
        assert!(
            bundle.lint.findings.is_empty(),
            "{} finding(s):\n{}",
            bundle.lint.findings.len(),
            bundle.lint.text(),
        );
    }

    /// Control for the guard above: without the reserved gutter the
    /// focusable rows sit under the scrollbar thumb, which is exactly
    /// the finding the composition is shaped to avoid. If this ever
    /// stops firing, the clean assertion above has stopped meaning
    /// anything.
    #[test]
    fn without_the_gutter_a_scrolling_table_trips_the_scrollbar_lint() {
        use crate::bundle::lint::FindingKind;

        let cols = cols();
        let mut root = crate::tree::column([scroll([table_body(body_rows(&cols, 60))])
            .key("naive")
            .height(Size::Fill(1.0))])
        .padding(Sides::all(tokens::SPACE_4))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));

        let bundle = crate::bundle::artifact::render_bundle_themed(
            &mut root,
            Rect::new(0.0, 0.0, 900.0, 420.0),
            &Theme::default().with_default_component_size(ComponentSize::Xs),
        );
        assert!(
            bundle
                .lint
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::ScrollbarObscuresFocusable),
            "control case must trip ScrollbarObscuresFocusable; got:\n{}",
            bundle.lint.text(),
        );
    }
}
