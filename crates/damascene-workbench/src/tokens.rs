//! VS Code workbench color tokens — the dense-app counterpart to
//! `damascene_core::tokens`.
//!
//! # Naming
//!
//! Every constant here is minted with [`Color::srgb_token`] under its
//! **verbatim VS Code theme key** — `"sideBar.background"`,
//! `"statusBar.background"`, `"focusBorder"`. That string is the part
//! that matters: it is the most heavily corpus-represented dense-app
//! token vocabulary in existence, and it is what the active palette
//! registers through `Palette::with_token`, so these names resolve
//! through the palette like any stock shadcn name does. Paint
//! [`SIDE_BAR_BG`] and you get whatever the live theme's
//! `sideBar.background` is — the baked hex below is only the fallback
//! for a palette that never registered the key.
//!
//! The Rust identifiers are flat `SCREAMING_SNAKE` (`SIDE_BAR_BG`,
//! `STATUS_BAR_BG`) — matching the `damascene_core::tokens` house style,
//! and settling the naming question left open in
//! `docs/WORKBENCH_VISION.md` in favor of flat constants over a
//! `workbench::status_bar::BG` module tree. Abbreviations follow the key
//! suffix: `.background` → `_BG`, `.foreground` → `_FG`, `.border` →
//! `_BORDER`.
//!
//! One name collides with the stock vocabulary: VS Code's `foreground`
//! key is spelled exactly like shadcn's `foreground` token.
//! `Palette::lookup` matches its built-in arms first, so [`FOREGROUND`]
//! resolves through `Palette::foreground` rather than through the extra
//! map. That is correct — both names mean "default text color", and
//! every theme in this crate points the two at one value.
//!
//! # Values — Dark Modern's, which is now a *variant*
//!
//! The constants keep Dark Modern's calibrated values. Since the
//! 2026-07-27 retarget (`docs/WORKBENCH_VISION.md`, §"Retarget
//! ratified") those are what [`crate::theme::dark_modern`] paints;
//! the crate default [`crate::theme::theme`] registers the same keys on
//! the stock slate + blue palette's slots, so under it a `vs::` constant
//! resolves to a slate value and the hex in its rustdoc is the
//! calibration citation rather than a prediction of the pixel. Both
//! remain available, and either can be rethemed key-by-key with
//! `Palette::with_token`.
//!
//! Colors are **Dark Modern**, VS Code's out-of-the-box dark theme,
//! resolved through its include chain
//! (`dark_vs.json` → `dark_plus.json` → `dark_modern.json`, later wins)
//! from the verbatim upstream artifacts in
//! `references/vscode-calibration/themes/`. Keys no theme in the chain
//! sets come from VS Code's built-in color registry, cited in
//! `references/vscode-calibration/README.md`; each such constant says so
//! in its rustdoc.
//!
//! Every constant's rustdoc carries its VS Code key and its source hex,
//! so a value can be checked against the reference without leaving the
//! file.
//!
//! # Metrics are provisional
//!
//! The calibration reference is **color only**. The metric constants at
//! the bottom of this module (bar heights, chip height, the workbench
//! radius) are the indicative values from `docs/WORKBENCH_VISION.md`,
//! and they are hypotheses until the screenshot-measurement step in that
//! document's calibration plan lands. They are marked individually.

#![warn(missing_docs)]

use damascene_core::tree::Color;

// ---------------------------------------------------------------------
// Editor — the base surface everything else layers against.
// ---------------------------------------------------------------------

/// `editor.background` — `#1F1F1F`. The base workbench surface; maps to
/// the `background` palette slot.
pub const EDITOR_BG: Color = Color::srgb_token("editor.background", 31, 31, 31, 255);
/// `editor.foreground` — `#CCCCCC`.
pub const EDITOR_FG: Color = Color::srgb_token("editor.foreground", 204, 204, 204, 255);
/// `foreground` — `#CCCCCC`. The workbench-wide default text color.
///
/// Shares its token string with shadcn's `foreground`, so this resolves
/// through `Palette::foreground` rather than the extra map. See the
/// module docs.
pub const FOREGROUND: Color = Color::srgb_token("foreground", 204, 204, 204, 255);
/// `descriptionForeground` — `#9D9D9D`. De-emphasized secondary text.
pub const DESCRIPTION_FG: Color = Color::srgb_token("descriptionForeground", 157, 157, 157, 255);
/// `errorForeground` — `#F85149`.
pub const ERROR_FG: Color = Color::srgb_token("errorForeground", 248, 81, 73, 255);
/// `textLink.foreground` — `#4daafc`.
pub const TEXT_LINK_FG: Color = Color::srgb_token("textLink.foreground", 77, 170, 252, 255);

// ---------------------------------------------------------------------
// Side bar — the layered panel that replaces shadcn's floating card.
// ---------------------------------------------------------------------

/// `sideBar.background` — `#181818`. One value step *below* the editor,
/// and the value that turns shadcn's floating cards into layered panels
/// (it backs the `card` palette slot).
pub const SIDE_BAR_BG: Color = Color::srgb_token("sideBar.background", 24, 24, 24, 255);
/// `sideBar.foreground` — `#CCCCCC`.
pub const SIDE_BAR_FG: Color = Color::srgb_token("sideBar.foreground", 204, 204, 204, 255);
/// `sideBar.border` — `#2B2B2B`.
pub const SIDE_BAR_BORDER: Color = Color::srgb_token("sideBar.border", 43, 43, 43, 255);
/// `sideBarTitle.foreground` — `#CCCCCC` (Dark Modern overrides
/// `dark_vs.json`'s `#BBBBBB`).
pub const SIDE_BAR_TITLE_FG: Color =
    Color::srgb_token("sideBarTitle.foreground", 204, 204, 204, 255);
/// `sideBarSectionHeader.background` — `#181818` (Dark Modern overrides
/// `dark_vs.json`'s transparent `#0000`).
pub const SIDE_BAR_SECTION_HEADER_BG: Color =
    Color::srgb_token("sideBarSectionHeader.background", 24, 24, 24, 255);
/// `sideBarSectionHeader.foreground` — `#CCCCCC`.
pub const SIDE_BAR_SECTION_HEADER_FG: Color =
    Color::srgb_token("sideBarSectionHeader.foreground", 204, 204, 204, 255);
/// `sideBarSectionHeader.border` — `#2B2B2B` (Dark Modern overrides
/// `dark_vs.json`'s `#ccc3`).
pub const SIDE_BAR_SECTION_HEADER_BORDER: Color =
    Color::srgb_token("sideBarSectionHeader.border", 43, 43, 43, 255);

// ---------------------------------------------------------------------
// Bars — status, title, activity, panel.
// ---------------------------------------------------------------------

/// `statusBar.background` — `#181818`.
pub const STATUS_BAR_BG: Color = Color::srgb_token("statusBar.background", 24, 24, 24, 255);
/// `statusBar.foreground` — `#CCCCCC`.
pub const STATUS_BAR_FG: Color = Color::srgb_token("statusBar.foreground", 204, 204, 204, 255);
/// `statusBar.border` — `#2B2B2B`. The over-rule separating the status
/// bar from the editor region.
pub const STATUS_BAR_BORDER: Color = Color::srgb_token("statusBar.border", 43, 43, 43, 255);
/// `statusBarItem.hoverBackground` — `#F1F1F133` (25% white wash).
pub const STATUS_BAR_ITEM_HOVER_BG: Color =
    Color::srgb_token("statusBarItem.hoverBackground", 241, 241, 241, 51);
/// `statusBarItem.remoteBackground` — `#0078D4` (Dark Modern overrides
/// `dark_vs.json`'s `#16825D`).
pub const STATUS_BAR_ITEM_REMOTE_BG: Color =
    Color::srgb_token("statusBarItem.remoteBackground", 0, 120, 212, 255);
/// `statusBarItem.remoteForeground` — `#FFFFFF`.
pub const STATUS_BAR_ITEM_REMOTE_FG: Color =
    Color::srgb_token("statusBarItem.remoteForeground", 255, 255, 255, 255);
/// `statusBarItem.prominentBackground` — `#6E768166`.
pub const STATUS_BAR_ITEM_PROMINENT_BG: Color =
    Color::srgb_token("statusBarItem.prominentBackground", 110, 118, 129, 102);

/// `titleBar.activeBackground` — `#181818`.
pub const TITLE_BAR_ACTIVE_BG: Color =
    Color::srgb_token("titleBar.activeBackground", 24, 24, 24, 255);
/// `titleBar.activeForeground` — `#CCCCCC`.
pub const TITLE_BAR_ACTIVE_FG: Color =
    Color::srgb_token("titleBar.activeForeground", 204, 204, 204, 255);
/// `titleBar.border` — `#2B2B2B`. The under-rule below the title strip.
pub const TITLE_BAR_BORDER: Color = Color::srgb_token("titleBar.border", 43, 43, 43, 255);

/// `activityBar.background` — `#181818`.
pub const ACTIVITY_BAR_BG: Color = Color::srgb_token("activityBar.background", 24, 24, 24, 255);
/// `activityBar.foreground` — `#D7D7D7`.
pub const ACTIVITY_BAR_FG: Color = Color::srgb_token("activityBar.foreground", 215, 215, 215, 255);
/// `activityBar.inactiveForeground` — `#868686`.
pub const ACTIVITY_BAR_INACTIVE_FG: Color =
    Color::srgb_token("activityBar.inactiveForeground", 134, 134, 134, 255);
/// `activityBar.border` — `#2B2B2B`.
pub const ACTIVITY_BAR_BORDER: Color = Color::srgb_token("activityBar.border", 43, 43, 43, 255);

/// `panel.background` — `#181818`.
pub const PANEL_BG: Color = Color::srgb_token("panel.background", 24, 24, 24, 255);
/// `panel.border` — `#2B2B2B`. The workbench's canonical hairline color.
/// It backs the `border` palette slot in [`crate::theme::dark_modern_palette`], so
/// under [`crate::theme::dark_modern`] this and
/// `damascene_core::tokens::BORDER` are one value; under
/// [`crate::theme::theme`] the pointing reverses — this key is registered
/// *onto* the palette's `border` slot — and they are still one value.
pub const PANEL_BORDER: Color = Color::srgb_token("panel.border", 43, 43, 43, 255);
/// `panelTitle.activeForeground` — `#CCCCCC`.
pub const PANEL_TITLE_ACTIVE_FG: Color =
    Color::srgb_token("panelTitle.activeForeground", 204, 204, 204, 255);
/// `panelTitle.inactiveForeground` — `#9D9D9D`.
pub const PANEL_TITLE_INACTIVE_FG: Color =
    Color::srgb_token("panelTitle.inactiveForeground", 157, 157, 157, 255);

// ---------------------------------------------------------------------
// Editor group header and tabs.
// ---------------------------------------------------------------------

/// `editorGroupHeader.tabsBackground` — `#181818`. The trough the tab
/// strip sits in; inactive tabs melt into it and the active tab lifts to
/// [`TAB_ACTIVE_BG`].
pub const EDITOR_GROUP_HEADER_TABS_BG: Color =
    Color::srgb_token("editorGroupHeader.tabsBackground", 24, 24, 24, 255);
/// `editorGroupHeader.tabsBorder` — `#2B2B2B`.
pub const EDITOR_GROUP_HEADER_TABS_BORDER: Color =
    Color::srgb_token("editorGroupHeader.tabsBorder", 43, 43, 43, 255);
/// `editorGroup.border` — `#FFFFFF17` (9% white). The split separator
/// between side-by-side editor groups.
pub const EDITOR_GROUP_BORDER: Color = Color::srgb_token("editorGroup.border", 255, 255, 255, 23);
/// `tab.activeBackground` — `#1F1F1F`. Deliberately identical to
/// [`EDITOR_BG`]: the active tab is continuous with the editor below it.
pub const TAB_ACTIVE_BG: Color = Color::srgb_token("tab.activeBackground", 31, 31, 31, 255);
/// `tab.activeForeground` — `#FFFFFF`.
pub const TAB_ACTIVE_FG: Color = Color::srgb_token("tab.activeForeground", 255, 255, 255, 255);
/// `tab.inactiveBackground` — `#181818`.
pub const TAB_INACTIVE_BG: Color = Color::srgb_token("tab.inactiveBackground", 24, 24, 24, 255);
/// `tab.inactiveForeground` — `#9D9D9D`.
pub const TAB_INACTIVE_FG: Color = Color::srgb_token("tab.inactiveForeground", 157, 157, 157, 255);
/// `tab.border` — `#2B2B2B`. The vertical rule between tabs.
pub const TAB_BORDER: Color = Color::srgb_token("tab.border", 43, 43, 43, 255);
/// `tab.activeBorderTop` — `#0078D4`. The accent bar over the active tab.
pub const TAB_ACTIVE_BORDER_TOP: Color = Color::srgb_token("tab.activeBorderTop", 0, 120, 212, 255);

// ---------------------------------------------------------------------
// Controls.
// ---------------------------------------------------------------------

/// `focusBorder` — `#0078D4`. The keyboard-focus ring; backs `ring`.
pub const FOCUS_BORDER: Color = Color::srgb_token("focusBorder", 0, 120, 212, 255);
/// `button.background` — `#0078D4`. The primary action fill.
pub const BUTTON_BG: Color = Color::srgb_token("button.background", 0, 120, 212, 255);
/// `button.foreground` — `#FFFFFF`.
pub const BUTTON_FG: Color = Color::srgb_token("button.foreground", 255, 255, 255, 255);
/// `button.hoverBackground` — `#026EC1`.
pub const BUTTON_HOVER_BG: Color = Color::srgb_token("button.hoverBackground", 2, 110, 193, 255);
/// `button.secondaryBackground` — `#00000000`, i.e. **fully
/// transparent**. VS Code's secondary button is a ghost button at rest
/// and only acquires a fill on hover; see [`BUTTON_SECONDARY_HOVER_BG`]
/// and [`crate::theme::dark_modern_palette`] for why the palette's `secondary` slot
/// takes the hover value instead of this one.
pub const BUTTON_SECONDARY_BG: Color = Color::srgb_token("button.secondaryBackground", 0, 0, 0, 0);
/// `button.secondaryForeground` — `#CCCCCC`.
pub const BUTTON_SECONDARY_FG: Color =
    Color::srgb_token("button.secondaryForeground", 204, 204, 204, 255);
/// `button.secondaryHoverBackground` — `#2B2B2B`.
pub const BUTTON_SECONDARY_HOVER_BG: Color =
    Color::srgb_token("button.secondaryHoverBackground", 43, 43, 43, 255);
/// `input.background` — `#313131`.
pub const INPUT_BG: Color = Color::srgb_token("input.background", 49, 49, 49, 255);
/// `input.border` — `#3C3C3C`. Backs the `input` palette slot, kept
/// distinct from [`PANEL_BORDER`] exactly as VS Code keeps them distinct.
pub const INPUT_BORDER: Color = Color::srgb_token("input.border", 60, 60, 60, 255);
/// `input.foreground` — `#CCCCCC`.
pub const INPUT_FG: Color = Color::srgb_token("input.foreground", 204, 204, 204, 255);
/// `input.placeholderForeground` — `#989898` (Dark Modern overrides
/// `dark_vs.json`'s `#A6A6A6`).
pub const INPUT_PLACEHOLDER_FG: Color =
    Color::srgb_token("input.placeholderForeground", 152, 152, 152, 255);
/// `dropdown.background` — `#313131`.
pub const DROPDOWN_BG: Color = Color::srgb_token("dropdown.background", 49, 49, 49, 255);
/// `dropdown.border` — `#3C3C3C`.
pub const DROPDOWN_BORDER: Color = Color::srgb_token("dropdown.border", 60, 60, 60, 255);
/// `dropdown.listBackground` — `#1F1F1F`.
pub const DROPDOWN_LIST_BG: Color = Color::srgb_token("dropdown.listBackground", 31, 31, 31, 255);
/// `checkbox.background` — `#313131`.
pub const CHECKBOX_BG: Color = Color::srgb_token("checkbox.background", 49, 49, 49, 255);
/// `checkbox.border` — `#3C3C3C` (Dark Modern overrides `dark_vs.json`'s
/// `#6B6B6B`).
pub const CHECKBOX_BORDER: Color = Color::srgb_token("checkbox.border", 60, 60, 60, 255);
/// `progressBar.background` — `#0078D4`.
pub const PROGRESS_BAR_BG: Color = Color::srgb_token("progressBar.background", 0, 120, 212, 255);

// ---------------------------------------------------------------------
// Floating surfaces — quick input, widgets, menus.
// ---------------------------------------------------------------------

/// `quickInput.background` — `#222222`. The command-palette surface;
/// backs the `popover` slot. Note it steps *up* from [`EDITOR_BG`] while
/// [`SIDE_BAR_BG`] steps down — floating surfaces rise, docked ones sink.
pub const QUICK_INPUT_BG: Color = Color::srgb_token("quickInput.background", 34, 34, 34, 255);
/// `quickInput.foreground` — `#CCCCCC`.
pub const QUICK_INPUT_FG: Color = Color::srgb_token("quickInput.foreground", 204, 204, 204, 255);
/// `editorWidget.background` — `#202020`.
pub const EDITOR_WIDGET_BG: Color = Color::srgb_token("editorWidget.background", 32, 32, 32, 255);
/// `widget.border` — `#313131` (Dark Modern overrides `dark_vs.json`'s
/// `#303031`).
pub const WIDGET_BORDER: Color = Color::srgb_token("widget.border", 49, 49, 49, 255);
/// `menu.background` — `#1F1F1F` (Dark Modern overrides `dark_vs.json`'s
/// `#252526`).
pub const MENU_BG: Color = Color::srgb_token("menu.background", 31, 31, 31, 255);
/// `menu.selectionBackground` — `#0078d4`.
pub const MENU_SELECTION_BG: Color =
    Color::srgb_token("menu.selectionBackground", 0, 120, 212, 255);
/// `pickerGroup.border` — `#3C3C3C`.
pub const PICKER_GROUP_BORDER: Color = Color::srgb_token("pickerGroup.border", 60, 60, 60, 255);

// ---------------------------------------------------------------------
// Badge and list.
//
// The `list.*` values are set by no theme in the Dark Modern chain; they
// come from VS Code's built-in color registry
// (`src/vs/platform/theme/common/colors/listColors.ts`), cited in
// `references/vscode-calibration/README.md`.
// ---------------------------------------------------------------------

/// `badge.background` — `#616161`.
pub const BADGE_BG: Color = Color::srgb_token("badge.background", 97, 97, 97, 255);
/// `badge.foreground` — `#F8F8F8`.
pub const BADGE_FG: Color = Color::srgb_token("badge.foreground", 248, 248, 248, 255);
/// `list.activeSelectionBackground` — `#04395E`. **Registry default**;
/// no theme in the Dark Modern chain sets it.
pub const LIST_ACTIVE_SELECTION_BG: Color =
    Color::srgb_token("list.activeSelectionBackground", 4, 57, 94, 255);
/// `list.activeSelectionForeground` — `Color.white`, i.e. `#FFFFFF`.
/// **Registry default.**
pub const LIST_ACTIVE_SELECTION_FG: Color =
    Color::srgb_token("list.activeSelectionForeground", 255, 255, 255, 255);
/// `list.inactiveSelectionBackground` — `#37373D`. **Registry default.**
pub const LIST_INACTIVE_SELECTION_BG: Color =
    Color::srgb_token("list.inactiveSelectionBackground", 55, 55, 61, 255);
/// `list.hoverBackground` — `#2A2D2E`. **Registry default.**
pub const LIST_HOVER_BG: Color = Color::srgb_token("list.hoverBackground", 42, 45, 46, 255);

// ---------------------------------------------------------------------
// Status colors.
//
// Dark Modern sets no `*Warning.foreground` key, so the warning role is
// a judgment call — see `crate::theme::dark_modern_palette`.
// ---------------------------------------------------------------------

/// `editorGutter.addedBackground` — `#2EA043`. The workbench's green.
pub const EDITOR_GUTTER_ADDED_BG: Color =
    Color::srgb_token("editorGutter.addedBackground", 46, 160, 67, 255);
/// `editorGutter.deletedBackground` — `#F85149`.
pub const EDITOR_GUTTER_DELETED_BG: Color =
    Color::srgb_token("editorGutter.deletedBackground", 248, 81, 73, 255);
/// `editorGutter.modifiedBackground` — `#0078D4`. The workbench's
/// informational blue.
pub const EDITOR_GUTTER_MODIFIED_BG: Color =
    Color::srgb_token("editorGutter.modifiedBackground", 0, 120, 212, 255);
/// `chat.editedFileForeground` — `#E2C08D`. The nearest thing Dark
/// Modern has to a warning color: the amber it uses for "this has been
/// touched, look at it".
pub const CHAT_EDITED_FILE_FG: Color =
    Color::srgb_token("chat.editedFileForeground", 226, 192, 141, 255);

// ---------------------------------------------------------------------
// Selection.
// ---------------------------------------------------------------------

/// `editor.inactiveSelectionBackground` — `#3A3D41` (from
/// `dark_vs.json`; Dark Modern does not override it).
pub const EDITOR_INACTIVE_SELECTION_BG: Color =
    Color::srgb_token("editor.inactiveSelectionBackground", 58, 61, 65, 255);

// ---------------------------------------------------------------------
// The roster.
// ---------------------------------------------------------------------

/// Every workbench color token in this module.
///
/// Exists so registration and coverage are checkable rather than
/// eyeballed: [`crate::theme::register_workbench_tokens`] is a hand-written
/// list, and the test that walks this slice is what keeps the two in
/// step when a constant is added here and forgotten there.
pub const ALL: &[Color] = &[
    EDITOR_BG,
    EDITOR_FG,
    FOREGROUND,
    DESCRIPTION_FG,
    ERROR_FG,
    TEXT_LINK_FG,
    SIDE_BAR_BG,
    SIDE_BAR_FG,
    SIDE_BAR_BORDER,
    SIDE_BAR_TITLE_FG,
    SIDE_BAR_SECTION_HEADER_BG,
    SIDE_BAR_SECTION_HEADER_FG,
    SIDE_BAR_SECTION_HEADER_BORDER,
    STATUS_BAR_BG,
    STATUS_BAR_FG,
    STATUS_BAR_BORDER,
    STATUS_BAR_ITEM_HOVER_BG,
    STATUS_BAR_ITEM_REMOTE_BG,
    STATUS_BAR_ITEM_REMOTE_FG,
    STATUS_BAR_ITEM_PROMINENT_BG,
    TITLE_BAR_ACTIVE_BG,
    TITLE_BAR_ACTIVE_FG,
    TITLE_BAR_BORDER,
    ACTIVITY_BAR_BG,
    ACTIVITY_BAR_FG,
    ACTIVITY_BAR_INACTIVE_FG,
    ACTIVITY_BAR_BORDER,
    PANEL_BG,
    PANEL_BORDER,
    PANEL_TITLE_ACTIVE_FG,
    PANEL_TITLE_INACTIVE_FG,
    EDITOR_GROUP_HEADER_TABS_BG,
    EDITOR_GROUP_HEADER_TABS_BORDER,
    EDITOR_GROUP_BORDER,
    TAB_ACTIVE_BG,
    TAB_ACTIVE_FG,
    TAB_INACTIVE_BG,
    TAB_INACTIVE_FG,
    TAB_BORDER,
    TAB_ACTIVE_BORDER_TOP,
    FOCUS_BORDER,
    BUTTON_BG,
    BUTTON_FG,
    BUTTON_HOVER_BG,
    BUTTON_SECONDARY_BG,
    BUTTON_SECONDARY_FG,
    BUTTON_SECONDARY_HOVER_BG,
    INPUT_BG,
    INPUT_BORDER,
    INPUT_FG,
    INPUT_PLACEHOLDER_FG,
    DROPDOWN_BG,
    DROPDOWN_BORDER,
    DROPDOWN_LIST_BG,
    CHECKBOX_BG,
    CHECKBOX_BORDER,
    PROGRESS_BAR_BG,
    QUICK_INPUT_BG,
    QUICK_INPUT_FG,
    EDITOR_WIDGET_BG,
    WIDGET_BORDER,
    MENU_BG,
    MENU_SELECTION_BG,
    PICKER_GROUP_BORDER,
    BADGE_BG,
    BADGE_FG,
    LIST_ACTIVE_SELECTION_BG,
    LIST_ACTIVE_SELECTION_FG,
    LIST_INACTIVE_SELECTION_BG,
    LIST_HOVER_BG,
    EDITOR_GUTTER_ADDED_BG,
    EDITOR_GUTTER_DELETED_BG,
    EDITOR_GUTTER_MODIFIED_BG,
    CHAT_EDITED_FILE_FG,
    EDITOR_INACTIVE_SELECTION_BG,
];

// ---------------------------------------------------------------------
// Metrics — PROVISIONAL.
//
// `references/vscode-calibration/` is color-only and says so. Every
// value below is the indicative figure from `docs/WORKBENCH_VISION.md`,
// standing in until the screenshot-measurement step of that document's
// calibration plan lands. Treat them as hypotheses.
// ---------------------------------------------------------------------

/// A 1px rule. Not a hypothesis: it is 1 logical pixel by definition.
pub const HAIRLINE: f32 = 1.0;

/// Status-bar height — **provisional** 22px, the indicative figure from
/// `docs/WORKBENCH_VISION.md`. Deliberately the shortest region in the
/// shell: it reads as instrumentation, not content.
pub const STATUS_BAR_HEIGHT: f32 = 22.0;

/// Pane-header height — **provisional** 22px, matching VS Code's side-bar
/// section headers rather than its taller (~35px) editor group header.
pub const PANE_HEADER_HEIGHT: f32 = 22.0;

/// Title-strip height — **provisional** 30px.
pub const TITLE_BAR_HEIGHT: f32 = 30.0;

/// Chip height — **provisional** 16px. The same height the stock badge
/// ladder's floor rung
/// ([`damascene_core::metrics::ComponentSize::Xxs`], the chrome rung)
/// now reaches; [`crate::chrome::chip`] still differs from a badge in
/// fill and radius, not in density.
pub const CHIP_HEIGHT: f32 = 16.0;

/// The workbench corner radius — **provisional** 2px.
///
/// This is the target the theme's radius scale is solved for: see
/// [`crate::theme::RADIUS_SCALE`] for the arithmetic that lands a default
/// `Md` control on this value.
pub const RADIUS: f32 = 2.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_carry_their_vs_code_key_verbatim() {
        // The token *string* is the load-bearing part of this module —
        // it is what `Palette::with_token` registers and what
        // `Palette::lookup` matches. Dotted VS Code keys, not
        // snake_cased Rust names.
        assert_eq!(SIDE_BAR_BG.token, Some("sideBar.background"));
        assert_eq!(STATUS_BAR_BG.token, Some("statusBar.background"));
        assert_eq!(PANEL_BORDER.token, Some("panel.border"));
        assert_eq!(FOCUS_BORDER.token, Some("focusBorder"));
        assert_eq!(
            LIST_ACTIVE_SELECTION_BG.token,
            Some("list.activeSelectionBackground")
        );
    }

    #[test]
    fn the_surface_ramp_is_layered_not_flat() {
        // Diagnosis 3 in WORKBENCH_VISION.md: docked regions sink below
        // the editor, floating ones rise above it. If these ever
        // collapse to one value the whole premise is gone.
        //
        // These greys are neutral, so any channel is the value step.
        let level = |c: Color| c.r;
        assert!(
            level(SIDE_BAR_BG) < level(EDITOR_BG),
            "side bar sinks below editor"
        );
        assert!(
            level(QUICK_INPUT_BG) > level(EDITOR_BG),
            "quick input rises above editor"
        );
        assert_eq!(
            (STATUS_BAR_BG.r, STATUS_BAR_BG.g, STATUS_BAR_BG.b),
            (SIDE_BAR_BG.r, SIDE_BAR_BG.g, SIDE_BAR_BG.b),
            "bars share the side bar's level in Dark Modern"
        );
    }

    #[test]
    fn alpha_bearing_keys_keep_their_alpha_byte() {
        // The 8-digit hex forms in the theme JSON carry alpha in the
        // last byte; dropping it silently would turn washes into solids.
        let alpha_u8 = |c: Color| (c.a * 255.0).round() as u8;
        assert_eq!(alpha_u8(STATUS_BAR_ITEM_HOVER_BG), 0x33); // #F1F1F133
        assert_eq!(alpha_u8(EDITOR_GROUP_BORDER), 0x17); // #FFFFFF17
        assert_eq!(alpha_u8(BUTTON_SECONDARY_BG), 0x00); // #00000000
    }
}
