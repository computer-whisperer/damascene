//! Carbon + workbench widget vocabulary.
//!
//! ## Two naming sources, deliberately
//!
//! - **Carbon** supplies the token and control layer: [`layer`], [`tag`],
//!   [`data_table`], [`structured_list`], [`side_nav`], [`ui_shell_header`].
//! - **VS Code's workbench** supplies the shell anatomy Carbon does not
//!   have: [`pane`], [`pane_header`], [`status_bar`], [`activity_bar`],
//!   [`breadcrumb_bar`], [`panel`].
//!
//! Carbon's `UIShell` is a header + side-nav *web app* shell. It has no
//! notion of a split-pane tool with per-pane headers, a status bar, or an
//! icon rail — those are desktop patterns, absent from the web component
//! literature. The most widely-trained vocabulary for them is VS Code's
//! theme keys (`editorGroupHeader`, `statusBar`, `activityBar`,
//! `sideBar`, `panel`), so the shell widgets borrow those names. The two
//! sets operate at different levels and don't collide.
//!
//! ## Why these don't set `surface_role`
//!
//! `SurfaceRole::Panel` and `Raised` paint a border plus a drop shadow
//! and *no fill*. That is the shadcn containment model: a surface is an
//! outline. Carbon's model is the opposite — a surface is a **value
//! step** ([`layer`]), and borders are reserved for hairline structure.
//!
//! So these widgets set `fill` explicitly and leave `surface_role` at
//! `None`. Consequences worth knowing:
//!
//! - No automatic border, which is the point.
//! - **No shadows anywhere**, including on popovers. Carbon uses
//!   elevation sparingly and relies on the layer step instead.
//! - The `ReinventedWidget` lint doesn't fire, because its signature is
//!   `fill == CARD && stroke == BORDER && radius > 0` and nothing here
//!   strokes its whole perimeter.
//! - `MissingSurfaceFill` doesn't fire either, since that lint only
//!   inspects nodes that *do* claim a `Panel` role.
//!
//! ## Hairlines
//!
//! Damascene has no per-side stroke (no `border-b` equivalent), so a
//! bottom rule is a 1px [`hairline`] child rather than a border on the
//! container. Every bar and table row below pays one extra node for it.
//! That verbosity is the concrete argument for adding
//! `El::border_b(...)` to core.

#![warn(missing_docs)]

use damascene_core::prelude::*;

use crate::tokens as carbon;
use crate::tokens::TypeStyle;

// ---------------------------------------------------------------------
// Type
// ---------------------------------------------------------------------

/// Apply a Carbon [`TypeStyle`] to a text element — size, line height,
/// weight, and letter spacing as one call.
///
/// Prefer this over setting the four properties individually so a type
/// decision stays traceable to a named Carbon style.
pub fn styled(el: El, style: TypeStyle) -> El {
    el.font_size(style.size)
        .line_height(style.line_height)
        .font_weight(style.weight)
        .letter_spacing(style.letter_spacing)
}

/// `label-01` text in [`carbon::TEXT_SECONDARY`] — column headers,
/// field labels, the left half of a [`prop_row`].
pub fn label(s: impl Into<String>) -> El {
    styled(text(s), carbon::LABEL_01).text_color(carbon::TEXT_SECONDARY)
}

/// `body-compact-01` text in [`carbon::TEXT_PRIMARY`] — the default for
/// dense UI content.
pub fn body(s: impl Into<String>) -> El {
    styled(text(s), carbon::BODY_COMPACT_01).text_color(carbon::TEXT_PRIMARY)
}

/// `helper-text-01` in [`carbon::TEXT_HELPER`] — hints and captions.
pub fn helper(s: impl Into<String>) -> El {
    styled(text(s), carbon::HELPER_TEXT_01).text_color(carbon::TEXT_HELPER)
}

/// `heading-compact-01` (14/18 semibold) — pane and panel titles.
pub fn heading(s: impl Into<String>) -> El {
    styled(text(s), carbon::HEADING_COMPACT_01).text_color(carbon::TEXT_PRIMARY)
}

/// `code-01` in JetBrains Mono — identifiers, hashes, paths, symbol
/// names. This is the correct place for a monospace face; measured
/// *quantities* should use [`numeric`] instead.
pub fn code(s: impl Into<String>) -> El {
    styled(text(s), carbon::CODE_01)
        .mono()
        .text_color(carbon::TEXT_PRIMARY)
}

/// `body-compact-01` with tabular (fixed-width) numerals — the correct
/// treatment for measured quantities.
///
/// This is the web convention (`font-variant-numeric: tabular-nums`,
/// Tailwind's `tabular-nums`) and it is *not* the same as [`code`]:
/// figures keep the UI face and simply stop shifting width as digits
/// change, so columns of numbers align without switching typeface.
pub fn numeric(s: impl Into<String>) -> El {
    styled(text(s), carbon::BODY_COMPACT_01)
        .tabular_numerals()
        .text_color(carbon::TEXT_PRIMARY)
}

// ---------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------

/// A 1px horizontal rule in [`carbon::BORDER_SUBTLE_01`].
///
/// Stands in for CSS `border-bottom`, which damascene cannot express.
pub fn hairline() -> El {
    divider()
        .width(Size::Fill(1.0))
        .height(Size::Fixed(carbon::HAIRLINE))
        .fill(carbon::BORDER_SUBTLE_01)
}

/// A 1px vertical rule in [`carbon::BORDER_SUBTLE_01`] — between
/// toolbar groups or side-by-side regions.
pub fn hairline_v() -> El {
    divider()
        .width(Size::Fixed(carbon::HAIRLINE))
        .height(Size::Fill(1.0))
        .fill(carbon::BORDER_SUBTLE_01)
}

/// Carbon's containment primitive: a surface one value step above its
/// parent.
///
/// `step` selects the fill — 0 is [`carbon::BACKGROUND`], 1
/// [`carbon::LAYER_01`], 2 [`carbon::LAYER_02`], 3+ [`carbon::LAYER_03`].
/// Nest layers to build hierarchy: a `layer(1)` pane holding a
/// `layer(2)` card holding a `layer(3)` popover reads correctly with no
/// borders at all.
///
/// Square by default, per Carbon. No padding is applied — a layer is a
/// surface, not a content box, and dense chrome usually wants the fill
/// to run edge to edge with padding applied to inner rows.
pub fn layer<I, E>(step: u8, children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .fill(layer_fill(step))
        .radius(carbon::RADIUS_NONE)
        .align(Align::Stretch)
}

/// The fill for a given [`layer`] step. Exposed so custom containers can
/// sit on the same ramp without reimplementing the mapping.
pub fn layer_fill(step: u8) -> Color {
    match step {
        0 => carbon::BACKGROUND,
        1 => carbon::LAYER_01,
        2 => carbon::LAYER_02,
        _ => carbon::LAYER_03,
    }
}

// ---------------------------------------------------------------------
// Tag — Carbon's one pill-shaped component
// ---------------------------------------------------------------------

/// Hue for a [`tag`]. Carbon ships each as a background/foreground pair
/// tuned for contrast on dark themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TagColor {
    /// Neutral — the default.
    #[default]
    Gray,
    /// Informational.
    Blue,
    /// Positive / passing.
    Green,
    /// Negative / failing.
    Red,
}

impl TagColor {
    fn pair(self) -> (Color, Color) {
        match self {
            TagColor::Gray => (carbon::TAG_BG_GRAY, carbon::TAG_FG_GRAY),
            TagColor::Blue => (carbon::TAG_BG_BLUE, carbon::TAG_FG_BLUE),
            TagColor::Green => (carbon::TAG_BG_GREEN, carbon::TAG_FG_GREEN),
            TagColor::Red => (carbon::TAG_BG_RED, carbon::TAG_FG_RED),
        }
    }
}

/// Carbon `Tag` — a compact pill for status and categorical values.
///
/// The one place this crate rounds anything: Carbon tags are pills while
/// every other Carbon surface is square. Reproduced faithfully rather
/// than squared for consistency, because a fresh author writing `tag()`
/// expects Carbon's shape.
pub fn tag(s: impl Into<String>, color: TagColor) -> El {
    let (bg, fg) = color.pair();
    row([styled(text(s), carbon::LABEL_01).text_color(fg)])
        .fill(bg)
        .radius(carbon::RADIUS_PILL)
        .padding(Sides::xy(carbon::SPACING_03, 0.0))
        .height(Size::Fixed(18.0))
        .align(Align::Center)
        .justify(Justify::Center)
}

// ---------------------------------------------------------------------
// Shell — VS Code workbench regions
// ---------------------------------------------------------------------

/// Carbon UI Shell header — a 48px [`carbon::LAYER_01`] bar with a
/// bottom hairline, holding a product name and trailing chrome.
pub fn ui_shell_header<I, E>(product: impl Into<String>, trailing: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut kids: Vec<El> = vec![
        styled(text(product), carbon::HEADING_COMPACT_01).text_color(carbon::TEXT_PRIMARY),
        spacer(),
    ];
    kids.extend(trailing.into_iter().map(Into::into));

    let bar = row(kids)
        .gap(carbon::SPACING_04)
        .padding(Sides::x(carbon::SPACING_05))
        .height(Size::Fill(1.0))
        .align(Align::Center);

    column([bar, hairline()])
        .fill(carbon::LAYER_01)
        .height(Size::Fixed(carbon::HEADER_HEIGHT))
        .align(Align::Stretch)
}

/// Carbon UI Shell side navigation — a fixed-width [`carbon::LAYER_01`]
/// column with a trailing hairline.
pub fn side_nav<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row([
        column(children)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
        hairline_v(),
    ])
    .fill(carbon::LAYER_01)
    .width(Size::Fixed(carbon::SIDE_NAV_WIDTH))
    .height(Size::Fill(1.0))
}

/// A side-nav row. `selected` paints [`carbon::LAYER_SELECTED_01`] plus
/// Carbon's 3px leading [`carbon::BORDER_INTERACTIVE`] marker.
pub fn side_nav_item(icon_name: IconName, text_label: impl Into<String>, selected: bool) -> El {
    // Unselected rows reserve the marker's width with an unfilled
    // divider — nothing is painted, so no raw transparent color is
    // needed and the label baseline stays put across states.
    let marker = {
        let d = divider().width(Size::Fixed(3.0)).height(Size::Fill(1.0));
        if selected {
            d.fill(carbon::BORDER_INTERACTIVE)
        } else {
            d
        }
    };

    let content = row([
        icon(icon_name)
            .icon_size(carbon::ICON_SIZE)
            .text_color(if selected {
                carbon::ICON_PRIMARY
            } else {
                carbon::ICON_SECONDARY
            }),
        styled(text(text_label), carbon::BODY_COMPACT_01).text_color(if selected {
            carbon::TEXT_PRIMARY
        } else {
            carbon::TEXT_SECONDARY
        }),
    ])
    .gap(carbon::SPACING_04)
    .padding(Sides::x(carbon::SPACING_05))
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
    .align(Align::Center);

    row([marker, content])
        .height(Size::Fixed(carbon::FIELD_HEIGHT_MD))
        .align(Align::Stretch)
        .fill(if selected {
            carbon::LAYER_SELECTED_01
        } else {
            carbon::LAYER_01
        })
}

/// A side-nav section label — uppercase `label-01` in
/// [`carbon::TEXT_HELPER`].
pub fn side_nav_label(s: impl Into<String>) -> El {
    styled(text(s.into().to_uppercase()), carbon::LABEL_01)
        .text_color(carbon::TEXT_HELPER)
        .letter_spacing(0.32)
        .padding(Sides {
            left: carbon::SPACING_05,
            right: carbon::SPACING_05,
            top: carbon::SPACING_05,
            bottom: carbon::SPACING_02,
        })
}

/// VS Code `activityBar` — a 48px icon rail. Carbon has no equivalent
/// region; this is the multi-pane-tool idiom.
pub fn activity_bar<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row([
        column(children)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Center)
            .gap(carbon::SPACING_02)
            .padding(Sides::y(carbon::SPACING_03)),
        hairline_v(),
    ])
    .fill(carbon::BACKGROUND)
    .width(Size::Fixed(carbon::ACTIVITY_BAR_WIDTH))
    .height(Size::Fill(1.0))
}

/// A square icon button for the [`activity_bar`], with an active
/// leading marker.
pub fn activity_bar_item(icon_name: IconName, active: bool, key: &str) -> El {
    row([icon(icon_name)
        .icon_size(carbon::ICON_SIZE_MD)
        .text_color(if active {
            carbon::ICON_PRIMARY
        } else {
            carbon::ICON_SECONDARY
        })])
    .key(key)
    .width(Size::Fixed(carbon::ACTIVITY_BAR_WIDTH - carbon::HAIRLINE))
    .height(Size::Fixed(carbon::ACTIVITY_BAR_WIDTH - carbon::HAIRLINE))
    .align(Align::Center)
    .justify(Justify::Center)
    .fill(if active {
        carbon::LAYER_01
    } else {
        carbon::BACKGROUND
    })
}

/// VS Code `editorGroup` — a work pane: a [`pane_header`] strip over a
/// body, on [`carbon::BACKGROUND`].
pub fn pane<I, E>(header: El, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        header,
        column(body)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .clip(),
    ])
    .fill(carbon::BACKGROUND)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
    .align(Align::Stretch)
}

/// VS Code `editorGroupHeader` — a 35px pane title strip with trailing
/// affordances and a bottom hairline.
pub fn pane_header<I, E>(icon_name: IconName, title: impl Into<String>, trailing: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut kids: Vec<El> = vec![
        icon(icon_name)
            .icon_size(carbon::ICON_SIZE)
            .text_color(carbon::ICON_SECONDARY),
        heading(title),
        spacer(),
    ];
    kids.extend(trailing.into_iter().map(Into::into));

    let bar = row(kids)
        .gap(carbon::SPACING_03)
        .padding(Sides::x(carbon::SPACING_04))
        .height(Size::Fill(1.0))
        .align(Align::Center);

    column([bar, hairline()])
        .fill(carbon::LAYER_01)
        .height(Size::Fixed(carbon::PANE_HEADER_HEIGHT))
        .align(Align::Stretch)
}

/// VS Code `panel` — a docked side or bottom region on
/// [`carbon::LAYER_01`], with a header strip.
pub fn panel<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let header = column([
        row([label(title.into().to_uppercase())])
            .padding(Sides::x(carbon::SPACING_04))
            .height(Size::Fill(1.0))
            .align(Align::Center),
        hairline(),
    ])
    .height(Size::Fixed(carbon::PANE_HEADER_HEIGHT))
    .align(Align::Stretch);

    column([
        header,
        column(body)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch)
            .clip(),
    ])
    .fill(carbon::LAYER_01)
    // Must fill, not hug: a panel is a region of a fixed-width rail, and
    // hugging lets long property values push it past the rail's edge.
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
    .align(Align::Stretch)
}

/// VS Code `breadcrumb` — a 22px context strip in `code-01`.
pub fn breadcrumb_bar(segments: &[&str]) -> El {
    let mut kids: Vec<El> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            kids.push(
                icon(IconName::ChevronRight)
                    .icon_size(12.0)
                    .text_color(carbon::TEXT_HELPER),
            );
        }
        let last = i + 1 == segments.len();
        kids.push(
            styled(text(*seg), carbon::CODE_01)
                .mono()
                .text_color(if last {
                    carbon::TEXT_PRIMARY
                } else {
                    carbon::TEXT_HELPER
                }),
        );
    }

    // SPACING_02 (4px), not SPACING_01: the icon-to-text lint floor is
    // 4px, and a 2px gap genuinely reads as a collision at 12px type.
    row(kids)
        .gap(carbon::SPACING_02)
        .padding(Sides::x(carbon::SPACING_04))
        .height(Size::Fixed(carbon::BREADCRUMB_HEIGHT))
        .align(Align::Center)
        .fill(carbon::BACKGROUND)
}

/// VS Code `statusBar` — a 22px bar of terse live readouts.
///
/// Deliberately the shortest region in the shell: at 22px it reads as
/// instrumentation rather than as content.
pub fn status_bar<I, E>(leading: I, trailing: Vec<El>) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut kids: Vec<El> = leading.into_iter().map(Into::into).collect();
    kids.push(spacer());
    kids.extend(trailing);

    column([
        hairline(),
        row(kids)
            .gap(carbon::SPACING_05)
            .padding(Sides::x(carbon::SPACING_03))
            .height(Size::Fill(1.0))
            .align(Align::Center),
    ])
    .fill(carbon::LAYER_01)
    .height(Size::Fixed(carbon::STATUS_BAR_HEIGHT))
    .align(Align::Stretch)
}

/// A single status-bar readout: an optional icon plus `label-01` text.
pub fn status_bar_item(icon_name: Option<IconName>, s: impl Into<String>, color: Color) -> El {
    let mut kids: Vec<El> = Vec::new();
    if let Some(n) = icon_name {
        kids.push(icon(n).icon_size(12.0).text_color(color));
    }
    kids.push(
        styled(text(s), carbon::LABEL_01)
            .text_color(color)
            .tabular_numerals(),
    );

    row(kids).gap(carbon::SPACING_02).align(Align::Center)
}

/// A toolbar strip on [`carbon::LAYER_01`] with a bottom hairline.
pub fn toolbar<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        row(children)
            .gap(carbon::SPACING_02)
            .padding(Sides::x(carbon::SPACING_03))
            .height(Size::Fill(1.0))
            .align(Align::Center),
        hairline(),
    ])
    .fill(carbon::LAYER_01)
    .height(Size::Fixed(carbon::FIELD_HEIGHT_SM))
    .align(Align::Stretch)
}

/// A square icon button sized to Carbon's `sm` field height.
pub fn toolbar_button(icon_name: IconName, key: &str, active: bool) -> El {
    row([icon(icon_name)
        .icon_size(carbon::ICON_SIZE)
        .text_color(if active {
            carbon::ICON_PRIMARY
        } else {
            carbon::ICON_SECONDARY
        })])
    .key(key)
    .width(Size::Fixed(carbon::FIELD_HEIGHT_SM - carbon::SPACING_02))
    .height(Size::Fixed(carbon::FIELD_HEIGHT_SM - carbon::SPACING_02))
    .align(Align::Center)
    .justify(Justify::Center)
    .fill(if active {
        carbon::LAYER_SELECTED_01
    } else {
        carbon::LAYER_01
    })
}

// ---------------------------------------------------------------------
// Property rows — Carbon StructuredList / Ant Descriptions
// ---------------------------------------------------------------------

/// A label/value row: label left in [`carbon::TEXT_SECONDARY`], value
/// right in [`carbon::TEXT_PRIMARY`], separated by a bottom hairline.
///
/// This is the inspector idiom — Carbon's `StructuredList`, Ant's
/// `Descriptions`. Structure comes from alignment and a rule, not from
/// wrapping each pair in a card.
pub fn prop_row(name: impl Into<String>, value: El) -> El {
    column([
        row([label(name), spacer(), value])
            .gap(carbon::SPACING_04)
            .padding(Sides::x(carbon::SPACING_04))
            .height(Size::Fill(1.0))
            .align(Align::Center),
        hairline(),
    ])
    .height(Size::Fixed(carbon::ROW_HEIGHT_SM))
    .align(Align::Stretch)
}

/// A vertical run of [`prop_row`]s or [`structured_list_row`]s.
pub fn structured_list<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .width(Size::Fill(1.0))
        .align(Align::Stretch)
}

/// A single-column list row at [`carbon::ROW_HEIGHT_XS`], with a bottom
/// hairline.
pub fn structured_list_row<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        row(children)
            .gap(carbon::SPACING_03)
            .padding(Sides::x(carbon::SPACING_04))
            .height(Size::Fill(1.0))
            .align(Align::Center),
        hairline(),
    ])
    .height(Size::Fixed(carbon::ROW_HEIGHT_XS))
    .align(Align::Stretch)
}

// ---------------------------------------------------------------------
// Data table — Carbon DataTable, virtualized
// ---------------------------------------------------------------------

/// A [`data_table`] column spec.
#[derive(Clone, Debug)]
pub struct Column {
    /// Header text.
    pub label: String,
    /// Column width. [`Size::Ch`] is useful for numeric columns — it
    /// sizes to a digit count in the cell's own font, so the column
    /// holds steady as values change width.
    pub width: Size,
    /// Right-align the cell content. Conventional for quantities.
    pub numeric: bool,
}

impl Column {
    /// A left-aligned text column.
    pub fn text(label: impl Into<String>, width: Size) -> Self {
        Self {
            label: label.into(),
            width,
            numeric: false,
        }
    }

    /// A right-aligned numeric column.
    pub fn numeric(label: impl Into<String>, width: Size) -> Self {
        Self {
            label: label.into(),
            width,
            numeric: true,
        }
    }
}

/// Carbon `DataTable`, virtualized.
///
/// `row_height` should normally be [`carbon::ROW_HEIGHT_XS`] (24px,
/// Carbon's `size="xs"`) for dense data. Rows are built lazily through
/// [`virtual_list`], so a table of hundreds of thousands of rows costs
/// only the visible window.
///
/// `cell(row, col)` returns the content for one cell; the table owns
/// width, alignment, padding, and the row hairline. Use [`body`],
/// [`code`], [`numeric`], or [`tag`] for cell content so type and color
/// stay consistent down a column.
///
/// `selected` paints one row with [`carbon::LAYER_SELECTED_01`] —
/// Carbon's `DataTable` selection treatment. Pass `None` for no
/// selection.
pub fn data_table<F>(
    columns: Vec<Column>,
    row_count: usize,
    row_height: f32,
    selected: Option<usize>,
    cell: F,
) -> El
where
    F: Fn(usize, usize) -> El + Send + Sync + 'static,
{
    let header_cols = columns.clone();
    let header = column([
        row(header_cols
            .iter()
            .map(|c| {
                let inner = row([label(c.label.clone()).ellipsis()])
                    .width(c.width)
                    .padding(Sides::x(carbon::SPACING_03))
                    .align(Align::Center)
                    .clip();
                if c.numeric {
                    inner.justify(Justify::End)
                } else {
                    inner
                }
            })
            .collect::<Vec<_>>())
        .height(Size::Fill(1.0))
        .align(Align::Stretch),
        hairline(),
    ])
    .fill(carbon::LAYER_ACCENT_01)
    .height(Size::Fixed(carbon::ROW_HEIGHT_SM))
    .align(Align::Stretch);

    let body_cols = columns;
    let rows = virtual_list(row_count, row_height, move |i| {
        let cells = body_cols
            .iter()
            .enumerate()
            .map(|(col_idx, c)| {
                let inner = row([cell(i, col_idx)])
                    .width(c.width)
                    .padding(Sides::x(carbon::SPACING_03))
                    .height(Size::Fill(1.0))
                    .align(Align::Center)
                    .clip();
                if c.numeric {
                    inner.justify(Justify::End)
                } else {
                    inner
                }
            })
            .collect::<Vec<_>>();

        let body = column([
            row(cells).height(Size::Fill(1.0)).align(Align::Stretch),
            hairline(),
        ])
        .height(Size::Fill(1.0))
        .align(Align::Stretch);

        if selected == Some(i) {
            body.fill(carbon::LAYER_SELECTED_01)
        } else {
            body
        }
    })
    .scrollbar_gutter();

    column([header, rows])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_steps_map_to_the_carbon_ramp() {
        assert_eq!(layer_fill(0).r, carbon::BACKGROUND.r);
        assert_eq!(layer_fill(1).r, carbon::LAYER_01.r);
        assert_eq!(layer_fill(2).r, carbon::LAYER_02.r);
        // Saturates rather than panicking on an out-of-range step.
        assert_eq!(layer_fill(9).r, carbon::LAYER_03.r);
    }

    #[test]
    fn layers_are_square_and_filled() {
        let l = layer(1, [text("x")]);
        assert_eq!(l.radius.tl, carbon::RADIUS_NONE);
        assert!(l.fill.is_some(), "a layer must carry its own fill");
    }

    #[test]
    fn layers_do_not_claim_a_surface_role() {
        // Panel/Raised would paint a border + shadow. Carbon's
        // containment is the value step alone.
        let l = layer(1, [text("x")]);
        assert_eq!(l.surface_role, SurfaceRole::None);
        assert!(l.stroke.is_none(), "a layer must not stroke its perimeter");
    }

    #[test]
    fn tag_is_the_only_rounded_surface() {
        let t = tag("passing", TagColor::Green);
        assert_eq!(t.radius.tl, carbon::RADIUS_PILL);
    }

    #[test]
    fn numeric_uses_tabular_figures_not_mono() {
        let n = numeric("5.8");
        assert!(n.text_tabular_numerals);
        // `numeric` keeps the UI face; `code` is the mono path.
        assert!(!n.font_mono);
        assert!(code("draw_indexed").font_mono);
    }

    #[test]
    fn type_styles_apply_all_four_properties() {
        let el = styled(text("x"), carbon::HEADING_COMPACT_01);
        assert_eq!(el.font_size, carbon::HEADING_COMPACT_01.size);
        assert_eq!(el.line_height, carbon::HEADING_COMPACT_01.line_height);
        assert_eq!(el.font_weight, carbon::HEADING_COMPACT_01.weight);
        assert_eq!(
            el.text_letter_spacing,
            carbon::HEADING_COMPACT_01.letter_spacing
        );
    }
}
