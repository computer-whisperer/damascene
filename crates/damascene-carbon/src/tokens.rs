//! Carbon Design System tokens — the dense-app counterpart to
//! `damascene_core::tokens`.
//!
//! Naming mirrors Carbon's published CSS custom properties so that LLM
//! training transfers the same way damascene-core's names transfer from
//! shadcn/Tailwind: `--cds-layer-01` is [`LAYER_01`], `--cds-spacing-05`
//! is [`SPACING_05`], `$body-compact-01` is [`BODY_COMPACT_01`].
//!
//! ## Why a second vocabulary
//!
//! damascene-core speaks shadcn, which is a *content-site* system: one
//! surface (`card == background` in the default palette), one line color,
//! generous padding, 12px type floor. Carbon is an *application* system —
//! IBM built it for dense enterprise data products. The two differ in
//! ways that matter here:
//!
//! - **Surfaces are a named ramp**, not a single `card`. [`LAYER_01`] /
//!   [`LAYER_02`] / [`LAYER_03`] step upward from [`BACKGROUND`], so
//!   containment reads as *value* rather than as an outline. shadcn has
//!   no equivalent.
//! - **Density is first-class**: [`SPACING_01`] is 2px (Tailwind's floor
//!   is 4px), and Carbon ships explicit `*-compact-*` type styles.
//! - **Corners are square.** Carbon components are `border-radius: 0`
//!   apart from pill-shaped tags. That is the single biggest visual
//!   difference from the shadcn set and it is deliberate, not a tuning
//!   choice.
//!
//! Token names here deliberately do **not** collide with
//! `damascene_core::tokens` (`CARD`, `MUTED`, `ACCENT`, `RADIUS_LG`, …),
//! so it is always unambiguous which system a call site is in.
//!
//! ## Values
//!
//! Colors are the **Gray 100 (`g100`)** theme — Carbon's darkest stock
//! theme — taken from the published palette rather than tuned by eye.
//! A fresh author writing `LAYER_01` should get Carbon's `#262626`, not
//! a house variant of it.
//!
//! These are minted via [`Color::srgb_token`], so they satisfy the
//! `RawColor` lint. `Palette::lookup` only recognizes the shadcn names,
//! so a Carbon token resolves to the literal rgba below regardless of
//! the active palette — see [`crate::theme`] for why that is the
//! intended behavior today.

#![warn(missing_docs)]

use damascene_core::tree::{Color, FontWeight};

// ---------------------------------------------------------------------
// Gray palette — Carbon's 10-step neutral scale.
//
// Exposed because Carbon documents its themes in terms of these steps
// ("layer-01 is gray-90 in g100"), and authors reach for them directly
// for one-off chrome.
// ---------------------------------------------------------------------

/// Carbon `gray-10` — `#f4f4f4`.
pub const GRAY_10: Color = Color::srgb_token("gray-10", 244, 244, 244, 255);
/// Carbon `gray-20` — `#e0e0e0`.
pub const GRAY_20: Color = Color::srgb_token("gray-20", 224, 224, 224, 255);
/// Carbon `gray-30` — `#c6c6c6`.
pub const GRAY_30: Color = Color::srgb_token("gray-30", 198, 198, 198, 255);
/// Carbon `gray-40` — `#a8a8a8`.
pub const GRAY_40: Color = Color::srgb_token("gray-40", 168, 168, 168, 255);
/// Carbon `gray-50` — `#8d8d8d`.
pub const GRAY_50: Color = Color::srgb_token("gray-50", 141, 141, 141, 255);
/// Carbon `gray-60` — `#6f6f6f`.
pub const GRAY_60: Color = Color::srgb_token("gray-60", 111, 111, 111, 255);
/// Carbon `gray-70` — `#525252`.
pub const GRAY_70: Color = Color::srgb_token("gray-70", 82, 82, 82, 255);
/// Carbon `gray-80` — `#393939`.
pub const GRAY_80: Color = Color::srgb_token("gray-80", 57, 57, 57, 255);
/// Carbon `gray-90` — `#262626`.
pub const GRAY_90: Color = Color::srgb_token("gray-90", 38, 38, 38, 255);
/// Carbon `gray-100` — `#161616`.
pub const GRAY_100: Color = Color::srgb_token("gray-100", 22, 22, 22, 255);

// ---------------------------------------------------------------------
// Layer ramp — the core Carbon containment idiom.
//
// Carbon's model: `background` is the page, and each nested surface
// steps up one `layer`. A pane on the page is `layer-01`; a card inside
// that pane is `layer-02`; a popover above it is `layer-03`. Nothing
// needs a border to be legible, which is the whole point.
// ---------------------------------------------------------------------

/// App page background — Carbon `background` (`gray-100`).
pub const BACKGROUND: Color = Color::srgb_token("background", 22, 22, 22, 255);
/// First surface step above [`BACKGROUND`] — Carbon `layer-01` (`gray-90`).
pub const LAYER_01: Color = Color::srgb_token("layer-01", 38, 38, 38, 255);
/// Second surface step — Carbon `layer-02` (`gray-80`). A surface nested
/// inside a [`LAYER_01`] container.
pub const LAYER_02: Color = Color::srgb_token("layer-02", 57, 57, 57, 255);
/// Third surface step — Carbon `layer-03` (`gray-70`). Floating content
/// above [`LAYER_02`].
pub const LAYER_03: Color = Color::srgb_token("layer-03", 82, 82, 82, 255);

/// Hover state for a [`LAYER_01`] surface — Carbon `layer-hover-01`.
pub const LAYER_HOVER_01: Color = Color::srgb_token("layer-hover-01", 51, 51, 51, 255);
/// Hover state for a [`LAYER_02`] surface — Carbon `layer-hover-02`.
pub const LAYER_HOVER_02: Color = Color::srgb_token("layer-hover-02", 71, 71, 71, 255);
/// Selected row / item on a [`LAYER_01`] surface — Carbon `layer-selected-01`.
pub const LAYER_SELECTED_01: Color = Color::srgb_token("layer-selected-01", 57, 57, 57, 255);
/// Accent companion to [`LAYER_01`] — Carbon `layer-accent-01`. Used for
/// table header rows and other bands that must read as *part of* the
/// layer without stepping fully up to the next one.
pub const LAYER_ACCENT_01: Color = Color::srgb_token("layer-accent-01", 57, 57, 57, 255);

/// Input/field fill on a [`BACKGROUND`] or [`LAYER_01`] surface — Carbon `field-01`.
pub const FIELD_01: Color = Color::srgb_token("field-01", 38, 38, 38, 255);
/// Input/field fill on a [`LAYER_02`] surface — Carbon `field-02`.
pub const FIELD_02: Color = Color::srgb_token("field-02", 57, 57, 57, 255);

// ---------------------------------------------------------------------
// Borders — Carbon separates "subtle" (structural hairlines) from
// "strong" (deliberate emphasis). Most shell chrome uses subtle.
// ---------------------------------------------------------------------

/// Hairline on a [`BACKGROUND`] surface — Carbon `border-subtle-00`.
pub const BORDER_SUBTLE_00: Color = Color::srgb_token("border-subtle-00", 57, 57, 57, 255);
/// Hairline on a [`LAYER_01`] surface — Carbon `border-subtle-01`.
pub const BORDER_SUBTLE_01: Color = Color::srgb_token("border-subtle-01", 57, 57, 57, 255);
/// Hairline on a [`LAYER_02`] surface — Carbon `border-subtle-02`.
pub const BORDER_SUBTLE_02: Color = Color::srgb_token("border-subtle-02", 82, 82, 82, 255);
/// Hairline on a [`LAYER_03`] surface — Carbon `border-subtle-03`.
pub const BORDER_SUBTLE_03: Color = Color::srgb_token("border-subtle-03", 111, 111, 111, 255);

/// Emphasized border on [`LAYER_01`] — Carbon `border-strong-01`.
pub const BORDER_STRONG_01: Color = Color::srgb_token("border-strong-01", 111, 111, 111, 255);
/// Emphasized border on [`LAYER_02`] — Carbon `border-strong-02`.
pub const BORDER_STRONG_02: Color = Color::srgb_token("border-strong-02", 141, 141, 141, 255);
/// Interactive/selected border — Carbon `border-interactive` (`blue-50`).
pub const BORDER_INTERACTIVE: Color = Color::srgb_token("border-interactive", 69, 137, 255, 255);

// ---------------------------------------------------------------------
// Text + icon — a real four-step hierarchy, versus shadcn's
// foreground / muted-foreground pair.
// ---------------------------------------------------------------------

/// Primary body and heading text — Carbon `text-primary` (`gray-10`).
pub const TEXT_PRIMARY: Color = Color::srgb_token("text-primary", 244, 244, 244, 255);
/// Secondary text — Carbon `text-secondary` (`gray-30`). Labels beside
/// a primary value, column headers.
pub const TEXT_SECONDARY: Color = Color::srgb_token("text-secondary", 198, 198, 198, 255);
/// Helper / caption text — Carbon `text-helper` (`gray-50`).
pub const TEXT_HELPER: Color = Color::srgb_token("text-helper", 141, 141, 141, 255);
/// Input placeholder — Carbon `text-placeholder` (`gray-60`).
pub const TEXT_PLACEHOLDER: Color = Color::srgb_token("text-placeholder", 111, 111, 111, 255);
/// Text on a saturated fill — Carbon `text-on-color`.
pub const TEXT_ON_COLOR: Color = Color::srgb_token("text-on-color", 255, 255, 255, 255);
/// Text on an inverted surface — Carbon `text-inverse`.
pub const TEXT_INVERSE: Color = Color::srgb_token("text-inverse", 22, 22, 22, 255);

/// Primary icon color — Carbon `icon-primary`.
pub const ICON_PRIMARY: Color = Color::srgb_token("icon-primary", 244, 244, 244, 255);
/// Secondary icon color — Carbon `icon-secondary`.
pub const ICON_SECONDARY: Color = Color::srgb_token("icon-secondary", 198, 198, 198, 255);

/// Hyperlink text — Carbon `link-primary` (`blue-40` on dark).
pub const LINK_PRIMARY: Color = Color::srgb_token("link-primary", 120, 169, 255, 255);

// ---------------------------------------------------------------------
// Interactive + support
// ---------------------------------------------------------------------

/// Primary interactive accent — Carbon `interactive` (`blue-50`).
pub const INTERACTIVE: Color = Color::srgb_token("interactive", 69, 137, 255, 255);
/// Focus ring — Carbon `focus`. White on dark themes, deliberately
/// higher-contrast than the interactive accent.
pub const FOCUS: Color = Color::srgb_token("focus", 255, 255, 255, 255);

/// Primary button fill — Carbon `button-primary` (`blue-60`).
pub const BUTTON_PRIMARY: Color = Color::srgb_token("button-primary", 15, 98, 254, 255);
/// Primary button hover fill — Carbon `button-primary-hover`.
pub const BUTTON_PRIMARY_HOVER: Color = Color::srgb_token("button-primary-hover", 3, 83, 233, 255);
/// Secondary button fill — Carbon `button-secondary` (`gray-60`).
pub const BUTTON_SECONDARY: Color = Color::srgb_token("button-secondary", 111, 111, 111, 255);
/// Danger button fill — Carbon `button-danger-primary` (`red-60`).
pub const BUTTON_DANGER: Color = Color::srgb_token("button-danger-primary", 218, 30, 40, 255);

/// Error status — Carbon `support-error` (`red-40` on dark).
pub const SUPPORT_ERROR: Color = Color::srgb_token("support-error", 255, 131, 137, 255);
/// Success status — Carbon `support-success` (`green-40`).
pub const SUPPORT_SUCCESS: Color = Color::srgb_token("support-success", 66, 190, 101, 255);
/// Warning status — Carbon `support-warning` (`yellow-30`).
pub const SUPPORT_WARNING: Color = Color::srgb_token("support-warning", 241, 194, 27, 255);
/// Informational status — Carbon `support-info` (`blue-50`).
pub const SUPPORT_INFO: Color = Color::srgb_token("support-info", 69, 137, 255, 255);

/// Modal scrim — Carbon `overlay` (black at 65%).
pub const OVERLAY: Color = Color::srgb_token("overlay", 0, 0, 0, 166);

// ---------------------------------------------------------------------
// Tag colors — Carbon `Tag` is the one pill-shaped component, and each
// hue ships as a background/foreground pair.
// ---------------------------------------------------------------------

/// Gray tag fill — Carbon `tag-background-gray`.
pub const TAG_BG_GRAY: Color = Color::srgb_token("tag-background-gray", 82, 82, 82, 255);
/// Gray tag text — Carbon `tag-color-gray`.
pub const TAG_FG_GRAY: Color = Color::srgb_token("tag-color-gray", 244, 244, 244, 255);
/// Blue tag fill — Carbon `tag-background-blue`.
pub const TAG_BG_BLUE: Color = Color::srgb_token("tag-background-blue", 0, 67, 206, 255);
/// Blue tag text — Carbon `tag-color-blue`.
pub const TAG_FG_BLUE: Color = Color::srgb_token("tag-color-blue", 208, 226, 255, 255);
/// Green tag fill — Carbon `tag-background-green`.
pub const TAG_BG_GREEN: Color = Color::srgb_token("tag-background-green", 4, 67, 23, 255);
/// Green tag text — Carbon `tag-color-green`.
pub const TAG_FG_GREEN: Color = Color::srgb_token("tag-color-green", 167, 240, 186, 255);
/// Red tag fill — Carbon `tag-background-red`.
pub const TAG_BG_RED: Color = Color::srgb_token("tag-background-red", 162, 25, 31, 255);
/// Red tag text — Carbon `tag-color-red`.
pub const TAG_FG_RED: Color = Color::srgb_token("tag-color-red", 255, 215, 217, 255);

// ---------------------------------------------------------------------
// Spacing — Carbon's 13-step scale.
//
// Note the floor: `SPACING_01` is 2px, where Tailwind (and therefore
// damascene-core's `SPACE_1`) starts at 4px. Dense chrome lives in
// steps 01–03.
// ---------------------------------------------------------------------

/// 2px — Carbon `spacing-01`. Tightest step; icon-to-glyph nudges.
pub const SPACING_01: f32 = 2.0;
/// 4px — Carbon `spacing-02`. Dense row vertical padding.
pub const SPACING_02: f32 = 4.0;
/// 8px — Carbon `spacing-03`. The workhorse for table cells and toolbars.
pub const SPACING_03: f32 = 8.0;
/// 12px — Carbon `spacing-04`.
pub const SPACING_04: f32 = 12.0;
/// 16px — Carbon `spacing-05`. Standard container padding.
pub const SPACING_05: f32 = 16.0;
/// 24px — Carbon `spacing-06`.
pub const SPACING_06: f32 = 24.0;
/// 32px — Carbon `spacing-07`.
pub const SPACING_07: f32 = 32.0;
/// 40px — Carbon `spacing-08`.
pub const SPACING_08: f32 = 40.0;
/// 48px — Carbon `spacing-09`.
pub const SPACING_09: f32 = 48.0;
/// 64px — Carbon `spacing-10`.
pub const SPACING_10: f32 = 64.0;
/// 80px — Carbon `spacing-11`.
pub const SPACING_11: f32 = 80.0;
/// 96px — Carbon `spacing-12`.
pub const SPACING_12: f32 = 96.0;
/// 160px — Carbon `spacing-13`.
pub const SPACING_13: f32 = 160.0;

// ---------------------------------------------------------------------
// Radius — Carbon is a square system.
// ---------------------------------------------------------------------

/// 0px — Carbon's default corner treatment for every surface and
/// control. Not a stylistic omission: Carbon components are square, and
/// this constant exists so call sites can say so explicitly rather than
/// leaving radius unset and looking accidental.
pub const RADIUS_NONE: f32 = 0.0;
/// 4px — the small radius Carbon v11 applies to a few components
/// (notifications, some containers).
pub const RADIUS_SM: f32 = 4.0;
/// Effectively-infinite radius, for pill-shaped [`crate::widgets::tag`].
pub const RADIUS_PILL: f32 = 999.0;

// ---------------------------------------------------------------------
// Type styles — Carbon's named type sets.
//
// Carbon's own face is IBM Plex Sans, which damascene does not bundle;
// Inter is the substitute (see `crate::theme`). Sizes, line heights,
// weights, and letter-spacing are Carbon's published values.
// ---------------------------------------------------------------------

/// A Carbon type style: size, line height, weight, and letter spacing.
///
/// Apply with [`crate::widgets::styled`] rather than setting the four
/// properties by hand, so a style stays a single named decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    /// Font size in logical px.
    pub size: f32,
    /// Line height in logical px.
    pub line_height: f32,
    /// Font weight.
    pub weight: FontWeight,
    /// Letter spacing in logical px (Carbon publishes these in px, not em).
    pub letter_spacing: f32,
}

impl TypeStyle {
    const fn new(size: f32, line_height: f32, weight: FontWeight, letter_spacing: f32) -> Self {
        Self {
            size,
            line_height,
            weight,
            letter_spacing,
        }
    }
}

/// 12/16 — Carbon `label-01`. Column headers, field labels, chip text.
pub const LABEL_01: TypeStyle = TypeStyle::new(12.0, 16.0, FontWeight::Regular, 0.32);
/// 14/18 — Carbon `label-02`.
pub const LABEL_02: TypeStyle = TypeStyle::new(14.0, 18.0, FontWeight::Regular, 0.16);
/// 12/16 — Carbon `helper-text-01`. Hints below a field.
pub const HELPER_TEXT_01: TypeStyle = TypeStyle::new(12.0, 16.0, FontWeight::Regular, 0.32);
/// 12/16 — Carbon `code-01`. Identifiers, hashes, paths in dense cells.
pub const CODE_01: TypeStyle = TypeStyle::new(12.0, 16.0, FontWeight::Regular, 0.32);
/// 14/20 — Carbon `code-02`. Source listings.
pub const CODE_02: TypeStyle = TypeStyle::new(14.0, 20.0, FontWeight::Regular, 0.32);
/// 14/18 — Carbon `body-compact-01`. **The default for dense UI**: same
/// size as `body-01` with a tighter leading, which is where a good part
/// of Carbon's density comes from.
pub const BODY_COMPACT_01: TypeStyle = TypeStyle::new(14.0, 18.0, FontWeight::Regular, 0.16);
/// 16/22 — Carbon `body-compact-02`.
pub const BODY_COMPACT_02: TypeStyle = TypeStyle::new(16.0, 22.0, FontWeight::Regular, 0.0);
/// 14/20 — Carbon `body-01`. Prose default.
pub const BODY_01: TypeStyle = TypeStyle::new(14.0, 20.0, FontWeight::Regular, 0.16);
/// 16/24 — Carbon `body-02`.
pub const BODY_02: TypeStyle = TypeStyle::new(16.0, 24.0, FontWeight::Regular, 0.0);
/// 14/18 semibold — Carbon `heading-compact-01`. Section headers in
/// dense chrome (pane titles, panel headers).
pub const HEADING_COMPACT_01: TypeStyle = TypeStyle::new(14.0, 18.0, FontWeight::Semibold, 0.16);
/// 16/22 semibold — Carbon `heading-compact-02`.
pub const HEADING_COMPACT_02: TypeStyle = TypeStyle::new(16.0, 22.0, FontWeight::Semibold, 0.0);
/// 14/18 semibold — Carbon `heading-01`.
pub const HEADING_01: TypeStyle = TypeStyle::new(14.0, 18.0, FontWeight::Semibold, 0.16);
/// 16/22 semibold — Carbon `heading-02`.
pub const HEADING_02: TypeStyle = TypeStyle::new(16.0, 22.0, FontWeight::Semibold, 0.0);
/// 20/28 — Carbon `heading-03`.
pub const HEADING_03: TypeStyle = TypeStyle::new(20.0, 28.0, FontWeight::Regular, 0.0);
/// 28/36 — Carbon `heading-04`.
pub const HEADING_04: TypeStyle = TypeStyle::new(28.0, 36.0, FontWeight::Regular, 0.0);
/// 32/40 — Carbon `heading-05`.
pub const HEADING_05: TypeStyle = TypeStyle::new(32.0, 40.0, FontWeight::Regular, 0.0);

// ---------------------------------------------------------------------
// Sizing — Carbon control/row heights, plus the VS Code workbench
// region metrics the shell widgets use.
// ---------------------------------------------------------------------

/// 24px — Carbon `DataTable` `size="xs"` row height. The densest tabular row.
pub const ROW_HEIGHT_XS: f32 = 24.0;
/// 32px — Carbon `DataTable` `size="sm"` row height.
pub const ROW_HEIGHT_SM: f32 = 32.0;
/// 40px — Carbon `DataTable` `size="md"` row height (Carbon's default).
pub const ROW_HEIGHT_MD: f32 = 40.0;
/// 48px — Carbon `DataTable` `size="lg"` row height.
pub const ROW_HEIGHT_LG: f32 = 48.0;

/// 32px — Carbon `sm` field/button height.
pub const FIELD_HEIGHT_SM: f32 = 32.0;
/// 40px — Carbon `md` field/button height (Carbon's default).
pub const FIELD_HEIGHT_MD: f32 = 40.0;

/// 48px — Carbon UI Shell header height.
pub const HEADER_HEIGHT: f32 = 48.0;
/// 256px — Carbon UI Shell side-nav width.
pub const SIDE_NAV_WIDTH: f32 = 256.0;

/// 22px — status bar height, from VS Code's `statusBar`. Carbon has no
/// status-bar component; the workbench is the reference for shell
/// anatomy Carbon does not cover.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;
/// 35px — pane/tab header height, from VS Code's `editorGroupHeader`.
pub const PANE_HEADER_HEIGHT: f32 = 35.0;
/// 22px — breadcrumb strip height, from VS Code's `breadcrumb`.
pub const BREADCRUMB_HEIGHT: f32 = 22.0;
/// 48px — icon rail width, from VS Code's `activityBar`.
pub const ACTIVITY_BAR_WIDTH: f32 = 48.0;

/// 1px — the structural hairline weight used throughout the shell.
pub const HAIRLINE: f32 = 1.0;

/// 16px — Carbon's default icon box, matching `ICON_SM` in core.
pub const ICON_SIZE: f32 = 16.0;
/// 20px — larger icon box for toolbar affordances.
pub const ICON_SIZE_MD: f32 = 20.0;
