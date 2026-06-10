//! Native math expression IR and box layout.
//!
//! This module is intentionally presentation-oriented. It is shaped like
//! MathML Core because that is the interchange target Damascene wants to accept,
//! but layout lowers into TeX-style boxes: width, ascent, descent, and a flat
//! list of positioned glyph/rule atoms.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::ops::Range;
use std::sync::Arc;

use crate::text::metrics as text_metrics;
use crate::tree::{Color, FontFamily, FontWeight, Rect, TextWrap};

const DEFAULT_RULE_THICKNESS: f32 = 1.1;
const SCRIPT_SCALE: f32 = 0.72;
const LARGE_OPERATOR_SCALE: f32 = 1.35;
const FRACTION_PAD_EM: f32 = 0.18;
const FRACTION_GAP_EM: f32 = 0.18;
const SQRT_GAP_EM: f32 = 0.10;
const TABLE_COL_GAP_EM: f32 = 0.8;
const TABLE_ROW_GAP_EM: f32 = 0.35;
const CASES_COL_GAP_EM: f32 = 0.5;
const RADICAL_GLYPH: char = '√';
const THIN_MATH_SPACE_EM: f32 = 0.08;
const MEDIUM_MATH_SPACE_EM: f32 = 0.18;
const THICK_MATH_SPACE_EM: f32 = 0.28;
#[cfg(feature = "symbols")]
const STRETCHY_VARIANT_CHARS: [char; 29] = [
    '(',
    ')',
    '[',
    ']',
    '{',
    '}',
    '|',
    '‖',
    '⌊',
    '⌋',
    '⌈',
    '⌉',
    RADICAL_GLYPH,
    '∑',
    '∫',
    '∏',
    '⋂',
    '⋃',
    '∐',
    '∮',
    '∬',
    '∭',
    '⨁',
    '⨂',
    '⨀',
    '⋁',
    '⋀',
    '⨄',
    '⨆',
];

/// Display style of a math expression, mirroring the MathML Core `display`
/// attribute (and TeX's text vs. display style).
///
/// The style affects layout: [`Block`](MathDisplay::Block) enlarges big
/// operators, places their limits above/below, and uses full-size fraction
/// parts, while [`Inline`](MathDisplay::Inline) keeps everything compact
/// enough to sit in a line of prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MathDisplay {
    /// Compact in-line style (`display="inline"`, TeX text style).
    #[default]
    Inline,
    /// Display style for standalone equations (`display="block"`).
    Block,
}

/// Horizontal alignment of one table column, mirroring the MathML Core
/// `columnalign` attribute on `<mtable>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MathColumnAlignment {
    /// Align cell contents to the left edge of the column.
    Left,
    /// Center cell contents within the column (the MathML default).
    #[default]
    Center,
    /// Align cell contents to the right edge of the column.
    Right,
}

/// Presentation-oriented math expression tree, shaped like MathML Core.
///
/// Each variant corresponds to a MathML Core element (noted per variant), so
/// MathML interchange is a near 1:1 mapping. Trees are produced by
/// [`parse_tex`]/[`parse_mathml`] (or built directly) and turned into
/// renderable boxes by [`layout_math`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum MathExpr {
    /// Horizontal sequence of sub-expressions (`<mrow>`).
    Row(Vec<MathExpr>),
    /// Identifier such as a variable or function name (`<mi>`); rendered italic.
    Identifier(String),
    /// Numeric literal (`<mn>`); rendered upright.
    Number(String),
    /// Operator, relation, or punctuation (`<mo>`). Spacing and big-operator
    /// behavior come from a built-in table keyed on the operator text.
    Operator(String),
    /// Operator (`<mo>`) with explicit attribute overrides; unset fields fall
    /// back to the built-in operator table used by [`MathExpr::Operator`].
    OperatorWithMetadata {
        /// The operator text.
        text: String,
        /// Space before the operator in `em` (MathML `lspace`).
        lspace: Option<f32>,
        /// Space after the operator in `em` (MathML `rspace`).
        rspace: Option<f32>,
        /// Whether to draw an enlarged glyph in block display (MathML `largeop`).
        large_operator: Option<bool>,
        /// Whether attached scripts become under/over limits in block display
        /// (MathML `movablelimits`).
        movable_limits: Option<bool>,
    },
    /// Literal text rendered upright (`<mtext>`).
    Text(String),
    /// Horizontal space of the given width in `em` (`<mspace>`).
    Space(f32),
    /// Fraction with a horizontal rule (`<mfrac>`).
    Fraction {
        /// Expression above the fraction rule.
        numerator: Arc<MathExpr>,
        /// Expression below the fraction rule.
        denominator: Arc<MathExpr>,
    },
    /// Square root (`<msqrt>`).
    Sqrt(Arc<MathExpr>),
    /// Root with an explicit degree, e.g. a cube root (`<mroot>`).
    Root {
        /// Expression under the radical.
        base: Arc<MathExpr>,
        /// Degree of the root, drawn small above the radical's hook.
        index: Arc<MathExpr>,
    },
    /// Base with attached subscript and/or superscript
    /// (`<msub>`/`<msup>`/`<msubsup>`).
    Scripts {
        /// Expression the scripts attach to.
        base: Arc<MathExpr>,
        /// Subscript, if any.
        sub: Option<Arc<MathExpr>>,
        /// Superscript, if any.
        sup: Option<Arc<MathExpr>>,
    },
    /// Base with material placed directly below and/or above it
    /// (`<munder>`/`<mover>`/`<munderover>`), e.g. limits of a sum.
    UnderOver {
        /// Expression the under/over material attaches to.
        base: Arc<MathExpr>,
        /// Expression centered below the base, if any.
        under: Option<Arc<MathExpr>>,
        /// Expression centered above the base, if any.
        over: Option<Arc<MathExpr>>,
    },
    /// Accented base such as a hat or overline (`<mover accent="true">`).
    Accent {
        /// Expression under the accent.
        base: Arc<MathExpr>,
        /// The accent expression, drawn close above the base.
        accent: Arc<MathExpr>,
        /// Whether the accent stretches to the base's width (MathML
        /// `stretchy`); currently honored for overline-style accents.
        stretch: bool,
    },
    /// Body wrapped in (possibly stretchy) fence delimiters (`<mfenced>`, or
    /// an `<mrow>` with stretchy `<mo>` fences).
    Fenced {
        /// Opening delimiter text, or `None` for no opening fence.
        open: Option<String>,
        /// Closing delimiter text, or `None` for no closing fence.
        close: Option<String>,
        /// Expression between the fences.
        body: Arc<MathExpr>,
    },
    /// Rows and columns of aligned cells (`<mtable>`), also used for matrix
    /// and `aligned`/`cases` TeX environments.
    Table {
        /// Cell expressions, outer `Vec` per row, inner `Vec` per column.
        rows: Vec<Vec<MathExpr>>,
        /// Per-column alignment (MathML `columnalign`); columns beyond the
        /// list fall back to the last entry or the default (center).
        column_alignments: Vec<MathColumnAlignment>,
        /// Gap between columns in `em` (MathML `columnspacing`), or `None`
        /// for the built-in default.
        column_gap: Option<f32>,
        /// Gap between rows in `em` (MathML `rowspacing`), or `None` for the
        /// built-in default.
        row_gap: Option<f32>,
    },
    /// Transparent wrapper recording which byte range of the original source
    /// produced `body`; emitted by [`parse_tex_with_source_ranges`] and
    /// ignored by layout. No MathML equivalent.
    Source {
        /// Byte range into the original source string.
        source: Range<usize>,
        /// The wrapped expression.
        body: Arc<MathExpr>,
    },
    /// Parse error or unsupported construct; the message is rendered as
    /// plain text in place of the expression.
    Error(String),
}

impl MathExpr {
    /// Builds a [`MathExpr::Row`] from `children`, collapsing the trivial
    /// cases: zero children yield an empty row and a single child is
    /// returned as-is without a wrapping row.
    pub fn row(children: impl IntoIterator<Item = MathExpr>) -> Self {
        let mut children: Vec<MathExpr> = children.into_iter().collect();
        match children.len() {
            0 => MathExpr::Row(Vec::new()),
            1 => children.pop().unwrap(),
            _ => MathExpr::Row(children),
        }
    }

    /// Returns the source byte range if this node is a [`MathExpr::Source`]
    /// wrapper, without descending into children.
    pub fn source_range(&self) -> Option<&Range<usize>> {
        match self {
            MathExpr::Source { source, .. } => Some(source),
            _ => None,
        }
    }

    /// Unwraps any (nested) [`MathExpr::Source`] wrappers and returns the
    /// underlying expression; returns `self` for all other variants.
    pub fn without_source(&self) -> &MathExpr {
        match self {
            MathExpr::Source { body, .. } => body.without_source(),
            _ => self,
        }
    }
}

/// Result of laying out a [`MathExpr`]: a TeX-style box plus its flattened
/// render atoms, produced by [`layout_math`].
///
/// Coordinates are in logical pixels, relative to the box origin at the left
/// end of the main baseline, with y growing downward. Negative y is therefore
/// above the baseline; `ascent` and `descent` are both positive extents.
#[derive(Clone, Debug, PartialEq)]
pub struct MathLayout {
    /// Total advance width of the box.
    pub width: f32,
    /// Extent above the main baseline (positive).
    pub ascent: f32,
    /// Extent below the main baseline (positive).
    pub descent: f32,
    /// Flat list of positioned atoms to paint, in baseline-relative
    /// coordinates (see the struct docs).
    pub atoms: Vec<MathAtom>,
}

impl MathLayout {
    /// Total box height: `ascent + descent`.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

/// One positioned paint primitive in a [`MathLayout`].
///
/// All coordinates follow the [`MathLayout`] convention: relative to the
/// box origin on the main baseline, y-down (negative y is above the
/// baseline). The renderer translates atoms to the element's baseline and
/// paints them in order.
#[derive(Clone, Debug, PartialEq)]
pub enum MathAtom {
    /// Text run drawn through the regular text pipeline.
    Glyph {
        /// Text to draw.
        text: String,
        /// Left edge of the run.
        x: f32,
        /// Baseline of the run relative to the main baseline
        /// (`0.0` = on the main baseline; negative = raised, e.g. superscripts).
        y_baseline: f32,
        /// Font size in logical pixels (already includes script scaling).
        size: f32,
        /// Font weight for the run.
        weight: FontWeight,
        /// Whether to draw the run in italic (used for identifiers).
        italic: bool,
    },
    /// Single glyph from the bundled math font, addressed by OpenType glyph
    /// id and drawn as a vector outline scaled into `rect`. Used for OpenType
    /// stretchy-delimiter variants, delimiter assembly parts, big-operator
    /// variants, and radical signs.
    GlyphId {
        /// OpenType glyph id in the math font.
        glyph_id: u16,
        /// Target rectangle the glyph outline is scaled to fill.
        rect: Rect,
        /// Source box of the glyph outline in font units (y-down, derived
        /// from the glyph bounding box and advance), mapped onto `rect`.
        view_box: Rect,
    },
    /// Filled rectangle, used for fraction rules and radical overbars.
    Rule {
        /// Rectangle to fill.
        rect: Rect,
    },
    /// Vector-drawn radical sign (fallback when no OpenType radical variant
    /// fits): a polyline from the left flair through the hook and tick up to
    /// the overbar, stroked at `thickness`.
    Radical {
        /// The five `[x, y]` polyline vertices, left to right; the final
        /// segment is the overbar across the radicand.
        points: [[f32; 2]; 5],
        /// Stroke thickness of the polyline.
        thickness: f32,
    },
    /// Stretched fence delimiter drawn as a vector shape (fallback when the
    /// math font offers no variant or assembly tall enough).
    Delimiter {
        /// Delimiter text, e.g. `"("` or `"{"`.
        delimiter: String,
        /// Rectangle the stretched delimiter must span.
        rect: Rect,
        /// Stroke thickness for the drawn shape.
        thickness: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MathOperatorClass {
    Ordinary,
    Binary,
    Relation,
    Large,
    Punctuation,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MathOperatorInfo {
    class: MathOperatorClass,
    lspace_em: f32,
    rspace_em: f32,
    large_operator: bool,
    movable_limits: bool,
}

impl MathOperatorInfo {
    fn new(class: MathOperatorClass, lspace_em: f32, rspace_em: f32) -> Self {
        Self {
            class,
            lspace_em,
            rspace_em,
            large_operator: false,
            movable_limits: false,
        }
    }

    fn large(mut self) -> Self {
        self.large_operator = true;
        self.movable_limits = true;
        self
    }

    fn large_with_side_scripts(mut self) -> Self {
        self.large_operator = true;
        self.movable_limits = false;
        self
    }
}

fn operator_info(operator: &str) -> MathOperatorInfo {
    use MathOperatorClass::*;
    match operator {
        "+" | "-" | "±" | "∓" | "·" | "×" | "÷" | "∪" | "∩" | "∧" | "∨" | "⊕" | "⊖" | "⊗" | "⊘"
        | "⊙" | "⋆" | "∗" | "∘" | "•" | "⊔" | "⊓" | "⨿" | "≀" | "◁" | "▷" | "⋄" | "∖" => {
            MathOperatorInfo::new(Binary, MEDIUM_MATH_SPACE_EM, MEDIUM_MATH_SPACE_EM)
        }
        "=" | "<" | ">" | "≤" | "≥" | "≠" | "≈" | "∼" | "→" | "←" | "↔" | "⇒" | "⇐" | "⇔" | "⟹"
        | "⟸" | "⟺" | "⟶" | "⟵" | "⟷" | "↦" | "⟼" | "↪" | "↩" | "∈" | "∉" | "∋" | "∌" | "⊂"
        | "⊃" | "⊆" | "⊇" | "⊊" | "⊋" | "≡" | "≃" | "≅" | "∝" | "≺" | "≻" | "⪯" | "⪰" | "≪"
        | "≫" | "∥" | "⊥" | "≍" | "≐" | "⊨" | "⊢" | "⊣" | "∴" | "∵" => {
            MathOperatorInfo::new(Relation, MEDIUM_MATH_SPACE_EM, MEDIUM_MATH_SPACE_EM)
        }
        "∑" | "∏" | "⋂" | "⋃" | "∐" | "⨁" | "⨂" | "⨀" | "⋁" | "⋀" | "⨄" | "⨆" => {
            MathOperatorInfo::new(Large, THIN_MATH_SPACE_EM, THIN_MATH_SPACE_EM).large()
        }
        "∫" | "∮" | "∬" | "∭" => {
            MathOperatorInfo::new(Large, THIN_MATH_SPACE_EM, THIN_MATH_SPACE_EM)
                .large_with_side_scripts()
        }
        "," | "." | ";" | ":" => MathOperatorInfo::new(Punctuation, 0.0, THIN_MATH_SPACE_EM),
        _ => MathOperatorInfo::new(Ordinary, 0.0, 0.0),
    }
}

#[derive(Clone, Copy, Debug)]
struct LayoutCtx {
    size: f32,
    display: MathDisplay,
}

impl LayoutCtx {
    fn script(self) -> Self {
        Self {
            size: self.metrics().script_size(),
            display: MathDisplay::Inline,
        }
    }

    fn large_operator(self) -> Self {
        Self {
            size: self.metrics().large_operator_size(),
            display: self.display,
        }
    }

    fn metrics(self) -> MathMetrics {
        MathMetrics {
            size: self.size,
            display: self.display,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MathMetrics {
    size: f32,
    display: MathDisplay,
}

impl MathMetrics {
    fn font_constants(self) -> Option<OpenTypeMathConstants> {
        open_type_math_constants()
    }

    fn script_size(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.script_scale(self.size))
            .unwrap_or(self.size * SCRIPT_SCALE)
            .max(6.0)
    }

    fn large_operator_size(self) -> f32 {
        self.size * LARGE_OPERATOR_SCALE
    }

    fn rule_thickness(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.fraction_rule_thickness(self.size))
            .unwrap_or(DEFAULT_RULE_THICKNESS * self.size / 16.0)
            .max(0.75)
    }

    fn radical_rule_thickness(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.radical_rule_thickness(self.size))
            .unwrap_or_else(|| self.rule_thickness())
            .max(0.75)
    }

    fn default_ascent(self) -> f32 {
        self.size * 0.75
    }

    fn default_descent(self) -> f32 {
        self.size * 0.25
    }

    fn glyph_ascent(self) -> f32 {
        self.size * 0.82
    }

    fn glyph_descent(self) -> f32 {
        self.size * 0.22
    }

    fn space_width(self, em: f32) -> f32 {
        self.size * em
    }

    fn operator_spacing_with_overrides(
        self,
        operator: &str,
        lspace_em: Option<f32>,
        rspace_em: Option<f32>,
    ) -> (f32, f32) {
        let info = operator_info(operator);
        (
            self.size * lspace_em.unwrap_or(info.lspace_em),
            self.size * rspace_em.unwrap_or(info.rspace_em),
        )
    }

    fn fraction_pad(self) -> f32 {
        self.size
            * if matches!(self.display, MathDisplay::Block) {
                FRACTION_PAD_EM
            } else {
                FRACTION_PAD_EM * 0.65
            }
    }

    fn fraction_numerator_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| {
                constants
                    .fraction_numerator_gap(self.size, matches!(self.display, MathDisplay::Block))
            })
            .unwrap_or_else(|| self.fraction_gap_fallback())
    }

    fn fraction_denominator_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| {
                constants
                    .fraction_denominator_gap(self.size, matches!(self.display, MathDisplay::Block))
            })
            .unwrap_or_else(|| self.fraction_gap_fallback())
    }

    fn fraction_gap_fallback(self) -> f32 {
        self.size
            * if matches!(self.display, MathDisplay::Block) {
                FRACTION_GAP_EM
            } else {
                FRACTION_GAP_EM * 0.55
            }
    }

    fn fraction_numerator_shift(self) -> f32 {
        self.font_constants()
            .and_then(|constants| {
                constants
                    .fraction_numerator_shift(self.size, matches!(self.display, MathDisplay::Block))
            })
            .unwrap_or(self.size * 0.55)
    }

    fn fraction_denominator_shift(self) -> f32 {
        self.font_constants()
            .and_then(|constants| {
                constants.fraction_denominator_shift(
                    self.size,
                    matches!(self.display, MathDisplay::Block),
                )
            })
            .unwrap_or(self.size * 0.55)
    }

    fn math_axis_shift(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.axis_height(self.size))
            .or_else(|| {
                matches!(self.display, MathDisplay::Block)
                    .then(|| self.operator_axis_shift())
                    .flatten()
            })
            .unwrap_or(self.size * 0.28)
    }

    fn operator_axis_shift(self) -> Option<f32> {
        let layout = math_glyph_layout("+", self.size, FontWeight::Regular);
        let baseline = layout.lines.first()?.baseline;
        Some((baseline - layout.line_height * 0.5).max(self.size * 0.2))
    }

    fn sqrt_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| {
                constants
                    .radical_vertical_gap(self.size, matches!(self.display, MathDisplay::Block))
            })
            .unwrap_or(self.size * SQRT_GAP_EM)
    }

    fn radical_width(self) -> f32 {
        self.size * 0.72
    }

    fn radical_left_flair_y(self) -> f32 {
        -self.size * 0.03
    }

    fn radical_hook_x(self) -> f32 {
        self.size * 0.12
    }

    fn radical_hook_y(self) -> f32 {
        -self.size * 0.1
    }

    fn radical_tick_x(self) -> f32 {
        self.size * 0.24
    }

    fn radical_tick_y(self, inner_descent: f32) -> f32 {
        (inner_descent * 0.75).max(self.size * 0.13)
    }

    fn radical_variant_for_height(self, target_height: f32) -> Option<OpenTypeDelimiterVariant> {
        self.stretchy_variant_for_height(RADICAL_GLYPH, target_height)
    }

    fn large_operator_variant_for_height(
        self,
        operator: &str,
        target_height: f32,
    ) -> Option<OpenTypeDelimiterVariant> {
        let operator = single_char(operator)?;
        is_large_operator_symbol(operator)
            .then(|| self.stretchy_variant_for_height(operator, target_height))?
    }

    fn root_offset_x(self, index_width: f32) -> f32 {
        self.font_constants()
            .map(|constants| {
                let before = constants
                    .radical_kern_before_degree(self.size)
                    .unwrap_or(0.0);
                let after = constants
                    .radical_kern_after_degree(self.size)
                    .unwrap_or(0.0);
                (before + index_width + after).max(index_width * 0.35)
            })
            .unwrap_or(index_width * 0.55)
    }

    fn root_index_shift(self, root_ascent: f32, index_descent: f32) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.radical_degree_bottom_raise_fraction())
            .map(|raise| -root_ascent * raise - index_descent)
            .unwrap_or(-root_ascent * 0.52)
    }

    fn script_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.space_after_script(self.size))
            .unwrap_or(self.size * 0.06)
    }

    fn superscript_shift(self, base_ascent: f32, sup_descent: f32) -> f32 {
        let min_shift = self
            .font_constants()
            .and_then(|constants| constants.superscript_shift_up(self.size))
            .unwrap_or(0.0);
        let bottom_min = self
            .font_constants()
            .and_then(|constants| constants.superscript_bottom_min(self.size))
            .unwrap_or(self.size * 0.18);
        -(base_ascent * 0.58)
            .max(min_shift)
            .max(sup_descent + bottom_min)
    }

    fn subscript_shift(self, base_descent: f32, sub_ascent: f32) -> f32 {
        let min_shift = self
            .font_constants()
            .and_then(|constants| constants.subscript_shift_down(self.size))
            .unwrap_or(self.size * 0.28);
        (base_descent + sub_ascent * 0.72).max(min_shift)
    }

    fn sub_superscript_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.sub_superscript_gap_min(self.size))
            .unwrap_or(self.size * 0.08)
    }

    fn under_over_gap(self) -> f32 {
        self.size * 0.12
    }

    fn upper_limit_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.upper_limit_gap_min(self.size))
            .unwrap_or_else(|| self.under_over_gap())
    }

    fn upper_limit_baseline_rise(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.upper_limit_baseline_rise_min(self.size))
            .unwrap_or(self.size * 0.35)
    }

    fn lower_limit_gap(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.lower_limit_gap_min(self.size))
            .unwrap_or_else(|| self.under_over_gap())
    }

    fn lower_limit_baseline_drop(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.lower_limit_baseline_drop_min(self.size))
            .unwrap_or(self.size * 0.35)
    }

    fn accent_gap(self) -> f32 {
        self.size * 0.06
    }

    fn table_col_gap(self, gap_em: Option<f32>) -> f32 {
        self.size * gap_em.unwrap_or(TABLE_COL_GAP_EM)
    }

    fn table_row_gap(self, gap_em: Option<f32>) -> f32 {
        self.size * gap_em.unwrap_or(TABLE_ROW_GAP_EM)
    }

    fn delimiter_gap(self) -> f32 {
        self.size * 0.08
    }

    fn delimiter_overshoot(self) -> f32 {
        (self.size * 0.08).max(self.rule_thickness()).max(
            self.font_constants()
                .and_then(|constants| constants.min_connector_overlap(self.size))
                .unwrap_or(0.0),
        )
    }

    fn delimited_sub_formula_min_height(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.delimited_sub_formula_min_height(self.size))
            .unwrap_or(self.size * 1.5)
    }

    fn should_stretch_delimiter(self, body: &MathLayout) -> bool {
        body.height() + self.delimiter_overshoot() * 2.0 >= self.delimited_sub_formula_min_height()
    }

    fn delimiter_variant_for_height(
        self,
        delimiter: char,
        target_height: f32,
    ) -> Option<OpenTypeDelimiterVariant> {
        self.stretchy_variant_for_height(delimiter, target_height)
    }

    fn stretchy_variant_for_height(
        self,
        glyph: char,
        target_height: f32,
    ) -> Option<OpenTypeDelimiterVariant> {
        self.font_constants().and_then(|constants| {
            constants.stretchy_variant_for_height(glyph, target_height, self.size)
        })
    }

    fn delimiter_assembly_parts(
        self,
        delimiter: char,
    ) -> Option<Vec<OpenTypeDelimiterAssemblyPart>> {
        self.font_constants()
            .and_then(|constants| constants.delimiter_assembly_parts(delimiter))
    }

    fn delimiter_width(self) -> f32 {
        self.size * 0.42
    }
}

#[derive(Clone, Debug)]
struct OpenTypeMathConstants {
    units_per_em: f32,
    script_percent_scale_down: i16,
    axis_height: i16,
    subscript_shift_down: i16,
    superscript_shift_up: i16,
    superscript_bottom_min: i16,
    sub_superscript_gap_min: i16,
    space_after_script: i16,
    upper_limit_gap_min: i16,
    upper_limit_baseline_rise_min: i16,
    lower_limit_gap_min: i16,
    lower_limit_baseline_drop_min: i16,
    fraction_numerator_shift_up: i16,
    fraction_numerator_display_style_shift_up: i16,
    fraction_denominator_shift_down: i16,
    fraction_denominator_display_style_shift_down: i16,
    fraction_rule_thickness: i16,
    fraction_numerator_gap_min: i16,
    fraction_num_display_style_gap_min: i16,
    fraction_denominator_gap_min: i16,
    fraction_denom_display_style_gap_min: i16,
    radical_rule_thickness: i16,
    radical_vertical_gap: i16,
    radical_display_style_vertical_gap: i16,
    radical_kern_before_degree: i16,
    radical_kern_after_degree: i16,
    radical_degree_bottom_raise_percent: i16,
    delimited_sub_formula_min_height: u16,
    min_connector_overlap: u16,
    #[cfg_attr(not(test), allow(dead_code))]
    delimiter_variants: Vec<OpenTypeDelimiterVariants>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
struct OpenTypeDelimiterVariants {
    delimiter: char,
    variants: Vec<OpenTypeDelimiterVariant>,
    assembly_parts: Vec<OpenTypeDelimiterAssemblyPart>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
struct OpenTypeDelimiterVariant {
    glyph_id: u16,
    advance: u16,
    horizontal_advance: u16,
    bbox: Option<OpenTypeGlyphBBox>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
struct OpenTypeDelimiterAssemblyPart {
    glyph_id: u16,
    start_connector_length: u16,
    end_connector_length: u16,
    full_advance: u16,
    horizontal_advance: u16,
    bbox: Option<OpenTypeGlyphBBox>,
    extender: bool,
}

#[derive(Clone, Copy, Debug)]
struct OpenTypeGlyphBBox {
    x_min: i16,
    y_min: i16,
    x_max: i16,
    y_max: i16,
}

impl OpenTypeDelimiterVariants {
    fn max_advance(&self) -> u16 {
        self.variants
            .iter()
            .map(|variant| variant.advance)
            .chain(self.assembly_parts.iter().map(|part| part.full_advance))
            .max()
            .unwrap_or(0)
    }
}

impl OpenTypeMathConstants {
    fn font_units(&self, value: i16, size: f32) -> Option<f32> {
        (value > 0 && self.units_per_em > 0.0).then(|| value as f32 / self.units_per_em * size)
    }

    fn signed_font_units(&self, value: i16, size: f32) -> Option<f32> {
        (value != 0 && self.units_per_em > 0.0).then(|| value as f32 / self.units_per_em * size)
    }

    fn script_scale(&self, size: f32) -> Option<f32> {
        (self.script_percent_scale_down > 0)
            .then(|| size * self.script_percent_scale_down as f32 / 100.0)
    }

    fn fraction_rule_thickness(&self, size: f32) -> Option<f32> {
        self.font_units(self.fraction_rule_thickness, size)
    }

    fn axis_height(&self, size: f32) -> Option<f32> {
        self.font_units(self.axis_height, size)
    }

    fn subscript_shift_down(&self, size: f32) -> Option<f32> {
        self.font_units(self.subscript_shift_down, size)
    }

    fn superscript_shift_up(&self, size: f32) -> Option<f32> {
        self.font_units(self.superscript_shift_up, size)
    }

    fn superscript_bottom_min(&self, size: f32) -> Option<f32> {
        self.font_units(self.superscript_bottom_min, size)
    }

    fn sub_superscript_gap_min(&self, size: f32) -> Option<f32> {
        self.font_units(self.sub_superscript_gap_min, size)
    }

    fn space_after_script(&self, size: f32) -> Option<f32> {
        self.font_units(self.space_after_script, size)
    }

    fn upper_limit_gap_min(&self, size: f32) -> Option<f32> {
        self.font_units(self.upper_limit_gap_min, size)
    }

    fn upper_limit_baseline_rise_min(&self, size: f32) -> Option<f32> {
        self.font_units(self.upper_limit_baseline_rise_min, size)
    }

    fn lower_limit_gap_min(&self, size: f32) -> Option<f32> {
        self.font_units(self.lower_limit_gap_min, size)
    }

    fn lower_limit_baseline_drop_min(&self, size: f32) -> Option<f32> {
        self.font_units(self.lower_limit_baseline_drop_min, size)
    }

    fn fraction_numerator_shift(&self, size: f32, display: bool) -> Option<f32> {
        let value = if display {
            self.fraction_numerator_display_style_shift_up
        } else {
            self.fraction_numerator_shift_up
        };
        self.font_units(value, size)
    }

    fn fraction_denominator_shift(&self, size: f32, display: bool) -> Option<f32> {
        let value = if display {
            self.fraction_denominator_display_style_shift_down
        } else {
            self.fraction_denominator_shift_down
        };
        self.font_units(value, size)
    }

    fn fraction_numerator_gap(&self, size: f32, display: bool) -> Option<f32> {
        let value = if display {
            self.fraction_num_display_style_gap_min
        } else {
            self.fraction_numerator_gap_min
        };
        self.font_units(value, size)
    }

    fn fraction_denominator_gap(&self, size: f32, display: bool) -> Option<f32> {
        let value = if display {
            self.fraction_denom_display_style_gap_min
        } else {
            self.fraction_denominator_gap_min
        };
        self.font_units(value, size)
    }

    fn radical_rule_thickness(&self, size: f32) -> Option<f32> {
        self.font_units(self.radical_rule_thickness, size)
    }

    fn radical_vertical_gap(&self, size: f32, display: bool) -> Option<f32> {
        let value = if display {
            self.radical_display_style_vertical_gap
        } else {
            self.radical_vertical_gap
        };
        self.font_units(value, size)
    }

    fn radical_kern_before_degree(&self, size: f32) -> Option<f32> {
        self.signed_font_units(self.radical_kern_before_degree, size)
    }

    fn radical_kern_after_degree(&self, size: f32) -> Option<f32> {
        self.signed_font_units(self.radical_kern_after_degree, size)
    }

    fn radical_degree_bottom_raise_fraction(&self) -> Option<f32> {
        (self.radical_degree_bottom_raise_percent > 0)
            .then(|| self.radical_degree_bottom_raise_percent as f32 / 100.0)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn delimiter_variant_count(&self, delimiter: char) -> usize {
        self.delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)
            .map(|variants| variants.variants.len())
            .unwrap_or(0)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn delimiter_assembly_part_count(&self, delimiter: char) -> usize {
        self.delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)
            .map(|variants| variants.assembly_parts.len())
            .unwrap_or(0)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn delimiter_max_advance(&self, delimiter: char, size: f32) -> Option<f32> {
        let advance = self
            .delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)?
            .max_advance();
        (advance > 0 && self.units_per_em > 0.0).then(|| advance as f32 / self.units_per_em * size)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn delimiter_extender_part_count(&self, delimiter: char) -> usize {
        self.delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)
            .map(|variants| {
                variants
                    .assembly_parts
                    .iter()
                    .filter(|part| part.extender)
                    .count()
            })
            .unwrap_or(0)
    }

    fn stretchy_variant_for_height(
        &self,
        glyph: char,
        target_height: f32,
        size: f32,
    ) -> Option<OpenTypeDelimiterVariant> {
        let variants = self
            .delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == glyph)?;
        variants.variants.iter().copied().find(|variant| {
            self.units_per_em > 0.0
                && variant.advance as f32 / self.units_per_em * size >= target_height
        })
    }

    fn delimiter_assembly_parts(
        &self,
        delimiter: char,
    ) -> Option<Vec<OpenTypeDelimiterAssemblyPart>> {
        let variants = self
            .delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)?;
        (!variants.assembly_parts.is_empty()).then(|| variants.assembly_parts.clone())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn delimiter_first_variant_glyph_id(&self, delimiter: char) -> Option<u16> {
        self.delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)?
            .variants
            .first()
            .map(|variant| variant.glyph_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn delimiter_has_assembly_connectors(&self, delimiter: char) -> bool {
        self.delimiter_variants
            .iter()
            .find(|variants| variants.delimiter == delimiter)
            .is_some_and(|variants| {
                variants.assembly_parts.iter().any(|part| {
                    part.glyph_id > 0
                        && (part.start_connector_length > 0 || part.end_connector_length > 0)
                })
            })
    }

    fn min_connector_overlap(&self, size: f32) -> Option<f32> {
        (self.min_connector_overlap > 0 && self.units_per_em > 0.0)
            .then(|| self.min_connector_overlap as f32 / self.units_per_em * size)
    }

    fn delimited_sub_formula_min_height(&self, size: f32) -> Option<f32> {
        (self.delimited_sub_formula_min_height > 0 && self.units_per_em > 0.0)
            .then(|| self.delimited_sub_formula_min_height as f32 / self.units_per_em * size)
    }
}

fn open_type_math_constants() -> Option<OpenTypeMathConstants> {
    #[cfg(feature = "symbols")]
    {
        static CONSTANTS: std::sync::OnceLock<Option<OpenTypeMathConstants>> =
            std::sync::OnceLock::new();
        CONSTANTS
            .get_or_init(|| parse_open_type_math_constants(damascene_fonts::NOTO_SANS_MATH_REGULAR))
            .clone()
    }
    #[cfg(not(feature = "symbols"))]
    {
        None
    }
}

#[cfg(feature = "symbols")]
fn parse_open_type_math_constants(font: &[u8]) -> Option<OpenTypeMathConstants> {
    let face = ttf_parser::Face::parse(font, 0).ok()?;
    let math = face.tables().math?;
    let constants = math.constants?;
    Some(OpenTypeMathConstants {
        units_per_em: face.units_per_em() as f32,
        script_percent_scale_down: constants.script_percent_scale_down(),
        axis_height: constants.axis_height().value,
        subscript_shift_down: constants.subscript_shift_down().value,
        superscript_shift_up: constants.superscript_shift_up().value,
        superscript_bottom_min: constants.superscript_bottom_min().value,
        sub_superscript_gap_min: constants.sub_superscript_gap_min().value,
        space_after_script: constants.space_after_script().value,
        upper_limit_gap_min: constants.upper_limit_gap_min().value,
        upper_limit_baseline_rise_min: constants.upper_limit_baseline_rise_min().value,
        lower_limit_gap_min: constants.lower_limit_gap_min().value,
        lower_limit_baseline_drop_min: constants.lower_limit_baseline_drop_min().value,
        fraction_numerator_shift_up: constants.fraction_numerator_shift_up().value,
        fraction_numerator_display_style_shift_up: constants
            .fraction_numerator_display_style_shift_up()
            .value,
        fraction_denominator_shift_down: constants.fraction_denominator_shift_down().value,
        fraction_denominator_display_style_shift_down: constants
            .fraction_denominator_display_style_shift_down()
            .value,
        fraction_rule_thickness: constants.fraction_rule_thickness().value,
        fraction_numerator_gap_min: constants.fraction_numerator_gap_min().value,
        fraction_num_display_style_gap_min: constants.fraction_num_display_style_gap_min().value,
        fraction_denominator_gap_min: constants.fraction_denominator_gap_min().value,
        fraction_denom_display_style_gap_min: constants
            .fraction_denom_display_style_gap_min()
            .value,
        radical_rule_thickness: constants.radical_rule_thickness().value,
        radical_vertical_gap: constants.radical_vertical_gap().value,
        radical_display_style_vertical_gap: constants.radical_display_style_vertical_gap().value,
        radical_kern_before_degree: constants.radical_kern_before_degree().value,
        radical_kern_after_degree: constants.radical_kern_after_degree().value,
        radical_degree_bottom_raise_percent: constants.radical_degree_bottom_raise_percent(),
        delimited_sub_formula_min_height: constants.delimited_sub_formula_min_height(),
        min_connector_overlap: math
            .variants
            .map(|variants| variants.min_connector_overlap)
            .unwrap_or(0),
        delimiter_variants: parse_open_type_delimiter_variants(&face, math.variants),
    })
}

#[cfg(feature = "symbols")]
fn parse_open_type_delimiter_variants(
    face: &ttf_parser::Face<'_>,
    variants: Option<ttf_parser::math::Variants<'_>>,
) -> Vec<OpenTypeDelimiterVariants> {
    let Some(variants) = variants else {
        return Vec::new();
    };
    STRETCHY_VARIANT_CHARS
        .into_iter()
        .filter_map(|delimiter| {
            let glyph = face.glyph_index(delimiter)?;
            let construction = variants.vertical_constructions.get(glyph)?;
            let glyph_variants = construction
                .variants
                .into_iter()
                .map(|variant| OpenTypeDelimiterVariant {
                    glyph_id: variant.variant_glyph.0,
                    advance: variant.advance_measurement,
                    horizontal_advance: face.glyph_hor_advance(variant.variant_glyph).unwrap_or(0),
                    bbox: face.glyph_bounding_box(variant.variant_glyph).map(|bbox| {
                        OpenTypeGlyphBBox {
                            x_min: bbox.x_min,
                            y_min: bbox.y_min,
                            x_max: bbox.x_max,
                            y_max: bbox.y_max,
                        }
                    }),
                })
                .collect();
            let assembly_parts = construction
                .assembly
                .map(|assembly| {
                    assembly
                        .parts
                        .into_iter()
                        .map(|part| OpenTypeDelimiterAssemblyPart {
                            glyph_id: part.glyph_id.0,
                            start_connector_length: part.start_connector_length,
                            end_connector_length: part.end_connector_length,
                            full_advance: part.full_advance,
                            horizontal_advance: face.glyph_hor_advance(part.glyph_id).unwrap_or(0),
                            bbox: face.glyph_bounding_box(part.glyph_id).map(|bbox| {
                                OpenTypeGlyphBBox {
                                    x_min: bbox.x_min,
                                    y_min: bbox.y_min,
                                    x_max: bbox.x_max,
                                    y_max: bbox.y_max,
                                }
                            }),
                            extender: part.part_flags.extender(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(OpenTypeDelimiterVariants {
                delimiter,
                variants: glyph_variants,
                assembly_parts,
            })
        })
        .collect()
}

/// Lays out a math expression into a TeX-style box of positioned atoms.
///
/// `size` is the base font size in logical pixels (scripts and limits are
/// scaled down from it, big operators up). `display` selects inline or block
/// conventions; see [`MathDisplay`]. The returned [`MathLayout`] uses
/// baseline-relative, y-down coordinates and is ready to paint.
pub fn layout_math(expr: &MathExpr, size: f32, display: MathDisplay) -> MathLayout {
    layout_expr(expr, LayoutCtx { size, display })
}

fn layout_expr(expr: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    match expr {
        MathExpr::Source { body, .. } => layout_expr(body, ctx),
        MathExpr::Row(children) => layout_row(children, ctx),
        MathExpr::Identifier(s) => layout_glyph(s, ctx, FontWeight::Regular, true),
        MathExpr::Number(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
        MathExpr::Operator(s) => layout_operator(s, ctx),
        MathExpr::OperatorWithMetadata {
            text,
            lspace,
            rspace,
            large_operator,
            ..
        } => layout_operator_with_spacing(text, *lspace, *rspace, *large_operator, ctx),
        MathExpr::Text(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
        MathExpr::Space(em) => MathLayout {
            width: metrics.space_width(*em),
            ascent: metrics.default_ascent(),
            descent: metrics.default_descent(),
            atoms: Vec::new(),
        },
        MathExpr::Fraction {
            numerator,
            denominator,
        } => layout_fraction(numerator, denominator, ctx),
        MathExpr::Sqrt(child) => layout_sqrt(child, ctx),
        MathExpr::Root { base, index } => layout_root(base, index, ctx),
        MathExpr::Scripts { base, sub, sup } => {
            layout_scripts(base, sub.as_deref(), sup.as_deref(), ctx)
        }
        MathExpr::UnderOver { base, under, over } => {
            layout_under_over(base, under.as_deref(), over.as_deref(), ctx)
        }
        MathExpr::Accent {
            base,
            accent,
            stretch,
        } => layout_accent(base, accent, *stretch, ctx),
        MathExpr::Fenced { open, close, body } => layout_fenced(open, close, body, ctx),
        MathExpr::Table {
            rows,
            column_alignments,
            column_gap,
            row_gap,
        } => layout_table(rows, column_alignments, *column_gap, *row_gap, ctx),
        MathExpr::Error(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
    }
}

fn layout_row(children: &[MathExpr], ctx: LayoutCtx) -> MathLayout {
    let mut width = 0.0;
    let metrics = ctx.metrics();
    let mut ascent: f32 = metrics.default_ascent();
    let mut descent: f32 = metrics.default_descent();
    let mut atoms = Vec::new();
    for child in children {
        let child_layout = layout_expr(child, ctx);
        translate_atoms(&mut atoms, child_layout.atoms, width, 0.0);
        width += child_layout.width;
        ascent = ascent.max(child_layout.ascent);
        descent = descent.max(child_layout.descent);
    }
    MathLayout {
        width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_glyph(s: &str, ctx: LayoutCtx, weight: FontWeight, italic: bool) -> MathLayout {
    if s.is_empty() {
        return MathLayout {
            width: 0.0,
            ascent: 0.0,
            descent: 0.0,
            atoms: Vec::new(),
        };
    }
    let measured = text_metrics::measure_text(s, ctx.size, weight, false, TextWrap::NoWrap, None);
    MathLayout {
        width: measured.width,
        ascent: ctx.metrics().glyph_ascent(),
        descent: ctx.metrics().glyph_descent(),
        atoms: vec![MathAtom::Glyph {
            text: s.to_string(),
            x: 0.0,
            y_baseline: 0.0,
            size: ctx.size,
            weight,
            italic,
        }],
    }
}

fn layout_operator(s: &str, ctx: LayoutCtx) -> MathLayout {
    layout_operator_with_spacing(s, None, None, None, ctx)
}

fn layout_operator_with_spacing(
    s: &str,
    lspace: Option<f32>,
    rspace: Option<f32>,
    large_operator: Option<bool>,
    ctx: LayoutCtx,
) -> MathLayout {
    let use_large_operator = large_operator.unwrap_or_else(|| is_large_operator_symbol_str(s));
    let glyph_ctx = if matches!(ctx.display, MathDisplay::Block) && use_large_operator {
        ctx.large_operator()
    } else {
        ctx
    };
    if matches!(ctx.display, MathDisplay::Block) && use_large_operator {
        let operator = MathExpr::OperatorWithMetadata {
            text: s.into(),
            lspace,
            rspace,
            large_operator: Some(true),
            movable_limits: None,
        };
        if let Some(layout) = layout_large_operator_variant(&operator, glyph_ctx) {
            return layout;
        }
    }
    layout_operator_glyph_with_spacing(s, lspace, rspace, glyph_ctx)
}

fn layout_operator_glyph_with_spacing(
    s: &str,
    lspace: Option<f32>,
    rspace: Option<f32>,
    ctx: LayoutCtx,
) -> MathLayout {
    let mut layout = layout_glyph(s, ctx, FontWeight::Regular, false);
    let (lspace, rspace) = ctx
        .metrics()
        .operator_spacing_with_overrides(s, lspace, rspace);
    // Negative values are valid MathML (tighten-up spacing) and must
    // shift / shrink just like positive values pad — only exact zero
    // on both sides is a no-op.
    if lspace != 0.0 || rspace != 0.0 {
        for atom in &mut layout.atoms {
            if let MathAtom::Glyph { x, .. } = atom {
                *x += lspace;
            }
        }
        layout.width += lspace + rspace;
    }
    layout
}

fn layout_operator_expr_glyph_fallback(expr: &MathExpr, ctx: LayoutCtx) -> Option<MathLayout> {
    match expr.without_source() {
        MathExpr::Operator(s) => Some(layout_operator_glyph_with_spacing(s, None, None, ctx)),
        MathExpr::OperatorWithMetadata {
            text,
            lspace,
            rspace,
            ..
        } => Some(layout_operator_glyph_with_spacing(
            text, *lspace, *rspace, ctx,
        )),
        _ => None,
    }
}

fn layout_fraction(numerator: &MathExpr, denominator: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let child_ctx = if matches!(ctx.display, MathDisplay::Block) {
        ctx
    } else {
        ctx.script()
    };
    let num = layout_expr(numerator, child_ctx);
    let den = layout_expr(denominator, child_ctx);
    let pad = metrics.fraction_pad();
    let num_gap = metrics.fraction_numerator_gap();
    let den_gap = metrics.fraction_denominator_gap();
    let rule = metrics.rule_thickness();
    // The math axis sits above the prose baseline. Keeping the fraction
    // rule on that axis makes inline fractions read as part of the line
    // instead of hanging mostly below it.
    let axis_shift = metrics.math_axis_shift();
    let rule_center_y = -axis_shift;
    let width = num.width.max(den.width) + pad * 2.0;
    let num_x = (width - num.width) * 0.5;
    let den_x = (width - den.width) * 0.5;
    let num_dy = (rule_center_y - num_gap - rule * 0.5 - num.descent)
        .min(-metrics.fraction_numerator_shift());
    let den_dy = (rule_center_y + den_gap + rule * 0.5 + den.ascent)
        .max(metrics.fraction_denominator_shift());
    let ascent = -num_dy + num.ascent;
    let descent = den_dy + den.descent;
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, num.atoms, num_x, num_dy);
    atoms.push(MathAtom::Rule {
        rect: Rect::new(0.0, rule_center_y - rule * 0.5, width, rule),
    });
    translate_atoms(&mut atoms, den.atoms, den_x, den_dy);
    MathLayout {
        width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_sqrt(child: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let inner = layout_expr(child, ctx);
    let gap = metrics.sqrt_gap();
    let rule = metrics.radical_rule_thickness();
    if let Some(layout) = layout_open_type_sqrt(inner.clone(), gap, rule, ctx) {
        return layout;
    }
    layout_vector_sqrt(inner, gap, rule, ctx)
}

fn layout_vector_sqrt(inner: MathLayout, gap: f32, rule: f32, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let radical_w = metrics.radical_width();
    let inner_x = radical_w + gap;
    let bar_y = -inner.ascent - gap - rule * 0.5;
    let tick_y = metrics.radical_tick_y(inner.descent);
    let end_x = inner_x + inner.width;
    let mut atoms = Vec::new();
    atoms.push(MathAtom::Radical {
        points: [
            [0.0, metrics.radical_left_flair_y()],
            [metrics.radical_hook_x(), metrics.radical_hook_y()],
            [metrics.radical_tick_x(), tick_y],
            [radical_w, bar_y],
            [end_x, bar_y],
        ],
        thickness: rule,
    });
    translate_atoms(&mut atoms, inner.atoms, inner_x, 0.0);
    MathLayout {
        width: end_x,
        ascent: -bar_y + rule * 0.5,
        descent: tick_y + rule * 0.5,
        atoms,
    }
}

fn layout_open_type_sqrt(
    inner: MathLayout,
    gap: f32,
    rule: f32,
    ctx: LayoutCtx,
) -> Option<MathLayout> {
    let metrics = ctx.metrics();
    let bar_y = -inner.ascent - gap - rule * 0.5;
    let tick_y = metrics.radical_tick_y(inner.descent);
    let target_height = tick_y - bar_y + rule;
    let variant = metrics.radical_variant_for_height(target_height)?;
    let bbox = variant.bbox?;
    let constants = metrics.font_constants()?;
    let scale = metrics.size / constants.units_per_em;
    let view_box = glyph_advance_view_box(bbox, variant.horizontal_advance, None)?;
    if view_box.w <= 0.0 || view_box.h <= 0.0 {
        return None;
    }
    let radical_w = view_box.w * scale;
    let radical_h = view_box.h * scale;
    let radical_rect = Rect::new(0.0, bar_y - rule * 0.5, radical_w, radical_h);
    let inner_x = radical_w + gap;
    let end_x = inner_x + inner.width;
    let overbar_x = (radical_w - rule * 0.5).max(0.0);
    let mut atoms = Vec::new();
    atoms.push(MathAtom::GlyphId {
        glyph_id: variant.glyph_id,
        rect: radical_rect,
        view_box,
    });
    atoms.push(MathAtom::Rule {
        rect: Rect::new(
            overbar_x,
            bar_y - rule * 0.5,
            (end_x - overbar_x).max(rule),
            rule,
        ),
    });
    translate_atoms(&mut atoms, inner.atoms, inner_x, 0.0);
    Some(MathLayout {
        width: end_x,
        ascent: (-bar_y + rule * 0.5).max(-radical_rect.y),
        descent: (tick_y + rule * 0.5).max(radical_rect.y + radical_rect.h),
        atoms,
    })
}

fn layout_root(base: &MathExpr, index: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let root = layout_sqrt(base, ctx);
    let index = layout_expr(index, ctx.script());
    let root_x = metrics.root_offset_x(index.width);
    let index_dy = metrics.root_index_shift(root.ascent, index.descent);
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, index.atoms, 0.0, index_dy);
    translate_atoms(&mut atoms, root.atoms, root_x, 0.0);
    MathLayout {
        width: root_x + root.width,
        ascent: root.ascent.max(-index_dy + index.ascent),
        descent: root.descent.max(index_dy + index.descent),
        atoms,
    }
}

fn layout_scripts(
    base: &MathExpr,
    sub: Option<&MathExpr>,
    sup: Option<&MathExpr>,
    ctx: LayoutCtx,
) -> MathLayout {
    if matches!(ctx.display, MathDisplay::Block) && is_display_limits_base(base) {
        return layout_under_over(base, sub, sup, ctx);
    }
    let display_large_operator =
        matches!(ctx.display, MathDisplay::Block) && is_large_operator_base(base);
    let base_ctx = if display_large_operator {
        ctx.large_operator()
    } else {
        ctx
    };
    let base_layout = if display_large_operator {
        layout_large_operator_variant(base, base_ctx)
            .or_else(|| layout_operator_expr_glyph_fallback(base, base_ctx))
            .unwrap_or_else(|| layout_expr(base, ctx))
    } else {
        layout_expr(base, base_ctx)
    };
    let script_ctx = ctx.script();
    let sub_layout = sub.map(|expr| layout_expr(expr, script_ctx));
    let sup_layout = sup.map(|expr| layout_expr(expr, script_ctx));
    let metrics = ctx.metrics();
    let script_gap = metrics.script_gap();
    let script_x = base_layout.width + script_gap;
    let sup_dy = sup_layout
        .as_ref()
        .map(|sup| metrics.superscript_shift(base_layout.ascent, sup.descent))
        .unwrap_or(0.0);
    let mut sub_dy = sub_layout
        .as_ref()
        .map(|sub| metrics.subscript_shift(base_layout.descent, sub.ascent))
        .unwrap_or(0.0);
    if let (Some(sub), Some(sup)) = (&sub_layout, &sup_layout) {
        let sup_bottom = sup_dy + sup.descent;
        let sub_top = sub_dy - sub.ascent;
        let gap = sub_top - sup_bottom;
        let min_gap = metrics.sub_superscript_gap();
        if gap < min_gap {
            sub_dy += min_gap - gap;
        }
    }
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, base_layout.atoms, 0.0, 0.0);
    let mut script_width: f32 = 0.0;
    let mut ascent = base_layout.ascent;
    let mut descent = base_layout.descent;
    if let Some(sup) = sup_layout {
        script_width = script_width.max(sup.width);
        ascent = ascent.max(-sup_dy + sup.ascent);
        translate_atoms(&mut atoms, sup.atoms, script_x, sup_dy);
    }
    if let Some(sub) = sub_layout {
        script_width = script_width.max(sub.width);
        descent = descent.max(sub_dy + sub.descent);
        translate_atoms(&mut atoms, sub.atoms, script_x, sub_dy);
    }
    MathLayout {
        width: base_layout.width + script_gap + script_width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_under_over(
    base: &MathExpr,
    under: Option<&MathExpr>,
    over: Option<&MathExpr>,
    ctx: LayoutCtx,
) -> MathLayout {
    let center_large_operator =
        matches!(ctx.display, MathDisplay::Block) && is_large_operator_base(base);
    let base_ctx = if center_large_operator {
        ctx.large_operator()
    } else {
        ctx
    };
    let base_layout = if center_large_operator {
        layout_large_operator_variant(base, base_ctx)
            .or_else(|| layout_operator_expr_glyph_fallback(base, base_ctx))
            .unwrap_or_else(|| layout_expr(base, ctx))
    } else {
        layout_expr(base, base_ctx)
    };
    let script_ctx = ctx.script();
    let under_layout = under.map(|expr| layout_expr(expr, script_ctx));
    let over_layout = over.map(|expr| layout_expr(expr, script_ctx));
    let metrics = ctx.metrics();
    let width = base_layout
        .width
        .max(under_layout.as_ref().map(|l| l.width).unwrap_or(0.0))
        .max(over_layout.as_ref().map(|l| l.width).unwrap_or(0.0));
    let base_x = (width - base_layout.width) * 0.5;
    let base_dy = if center_large_operator {
        base_ctx.metrics().math_axis_shift() - ctx.metrics().math_axis_shift()
    } else {
        0.0
    };
    let base_top = -base_layout.ascent + base_dy;
    let base_bottom = base_layout.descent + base_dy;
    let mut atoms = Vec::new();
    let mut ascent = -base_top;
    let mut descent = base_bottom;
    translate_atoms(&mut atoms, base_layout.atoms, base_x, base_dy);
    if let Some(over) = over_layout {
        let over_x = (width - over.width) * 0.5;
        let over_dy = (base_top - metrics.upper_limit_gap() - over.descent)
            .min(base_dy - metrics.upper_limit_baseline_rise());
        ascent = ascent.max(-over_dy + over.ascent);
        translate_atoms(&mut atoms, over.atoms, over_x, over_dy);
    }
    if let Some(under) = under_layout {
        let under_x = (width - under.width) * 0.5;
        let under_dy = (base_bottom + metrics.lower_limit_gap() + under.ascent)
            .max(base_dy + metrics.lower_limit_baseline_drop());
        descent = descent.max(under_dy + under.descent);
        translate_atoms(&mut atoms, under.atoms, under_x, under_dy);
    }
    MathLayout {
        width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_accent(base: &MathExpr, accent: &MathExpr, stretch: bool, ctx: LayoutCtx) -> MathLayout {
    let base_layout = layout_expr(base, ctx);
    if stretch && is_overline_accent(accent) {
        return layout_overline(base_layout, ctx);
    }

    let accent_layout = layout_accent_mark(accent, ctx.script());
    let metrics = ctx.metrics();
    let gap = metrics.accent_gap();
    let width = base_layout.width.max(accent_layout.width);
    let base_x = (width - base_layout.width) * 0.5;
    let accent_x = (width - accent_layout.width) * 0.5;
    let accent_dy = -base_layout.ascent - gap - accent_layout.descent;
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, base_layout.atoms, base_x, 0.0);
    translate_atoms(&mut atoms, accent_layout.atoms, accent_x, accent_dy);
    MathLayout {
        width,
        ascent: base_layout.ascent.max(-accent_dy + accent_layout.ascent),
        descent: base_layout.descent,
        atoms,
    }
}

fn layout_overline(base_layout: MathLayout, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let rule = metrics.rule_thickness();
    let gap = metrics.accent_gap();
    let rule_y = -base_layout.ascent - gap - rule;
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, base_layout.atoms, 0.0, 0.0);
    atoms.push(MathAtom::Rule {
        rect: Rect::new(0.0, rule_y, base_layout.width.max(rule), rule),
    });
    MathLayout {
        width: base_layout.width,
        ascent: (-rule_y).max(base_layout.ascent),
        descent: base_layout.descent,
        atoms,
    }
}

fn is_overline_accent(expr: &MathExpr) -> bool {
    matches!(
        expr.without_source(),
        MathExpr::Operator(s) | MathExpr::Text(s) | MathExpr::Identifier(s)
            if matches!(s.as_str(), "¯" | "‾")
    )
}

fn layout_accent_mark(accent: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    match accent.without_source() {
        MathExpr::Operator(s) if s == "^" => layout_operator("ˆ", ctx),
        MathExpr::Operator(s) if s == "~" => layout_operator("˜", ctx),
        _ => layout_expr(accent, ctx),
    }
}

fn is_display_limits_base(expr: &MathExpr) -> bool {
    match expr.without_source() {
        MathExpr::Operator(_) | MathExpr::OperatorWithMetadata { .. } => has_movable_limits(expr),
        MathExpr::Text(s) => matches!(
            s.as_str(),
            "lim" | "max" | "min" | "sup" | "inf" | "det" | "gcd" | "Pr" | "lim inf" | "lim sup"
        ),
        _ => false,
    }
}

fn has_movable_limits(expr: &MathExpr) -> bool {
    match expr.without_source() {
        MathExpr::Operator(s) => operator_info(s).movable_limits,
        MathExpr::OperatorWithMetadata {
            text,
            movable_limits,
            ..
        } => movable_limits.unwrap_or_else(|| operator_info(text).movable_limits),
        _ => false,
    }
}

fn is_large_operator_base(expr: &MathExpr) -> bool {
    match expr.without_source() {
        MathExpr::Operator(s) => is_large_operator_symbol_str(s),
        MathExpr::OperatorWithMetadata {
            text,
            large_operator,
            ..
        } => large_operator.unwrap_or_else(|| operator_info(text).large_operator),
        _ => false,
    }
}

fn is_large_operator_symbol_str(s: &str) -> bool {
    operator_info(s).large_operator
}

fn is_large_operator_symbol(ch: char) -> bool {
    operator_info(&ch.to_string()).large_operator
}

fn layout_large_operator_variant(expr: &MathExpr, ctx: LayoutCtx) -> Option<MathLayout> {
    let (operator, lspace_override, rspace_override) = match expr.without_source() {
        MathExpr::Operator(operator) => (operator.as_str(), None, None),
        MathExpr::OperatorWithMetadata {
            text,
            lspace,
            rspace,
            ..
        } => (text.as_str(), *lspace, *rspace),
        _ => return None,
    };
    let metrics = ctx.metrics();
    let variant = metrics.large_operator_variant_for_height(operator, ctx.size)?;
    let bbox = variant.bbox?;
    let constants = metrics.font_constants()?;
    let scale = metrics.size / constants.units_per_em;
    let view_box = glyph_advance_view_box(bbox, variant.horizontal_advance, None)?;
    let glyph_width = view_box.w * scale;
    let glyph_height = view_box.h * scale;
    if glyph_width <= 0.0 || glyph_height <= 0.0 {
        return None;
    }
    let width = (variant.horizontal_advance as f32 * scale).max(glyph_width);
    let target_center_y = -metrics.math_axis_shift();
    let glyph_center_y = view_box.y * scale + glyph_height * 0.5;
    let glyph_y = target_center_y - glyph_center_y;
    let (lspace, rspace) =
        metrics.operator_spacing_with_overrides(operator, lspace_override, rspace_override);
    let rect = Rect::new(
        lspace + (width - glyph_width) * 0.5,
        glyph_y + view_box.y * scale,
        glyph_width,
        glyph_height,
    );
    Some(MathLayout {
        width: width + lspace + rspace,
        ascent: -rect.y,
        descent: rect.y + rect.h,
        atoms: vec![MathAtom::GlyphId {
            glyph_id: variant.glyph_id,
            rect,
            view_box,
        }],
    })
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

fn layout_fenced(
    open: &Option<String>,
    close: &Option<String>,
    body: &MathExpr,
    ctx: LayoutCtx,
) -> MathLayout {
    let body_layout = layout_expr(body, ctx);
    let delimiter_rect = delimiter_rect(&body_layout, ctx);
    let metrics = ctx.metrics();
    let gap = metrics.delimiter_gap();
    let stretch_delimiters = metrics.should_stretch_delimiter(&body_layout);
    let open_layout = open
        .as_deref()
        .map(|delimiter| layout_delimiter(delimiter, delimiter_rect, stretch_delimiters, ctx));
    let close_layout = close
        .as_deref()
        .map(|delimiter| layout_delimiter(delimiter, delimiter_rect, stretch_delimiters, ctx));
    let open_width = open_layout
        .as_ref()
        .map(|layout| layout.width + gap)
        .unwrap_or(0.0);
    let close_width = close_layout
        .as_ref()
        .map(|layout| layout.width + gap)
        .unwrap_or(0.0);
    let delimiter_ascent = open_layout
        .as_ref()
        .into_iter()
        .chain(close_layout.as_ref())
        .map(|layout| layout.ascent)
        .fold(0.0, f32::max);
    let delimiter_descent = open_layout
        .as_ref()
        .into_iter()
        .chain(close_layout.as_ref())
        .map(|layout| layout.descent)
        .fold(0.0, f32::max);
    let mut atoms = Vec::new();
    if let Some(open) = open_layout {
        translate_atoms(&mut atoms, open.atoms, 0.0, 0.0);
    }
    translate_atoms(&mut atoms, body_layout.atoms, open_width, 0.0);
    if let Some(close) = close_layout {
        translate_atoms(
            &mut atoms,
            close.atoms,
            open_width + body_layout.width + gap,
            0.0,
        );
    }
    MathLayout {
        width: open_width + body_layout.width + close_width,
        ascent: body_layout.ascent.max(delimiter_ascent),
        descent: body_layout.descent.max(delimiter_descent),
        atoms,
    }
}

fn delimiter_rect(body: &MathLayout, ctx: LayoutCtx) -> Rect {
    let metrics = ctx.metrics();
    let overshoot = metrics.delimiter_overshoot();
    let top = -body.ascent - overshoot;
    let bottom = body.descent + overshoot;
    Rect::new(0.0, top, metrics.delimiter_width(), bottom - top)
}

fn layout_delimiter(delimiter: &str, rect: Rect, stretch: bool, ctx: LayoutCtx) -> MathLayout {
    if !stretch || !is_vector_delimiter(delimiter) {
        return layout_glyph(delimiter, ctx, FontWeight::Regular, false);
    }
    if let Some(delimiter) = delimiter
        .chars()
        .next()
        .filter(|_| delimiter.chars().count() == 1)
        && let Some(variant) = ctx
            .metrics()
            .delimiter_variant_for_height(delimiter, rect.h)
        && let Some(layout) = layout_delimiter_variant(variant, rect, ctx)
    {
        return layout;
    }
    if let Some(delimiter) = delimiter
        .chars()
        .next()
        .filter(|_| delimiter.chars().count() == 1)
        && let Some(parts) = ctx.metrics().delimiter_assembly_parts(delimiter)
        && let Some(layout) = layout_delimiter_assembly(&parts, rect, ctx)
    {
        return layout;
    }
    MathLayout {
        width: rect.w,
        ascent: -rect.y,
        descent: rect.y + rect.h,
        atoms: vec![MathAtom::Delimiter {
            delimiter: delimiter.to_string(),
            rect,
            thickness: ctx.metrics().rule_thickness(),
        }],
    }
}

fn is_vector_delimiter(delimiter: &str) -> bool {
    matches!(
        delimiter,
        "(" | ")" | "[" | "]" | "{" | "}" | "|" | "‖" | "⟨" | "⟩" | "⌊" | "⌋" | "⌈" | "⌉"
    )
}

fn layout_delimiter_variant(
    variant: OpenTypeDelimiterVariant,
    target_rect: Rect,
    ctx: LayoutCtx,
) -> Option<MathLayout> {
    let bbox = variant.bbox?;
    let metrics = ctx.metrics();
    let constants = metrics.font_constants()?;
    let scale = metrics.size / constants.units_per_em;
    let width = (variant.horizontal_advance as f32 * scale).max(target_rect.w);
    let view_box = glyph_advance_view_box(bbox, variant.horizontal_advance, None)?;
    let glyph_height = view_box.h * scale;
    if view_box.w <= 0.0 || glyph_height <= 0.0 {
        return None;
    }
    let target_center_y = target_rect.y + target_rect.h * 0.5;
    let glyph_center_y = view_box.y * scale + glyph_height * 0.5;
    let glyph_y = target_center_y - glyph_center_y;
    let rect = Rect::new(
        (width - view_box.w * scale) * 0.5,
        glyph_y + view_box.y * scale,
        view_box.w * scale,
        glyph_height,
    );
    Some(MathLayout {
        width,
        ascent: (-rect.y).max(-target_rect.y),
        descent: (rect.y + rect.h).max(target_rect.y + target_rect.h),
        atoms: vec![MathAtom::GlyphId {
            glyph_id: variant.glyph_id,
            rect,
            view_box,
        }],
    })
}

fn layout_delimiter_assembly(
    parts: &[OpenTypeDelimiterAssemblyPart],
    target_rect: Rect,
    ctx: LayoutCtx,
) -> Option<MathLayout> {
    let metrics = ctx.metrics();
    let constants = metrics.font_constants()?;
    if constants.units_per_em <= 0.0 {
        return None;
    }
    let scale = metrics.size / constants.units_per_em;
    let overlap_units = constants.min_connector_overlap.max(1);
    let target_units = target_rect.h / scale;
    let source_parts: Vec<OpenTypeDelimiterAssemblyPart> = parts.iter().rev().copied().collect();
    let mut assembly = source_parts.clone();
    let extender_parts: Vec<OpenTypeDelimiterAssemblyPart> = source_parts
        .iter()
        .copied()
        .filter(|part| part.extender)
        .collect();
    if extender_parts.is_empty() {
        return None;
    }

    let mut extra_repeats = 0;
    while assembly_max_length_units(&assembly, overlap_units) < target_units {
        extra_repeats += 1;
        assembly = Vec::with_capacity(source_parts.len() + extra_repeats * extender_parts.len());
        for part in &source_parts {
            assembly.push(*part);
            if part.extender {
                assembly.extend(std::iter::repeat_n(*part, extra_repeats));
            }
        }
    }

    let overlaps = assembly_overlaps_for_target(&assembly, target_units, overlap_units);
    let total_units = assembly_raw_advance_units(&assembly) - overlaps.iter().sum::<f32>();
    let total_height = total_units * scale;
    let target_center_y = target_rect.y + target_rect.h * 0.5;
    let top = target_center_y - total_height * 0.5;
    let width = assembly
        .iter()
        .filter_map(|part| {
            let bbox = part.bbox?;
            Some(
                (part.horizontal_advance as f32 * scale)
                    .max((bbox.x_max - bbox.x_min) as f32 * scale),
            )
        })
        .fold(target_rect.w, f32::max);

    let mut cursor_units = 0.0;
    let mut atoms = Vec::with_capacity(assembly.len());
    for (index, part) in assembly.iter().enumerate() {
        let bbox = part.bbox?;
        let slot_height = part.full_advance as f32 * scale;
        let view_box =
            glyph_advance_view_box(bbox, part.horizontal_advance, Some(part.full_advance))?;
        let glyph_width = view_box.w * scale;
        let glyph_height = view_box.h * scale;
        if glyph_width <= 0.0 || glyph_height <= 0.0 || slot_height <= 0.0 {
            return None;
        }
        let rect = Rect::new(
            (width - glyph_width) * 0.5,
            top + cursor_units * scale,
            glyph_width,
            slot_height.max(glyph_height),
        );
        atoms.push(MathAtom::GlyphId {
            glyph_id: part.glyph_id,
            rect,
            view_box,
        });
        if index + 1 < assembly.len() {
            cursor_units += part.full_advance as f32 - overlaps[index];
        }
    }

    Some(MathLayout {
        width,
        ascent: (-top).max(-target_rect.y),
        descent: (top + total_height).max(target_rect.y + target_rect.h),
        atoms,
    })
}

fn glyph_advance_view_box(
    bbox: OpenTypeGlyphBBox,
    horizontal_advance: u16,
    vertical_advance: Option<u16>,
) -> Option<Rect> {
    let x = (bbox.x_min as f32).min(0.0);
    let width = (horizontal_advance as f32)
        .max(bbox.x_max as f32 - x)
        .max((bbox.x_max - bbox.x_min) as f32);
    let y = -(bbox.y_max as f32);
    let height = vertical_advance
        .map(f32::from)
        .unwrap_or((bbox.y_max - bbox.y_min) as f32)
        .max((bbox.y_max - bbox.y_min) as f32);
    (width > 0.0 && height > 0.0).then(|| Rect::new(x, y, width, height))
}

fn assembly_raw_advance_units(parts: &[OpenTypeDelimiterAssemblyPart]) -> f32 {
    parts.iter().map(|part| part.full_advance as f32).sum()
}

fn assembly_max_length_units(parts: &[OpenTypeDelimiterAssemblyPart], min_overlap: u16) -> f32 {
    assembly_raw_advance_units(parts)
        - assembly_overlap_limits(parts, min_overlap)
            .iter()
            .map(|(min, _)| *min)
            .sum::<f32>()
}

fn assembly_overlap_limits(
    parts: &[OpenTypeDelimiterAssemblyPart],
    min_overlap: u16,
) -> Vec<(f32, f32)> {
    parts
        .windows(2)
        .map(|pair| {
            let min = min_overlap as f32;
            let max = pair[0]
                .end_connector_length
                .min(pair[1].start_connector_length)
                .max(min_overlap) as f32;
            (min, max)
        })
        .collect()
}

fn assembly_overlaps_for_target(
    parts: &[OpenTypeDelimiterAssemblyPart],
    target_units: f32,
    min_overlap: u16,
) -> Vec<f32> {
    let limits = assembly_overlap_limits(parts, min_overlap);
    if limits.is_empty() {
        return Vec::new();
    }
    let raw = assembly_raw_advance_units(parts);
    let min_sum: f32 = limits.iter().map(|(min, _)| *min).sum();
    let max_sum: f32 = limits.iter().map(|(_, max)| *max).sum();
    let desired_sum = (raw - target_units).clamp(min_sum, max_sum);
    let mut overlaps: Vec<f32> = limits.iter().map(|(min, _)| *min).collect();
    let mut remaining = desired_sum - min_sum;

    while remaining > 0.001 {
        let adjustable: Vec<usize> = overlaps
            .iter()
            .zip(limits.iter())
            .enumerate()
            .filter_map(|(index, (overlap, (_, max)))| (*overlap < *max - 0.001).then_some(index))
            .collect();
        if adjustable.is_empty() {
            break;
        }
        let share = remaining / adjustable.len() as f32;
        let mut distributed = 0.0;
        for index in adjustable {
            let capacity = limits[index].1 - overlaps[index];
            let add = share.min(capacity);
            overlaps[index] += add;
            distributed += add;
        }
        if distributed <= 0.001 {
            break;
        }
        remaining -= distributed;
    }

    overlaps
}

fn layout_table(
    rows: &[Vec<MathExpr>],
    column_alignments: &[MathColumnAlignment],
    column_gap: Option<f32>,
    row_gap: Option<f32>,
    ctx: LayoutCtx,
) -> MathLayout {
    if rows.is_empty() {
        return MathLayout {
            width: 0.0,
            ascent: 0.0,
            descent: 0.0,
            atoms: Vec::new(),
        };
    }
    let cell_layouts: Vec<Vec<MathLayout>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| layout_expr(cell, ctx)).collect())
        .collect();
    let metrics = ctx.metrics();
    let col_count = cell_layouts.iter().map(Vec::len).max().unwrap_or(0);
    let mut col_widths = vec![0.0_f32; col_count];
    let mut row_ascents = vec![metrics.default_ascent(); rows.len()];
    let mut row_descents = vec![metrics.default_descent(); rows.len()];
    for (row_index, row) in cell_layouts.iter().enumerate() {
        for (col_index, cell) in row.iter().enumerate() {
            col_widths[col_index] = col_widths[col_index].max(cell.width);
            row_ascents[row_index] = row_ascents[row_index].max(cell.ascent);
            row_descents[row_index] = row_descents[row_index].max(cell.descent);
        }
    }
    let col_gap = metrics.table_col_gap(column_gap);
    let row_gap = metrics.table_row_gap(row_gap);
    let width = col_widths.iter().sum::<f32>() + col_gap * col_count.saturating_sub(1) as f32;
    let row_heights: Vec<f32> = row_ascents
        .iter()
        .zip(row_descents.iter())
        .map(|(ascent, descent)| ascent + descent)
        .collect();
    let height = row_heights.iter().sum::<f32>() + row_gap * rows.len().saturating_sub(1) as f32;
    let baseline_origin = height * 0.5 + metrics.math_axis_shift();
    let mut atoms = Vec::new();
    let mut row_top = 0.0;
    for (row_index, row) in cell_layouts.into_iter().enumerate() {
        let row_baseline = row_top + row_ascents[row_index];
        let mut col_left = 0.0;
        for (col_index, cell) in row.into_iter().enumerate() {
            let col_extra = col_widths[col_index] - cell.width;
            let align = column_alignments
                .get(col_index)
                .copied()
                .unwrap_or_default();
            let cell_x = col_left
                + match align {
                    MathColumnAlignment::Left => 0.0,
                    MathColumnAlignment::Center => col_extra * 0.5,
                    MathColumnAlignment::Right => col_extra,
                };
            translate_atoms(
                &mut atoms,
                cell.atoms,
                cell_x,
                row_baseline - baseline_origin,
            );
            col_left += col_widths[col_index] + col_gap;
        }
        row_top += row_heights[row_index] + row_gap;
    }
    MathLayout {
        width,
        ascent: baseline_origin,
        descent: height - baseline_origin,
        atoms,
    }
}

fn translate_atoms(out: &mut Vec<MathAtom>, atoms: Vec<MathAtom>, dx: f32, dy: f32) {
    out.extend(atoms.into_iter().map(|atom| match atom {
        MathAtom::Glyph {
            text,
            x,
            y_baseline,
            size,
            weight,
            italic,
        } => MathAtom::Glyph {
            text,
            x: x + dx,
            y_baseline: y_baseline + dy,
            size,
            weight,
            italic,
        },
        MathAtom::GlyphId {
            glyph_id,
            rect,
            view_box,
        } => MathAtom::GlyphId {
            glyph_id,
            rect: Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h),
            view_box,
        },
        MathAtom::Rule { rect } => MathAtom::Rule {
            rect: Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h),
        },
        MathAtom::Radical { points, thickness } => MathAtom::Radical {
            points: points.map(|[x, y]| [x + dx, y + dy]),
            thickness,
        },
        MathAtom::Delimiter {
            delimiter,
            rect,
            thickness,
        } => MathAtom::Delimiter {
            delimiter,
            rect: Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h),
            thickness,
        },
    }));
}

/// Parses a TeX/LaTeX math string (math-mode content only, without `$`
/// delimiters) into a [`MathExpr`] tree.
///
/// Supports the common LaTeX math subset: symbol and function commands,
/// `_`/`^` scripts, `\frac`, `\sqrt[n]`, accents, stretchy fences via
/// `\left`/`\right`, and environments such as `matrix`/`pmatrix`/`bmatrix`,
/// `aligned`/`align`, and `cases`. Unknown commands degrade gracefully into
/// literal [`MathExpr::Identifier`] text (e.g. `\foo`); structural problems
/// (unbalanced braces, trailing input, unsupported environments) are
/// returned as [`MathParseError`].
pub fn parse_tex(input: &str) -> Result<MathExpr, MathParseError> {
    let mut parser = TexParser::new(input);
    let expr = parser.parse_row(None)?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err(parser.error("unexpected trailing input"));
    }
    Ok(expr)
}

/// Like [`parse_tex`], but wraps parsed sub-expressions in
/// [`MathExpr::Source`] nodes recording their byte ranges in `input`.
///
/// The wrappers are transparent to layout; consumers (e.g. editors mapping
/// rendered math back to source) read them via [`MathExpr::source_range`].
pub fn parse_tex_with_source_ranges(input: &str) -> Result<MathExpr, MathParseError> {
    let mut parser = TexParser::with_source_ranges(input);
    let expr = parser.parse_row(None)?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err(parser.error("unexpected trailing input"));
    }
    Ok(expr)
}

/// Parses a MathML Core document (a `<math>` element) into a [`MathExpr`]
/// tree, discarding the root's `display` attribute; see
/// [`parse_mathml_with_display`] to keep it.
pub fn parse_mathml(input: &str) -> Result<MathExpr, MathParseError> {
    Ok(parse_mathml_with_display(input)?.0)
}

/// Parses a MathML Core document into a [`MathExpr`] tree together with the
/// [`MathDisplay`] taken from the root element's `display` attribute
/// (`"block"` maps to [`MathDisplay::Block`], anything else to
/// [`MathDisplay::Inline`]).
///
/// Supported elements map 1:1 onto [`MathExpr`] variants; unsupported
/// elements become [`MathExpr::Error`] nodes rather than failing the parse.
/// Malformed XML and arity errors are returned as [`MathParseError`].
pub fn parse_mathml_with_display(input: &str) -> Result<(MathExpr, MathDisplay), MathParseError> {
    let doc = roxmltree::Document::parse(input).map_err(|err| {
        let pos = err.pos();
        MathParseError {
            message: err.to_string(),
            byte: text_pos_to_byte(input, pos.row, pos.col),
        }
    })?;
    let root = doc.root_element();
    let display = match root.attribute("display") {
        Some("block") => MathDisplay::Block,
        _ => MathDisplay::Inline,
    };
    let expr = parse_mathml_node(root)?;
    Ok((expr, display))
}

/// Error from [`parse_tex`] or [`parse_mathml`], with a position for
/// diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathParseError {
    /// Human-readable description of what went wrong.
    pub message: String,
    /// Byte offset into the input string where the error was detected.
    pub byte: usize,
}

fn parse_mathml_node(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let name = node.tag_name().name();
    match name {
        "math" | "mrow" => Ok(MathExpr::row(parse_mathml_children(node)?)),
        "mi" => Ok(MathExpr::Identifier(normalized_node_text(node))),
        "mn" => Ok(MathExpr::Number(normalized_node_text(node))),
        "mo" => parse_mathml_operator(node),
        "mtext" => Ok(MathExpr::Text(normalized_node_text(node))),
        "mspace" => Ok(MathExpr::Space(parse_mathml_space(node))),
        "mfrac" => {
            let children = mathml_element_children(node);
            require_mathml_arity(node, &children, 2)?;
            Ok(MathExpr::Fraction {
                numerator: Arc::new(parse_mathml_node(children[0])?),
                denominator: Arc::new(parse_mathml_node(children[1])?),
            })
        }
        "msqrt" => Ok(MathExpr::Sqrt(Arc::new(MathExpr::row(
            parse_mathml_children(node)?,
        )))),
        "mroot" => {
            let children = mathml_element_children(node);
            require_mathml_arity(node, &children, 2)?;
            Ok(MathExpr::Root {
                base: Arc::new(parse_mathml_node(children[0])?),
                index: Arc::new(parse_mathml_node(children[1])?),
            })
        }
        "msub" => parse_mathml_scripts(node, true, false),
        "msup" => parse_mathml_scripts(node, false, true),
        "msubsup" => parse_mathml_scripts(node, true, true),
        "munder" => parse_mathml_under_over(node, true, false),
        "mover" if mathml_bool_attr(node.attribute("accent")) => parse_mathml_accent(node),
        "mover" => parse_mathml_under_over(node, false, true),
        "munderover" => parse_mathml_under_over(node, true, true),
        "mfenced" => parse_mathml_fenced(node),
        "semantics" => parse_mathml_semantics(node),
        "mtable" => parse_mathml_table(node),
        "mtr" => Ok(MathExpr::row(
            mathml_element_children(node)
                .into_iter()
                .map(parse_mathml_node)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "mtd" => Ok(MathExpr::row(parse_mathml_children(node)?)),
        unsupported => Ok(MathExpr::Error(format!(
            "unsupported MathML element <{unsupported}>"
        ))),
    }
}

fn parse_mathml_children(node: roxmltree::Node<'_, '_>) -> Result<Vec<MathExpr>, MathParseError> {
    mathml_element_children(node)
        .into_iter()
        .map(parse_mathml_node)
        .collect()
}

fn parse_mathml_operator(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let operator = normalized_node_text(node);
    let lspace = node.attribute("lspace").and_then(parse_em_length);
    let rspace = node.attribute("rspace").and_then(parse_em_length);
    let large_operator = node.attribute("largeop").map(mathml_bool_attr_value);
    let movable_limits = node.attribute("movablelimits").map(mathml_bool_attr_value);
    if lspace.is_none() && rspace.is_none() && large_operator.is_none() && movable_limits.is_none()
    {
        return Ok(MathExpr::Operator(operator));
    }
    Ok(MathExpr::OperatorWithMetadata {
        text: operator,
        lspace,
        rspace,
        large_operator,
        movable_limits,
    })
}

fn parse_mathml_semantics(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let children = mathml_element_children(node);
    let Some(presentation) = children
        .into_iter()
        .find(|child| !matches!(child.tag_name().name(), "annotation" | "annotation-xml"))
    else {
        return Err(mathml_error_at(
            node,
            "<semantics> expected a presentation child".to_string(),
        ));
    };
    parse_mathml_node(presentation)
}

fn mathml_element_children<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
) -> Vec<roxmltree::Node<'a, 'input>> {
    node.children()
        .filter(roxmltree::Node::is_element)
        .collect()
}

fn require_mathml_arity(
    node: roxmltree::Node<'_, '_>,
    children: &[roxmltree::Node<'_, '_>],
    expected: usize,
) -> Result<(), MathParseError> {
    if children.len() == expected {
        Ok(())
    } else {
        Err(mathml_error_at(
            node,
            format!(
                "<{}> expected {expected} element children, got {}",
                node.tag_name().name(),
                children.len()
            ),
        ))
    }
}

fn parse_mathml_scripts(
    node: roxmltree::Node<'_, '_>,
    has_sub: bool,
    has_sup: bool,
) -> Result<MathExpr, MathParseError> {
    let children = mathml_element_children(node);
    let expected = 1 + usize::from(has_sub) + usize::from(has_sup);
    require_mathml_arity(node, &children, expected)?;
    let base = Arc::new(parse_mathml_node(children[0])?);
    let sub = has_sub.then(|| {
        let index = 1;
        parse_mathml_node(children[index]).map(Arc::new)
    });
    let sup = has_sup.then(|| {
        let index = if has_sub { 2 } else { 1 };
        parse_mathml_node(children[index]).map(Arc::new)
    });
    Ok(MathExpr::Scripts {
        base,
        sub: sub.transpose()?,
        sup: sup.transpose()?,
    })
}

fn parse_mathml_under_over(
    node: roxmltree::Node<'_, '_>,
    has_under: bool,
    has_over: bool,
) -> Result<MathExpr, MathParseError> {
    let children = mathml_element_children(node);
    let expected = 1 + usize::from(has_under) + usize::from(has_over);
    require_mathml_arity(node, &children, expected)?;
    let base = Arc::new(parse_mathml_node(children[0])?);
    let under = has_under.then(|| {
        let index = 1;
        parse_mathml_node(children[index]).map(Arc::new)
    });
    let over = has_over.then(|| {
        let index = if has_under { 2 } else { 1 };
        parse_mathml_node(children[index]).map(Arc::new)
    });
    Ok(MathExpr::UnderOver {
        base,
        under: under.transpose()?,
        over: over.transpose()?,
    })
}

fn parse_mathml_accent(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let children = mathml_element_children(node);
    require_mathml_arity(node, &children, 2)?;
    let accent = parse_mathml_node(children[1])?;
    let stretch =
        mathml_bool_attr(children[1].attribute("stretchy")) || is_overline_accent(&accent);
    Ok(MathExpr::Accent {
        base: Arc::new(parse_mathml_node(children[0])?),
        accent: Arc::new(accent),
        stretch,
    })
}

fn mathml_bool_attr(value: Option<&str>) -> bool {
    value.is_some_and(mathml_bool_attr_value)
}

fn mathml_bool_attr_value(value: &str) -> bool {
    matches!(value.trim(), "true" | "1")
}

fn parse_mathml_table(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let mut rows = Vec::new();
    for row_node in mathml_element_children(node) {
        if !matches!(row_node.tag_name().name(), "mtr" | "mlabeledtr") {
            return Err(mathml_error_at(
                row_node,
                format!(
                    "<mtable> expected row element children, got <{}>",
                    row_node.tag_name().name()
                ),
            ));
        }
        let mut row = Vec::new();
        for cell_node in mathml_element_children(row_node) {
            require_mathml_tag(cell_node, "mtd")?;
            row.push(MathExpr::row(parse_mathml_children(cell_node)?));
        }
        rows.push(row);
    }
    let column_alignments = parse_mathml_column_alignments(node.attribute("columnalign"))?;
    let column_gap = parse_mathml_table_spacing(node.attribute("columnspacing"))?;
    let row_gap = parse_mathml_table_spacing(node.attribute("rowspacing"))?;
    Ok(MathExpr::Table {
        rows,
        column_alignments,
        column_gap,
        row_gap,
    })
}

fn parse_mathml_column_alignments(
    value: Option<&str>,
) -> Result<Vec<MathColumnAlignment>, MathParseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .split_whitespace()
        .map(|token| match token {
            "left" => Ok(MathColumnAlignment::Left),
            "center" => Ok(MathColumnAlignment::Center),
            "right" => Ok(MathColumnAlignment::Right),
            "decimal" => Ok(MathColumnAlignment::Right),
            other => Err(MathParseError {
                message: format!("unsupported MathML columnalign value {other:?}"),
                byte: 0,
            }),
        })
        .collect()
}

fn parse_mathml_table_spacing(value: Option<&str>) -> Result<Option<f32>, MathParseError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(first) = value.split_whitespace().next() else {
        return Ok(None);
    };
    parse_mathml_em_length(first).map(Some)
}

fn parse_mathml_em_length(value: &str) -> Result<f32, MathParseError> {
    let number = value.strip_suffix("em").unwrap_or(value);
    let parsed = number.parse::<f32>().map_err(|_| MathParseError {
        message: format!("unsupported MathML table spacing value {value:?}"),
        byte: 0,
    })?;
    if parsed.is_sign_negative() {
        return Err(MathParseError {
            message: format!("negative MathML table spacing value {value:?}"),
            byte: 0,
        });
    }
    Ok(parsed)
}

fn parse_mathml_fenced(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let open = parse_fence_attr(node.attribute("open").unwrap_or("("));
    let close = parse_fence_attr(node.attribute("close").unwrap_or(")"));
    let separator = match node.attribute("separators") {
        Some(value) => value
            .chars()
            .find(|ch| !ch.is_whitespace())
            .map(|ch| ch.to_string()),
        None => Some(",".to_string()),
    };
    let children = parse_mathml_children(node)?;
    let mut body = Vec::new();
    for (index, child) in children.into_iter().enumerate() {
        if index > 0
            && let Some(separator) = &separator
        {
            body.push(MathExpr::Operator(separator.clone()));
        }
        body.push(child);
    }
    Ok(MathExpr::Fenced {
        open,
        close,
        body: Arc::new(MathExpr::row(body)),
    })
}

fn parse_fence_attr(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "." {
        None
    } else {
        Some(value.to_string())
    }
}

fn require_mathml_tag(node: roxmltree::Node<'_, '_>, expected: &str) -> Result<(), MathParseError> {
    if node.tag_name().name() == expected {
        Ok(())
    } else {
        Err(mathml_error_at(
            node,
            format!(
                "expected <{expected}> element, got <{}>",
                node.tag_name().name()
            ),
        ))
    }
}

fn normalized_node_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(roxmltree::Node::is_text)
        .filter_map(|n| n.text())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_mathml_space(node: roxmltree::Node<'_, '_>) -> f32 {
    node.attribute("width")
        .and_then(parse_em_length)
        .unwrap_or(0.3)
}

fn parse_em_length(s: &str) -> Option<f32> {
    let trimmed = s.trim();
    if let Some(number) = trimmed.strip_suffix("em") {
        return number.trim().parse().ok();
    }
    if let Some(number) = trimmed.strip_suffix("px") {
        return number.trim().parse::<f32>().ok().map(|px| px / 16.0);
    }
    trimmed.parse().ok()
}

fn mathml_error_at(node: roxmltree::Node<'_, '_>, message: String) -> MathParseError {
    MathParseError {
        message,
        byte: node.range().start,
    }
}

fn text_pos_to_byte(input: &str, row: u32, col: u32) -> usize {
    let mut current_row = 1;
    let mut current_col = 1;
    for (byte, ch) in input.char_indices() {
        if current_row == row && current_col == col {
            return byte;
        }
        if ch == '\n' {
            current_row += 1;
            current_col = 1;
        } else {
            current_col += 1;
        }
    }
    input.len()
}

struct TexParser<'a> {
    input: &'a str,
    pos: usize,
    source_ranges: bool,
}

impl<'a> TexParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            source_ranges: false,
        }
    }

    fn with_source_ranges(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            source_ranges: true,
        }
    }

    fn source_wrap(&self, start: usize, expr: MathExpr) -> MathExpr {
        if self.source_ranges {
            MathExpr::Source {
                source: start..self.pos,
                body: Arc::new(expr),
            }
        } else {
            expr
        }
    }

    fn parse_row(&mut self, until: Option<char>) -> Result<MathExpr, MathParseError> {
        let start = self.pos;
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.starts_with_command("right") {
                return Err(self.error("unexpected \\right"));
            }
            match self.peek() {
                None => {
                    if until.is_some() {
                        return Err(self.error("unclosed group"));
                    }
                    break;
                }
                Some(ch) if Some(ch) == until => {
                    self.bump();
                    break;
                }
                Some('}') => return Err(self.error("unexpected closing brace")),
                _ => {
                    let atom = self.parse_atom_with_scripts()?;
                    items.push(atom);
                }
            }
        }
        let expr = MathExpr::row(items);
        Ok(self.source_wrap(start, expr))
    }

    fn parse_row_until_right(&mut self) -> Result<MathExpr, MathParseError> {
        let start = self.pos;
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                return Err(self.error("unclosed \\left"));
            }
            if self.starts_with_command("right") {
                break;
            }
            if self.peek() == Some('}') {
                return Err(self.error("unexpected closing brace"));
            }
            let atom = self.parse_atom_with_scripts()?;
            items.push(atom);
        }
        let expr = MathExpr::row(items);
        Ok(self.source_wrap(start, expr))
    }

    fn parse_table_environment(
        &mut self,
        env: &str,
        column_alignments: Vec<MathColumnAlignment>,
        column_gap: Option<f32>,
        row_gap: Option<f32>,
    ) -> Result<MathExpr, MathParseError> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut cell = Vec::new();

        loop {
            self.skip_ws();
            if self.peek().is_none() {
                return Err(self.error(&format!("unclosed \\begin{{{env}}}")));
            }
            if self.starts_with_command("end") {
                self.consume_environment_end(env)?;
                if !row.is_empty() || !cell.is_empty() || rows.is_empty() {
                    row.push(MathExpr::row(std::mem::take(&mut cell)));
                    rows.push(row);
                }
                break;
            }
            if self.peek() == Some('&') {
                self.bump();
                row.push(MathExpr::row(std::mem::take(&mut cell)));
                continue;
            }
            if self.starts_with_row_separator() {
                self.consume_row_separator()?;
                row.push(MathExpr::row(std::mem::take(&mut cell)));
                rows.push(std::mem::take(&mut row));
                continue;
            }

            cell.push(self.parse_atom_with_scripts()?);
        }

        self.validate_tex_table_shape(env, &rows, &column_alignments)?;

        let table = MathExpr::Table {
            rows,
            column_alignments,
            column_gap,
            row_gap,
        };
        Ok(match env {
            "matrix" | "array" | "aligned" | "align" => table,
            "pmatrix" => MathExpr::Fenced {
                open: Some("(".into()),
                close: Some(")".into()),
                body: Arc::new(table),
            },
            "bmatrix" => MathExpr::Fenced {
                open: Some("[".into()),
                close: Some("]".into()),
                body: Arc::new(table),
            },
            "Bmatrix" => MathExpr::Fenced {
                open: Some("{".into()),
                close: Some("}".into()),
                body: Arc::new(table),
            },
            "vmatrix" => MathExpr::Fenced {
                open: Some("|".into()),
                close: Some("|".into()),
                body: Arc::new(table),
            },
            "Vmatrix" => MathExpr::Fenced {
                open: Some("‖".into()),
                close: Some("‖".into()),
                body: Arc::new(table),
            },
            "cases" => MathExpr::Fenced {
                open: Some("{".into()),
                close: None,
                body: Arc::new(table),
            },
            _ => return Err(self.error(&format!("unsupported math environment {env}"))),
        })
    }

    fn validate_tex_table_shape(
        &self,
        env: &str,
        rows: &[Vec<MathExpr>],
        column_alignments: &[MathColumnAlignment],
    ) -> Result<(), MathParseError> {
        let Some(first_row) = rows.first() else {
            return Ok(());
        };
        let expected_cols = first_row.len();
        for (row_index, row) in rows.iter().enumerate().skip(1) {
            if row.len() != expected_cols {
                return Err(self.error(&format!(
                    "inconsistent column count in {env}: row {} has {}, expected {expected_cols}",
                    row_index + 1,
                    row.len()
                )));
            }
        }
        if !column_alignments.is_empty() && column_alignments.len() != expected_cols {
            return Err(self.error(&format!(
                "{env} alignment spec has {} columns, but table has {expected_cols}",
                column_alignments.len()
            )));
        }
        Ok(())
    }

    fn parse_atom_with_scripts(&mut self) -> Result<MathExpr, MathParseError> {
        let start = self.pos;
        let mut base = self.parse_atom()?;
        let mut sub = None;
        let mut sup = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('_') => {
                    // TeX errors on `x_1_2` ("Double subscript") rather
                    // than silently keeping one of the scripts; match it
                    // so the dropped script is a reported error, not
                    // silent data loss.
                    if sub.is_some() {
                        return Err(self.error(
                            "double subscript: brace the first script, e.g. `x_{1_2}` or `{x_1}_2`",
                        ));
                    }
                    self.bump();
                    sub = Some(Arc::new(self.parse_script_arg()?));
                }
                Some('^') => {
                    if sup.is_some() {
                        return Err(self.error("double superscript: brace the first script, e.g. `x^{1^2}` or `{x^1}^2`"));
                    }
                    self.bump();
                    sup = Some(Arc::new(self.parse_script_arg()?));
                }
                _ => break,
            }
        }
        if sub.is_some() || sup.is_some() {
            base = MathExpr::Scripts {
                base: Arc::new(base),
                sub,
                sup,
            };
            base = self.source_wrap(start, base);
        }
        Ok(base)
    }

    fn parse_script_arg(&mut self) -> Result<MathExpr, MathParseError> {
        self.skip_ws();
        if self.peek() == Some('{') {
            self.bump();
            self.parse_row(Some('}'))
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> Result<MathExpr, MathParseError> {
        self.skip_ws();
        let start = self.pos;
        match self.peek() {
            Some('{') => {
                self.bump();
                let expr = self.parse_row(Some('}'))?;
                Ok(self.source_wrap(start, expr))
            }
            Some('\\') => self.parse_command(),
            Some(ch) if ch.is_ascii_digit() => {
                let text = self.take_while(|c| c.is_ascii_digit() || c == '.');
                Ok(self.source_wrap(start, MathExpr::Number(text)))
            }
            Some(ch) if ch.is_alphabetic() => {
                self.bump();
                Ok(self.source_wrap(start, MathExpr::Identifier(ch.to_string())))
            }
            Some(ch) => {
                self.bump();
                let expr = if ch.is_whitespace() {
                    MathExpr::Space(0.3)
                } else {
                    MathExpr::Operator(ch.to_string())
                };
                Ok(self.source_wrap(start, expr))
            }
            None => Err(self.error("expected math atom")),
        }
    }

    fn parse_command(&mut self) -> Result<MathExpr, MathParseError> {
        let start = self.pos;
        let expr = self.parse_command_unwrapped()?;
        Ok(self.source_wrap(start, expr))
    }

    fn parse_command_unwrapped(&mut self) -> Result<MathExpr, MathParseError> {
        self.expect('\\')?;
        let name = self.take_while(|c| c.is_ascii_alphabetic());
        if name.is_empty() {
            let escaped = self
                .bump()
                .ok_or_else(|| self.error("expected escaped character"))?;
            return Ok(match escaped {
                ',' => MathExpr::Space(THIN_MATH_SPACE_EM),
                ':' => MathExpr::Space(MEDIUM_MATH_SPACE_EM),
                ';' => MathExpr::Space(THICK_MATH_SPACE_EM),
                '!' => MathExpr::Space(-THIN_MATH_SPACE_EM),
                ' ' => MathExpr::Space(MEDIUM_MATH_SPACE_EM),
                _ => MathExpr::Operator(escaped.to_string()),
            });
        }
        match name.as_str() {
            "frac" | "tfrac" | "dfrac" => {
                let numerator = Arc::new(self.parse_required_group()?);
                let denominator = Arc::new(self.parse_required_group()?);
                Ok(MathExpr::Fraction {
                    numerator,
                    denominator,
                })
            }
            "binom" => {
                let numerator = self.parse_required_group()?;
                let denominator = self.parse_required_group()?;
                Ok(MathExpr::Fenced {
                    open: Some("(".into()),
                    close: Some(")".into()),
                    body: Arc::new(MathExpr::Table {
                        rows: vec![vec![numerator], vec![denominator]],
                        column_alignments: Vec::new(),
                        column_gap: None,
                        row_gap: None,
                    }),
                })
            }
            "sqrt" => {
                let index = self.parse_optional_bracket_group()?;
                let base = Arc::new(self.parse_required_group()?);
                Ok(match index {
                    Some(index) => MathExpr::Root {
                        base,
                        index: Arc::new(index),
                    },
                    None => MathExpr::Sqrt(base),
                })
            }
            "hat" | "widehat" => Ok(MathExpr::Accent {
                base: Arc::new(self.parse_required_group()?),
                accent: Arc::new(MathExpr::Operator("ˆ".into())),
                stretch: false,
            }),
            "bar" => Ok(MathExpr::Accent {
                base: Arc::new(self.parse_required_group()?),
                accent: Arc::new(MathExpr::Operator("¯".into())),
                stretch: false,
            }),
            "overline" => Ok(MathExpr::Accent {
                base: Arc::new(self.parse_required_group()?),
                accent: Arc::new(MathExpr::Operator("‾".into())),
                stretch: true,
            }),
            "vec" => Ok(MathExpr::Accent {
                base: Arc::new(self.parse_required_group()?),
                accent: Arc::new(MathExpr::Operator("→".into())),
                stretch: false,
            }),
            "tilde" | "widetilde" => Ok(MathExpr::Accent {
                base: Arc::new(self.parse_required_group()?),
                accent: Arc::new(MathExpr::Operator("˜".into())),
                stretch: false,
            }),
            "left" => {
                let open = self.parse_delimiter()?;
                let body = Arc::new(self.parse_row_until_right()?);
                self.consume_command("right")?;
                let close = self.parse_delimiter()?;
                Ok(MathExpr::Fenced { open, close, body })
            }
            "right" => Err(self.error("unexpected \\right")),
            "begin" => {
                let env = self.parse_environment_name()?;
                match env.as_str() {
                    "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix"
                    | "cases" | "aligned" | "align" => {
                        let options = default_tex_table_options(&env);
                        self.parse_table_environment(
                            &env,
                            options.column_alignments,
                            options.column_gap,
                            options.row_gap,
                        )
                    }
                    "array" => {
                        let column_alignments = self.parse_array_column_alignments()?;
                        self.parse_table_environment(&env, column_alignments, None, None)
                    }
                    _ => Err(self.error(&format!("unsupported math environment {env}"))),
                }
            }
            "end" => Err(self.error("unexpected \\end")),
            "text" | "mathrm" | "operatorname" => Ok(MathExpr::Text(self.parse_text_group()?)),
            "mathbf" | "boldsymbol" | "mathcal" => self.parse_required_group(),
            "mathbb" => {
                let expr = self.parse_required_group()?;
                Ok(map_mathbb_expr(expr))
            }
            // Greek lowercase
            "alpha" => Ok(MathExpr::Identifier("α".into())),
            "beta" => Ok(MathExpr::Identifier("β".into())),
            "gamma" => Ok(MathExpr::Identifier("γ".into())),
            "delta" => Ok(MathExpr::Identifier("δ".into())),
            "varepsilon" | "epsilon" => Ok(MathExpr::Identifier("ε".into())),
            "zeta" => Ok(MathExpr::Identifier("ζ".into())),
            "eta" => Ok(MathExpr::Identifier("η".into())),
            "theta" => Ok(MathExpr::Identifier("θ".into())),
            "vartheta" => Ok(MathExpr::Identifier("ϑ".into())),
            "iota" => Ok(MathExpr::Identifier("ι".into())),
            "kappa" => Ok(MathExpr::Identifier("κ".into())),
            "varkappa" => Ok(MathExpr::Identifier("ϰ".into())),
            "lambda" => Ok(MathExpr::Identifier("λ".into())),
            "mu" => Ok(MathExpr::Identifier("μ".into())),
            "nu" => Ok(MathExpr::Identifier("ν".into())),
            "xi" => Ok(MathExpr::Identifier("ξ".into())),
            "pi" => Ok(MathExpr::Identifier("π".into())),
            "varpi" => Ok(MathExpr::Identifier("ϖ".into())),
            "rho" => Ok(MathExpr::Identifier("ρ".into())),
            "varrho" => Ok(MathExpr::Identifier("ϱ".into())),
            "sigma" => Ok(MathExpr::Identifier("σ".into())),
            "varsigma" => Ok(MathExpr::Identifier("ς".into())),
            "tau" => Ok(MathExpr::Identifier("τ".into())),
            "upsilon" => Ok(MathExpr::Identifier("υ".into())),
            "phi" | "varphi" => Ok(MathExpr::Identifier("φ".into())),
            "chi" => Ok(MathExpr::Identifier("χ".into())),
            "psi" => Ok(MathExpr::Identifier("ψ".into())),
            "omega" => Ok(MathExpr::Identifier("ω".into())),
            // Greek uppercase
            "Gamma" => Ok(MathExpr::Identifier("Γ".into())),
            "Delta" => Ok(MathExpr::Identifier("Δ".into())),
            "Theta" => Ok(MathExpr::Identifier("Θ".into())),
            "Lambda" => Ok(MathExpr::Identifier("Λ".into())),
            "Xi" => Ok(MathExpr::Identifier("Ξ".into())),
            "Pi" => Ok(MathExpr::Identifier("Π".into())),
            "Sigma" => Ok(MathExpr::Identifier("Σ".into())),
            "Upsilon" => Ok(MathExpr::Identifier("Υ".into())),
            "Phi" => Ok(MathExpr::Identifier("Φ".into())),
            "Psi" => Ok(MathExpr::Identifier("Ψ".into())),
            "Omega" => Ok(MathExpr::Identifier("Ω".into())),
            // Other identifiers
            "partial" => Ok(MathExpr::Identifier("∂".into())),
            "infty" => Ok(MathExpr::Identifier("∞".into())),
            "hbar" => Ok(MathExpr::Identifier("ℏ".into())),
            "Re" => Ok(MathExpr::Identifier("ℜ".into())),
            "Im" => Ok(MathExpr::Identifier("ℑ".into())),
            "aleph" => Ok(MathExpr::Identifier("ℵ".into())),
            "beth" => Ok(MathExpr::Identifier("ℶ".into())),
            "wp" => Ok(MathExpr::Identifier("℘".into())),
            "ell" => Ok(MathExpr::Identifier("ℓ".into())),
            "emptyset" | "varnothing" => Ok(MathExpr::Identifier("∅".into())),
            "triangle" => Ok(MathExpr::Identifier("△".into())),
            "square" => Ok(MathExpr::Identifier("□".into())),
            "flat" => Ok(MathExpr::Identifier("♭".into())),
            "sharp" => Ok(MathExpr::Identifier("♯".into())),
            "natural" => Ok(MathExpr::Identifier("♮".into())),
            // Binary operators
            "pm" => Ok(MathExpr::Operator("±".into())),
            "mp" => Ok(MathExpr::Operator("∓".into())),
            "cdot" => Ok(MathExpr::Operator("·".into())),
            "times" => Ok(MathExpr::Operator("×".into())),
            "div" => Ok(MathExpr::Operator("÷".into())),
            "cup" => Ok(MathExpr::Operator("∪".into())),
            "cap" => Ok(MathExpr::Operator("∩".into())),
            "wedge" | "land" => Ok(MathExpr::Operator("∧".into())),
            "vee" | "lor" => Ok(MathExpr::Operator("∨".into())),
            "oplus" => Ok(MathExpr::Operator("⊕".into())),
            "ominus" => Ok(MathExpr::Operator("⊖".into())),
            "otimes" => Ok(MathExpr::Operator("⊗".into())),
            "oslash" => Ok(MathExpr::Operator("⊘".into())),
            "odot" => Ok(MathExpr::Operator("⊙".into())),
            "circ" => Ok(MathExpr::Operator("∘".into())),
            "bullet" => Ok(MathExpr::Operator("•".into())),
            "star" => Ok(MathExpr::Operator("⋆".into())),
            "ast" => Ok(MathExpr::Operator("∗".into())),
            "sqcup" => Ok(MathExpr::Operator("⊔".into())),
            "sqcap" => Ok(MathExpr::Operator("⊓".into())),
            "amalg" => Ok(MathExpr::Operator("⨿".into())),
            "wr" => Ok(MathExpr::Operator("≀".into())),
            "triangleleft" => Ok(MathExpr::Operator("◁".into())),
            "triangleright" => Ok(MathExpr::Operator("▷".into())),
            "diamond" => Ok(MathExpr::Operator("⋄".into())),
            "setminus" | "smallsetminus" => Ok(MathExpr::Operator("∖".into())),
            // Relations
            "approx" => Ok(MathExpr::Operator("≈".into())),
            "sim" => Ok(MathExpr::Operator("∼".into())),
            "simeq" => Ok(MathExpr::Operator("≃".into())),
            "cong" => Ok(MathExpr::Operator("≅".into())),
            "equiv" => Ok(MathExpr::Operator("≡".into())),
            "propto" => Ok(MathExpr::Operator("∝".into())),
            "le" | "leq" => Ok(MathExpr::Operator("≤".into())),
            "ge" | "geq" => Ok(MathExpr::Operator("≥".into())),
            "ne" | "neq" => Ok(MathExpr::Operator("≠".into())),
            "ll" => Ok(MathExpr::Operator("≪".into())),
            "gg" => Ok(MathExpr::Operator("≫".into())),
            "prec" => Ok(MathExpr::Operator("≺".into())),
            "succ" => Ok(MathExpr::Operator("≻".into())),
            "preceq" => Ok(MathExpr::Operator("⪯".into())),
            "succeq" => Ok(MathExpr::Operator("⪰".into())),
            "parallel" => Ok(MathExpr::Operator("∥".into())),
            "perp" => Ok(MathExpr::Operator("⊥".into())),
            "asymp" => Ok(MathExpr::Operator("≍".into())),
            "doteq" => Ok(MathExpr::Operator("≐".into())),
            "models" => Ok(MathExpr::Operator("⊨".into())),
            "vdash" => Ok(MathExpr::Operator("⊢".into())),
            "dashv" => Ok(MathExpr::Operator("⊣".into())),
            "therefore" => Ok(MathExpr::Operator("∴".into())),
            "because" => Ok(MathExpr::Operator("∵".into())),
            // Set theory
            "in" => Ok(MathExpr::Operator("∈".into())),
            "notin" => Ok(MathExpr::Operator("∉".into())),
            "ni" => Ok(MathExpr::Operator("∋".into())),
            "subset" => Ok(MathExpr::Operator("⊂".into())),
            "supset" => Ok(MathExpr::Operator("⊃".into())),
            "subseteq" => Ok(MathExpr::Operator("⊆".into())),
            "supseteq" => Ok(MathExpr::Operator("⊇".into())),
            "subsetneq" => Ok(MathExpr::Operator("⊊".into())),
            "supsetneq" => Ok(MathExpr::Operator("⊋".into())),
            "complement" => Ok(MathExpr::Operator("∁".into())),
            // Logic / quantifiers
            "forall" => Ok(MathExpr::Operator("∀".into())),
            "exists" => Ok(MathExpr::Operator("∃".into())),
            "nexists" => Ok(MathExpr::Operator("∄".into())),
            "neg" | "lnot" => Ok(MathExpr::Operator("¬".into())),
            "top" => Ok(MathExpr::Operator("⊤".into())),
            "bot" => Ok(MathExpr::Operator("⊥".into())),
            "implies" => Ok(extra_wide_operator("⟹")),
            "impliedby" => Ok(extra_wide_operator("⟸")),
            "iff" => Ok(extra_wide_operator("⟺")),
            // Arrows
            "to" | "rightarrow" => Ok(MathExpr::Operator("→".into())),
            "leftarrow" | "gets" => Ok(MathExpr::Operator("←".into())),
            "leftrightarrow" => Ok(MathExpr::Operator("↔".into())),
            "Rightarrow" => Ok(MathExpr::Operator("⇒".into())),
            "Leftarrow" => Ok(MathExpr::Operator("⇐".into())),
            "Leftrightarrow" => Ok(MathExpr::Operator("⇔".into())),
            "longrightarrow" => Ok(MathExpr::Operator("⟶".into())),
            "longleftarrow" => Ok(MathExpr::Operator("⟵".into())),
            "longleftrightarrow" => Ok(MathExpr::Operator("⟷".into())),
            "Longrightarrow" => Ok(MathExpr::Operator("⟹".into())),
            "Longleftarrow" => Ok(MathExpr::Operator("⟸".into())),
            "Longleftrightarrow" => Ok(MathExpr::Operator("⟺".into())),
            "uparrow" => Ok(MathExpr::Operator("↑".into())),
            "downarrow" => Ok(MathExpr::Operator("↓".into())),
            "updownarrow" => Ok(MathExpr::Operator("↕".into())),
            "Uparrow" => Ok(MathExpr::Operator("⇑".into())),
            "Downarrow" => Ok(MathExpr::Operator("⇓".into())),
            "Updownarrow" => Ok(MathExpr::Operator("⇕".into())),
            "mapsto" => Ok(MathExpr::Operator("↦".into())),
            "longmapsto" => Ok(MathExpr::Operator("⟼".into())),
            "hookrightarrow" => Ok(MathExpr::Operator("↪".into())),
            "hookleftarrow" => Ok(MathExpr::Operator("↩".into())),
            // Large operators
            "sum" => Ok(MathExpr::Operator("∑".into())),
            "prod" => Ok(MathExpr::Operator("∏".into())),
            "coprod" => Ok(MathExpr::Operator("∐".into())),
            "int" => Ok(MathExpr::Operator("∫".into())),
            "oint" => Ok(MathExpr::Operator("∮".into())),
            "iint" => Ok(MathExpr::Operator("∬".into())),
            "iiint" => Ok(MathExpr::Operator("∭".into())),
            "bigcup" => Ok(MathExpr::Operator("⋃".into())),
            "bigcap" => Ok(MathExpr::Operator("⋂".into())),
            "biguplus" => Ok(MathExpr::Operator("⨄".into())),
            "bigsqcup" => Ok(MathExpr::Operator("⨆".into())),
            "bigvee" => Ok(MathExpr::Operator("⋁".into())),
            "bigwedge" => Ok(MathExpr::Operator("⋀".into())),
            "bigoplus" => Ok(MathExpr::Operator("⨁".into())),
            "bigotimes" => Ok(MathExpr::Operator("⨂".into())),
            "bigodot" => Ok(MathExpr::Operator("⨀".into())),
            // Misc symbols
            "nabla" => Ok(MathExpr::Operator("∇".into())),
            "dagger" => Ok(MathExpr::Operator("†".into())),
            "ddagger" => Ok(MathExpr::Operator("‡".into())),
            "mid" => Ok(MathExpr::Operator("|".into())),
            "angle" => Ok(MathExpr::Operator("∠".into())),
            "measuredangle" => Ok(MathExpr::Operator("∡".into())),
            // Dots
            "ldots" | "dots" => Ok(MathExpr::Text("...".into())),
            "cdots" => Ok(MathExpr::Operator("⋯".into())),
            "vdots" => Ok(MathExpr::Operator("⋮".into())),
            "ddots" => Ok(MathExpr::Operator("⋱".into())),
            // Function-like operator names (rendered upright)
            "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "sinh" | "cosh" | "tanh" | "coth"
            | "arcsin" | "arccos" | "arctan" | "log" | "lg" | "ln" | "exp" | "lim" | "max"
            | "min" | "sup" | "inf" | "det" | "arg" | "deg" | "dim" | "hom" | "ker" => {
                Ok(MathExpr::Text(name))
            }
            "gcd" => Ok(MathExpr::Text("gcd".into())),
            "Pr" => Ok(MathExpr::Text("Pr".into())),
            "liminf" => Ok(MathExpr::Text("lim inf".into())),
            "limsup" => Ok(MathExpr::Text("lim sup".into())),
            // Spacing
            "quad" => Ok(MathExpr::Space(1.0)),
            "qquad" => Ok(MathExpr::Space(2.0)),
            "thinspace" => Ok(MathExpr::Space(THIN_MATH_SPACE_EM)),
            "medspace" => Ok(MathExpr::Space(MEDIUM_MATH_SPACE_EM)),
            "thickspace" => Ok(MathExpr::Space(THICK_MATH_SPACE_EM)),
            "negthinspace" => Ok(MathExpr::Space(-THIN_MATH_SPACE_EM)),
            "negmedspace" => Ok(MathExpr::Space(-MEDIUM_MATH_SPACE_EM)),
            "negthickspace" => Ok(MathExpr::Space(-THICK_MATH_SPACE_EM)),
            "enspace" => Ok(MathExpr::Space(0.5)),
            "space" => Ok(MathExpr::Space(MEDIUM_MATH_SPACE_EM)),
            _ => match delimiter_command(&name) {
                Some(symbol) => Ok(MathExpr::Operator(symbol)),
                None => Ok(MathExpr::Identifier(format!("\\{name}"))),
            },
        }
    }

    fn parse_required_group(&mut self) -> Result<MathExpr, MathParseError> {
        self.skip_ws();
        self.expect('{')?;
        self.parse_row(Some('}'))
    }

    fn parse_text_group(&mut self) -> Result<String, MathParseError> {
        self.skip_ws();
        self.expect('{')?;
        let mut depth = 1;
        let mut text = String::new();
        while let Some(ch) = self.bump() {
            match ch {
                '\\' => {
                    let escaped = self
                        .bump()
                        .ok_or_else(|| self.error("unclosed text group"))?;
                    text.push(escaped);
                }
                '{' => {
                    depth += 1;
                    text.push(ch);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(text.split_whitespace().collect::<Vec<_>>().join(" "));
                    }
                    text.push(ch);
                }
                _ => text.push(ch),
            }
        }
        Err(self.error("unclosed text group"))
    }

    fn parse_optional_bracket_group(&mut self) -> Result<Option<MathExpr>, MathParseError> {
        self.skip_ws();
        if self.peek() != Some('[') {
            return Ok(None);
        }
        self.bump();
        self.parse_row(Some(']')).map(Some)
    }

    fn parse_delimiter(&mut self) -> Result<Option<String>, MathParseError> {
        self.skip_ws();
        let delimiter = match self.bump() {
            Some('.') => return Ok(None),
            Some('\\') => {
                let name = self.take_while(|c| c.is_ascii_alphabetic());
                if name.is_empty() {
                    self.bump()
                        .ok_or_else(|| self.error("expected delimiter after escape"))?
                        .to_string()
                } else {
                    delimiter_command(&name).unwrap_or_else(|| format!("\\{name}"))
                }
            }
            Some(ch) => ch.to_string(),
            None => return Err(self.error("expected delimiter")),
        };
        Ok(Some(delimiter))
    }

    fn parse_environment_name(&mut self) -> Result<String, MathParseError> {
        self.skip_ws();
        self.expect('{')?;
        let name = self.take_while(|c| c != '}');
        self.expect('}')?;
        if name.is_empty() {
            return Err(self.error("expected environment name"));
        }
        Ok(name)
    }

    fn parse_array_column_alignments(
        &mut self,
    ) -> Result<Vec<MathColumnAlignment>, MathParseError> {
        self.skip_ws();
        self.expect('{')?;
        let mut alignments = Vec::new();
        loop {
            match self.bump() {
                Some('}') => break,
                Some('l') => alignments.push(MathColumnAlignment::Left),
                Some('c') => alignments.push(MathColumnAlignment::Center),
                Some('r') => alignments.push(MathColumnAlignment::Right),
                Some('|') | Some(' ') | Some('\t') | Some('\n') | Some('\r') => {}
                Some(ch) => {
                    return Err(
                        self.error(&format!("unsupported array alignment specifier {ch:?}"))
                    );
                }
                None => return Err(self.error("unclosed array alignment spec")),
            }
        }
        Ok(alignments)
    }

    fn consume_environment_end(&mut self, expected: &str) -> Result<(), MathParseError> {
        self.consume_command("end")?;
        let found = self.parse_environment_name()?;
        if found == expected {
            Ok(())
        } else {
            Err(self.error(&format!("expected \\end{{{expected}}}")))
        }
    }

    fn starts_with_row_separator(&self) -> bool {
        self.input[self.pos..].starts_with(r"\\")
    }

    fn consume_row_separator(&mut self) -> Result<(), MathParseError> {
        if !self.starts_with_row_separator() {
            return Err(self.error(r"expected \\"));
        }
        self.expect('\\')?;
        self.expect('\\')
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
            self.bump();
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), MathParseError> {
        match self.bump() {
            Some(ch) if ch == expected => Ok(()),
            _ => Err(self.error(&format!("expected '{expected}'"))),
        }
    }

    fn take_while(&mut self, mut f: impl FnMut(char) -> bool) -> String {
        let start = self.pos;
        while matches!(self.peek(), Some(ch) if f(ch)) {
            self.bump();
        }
        self.input[start..self.pos].to_string()
    }

    fn starts_with_command(&self, command: &str) -> bool {
        let rest = &self.input[self.pos..];
        let Some(after_slash) = rest.strip_prefix('\\') else {
            return false;
        };
        let Some(after_command) = after_slash.strip_prefix(command) else {
            return false;
        };
        !matches!(after_command.chars().next(), Some(ch) if ch.is_ascii_alphabetic())
    }

    fn consume_command(&mut self, command: &str) -> Result<(), MathParseError> {
        if !self.starts_with_command(command) {
            return Err(self.error(&format!("expected \\{command}")));
        }
        self.expect('\\')?;
        let found = self.take_while(|c| c.is_ascii_alphabetic());
        if found == command {
            Ok(())
        } else {
            Err(self.error(&format!("expected \\{command}")))
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn error(&self, message: &str) -> MathParseError {
        MathParseError {
            message: message.to_string(),
            byte: self.pos,
        }
    }
}

fn extra_wide_operator(symbol: &str) -> MathExpr {
    let spacing = MEDIUM_MATH_SPACE_EM + THICK_MATH_SPACE_EM;
    MathExpr::OperatorWithMetadata {
        text: symbol.into(),
        lspace: Some(spacing),
        rspace: Some(spacing),
        large_operator: None,
        movable_limits: None,
    }
}

fn delimiter_command(command: &str) -> Option<String> {
    let delimiter = match command {
        "lbrace" => "{",
        "rbrace" => "}",
        "lparen" => "(",
        "rparen" => ")",
        "lbrack" => "[",
        "rbrack" => "]",
        "langle" => "⟨",
        "rangle" => "⟩",
        "vert" => "|",
        "Vert" => "‖",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        _ => return None,
    };
    Some(delimiter.to_string())
}

fn map_mathbb_expr(expr: MathExpr) -> MathExpr {
    match expr {
        MathExpr::Identifier(text) => MathExpr::Identifier(map_mathbb_text(&text)),
        MathExpr::Text(text) => MathExpr::Text(map_mathbb_text(&text)),
        MathExpr::Row(children) => MathExpr::row(children.into_iter().map(map_mathbb_expr)),
        other => other,
    }
}

fn map_mathbb_text(text: &str) -> String {
    text.chars().map(map_mathbb_char).collect()
}

fn map_mathbb_char(ch: char) -> char {
    match ch {
        'A' => '𝔸',
        'B' => '𝔹',
        'C' => 'ℂ',
        'D' => '𝔻',
        'E' => '𝔼',
        'F' => '𝔽',
        'G' => '𝔾',
        'H' => 'ℍ',
        'I' => '𝕀',
        'J' => '𝕁',
        'K' => '𝕂',
        'L' => '𝕃',
        'M' => '𝕄',
        'N' => 'ℕ',
        'O' => '𝕆',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'S' => '𝕊',
        'T' => '𝕋',
        'U' => '𝕌',
        'V' => '𝕍',
        'W' => '𝕎',
        'X' => '𝕏',
        'Y' => '𝕐',
        'Z' => 'ℤ',
        _ => ch,
    }
}

struct TexTableOptions {
    column_alignments: Vec<MathColumnAlignment>,
    column_gap: Option<f32>,
    row_gap: Option<f32>,
}

fn default_tex_table_options(env: &str) -> TexTableOptions {
    match env {
        "cases" => TexTableOptions {
            column_alignments: vec![MathColumnAlignment::Left, MathColumnAlignment::Left],
            column_gap: Some(CASES_COL_GAP_EM),
            row_gap: None,
        },
        "aligned" | "align" => TexTableOptions {
            column_alignments: vec![MathColumnAlignment::Right, MathColumnAlignment::Left],
            column_gap: Some(MEDIUM_MATH_SPACE_EM),
            row_gap: None,
        },
        _ => TexTableOptions {
            column_alignments: Vec::new(),
            column_gap: None,
            row_gap: None,
        },
    }
}

pub(crate) fn math_glyph_layout(
    text: &str,
    size: f32,
    weight: FontWeight,
) -> text_metrics::TextLayout {
    text_metrics::layout_text_with_line_height_and_family(
        text,
        size,
        text_metrics::line_height(size),
        FontFamily::Inter,
        weight,
        false,
        TextWrap::NoWrap,
        None,
    )
}

pub(crate) fn resolved_math_color(color: Option<Color>) -> Color {
    color.unwrap_or(crate::tokens::FOREGROUND)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_radical_shape(layout: &MathLayout) -> bool {
        layout
            .atoms
            .iter()
            .any(|atom| matches!(atom, MathAtom::Radical { .. } | MathAtom::GlyphId { .. }))
    }

    fn expect_source(expr: &MathExpr, expected: Range<usize>) -> &MathExpr {
        let MathExpr::Source { source, body } = expr else {
            panic!("expected source wrapper, got {expr:?}");
        };
        assert_eq!(*source, expected);
        body
    }

    fn assert_no_unknown_tex_commands(expr: &MathExpr) {
        match expr {
            MathExpr::Identifier(text) => {
                assert!(
                    !text.starts_with('\\'),
                    "unexpected raw TeX command identifier {text:?} in {expr:?}"
                );
            }
            MathExpr::Row(children) => {
                for child in children {
                    assert_no_unknown_tex_commands(child);
                }
            }
            MathExpr::Fraction {
                numerator,
                denominator,
            } => {
                assert_no_unknown_tex_commands(numerator);
                assert_no_unknown_tex_commands(denominator);
            }
            MathExpr::Sqrt(child) => assert_no_unknown_tex_commands(child),
            MathExpr::Root { base, index } => {
                assert_no_unknown_tex_commands(base);
                assert_no_unknown_tex_commands(index);
            }
            MathExpr::Scripts { base, sub, sup } => {
                assert_no_unknown_tex_commands(base);
                if let Some(sub) = sub {
                    assert_no_unknown_tex_commands(sub);
                }
                if let Some(sup) = sup {
                    assert_no_unknown_tex_commands(sup);
                }
            }
            MathExpr::UnderOver { base, under, over } => {
                assert_no_unknown_tex_commands(base);
                if let Some(under) = under {
                    assert_no_unknown_tex_commands(under);
                }
                if let Some(over) = over {
                    assert_no_unknown_tex_commands(over);
                }
            }
            MathExpr::Accent { base, accent, .. } => {
                assert_no_unknown_tex_commands(base);
                assert_no_unknown_tex_commands(accent);
            }
            MathExpr::Fenced { body, .. } => assert_no_unknown_tex_commands(body),
            MathExpr::Table { rows, .. } => {
                for row in rows {
                    for cell in row {
                        assert_no_unknown_tex_commands(cell);
                    }
                }
            }
            MathExpr::Source { body, .. } => assert_no_unknown_tex_commands(body),
            MathExpr::Operator(_)
            | MathExpr::OperatorWithMetadata { .. }
            | MathExpr::Text(_)
            | MathExpr::Number(_)
            | MathExpr::Space(_)
            | MathExpr::Error(_) => {}
        }
    }

    #[test]
    fn tex_source_ranges_are_opt_in_and_do_not_change_layout() {
        let input = r"\frac{x_1}{2}";
        let plain = parse_tex(input).expect("plain tex");
        let sourced = parse_tex_with_source_ranges(input).expect("source-backed tex");

        assert!(!matches!(plain, MathExpr::Source { .. }));
        assert_eq!(
            layout_math(&plain, 16.0, MathDisplay::Block),
            layout_math(&sourced, 16.0, MathDisplay::Block)
        );
        assert!(matches!(
            expect_source(&sourced, 0..input.len()).without_source(),
            MathExpr::Fraction { .. }
        ));
    }

    #[test]
    fn tex_source_ranges_track_script_components() {
        let expr = parse_tex_with_source_ranges("x_1^2").expect("source-backed tex");
        let root = expect_source(&expr, 0..5);
        let body = expect_source(root, 0..5);
        let MathExpr::Scripts { base, sub, sup } = body else {
            panic!("expected scripts, got {body:?}");
        };

        assert_eq!(
            expect_source(base, 0..1).without_source(),
            &MathExpr::Identifier("x".into())
        );
        assert_eq!(
            expect_source(sub.as_deref().expect("subscript"), 2..3).without_source(),
            &MathExpr::Number("1".into())
        );
        assert_eq!(
            expect_source(sup.as_deref().expect("superscript"), 4..5).without_source(),
            &MathExpr::Number("2".into())
        );
    }

    #[cfg(feature = "symbols")]
    #[test]
    fn loads_bundled_open_type_math_constants() {
        let constants = open_type_math_constants().expect("bundled math font has a MATH table");
        assert!(
            constants
                .script_scale(16.0)
                .is_some_and(|size| size > 6.0 && size < 16.0),
            "script scale should come from Noto Sans Math"
        );
        assert!(
            constants
                .fraction_rule_thickness(16.0)
                .is_some_and(|thickness| thickness > 0.75 && thickness < 2.0),
            "fraction rule thickness should come from Noto Sans Math"
        );
        assert!(
            constants
                .axis_height(16.0)
                .is_some_and(|axis| axis > 1.0 && axis < 8.0),
            "axis height should come from Noto Sans Math"
        );
        assert!(
            constants
                .superscript_shift_up(16.0)
                .is_some_and(|shift| shift > 1.0 && shift < 16.0),
            "superscript shift should come from Noto Sans Math"
        );
        assert!(
            constants
                .subscript_shift_down(16.0)
                .is_some_and(|shift| shift > 1.0 && shift < 16.0),
            "subscript shift should come from Noto Sans Math"
        );
        assert!(
            constants
                .space_after_script(16.0)
                .is_some_and(|space| space > 0.1 && space < 4.0),
            "script spacing should come from Noto Sans Math"
        );
        assert!(
            constants
                .upper_limit_gap_min(16.0)
                .is_some_and(|gap| gap > 0.5 && gap < 8.0),
            "upper limit gap should come from Noto Sans Math"
        );
        assert!(
            constants
                .lower_limit_baseline_drop_min(16.0)
                .is_some_and(|drop| drop > 1.0 && drop < 20.0),
            "lower limit baseline drop should come from Noto Sans Math"
        );
        assert!(
            constants
                .fraction_numerator_gap(16.0, true)
                .is_some_and(|gap| gap > 0.5 && gap < 8.0),
            "display numerator gap should come from Noto Sans Math"
        );
        assert!(
            constants
                .fraction_denominator_gap(16.0, true)
                .is_some_and(|gap| gap > 0.5 && gap < 8.0),
            "display denominator gap should come from Noto Sans Math"
        );
        assert!(
            constants
                .fraction_numerator_shift(16.0, true)
                .is_some_and(|shift| shift > 1.0 && shift < 24.0),
            "display numerator shift should come from Noto Sans Math"
        );
        assert!(
            constants
                .fraction_denominator_shift(16.0, true)
                .is_some_and(|shift| shift > 1.0 && shift < 24.0),
            "display denominator shift should come from Noto Sans Math"
        );
        assert!(
            constants
                .radical_rule_thickness(16.0)
                .is_some_and(|thickness| thickness > 0.75 && thickness < 2.0),
            "radical rule thickness should come from Noto Sans Math"
        );
        assert!(
            constants
                .radical_vertical_gap(16.0, true)
                .is_some_and(|gap| gap > 0.5 && gap < 8.0),
            "display radical gap should come from Noto Sans Math"
        );
        assert!(
            constants
                .radical_kern_before_degree(16.0)
                .is_some_and(|kern| kern > 0.0 && kern < 8.0),
            "radical degree before-kern should come from Noto Sans Math"
        );
        assert!(
            constants
                .radical_kern_after_degree(16.0)
                .is_some_and(|kern| kern < 0.0 && kern > -8.0),
            "radical degree after-kern should come from Noto Sans Math"
        );
        assert!(
            constants
                .radical_degree_bottom_raise_fraction()
                .is_some_and(|raise| raise > 0.0 && raise < 1.0),
            "radical degree raise should come from Noto Sans Math"
        );
        assert!(
            constants
                .min_connector_overlap(16.0)
                .is_some_and(|overlap| overlap > 0.0),
            "delimiter connector overlap should come from Noto Sans Math"
        );
        assert!(
            constants
                .delimited_sub_formula_min_height(16.0)
                .is_some_and(|height| height > 8.0 && height < 40.0),
            "delimiter stretch threshold should come from Noto Sans Math"
        );
        assert!(
            constants.delimiter_variant_count('(') > 0,
            "left paren should expose vertical delimiter variants"
        );
        assert!(
            constants.delimiter_variant_count(RADICAL_GLYPH) > 0,
            "radical should expose vertical math glyph variants"
        );
        assert!(
            constants.delimiter_variant_count('∑') > 0,
            "summation should expose vertical math glyph variants"
        );
        assert!(
            constants.delimiter_variant_count('∫') > 0,
            "integral should expose vertical math glyph variants"
        );
        assert!(
            constants
                .delimiter_first_variant_glyph_id('(')
                .is_some_and(|glyph_id| glyph_id > 0),
            "left paren variants should preserve glyph IDs"
        );
        assert!(
            constants.delimiter_assembly_part_count('{') > 0,
            "left brace should expose a vertical delimiter assembly"
        );
        assert!(
            constants.delimiter_extender_part_count('{') > 0,
            "left brace assembly should expose extender parts"
        );
        assert!(
            constants.delimiter_has_assembly_connectors('{'),
            "left brace assembly should preserve connector metadata"
        );
        assert!(
            constants
                .delimiter_max_advance('(', 16.0)
                .is_some_and(|advance| advance > 16.0),
            "delimiter variant advances should scale into px"
        );
    }

    #[test]
    fn parses_fraction_with_scripts() {
        let expr = parse_tex(r"\frac{a^2+b^2}{\sqrt{x_1+x_2}}").expect("valid tex");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(layout.width > 20.0, "width = {}", layout.width);
        assert!(layout.ascent > 10.0, "ascent = {}", layout.ascent);
        assert!(layout.descent > 10.0, "descent = {}", layout.descent);
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Rule { .. })),
            "fraction should emit rule atoms"
        );
        assert!(
            has_radical_shape(&layout),
            "sqrt should emit a radical shape atom"
        );
    }

    #[test]
    fn display_fraction_honors_baseline_shifts() {
        let layout = layout_math(
            &parse_tex(r"\frac{1}{2}").unwrap(),
            16.0,
            MathDisplay::Block,
        );
        let metrics = LayoutCtx {
            size: 16.0,
            display: MathDisplay::Block,
        }
        .metrics();
        let numerator_y = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text, y_baseline, ..
                } if text == "1" => Some(*y_baseline),
                _ => None,
            })
            .expect("numerator baseline");
        let denominator_y = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text, y_baseline, ..
                } if text == "2" => Some(*y_baseline),
                _ => None,
            })
            .expect("denominator baseline");

        assert!(
            -numerator_y >= metrics.fraction_numerator_shift() - 0.1,
            "numerator shift = {}, min = {}",
            -numerator_y,
            metrics.fraction_numerator_shift()
        );
        assert!(
            denominator_y >= metrics.fraction_denominator_shift() - 0.1,
            "denominator shift = {denominator_y}, min = {}",
            metrics.fraction_denominator_shift()
        );
    }

    #[test]
    fn scripts_with_sub_and_sup_keep_minimum_gap() {
        let layout = layout_math(&parse_tex(r"x_1^2").unwrap(), 16.0, MathDisplay::Inline);
        let sub_top = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text,
                    y_baseline,
                    size,
                    ..
                } if text == "1" => Some(
                    y_baseline
                        - LayoutCtx {
                            size: *size,
                            display: MathDisplay::Inline,
                        }
                        .metrics()
                        .glyph_ascent(),
                ),
                _ => None,
            })
            .expect("subscript top");
        let sup_bottom = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text,
                    y_baseline,
                    size,
                    ..
                } if text == "2" => Some(
                    y_baseline
                        + LayoutCtx {
                            size: *size,
                            display: MathDisplay::Inline,
                        }
                        .metrics()
                        .glyph_descent(),
                ),
                _ => None,
            })
            .expect("superscript bottom");
        let min_gap = LayoutCtx {
            size: 16.0,
            display: MathDisplay::Inline,
        }
        .metrics()
        .sub_superscript_gap();

        assert!(
            sub_top - sup_bottom >= min_gap - 0.1,
            "script gap = {}, min = {min_gap}",
            sub_top - sup_bottom
        );
    }

    #[test]
    fn parses_indexed_tex_root() {
        let expr = parse_tex(r"\sqrt[3]{x+1}").expect("valid tex");
        match expr {
            MathExpr::Root { base, index } => {
                assert_eq!(*index, MathExpr::Number("3".into()));
                assert!(matches!(*base, MathExpr::Row(_)));
            }
            other => panic!("expected indexed root, got {other:?}"),
        }
        let layout = layout_math(
            &parse_tex(r"\sqrt[3]{x+1}").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            has_radical_shape(&layout),
            "indexed root should emit a radical shape atom"
        );
    }

    #[test]
    fn indexed_root_uses_open_type_degree_metrics() {
        let ctx = LayoutCtx {
            size: 16.0,
            display: MathDisplay::Inline,
        };
        let metrics = ctx.metrics();
        let base = parse_tex(r"x+1").expect("valid root base");
        let index_expr = MathExpr::Number("3".into());
        let root = layout_sqrt(&base, ctx);
        let index = layout_expr(&index_expr, ctx.script());
        let layout = layout_root(&base, &index_expr, ctx);
        let constants = metrics.font_constants().expect("bundled math constants");
        let expected_root_x = (constants
            .radical_kern_before_degree(ctx.size)
            .unwrap_or(0.0)
            + index.width
            + constants.radical_kern_after_degree(ctx.size).unwrap_or(0.0))
        .max(index.width * 0.35);
        let expected_index_dy = -root.ascent
            * constants
                .radical_degree_bottom_raise_fraction()
                .expect("root degree raise")
            - index.descent;
        let index_atom = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text,
                    x,
                    y_baseline,
                    ..
                } if text == "3" => Some((*x, *y_baseline)),
                _ => None,
            })
            .expect("root index glyph");
        let root_x = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::GlyphId { rect, .. } => Some(rect.x),
                MathAtom::Radical { points, .. } => Some(points[0][0]),
                _ => None,
            })
            .expect("root radical atom");

        assert!(
            (index_atom.0 - 0.0).abs() < 0.1,
            "index x = {}",
            index_atom.0
        );
        assert!(
            (index_atom.1 - expected_index_dy).abs() < 0.1,
            "index baseline = {}, expected {expected_index_dy}",
            index_atom.1
        );
        assert!(
            (root_x - expected_root_x).abs() < 0.1,
            "root x = {root_x}, expected {expected_root_x}"
        );
    }

    #[test]
    fn parses_tex_accents() {
        let expr = parse_tex(r"\hat{x} + \overline{ab} + \vec{v}").expect("valid tex accents");
        let MathExpr::Row(children) = expr else {
            panic!("expected row expression");
        };
        assert!(
            children
                .iter()
                .filter(|child| matches!(child, MathExpr::Accent { .. }))
                .count()
                >= 3,
            "expected accent expressions in {children:?}"
        );

        let overline = layout_math(
            &parse_tex(r"\overline{ab}").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            overline
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Rule { rect } if rect.y < -10.0)),
            "overline should emit a rule above the base"
        );
    }

    #[test]
    fn parses_tex_text_groups() {
        let expr = parse_tex(r"x \text{ if } y \operatorname{max}").expect("valid tex text");
        let MathExpr::Row(children) = expr else {
            panic!("expected row expression");
        };
        assert!(
            children
                .iter()
                .any(|child| matches!(child, MathExpr::Text(text) if text == "if")),
            "expected text group in {children:?}"
        );
        assert!(
            children
                .iter()
                .any(|child| matches!(child, MathExpr::Text(text) if text == "max")),
            "expected operatorname text in {children:?}"
        );
    }

    #[test]
    fn parses_common_tex_symbol_commands() {
        let expr =
            parse_tex(r"\alpha+\beta\to\gamma+\emptyset+\varnothing").expect("valid tex symbols");
        let MathExpr::Row(children) = expr else {
            panic!("expected row expression");
        };
        assert!(
            children
                .iter()
                .any(|child| matches!(child, MathExpr::Identifier(text) if text == "∅")),
            "expected empty-set symbol in {children:?}"
        );
        assert!(
            children.iter().all(
                |child| !matches!(child, MathExpr::Identifier(text) if text.starts_with('\\'))
            ),
            "expected supported symbol commands in {children:?}"
        );
    }

    #[test]
    fn parses_aligned_tex_environment() {
        let expr = parse_tex(
            r"\begin{aligned}
(a + b)^2 &= a^2 + 2ab + b^2 \\
(a - b)^2 &= a^2 - 2ab + b^2 \\
(a+b)(a-b) &= a^2 - b^2
\end{aligned}",
        )
        .expect("valid aligned environment");

        let MathExpr::Table {
            rows,
            column_alignments,
            ..
        } = expr
        else {
            panic!("expected aligned environment to parse as table");
        };
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| row.len() == 2), "rows = {rows:?}");
        assert_eq!(
            column_alignments,
            vec![MathColumnAlignment::Right, MathColumnAlignment::Left]
        );
    }

    #[test]
    fn parses_markdown_math_stress_tex_commands() {
        let formulas = [
            r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
            r"\int_{-\infty}^{\infty} e^{-x^2}\, dx = \sqrt{\pi}",
            r"\hat{f}(\xi) = \int_{-\infty}^{\infty} f(x)\, e^{-2\pi i x \xi}\, dx",
            r"\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}",
            r"\nabla \times \mathbf{B} = \mu_0 \mathbf{J} + \mu_0 \varepsilon_0 \frac{\partial \mathbf{E}}{\partial t}",
            r"\begin{aligned}
S &= \sum_{k=0}^{n} r^k = 1 + r + r^2 + \cdots + r^n \\
rS &= r + r^2 + \cdots + r^{n+1} \\
S - rS &= 1 - r^{n+1} \\
S &= \frac{1 - r^{n+1}}{1 - r}, \quad r \neq 1
\end{aligned}",
            r"R(\theta) = \begin{pmatrix} \cos\theta & -\sin\theta \\ \sin\theta & \cos\theta \end{pmatrix}",
            r"\det(A) = \begin{vmatrix} a & b & c \\ d & e & f \\ g & h & i \end{vmatrix}",
            r"f'(x) = \lim_{h \to 0} \frac{f(x+h) - f(x)}{h}",
            r"P(A \mid B) = \frac{P(B \mid A)\, P(A)}{P(B)}",
            r"f(x \mid \mu, \sigma^2) = \frac{1}{\sqrt{2\pi\sigma^2}}",
            r"\mathbb{E}[X] = \int_{-\infty}^{\infty} x\, f(x)\, dx, \qquad \operatorname{Var}(X) = \mathbb{E}[X^2] - (\mathbb{E}[X])^2",
            r"( x + y )^n = \sum_{k=0}^{n} \binom{n}{k} x^{n-k} y^k",
            r"\varphi(n) = n \prod_{p \mid n} \left(1 - \frac{1}{p}\right)",
            r"A = A^\dagger",
            r"E_n = \frac{n^2 \pi^2 \hbar^2}{2mL^2}",
            r"|\langle \mathbf{u}, \mathbf{v} \rangle|^2 \leq \langle \mathbf{u}, \mathbf{u} \rangle \cdot \langle \mathbf{v}, \mathbf{v} \rangle",
            r"\alpha,\ \beta,\ \gamma,\ \delta,\ \varepsilon,\ \zeta,\ \eta,\ \theta,\ \iota,\ \kappa,\ \lambda,\ \mu,\ \nu,\ \xi,\ \pi,\ \rho,\ \sigma,\ \tau,\ \upsilon,\ \phi,\ \chi,\ \psi,\ \omega",
            r"\Gamma(n) = (n-1)! \qquad \Gamma\!\left(\tfrac{1}{2}\right) = \sqrt{\pi}",
            r"\zeta(s) = \sum_{n=1}^{\infty} \frac{1}{n^s}, \quad \Re(s) > 1",
        ];

        for formula in formulas {
            let expr = parse_tex(formula)
                .unwrap_or_else(|err| panic!("failed to parse {formula:?}: {}", err.message));
            assert_no_unknown_tex_commands(&expr);
            let layout = layout_math(&expr, 16.0, MathDisplay::Block);
            assert!(
                layout.width.is_finite() && layout.height().is_finite(),
                "layout should be finite for {formula:?}: {layout:?}"
            );
        }
    }

    #[test]
    fn operator_metadata_covers_spacing_and_large_ops() {
        let plus = operator_info("+");
        assert_eq!(plus.class, MathOperatorClass::Binary);
        assert!(plus.lspace_em > 0.0);
        assert!(plus.rspace_em > 0.0);

        let comma = operator_info(",");
        assert_eq!(comma.class, MathOperatorClass::Punctuation);
        assert_eq!(comma.lspace_em, 0.0);
        assert!(comma.rspace_em > 0.0);

        let sum = operator_info("∑");
        assert_eq!(sum.class, MathOperatorClass::Large);
        assert!(sum.large_operator);
        assert!(sum.movable_limits);

        let integral = operator_info("∫");
        assert_eq!(integral.class, MathOperatorClass::Large);
        assert!(integral.large_operator);
        assert!(!integral.movable_limits);

        // Expanded class coverage
        let wedge = operator_info("∧");
        assert_eq!(wedge.class, MathOperatorClass::Binary);
        let oplus = operator_info("⊕");
        assert_eq!(oplus.class, MathOperatorClass::Binary);

        let element_of = operator_info("∈");
        assert_eq!(element_of.class, MathOperatorClass::Relation);
        let double_right_arrow = operator_info("⇒");
        assert_eq!(double_right_arrow.class, MathOperatorClass::Relation);

        let coproduct = operator_info("∐");
        assert_eq!(coproduct.class, MathOperatorClass::Large);
        assert!(coproduct.movable_limits);

        let contour_integral = operator_info("∮");
        assert_eq!(contour_integral.class, MathOperatorClass::Large);
        assert!(contour_integral.large_operator);
        assert!(!contour_integral.movable_limits);
    }

    #[test]
    fn parses_logic_and_set_theory_commands() {
        let expr = parse_tex(r"\forall x \in S, \exists y \notin T \implies x \neq y")
            .expect("valid logic tex");
        assert_no_unknown_tex_commands(&expr);

        let MathExpr::Row(children) = &expr else {
            panic!("expected row");
        };

        assert!(
            children
                .iter()
                .any(|c| matches!(c.without_source(), MathExpr::Operator(s) if s == "∀")),
            "expected ∀ in {children:?}"
        );
        assert!(
            children
                .iter()
                .any(|c| matches!(c.without_source(), MathExpr::Operator(s) if s == "∃")),
            "expected ∃ in {children:?}"
        );
        assert!(
            children
                .iter()
                .any(|c| matches!(c.without_source(), MathExpr::Operator(s) if s == "∈")),
            "expected ∈ in {children:?}"
        );
        assert!(
            children
                .iter()
                .any(|c| matches!(c.without_source(), MathExpr::Operator(s) if s == "∉")),
            "expected ∉ in {children:?}"
        );

        let implies = children
            .iter()
            .find_map(|child| match child.without_source() {
                MathExpr::OperatorWithMetadata {
                    text,
                    lspace,
                    rspace,
                    ..
                } if text == "⟹" => Some((*lspace, *rspace)),
                _ => None,
            })
            .expect("expected \\implies operator with metadata");
        let (lspace, rspace) = implies;
        // Should be wider than the default relation spacing.
        assert!(
            lspace.is_some_and(|v| v > MEDIUM_MATH_SPACE_EM),
            "lspace = {lspace:?}"
        );
        assert!(
            rspace.is_some_and(|v| v > MEDIUM_MATH_SPACE_EM),
            "rspace = {rspace:?}"
        );
    }

    #[test]
    fn parses_function_like_operators_with_limits() {
        let expr = parse_tex(r"\gcd_{p \mid n}(p) + \liminf_{n \to \infty} a_n")
            .expect("valid function tex");
        assert_no_unknown_tex_commands(&expr);

        let layout = layout_math(&expr, 22.0, MathDisplay::Block);
        assert!(layout.width.is_finite() && layout.height().is_finite());

        // \gcd should be a Text base; \liminf renders with a thin space.
        assert!(is_display_limits_base(&MathExpr::Text("gcd".into())));
        assert!(is_display_limits_base(&MathExpr::Text("Pr".into())));
        assert!(is_display_limits_base(&MathExpr::Text("det".into())));
        assert!(is_display_limits_base(&MathExpr::Text("lim inf".into())));
        assert!(is_display_limits_base(&MathExpr::Text("lim sup".into())));
    }

    #[test]
    fn parses_bare_delimiter_commands_as_operators() {
        let expr = parse_tex(r"\lfloor x \rfloor + \lceil y \rceil + \langle u, v \rangle")
            .expect("valid bare delimiter tex");
        assert_no_unknown_tex_commands(&expr);
        let MathExpr::Row(children) = &expr else {
            panic!("expected row");
        };
        for symbol in ["⌊", "⌋", "⌈", "⌉", "⟨", "⟩"] {
            assert!(
                children
                    .iter()
                    .any(|c| matches!(c.without_source(), MathExpr::Operator(s) if s == symbol)),
                "expected {symbol} in {children:?}"
            );
        }
    }

    #[test]
    fn parses_arrows_and_big_operators() {
        let formulas = [
            r"f : A \hookrightarrow B",
            r"x \mapsto x^2",
            r"a \Rightarrow b \Leftrightarrow c",
            r"\bigoplus_{i=1}^{n} V_i \cong \bigotimes_{j=1}^{m} W_j",
            r"\oint_{\partial \Omega} \omega = \iint_{\Omega} d\omega",
            r"\coprod_{i \in I} X_i",
            r"\bigvee_{k} a_k \wedge \bigwedge_{k} b_k",
            r"A \subseteq B \subset C \supseteq D \supset E",
            r"x \equiv y",
            r"\therefore \forall \epsilon > 0, \exists \delta > 0",
            r"\neg p \vee q \iff p \implies q",
            r"\gcd(a, b) \cdot \mathrm{lcm}(a, b) = a \cdot b",
            r"\Pr(A \cup B) \leq \Pr(A) + \Pr(B)",
            r"\liminf_{n \to \infty} a_n \leq \limsup_{n \to \infty} a_n",
            r"\sin^2\theta + \cos^2\theta = 1, \quad \tan\theta = \frac{\sin\theta}{\cos\theta}",
            r"\arcsin x + \arccos x = \frac{\pi}{2}",
            r"\sinh x = \frac{e^x - e^{-x}}{2}, \quad \cosh x = \frac{e^x + e^{-x}}{2}",
            r"\Pi_{i} a_i \prec \Sigma_{i} a_i \succ \Phi_{i} a_i",
            r"a \parallel b, \quad u \perp v",
            r"\vec{v} \cdot \vec{w} = \|\vec{v}\| \|\vec{w}\| \cos\theta",
            r"x \ll y \ll z, \quad a \gg b \gg c",
            r"\aleph_0 < 2^{\aleph_0}",
            r"\Im(z) + i\,\Re(z), \quad \ell^2(\mathbb{N})",
            r"x \star y \ne y \star x, \quad a \oplus b \otimes c",
            r"p \models \phi \vdash \psi \dashv \rho",
        ];

        for formula in formulas {
            let expr = parse_tex(formula)
                .unwrap_or_else(|err| panic!("failed to parse {formula:?}: {}", err.message));
            assert_no_unknown_tex_commands(&expr);
            let layout = layout_math(&expr, 16.0, MathDisplay::Block);
            assert!(
                layout.width.is_finite() && layout.height().is_finite(),
                "layout finite for {formula:?}"
            );
        }
    }

    #[test]
    fn display_sum_scripts_layout_as_limits() {
        let expr = parse_tex(r"\sum_{i=1}^{n} x_i").expect("valid tex");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        let metrics = LayoutCtx {
            size: 16.0,
            display: MathDisplay::Block,
        }
        .metrics();
        let sum_center_y = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text, y_baseline, ..
                } if text == "∑" => Some(*y_baseline),
                MathAtom::GlyphId { rect, .. } => Some(rect.y + rect.h * 0.5),
                _ => None,
            })
            .expect("sum center");
        let upper_y = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text, y_baseline, ..
                } if text == "n" => Some(*y_baseline),
                _ => None,
            })
            .expect("upper limit baseline");
        let lower_y = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph {
                    text, y_baseline, ..
                } if text == "i" => Some(*y_baseline),
                _ => None,
            })
            .expect("lower limit baseline");
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, y_baseline, .. } if text == "n" && *y_baseline < 0.0)),
            "sum upper limit should sit above the operator"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, y_baseline, .. } if text == "i" && *y_baseline > 0.0)),
            "sum lower limit should sit below the operator"
        );
        assert!(
            sum_center_y - upper_y >= metrics.upper_limit_baseline_rise() - 0.1,
            "upper limit rise = {}, min = {}",
            sum_center_y - upper_y,
            metrics.upper_limit_baseline_rise()
        );
        assert!(
            lower_y - sum_center_y >= metrics.lower_limit_baseline_drop() - 0.1,
            "lower limit drop = {}, min = {}",
            lower_y - sum_center_y,
            metrics.lower_limit_baseline_drop()
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::GlyphId { .. })),
            "display sum should use an OpenType operator variant"
        );
        assert!(
            (sum_center_y + metrics.math_axis_shift()).abs() < 0.75,
            "display sum should center on the parent math axis"
        );
    }

    #[test]
    fn display_integral_uses_open_type_variant() {
        let display = layout_math(&parse_tex(r"\int").unwrap(), 16.0, MathDisplay::Block);
        let inline = layout_math(&parse_tex(r"\int").unwrap(), 16.0, MathDisplay::Inline);
        assert!(
            display
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::GlyphId { .. })),
            "display integral should use an OpenType operator variant"
        );
        assert!(
            display.height() > inline.height() * 1.4,
            "display integral height = {}, inline height = {}",
            display.height(),
            inline.height()
        );
    }

    #[test]
    fn mathml_largeop_false_keeps_integral_unexpanded() {
        let expr = parse_mathml(r#"<math><mo largeop="false">∫</mo></math>"#)
            .expect("valid MathML integral");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(
            !layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::GlyphId { .. })),
            "largeop=false should keep display integral on the ordinary glyph path"
        );
    }

    #[test]
    fn display_integral_scripts_stay_on_side_of_large_operator() {
        let layout = layout_math(
            &parse_tex(r"\int_0^1 f(x)dx").unwrap(),
            16.0,
            MathDisplay::Block,
        );
        let integral_rect = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::GlyphId { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("large integral glyph");
        let lower = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph { text, x, .. } if text == "0" => Some(*x),
                _ => None,
            })
            .expect("lower integral script");
        let upper = layout
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph { text, x, .. } if text == "1" => Some(*x),
                _ => None,
            })
            .expect("upper integral script");

        assert!(
            lower >= integral_rect.right() - 0.5 && upper >= integral_rect.right() - 0.5,
            "integral scripts should stay to the side, rect = {integral_rect:?}, lower x = {lower}, upper x = {upper}"
        );
    }

    #[test]
    fn parses_tex_left_right_fences() {
        let expr = parse_tex(r"\left(\frac{a}{b}\right)").expect("valid fenced tex");
        match expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open.as_deref(), Some("("));
                assert_eq!(close.as_deref(), Some(")"));
                assert!(matches!(*body, MathExpr::Fraction { .. }));
            }
            other => panic!("expected fenced expression, got {other:?}"),
        }
        let layout = layout_math(
            &parse_tex(r"\left(\begin{matrix}a\\b\\c\end{matrix}\right)").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::GlyphId { rect, .. } if rect.h > 16.0)),
            "fence should emit a stretched OpenType delimiter variant glyph"
        );
    }

    #[test]
    fn simple_tex_left_right_fences_remain_glyphs() {
        let layout = layout_math(
            &parse_tex(r"\left(x\right)").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            !layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Delimiter { .. })),
            "simple fences should stay as glyphs below the font stretch threshold"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, .. } if text == "(")),
            "left fence should emit a glyph atom"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, .. } if text == ")")),
            "right fence should emit a glyph atom"
        );
    }

    #[test]
    fn stretched_tex_fences_use_open_type_variant_glyphs() {
        let layout = layout_math(
            &parse_tex(r"\left(\begin{matrix}a&b\\c&d\end{matrix}\right)").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::GlyphId { .. })),
            "moderately stretched fences should use exact OpenType delimiter variant glyphs"
        );
    }

    #[test]
    fn very_tall_tex_fences_use_open_type_assembly_parts() {
        let expr =
            parse_tex(r"\left\{\begin{matrix}a\\b\\c\\d\\e\\f\\g\\h\end{matrix}\right.").unwrap();
        let layout = layout_math(&expr, 16.0, MathDisplay::Inline);
        let glyph_id_count = layout
            .atoms
            .iter()
            .filter(|atom| matches!(atom, MathAtom::GlyphId { .. }))
            .count();
        assert!(
            glyph_id_count > 2,
            "very tall fences should use repeated OpenType assembly glyph parts"
        );
        assert!(
            !layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Delimiter { .. })),
            "font assembly should avoid the hand-drawn delimiter fallback"
        );
        let MathExpr::Fenced { body, .. } = expr else {
            panic!("expected fenced expression");
        };
        let ctx = LayoutCtx {
            size: 16.0,
            display: MathDisplay::Inline,
        };
        let target_rect = delimiter_rect(&layout_expr(&body, ctx), ctx);
        let assembled_top = layout
            .atoms
            .iter()
            .filter_map(|atom| match atom {
                MathAtom::GlyphId { rect, .. } => Some(rect.y),
                _ => None,
            })
            .fold(f32::INFINITY, f32::min);
        let assembled_bottom = layout
            .atoms
            .iter()
            .filter_map(|atom| match atom {
                MathAtom::GlyphId { rect, .. } => Some(rect.y + rect.h),
                _ => None,
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            assembled_bottom - assembled_top <= target_rect.h + 0.5,
            "assembled delimiter height should track target height"
        );
    }

    #[test]
    fn rejects_unmatched_tex_right_fence() {
        let err = parse_tex(r"x \right)").expect_err("invalid unmatched fence");
        assert!(err.message.contains("unexpected \\right"));
    }

    #[test]
    fn parses_tex_matrix_environment() {
        let expr = parse_tex(r"\begin{matrix}a&b\\c&d\end{matrix}").expect("valid matrix");
        match expr {
            MathExpr::Table {
                rows,
                column_alignments,
                ..
            } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[1].len(), 2);
                assert_eq!(rows[0][0], MathExpr::Identifier("a".into()));
                assert_eq!(rows[1][1], MathExpr::Identifier("d".into()));
                assert!(column_alignments.is_empty());
            }
            other => panic!("expected table expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_tex_bmatrix_as_fenced_table() {
        let expr =
            parse_tex(r"\begin{bmatrix}a&b\\c&d\end{bmatrix}").expect("valid bracketed matrix");
        match expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open.as_deref(), Some("["));
                assert_eq!(close.as_deref(), Some("]"));
                match body.as_ref() {
                    MathExpr::Table { rows, .. } => {
                        assert_eq!(rows.len(), 2);
                        assert_eq!(rows[0].len(), 2);
                    }
                    other => panic!("expected table body, got {other:?}"),
                }
            }
            other => panic!("expected fenced matrix, got {other:?}"),
        }
    }

    #[test]
    fn parses_tex_cases_as_left_braced_table() {
        let expr = parse_tex(r"\begin{cases}x&x>0\\-x&x<0\end{cases}").expect("valid cases");
        match expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open.as_deref(), Some("{"));
                assert_eq!(close.as_deref(), None);
                match body.as_ref() {
                    MathExpr::Table {
                        column_alignments,
                        column_gap,
                        ..
                    } => {
                        assert_eq!(
                            column_alignments,
                            &vec![MathColumnAlignment::Left, MathColumnAlignment::Left]
                        );
                        assert_eq!(*column_gap, Some(CASES_COL_GAP_EM));
                    }
                    other => panic!("expected table body, got {other:?}"),
                }
            }
            other => panic!("expected left-braced cases table, got {other:?}"),
        }
    }

    #[test]
    fn parses_tex_array_column_alignments() {
        let expr = parse_tex(r"\begin{array}{lr}x&100\\xx&2\end{array}").expect("valid array");
        match expr {
            MathExpr::Table {
                rows,
                column_alignments,
                ..
            } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(
                    column_alignments,
                    vec![MathColumnAlignment::Left, MathColumnAlignment::Right]
                );
            }
            other => panic!("expected array table, got {other:?}"),
        }
    }

    #[test]
    fn ignores_trailing_tex_table_row_separator() {
        let expr = parse_tex(r"\begin{matrix}a&b\\c&d\\\end{matrix}")
            .expect("valid matrix with trailing row separator");
        match expr {
            MathExpr::Table { rows, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[1].len(), 2);
            }
            other => panic!("expected table expression, got {other:?}"),
        }
    }

    #[test]
    fn rejects_inconsistent_tex_table_columns() {
        let err =
            parse_tex(r"\begin{matrix}a&b\\c\end{matrix}").expect_err("invalid ragged matrix");
        assert!(err.message.contains("inconsistent column count"));
    }

    #[test]
    fn rejects_mismatched_tex_array_alignment_spec() {
        let err = parse_tex(r"\begin{array}{lr}x&100&z\\xx&2&y\end{array}")
            .expect_err("invalid array alignment spec");
        assert!(err.message.contains("alignment spec has 2 columns"));
    }

    #[test]
    fn table_layout_honors_column_alignment() {
        let left_aligned = layout_math(
            &MathExpr::Table {
                rows: vec![
                    vec![MathExpr::Identifier("x".into())],
                    vec![MathExpr::Identifier("xxxx".into())],
                ],
                column_alignments: vec![MathColumnAlignment::Left],
                column_gap: None,
                row_gap: None,
            },
            16.0,
            MathDisplay::Inline,
        );
        let right_aligned = layout_math(
            &MathExpr::Table {
                rows: vec![
                    vec![MathExpr::Identifier("x".into())],
                    vec![MathExpr::Identifier("xxxx".into())],
                ],
                column_alignments: vec![MathColumnAlignment::Right],
                column_gap: None,
                row_gap: None,
            },
            16.0,
            MathDisplay::Inline,
        );
        let left_x = left_aligned
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph { text, x, .. } if text == "x" => Some(*x),
                _ => None,
            })
            .expect("left-aligned first cell glyph");
        let right_x = right_aligned
            .atoms
            .iter()
            .find_map(|atom| match atom {
                MathAtom::Glyph { text, x, .. } if text == "x" => Some(*x),
                _ => None,
            })
            .expect("right-aligned first cell glyph");

        assert!(left_x < 0.1, "left-aligned glyph x = {left_x}");
        assert!(
            right_x > left_x + 10.0,
            "right alignment should shift narrow cells across wider columns"
        );
    }

    #[test]
    fn table_layout_honors_table_spacing() {
        let loose = layout_math(
            &MathExpr::Table {
                rows: vec![
                    vec![
                        MathExpr::Identifier("a".into()),
                        MathExpr::Identifier("b".into()),
                    ],
                    vec![
                        MathExpr::Identifier("c".into()),
                        MathExpr::Identifier("d".into()),
                    ],
                ],
                column_alignments: Vec::new(),
                column_gap: Some(2.0),
                row_gap: Some(1.0),
            },
            16.0,
            MathDisplay::Inline,
        );
        let tight = layout_math(
            &MathExpr::Table {
                rows: vec![
                    vec![
                        MathExpr::Identifier("a".into()),
                        MathExpr::Identifier("b".into()),
                    ],
                    vec![
                        MathExpr::Identifier("c".into()),
                        MathExpr::Identifier("d".into()),
                    ],
                ],
                column_alignments: Vec::new(),
                column_gap: Some(0.25),
                row_gap: Some(0.1),
            },
            16.0,
            MathDisplay::Inline,
        );

        assert!(
            loose.width > tight.width + 20.0,
            "loose width = {}, tight width = {}",
            loose.width,
            tight.width
        );
        assert!(
            loose.height() > tight.height() + 10.0,
            "loose height = {}, tight height = {}",
            loose.height(),
            tight.height()
        );
    }

    #[test]
    fn table_layout_centers_on_math_axis() {
        let layout = layout_math(
            &MathExpr::Table {
                rows: vec![
                    vec![
                        MathExpr::Identifier("a".into()),
                        MathExpr::Identifier("b".into()),
                    ],
                    vec![
                        MathExpr::Identifier("c".into()),
                        MathExpr::Identifier("d".into()),
                    ],
                ],
                column_alignments: Vec::new(),
                column_gap: None,
                row_gap: None,
            },
            16.0,
            MathDisplay::Block,
        );
        let visual_center_y = (layout.descent - layout.ascent) * 0.5;
        assert!(
            visual_center_y < -2.0,
            "table visual center should sit on the math axis above baseline, got {visual_center_y}"
        );
    }

    #[test]
    fn math_axis_prefers_open_type_axis_height() {
        let size = 14.0;
        let metrics = LayoutCtx {
            size,
            display: MathDisplay::Block,
        }
        .metrics();
        let expected = metrics
            .font_constants()
            .and_then(|constants| constants.axis_height(size))
            .unwrap_or_else(|| {
                metrics
                    .operator_axis_shift()
                    .expect("operator axis fallback")
            });

        assert!(
            (metrics.math_axis_shift() - expected).abs() < 0.1,
            "axis = {}, expected = {expected}",
            metrics.math_axis_shift()
        );
    }

    #[test]
    fn rejects_mismatched_tex_environment_end() {
        let err = parse_tex(r"\begin{matrix}a\end{pmatrix}").expect_err("invalid environment");
        assert!(err.message.contains(r"expected \end{matrix}"));
    }

    #[test]
    fn reports_unclosed_group() {
        let err = parse_tex(r"\frac{1}{x").expect_err("invalid tex");
        assert!(err.message.contains("unclosed group"));
    }

    #[test]
    fn rejects_tex_double_scripts_like_tex_does() {
        // TeX errors on `x_1_2` ("Double subscript") instead of
        // silently keeping one script. Braced forms stay valid.
        let err = parse_tex("x_1_2").expect_err("double subscript");
        assert!(err.message.contains("double subscript"), "{}", err.message);
        let err = parse_tex("x^1^2").expect_err("double superscript");
        assert!(
            err.message.contains("double superscript"),
            "{}",
            err.message
        );
        // One of each is fine, in either order; nesting via braces too.
        parse_tex("x_1^2").expect("sub then sup");
        parse_tex("x^2_1").expect("sup then sub");
        parse_tex("x_{1_2}").expect("braced nested subscript");
        parse_tex("{x_1}_2").expect("braced base with outer subscript");
    }

    #[test]
    fn negative_mathml_operator_spacing_tightens_layout() {
        // `lspace` / `rspace` accept negative em values (valid MathML
        // tighten-up spacing); they must shift glyphs and shrink width
        // rather than being dropped by a positive-only guard.
        let zero = parse_mathml(r#"<math><mo lspace="0em" rspace="0em">+</mo></math>"#).unwrap();
        let neg =
            parse_mathml(r#"<math><mo lspace="-0.15em" rspace="-0.15em">+</mo></math>"#).unwrap();
        let zero_layout = layout_math(&zero, 16.0, MathDisplay::Inline);
        let neg_layout = layout_math(&neg, 16.0, MathDisplay::Inline);
        let expected = 2.0 * 0.15 * 16.0;
        assert!(
            (zero_layout.width - neg_layout.width - expected).abs() < 1e-3,
            "negative spacing should remove {expected}px: zero={} neg={}",
            zero_layout.width,
            neg_layout.width,
        );
    }

    #[test]
    fn parses_mathml_fraction_with_scripts() {
        let expr = parse_mathml(
            r#"
            <math>
              <mfrac>
                <mrow>
                  <msup><mi>a</mi><mn>2</mn></msup>
                  <mo>+</mo>
                  <msup><mi>b</mi><mn>2</mn></msup>
                </mrow>
                <msqrt>
                  <msub><mi>x</mi><mn>1</mn></msub>
                  <mo>+</mo>
                  <msub><mi>x</mi><mn>2</mn></msub>
                </msqrt>
              </mfrac>
            </math>
            "#,
        )
        .expect("valid mathml");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(layout.width > 20.0, "width = {}", layout.width);
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Rule { .. })),
            "fraction should emit rule atoms"
        );
        assert!(
            has_radical_shape(&layout),
            "sqrt should emit a radical shape atom"
        );
    }

    #[test]
    fn parses_mathml_indexed_root() {
        let expr = parse_mathml(
            r#"
            <math>
              <mroot>
                <mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow>
                <mn>3</mn>
              </mroot>
            </math>
            "#,
        )
        .expect("valid mathml");
        match expr {
            MathExpr::Root { base, index } => {
                assert_eq!(*index, MathExpr::Number("3".into()));
                assert!(matches!(*base, MathExpr::Row(_)));
            }
            other => panic!("expected indexed root, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_under_over() {
        let expr = parse_mathml(
            r#"
            <math>
              <munderover>
                <mo>∑</mo>
                <mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow>
                <mi>n</mi>
              </munderover>
            </math>
            "#,
        )
        .expect("valid mathml");
        match expr {
            MathExpr::UnderOver { base, under, over } => {
                assert_eq!(*base, MathExpr::Operator("∑".into()));
                assert!(matches!(*under.unwrap(), MathExpr::Row(_)));
                assert_eq!(*over.unwrap(), MathExpr::Identifier("n".into()));
            }
            other => panic!("expected under/over expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_operator_spacing_attributes() {
        let expr = parse_mathml(r#"<math><mo lspace="0em" rspace="0.5em">+</mo></math>"#)
            .expect("valid spaced operator");
        assert_eq!(
            expr,
            MathExpr::OperatorWithMetadata {
                text: "+".into(),
                lspace: Some(0.0),
                rspace: Some(0.5),
                large_operator: None,
                movable_limits: None,
            }
        );

        let default_width =
            layout_math(&MathExpr::Operator("+".into()), 16.0, MathDisplay::Inline).width;
        let custom_width = layout_math(&expr, 16.0, MathDisplay::Inline).width;
        assert!(
            custom_width > default_width,
            "custom width = {custom_width}, default width = {default_width}"
        );
    }

    #[test]
    fn parses_mathml_operator_limit_attributes() {
        let expr = parse_mathml(
            r#"
            <math>
              <msub>
                <mo movablelimits="true">lim</mo>
                <mi>x</mi>
              </msub>
            </math>
            "#,
        )
        .expect("valid movable limits operator");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, y_baseline, .. } if text == "x" && *y_baseline > 0.0)),
            "movablelimits operator should place display subscript underneath"
        );

        let large = parse_mathml(r#"<math><mo largeop="true">∫</mo></math>"#)
            .expect("valid large operator");
        assert!(matches!(
            large,
            MathExpr::OperatorWithMetadata {
                large_operator: Some(true),
                ..
            }
        ));
    }

    #[test]
    fn parses_mathml_accent_mover() {
        let expr = parse_mathml(
            r#"
            <math>
              <mover accent="true">
                <mi>x</mi>
                <mo>^</mo>
              </mover>
            </math>
            "#,
        )
        .expect("valid mathml accent");
        match expr {
            MathExpr::Accent {
                base,
                accent,
                stretch,
            } => {
                assert_eq!(*base, MathExpr::Identifier("x".into()));
                assert_eq!(*accent, MathExpr::Operator("^".into()));
                assert!(!stretch);
            }
            other => panic!("expected accent expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_semantics_wrapper() {
        let expr = parse_mathml(
            r#"
            <math>
              <semantics>
                <mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow>
                <annotation encoding="application/x-tex">x+1</annotation>
              </semantics>
            </math>
            "#,
        )
        .expect("valid mathml semantics wrapper");
        match expr {
            MathExpr::Row(children) => {
                assert_eq!(children.len(), 3);
                assert_eq!(children[0], MathExpr::Identifier("x".into()));
                assert_eq!(children[2], MathExpr::Number("1".into()));
            }
            other => panic!("expected row expression, got {other:?}"),
        }
    }

    #[test]
    fn rejects_mathml_semantics_without_presentation_child() {
        let err = parse_mathml(
            r#"
            <math>
              <semantics>
                <annotation encoding="application/x-tex">x+1</annotation>
              </semantics>
            </math>
            "#,
        )
        .expect_err("invalid mathml semantics wrapper");
        assert!(
            err.message
                .contains("<semantics> expected a presentation child")
        );
    }

    #[test]
    fn parses_mathml_fenced_expression() {
        let expr = parse_mathml(
            r#"
            <math>
              <mfenced open="[" close="]" separators=",">
                <mi>a</mi>
                <mi>b</mi>
              </mfenced>
            </math>
            "#,
        )
        .expect("valid mathml fenced expression");
        match expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open.as_deref(), Some("["));
                assert_eq!(close.as_deref(), Some("]"));
                match body.as_ref() {
                    MathExpr::Row(children) => {
                        assert_eq!(children.len(), 3);
                        assert_eq!(children[1], MathExpr::Operator(",".into()));
                    }
                    other => panic!("expected row body, got {other:?}"),
                }
            }
            other => panic!("expected fenced expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_table() {
        let expr = parse_mathml(
            r#"
            <math>
              <mtable>
                <mtr>
                  <mtd><mi>a</mi></mtd>
                  <mtd><mi>b</mi></mtd>
                </mtr>
                <mtr>
                  <mtd><mi>c</mi></mtd>
                  <mtd><mi>d</mi></mtd>
                </mtr>
              </mtable>
            </math>
            "#,
        )
        .expect("valid mathml");
        match expr {
            MathExpr::Table { rows, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[1].len(), 2);
            }
            other => panic!("expected table expression, got {other:?}"),
        }
        let layout = layout_math(
            &parse_mathml(
                r#"<math><mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr><mtd><mi>c</mi></mtd><mtd><mi>d</mi></mtd></mtr></mtable></math>"#,
            )
            .unwrap(),
            16.0,
            MathDisplay::Block,
        );
        assert!(layout.width > 20.0, "width = {}", layout.width);
        assert!(layout.ascent > 10.0, "ascent = {}", layout.ascent);
        assert!(layout.descent > 10.0, "descent = {}", layout.descent);
    }

    #[test]
    fn parses_mathml_table_column_alignment() {
        let expr = parse_mathml(
            r#"
            <math>
              <mtable columnalign="left right">
                <mtr>
                  <mtd><mi>x</mi></mtd>
                  <mtd><mn>100</mn></mtd>
                </mtr>
              </mtable>
            </math>
            "#,
        )
        .expect("valid aligned mathml table");
        match expr {
            MathExpr::Table {
                column_alignments, ..
            } => {
                assert_eq!(
                    column_alignments,
                    vec![MathColumnAlignment::Left, MathColumnAlignment::Right]
                );
            }
            other => panic!("expected table expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_table_spacing() {
        let expr = parse_mathml(
            r#"
            <math>
              <mtable columnspacing="0.5em" rowspacing="0.2em">
                <mtr>
                  <mtd><mi>a</mi></mtd>
                  <mtd><mi>b</mi></mtd>
                </mtr>
                <mtr>
                  <mtd><mi>c</mi></mtd>
                  <mtd><mi>d</mi></mtd>
                </mtr>
              </mtable>
            </math>
            "#,
        )
        .expect("valid spaced mathml table");
        match expr {
            MathExpr::Table {
                column_gap,
                row_gap,
                ..
            } => {
                assert_eq!(column_gap, Some(0.5));
                assert_eq!(row_gap, Some(0.2));
            }
            other => panic!("expected table expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_display_attribute() {
        let (expr, display) = parse_mathml_with_display(
            r#"<math display="block"><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup></math>"#,
        )
        .expect("valid mathml");
        assert_eq!(display, MathDisplay::Block);
        match expr {
            MathExpr::Scripts { base, sub, sup } => {
                assert_eq!(*base, MathExpr::Identifier("x".into()));
                assert_eq!(*sub.unwrap(), MathExpr::Number("1".into()));
                assert_eq!(*sup.unwrap(), MathExpr::Number("2".into()));
            }
            other => panic!("expected scripts expression, got {other:?}"),
        }
    }

    #[test]
    fn rejects_wrong_mathml_arity() {
        let err =
            parse_mathml(r#"<math><mfrac><mi>a</mi></mfrac></math>"#).expect_err("invalid arity");
        assert!(err.message.contains("expected 2 element children"));
    }
}
