//! Design tokens — the named, finite vocabulary of values everything else
//! refers to.
//!
//! Tokens are public `const` values, not a runtime struct. That means:
//!
//! - Reading the file once gives you the entire vocabulary.
//! - Components reference tokens by name (`tokens::CARD`) directly,
//!   no theme handle threaded through every call.
//! - Constants are inlined and indistinguishable from any other f32/Color
//!   at runtime — there's no `OnceLock` to initialize.
//!
//! Naming intentionally shadows shadcn/Tailwind so LLM training transfers:
//! `CARD`, `MUTED_FOREGROUND`, `RADIUS_LG`, `SPACE_3`, etc.
//!
//! ## Palette resolution
//!
//! [`Color`] tokens carry a `token: Some(name)` marker. The renderer
//! resolves each tokened color through the active [`crate::Palette`] at
//! paint time, so the rgba seen on screen tracks
//! [`crate::Theme`]`::palette()` rather than the constants in this file.
//! The constants here serve as the **fallback rgba** — used when a token
//! has no palette entry or when the host renders without a theme
//! (`draw_ops` with the default `Theme`, whose default palette mirrors
//! these constants exactly). The fallback values match the default
//! Damascene Dark palette, which is copied from shadcn/ui zinc dark; pick a
//! different palette at runtime via [`crate::Theme::with_palette`] or
//! [`crate::Theme::damascene_light`].

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::tree::Color;

// ---- Palette ----
//
// Color tokens carry the default Damascene Dark / shadcn zinc dark rgba as
// their compile-time fallback. Apps swap to a light palette at runtime via
// `Theme::damascene_light()` (see `crate::Palette` for the full token →
// rgba mapping for each palette).

// Core shadcn-shaped semantic colors.
/// App-level page background — the shadcn `background` token.
pub const BACKGROUND: Color = Color::srgb_token("background", 9, 9, 11, 255);
/// Default text color on [`BACKGROUND`] — the shadcn `foreground` token.
pub const FOREGROUND: Color = Color::srgb_token("foreground", 250, 250, 250, 255);

/// Card surface fill — the shadcn `card` token.
pub const CARD: Color = Color::srgb_token("card", 9, 9, 11, 255);
/// Text color on [`CARD`] surfaces — the shadcn `card-foreground` token.
pub const CARD_FOREGROUND: Color = Color::srgb_token("card-foreground", 250, 250, 250, 255);

/// Popover/menu/tooltip surface fill — the shadcn `popover` token.
pub const POPOVER: Color = Color::srgb_token("popover", 9, 9, 11, 255);
/// Text color on [`POPOVER`] surfaces — the shadcn `popover-foreground` token.
pub const POPOVER_FOREGROUND: Color = Color::srgb_token("popover-foreground", 250, 250, 250, 255);

/// Primary action color — the shadcn `primary` token; applied by `.primary()`.
pub const PRIMARY: Color = Color::srgb_token("primary", 250, 250, 250, 255);
/// Text/icon color on solid [`PRIMARY`] fills — the shadcn `primary-foreground` token.
pub const PRIMARY_FOREGROUND: Color = Color::srgb_token("primary-foreground", 24, 24, 27, 255);

/// Secondary action surface fill — the shadcn `secondary` token; the default button look.
pub const SECONDARY: Color = Color::srgb_token("secondary", 39, 39, 42, 255);
/// Text color on [`SECONDARY`] fills — the shadcn `secondary-foreground` token.
pub const SECONDARY_FOREGROUND: Color =
    Color::srgb_token("secondary-foreground", 250, 250, 250, 255);

/// Neutral muted surface fill — the shadcn `muted` token; applied by `.muted()`.
pub const MUTED: Color = Color::srgb_token("muted", 39, 39, 42, 255);
/// De-emphasized text color (captions, placeholders) — the shadcn `muted-foreground` token.
pub const MUTED_FOREGROUND: Color = Color::srgb_token("muted-foreground", 161, 161, 170, 255);

/// Hover/current-item highlight surface — the shadcn `accent` token; backs `.current()`.
pub const ACCENT: Color = Color::srgb_token("accent", 39, 39, 42, 255);
/// Text color on [`ACCENT`] surfaces — the shadcn `accent-foreground` token.
pub const ACCENT_FOREGROUND: Color = Color::srgb_token("accent-foreground", 250, 250, 250, 255);

/// Destructive action color (delete buttons, error treatments) — the shadcn `destructive` token.
pub const DESTRUCTIVE: Color = Color::srgb_token("destructive", 127, 29, 29, 255);
/// Text color on solid [`DESTRUCTIVE`] fills — the shadcn `destructive-foreground` token.
pub const DESTRUCTIVE_FOREGROUND: Color =
    Color::srgb_token("destructive-foreground", 250, 250, 250, 255);

/// Default border/divider stroke — the shadcn `border` token.
pub const BORDER: Color = Color::srgb_token("border", 39, 39, 42, 255);
/// Input-field border stroke — the shadcn `input` token; also used by `.outline()`.
pub const INPUT: Color = Color::srgb_token("input", 39, 39, 42, 255);
/// Keyboard-focus ring stroke — the shadcn `ring` token; drawn [`RING_WIDTH`] outside the bounds.
pub const RING: Color = Color::srgb_token("ring", 212, 212, 216, 255);

/// Positive status color — applied by `.success()` (badges, toasts, validation).
pub const SUCCESS: Color = Color::srgb_token("success", 16, 185, 129, 255);
/// Text color on solid [`SUCCESS`] fills.
pub const SUCCESS_FOREGROUND: Color = Color::srgb_token("success-foreground", 5, 46, 22, 255);
/// Cautionary status color — applied by `.warning()`.
pub const WARNING: Color = Color::srgb_token("warning", 245, 158, 11, 255);
/// Text color on solid [`WARNING`] fills.
pub const WARNING_FOREGROUND: Color = Color::srgb_token("warning-foreground", 69, 26, 3, 255);
/// Informational status color — applied by `.info()`.
pub const INFO: Color = Color::srgb_token("info", 59, 130, 246, 255);
/// Text color on solid [`INFO`] fills.
pub const INFO_FOREGROUND: Color = Color::srgb_token("info-foreground", 239, 246, 255, 255);

// Extension colors. These remain semantic, but they describe a specific
// component/domain rather than the reusable shadcn core palette.
/// Dimming scrim drawn behind modal overlays (dialogs, sheets).
pub const OVERLAY_SCRIM: Color = Color::srgb_token("overlay-scrim", 0, 0, 0, 204);

/// Themed link color. Picked up automatically by `.link(url)` runs
/// (and any `RunStyle.link.is_some()` run, regardless of how it was
/// constructed). Distinct from `PRIMARY` so an underlined link reads
/// as a link, not an action accent — brighter on dark, darker on light.
pub const LINK_FOREGROUND: Color = Color::srgb_token("link-foreground", 96, 165, 250, 255);

// ---- Spacing ----
//
// Spacing follows Tailwind's numeric scale so layout code reads like
// the UI examples LLMs have seen most often: `gap-3` is 12 px, `p-4`
// is 16 px, `mt-2` is 8 px, etc.
/// 0 logical px — Tailwind step 0; collapses a gap or inset entirely.
pub const SPACE_0: f32 = 0.0;
/// 4 logical px — Tailwind step 1; the tightest non-zero inset (e.g. tooltip vertical padding).
pub const SPACE_1: f32 = 4.0;
/// 8 logical px — Tailwind step 2; the common small gap (icon-to-label, breadcrumb segments).
pub const SPACE_2: f32 = 8.0;
/// 12 logical px — Tailwind step 3; compact surface padding (code blocks, toast content).
pub const SPACE_3: f32 = 12.0;
/// 16 logical px — Tailwind step 4; the default padding for cards, sheets, and popovers.
pub const SPACE_4: f32 = 16.0;
/// 20 logical px — Tailwind step 5.
pub const SPACE_5: f32 = 20.0;
/// 24 logical px — Tailwind step 6; roomy section padding.
pub const SPACE_6: f32 = 24.0;
/// 28 logical px — Tailwind step 7.
pub const SPACE_7: f32 = 28.0;
/// 32 logical px — Tailwind step 8; large separation between sections.
pub const SPACE_8: f32 = 32.0;
/// 40 logical px — Tailwind step 10.
pub const SPACE_10: f32 = 40.0;
/// 48 logical px — Tailwind step 12; the largest step (page-level gutters).
pub const SPACE_12: f32 = 48.0;

// ---- Pinned-pane sizing ----
//
// Conventional starting widths for a resizable sidebar (file tree,
// settings nav, inspector). Sourced from VS Code (~240px), Slack
// (~270px), and Finder (~250px) — wide enough that label content
// stays readable, narrow enough that the main work area still
// dominates. `_MIN` is the floor below which file/label content
// truncates badly; `_MAX` is the ceiling above which a sidebar
// stops being a sidebar.
/// Conventional starting width for a pinned sidebar (256 logical px) — the stock `sidebar` widget's default.
pub const SIDEBAR_WIDTH: f32 = 256.0;
/// Resize floor for a sidebar (180 logical px) — below this, file/label content truncates badly.
pub const SIDEBAR_WIDTH_MIN: f32 = 180.0;
/// Resize ceiling for a sidebar (480 logical px) — above this, a sidebar stops being a sidebar.
pub const SIDEBAR_WIDTH_MAX: f32 = 480.0;

// ---- Control sizing ----
//
// Shared row height for input-tier controls — buttons, selects, text
// inputs, tabs, pagination, command-palette rows. Form layouts depend
// on these aligning across widget kinds, so the value lives at the
// token tier rather than each widget hardcoding 32.0. Use this when
// sizing a parent container that has to fit a control row, or when
// composing a new control-shaped widget.
//
// Matches Tailwind/shadcn `h-8` (32 px) — the default for `Button`,
// `Input`, `Select`, etc. Square icon-only controls (icon button,
// pagination cell) use this as both width and height.
/// Shared row height for input-tier controls (32 logical px) — Tailwind/shadcn `h-8`.
pub const CONTROL_HEIGHT: f32 = 32.0;

// ---- Radius ----
/// 4 px corner radius — dense inner elements (menu items, tooltips, list rows).
pub const RADIUS_SM: f32 = 4.0;
/// 8 px corner radius — the standard control and popup radius.
pub const RADIUS_MD: f32 = 8.0;
/// 12 px corner radius — large surfaces (cards, dialogs).
pub const RADIUS_LG: f32 = 12.0;
/// Effectively-infinite radius (999 px) — yields pill / fully-rounded shapes.
pub const RADIUS_PILL: f32 = 999.0;

// ---- Scrollbar thumb (overlay indicator on scrollable viewports) ----
/// Visible thumb width when idle. Kept thin so it doesn't crowd
/// content; the hitbox is wider so the thumb still feels grabbable
/// (Fitts's law).
pub const SCROLLBAR_THUMB_WIDTH: f32 = 6.0;
/// Visible thumb width while hovered or being dragged. The thumb
/// expands toward the viewport interior so the cursor sits inside
/// it instead of pinning the right edge.
pub const SCROLLBAR_THUMB_WIDTH_ACTIVE: f32 = 10.0;
/// Track / thumb hitbox width — the column on the right edge that
/// accepts pointer presses for thumb-grab and click-to-page. Always
/// wider than the visible thumb (idle or active) so grabbing a
/// thin idle thumb is still easy. Matches the shadcn ScrollArea
/// track width convention.
pub const SCROLLBAR_HITBOX_WIDTH: f32 = 14.0;
/// Gap (logical px) between the thumb and the viewport's right edge.
pub const SCROLLBAR_TRACK_INSET: f32 = 2.0;
/// Right-edge gutter that fully clears the *active* (widest) thumb —
/// what `El::scrollbar_gutter()` reserves so scroll content and focus
/// rings never sit under the thumb. The CSS `scrollbar-gutter: stable`
/// analogue, and the same value the `ScrollbarObscuresFocusable` lint
/// measures against.
pub const SCROLLBAR_GUTTER: f32 = SCROLLBAR_THUMB_WIDTH_ACTIVE + SCROLLBAR_TRACK_INSET;
/// Minimum thumb height (logical px) so the thumb stays grabbable on
/// very long content.
pub const SCROLLBAR_THUMB_MIN_H: f32 = 24.0;
/// Idle thumb fill — subtle on background/card.
pub const SCROLLBAR_THUMB_FILL: Color = Color::srgb_token("scrollbar-thumb", 113, 113, 122, 120);
/// Active (hovered or dragged) thumb fill — fully opaque accent.
pub const SCROLLBAR_THUMB_FILL_ACTIVE: Color =
    Color::srgb_token("scrollbar-thumb-active", 161, 161, 170, 220);

// ---- Shadow ----
//
// `El::shadow(level)` carries a scalar elevation level; the renderer
// expands it into a Tailwind-style two-layer drop shadow via
// [`crate::paint::shadow::shadow_recipe`]. The tokens sit exactly on
// Tailwind v4's box-shadow scale (see that module for the recipes);
// non-token levels interpolate between the anchors.
/// Extra-small elevation — buttons and other raised controls
/// (Tailwind `shadow-xs`: 0 1 2 / 0.05).
pub const SHADOW_XS: f32 = 2.0;
/// Small elevation — cards (Tailwind `shadow-sm`, two layers:
/// 0 1 3 / 0.10 + 0 1 2 −1 / 0.10).
pub const SHADOW_SM: f32 = 4.0;
/// Medium elevation — popovers, dropdowns, tooltips, toasts
/// (Tailwind `shadow-md`: 0 4 6 −1 / 0.10 + 0 2 4 −2 / 0.10).
pub const SHADOW_MD: f32 = 12.0;
/// Large elevation — modal dialogs and sheets (Tailwind `shadow-lg`:
/// 0 10 15 −3 / 0.10 + 0 4 6 −4 / 0.10).
pub const SHADOW_LG: f32 = 24.0;

// ---- Typography ----
//
// Font-size tokens are pairs, matching Tailwind's default type scale:
// a `text-sm` token is 14/20, `text-2xl` is 24/32, and so on. Text
// roles should choose one of these tokens rather than setting a raw
// font size and letting measurement infer a line height later.
/// A font-size / line-height pair from the type scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeToken {
    /// Font size in logical px.
    pub size: f32,
    /// Line height in logical px.
    pub line_height: f32,
}

/// 12/16 — Tailwind `text-xs`; the Caption and Code roles, dense cells.
pub const TEXT_XS: TypeToken = TypeToken {
    size: 12.0,
    line_height: 16.0,
};
/// 14/20 — Tailwind `text-sm`; the Body and Label roles' default size.
pub const TEXT_SM: TypeToken = TypeToken {
    size: 14.0,
    line_height: 20.0,
};
/// 16/24 — Tailwind `text-base`; the Title role.
pub const TEXT_BASE: TypeToken = TypeToken {
    size: 16.0,
    line_height: 24.0,
};
/// 18/28 — Tailwind `text-lg`.
pub const TEXT_LG: TypeToken = TypeToken {
    size: 18.0,
    line_height: 28.0,
};
/// 20/28 — Tailwind `text-xl`.
pub const TEXT_XL: TypeToken = TypeToken {
    size: 20.0,
    line_height: 28.0,
};
/// 24/32 — Tailwind `text-2xl`; the Heading role.
pub const TEXT_2XL: TypeToken = TypeToken {
    size: 24.0,
    line_height: 32.0,
};
/// 30/36 — Tailwind `text-3xl`; the Display role.
pub const TEXT_3XL: TypeToken = TypeToken {
    size: 30.0,
    line_height: 36.0,
};

/// Look up the type-scale token whose `size` matches exactly; `None`
/// when `size` is not on the scale.
pub fn type_token_for_size(size: f32) -> Option<TypeToken> {
    [
        TEXT_XS, TEXT_SM, TEXT_BASE, TEXT_LG, TEXT_XL, TEXT_2XL, TEXT_3XL,
    ]
    .into_iter()
    .find(|token| (token.size - size).abs() < f32::EPSILON)
}

/// Line height for a font size: the scale token's paired value when
/// `size` is on the scale, otherwise `ceil(size × 1.3)`. Used when a
/// raw font size is set without an explicit line height.
pub fn line_height_for_size(size: f32) -> f32 {
    type_token_for_size(size)
        .map(|token| token.line_height)
        .unwrap_or((size * 1.3).ceil())
}

// ---- Icons ----
//
// Common lucide/shadcn icon boxes. `ICON_SM` is Tailwind `size-4`;
// `ICON_XS` maps to the common `size-3.5` treatment used in dense
// cells and compact status rows.
/// 14 px icon box — the common `size-3.5` treatment for dense cells and compact status rows.
pub const ICON_XS: f32 = 14.0;
/// 16 px icon box — Tailwind `size-4`; the default inline/menu/sidebar icon size.
pub const ICON_SM: f32 = 16.0;
/// 20 px icon box — for prominent or standalone icons.
pub const ICON_MD: f32 = 20.0;
/// 24 px icon box — Tailwind `size-6`; toolbar/feature icons.
pub const ICON_LG: f32 = 24.0;
/// 40 px icon box — empty-state and hero icons that anchor a centered
/// "no results" / onboarding panel. Saves reaching for a magic number.
pub const ICON_XL: f32 = 40.0;

// ---- State styling ----
//
// Visual deltas applied when an element is in a non-default interaction
// state. Renderer consumes these.

/// How much to darken a fill on press, as a 0..1 factor.
pub const PRESS_DARKEN: f32 = 0.12;
/// How much to lighten a fill on hover, as a 0..1 factor.
pub const HOVER_LIGHTEN: f32 = 0.06;
/// Peak alpha contribution from a fully-eased hover envelope on a
/// surface with no resting fill (`.ghost()`, inactive tab triggers,
/// `.outline()`). Hover/press envelopes only modulate an existing
/// fill, so without a synthesized state-only fill these surfaces show
/// no interaction feedback. Mirrors the shadcn idiom
/// `hover:bg-accent active:bg-accent/80` — transparent at rest, a faint
/// raised surface fades in on interaction.
pub const STATE_FILL_HOVER_ALPHA: f32 = 0.40;
/// Additional peak alpha contribution from a fully-eased press
/// envelope. Sums with [`STATE_FILL_HOVER_ALPHA`] (clamped to 1.0) so
/// a press while hovered reads slightly more committed than hover
/// alone, but still quieter than the active/current treatment.
pub const STATE_FILL_PRESS_ALPHA: f32 = 0.25;
/// Opacity multiplier when an element is disabled.
pub const DISABLED_ALPHA: f32 = 0.5;
/// Ring outset (additional focus stroke beyond the element bounds).
pub const RING_WIDTH: f32 = 2.0;
/// Conservative default pointer hit-target outset for stock controls.
/// Kept below [`RING_WIDTH`] so the focus-ring gutter usually has
/// enough room for adjacent controls to expand without overlapping.
pub const HIT_OVERFLOW: f32 = RING_WIDTH * 0.5;
/// Minimum pointer-target size in logical pixels for interactive
/// nodes — the runtime auto-inflates the hit rect of any focusable
/// or selectable node whose painted size is smaller, splitting the
/// deficit symmetrically so smaller controls (icon buttons, dense
/// table cells) still meet the touch-ergonomic floor without
/// changing what is drawn.
///
/// 44 logical px is the iOS HIG minimum and a slight tightening of
/// Material's 48dp; both refer to the same 9-mm fingertip-pad
/// research and either would work. Apps that genuinely need a
/// smaller target can declare an explicit `hit_overflow` — auto-
/// inflation is additive, never subtractive, so the explicit
/// value is always honored as a floor on its own.
pub const MIN_TOUCH_TARGET: f32 = 44.0;
/// Background tint for selected text in `text_input` / `text_area`.
/// Tinted accent at low alpha so glyphs stay readable through the
/// selection rectangle.
pub const SELECTION_BG: Color = Color::srgb_token("selection-bg", 96, 165, 250, 96);
/// Selection-band fill applied while a text input lacks focus. A
/// neutral, low-saturation cousin of [`SELECTION_BG`]; the painter
/// lerps from this toward `SELECTION_BG` as the input regains focus
/// (see [`crate::tree::El::dim_fill`]). Matches the macOS convention
/// where unfocused selection reads as gray rather than blue.
pub const SELECTION_BG_UNFOCUSED: Color =
    Color::srgb_token("selection-bg-unfocused", 113, 113, 122, 64);
