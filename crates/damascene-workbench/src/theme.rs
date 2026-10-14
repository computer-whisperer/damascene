//! The workbench theme — palette, metrics, and fonts as one decision.
//!
//! # Two vocabularies, one set of pixels
//!
//! An app on this theme paints from two token sets at once:
//!
//! - **VS Code keys** ([`crate::tokens`]) used by this crate's chrome.
//! - **shadcn names** (`damascene_core::tokens`) used by every stock
//!   widget you drop in — `button`, `checkbox`, `text_input`, `sidebar`,
//!   `editor_tabs`.
//!
//! Unlike the retired Carbon experiment, both sets resolve through the
//! *same* [`Palette`], which also carries the workbench keys as extra
//! tokens (`Palette::with_token`), so a palette swap moves both. That is
//! what the open token namespace bought (see
//! `docs/VOCABULARY_PARITY.md` §2).
//!
//! # Which values — the keys are the naming layer, not the colors
//!
//! [`theme`], the default, paints from the stock
//! `Palette::radix_slate_blue_dark` (background `#111113`, card
//! `#18191B`, primary `#0090FF`), and
//! [`register_workbench_tokens_from_palette`] remaps every VS Code key
//! onto the slot that owns its semantic — `sideBar.background` → `card`,
//! `button.background` → `primary`, `focusBorder` → `ring`. The
//! vocabulary is VS Code's; the values are the palette's, so swapping
//! the palette moves the whole key set at once.
//!
//! Dark Modern's values survive verbatim as [`dark_modern`], built from
//! [`dark_modern_palette`] + [`register_workbench_tokens`]. It is the
//! calibrated reference (`references/vscode-calibration/`) and the
//! demonstration that the vocabulary rethemes —
//! `examples/slicer_match.rs` reskins the whole crate in ~30
//! `with_token` lines.
//!
//! Ratified 2026-07-27; the evidence and the rejected
//! Dark-Modern-as-default position are in `docs/WORKBENCH_VISION.md`,
//! §"Retarget ratified".
//!
//! # Dark only
//!
//! `docs/WORKBENCH_VISION.md` left "whether the crate ships light themes
//! at all" open. It does not, yet: there is no consumer asking for
//! light. When one does, the shape is
//! [`register_workbench_tokens_from_palette`] over
//! `Palette::radix_slate_blue_light` — the remap is slot-derived, so it
//! carries to a light palette unchanged.

#![warn(missing_docs)]

use damascene_core::Theme;
use damascene_core::metrics::ComponentSize;
use damascene_core::theme::palette::Palette;
use damascene_core::tree::FontFamily;

use crate::tokens as vs;

/// The multiplicative radius scale that squares the shadcn ladder down
/// to the workbench's ~0–4px band.
///
/// # The arithmetic
///
/// `Theme::with_radius_scale` multiplies every *theme-default* (not
/// author-specified) nonzero corner, including the output of
/// `control_metrics` — which is the ladder that actually rounds buttons,
/// icon buttons and inputs (`damascene-core/src/metrics.rs`):
///
/// | `ComponentSize` | stock control radius | × scale |
/// |---|---|---|
/// | `Xxs` | 4px | 1.14px |
/// | `Xs` | 5px | 1.43px |
/// | `Sm` | 6px | 1.71px |
/// | `Md` | 7px | **2.00px** |
/// | `Lg` | 8px | 2.29px |
///
/// Solving the `Md` row for [`vs::RADIUS`] (2px) gives the constant:
///
/// ```text
/// scale = RADIUS / control_radius(Md) = 2.0 / 7.0 = 0.285714…
/// ```
///
/// `Md` is the solve point because it is shadcn's own default rung — the
/// one a reader means by "the default control radius" — even though this
/// theme ships controls at `Xs`. Anchoring on the shared reference rung
/// keeps the number checkable against upstream shadcn rather than
/// against a local choice.
///
/// The container ladder rides along: `RADIUS_SM` 4px → 1.14px,
/// `RADIUS_MD` 8px → 2.29px, `RADIUS_LG` 12px → 3.43px, and the stock
/// badge's 6px → 1.71px. The whole system lands inside the 0–5px band
/// `docs/WORKBENCH_VISION.md` measured off real tools.
///
/// Two carve-outs in core apply and are wanted here: `RADIUS_PILL`
/// (999px) is exempt, so avatars and pill affordances stay round the way
/// `rounded-full` ignores `--radius` on the web; and an explicit
/// `.radius()` survives untouched, which is how [`crate::chrome::chip`]
/// pins itself at exactly 2px instead of inheriting a scaled value.
pub const RADIUS_SCALE: f32 = vs::RADIUS / 7.0;

/// Type scale: 13/14 — VS Code's 13px workbench UI font over
/// damascene's 14px `TEXT_SM` body/label baseline.
///
/// `Theme::with_type_scale` is the rem analogue: every role- and
/// rung-derived size scales together, line heights included, so the
/// whole ladder lands where VS Code's does — body/label 14 → 13px,
/// caption 12 → 11.1px (VS Code's small text runs ~11px), titles and
/// headings proportional. Hand-picked `.font_size(...)` values
/// survive, exactly as `text-[15px]` ignores a rem theme. Control
/// heights are deliberately independent — density is
/// `with_default_component_size`'s job.
pub const TYPE_SCALE: f32 = 13.0 / 14.0;

/// Dark Modern mapped onto the shadcn palette slots, plus every
/// workbench key registered as an extra token.
///
/// This is [`dark_modern`]'s palette — the *variant*, not the crate
/// default. [`theme`] paints from [`slate_palette`] (ratified
/// 2026-07-27; see the module docs). It keeps its original name because
/// downstream rethemes spread it (`..theme::dark_modern_palette()`), and because
/// everything below it is still true of Dark Modern.
///
/// # The slot mapping
///
/// | slot | VS Code key | value |
/// |---|---|---|
/// | `background` | `editor.background` | `#1F1F1F` |
/// | `foreground` | `foreground` | `#CCCCCC` |
/// | `card` | `sideBar.background` | `#181818` |
/// | `card-foreground` | `sideBar.foreground` | `#CCCCCC` |
/// | `popover` | `quickInput.background` | `#222222` |
/// | `popover-foreground` | `quickInput.foreground` | `#CCCCCC` |
/// | `primary` | `button.background` | `#0078D4` |
/// | `primary-foreground` | `button.foreground` | `#FFFFFF` |
/// | `secondary` | `button.secondaryHoverBackground` | `#2B2B2B` |
/// | `secondary-foreground` | `button.secondaryForeground` | `#CCCCCC` |
/// | `muted` | `list.hoverBackground` | `#2A2D2E` |
/// | `muted-foreground` | `descriptionForeground` | `#9D9D9D` |
/// | `accent` | `list.activeSelectionBackground` | `#04395E` |
/// | `accent-foreground` | `list.activeSelectionForeground` | `#FFFFFF` |
/// | `destructive` | `errorForeground` | `#F85149` |
/// | `destructive-foreground` | `button.foreground` | `#FFFFFF` |
/// | `border` | `panel.border` | `#2B2B2B` |
/// | `input` | `input.border` | `#3C3C3C` |
/// | `ring` | `focusBorder` | `#0078D4` |
/// | `success` | `editorGutter.addedBackground` | `#2EA043` |
/// | `success-foreground` | `button.foreground` | `#FFFFFF` |
/// | `warning` | `chat.editedFileForeground` | `#E2C08D` |
/// | `warning-foreground` | `editor.background` | `#1F1F1F` |
/// | `info` | `editorGutter.modifiedBackground` | `#0078D4` |
/// | `info-foreground` | `button.foreground` | `#FFFFFF` |
/// | `link-foreground` | `textLink.foreground` | `#4daafc` |
/// | `badge` | `badge.background` | `#616161` |
/// | `badge-foreground` | `badge.foreground` | `#F8F8F8` |
/// | `selection-bg` | `list.activeSelectionBackground` | `#04395E` |
/// | `selection-bg-unfocused` | `editor.inactiveSelectionBackground` | `#3A3D41` |
///
/// # The rows that are decisions rather than transcriptions
///
/// - **`card` = `sideBar.background`.** The load-bearing row. In the
///   stock dark palette `card == background`, so a card is visible only
///   as an outline and reads as a sheet floating on a canvas. Pointing
///   `card` at the side bar's *darker* value converts every stock
///   `card()`, `sidebar()` and `popover`-adjacent surface into a layered
///   workbench panel — diagnosis 3 of `docs/WORKBENCH_VISION.md`, fixed
///   by one row of a table.
/// - **`secondary` = `button.secondaryHoverBackground`.** VS Code's
///   `button.secondaryBackground` is literally `#00000000` — its
///   secondary button is a ghost that only acquires a fill on hover.
///   shadcn's `secondary` slot is a *rest* fill and is used for far more
///   than buttons, so transcribing the transparent value would make
///   secondary surfaces vanish. The hover value is the same color family
///   at the intended weight. The literal key is still available as
///   [`vs::BUTTON_SECONDARY_BG`].
/// - **`warning` = `chat.editedFileForeground`.** Dark Modern sets no
///   `*Warning.foreground` key at all. This is the theme's own amber for
///   "touched, look at it", and it is the only amber in the chain at a
///   brightness matching `destructive` (`#F85149`) and `success`
///   (`#2EA043`). Because it is a light tint rather than a saturated
///   fill, `warning-foreground` is dark (`editor.background`) where the
///   other status foregrounds are white.
/// - **`muted`/`accent` split.** The stock dark palette collapses
///   `secondary`/`muted`/`accent`/`border`/`input` onto one value. Here
///   `accent` is the *selected* list row and `muted` is the *hovered*
///   one, which is exactly the pair VS Code distinguishes and is what
///   makes list interaction legible.
///
/// # Slots left on the stock dark values
///
/// `overlay-scrim`, `scrollbar-thumb` and `scrollbar-thumb-active` come
/// through `..Palette::damascene_dark()`. No theme in the Dark Modern
/// chain sets a scrim or scrollbar key, and the calibration reference
/// deliberately vendors only the theme files — inventing values here
/// would be recalled color, which the calibration plan forbids.
pub fn dark_modern_palette() -> Palette {
    let p = Palette {
        background: vs::EDITOR_BG,
        foreground: vs::FOREGROUND,

        card: vs::SIDE_BAR_BG,
        card_foreground: vs::SIDE_BAR_FG,

        popover: vs::QUICK_INPUT_BG,
        popover_foreground: vs::QUICK_INPUT_FG,

        primary: vs::BUTTON_BG,
        primary_foreground: vs::BUTTON_FG,

        secondary: vs::BUTTON_SECONDARY_HOVER_BG,
        secondary_foreground: vs::BUTTON_SECONDARY_FG,

        muted: vs::LIST_HOVER_BG,
        muted_foreground: vs::DESCRIPTION_FG,

        accent: vs::LIST_ACTIVE_SELECTION_BG,
        accent_foreground: vs::LIST_ACTIVE_SELECTION_FG,

        destructive: vs::ERROR_FG,
        destructive_foreground: vs::BUTTON_FG,

        border: vs::PANEL_BORDER,
        input: vs::INPUT_BORDER,
        ring: vs::FOCUS_BORDER,

        success: vs::EDITOR_GUTTER_ADDED_BG,
        success_foreground: vs::BUTTON_FG,
        warning: vs::CHAT_EDITED_FILE_FG,
        warning_foreground: vs::EDITOR_BG,
        info: vs::EDITOR_GUTTER_MODIFIED_BG,
        info_foreground: vs::BUTTON_FG,

        link_foreground: vs::TEXT_LINK_FG,

        badge: vs::BADGE_BG,
        badge_foreground: vs::BADGE_FG,

        selection_bg: vs::LIST_ACTIVE_SELECTION_BG,
        selection_bg_unfocused: vs::EDITOR_INACTIVE_SELECTION_BG,

        ..Palette::damascene_dark()
    };
    register_workbench_tokens(p)
}

/// Register the workbench key set on a palette's extra namespace.
///
/// Split out from [`dark_modern_palette`] so a downstream *theme* of
/// the workbench vocabulary — eutectic's ui-oracle values, say — can
/// start from its own slot mapping and still resolve
/// `sideBar.background` and friends.
///
/// Keys whose value is a pure transcription of a slot are registered
/// anyway: the point of the namespace is that
/// `Color::srgb_token("sideBar.background", …)` follows a palette swap,
/// which requires the entry to exist even when it currently agrees with
/// `card`.
///
/// `foreground` is deliberately absent: it is a stock shadcn name, so
/// `Palette::lookup` would shadow the extra entry with `Palette::foreground`
/// before ever consulting the map. Registering it would be dead weight
/// that reads as if it worked.
///
/// # Retheming a single key
///
/// Registering the same extra name twice keeps the last color, so an app
/// recolors the workbench by chaining `with_token` *after* this call — no
/// forked constants, and every `vs::` color carrying that name re-points
/// at paint time:
///
/// ```ignore
/// use damascene_core::Color;
/// use damascene_core::theme::palette::Palette;
/// use damascene_workbench::theme;
///
/// // Blue side bar everywhere `vs::SIDE_BAR_BG` is painted — including
/// // the shadcn `card` slot, which maps to that same token.
/// let p: Palette = theme::register_workbench_tokens(Palette::damascene_dark())
///     .with_token("sideBar.background", Color::rgba(20, 30, 60, 255));
/// ```
///
/// The order matters only against this function; stock shadcn names
/// (`background`, `foreground`, `border`, …) still win over extras and
/// have to be set on the palette's own fields instead.
pub fn register_workbench_tokens(p: Palette) -> Palette {
    p
        // Editor / text
        .with_token("editor.background", vs::EDITOR_BG)
        .with_token("editor.foreground", vs::EDITOR_FG)
        .with_token("descriptionForeground", vs::DESCRIPTION_FG)
        .with_token("errorForeground", vs::ERROR_FG)
        .with_token("textLink.foreground", vs::TEXT_LINK_FG)
        // Side bar
        .with_token("sideBar.background", vs::SIDE_BAR_BG)
        .with_token("sideBar.foreground", vs::SIDE_BAR_FG)
        .with_token("sideBar.border", vs::SIDE_BAR_BORDER)
        .with_token("sideBarTitle.foreground", vs::SIDE_BAR_TITLE_FG)
        .with_token(
            "sideBarSectionHeader.background",
            vs::SIDE_BAR_SECTION_HEADER_BG,
        )
        .with_token(
            "sideBarSectionHeader.foreground",
            vs::SIDE_BAR_SECTION_HEADER_FG,
        )
        .with_token(
            "sideBarSectionHeader.border",
            vs::SIDE_BAR_SECTION_HEADER_BORDER,
        )
        // Bars
        .with_token("statusBar.background", vs::STATUS_BAR_BG)
        .with_token("statusBar.foreground", vs::STATUS_BAR_FG)
        .with_token("statusBar.border", vs::STATUS_BAR_BORDER)
        .with_token(
            "statusBarItem.hoverBackground",
            vs::STATUS_BAR_ITEM_HOVER_BG,
        )
        .with_token(
            "statusBarItem.remoteBackground",
            vs::STATUS_BAR_ITEM_REMOTE_BG,
        )
        .with_token(
            "statusBarItem.remoteForeground",
            vs::STATUS_BAR_ITEM_REMOTE_FG,
        )
        .with_token(
            "statusBarItem.prominentBackground",
            vs::STATUS_BAR_ITEM_PROMINENT_BG,
        )
        .with_token("titleBar.activeBackground", vs::TITLE_BAR_ACTIVE_BG)
        .with_token("titleBar.activeForeground", vs::TITLE_BAR_ACTIVE_FG)
        .with_token("titleBar.border", vs::TITLE_BAR_BORDER)
        .with_token("activityBar.background", vs::ACTIVITY_BAR_BG)
        .with_token("activityBar.foreground", vs::ACTIVITY_BAR_FG)
        .with_token(
            "activityBar.inactiveForeground",
            vs::ACTIVITY_BAR_INACTIVE_FG,
        )
        .with_token("activityBar.border", vs::ACTIVITY_BAR_BORDER)
        .with_token("panel.background", vs::PANEL_BG)
        .with_token("panel.border", vs::PANEL_BORDER)
        .with_token("panelTitle.activeForeground", vs::PANEL_TITLE_ACTIVE_FG)
        .with_token("panelTitle.inactiveForeground", vs::PANEL_TITLE_INACTIVE_FG)
        // Editor group header and tabs
        .with_token(
            "editorGroupHeader.tabsBackground",
            vs::EDITOR_GROUP_HEADER_TABS_BG,
        )
        .with_token(
            "editorGroupHeader.tabsBorder",
            vs::EDITOR_GROUP_HEADER_TABS_BORDER,
        )
        .with_token("editorGroup.border", vs::EDITOR_GROUP_BORDER)
        .with_token("tab.activeBackground", vs::TAB_ACTIVE_BG)
        .with_token("tab.activeForeground", vs::TAB_ACTIVE_FG)
        .with_token("tab.inactiveBackground", vs::TAB_INACTIVE_BG)
        .with_token("tab.inactiveForeground", vs::TAB_INACTIVE_FG)
        .with_token("tab.border", vs::TAB_BORDER)
        .with_token("tab.activeBorderTop", vs::TAB_ACTIVE_BORDER_TOP)
        // Controls
        .with_token("focusBorder", vs::FOCUS_BORDER)
        .with_token("button.background", vs::BUTTON_BG)
        .with_token("button.foreground", vs::BUTTON_FG)
        .with_token("button.hoverBackground", vs::BUTTON_HOVER_BG)
        .with_token("button.secondaryBackground", vs::BUTTON_SECONDARY_BG)
        .with_token("button.secondaryForeground", vs::BUTTON_SECONDARY_FG)
        .with_token(
            "button.secondaryHoverBackground",
            vs::BUTTON_SECONDARY_HOVER_BG,
        )
        .with_token("input.background", vs::INPUT_BG)
        .with_token("input.border", vs::INPUT_BORDER)
        .with_token("input.foreground", vs::INPUT_FG)
        .with_token("input.placeholderForeground", vs::INPUT_PLACEHOLDER_FG)
        .with_token("dropdown.background", vs::DROPDOWN_BG)
        .with_token("dropdown.border", vs::DROPDOWN_BORDER)
        .with_token("dropdown.listBackground", vs::DROPDOWN_LIST_BG)
        .with_token("checkbox.background", vs::CHECKBOX_BG)
        .with_token("checkbox.border", vs::CHECKBOX_BORDER)
        .with_token("progressBar.background", vs::PROGRESS_BAR_BG)
        // Floating surfaces
        .with_token("quickInput.background", vs::QUICK_INPUT_BG)
        .with_token("quickInput.foreground", vs::QUICK_INPUT_FG)
        .with_token("editorWidget.background", vs::EDITOR_WIDGET_BG)
        .with_token("widget.border", vs::WIDGET_BORDER)
        .with_token("menu.background", vs::MENU_BG)
        .with_token("menu.selectionBackground", vs::MENU_SELECTION_BG)
        .with_token("pickerGroup.border", vs::PICKER_GROUP_BORDER)
        // Badge and list
        .with_token("badge.background", vs::BADGE_BG)
        .with_token("badge.foreground", vs::BADGE_FG)
        .with_token(
            "list.activeSelectionBackground",
            vs::LIST_ACTIVE_SELECTION_BG,
        )
        .with_token(
            "list.activeSelectionForeground",
            vs::LIST_ACTIVE_SELECTION_FG,
        )
        .with_token(
            "list.inactiveSelectionBackground",
            vs::LIST_INACTIVE_SELECTION_BG,
        )
        .with_token("list.hoverBackground", vs::LIST_HOVER_BG)
        // Status colors and selection
        .with_token("editorGutter.addedBackground", vs::EDITOR_GUTTER_ADDED_BG)
        .with_token(
            "editorGutter.deletedBackground",
            vs::EDITOR_GUTTER_DELETED_BG,
        )
        .with_token(
            "editorGutter.modifiedBackground",
            vs::EDITOR_GUTTER_MODIFIED_BG,
        )
        .with_token("chat.editedFileForeground", vs::CHAT_EDITED_FILE_FG)
        .with_token(
            "editor.inactiveSelectionBackground",
            vs::EDITOR_INACTIVE_SELECTION_BG,
        )
}

/// The palette [`theme`] paints from: the stock
/// `Palette::radix_slate_blue_dark`, with the workbench key vocabulary
/// remapped onto its slots by
/// [`register_workbench_tokens_from_palette`].
///
/// No slot is overridden. The whole point of the 2026-07-27 retarget is
/// that the values are the stock palette's — background `#111113`, card
/// `#18191B`, primary `#0090FF` — and the VS Code keys name them rather
/// than supply them.
pub fn slate_palette() -> Palette {
    register_workbench_tokens_from_palette(Palette::radix_slate_blue_dark())
}

/// Register the workbench key vocabulary on `p`, deriving every value
/// from `p`'s **own shadcn slots**.
///
/// This is the retarget's mechanism ([`slate_palette`] is one call to
/// it): each VS Code key gets the slot that owns its semantic, so a
/// palette swap moves the entire key set at once — including to a light
/// palette. Contrast [`register_workbench_tokens`], which registers Dark
/// Modern's literal values.
///
/// # The remap
///
/// | VS Code keys | slot |
/// |---|---|
/// | `editor.background`, `tab.activeBackground` | `background` |
/// | `sideBar.background`, `sideBarSectionHeader.background`, `statusBar.background`, `titleBar.activeBackground`, `activityBar.background`, `panel.background`, `editorGroupHeader.tabsBackground`, `tab.inactiveBackground` | `card` |
/// | `sideBar.foreground` | `card-foreground` |
/// | `quickInput.background`, `editorWidget.background`, `menu.background`, `dropdown.listBackground` | `popover` |
/// | `quickInput.foreground` | `popover-foreground` |
/// | `editor.foreground`, `input.foreground`, `tab.activeForeground`, `titleBar.activeForeground`, `activityBar.foreground`, `panelTitle.activeForeground`, `statusBarItem.hoverBackground` (a wash — see below) | `foreground` |
/// | `descriptionForeground`, `input.placeholderForeground`, `statusBarItem.prominentBackground` (a wash), and every dimmed label (`sideBarTitle`, `sideBarSectionHeader`, `statusBar`, `tab.inactive*`, `activityBar.inactive*`, `panelTitle.inactive*`) | `muted-foreground` |
/// | `button.background`, `progressBar.background`, `tab.activeBorderTop`, `statusBarItem.remoteBackground`, `menu.selectionBackground` | `primary` |
/// | `button.foreground`, `statusBarItem.remoteForeground` | `primary-foreground` |
/// | `button.secondaryBackground`, `button.secondaryHoverBackground` | `secondary` |
/// | `button.secondaryForeground` | `secondary-foreground` |
/// | `badge.background` | `badge` |
/// | `badge.foreground` | `badge-foreground` |
/// | `list.hoverBackground`, `editor.inactiveSelectionBackground`, `list.inactiveSelectionBackground` | `muted` |
/// | `list.activeSelectionBackground` | `accent` |
/// | `list.activeSelectionForeground` | `accent-foreground` |
/// | `button.hoverBackground` | `primary` darkened 0.12 |
/// | every chrome `*.border` | `border` |
/// | `input.border`, `dropdown.border`, `checkbox.border`, `widget.border` | `input` |
/// | `input.background`, `dropdown.background`, `checkbox.background` | `muted` darkened 0.08 |
/// | `focusBorder` | `ring` |
/// | `errorForeground` | `destructive` |
/// | `editorGutter.addedBackground` | `success` |
/// | `editorGutter.deletedBackground` | `destructive` |
/// | `editorGutter.modifiedBackground` | `info` |
/// | `chat.editedFileForeground` | `warning` |
/// | `textLink.foreground` | `link-foreground` |
///
/// # The rows that are decisions rather than table lookups
///
/// - **The surface model inverts.** VS Code sinks chrome *below* the
///   editor (`#181818` under `#1F1F1F`); the shadcn genre raises chrome
///   *above* content (`card` over `background`). So every chrome ground
///   — side bar, status bar, title bar, activity bar, panel, tab strip —
///   takes `card`, and `editor.background` takes `background`.
///   `tab.activeBackground` follows the *content*, not the strip: it is
///   `background`, keeping the active tab continuous with the well below
///   it exactly as Dark Modern's `tab.activeBackground == editor.background`
///   does. `docs/WORKBENCH_VISION.md`, §"Retarget ratified".
/// - **`input.background` is `muted` darkened 0.08, not a slot.** That
///   is `SurfaceRole::Input`'s own trough derivation
///   (`damascene-core/src/theme/mod.rs`), so the key names what a stock
///   `text_input`, `select` or `text_area` actually renders instead of
///   sitting one step off it. `dropdown`/`checkbox` share the trough
///   because they are the same role.
/// - **Control borders take `input`, chrome borders take `border`.** The
///   split is semantic and currently invisible — the slate palette gives
///   both slots `#363A3F` — but it survives a palette that separates
///   them, which is what VS Code's `input.border` ≠ `panel.border` meant.
///   `widget.border` joins the control group because
///   `SurfaceRole::Popover` strokes with `tokens::INPUT`.
/// - **`list.activeSelectionBackground` is `accent`, not
///   `selection-bg`.** `selection-bg` is the slot that *owns* the
///   semantic, and it was the ratified assignment — but it is designed
///   **translucent** (`#0090FF` at 38%), and `Palette::resolve` takes
///   alpha from the requesting color rather than from the palette entry.
///   An opaque key pointed at it therefore paints full-strength
///   `#0090FF`: measured by rendering it, the selected row in
///   `examples/parts.rs` comes out a saturated blue band (probed
///   `#0090FF`) carrying `foreground` text at 2.8:1 — under AA — and
///   rows that keep their own muted paint under the band, like
///   `examples/voice.rs`'s chevron and count chip, land near 1.6:1. It
///   also spends the sparse accent the genre finding is about on every
///   selected row. `accent` + `accent-foreground` (`#0D2847` /
///   `#70B8FF`, 7.1:1) is shadcn's own selected-row pair and is what
///   `SurfaceRole::Current` paints, so a stock "current item" and a
///   hand-painted selected row agree.
///   `list.inactiveSelectionBackground` and
///   `editor.inactiveSelectionBackground` take `muted` for the same
///   reason — `selection-bg-unfocused` is a translucent grey whose
///   opaque form (`#696E77`) would read as a light band over the whole
///   row. Both translucent slots stay reachable under their stock names
///   for the widgets that respect their alpha (`text_input`'s selection).
/// - **The four alpha-bearing keys are washes, so their slot must be
///   *light*.** `statusBarItem.hoverBackground` (`#F1F1F133`) and
///   `statusBarItem.prominentBackground` (`#6E768166`) keep their baked
///   alpha through `resolve`, so pointing them at a near-background slot
///   would erase them; they take `foreground` and `muted-foreground`,
///   the light values whose 20%/40% wash over `card` reproduces what VS
///   Code's greys do over `#181818`. `editorGroup.border`
///   (`#FFFFFF17`) stays on `border` — it is a border key and VS Code's
///   own value is a whisper — and `button.secondaryBackground`
///   (`#00000000`) stays invisible on `secondary`, both as documented in
///   [`crate::tokens`].
/// - **`badge.background` is its own slot now, not `secondary`.** It
///   was `secondary` through 2026-07-27, and the acceptance round
///   caught what that costs: `secondary` is a *near-surface* value
///   (`#212225` against a `#18191B` card), so a
///   [`crate::chrome::chip`] on a [`crate::chrome::pane_header`]
///   measured **1.11:1** — the chip was invisible and only its label
///   read. VS Code's own `badge.background` is the opposite kind of
///   value: `#616161` stands 2.9:1 *above* its `#181818` chrome,
///   because a count chip is an object on the panel. Core now carries a
///   dedicated `badge` slot for exactly that material (the stock slate
///   ramp spends slate-9 on it, 3.4:1 over `card`), and this key points
///   at it. `damascene_core::tokens::BADGE` and `vs::BADGE_BG` are
///   consequently one value under both themes, the same way
///   `tokens::BORDER` and `vs::PANEL_BORDER` are.
/// - **Dimmed labels are a judgment call per key.** Dark Modern paints
///   `sideBarTitle.foreground`, `sideBarSectionHeader.foreground` and
///   `statusBar.foreground` at full `#CCCCCC`; here they take
///   `muted-foreground`. All three render as small uppercase
///   instrumentation in [`crate::chrome`], the genre dims them, and
///   `muted-foreground` (`#B0B4BA`) is still ~8:1 on `card`. The
///   keys VS Code itself dims (`tab.inactiveForeground`,
///   `activityBar.inactiveForeground`, `panelTitle.inactiveForeground`)
///   take the same slot, so the two groups do not collapse into
///   different values for the same visual weight.
pub fn register_workbench_tokens_from_palette(p: Palette) -> Palette {
    // Snapshot the slots: `with_token` consumes the palette.
    let background = p.background;
    let foreground = p.foreground;
    let card = p.card;
    let card_foreground = p.card_foreground;
    let popover = p.popover;
    let popover_foreground = p.popover_foreground;
    let primary = p.primary;
    let primary_foreground = p.primary_foreground;
    let secondary = p.secondary;
    let secondary_foreground = p.secondary_foreground;
    let muted = p.muted;
    let muted_foreground = p.muted_foreground;
    let accent = p.accent;
    let accent_foreground = p.accent_foreground;
    let destructive = p.destructive;
    let border = p.border;
    let input = p.input;
    let ring = p.ring;
    let success = p.success;
    let warning = p.warning;
    let info = p.info;
    let link_foreground = p.link_foreground;
    let badge = p.badge;
    let badge_foreground = p.badge_foreground;
    // The input trough, mirroring `SurfaceRole::Input` verbatim —
    // `palette.resolve(tokens::MUTED).darken(0.08)` — so the key and the
    // rendered control cannot drift.
    let trough = p.resolve(damascene_core::tokens::MUTED).darken(0.08);

    p
        // Editor / text
        .with_token("editor.background", background)
        .with_token("editor.foreground", foreground)
        .with_token("descriptionForeground", muted_foreground)
        .with_token("errorForeground", destructive)
        .with_token("textLink.foreground", link_foreground)
        // Side bar — chrome, so `card`
        .with_token("sideBar.background", card)
        .with_token("sideBar.foreground", card_foreground)
        .with_token("sideBar.border", border)
        .with_token("sideBarTitle.foreground", muted_foreground)
        .with_token("sideBarSectionHeader.background", card)
        .with_token("sideBarSectionHeader.foreground", muted_foreground)
        .with_token("sideBarSectionHeader.border", border)
        // Bars
        .with_token("statusBar.background", card)
        .with_token("statusBar.foreground", muted_foreground)
        .with_token("statusBar.border", border)
        // A 20% wash: needs a light rgb to lighten the bar (see docs).
        .with_token("statusBarItem.hoverBackground", foreground)
        .with_token("statusBarItem.remoteBackground", primary)
        .with_token("statusBarItem.remoteForeground", primary_foreground)
        // A 40% wash, same reasoning one step dimmer.
        .with_token("statusBarItem.prominentBackground", muted_foreground)
        .with_token("titleBar.activeBackground", card)
        .with_token("titleBar.activeForeground", foreground)
        .with_token("titleBar.border", border)
        .with_token("activityBar.background", card)
        .with_token("activityBar.foreground", foreground)
        .with_token("activityBar.inactiveForeground", muted_foreground)
        .with_token("activityBar.border", border)
        .with_token("panel.background", card)
        .with_token("panel.border", border)
        .with_token("panelTitle.activeForeground", foreground)
        .with_token("panelTitle.inactiveForeground", muted_foreground)
        // Editor group header and tabs
        .with_token("editorGroupHeader.tabsBackground", card)
        .with_token("editorGroupHeader.tabsBorder", border)
        .with_token("editorGroup.border", border)
        // The active tab is continuous with the content well, not with
        // the strip — the one place the inverted model still points a
        // chrome key at `background`.
        .with_token("tab.activeBackground", background)
        .with_token("tab.activeForeground", foreground)
        .with_token("tab.inactiveBackground", card)
        .with_token("tab.inactiveForeground", muted_foreground)
        .with_token("tab.border", border)
        .with_token("tab.activeBorderTop", primary)
        // Controls
        .with_token("focusBorder", ring)
        .with_token("button.background", primary)
        .with_token("button.foreground", primary_foreground)
        // No hover slot exists in the shadcn vocabulary; the darken
        // matches the direction of Dark Modern's `#026EC1` under
        // `#0078D4`.
        .with_token("button.hoverBackground", primary.darken(0.12))
        .with_token("button.secondaryBackground", secondary)
        .with_token("button.secondaryForeground", secondary_foreground)
        .with_token("button.secondaryHoverBackground", secondary)
        .with_token("input.background", trough)
        .with_token("input.border", input)
        .with_token("input.foreground", foreground)
        .with_token("input.placeholderForeground", muted_foreground)
        .with_token("dropdown.background", trough)
        .with_token("dropdown.border", input)
        .with_token("dropdown.listBackground", popover)
        .with_token("checkbox.background", trough)
        .with_token("checkbox.border", input)
        .with_token("progressBar.background", primary)
        // Floating surfaces
        .with_token("quickInput.background", popover)
        .with_token("quickInput.foreground", popover_foreground)
        .with_token("editorWidget.background", popover)
        .with_token("widget.border", input)
        .with_token("menu.background", popover)
        .with_token("menu.selectionBackground", primary)
        .with_token("pickerGroup.border", border)
        // Badge and list
        .with_token("badge.background", badge)
        .with_token("badge.foreground", badge_foreground)
        .with_token("list.activeSelectionBackground", accent)
        .with_token("list.activeSelectionForeground", accent_foreground)
        .with_token("list.inactiveSelectionBackground", muted)
        .with_token("list.hoverBackground", muted)
        // Status colors and selection
        .with_token("editorGutter.addedBackground", success)
        .with_token("editorGutter.deletedBackground", destructive)
        .with_token("editorGutter.modifiedBackground", info)
        .with_token("chat.editedFileForeground", warning)
        .with_token("editor.inactiveSelectionBackground", muted)
}

/// The workbench density profile over `palette` — the half of the theme
/// that is *not* color.
///
/// Shared by [`theme`] and [`dark_modern`] so the two differ in exactly
/// one thing: their palette.
fn profile(palette: Palette) -> Theme {
    use damascene_core::shader::UniformValue;
    use damascene_core::tree::SurfaceRole;

    Theme::default()
        .with_palette(palette)
        .with_default_component_size(ComponentSize::Xs)
        .with_radius_scale(RADIUS_SCALE)
        .with_shadow_scale(0.0)
        .with_type_scale(TYPE_SCALE)
        // Flat app, shadowed overlays: the scaled-away role default is
        // omitted rather than zeroed, exactly so this re-elevation can
        // land (see `Theme::with_shadow_scale`). One tier for the whole
        // popover family — VS Code's single `widget.shadow` — rather
        // than shadcn's md/lg split.
        .with_role_uniform(
            SurfaceRole::Popover,
            "shadow",
            UniformValue::F32(damascene_core::tokens::SHADOW_MD),
        )
        .with_font_family(FontFamily::Inter)
        .with_mono_font_family(FontFamily::JetBrainsMono)
}

/// **The workbench theme.** [`slate_palette`] — slate + blue with the VS
/// Code keys remapped onto its slots — over `Xs` controls,
/// [`RADIUS_SCALE`], flat chrome with shadowed overlays, Inter +
/// JetBrains Mono.
///
/// Ratified 2026-07-27 (`docs/WORKBENCH_VISION.md`, §"Retarget
/// ratified"): the crate's payload is the VS Code *key vocabulary*, the
/// density profile and the chrome recipes; only the values were ever
/// VS-Code-specific, and they now come from the stock palette. For Dark
/// Modern's values, use [`dark_modern`] — same profile, same keys, the
/// calibrated colors.
///
/// The four structural inversions from `docs/WORKBENCH_VISION.md`,
/// and where each one lives:
///
/// 1. **Control scale** — `with_default_component_size(Xs)` puts stock
///    buttons and inputs on the 28px rung instead of shadcn's 36px `Md`,
///    and [`TYPE_SCALE`] takes the whole type ladder to VS Code's 13px
///    baseline (body/label 13px, captions ~11px).
/// 2. **Radius + shadow** — [`RADIUS_SCALE`] takes every theme-default
///    corner, controls included, down to the 1–3.5px band, and
///    `with_shadow_scale(0.0)` flattens every recipe shadow: cards and
///    buttons stop floating. Overlays are re-elevated selectively — a
///    `Popover`-role uniform restores `SHADOW_MD` on menus, tooltips,
///    dialogs, and palettes, matching VS Code's shadowed
///    `widget.shadow` on its otherwise flat chrome.
/// 3. **Layered surfaces** — the chrome-grounds-take-`card` rows of
///    [`register_workbench_tokens_from_palette`], plus
///    [`crate::chrome`]'s hairline separators. The *direction* is the
///    retarget's one visible break with VS Code: chrome now rises above
///    the content well (`#18191B` over `#111113`) where Dark Modern
///    sinks it below (`#181818` under `#1F1F1F`).
/// 4. **Whitespace** — the crate's chrome recipes; the stock container
///    paddings are unchanged, because damascene deliberately has no
///    density knob (`docs/VOCABULARY_PARITY.md`, "Rejected").
///
/// Inversions 1, 2 and 4 are density, not color: they are unchanged by
/// the retarget and shared with [`dark_modern`] through the same
/// builder.
///
/// Fonts follow the stock configuration — Inter for UI, JetBrains Mono
/// for code — which is also what a default `Theme` already carries. Both
/// are set explicitly so the theme states its whole intent in one place
/// rather than inheriting half of it.
pub fn theme() -> Theme {
    profile(slate_palette())
}

/// The workbench profile painted in **VS Code Dark Modern** —
/// [`dark_modern_palette`] and [`register_workbench_tokens`], i.e. every
/// key on its calibrated
/// upstream value (`references/vscode-calibration/`).
///
/// This was the crate's `theme()` through 2026-07-27 and is byte-identical
/// to it: same density profile, same key set, Dark Modern's colors. It
/// stays because the calibration is paid for and proven, and because it
/// is the reference a "make it look like VS Code" request means
/// literally.
///
/// Note that it keeps VS Code's *sunken* chrome model (`sideBar.background`
/// darker than `editor.background`), where [`theme`] raises chrome above
/// the content well.
pub fn dark_modern() -> Theme {
    profile(dark_modern_palette())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: damascene_core::tree::Color) -> (f32, f32, f32) {
        (c.r, c.g, c.b)
    }

    /// WCAG 2.x relative luminance of an opaque sRGB color.
    fn luminance(c: damascene_core::tree::Color) -> f32 {
        let [r, g, b, _] = c.to_srgb_u8a();
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    /// WCAG 2.x contrast ratio between two opaque sRGB colors.
    fn contrast(a: damascene_core::tree::Color, b: damascene_core::tree::Color) -> f32 {
        let (mut hi, mut lo) = (luminance(a), luminance(b));
        if hi < lo {
            std::mem::swap(&mut hi, &mut lo);
        }
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn dark_modern_records_the_status_pairs_the_transcription_costs() {
        // Core holds every stock palette's solid status pairs to
        // 4.5:1 (`status_foregrounds_clear_the_small_text_floor`).
        // Dark Modern cannot join them: `success-foreground`,
        // `destructive-foreground` and `info-foreground` are all
        // `button.foreground` (`#FFFFFF`), transcribed, and VS Code's
        // own `#F85149` / `#2EA043` fills sit in the band where white
        // reads as legible and measures under AA. Inventing a dark
        // label here is exactly what `docs/WORKBENCH_VISION.md`'s
        // calibration plan forbids, so the reference wins and this
        // test records by how much — the precedent the chip material
        // set in `chrome::tests::palettes`.
        //
        // The exception is scoped to the two pairs that miss. If a
        // future edit moves any *other* pair below the core floor, or
        // moves these two further down, this test fails.
        let p = dark_modern_palette();
        let measured = |fill, fg| (contrast(fill, fg) * 100.0).round() / 100.0;

        assert_eq!(measured(p.destructive, p.destructive_foreground), 3.35);
        assert_eq!(measured(p.success, p.success_foreground), 3.37);

        for (role, fill, fg) in [
            ("primary", p.primary, p.primary_foreground),
            ("warning", p.warning, p.warning_foreground),
            ("info", p.info, p.info_foreground),
        ] {
            let ratio = contrast(fill, fg);
            assert!(
                ratio >= 4.5,
                "{role}: Dark Modern clears the core floor here at \
                 {ratio:.2}:1 — only destructive/success are excused"
            );
        }
    }

    #[test]
    fn slate_palette_inherits_the_retuned_status_labels() {
        // The default theme paints the stock slate palette, so the
        // core retune must reach it rather than stop at the crate
        // boundary — and `button.foreground` / `statusBarItem.
        // remoteForeground`, which the remap derives from
        // `primary-foreground`, must move with it.
        let p = slate_palette();
        let stock = Palette::radix_slate_blue_dark();
        assert_eq!(rgb(p.primary_foreground), rgb(stock.primary_foreground));
        assert!(
            contrast(p.primary, p.primary_foreground) >= 4.5,
            "the default theme's primary button label must clear AA"
        );
        assert_eq!(
            rgb(p.lookup("button.foreground").expect("key is registered")),
            rgb(stock.primary_foreground)
        );
    }

    #[test]
    fn card_is_the_side_bar_not_the_background() {
        // The load-bearing row: this is what converts floating cards
        // into layered panels. If it ever collapses back to
        // `background`, the crate's reason to exist is gone.
        let p = dark_modern_palette();
        assert_ne!(rgb(p.card), rgb(p.background));
        assert_eq!(rgb(p.card), rgb(vs::SIDE_BAR_BG));
    }

    #[test]
    fn docked_surfaces_sink_and_floating_surfaces_rise() {
        let p = dark_modern_palette();
        assert!(p.card.r < p.background.r, "side bar sinks below the editor");
        assert!(
            p.popover.r > p.background.r,
            "quick input rises above the editor"
        );
    }

    #[test]
    fn muted_accent_border_and_input_are_not_collapsed() {
        // The stock dark palette maps secondary/muted/accent/border/input
        // onto one value; VS Code distinguishes all of them.
        let p = dark_modern_palette();
        assert_ne!(rgb(p.accent), rgb(p.muted));
        assert_ne!(rgb(p.border), rgb(p.input));
        assert_ne!(rgb(p.secondary), rgb(p.muted));
    }

    #[test]
    fn secondary_is_not_the_transparent_literal() {
        // `button.secondaryBackground` is `#00000000`. Transcribing it
        // would make every secondary surface invisible.
        let p = dark_modern_palette();
        assert!(p.secondary.a > 0.0);
        assert_eq!(rgb(p.secondary), rgb(vs::BUTTON_SECONDARY_HOVER_BG));
    }

    #[test]
    fn workbench_keys_resolve_through_the_palette() {
        // The whole point of the open token namespace: a VS Code key
        // minted with `srgb_token` must come back from `lookup`, not
        // fall through to its baked literal.
        let p = dark_modern_palette();
        for key in [
            "sideBar.background",
            "statusBar.background",
            "panel.border",
            "focusBorder",
            "button.background",
            "quickInput.background",
            "badge.background",
            "list.activeSelectionBackground",
            "list.hoverBackground",
            "editorGroupHeader.tabsBackground",
            "tab.activeBackground",
            "titleBar.activeBackground",
        ] {
            assert!(p.lookup(key).is_some(), "{key} must resolve");
        }
        assert_eq!(
            rgb(p.lookup("sideBar.background").unwrap()),
            rgb(vs::SIDE_BAR_BG)
        );
        assert_eq!(
            rgb(p.lookup("badge.background").unwrap()),
            rgb(vs::BADGE_BG)
        );
    }

    #[test]
    fn every_workbench_token_resolves_to_itself() {
        // Guards the registration list against drift: any constant in
        // `tokens` whose key is registered must round-trip to its own
        // value. `foreground` is the documented exception — a stock
        // shadcn name that `lookup` shadows before reaching the map.
        let p = dark_modern_palette();
        for c in crate::tokens::ALL {
            let key = c.token.expect("workbench tokens carry their key");
            let resolved = p.lookup(key).unwrap_or_else(|| panic!("{key} unresolved"));
            if key == "foreground" {
                // Shadowed by `Palette::foreground` — same value here,
                // by construction of the slot mapping.
                assert_eq!(rgb(resolved), rgb(p.foreground));
            } else {
                assert_eq!(
                    rgb(resolved),
                    rgb(*c),
                    "{key} resolved to a different value"
                );
            }
        }
    }

    #[test]
    fn stock_border_token_resolves_to_the_panel_hairline() {
        // Chrome that paints `damascene_core::tokens::BORDER` — every
        // stock widget, and `chrome::hairline` — must land on VS Code's
        // hairline color under this theme.
        let p = dark_modern_palette();
        assert_eq!(
            rgb(p.lookup("border").unwrap()),
            rgb(vs::PANEL_BORDER),
            "the stock `border` token must resolve to panel.border"
        );
    }

    #[test]
    fn radius_scale_lands_a_default_md_control_on_the_workbench_radius() {
        // The documented solve: scale = RADIUS / control_radius(Md).
        // `control_metrics` is private, so the ladder value is restated
        // here; if core's Md control radius ever moves off 7px this
        // assertion is the tripwire, via the rendered-radius test below.
        let md_control_radius = 7.0_f32;
        assert!((RADIUS_SCALE * md_control_radius - vs::RADIUS).abs() < 1e-4);
    }

    #[test]
    fn radius_scale_squares_a_stock_button_through_the_metrics_pass() {
        // End-to-end rather than arithmetic-only: the metrics pass is
        // `pub(crate)`, so drive it the way a host does — through the
        // bundle renderer, which applies the theme to the tree in place.
        use damascene_core::bundle::artifact::render_bundle_themed;
        use damascene_core::tree::Rect;
        use damascene_core::widgets::button::button;

        let t = theme();
        let mut el = button("Save");
        render_bundle_themed(&mut el, Rect::new(0.0, 0.0, 200.0, 60.0), &t);
        assert!(
            el.radius.tl > 0.0 && el.radius.tl < 2.0,
            "an Xs button (5px stock) must land in the workbench radius band, got {}",
            el.radius.tl
        );
        // 28px, not shadcn's 36px `Md` — inversion 1.
        assert_eq!(el.height, damascene_core::tree::Size::Fixed(28.0));
    }

    #[test]
    fn a_stock_table_lands_on_the_reference_row_pitch_with_no_app_code() {
        // Inversion 1 reaching the data view. `references/workbench-
        // validation/parts/index.html` declares its parts-index rows as
        // `tbody tr { height: 28px }` and renders them at a 30px pitch
        // (rules measured off `reference.png` at y = 164, 194, 224, 254,
        // 284). Before cell padding joined the ComponentSize ladder a
        // stock table under this theme measured 35.57px — which is why
        // both `examples/parts.rs` and `examples/parts_v2.rs` used to
        // carry a local `ROW_PAD_Y = 5.0` and re-pad every cell.
        //
        // Nothing here builds a cell by hand: `theme()`'s `Xs` rung is
        // the whole configuration.
        use damascene_core::bundle::artifact::render_bundle_themed;
        use damascene_core::tree::Rect;
        use damascene_core::widgets::table::{
            TableColumn, table, table_body, table_header, table_header_cells, table_row_cells,
        };
        use damascene_core::{column, text};

        const COLS: &[TableColumn] = &[
            TableColumn::fill(1.0),
            TableColumn::fill(1.0),
            TableColumn::fill(0.6).align_end(),
        ];
        let mut root = column([table([
            table_header([table_header_cells(COLS, ["Reference", "Value", "Stock"])]),
            table_body((0..5).map(|i| {
                table_row_cells(
                    format!("row:{i}"),
                    COLS,
                    [
                        text(format!("RC0603FR-071{i}KL")).mono(),
                        text("10 kΩ 1%"),
                        text("4,812").tabular_numerals(),
                    ],
                )
            })),
        ])]);
        render_bundle_themed(&mut root, Rect::new(0.0, 0.0, 960.0, 600.0), &theme());

        let body = &root.children[0].children[1];
        let pitch = body.children[1].computed_rect.y - body.children[0].computed_rect.y;
        // Last row: no `border_b`, so this is the content box alone.
        let content = body.children.last().unwrap().computed_rect.h;
        assert_eq!(content, 28.0, "the reference's declared `tbody tr` height");
        assert_eq!(pitch, 29.0, "one px under the reference's rendered 30");
        // The cells the rung stamped, against the hand-tightened
        // `Sides::xy(SPACE_2, 5.0)` the validation apps converged on.
        let cell = &body.children[0].children[0];
        assert_eq!(cell.padding.left, damascene_core::tokens::SPACE_2);
        assert!(
            (cell.padding.top - 5.0).abs() < 0.3,
            "vertical padding {} should land on the hand-measured 5px",
            cell.padding.top
        );
    }

    #[test]
    fn theme_builds_with_the_workbench_palette_and_metrics() {
        let t = theme();
        // The default paints slate, not Dark Modern (ratified
        // 2026-07-27) — card is the stock `#18191B`.
        assert_eq!(
            rgb(t.palette().card),
            rgb(Palette::radix_slate_blue_dark().card)
        );
        assert_ne!(rgb(t.palette().card), rgb(vs::SIDE_BAR_BG));
        assert_eq!(
            t.metrics().default_component_size(),
            ComponentSize::Xs,
            "workbench controls sit on the 28px rung"
        );
        assert_eq!(t.metrics().radius_scale(), RADIUS_SCALE);
        assert_eq!(t.mono_font_family(), FontFamily::JetBrainsMono);
        assert_eq!(t.font_family(), FontFamily::Inter);
    }

    // -----------------------------------------------------------------
    // The 2026-07-27 retarget: slate values under the VS Code keys,
    // Dark Modern demoted to a variant.
    // -----------------------------------------------------------------

    fn u8s(c: damascene_core::tree::Color) -> [u8; 3] {
        let [r, g, b, _] = c.to_srgb_u8a();
        [r, g, b]
    }

    #[test]
    fn the_two_themes_are_pinned_to_their_backgrounds() {
        // One assertion each, on the value a screenshot can be probed
        // for: the default is the stock slate ramp, the variant is
        // Dark Modern's editor grey.
        assert_eq!(u8s(theme().palette().background), [0x11, 0x11, 0x13]);
        assert_eq!(rgb(dark_modern().palette().background), rgb(vs::EDITOR_BG));
        assert_eq!(rgb(dark_modern().palette().card), rgb(vs::SIDE_BAR_BG));
    }

    #[test]
    fn the_same_keys_resolve_to_slate_by_default_and_dark_modern_in_the_variant() {
        let slate = slate_palette();
        let dm = dark_modern_palette();

        // Chrome ground: the value round-2 measured, and the value VS
        // Code ships.
        assert_eq!(
            u8s(slate.lookup("sideBar.background").unwrap()),
            [0x18, 0x19, 0x1B]
        );
        assert_eq!(
            u8s(dm.lookup("sideBar.background").unwrap()),
            [0x18, 0x18, 0x18]
        );
        // Primary action fill.
        assert_eq!(
            u8s(slate.lookup("button.background").unwrap()),
            [0x00, 0x90, 0xFF]
        );
        assert_eq!(
            u8s(dm.lookup("button.background").unwrap()),
            [0x00, 0x78, 0xD4]
        );
        // Content well.
        assert_eq!(
            u8s(slate.lookup("editor.background").unwrap()),
            [0x11, 0x11, 0x13]
        );
        assert_eq!(
            u8s(dm.lookup("editor.background").unwrap()),
            [0x1F, 0x1F, 0x1F]
        );
    }

    #[test]
    fn chrome_rises_above_the_content_well_under_the_default() {
        // The retarget's structural break: VS Code sinks chrome below the
        // editor, the shadcn genre raises it above the content. Both
        // models must stay *layered* — the failure mode is collapse.
        let slate = slate_palette();
        assert!(
            slate.card.r > slate.background.r,
            "chrome rises above content in the slate default"
        );
        assert!(
            dark_modern_palette().card.r < dark_modern_palette().background.r,
            "Dark Modern keeps its sunken chrome"
        );
        assert_ne!(rgb(slate.card), rgb(slate.background));
    }

    #[test]
    fn remapped_keys_take_the_slot_that_owns_their_semantic() {
        let p = slate_palette();
        let same = |key: &str, slot: damascene_core::tree::Color| {
            assert_eq!(
                u8s(p.lookup(key).unwrap_or_else(|| panic!("{key} unresolved"))),
                u8s(slot),
                "{key}"
            );
        };
        let stock = Palette::radix_slate_blue_dark();

        // Chrome grounds, all one slot, so a palette swap moves the
        // whole shell at once.
        for key in [
            "sideBar.background",
            "sideBarSectionHeader.background",
            "statusBar.background",
            "titleBar.activeBackground",
            "activityBar.background",
            "panel.background",
            "editorGroupHeader.tabsBackground",
            "tab.inactiveBackground",
        ] {
            same(key, stock.card);
        }
        // Content, and the active tab that is continuous with it.
        same("editor.background", stock.background);
        same("tab.activeBackground", stock.background);
        // Floating family.
        for key in [
            "quickInput.background",
            "editorWidget.background",
            "menu.background",
            "dropdown.listBackground",
        ] {
            same(key, stock.popover);
        }
        // Borders: chrome on `border`, controls on `input`.
        for key in [
            "sideBar.border",
            "sideBarSectionHeader.border",
            "statusBar.border",
            "titleBar.border",
            "activityBar.border",
            "panel.border",
            "tab.border",
            "editorGroupHeader.tabsBorder",
            "editorGroup.border",
            "pickerGroup.border",
        ] {
            same(key, stock.border);
        }
        for key in [
            "input.border",
            "dropdown.border",
            "checkbox.border",
            "widget.border",
        ] {
            same(key, stock.input);
        }
        // Roles.
        same("button.foreground", stock.primary_foreground);
        same("button.secondaryBackground", stock.secondary);
        same("button.secondaryForeground", stock.secondary_foreground);
        same("badge.background", stock.badge);
        same("badge.foreground", stock.badge_foreground);
        same("focusBorder", stock.ring);
        same("descriptionForeground", stock.muted_foreground);
        same("errorForeground", stock.destructive);
        same("textLink.foreground", stock.link_foreground);
        same("editorGutter.addedBackground", stock.success);
        same("editorGutter.modifiedBackground", stock.info);
        same("editorGutter.deletedBackground", stock.destructive);
        same("chat.editedFileForeground", stock.warning);
        same("list.hoverBackground", stock.muted);
        same("list.activeSelectionBackground", stock.accent);
        same("tab.activeBorderTop", stock.primary);
    }

    #[test]
    fn input_background_is_the_trough_stock_inputs_actually_render() {
        // `SurfaceRole::Input` derives its fill as
        // `palette.resolve(MUTED).darken(0.08)`; the key names that
        // derived value, not `muted` itself, or the label would sit one
        // step off the control it describes.
        let p = slate_palette();
        let stock = Palette::radix_slate_blue_dark();
        let trough = stock.resolve(damascene_core::tokens::MUTED).darken(0.08);
        for key in [
            "input.background",
            "dropdown.background",
            "checkbox.background",
        ] {
            assert_eq!(u8s(p.lookup(key).unwrap()), u8s(trough), "{key}");
        }
        assert_ne!(
            u8s(trough),
            u8s(stock.muted),
            "the darken must do something"
        );
    }

    #[test]
    fn no_key_keeps_a_dark_modern_value_in_the_default() {
        // The retarget's completeness check: every registered key must
        // resolve to something in the *new* palette's vocabulary — a
        // slot value or the derived input trough. A key forgotten in the
        // remap would resolve to its Dark Modern literal, which is not
        // in this set, and land here.
        let p = slate_palette();
        let stock = Palette::radix_slate_blue_dark();
        let mut allowed: Vec<[u8; 3]> = [
            stock.background,
            stock.foreground,
            stock.card,
            stock.card_foreground,
            stock.popover,
            stock.popover_foreground,
            stock.primary,
            stock.primary_foreground,
            stock.secondary,
            stock.secondary_foreground,
            stock.muted,
            stock.muted_foreground,
            stock.accent,
            stock.accent_foreground,
            stock.destructive,
            stock.destructive_foreground,
            stock.border,
            stock.input,
            stock.ring,
            stock.success,
            stock.warning,
            stock.info,
            stock.link_foreground,
            stock.badge,
            stock.badge_foreground,
        ]
        .iter()
        .map(|c| u8s(*c))
        .collect();
        allowed.push(u8s(stock
            .resolve(damascene_core::tokens::MUTED)
            .darken(0.08)));
        allowed.push(u8s(stock.primary.darken(0.12))); // button.hoverBackground

        for c in crate::tokens::ALL {
            let key = c.token.expect("workbench tokens carry their key");
            let resolved = p
                .lookup(key)
                .unwrap_or_else(|| panic!("{key} is not registered on the slate palette"));
            assert!(
                allowed.contains(&u8s(resolved)),
                "{key} resolved to {:?}, which is not a slate palette value \
                 — it probably kept its Dark Modern literal",
                u8s(resolved)
            );
        }
    }

    #[test]
    fn the_remap_is_slot_derived_so_it_follows_any_palette() {
        // Not slate-specific: the same call over a different stock
        // palette must move every key with it. This is the property that
        // makes the next palette swap one line.
        let sand = register_workbench_tokens_from_palette(Palette::radix_sand_amber_dark());
        assert_eq!(
            u8s(sand.lookup("sideBar.background").unwrap()),
            u8s(Palette::radix_sand_amber_dark().card)
        );
        assert_eq!(
            u8s(sand.lookup("button.background").unwrap()),
            u8s(Palette::radix_sand_amber_dark().primary)
        );
    }

    #[test]
    fn the_badge_slot_is_filled_deliberately_in_both_palettes() {
        // Dark Modern transcribes VS Code's own key; slate takes the
        // stock ramp's chip step. Neither may fall back to the
        // `..Palette::damascene_dark()` zinc value, which is a
        // different ramp's grey.
        let dm = dark_modern_palette();
        assert_eq!(rgb(dm.badge), rgb(vs::BADGE_BG));
        assert_eq!(rgb(dm.badge_foreground), rgb(vs::BADGE_FG));

        let slate = slate_palette();
        assert_eq!(
            rgb(slate.badge),
            rgb(Palette::radix_slate_blue_dark().badge)
        );
        assert_eq!(u8s(slate.badge), [0x69, 0x6E, 0x77]);
        assert_ne!(
            rgb(slate.badge),
            rgb(Palette::damascene_dark().badge),
            "the slate palette must not inherit zinc's chip step"
        );
    }

    #[test]
    fn the_stock_badge_token_and_the_vs_code_key_are_one_value() {
        // `chrome::chip` paints the *stock* token so a downstream theme
        // of this vocabulary moves it; the VS Code key names the same
        // material. Exactly the `tokens::BORDER` / `panel.border`
        // relationship, and it must hold under both palettes.
        for p in [slate_palette(), dark_modern_palette()] {
            let stock = p.resolve(damascene_core::tokens::BADGE);
            let key = p.lookup("badge.background").expect("key is registered");
            assert_eq!(u8s(stock), u8s(key));

            let stock_fg = p.resolve(damascene_core::tokens::BADGE_FOREGROUND);
            let key_fg = p.lookup("badge.foreground").expect("key is registered");
            assert_eq!(u8s(stock_fg), u8s(key_fg));
        }
    }

    #[test]
    fn the_badge_key_no_longer_borrows_the_secondary_slot() {
        // The acceptance complaint, as a tripwire: `secondary` is a
        // near-surface value, and pointing the chip material at it is
        // what made chips unreadable on a pane header.
        let p = slate_palette();
        assert_ne!(
            u8s(p.lookup("badge.background").unwrap()),
            u8s(Palette::radix_slate_blue_dark().secondary),
        );
    }

    #[test]
    fn the_two_themes_differ_only_in_their_palette() {
        // Density is not color: the retarget must not have moved a
        // metric. Both themes come out of one builder, and this is the
        // tripwire if that ever forks.
        let d = theme();
        let m = dark_modern();
        assert_eq!(
            d.metrics().default_component_size(),
            m.metrics().default_component_size()
        );
        assert_eq!(d.metrics().radius_scale(), m.metrics().radius_scale());
        assert_eq!(d.font_family(), m.font_family());
        assert_eq!(d.mono_font_family(), m.mono_font_family());
    }
}
