//! Table — shadcn-shaped table anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `table([table_header([table_row([...])]), table_body([...])])`.
//! Rows carry the theme-facing table metrics; `table_header` promotes
//! direct `table_row` children from body-row metrics to header metrics.

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
            if !row.explicit_radius {
                row.radius = crate::tree::Corners::ZERO;
            }
        }
    }

    // shadcn's header row carries the same border-b as body rows —
    // it is what visually separates <thead> from <tbody>.
    header = header.child(row_rule());
    header
}

/// Body section holding the data [`table_row`]s, like `<tbody>`.
/// Rows are separated by 1px border-colored rules — the shadcn table
/// is row-bordered (`tr` gets `border-b`, with `tbody
/// tr:last-child` unbordered), not a full cell grid.
#[track_caller]
pub fn table_body<I, E>(rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = Vec::new();
    for row in rows {
        if !children.is_empty() {
            children.push(row_rule());
        }
        children.push(row.into());
    }
    El::new(Kind::Custom("table_body"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
}

/// The 1px horizontal rule between table rows (and under the header).
fn row_rule() -> El {
    El::new(Kind::Group)
        .fill(tokens::BORDER)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(1.0))
}

/// A row of cells (like `<tr>`) carrying the theme's table-row
/// metrics; cells stretch vertically so their padded rows align.
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
}

/// Header cell from a plain label (like `<th>`) — muted medium-weight
/// label text on a transparent ground (shadcn header rows carry no
/// fill; the border-b rule below the row is the header chrome).
#[track_caller]
pub fn table_head(label: impl Into<String>) -> El {
    table_head_el(text(label))
}

/// Header cell from arbitrary content — applies the header chrome and
/// recursively restyles text descendants to the muted caption treatment.
#[track_caller]
pub fn table_head_el(content: impl Into<El>) -> El {
    let mut el = content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
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
/// horizontal rules between rows come from [`table_body`].
#[track_caller]
pub fn table_cell(content: impl Into<El>) -> El {
    let el = content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
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
    fn table_header_promotes_direct_table_rows() {
        let header = table_header([table_row([table_head("Name")])]);

        // Promoted row plus the head/body separating rule.
        assert_eq!(header.children.len(), 2);
        assert_eq!(
            header.children[0].metrics_role,
            Some(MetricsRole::TableHeader)
        );
        assert_eq!(header.children[0].align, Align::Stretch);
        assert_eq!(header.children[1].fill, Some(tokens::BORDER));
        assert_eq!(header.children[1].height, Size::Fixed(1.0));
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
    fn table_rows_are_rule_separated_not_grid() {
        // shadcn table anatomy: padded borderless cells, transparent
        // header, and 1px rules between rows (none after the last).
        let body_cell = table_cell(text("Ada"));
        assert_eq!(
            body_cell.padding,
            Sides::xy(tokens::SPACE_3, tokens::SPACE_2)
        );
        assert_eq!(body_cell.stroke, None);
        assert_eq!(body_cell.radius, Corners::ZERO);

        let head = table_head("Name");
        assert_eq!(head.fill, None);
        assert_eq!(head.stroke, None);

        let body = table_body([
            table_row([table_cell(text("a"))]),
            table_row([table_cell(text("b"))]),
        ]);
        assert_eq!(body.children.len(), 3, "two rows + one rule between");
        assert_eq!(body.children[1].fill, Some(tokens::BORDER));
        assert_eq!(body.children[1].height, Size::Fixed(1.0));
        assert_ne!(body.children[2].fill, Some(tokens::BORDER));
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
            .filter(|op| matches!(op, DrawOp::Quad { rect, .. } if rect.h == 1.0))
            .count();
        assert!(
            rule_quads >= 1,
            "expected the header/body separating rule, got {rule_quads} 1px quads"
        );
    }
}
