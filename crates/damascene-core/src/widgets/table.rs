//! Table — shadcn-shaped table anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `table([table_header([table_row([...])]), table_body([...])])`.
//! Rows carry the theme-facing table metrics; `table_header` promotes
//! direct `table_row` children from body-row metrics to header metrics.

use std::panic::Location;

use super::text::text;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn table<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("table"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
        .clip()
}

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

    header
}

#[track_caller]
pub fn table_body<I, E>(rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("table_body"))
        .at_loc(Location::caller())
        .children(rows)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
}

#[track_caller]
pub fn table_row<I, E>(cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(cells)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::TableRow)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
        .default_gap(0.0)
        .default_radius(0.0)
}

#[track_caller]
pub fn table_head(label: impl Into<String>) -> El {
    table_head_el(text(label))
}

#[track_caller]
pub fn table_head_el(content: impl Into<El>) -> El {
    let mut el = content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .radius(0.0);
    apply_head_style(&mut el);
    el
}

#[track_caller]
pub fn table_cell(content: impl Into<El>) -> El {
    content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .stroke(tokens::BORDER)
        .radius(0.0)
}

fn apply_head_style(el: &mut El) {
    if el.kind == Kind::Text {
        el.text_role = TextRole::Caption;
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

        assert_eq!(header.children.len(), 1);
        assert_eq!(
            header.children[0].metrics_role,
            Some(MetricsRole::TableHeader)
        );
        assert_eq!(header.children[0].align, Align::Stretch);
    }

    #[test]
    fn table_head_el_styles_rich_text_children() {
        let head = table_head_el(text_runs([text("Rich "), text("head").bold()]));

        assert_eq!(head.kind, Kind::Inlines);
        assert_eq!(head.children[0].text_role, TextRole::Caption);
        assert_eq!(head.children[0].font_weight, FontWeight::Medium);
        assert_eq!(head.children[1].text_role, TextRole::Caption);
        assert_eq!(head.children[1].font_weight, FontWeight::Bold);
        assert_eq!(head.children[1].text.as_deref(), Some("head"));
    }

    #[test]
    fn table_cells_carry_grid_chrome() {
        let body = table_cell(text("Ada"));
        assert_eq!(body.padding, Sides::xy(tokens::SPACE_3, tokens::SPACE_2));
        assert_eq!(body.stroke, Some(tokens::BORDER));
        assert_eq!(body.stroke_width, 1.0);
        assert_eq!(body.radius, Corners::ZERO);

        let head = table_head("Name");
        assert_eq!(head.fill, Some(tokens::MUTED));
        assert_eq!(head.stroke, Some(tokens::BORDER));
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
        let border_quads = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("text")))
            .count();
        assert!(
            border_quads >= 4,
            "expected cell chrome quads for the table cells, got {border_quads}"
        );
    }
}
