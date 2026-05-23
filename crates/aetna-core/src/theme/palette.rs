//! Runtime color palette — the swappable rgba backing for color tokens.
//!
//! Tokens (e.g. [`crate::tokens::CARD`]) are [`Color`] values carrying
//! both a fallback rgba and a `token: Some("card")` name. The renderer
//! consults the active [`crate::Theme`]'s palette at paint time, looking
//! up that name to pick the rgba for the currently-active palette. Apps
//! swap palettes at runtime by returning a different [`crate::Theme`] from
//! [`crate::event::App::theme`] each frame.
//!
//! Direct token references swap perfectly across palettes.
//! [`Color::with_alpha`] is alpha-only, so it keeps the token name and
//! resolves cleanly (the user's alpha override survives). The rgb-
//! modifying ops [`Color::darken`]/[`Color::lighten`]/[`Color::mix`]
//! strip the token name — once you've derived rgb from a token, swapping
//! the palette would silently discard the derivation, so derived colors
//! opt out of resolution and render exactly as computed. State animations
//! (hover lighten, press darken, focus mix) all flow through this path.
//!
//! Consequence: a derived color computed against the dark palette renders
//! with its dark-derived rgb even when the active palette is light.
//! `theme.rs:apply_role_material` is the one library site that runs an
//! rgb op against a token (`MUTED.darken(0.08)` for the Sunken role);
//! a tighter fix later is to add an input/container token to `Palette` so the
//! role can reference a resolvable name. State animations don't need this
//! — they want the per-frame derivation to win, not the palette default.
//!
//! The core vocabulary is intentionally close to shadcn/ui: background /
//! foreground pairs for surfaces and actions, plus border, input, ring,
//! and semantic status roles. Link, scrollbar, overlay, and selection
//! tokens are component/domain extensions.

use crate::tree::Color;

/// Runtime backing for the design-token color vocabulary.
///
/// One field per theme-variant token.
#[derive(Clone, Debug)]
pub struct Palette {
    // Core shadcn-shaped semantic colors.
    pub background: Color,
    pub foreground: Color,

    pub card: Color,
    pub card_foreground: Color,

    pub popover: Color,
    pub popover_foreground: Color,

    pub primary: Color,
    pub primary_foreground: Color,

    pub secondary: Color,
    pub secondary_foreground: Color,

    pub muted: Color,
    pub muted_foreground: Color,

    pub accent: Color,
    pub accent_foreground: Color,

    pub destructive: Color,
    pub destructive_foreground: Color,

    pub border: Color,
    pub input: Color,
    pub ring: Color,

    pub success: Color,
    pub success_foreground: Color,
    pub warning: Color,
    pub warning_foreground: Color,
    pub info: Color,
    pub info_foreground: Color,

    // Extensions.
    pub overlay_scrim: Color,
    pub link_foreground: Color,

    pub scrollbar_thumb_fill: Color,
    pub scrollbar_thumb_fill_active: Color,

    pub selection_bg: Color,
    pub selection_bg_unfocused: Color,
}

impl Palette {
    /// Aetna's default dark palette, copied from shadcn/ui's zinc dark
    /// theme scaffold. These rgba values also serve as the compile-time
    /// fallback baked into the constants in [`crate::tokens`].
    pub const fn aetna_dark() -> Self {
        Self {
            background: Color::srgb_token("background", 9, 9, 11, 255),
            foreground: Color::srgb_token("foreground", 250, 250, 250, 255),

            card: Color::srgb_token("card", 9, 9, 11, 255),
            card_foreground: Color::srgb_token("card-foreground", 250, 250, 250, 255),

            popover: Color::srgb_token("popover", 9, 9, 11, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 250, 250, 250, 255),

            primary: Color::srgb_token("primary", 250, 250, 250, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 24, 24, 27, 255),

            secondary: Color::srgb_token("secondary", 39, 39, 42, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 250, 250, 250, 255),

            muted: Color::srgb_token("muted", 39, 39, 42, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 161, 161, 170, 255),

            accent: Color::srgb_token("accent", 39, 39, 42, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 250, 250, 250, 255),

            destructive: Color::srgb_token("destructive", 127, 29, 29, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 250, 250, 250, 255),

            border: Color::srgb_token("border", 39, 39, 42, 255),
            input: Color::srgb_token("input", 39, 39, 42, 255),
            ring: Color::srgb_token("ring", 212, 212, 216, 255),

            success: Color::srgb_token("success", 16, 185, 129, 255),
            success_foreground: Color::srgb_token("success-foreground", 5, 46, 22, 255),
            warning: Color::srgb_token("warning", 245, 158, 11, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 69, 26, 3, 255),
            info: Color::srgb_token("info", 59, 130, 246, 255),
            info_foreground: Color::srgb_token("info-foreground", 239, 246, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 96, 165, 250, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 113, 113, 122, 120),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 161, 161, 170, 220),

            selection_bg: Color::srgb_token("selection-bg", 96, 165, 250, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 113, 113, 122, 64),
        }
    }

    /// Aetna's default light palette, copied from shadcn/ui's zinc light
    /// theme scaffold.
    pub const fn aetna_light() -> Self {
        Self {
            background: Color::srgb_token("background", 255, 255, 255, 255),
            foreground: Color::srgb_token("foreground", 9, 9, 11, 255),

            card: Color::srgb_token("card", 255, 255, 255, 255),
            card_foreground: Color::srgb_token("card-foreground", 9, 9, 11, 255),

            popover: Color::srgb_token("popover", 255, 255, 255, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 9, 9, 11, 255),

            primary: Color::srgb_token("primary", 24, 24, 27, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 250, 250, 250, 255),

            secondary: Color::srgb_token("secondary", 244, 244, 245, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 24, 24, 27, 255),

            muted: Color::srgb_token("muted", 244, 244, 245, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 113, 113, 122, 255),

            accent: Color::srgb_token("accent", 244, 244, 245, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 24, 24, 27, 255),

            destructive: Color::srgb_token("destructive", 239, 68, 68, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 250, 250, 250, 255),

            border: Color::srgb_token("border", 228, 228, 231, 255),
            input: Color::srgb_token("input", 228, 228, 231, 255),
            ring: Color::srgb_token("ring", 24, 24, 27, 255),

            success: Color::srgb_token("success", 16, 185, 129, 255),
            success_foreground: Color::srgb_token("success-foreground", 5, 46, 22, 255),
            warning: Color::srgb_token("warning", 245, 158, 11, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 69, 26, 3, 255),
            info: Color::srgb_token("info", 37, 99, 235, 255),
            info_foreground: Color::srgb_token("info-foreground", 239, 246, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 37, 99, 235, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 113, 113, 122, 90),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 82, 82, 91, 220),

            selection_bg: Color::srgb_token("selection-bg", 37, 99, 235, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 113, 113, 122, 56),
        }
    }

    /// Radix Colors-inspired slate + blue dark palette. The neutral
    /// surfaces come from Radix `slate`; the action, link, focus, and
    /// selected/current treatments come from Radix `blue`. This keeps
    /// the original Aetna black/blue feel, but uses a complete public
    /// scale rather than hand-picked ad hoc values.
    pub const fn radix_slate_blue_dark() -> Self {
        Self {
            background: Color::srgb_token("background", 17, 17, 19, 255),
            foreground: Color::srgb_token("foreground", 237, 238, 240, 255),

            card: Color::srgb_token("card", 24, 25, 27, 255),
            card_foreground: Color::srgb_token("card-foreground", 237, 238, 240, 255),

            popover: Color::srgb_token("popover", 24, 25, 27, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 237, 238, 240, 255),

            primary: Color::srgb_token("primary", 0, 144, 255, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 255, 255, 255, 255),

            secondary: Color::srgb_token("secondary", 33, 34, 37, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 237, 238, 240, 255),

            muted: Color::srgb_token("muted", 33, 34, 37, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 176, 180, 186, 255),

            accent: Color::srgb_token("accent", 13, 40, 71, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 112, 184, 255, 255),

            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 255, 255, 255, 255),

            border: Color::srgb_token("border", 54, 58, 63, 255),
            input: Color::srgb_token("input", 54, 58, 63, 255),
            ring: Color::srgb_token("ring", 0, 144, 255, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 255, 255, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 112, 184, 255, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 105, 110, 119, 120),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 176, 180, 186, 220),

            selection_bg: Color::srgb_token("selection-bg", 0, 144, 255, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 105, 110, 119, 64),
        }
    }

    /// Radix Colors-inspired slate + blue light palette.
    pub const fn radix_slate_blue_light() -> Self {
        Self {
            background: Color::srgb_token("background", 252, 252, 253, 255),
            foreground: Color::srgb_token("foreground", 28, 32, 36, 255),

            card: Color::srgb_token("card", 255, 255, 255, 255),
            card_foreground: Color::srgb_token("card-foreground", 28, 32, 36, 255),

            popover: Color::srgb_token("popover", 255, 255, 255, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 28, 32, 36, 255),

            primary: Color::srgb_token("primary", 0, 144, 255, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 255, 255, 255, 255),

            secondary: Color::srgb_token("secondary", 240, 240, 243, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 28, 32, 36, 255),

            muted: Color::srgb_token("muted", 240, 240, 243, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 96, 100, 108, 255),

            accent: Color::srgb_token("accent", 230, 244, 254, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 13, 116, 206, 255),

            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 255, 255, 255, 255),

            border: Color::srgb_token("border", 217, 217, 224, 255),
            input: Color::srgb_token("input", 205, 206, 214, 255),
            ring: Color::srgb_token("ring", 0, 144, 255, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 25, 59, 45, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 255, 255, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 13, 116, 206, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 139, 141, 152, 90),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 96, 100, 108, 220),

            selection_bg: Color::srgb_token("selection-bg", 0, 144, 255, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 139, 141, 152, 56),
        }
    }

    /// Return a Radix slate + blue palette for the requested luminance mode.
    pub const fn radix_slate_blue(is_dark: bool) -> Self {
        if is_dark {
            Self::radix_slate_blue_dark()
        } else {
            Self::radix_slate_blue_light()
        }
    }

    /// Radix Colors-inspired sand + amber dark palette — warm sepia
    /// neutrals from `sand` paired with a bright `amber` accent. The
    /// amber-9 yellow is luminous enough that primary action surfaces
    /// take a dark foreground; everywhere else follows the same role
    /// mapping as the slate + blue variant.
    pub const fn radix_sand_amber_dark() -> Self {
        Self {
            background: Color::srgb_token("background", 17, 17, 16, 255),
            foreground: Color::srgb_token("foreground", 238, 238, 236, 255),

            card: Color::srgb_token("card", 25, 25, 24, 255),
            card_foreground: Color::srgb_token("card-foreground", 238, 238, 236, 255),

            popover: Color::srgb_token("popover", 25, 25, 24, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 238, 238, 236, 255),

            primary: Color::srgb_token("primary", 255, 197, 61, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 33, 32, 28, 255),

            secondary: Color::srgb_token("secondary", 34, 34, 33, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 238, 238, 236, 255),

            muted: Color::srgb_token("muted", 34, 34, 33, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 181, 179, 173, 255),

            accent: Color::srgb_token("accent", 48, 32, 8, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 255, 202, 22, 255),

            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 255, 255, 255, 255),

            border: Color::srgb_token("border", 59, 58, 55, 255),
            input: Color::srgb_token("input", 59, 58, 55, 255),
            ring: Color::srgb_token("ring", 255, 197, 61, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 255, 255, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 255, 202, 22, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 111, 109, 102, 120),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 181, 179, 173, 220),

            selection_bg: Color::srgb_token("selection-bg", 255, 197, 61, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 111, 109, 102, 64),
        }
    }

    /// Radix Colors-inspired sand + amber light palette.
    pub const fn radix_sand_amber_light() -> Self {
        Self {
            background: Color::srgb_token("background", 253, 253, 252, 255),
            foreground: Color::srgb_token("foreground", 33, 32, 28, 255),

            card: Color::srgb_token("card", 255, 255, 255, 255),
            card_foreground: Color::srgb_token("card-foreground", 33, 32, 28, 255),

            popover: Color::srgb_token("popover", 255, 255, 255, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 33, 32, 28, 255),

            primary: Color::srgb_token("primary", 255, 197, 61, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 33, 32, 28, 255),

            secondary: Color::srgb_token("secondary", 241, 240, 239, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 33, 32, 28, 255),

            muted: Color::srgb_token("muted", 241, 240, 239, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 99, 99, 94, 255),

            accent: Color::srgb_token("accent", 255, 247, 194, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 171, 100, 0, 255),

            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 255, 255, 255, 255),

            border: Color::srgb_token("border", 218, 217, 214, 255),
            input: Color::srgb_token("input", 207, 206, 202, 255),
            ring: Color::srgb_token("ring", 255, 197, 61, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 25, 59, 45, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 255, 255, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 171, 100, 0, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 141, 141, 134, 90),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 99, 99, 94, 220),

            selection_bg: Color::srgb_token("selection-bg", 255, 197, 61, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 141, 141, 134, 56),
        }
    }

    /// Return a Radix sand + amber palette for the requested luminance mode.
    pub const fn radix_sand_amber(is_dark: bool) -> Self {
        if is_dark {
            Self::radix_sand_amber_dark()
        } else {
            Self::radix_sand_amber_light()
        }
    }

    /// Radix Colors-inspired mauve + violet dark palette — purple-tinged
    /// neutrals from `mauve` paired with a `violet` accent. Same role
    /// mapping as the slate + blue variant; the violet-9 base is deep
    /// enough that primary action surfaces stay readable with a white
    /// foreground.
    pub const fn radix_mauve_violet_dark() -> Self {
        Self {
            background: Color::srgb_token("background", 18, 17, 19, 255),
            foreground: Color::srgb_token("foreground", 238, 238, 240, 255),

            card: Color::srgb_token("card", 26, 25, 27, 255),
            card_foreground: Color::srgb_token("card-foreground", 238, 238, 240, 255),

            popover: Color::srgb_token("popover", 26, 25, 27, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 238, 238, 240, 255),

            primary: Color::srgb_token("primary", 110, 86, 207, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 255, 255, 255, 255),

            secondary: Color::srgb_token("secondary", 35, 34, 37, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 238, 238, 240, 255),

            muted: Color::srgb_token("muted", 35, 34, 37, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 181, 178, 188, 255),

            accent: Color::srgb_token("accent", 41, 31, 67, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 186, 167, 255, 255),

            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 255, 255, 255, 255),

            border: Color::srgb_token("border", 60, 57, 63, 255),
            input: Color::srgb_token("input", 60, 57, 63, 255),
            ring: Color::srgb_token("ring", 110, 86, 207, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 255, 255, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 186, 167, 255, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 111, 109, 120, 120),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 181, 178, 188, 220),

            selection_bg: Color::srgb_token("selection-bg", 110, 86, 207, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 111, 109, 120, 64),
        }
    }

    /// Radix Colors-inspired mauve + violet light palette.
    pub const fn radix_mauve_violet_light() -> Self {
        Self {
            background: Color::srgb_token("background", 253, 252, 253, 255),
            foreground: Color::srgb_token("foreground", 33, 31, 38, 255),

            card: Color::srgb_token("card", 255, 255, 255, 255),
            card_foreground: Color::srgb_token("card-foreground", 33, 31, 38, 255),

            popover: Color::srgb_token("popover", 255, 255, 255, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 33, 31, 38, 255),

            primary: Color::srgb_token("primary", 110, 86, 207, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 255, 255, 255, 255),

            secondary: Color::srgb_token("secondary", 242, 239, 243, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 33, 31, 38, 255),

            muted: Color::srgb_token("muted", 242, 239, 243, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 101, 99, 109, 255),

            accent: Color::srgb_token("accent", 244, 240, 254, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 101, 80, 185, 255),

            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 255, 255, 255, 255),

            border: Color::srgb_token("border", 219, 216, 224, 255),
            input: Color::srgb_token("input", 208, 205, 215, 255),
            ring: Color::srgb_token("ring", 110, 86, 207, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 25, 59, 45, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 255, 255, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 101, 80, 185, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 142, 140, 153, 90),
            scrollbar_thumb_fill_active: Color::srgb_token("scrollbar-thumb-active", 101, 99, 109, 220),

            selection_bg: Color::srgb_token("selection-bg", 110, 86, 207, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 142, 140, 153, 56),
        }
    }

    /// Return a Radix mauve + violet palette for the requested luminance mode.
    pub const fn radix_mauve_violet(is_dark: bool) -> Self {
        if is_dark {
            Self::radix_mauve_violet_dark()
        } else {
            Self::radix_mauve_violet_light()
        }
    }

    /// Replace `c`'s rgb with this palette's value for its token name,
    /// keeping the token name and the input alpha. Colors with no token
    /// name pass through unchanged — that includes raw `Color::rgba`
    /// values *and* the output of rgb-modifying ops (which strip the
    /// token name; see [`Color`] module docs). Colors whose token isn't
    /// a palette member (theme-invariant tokens like `text-on-solid-dark`,
    /// unknown tokens) also pass through unchanged.
    ///
    /// Alpha is taken from the input so [`Color::with_alpha`] overrides
    /// survive resolution.
    pub fn resolve(&self, c: Color) -> Color {
        match c.token.and_then(|name| self.lookup(name)) {
            Some(swap) => Color {
                r: swap.r,
                g: swap.g,
                b: swap.b,
                a: c.a,
                space: swap.space,
                token: c.token,
            },
            None => c,
        }
    }

    /// Resolve a token name to its rgba in this palette. Returns `None`
    /// for theme-invariant tokens (the renderer falls back to the
    /// `Color`'s baked rgba) and for unknown names.
    pub fn lookup(&self, token: &str) -> Option<Color> {
        Some(match token {
            "background" => self.background,
            "foreground" => self.foreground,
            "card" => self.card,
            "card-foreground" => self.card_foreground,
            "popover" => self.popover,
            "popover-foreground" => self.popover_foreground,
            "primary" => self.primary,
            "primary-foreground" => self.primary_foreground,
            "secondary" => self.secondary,
            "secondary-foreground" => self.secondary_foreground,
            "muted" => self.muted,
            "muted-foreground" => self.muted_foreground,
            "accent" => self.accent,
            "accent-foreground" => self.accent_foreground,
            "destructive" => self.destructive,
            "destructive-foreground" => self.destructive_foreground,
            "border" => self.border,
            "input" => self.input,
            "ring" => self.ring,
            "success" => self.success,
            "success-foreground" => self.success_foreground,
            "warning" => self.warning,
            "warning-foreground" => self.warning_foreground,
            "info" => self.info,
            "info-foreground" => self.info_foreground,
            "overlay-scrim" => self.overlay_scrim,
            "link-foreground" => self.link_foreground,
            "scrollbar-thumb" => self.scrollbar_thumb_fill,
            "scrollbar-thumb-active" => self.scrollbar_thumb_fill_active,
            "selection-bg" => self.selection_bg,
            "selection-bg-unfocused" => self.selection_bg_unfocused,
            _ => return None,
        })
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::aetna_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens;

    #[test]
    fn dark_lookup_round_trips() {
        let p = Palette::aetna_dark();
        let bg = p.lookup("background").expect("background present");
        assert_eq!(bg, p.background);
    }

    #[test]
    fn aetna_dark_matches_token_fallbacks() {
        let palette = Palette::aetna_dark();
        for token in [
            tokens::BACKGROUND,
            tokens::FOREGROUND,
            tokens::CARD,
            tokens::CARD_FOREGROUND,
            tokens::POPOVER,
            tokens::POPOVER_FOREGROUND,
            tokens::PRIMARY,
            tokens::PRIMARY_FOREGROUND,
            tokens::SECONDARY,
            tokens::SECONDARY_FOREGROUND,
            tokens::MUTED,
            tokens::MUTED_FOREGROUND,
            tokens::ACCENT,
            tokens::ACCENT_FOREGROUND,
            tokens::DESTRUCTIVE,
            tokens::DESTRUCTIVE_FOREGROUND,
            tokens::BORDER,
            tokens::INPUT,
            tokens::RING,
            tokens::SUCCESS,
            tokens::SUCCESS_FOREGROUND,
            tokens::WARNING,
            tokens::WARNING_FOREGROUND,
            tokens::INFO,
            tokens::INFO_FOREGROUND,
            tokens::OVERLAY_SCRIM,
            tokens::LINK_FOREGROUND,
            tokens::SCROLLBAR_THUMB_FILL,
            tokens::SCROLLBAR_THUMB_FILL_ACTIVE,
            tokens::SELECTION_BG,
            tokens::SELECTION_BG_UNFOCUSED,
        ] {
            assert_eq!(palette.resolve(token), token);
        }
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let p = Palette::aetna_dark();
        assert!(p.lookup("not-a-token").is_none());
    }

    #[test]
    fn removed_legacy_tokens_not_in_palette() {
        let p = Palette::aetna_dark();
        assert!(p.lookup("bg-app").is_none());
        assert!(p.lookup("bg-card").is_none());
        assert!(p.lookup("bg-muted").is_none());
        assert!(p.lookup("text-on-solid-dark").is_none());
        assert!(p.lookup("text-on-solid-light").is_none());
    }

    #[test]
    fn resolve_passes_through_unrecognized() {
        let p = Palette::aetna_dark();
        let raw = Color::srgb_u8a(1, 2, 3, 4);
        assert_eq!(p.resolve(raw), raw);
        let invariant = Color::srgb_token("text-on-solid-dark", 8, 16, 25, 255);
        assert_eq!(p.resolve(invariant), invariant);
    }

    #[test]
    fn resolve_preserves_alpha_override() {
        let p = Palette::aetna_dark();
        let translucent = p.card.with_alpha_u8(120);
        let resolved = p.resolve(translucent);
        // rgb tracks the palette's card, alpha tracks the override.
        assert_eq!(
            (resolved.r, resolved.g, resolved.b),
            (p.card.r, p.card.g, p.card.b)
        );
        assert_eq!(resolved.to_srgb_u8a()[3], 120);
        assert_eq!(resolved.token, Some("card"));
    }

    #[test]
    fn aetna_light_differs_from_aetna_dark() {
        let dark = Palette::aetna_dark();
        let light = Palette::aetna_light();
        // background is one of the tokens that visibly inverts.
        assert_ne!(
            (dark.background.r, dark.background.g, dark.background.b),
            (light.background.r, light.background.g, light.background.b),
        );
        // Text foreground also inverts.
        assert_ne!(
            (dark.foreground.r, dark.foreground.g, dark.foreground.b),
            (light.foreground.r, light.foreground.g, light.foreground.b),
        );
        // Token names match — same vocabulary, different rgb.
        assert_eq!(dark.background.token, light.background.token);
    }

    #[test]
    fn resolve_against_light_swaps_rgb() {
        let light = Palette::aetna_light();
        // A token-tagged color authored against dark values resolves to
        // the light palette's rgb.
        let dark_card = Color::srgb_token("card", 23, 26, 33, 255);
        let resolved = light.resolve(dark_card);
        assert_eq!(
            (resolved.r, resolved.g, resolved.b),
            (light.card.r, light.card.g, light.card.b),
        );
    }

    #[test]
    fn rgb_ops_strip_token_so_resolve_passes_through() {
        // State animations (hover lighten, press darken) build colors
        // via .darken/.lighten/.mix on token-tagged base colors. Those
        // ops strip the token, so resolve passes them through unchanged
        // and the per-frame derivation wins — which is what we want.
        let p = Palette::aetna_dark();
        let darkened = p.muted.darken(0.5);
        assert_eq!(darkened.token, None);
        let resolved = p.resolve(darkened);
        assert_eq!(resolved, darkened);
    }
}
