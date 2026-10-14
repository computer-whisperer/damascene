//! Table — shadcn-shaped table anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `table([table_header([table_row([...])]), table_body([...])])`.
//! Rows carry the theme-facing table metrics; `table_header` promotes
//! direct `table_row` children from body-row metrics to header metrics.
//!
//! Plain rows are static data, like an HTML `<tr>`. A row the user
//! should be able to select or open is a [`table_row_keyed`] — the
//! `<tr tabindex="0">` shape (issue #144): Tab reaches it, ↑ / ↓ and
//! Home / End step between the body's keyed rows, and Enter / Space
//! or a click emit `Activate` / `Click` routed to its key.
//!
//! # Column geometry
//!
//! Cell constructors ([`table_cell`], [`table_head`]) hardcode
//! `Size::Fill(1.0)`, so every column of a hand-built table is equally
//! wide. Giving a table real column proportions therefore meant
//! restating a weight on the header cell *and* on the same-position cell
//! of every body row — the measured failure mode is a local
//! `cell(content, weight)` / `head(label, weight)` helper pair minted in
//! each app, with the weights duplicated across the header row and the
//! body-row builder.
//!
//! [`TableColumn`] is the one description both halves render from:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! const COLS: &[TableColumn] = &[
//!     TableColumn::fill(0.6),               // Reference
//!     TableColumn::fill(1.1),               // Value
//!     TableColumn::fixed(120.0),            // Footprint
//!     TableColumn::fill(0.6).align_end(),   // Stock
//! ];
//!
//! table([
//!     table_header([table_header_cells(COLS, ["Reference", "Value", "Footprint", "Stock"])]),
//!     table_body(parts.iter().enumerate().map(|(i, p)| {
//!         table_row_cells(
//!             format!("row:{i}"),
//!             COLS,
//!             [
//!                 text(p.reference).mono(),
//!                 text(p.value),
//!                 text(p.footprint).muted(),
//!                 text(p.stock).tabular_numerals(),
//!             ],
//!         )
//!     })),
//! ])
//! ```
//!
//! Oracle: TanStack Table / shadcn's `DataTable` recipe, whose
//! `columns` array is exactly this — one positional spec the header and
//! the rows both read. Deliberately geometry-only: no sort state, no
//! selection model, no accessors.
//!
//! # When to reach for [`data_table`] instead
//!
//! This module is anatomy without behaviour, and it owns no scroll: a
//! `table` inside `scroll([...])` scrolls its header away with the
//! rows. [`data_table`](crate::widgets::data_table) is the sibling that
//! adds the behaving half — sticky header and footer (by keeping them
//! outside an internal scroll), keyed focusable rows with vertical
//! arrow-key navigation, expandable detail rows, group rows, a sort
//! affordance, and a virtualized variant for large row sets. Reach for
//! plain `table` for a short, static, hug-height table; reach for
//! `data_table` as soon as the rows are data the user acts on.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use super::text::text;
use crate::a11y::Role;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

/// Table root — a full-width clipped column holding [`table_header`]
/// and [`table_body`], like an HTML `<table>`.
///
/// The `.clip()` is the scissor that keeps a too-wide cell from
/// bleeding past the table's edge, and it applies to the focus ring
/// too: a focusable row's outward ring would paint into a band the
/// scissor cuts away. [`table_row`] therefore draws its ring
/// [`FocusRingPlacement::Inside`], which is what makes a table of
/// focusable rows keyboard-navigable instead of mouse-only.
#[track_caller]
pub fn table<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("table"))
        .at_loc(Location::caller())
        .role(Role::Table)
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
        .clip()
}

/// Header section (like `<thead>`). Direct [`table_row`] children are
/// promoted from body-row metrics to header metrics and squared off.
#[track_caller]
pub fn table_header<I, E>(rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut header = El::new(Kind::Custom("table_header"))
        .at_loc(Location::caller())
        .children(rows)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch);

    // Promote `table_row(...)` children from body-row metrics to header
    // metrics. Table chrome lives on the cells, so rows stay hug-height
    // and stretch their children vertically.
    for row in &mut header.children {
        if row.metrics_role == Some(MetricsRole::TableRow) {
            row.metrics_role = Some(MetricsRole::TableHeader);
            if row.radius_origin == RadiusOrigin::ThemeDefault {
                row.radius = crate::tree::Corners::ZERO;
            }
        }
    }

    // shadcn's header row carries the same border-b as body rows —
    // it is what visually separates <thead> from <tbody>.
    if let Some(last) = header.children.pop() {
        header = header.child(last.border_b());
    }
    header
}

/// Body section holding the data [`table_row`]s, like `<tbody>`.
/// Every row except the last carries a 1px `.border_b()` — the shadcn
/// table is row-bordered (`tr` gets `border-b`, with `tbody
/// tr:last-child` unbordered), not a full cell grid.
///
/// The body is a vertical arrow-nav group: with focus on a
/// [`table_row_keyed`], ↑ / ↓ step to the adjacent keyed row and Home
/// / End jump to the first / last (the rules between rows aren't
/// focusable, so they're invisible to the stepping). Bodies of plain
/// rows have no members and the flag is inert.
#[track_caller]
pub fn table_body<I, E>(rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = rows.into_iter().map(Into::into).collect();
    let n = children.len();
    for row in children.iter_mut().take(n.saturating_sub(1)) {
        *row = std::mem::take(row).border_b();
    }
    El::new(Kind::Custom("table_body"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
        .arrow_nav(ArrowNav::Vertical)
}

/// A row of cells (like `<tr>`) carrying the theme's table-row
/// metrics; cells stretch vertically so their padded rows align.
///
/// The `Align::Stretch` that does the stretching also means a cell's
/// *text* sits at the top of a row taller than one line — visible as
/// soon as the row takes a fixed height. Add `.align(Align::Center)` to
/// the row for vertically centered cells.
///
/// Rows carry [`FocusRingPlacement::Inside`] so a `.focusable()` row is
/// keyboard-reachable inside [`table`]'s clip — see that function's
/// docs for why an outward ring cannot survive there. Non-focusable
/// rows paint no ring at all, so the stamp costs them nothing.
///
/// For a row whose cells follow a shared [`TableColumn`] spec, use
/// [`table_row_cells`] — it builds this row and stamps the column
/// geometry in one call.
///
/// The row is also where the theme's density lands: the metrics pass
/// stamps the resolved
/// [`ComponentSize`](crate::metrics::ComponentSize)'s cell padding onto
/// this row's children (see [`table_cell`]). `.size(...)` on the row
/// picks a rung for that row alone.
#[track_caller]
pub fn table_row<I, E>(cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(cells)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::TableRow)
        .role(Role::Row)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
        .default_gap(0.0)
        .default_radius(0.0)
        // Rows are flush by construction (zero gap, `border_b` seams)
        // and live inside `table`'s scissor, so an outward focus ring
        // is both clipped by the root and overpainted by the next row
        // — `FindingKind::FocusRingObscured` on every focusable row.
        // Same dense-flush trade `button_group`'s joined children make.
        .focus_ring_inside()
}

/// Geometry spec for one table column: how wide it is and how its
/// content aligns horizontally.
///
/// One array of these is the whole column model — [`table_header_cells`]
/// and [`table_row_cells`] stamp entry `i` onto the cell at position
/// `i`, so the header and every body row read from a single source and
/// the weights stop being duplicated per row. See the module docs for
/// the worked example.
///
/// Oracle: TanStack Table / shadcn `DataTable`'s `columns` array
/// (`docs/NAMING_ORACLE.md`). Deliberately the geometry half only —
/// there is no sort state, selection model, or accessor here.
///
/// Both fields are public, so a column that wants a `Size` the
/// constructors don't name writes it directly:
///
/// ```ignore
/// // Five tabular digit slots in the cell's own font.
/// TableColumn { width: Size::Ch(5.0), align: TextAlign::End }
/// ```
///
/// `align_end()` sets the *box* alignment only; a right-aligned numeric
/// column usually also wants `.tabular_numerals()` on its text, which
/// stays the caller's call because it depends on whether the values are
/// numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableColumn {
    /// The column's width, applied to the header cell and every body
    /// cell at this position. `Size::Fill(w)` shares leftover width by
    /// weight (the stock cells' `Fill(1.0)` is the equal-columns
    /// default); `Size::Fixed(px)` pins a gutter.
    pub width: Size,
    /// Horizontal alignment of the cell's content within that width.
    /// [`TextAlign::Start`] (the default) leaves the cell untouched.
    pub align: TextAlign,
}

impl Default for TableColumn {
    /// `Fill(1.0)`, start-aligned — the geometry the bare
    /// [`table_cell`] / [`table_head`] constructors already produce.
    fn default() -> Self {
        Self::fill(1.0)
    }
}

impl TableColumn {
    /// A column claiming `weight` of the leftover width, Tailwind's
    /// `flex-<n>` / CSS `flex-grow`. Weights are relative: `fill(2.0)`
    /// beside `fill(1.0)` is twice as wide.
    pub const fn fill(weight: f32) -> Self {
        Self {
            width: Size::Fill(weight),
            align: TextAlign::Start,
        }
    }

    /// A column pinned to `px` logical pixels, whatever the table's
    /// width — for gutters holding bounded content (a checkbox, a
    /// status dot, a fixed-format timestamp).
    pub const fn fixed(px: f32) -> Self {
        Self {
            width: Size::Fixed(px),
            align: TextAlign::Start,
        }
    }

    /// A column `n` tabular digits wide — the CSS `ch` unit
    /// ([`Size::Ch`]), resolved against the cell's own font.
    ///
    /// The numeric-gutter constructor: a five-digit reading holds the
    /// same width whether it currently reads `7` or `12,480`, so a
    /// column of live values stops twitching as the data changes.
    /// Pair it with `.align_end()` and `.tabular_numerals()` on the
    /// cell text.
    pub const fn ch(n: f32) -> Self {
        Self {
            width: Size::Ch(n),
            align: TextAlign::Start,
        }
    }

    /// Align this column's content to the leading edge (the default).
    /// Spelled out for symmetry in a `columns` array that mixes
    /// alignments.
    pub const fn align_start(mut self) -> Self {
        self.align = TextAlign::Start;
        self
    }

    /// Center this column's content.
    pub const fn align_center(mut self) -> Self {
        self.align = TextAlign::Center;
        self
    }

    /// Align this column's content to the trailing edge — the numeric
    /// column treatment. Pair with `.tabular_numerals()` on the cell
    /// text so the digits stack.
    pub const fn align_end(mut self) -> Self {
        self.align = TextAlign::End;
        self
    }
}

/// Header row (`<tr>` of `<th>`) built from a [`TableColumn`] spec —
/// each label becomes a [`table_head_el`] carrying its column's width
/// and alignment.
///
/// Goes straight inside [`table_header`], which promotes it to header
/// metrics and gives it the head/body separating rule:
///
/// ```ignore
/// table_header([table_header_cells(COLS, ["Name", "Owner", "Stock"])])
/// ```
///
/// Labels are `impl Into<El>`, so plain strings work (`&str`/`String`
/// convert to `text(...)`) and so does rich content — a sort caret, an
/// icon, a select-all checkbox — passed as an `El`. `table_head_el`'s
/// muted-label restyle applies either way.
///
/// **Count mismatch is tolerated, not asserted.** A cell past the end
/// of `cols` keeps the stock `Fill(1.0)` start-aligned geometry, and a
/// column past the end of the cells is simply unused. Widget
/// constructors here never panic on an authoring mistake — the result
/// renders, visibly wrong in the one column that drifted, which is what
/// the header/body pair makes obvious at a glance.
///
/// Without a column spec, build the row by hand from [`table_row`] and
/// [`table_head`] / [`table_head_el`].
#[track_caller]
pub fn table_header_cells<I, E>(cols: &[TableColumn], labels: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let cells: Vec<El> = labels
        .into_iter()
        .enumerate()
        .map(|(i, label)| stamp_column(table_head_el(label), cols.get(i)))
        .collect();
    table_row(cells)
}

/// Body row (`<tr>` of `<td>`) built from a [`TableColumn`] spec — each
/// item is wrapped in [`table_cell`]'s chrome and stamped with its
/// column's width and alignment.
///
/// The `key` is required because a table row is an identity: hover
/// state, selection, focus, and hit-testing all resolve through it, and
/// a row keyed off the data (`format!("part:{}", p.id)`) survives sort
/// and filter reordering in a way an index does not. Keyed rows also
/// pick up the stock hover response — shadcn's `hover:bg-muted/50` on
/// `<tr>`; add `.no_hover()` for a purely static table.
///
/// ```ignore
/// table_body(rows.iter().map(|r| {
///     table_row_cells(
///         format!("row:{}", r.id),
///         COLS,
///         [text(r.name), text(r.owner).muted(), badge(r.status)],
///     )
///     .focusable()
/// }))
/// ```
///
/// Pass *content*, not pre-built [`table_cell`]s — the cell chrome
/// (padding, ellipsis, square corners) is applied here, and re-wrapping
/// a cell would clobber a padding the caller had customized. A row that
/// needs per-cell chrome tweaks builds itself from [`table_row`] and
/// [`table_cell`] instead.
///
/// Count mismatch behaves exactly as in [`table_header_cells`]: extra
/// cells keep the stock `Fill(1.0)`, extra columns go unused, nothing
/// panics.
#[track_caller]
pub fn table_row_cells<I, E>(key: impl Into<String>, cols: &[TableColumn], cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let cells: Vec<El> = cells
        .into_iter()
        .enumerate()
        .map(|(i, content)| stamp_column(table_cell(content), cols.get(i)))
        .collect();
    table_row(cells).key(key)
}

/// Stamp one column's geometry onto the cell that sits in it.
///
/// Width is a plain override of the constructors' `Fill(1.0)`.
/// Alignment has to route by shape, because `table_cell` styles the
/// content *in place* rather than wrapping it — so the "cell" is
/// whatever the caller passed:
///
/// - text (`Kind::Text`, `Kind::Heading`, `Kind::Inlines`) aligns
///   within its own box via [`El::text_align`];
/// - a row container distributes along its main axis
///   ([`El::justify`]);
/// - a column container aligns on its cross axis ([`El::align`]).
///
/// Anything else (a `Kind::Badge` pill, an image) has no horizontal
/// alignment of its own — it stretches to the column, as the stock
/// cell already does. Wrap it in a `row([...])` to position it.
///
/// `pub(crate)` so [`data_table`](crate::widgets::data_table) can stamp
/// its footer row from the same routing rather than growing a second
/// copy that drifts. Nothing here is privileged — it composes only
/// public `El` builders, so a forked widget reproduces it verbatim.
pub(crate) fn stamp_column(cell: El, col: Option<&TableColumn>) -> El {
    let Some(col) = col else {
        return cell;
    };
    let cell = cell.width(col.width);
    if col.align == TextAlign::Start {
        // The constructors' default; nothing to say.
        return cell;
    }
    match cell.kind {
        Kind::Text | Kind::Heading | Kind::Inlines => cell.text_align(col.align),
        _ => match cell.axis {
            Axis::Row => cell.justify(match col.align {
                TextAlign::Start => Justify::Start,
                TextAlign::Center => Justify::Center,
                TextAlign::End => Justify::End,
            }),
            Axis::Column => cell.align(match col.align {
                TextAlign::Start => Align::Start,
                TextAlign::Center => Align::Center,
                TextAlign::End => Align::End,
            }),
            Axis::Overlay => cell,
        },
    }
}

/// A keyboard-operable data row — [`table_row`] with an identity, the
/// `<tr tabindex="0">` shape (issue #144). `key` makes it a hit-test
/// and focus target: a click or Enter / Space emits `Click` /
/// `Activate` routed to `key` (fold with
/// [`UiEvent::is_click_or_activate`](crate::event::UiEvent::is_click_or_activate)),
/// Tab reaches it, and inside a [`table_body`] ↑ / ↓ / Home / End
/// step between keyed rows. The focus ring sits inside the row so it
/// isn't clipped by the table. Mark the current row
/// `.aria_selected(true)` and give it a selected fill so sighted and
/// screen-reader users see the same state. Row stepping applies while
/// the *row* is focused: with focus on a control inside a cell (an
/// actions-column button) the arrows belong to that control, and
/// Shift+Tab returns to the row.
///
/// ```ignore
/// table_body(items.iter().enumerate().map(|(i, it)| {
///     table_row_keyed(format!("inv-row-{i}"), [table_cell(text(&it.name))])
///         .aria_selected(state.selected == Some(i))
/// }))
/// ```
#[track_caller]
pub fn table_row_keyed<I, E>(key: impl Into<String>, cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    table_row(cells)
        .at_loc(Location::caller())
        .key(key)
        .focusable()
        .focus_ring_inside()
}

/// Header cell from a plain label (like `<th>`) — muted medium-weight
/// label text on a transparent ground (shadcn header rows carry no
/// fill; the border-b rule below the row is the header chrome).
///
/// Takes a string, so a header holding anything else — a sort caret, an
/// icon, a checkbox — goes through [`table_head_el`], which applies the
/// same chrome to an arbitrary `El`.
///
/// Width is a flat `Size::Fill(1.0)` — every column equally wide. For
/// real column proportions, build the header row with
/// [`table_header_cells`] and a [`TableColumn`] spec rather than
/// restating a `.width(...)` here and again on every body row.
#[track_caller]
pub fn table_head(label: impl Into<String>) -> El {
    table_head_el(text(label))
}

/// Header cell from arbitrary content — applies the header chrome and
/// recursively restyles text descendants to the muted caption treatment.
///
/// Like [`table_head`], sized `Size::Fill(1.0)`; see
/// [`table_header_cells`] for the column-spec path. Padding follows the
/// theme's [`ComponentSize`](crate::metrics::ComponentSize) rung exactly
/// as [`table_cell`]'s does.
#[track_caller]
pub fn table_head_el(content: impl Into<El>) -> El {
    let mut el = content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        // The `Md` rung's value, restated as the bare-constructor
        // default — see `table_cell`.
        .default_padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .radius(0.0);
    // The cell chrome is applied to the content El itself (no wrapper
    // node), so only stamp the header-cell role when the content
    // hasn't already declared one — a sortable-header `button` keeps
    // its `Role::Button`.
    if el.a11y.as_deref().is_none_or(|p| p.role.is_none()) {
        el = el.role(Role::ColumnHeader);
    }
    apply_head_style(&mut el);
    el
}

/// Body cell (like `<td>`) — wraps arbitrary content in the padded,
/// ellipsizing cell chrome. Cells carry no borders of their own;
/// the horizontal rules between rows are the rows' `.border_b()`,
/// applied by [`table_body`].
///
/// Width is a flat `Size::Fill(1.0)`, so a hand-built table has equal
/// columns. Proportioned or aligned columns come from
/// [`table_row_cells`] and a shared [`TableColumn`] spec — this
/// constructor stays the escape hatch for a row that needs per-cell
/// chrome the spec doesn't describe.
///
/// # Padding is on the size ladder
///
/// Cell padding is the one container metric the
/// [`ComponentSize`](crate::metrics::ComponentSize) ladder keys, because
/// the row pitch it sets is what makes a data view read dense or airy.
/// The metrics pass stamps the theme's resolved rung onto every cell of
/// a [`table_row`], sized so the **row's content box matches a control
/// of that rung** — 36 px at `Md` (shadcn's own, and the value this
/// constructor bakes as its bare default), 28 px at `Xs`, which is what
/// `damascene_workbench::theme` ships. An explicit `.padding(...)` /
/// `.py(...)` on the cell opts out and survives untouched.
#[track_caller]
pub fn table_cell(content: impl Into<El>) -> El {
    let el = content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        // shadcn's `p-2 px-3`, which is also the ladder's `Md` rung —
        // so a cell that never meets the metrics pass (a bare unit test,
        // a fragment built outside a `table_row`) still looks right.
        .default_padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .radius(0.0);
    // As in [`table_head_el`]: the chrome lands on the content El
    // itself, so keep any role the content already carries (e.g. a
    // `button` in an actions column).
    if el.a11y.as_deref().is_none_or(|p| p.role.is_none()) {
        el.role(Role::Cell)
    } else {
        el
    }
}

fn apply_head_style(el: &mut El) {
    if el.kind == Kind::Text {
        el.text_role = TextRole::Label;
        if el.font_weight == FontWeight::Regular {
            el.font_weight = FontWeight::Medium;
        }
        el.text_color = Some(tokens::MUTED_FOREGROUND);
    }
    for child in &mut el.children {
        apply_head_style(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_row_keyed_is_a_focusable_keyed_row() {
        let r = table_row_keyed("row-1", [table_cell("a")]);
        assert_eq!(r.key.as_deref(), Some("row-1"));
        assert!(r.focusable, "keyed rows are Tab / arrow reachable");
        assert_eq!(r.focus_ring_placement, FocusRingPlacement::Inside);
        assert_eq!(r.metrics_role, Some(MetricsRole::TableRow));
        assert_eq!(
            r.a11y.as_deref().and_then(|p| p.role),
            Some(Role::Row),
            "still a table row to AT"
        );

        // Plain rows stay static data.
        let plain = table_row([table_cell("a")]);
        assert!(plain.key.is_none());
        assert!(!plain.focusable);
    }

    #[test]
    fn table_body_is_a_vertical_arrow_nav_group() {
        let body = table_body([
            table_row_keyed("r0", [table_cell("a")]),
            table_row_keyed("r1", [table_cell("b")]),
        ]);
        assert_eq!(body.arrow_nav, Some(ArrowNav::Vertical));
        // Two rows, no interleaved rule element: the row separator is a
        // `border_b()` on every row but the last, so every child of the
        // body is a keyed, arrow-steppable row.
        assert_eq!(body.children.len(), 2);
        assert_eq!(body.children[0].key.as_deref(), Some("r0"));
        assert_eq!(body.children[1].key.as_deref(), Some("r1"));
        assert_eq!(
            body.children[0].border.as_deref().map(|b| b.widths.bottom),
            Some(1.0),
            "all but the last row carry the separating rule"
        );
        assert!(
            body.children[1]
                .border
                .as_deref()
                .is_none_or(|b| b.widths.bottom == 0.0),
            "the last row is unruled"
        );
    }

    #[test]
    fn table_header_promotes_direct_table_rows() {
        let header = table_header([table_row([table_head("Name")])]);

        // Promoted row carrying the head/body separating border-b.
        assert_eq!(header.children.len(), 1);
        assert_eq!(
            header.children[0].metrics_role,
            Some(MetricsRole::TableHeader)
        );
        assert_eq!(header.children[0].align, Align::Stretch);
        let border = header.children[0].border.as_deref().unwrap();
        assert_eq!(border.widths, Sides::bottom(1.0));
        assert_eq!(border.color, None, "header border uses the token default");
    }

    #[test]
    fn table_head_el_styles_rich_text_children() {
        let head = table_head_el(text_runs([text("Rich "), text("head").bold()]));

        assert_eq!(head.kind, Kind::Inlines);
        assert_eq!(head.children[0].text_role, TextRole::Label);
        assert_eq!(head.children[0].font_weight, FontWeight::Medium);
        assert_eq!(head.children[1].text_role, TextRole::Label);
        assert_eq!(head.children[1].font_weight, FontWeight::Bold);
        assert_eq!(head.children[1].text.as_deref(), Some("head"));
    }

    #[test]
    fn table_rows_are_border_separated_not_grid() {
        // shadcn table anatomy: padded borderless cells, transparent
        // header, and `border-b` on every row but the last.
        //
        // The padding here is the *constructor default* — the ladder's
        // `Md` rung, which is the value this constructor hardcoded
        // before cell padding joined `ComponentSize`. `explicit_padding`
        // must stay false or the metrics pass can never densify it; see
        // `metrics::tests::the_default_component_size_densifies_a_table_end_to_end`.
        let body_cell = table_cell(text("Ada"));
        assert_eq!(
            body_cell.padding,
            Sides::xy(tokens::SPACE_3, tokens::SPACE_2)
        );
        assert!(
            !body_cell.explicit_padding,
            "a stock cell's padding belongs to the theme, not the author"
        );
        assert!(!table_head("Name").explicit_padding);
        assert_eq!(body_cell.stroke, None);
        assert_eq!(body_cell.radius, Corners::ZERO);

        let head = table_head("Name");
        assert_eq!(head.fill, None);
        assert_eq!(head.stroke, None);

        let body = table_body([
            table_row([table_cell(text("a"))]),
            table_row([table_cell(text("b"))]),
        ]);
        assert_eq!(body.children.len(), 2, "rows only — no rule children");
        let first = body.children[0].border.as_deref().unwrap();
        assert_eq!(first.widths, Sides::bottom(1.0));
        assert!(
            body.children[1].border.is_none(),
            "tbody tr:last-child is unbordered"
        );
    }

    #[test]
    fn table_header_text_emits_glyph_run_after_layout() {
        use crate::Rect;
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = table([
            table_header([table_row([table_head("Name"), table_head("Role")])]),
            table_body([table_row([
                table_cell(text("Ada")),
                table_cell(text("dev")),
            ])]),
        ]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 320.0, 200.0));

        let ops = draw_ops(&tree, &state);
        assert!(
            ops.iter().any(|op| matches!(
                op,
                DrawOp::GlyphRun { text, .. } if text == "Name"
            )),
            "expected header text to be painted; ops were {ops:?}"
        );
        let rule_quads = ops
            .iter()
            .filter(|op| {
                matches!(op, DrawOp::Quad { id, rect, .. }
                    if id.ends_with(".border-b") && rect.h == 1.0)
            })
            .count();
        assert!(
            rule_quads >= 1,
            "expected the header/body separating border-b quad, got {rule_quads}"
        );
    }

    // ---- column model -------------------------------------------------

    /// The five-column parts-index spec the validation apps hand-rolled.
    const COLS: &[TableColumn] = &[
        TableColumn::fill(0.6),
        TableColumn::fill(1.1),
        TableColumn::fixed(120.0),
        TableColumn::fill(1.4),
        TableColumn::fill(0.6).align_end(),
    ];

    #[test]
    fn columns_stamp_width_by_position() {
        let head = table_header_cells(COLS, ["Ref", "Value", "Footprint", "Library", "Stock"]);
        let body = table_row_cells(
            "row:0",
            COLS,
            [
                text("R14"),
                text("10k"),
                text("0603"),
                text("Device"),
                text("8,420"),
            ],
        );

        let expected = [
            Size::Fill(0.6),
            Size::Fill(1.1),
            Size::Fixed(120.0),
            Size::Fill(1.4),
            Size::Fill(0.6),
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(head.children[i].width, *want, "header cell {i}");
            assert_eq!(body.children[i].width, *want, "body cell {i}");
        }
    }

    #[test]
    fn header_and_body_share_one_column_spec() {
        // The point of the type: no second place to restate a weight.
        // Header cell i and body cell i must agree on width and
        // alignment for every i, from the same `COLS`.
        let head = table_header_cells(COLS, ["Ref", "Value", "Footprint", "Library", "Stock"]);
        let body = table_row_cells(
            "row:0",
            COLS,
            [
                text("R14"),
                text("10k"),
                text("0603"),
                text("Device"),
                text("8,420"),
            ],
        );
        assert_eq!(head.children.len(), body.children.len());
        for i in 0..COLS.len() {
            assert_eq!(
                (head.children[i].width, head.children[i].text_align),
                (body.children[i].width, body.children[i].text_align),
                "column {i} drifted between header and body",
            );
        }
    }

    #[test]
    fn alignment_stamps_text_align_on_text_cells() {
        let body = table_row_cells("row:0", COLS, [text("a"), text("b"), text("c")]);
        assert_eq!(
            body.children[0].text_align,
            TextAlign::Start,
            "a start column leaves the cell at the default",
        );

        let cols = [TableColumn::fill(1.0).align_end()];
        let row = table_row_cells("row:0", &cols, [text("8,420")]);
        assert_eq!(row.children[0].kind, Kind::Text);
        assert_eq!(row.children[0].text_align, TextAlign::End);

        // Header cells take the same stamp, so the "Stock" label sits
        // over its own digits.
        let head = table_header_cells(&cols, ["Stock"]);
        assert_eq!(head.children[0].text_align, TextAlign::End);
    }

    #[test]
    fn alignment_stamps_justify_on_container_cells() {
        // `table_cell` styles content in place, so a cell built from a
        // row must be aligned on its main axis instead.
        let cols = [
            TableColumn::fill(1.0).align_end(),
            TableColumn::fill(1.0).align_center(),
        ];
        let r = table_row_cells(
            "row:0",
            &cols,
            [
                row([text("12"), text("mm")]),
                column([text("top"), text("bottom")]),
            ],
        );
        assert_eq!(r.children[0].justify, Justify::End);
        assert_eq!(
            r.children[1].align,
            Align::Center,
            "a column cell aligns on its cross axis",
        );
    }

    #[test]
    fn extra_cells_keep_the_stock_fill_and_extra_columns_go_unused() {
        // Documented tolerance: no panic, no debug_assert. A cell past
        // the spec is a plain `table_cell` (Fill(1.0), start-aligned)
        // and a column past the cells simply never gets stamped.
        let cols = [TableColumn::fill(3.0), TableColumn::fill(2.0).align_end()];

        let too_many = table_row_cells("row:0", &cols, [text("a"), text("b"), text("c")]);
        assert_eq!(too_many.children.len(), 3);
        assert_eq!(too_many.children[2].width, Size::Fill(1.0));
        assert_eq!(too_many.children[2].text_align, TextAlign::Start);

        let too_few = table_row_cells("row:1", &cols, [text("a")]);
        assert_eq!(too_few.children.len(), 1);
        assert_eq!(too_few.children[0].width, Size::Fill(3.0));

        let head_too_many = table_header_cells(&cols, ["a", "b", "c"]);
        assert_eq!(head_too_many.children[2].width, Size::Fill(1.0));
    }

    #[test]
    fn row_cells_key_the_row_and_keep_the_cell_chrome() {
        let r = table_row_cells("part:R14", COLS, [text("R14"), text("10k")]);
        assert_eq!(r.key.as_deref(), Some("part:R14"));
        assert_eq!(r.metrics_role, Some(MetricsRole::TableRow));
        // Cells still carry the stock `table_cell` chrome — the `Md`
        // constructor default, before any theme has stamped a rung.
        assert_eq!(
            r.children[0].padding,
            Sides::xy(tokens::SPACE_3, tokens::SPACE_2)
        );
        assert_eq!(r.children[0].radius, Corners::ZERO);

        // Header cells still carry the muted-label restyle.
        let head = table_header_cells(COLS, ["Ref"]);
        assert_eq!(head.children[0].text_role, TextRole::Label);
        assert_eq!(head.children[0].font_weight, FontWeight::Medium);
    }

    #[test]
    fn header_cells_row_is_promoted_by_table_header() {
        // The spec-built row is a plain `table_row`, so `table_header`'s
        // promotion (header metrics + the separating rule) still fires.
        let header = table_header([table_header_cells(COLS, ["Ref", "Value"])]);
        assert_eq!(
            header.children[0].metrics_role,
            Some(MetricsRole::TableHeader)
        );
        assert_eq!(
            header.children[0].border.as_deref().unwrap().widths,
            Sides::bottom(1.0)
        );
    }

    // ---- focusable rows -------------------------------------------------

    #[test]
    fn rows_draw_their_focus_ring_inside() {
        assert_eq!(
            table_row([table_cell(text("a"))]).focus_ring_placement,
            FocusRingPlacement::Inside,
        );
        assert_eq!(
            table_row_cells("row:0", COLS, [text("a")]).focus_ring_placement,
            FocusRingPlacement::Inside,
        );
    }

    /// A table of focusable rows must be keyboard-navigable, not
    /// mouse-only. Two validation rounds hit `FocusRingObscured` on
    /// every row because `table()` clips and an outward ring paints
    /// into the scissored band; the control case below re-creates that
    /// failure so the assertion above can't quietly stop meaning
    /// anything.
    #[test]
    fn table_of_focusable_rows_is_lint_clean() {
        use crate::bundle::lint::FindingKind;
        use crate::layout::layout;
        use crate::state::UiState;

        fn parts_table(outward_rings: bool) -> El {
            let rows: Vec<El> = (0..8)
                .map(|i| {
                    let r = table_row_cells(
                        format!("row:{i}"),
                        COLS,
                        [
                            text(format!("R{i}")),
                            text("10k"),
                            text("R_0603"),
                            text("Device"),
                            text("8,420"),
                        ],
                    )
                    .focusable();
                    if outward_rings {
                        r.focus_ring_outside()
                    } else {
                        r
                    }
                })
                .collect();
            crate::tree::column([table([
                table_header([table_header_cells(
                    COLS,
                    ["Ref", "Value", "Footprint", "Library", "Stock"],
                )]),
                table_body(rows),
            ])])
            .padding(Sides::all(tokens::SPACE_4))
        }

        fn obscured(mut root: El) -> Vec<String> {
            let mut state = UiState::new();
            layout(&mut root, &mut state, Rect::new(0.0, 0.0, 720.0, 400.0));
            crate::bundle::lint::lint(&root, &state, &crate::theme::Theme::default())
                .findings
                .into_iter()
                .filter(|f| f.kind == FindingKind::FocusRingObscured)
                .map(|f| f.node_id)
                .collect()
        }

        assert!(
            !obscured(parts_table(true)).is_empty(),
            "control: an outward ring inside table()'s clip must still trip the lint",
        );
        let findings = obscured(parts_table(false));
        assert!(
            findings.is_empty(),
            "stock focusable rows must not trip FocusRingObscured: {findings:?}",
        );
    }

    /// The row_rule → `.border_b()` conversion is pixel-neutral: each
    /// deleted 1px rule child is replaced by its row growing 1px via
    /// the border joining the content inset. Lay out the legacy
    /// interleaved-rule anatomy next to the current one and assert the
    /// total painted height and every rule's y position are unchanged.
    #[test]
    fn border_b_conversion_is_pixel_neutral_with_legacy_rules() {
        use crate::Rect;
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::layout::layout;
        use crate::state::UiState;

        fn legacy_rule() -> El {
            El::new(Kind::Group)
                .fill(tokens::BORDER)
                .width(Size::Fill(1.0))
                .height(Size::Fixed(1.0))
        }
        fn rows() -> [El; 3] {
            ["Ada", "Grace", "Edsger"].map(|n| table_row([table_cell(text(n))]))
        }

        // Legacy anatomy: rules interleaved between rows, plus one
        // under the header row (what table_header/table_body emitted
        // before per-side borders).
        let [r0, r1, r2] = rows();
        let legacy_header = El::new(Kind::Custom("table_header"))
            .children([table_row([table_head("Name")]), legacy_rule()])
            .axis(Axis::Column)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
            .align(Align::Stretch);
        let legacy_body = El::new(Kind::Custom("table_body"))
            .children([r0, legacy_rule(), r1, legacy_rule(), r2])
            .axis(Axis::Column)
            .width(Size::Fill(1.0))
            .height(Size::Hug)
            .align(Align::Stretch);
        let mut legacy = table([legacy_header, legacy_body]);
        // table_header's promotion only runs in the real constructor.
        legacy.children[0].children[0].metrics_role = Some(MetricsRole::TableHeader);

        let mut current = table([
            table_header([table_row([table_head("Name")])]),
            table_body(rows()),
        ]);

        let viewport = Rect::new(0.0, 0.0, 320.0, 400.0);
        let mut state = UiState::new();
        layout(&mut legacy, &mut state, viewport);
        let mut state2 = UiState::new();
        layout(&mut current, &mut state2, viewport);

        // Total painted height: the tables hug their content, so the
        // roots' computed heights must match exactly.
        assert_eq!(
            legacy.computed_rect.h, current.computed_rect.h,
            "conversion changed the table's total height"
        );

        // Rule positions: every legacy 1px rule quad has a border-b
        // quad at the same y in the converted table.
        let legacy_ops = draw_ops(&legacy, &state);
        let current_ops = draw_ops(&current, &state2);
        let legacy_rule_ys: Vec<f32> = legacy_ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Quad { id, rect, .. } if rect.h == 1.0 && !id.ends_with(".border-b") => {
                    Some(rect.y)
                }
                _ => None,
            })
            .collect();
        let border_ys: Vec<f32> = current_ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Quad { id, rect, .. } if id.ends_with(".border-b") => Some(rect.y),
                _ => None,
            })
            .collect();
        assert_eq!(legacy_rule_ys.len(), 3, "header rule + two body rules");
        assert_eq!(
            legacy_rule_ys, border_ys,
            "border-b quads must land where the legacy rules painted"
        );
    }
}
