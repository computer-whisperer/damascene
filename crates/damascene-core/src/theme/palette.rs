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
//! `theme/mod.rs:apply_role_material` is the one library site that runs an
//! rgb op against a token (`MUTED.darken(0.08)` for the Sunken role) and it
//! palette-resolves the base *before* the op, so the hazard is contained
//! there. State animations don't need that treatment — they want the
//! per-frame derivation to win, not the palette default.
//!
//! The core vocabulary is intentionally close to shadcn/ui: background /
//! foreground pairs for surfaces and actions, plus border, input, ring,
//! and semantic status roles. Link, scrollbar, overlay, and selection
//! tokens are component/domain extensions.
//!
//! The vocabulary is open at the edges: [`Palette::with_token`] registers
//! any additional name, the way `var(--layer-01)` works on the web once
//! you declare it. Registered names swap with the palette; unregistered
//! ones keep passing through with their baked rgba, which is how
//! theme-invariant tokens stay theme-invariant.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::tree::Color;
use std::collections::BTreeMap;

/// Runtime backing for the design-token color vocabulary.
///
/// One field per theme-variant token, plus [`Palette::extra`] for names a
/// design system mints itself.
#[derive(Clone, Debug)]
pub struct Palette {
    // Core shadcn-shaped semantic colors.
    /// App-level page background — backs the `background` token.
    pub background: Color,
    /// Default text color — backs the `foreground` token.
    pub foreground: Color,

    /// Card surface fill — backs the `card` token.
    pub card: Color,
    /// Text color on card surfaces — backs the `card-foreground` token.
    pub card_foreground: Color,

    /// Popover/menu/tooltip surface fill — backs the `popover` token.
    pub popover: Color,
    /// Text color on popover surfaces — backs the `popover-foreground` token.
    pub popover_foreground: Color,

    /// Primary action color — backs the `primary` token.
    pub primary: Color,
    /// Text/icon color on solid primary fills — backs the `primary-foreground` token.
    pub primary_foreground: Color,

    /// Secondary action surface fill — backs the `secondary` token.
    pub secondary: Color,
    /// Text color on secondary fills — backs the `secondary-foreground` token.
    pub secondary_foreground: Color,

    /// Neutral muted surface fill — backs the `muted` token.
    pub muted: Color,
    /// De-emphasized text color — backs the `muted-foreground` token.
    pub muted_foreground: Color,

    /// Hover/current-item highlight surface — backs the `accent` token.
    pub accent: Color,
    /// Text color on accent surfaces — backs the `accent-foreground` token.
    pub accent_foreground: Color,

    /// Destructive action color — backs the `destructive` token.
    pub destructive: Color,
    /// Text color on solid destructive fills — backs the `destructive-foreground` token.
    pub destructive_foreground: Color,

    /// Default border/divider stroke — backs the `border` token.
    pub border: Color,
    /// Input-field border stroke — backs the `input` token.
    pub input: Color,
    /// Keyboard-focus ring stroke — backs the `ring` token.
    pub ring: Color,

    /// Positive status color — backs the `success` token.
    pub success: Color,
    /// Text color on solid success fills — backs the `success-foreground` token.
    pub success_foreground: Color,
    /// Cautionary status color — backs the `warning` token.
    pub warning: Color,
    /// Text color on solid warning fills — backs the `warning-foreground` token.
    pub warning_foreground: Color,
    /// Informational status color — backs the `info` token.
    pub info: Color,
    /// Text color on solid info fills — backs the `info-foreground` token.
    pub info_foreground: Color,

    // Text-grade status tones. The base status colors are fill-grade —
    // tuned to sit *under* text on solid surfaces — and fail WCAG AA
    // when used *as* text on tinted or plain app surfaces (the dark
    // `destructive`, a deep red-900, is nearly invisible as text on
    // its own tint). These are the readable counterparts, the
    // tailwind `text-red-400` / `dark:`-vs-light split made explicit:
    // what tinted badges, text-only status treatments, and syntax
    // highlighting color their text with. Every value is AA-checked
    // against its 15%-alpha tint, the card, and the page by the
    // `tinted_status_text_meets_aa_in_every_palette` lint test.
    /// Destructive-colored *text* on tints/surfaces — backs the
    /// `destructive-tint-foreground` token.
    pub destructive_tint_foreground: Color,
    /// Success-colored *text* on tints/surfaces — backs the
    /// `success-tint-foreground` token.
    pub success_tint_foreground: Color,
    /// Warning-colored *text* on tints/surfaces — backs the
    /// `warning-tint-foreground` token.
    pub warning_tint_foreground: Color,
    /// Info-colored *text* on tints/surfaces — backs the
    /// `info-tint-foreground` token.
    pub info_tint_foreground: Color,

    // Extensions.
    /// Dimming scrim behind modal overlays — backs the `overlay-scrim` token.
    pub overlay_scrim: Color,
    /// Hyperlink text color — backs the `link-foreground` token.
    pub link_foreground: Color,

    /// Solid instrument-chip material — backs the `badge` token. See
    /// the "Picking a `badge`" section on this impl for how each stock
    /// variant's value is chosen.
    pub badge: Color,
    /// Text/icon color on a solid [`Palette::badge`] fill — backs the
    /// `badge-foreground` token.
    pub badge_foreground: Color,

    /// Idle scrollbar thumb fill — backs the `scrollbar-thumb` token.
    pub scrollbar_thumb_fill: Color,
    /// Hovered/dragged scrollbar thumb fill — backs the `scrollbar-thumb-active` token.
    pub scrollbar_thumb_fill_active: Color,

    /// Text-selection band while the input is focused — backs the `selection-bg` token.
    pub selection_bg: Color,
    /// Text-selection band while the input lacks focus — backs the `selection-bg-unfocused` token.
    pub selection_bg_unfocused: Color,

    /// Token names minted outside the vocabulary above, registered via
    /// [`Palette::with_token`]. Consulted by [`Palette::lookup`] only
    /// after the built-in names miss, so a stock name can never be
    /// shadowed.
    pub extra: BTreeMap<&'static str, Color>,
}

impl Palette {
    // ---- Picking a `badge` -------------------------------------------
    //
    // [`crate::tokens::BADGE`] is the solid instrument-chip material.
    // Its one job is to be *notably* offset from every surface in the
    // ramp: VS Code's `badge.background` (`#616161`) sits 2.9:1 above
    // its `#181818` chrome and 2.7:1 above its `#1F1F1F` editor, which
    // is what makes a count chip read as an object on the panel rather
    // than a slightly different patch of panel. A chip pointed at a
    // near-surface slot is the failure mode this slot exists to end:
    // the workbench theme used to paint chips in `secondary`, measured
    // at **1.11:1** over `card` — invisible.
    //
    // Every stock variant therefore takes its **neutral ramp's solid
    // step** — Radix step 9, zinc-500 for the shadcn ramp. That is the
    // step each ramp already designates for opaque non-text fills, and
    // (not by accident) the same value each palette already spends on
    // `scrollbar-thumb`, the other opaque neutral in the vocabulary.
    // Neutral, not accent: a chip is instrumentation, and the accent is
    // spent sparsely elsewhere.
    //
    // `badge_foreground` is then whichever end of the ramp wins the
    // contrast test against that fill — near-white on the dark
    // variants, near-black on the light ones, where a light-mode step 9
    // is too pale for white text. Measured ratios per variant are in
    // each constructor's comment; every one clears 3:1 material over
    // `card` and 4.5:1 label over the fill.

    // ---- Picking a status `*-foreground` -----------------------------
    //
    // `primary`, `destructive`, `success`, `warning` and `info` are the
    // five **solid** roles: `button(..).info()`, a tinted chip, a toast
    // header all paint the fill and set the label to the paired
    // `*-foreground`. That label is caption-to-body sized, so the pair
    // is held to WCAG's small-text floor, **4.5:1** — the same floor
    // `badge_foreground` is solved for one section up.
    //
    // The fills are ratified and do not move. What moves is the
    // foreground, and the rule is: **take whichever end of the fill's
    // own hue ramp wins**, not "white on color". That reflex is what
    // this section exists to correct. Every one of these ramps spends
    // its step 9 / -500 rung on the solid fill, and a step 9 is
    // engineered for ~3:1 against white — a *large-text* / non-text
    // floor. Measured before this retune: Radix blue-9 `#0090FF` on
    // white was 3.26:1, red-9 `#E5484D` 3.91:1, Tailwind blue-500
    // `#3B82F6` under near-white 3.38:1. All three are legible-looking
    // and all three miss AA for a chip label.
    //
    // So a bright fill takes a **hue-tinted near-black** label, which
    // is the move `warning_foreground` (`#4F3422` on amber-9, 7.2:1)
    // and the dark variants' `success_foreground` (`#0E1512` on
    // green-9, 5.9:1) already made. Where the ramp publishes a dark
    // end deep enough, that step is used verbatim: Radix's dark-scale
    // step 1 (`#0D1520` blue, `#191111` red, `#0E1512` green). Where
    // it does not — Tailwind's ramp bottoms out at -950, and blue-950
    // `#172554` only reaches 4.00:1 on blue-500 — the -950 step is
    // taken one stop further down its own line toward black
    // (`#172554` × 0.5 → `#0C122A`, `#450A0A` × 0.6 → `#290606`),
    // which keeps the hue and buys the margin.
    //
    // Only a genuinely dark fill keeps a light label: zinc's
    // `destructive` is red-900 `#7F1D1D` (near-white, 9.6:1), and
    // violet-9 `#6E56CF` is deep enough that white still wins at
    // 5.4:1 — its black end would only reach 3.9:1.
    //
    // Measured ratios per variant are in each constructor's comment,
    // and `status_foregrounds_clear_the_small_text_floor` asserts the
    // floor across every stock variant so an edit here cannot regress
    // it. The one exception is outside this file:
    // `damascene-workbench`'s `dark_modern_palette` **transcribes** VS
    // Code's own keys, where `button.foreground` is `#FFFFFF` over
    // `#F85149`/`#2EA043` at 3.35:1/3.37:1. The calibration plan
    // forbids inventing values there, so the reference wins and the
    // workbench test records by how much — the same precedent the chip
    // material set.

    /// Damascene's default dark palette, copied from shadcn/ui's zinc dark
    /// theme scaffold. These rgba values also serve as the compile-time
    /// fallback baked into the constants in [`crate::tokens`].
    pub const fn damascene_dark() -> Self {
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

            // Status labels, all on the ramp's dark end (see "Picking a
            // status `*-foreground`"): success green-950 5.88:1,
            // warning amber-950 6.97:1. `info` is the one that had to
            // go past the published ramp — blue-950 `#172554` reaches
            // only 4.00:1 on blue-500, so the label is that step taken
            // half-way further to black, 5.03:1 (was `#EFF6FF`,
            // **3.38:1**). `destructive` above keeps its near-white
            // label because red-900 is a genuinely dark fill (9.60:1).
            success: Color::srgb_token("success", 16, 185, 129, 255),
            success_foreground: Color::srgb_token("success-foreground", 5, 46, 22, 255),
            warning: Color::srgb_token("warning", 245, 158, 11, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 69, 26, 3, 255),
            info: Color::srgb_token("info", 59, 130, 246, 255),
            info_foreground: Color::srgb_token("info-foreground", 12, 18, 42, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                248,
                113,
                113,
                255,
            ),
            success_tint_foreground: Color::srgb_token(
                "success-tint-foreground",
                52,
                211,
                153,
                255,
            ),
            warning_tint_foreground: Color::srgb_token(
                "warning-tint-foreground",
                251,
                191,
                36,
                255,
            ),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 96, 165, 250, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 96, 165, 250, 255),

            // zinc-500 `#71717A` — 4.1:1 over `card`, the brightest
            // chip step in the stock set because zinc's `card` is the
            // darkest ground (`#09090B`). Label is zinc-50, 4.6:1.
            badge: Color::srgb_token("badge", 113, 113, 122, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 250, 250, 250, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 113, 113, 122, 120),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                161,
                161,
                170,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 96, 165, 250, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 113, 113, 122, 64),

            extra: BTreeMap::new(),
        }
    }

    /// Damascene's default light palette, copied from shadcn/ui's zinc light
    /// theme scaffold.
    pub const fn damascene_light() -> Self {
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

            // Light mode's `destructive` is red-**500**, not the dark
            // variant's red-900, so the near-white label that works
            // there measured **3.61:1** here. Same treatment as `info`
            // in the dark variant: red-950 `#450A0A` still only
            // reaches 4.29:1, so the label is that step taken further
            // down its own line toward black — 4.97:1.
            destructive: Color::srgb_token("destructive", 239, 68, 68, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 41, 6, 6, 255),

            border: Color::srgb_token("border", 228, 228, 231, 255),
            input: Color::srgb_token("input", 228, 228, 231, 255),
            ring: Color::srgb_token("ring", 24, 24, 27, 255),

            success: Color::srgb_token("success", 16, 185, 129, 255),
            success_foreground: Color::srgb_token("success-foreground", 5, 46, 22, 255),
            warning: Color::srgb_token("warning", 245, 158, 11, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 69, 26, 3, 255),
            // success 5.88:1, warning 6.97:1 — unchanged. `info` is the
            // one slot where light and dark disagree on the *direction*
            // of the label, and legitimately: this is blue-**600**,
            // dark enough that the near-white label clears at 4.75:1,
            // where the dark variant's blue-500 needed the black end.
            info: Color::srgb_token("info", 37, 99, 235, 255),
            info_foreground: Color::srgb_token("info-foreground", 239, 246, 255, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                185,
                28,
                28,
                255,
            ),
            success_tint_foreground: Color::srgb_token("success-tint-foreground", 4, 120, 87, 255),
            warning_tint_foreground: Color::srgb_token("warning-tint-foreground", 146, 64, 14, 255),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 29, 78, 216, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 37, 99, 235, 255),

            // The same zinc-500 step, now reading *dark* against white
            // surfaces — 4.8:1 over `card`. zinc's mid step is dark
            // enough that the near-white label still wins (4.6:1 vs
            // 4.1:1 for zinc-950), so light and dark agree here where
            // the Radix variants flip.
            badge: Color::srgb_token("badge", 113, 113, 122, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 250, 250, 250, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 113, 113, 122, 90),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                82,
                82,
                91,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 37, 99, 235, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 113, 113, 122, 56),

            extra: BTreeMap::new(),
        }
    }

    /// Radix Colors-inspired slate + blue dark palette. The neutral
    /// surfaces come from Radix `slate`; the action, link, focus, and
    /// selected/current treatments come from Radix `blue`. This keeps
    /// the original Damascene black/blue feel, but uses a complete public
    /// scale rather than hand-picked ad hoc values.
    pub const fn radix_slate_blue_dark() -> Self {
        Self {
            background: Color::srgb_token("background", 17, 17, 19, 255),
            foreground: Color::srgb_token("foreground", 237, 238, 240, 255),

            card: Color::srgb_token("card", 24, 25, 27, 255),
            card_foreground: Color::srgb_token("card-foreground", 237, 238, 240, 255),

            popover: Color::srgb_token("popover", 24, 25, 27, 255),
            popover_foreground: Color::srgb_token("popover-foreground", 237, 238, 240, 255),

            // Blue-9 is the ramp's solid step, engineered for ~3:1
            // against white: the white label measured **3.26:1**. The
            // label takes Radix's blue **dark-1** (`#0D1520`) instead
            // — 5.62:1, and the same value `info-foreground` below
            // takes, since both slots are this one blue.
            primary: Color::srgb_token("primary", 0, 144, 255, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 13, 21, 32, 255),

            secondary: Color::srgb_token("secondary", 33, 34, 37, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 237, 238, 240, 255),

            muted: Color::srgb_token("muted", 33, 34, 37, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 176, 180, 186, 255),

            accent: Color::srgb_token("accent", 13, 40, 71, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 112, 184, 255, 255),

            // Red-9 under a white label measured **3.91:1**. The label
            // takes Radix red **dark-1** (`#191111`) — 4.75:1.
            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 25, 17, 17, 255),

            border: Color::srgb_token("border", 54, 58, 63, 255),
            input: Color::srgb_token("input", 54, 58, 63, 255),
            ring: Color::srgb_token("ring", 0, 144, 255, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            // Blue-9 under a white label measured **3.26:1**. The label
            // takes Radix blue **dark-1** (`#0D1520`) — 5.62:1.
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 13, 21, 32, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                255,
                149,
                146,
                255,
            ),
            success_tint_foreground: Color::srgb_token(
                "success-tint-foreground",
                61,
                214,
                140,
                255,
            ),
            warning_tint_foreground: Color::srgb_token(
                "warning-tint-foreground",
                255,
                202,
                22,
                255,
            ),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 112, 184, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 112, 184, 255, 255),

            // Radix slate-9 `#696E77` — the ramp's solid step, 3.4:1
            // over `card` (`#18191B`) and 3.7:1 over `background`.
            // Label is white, 5.1:1; slate-12 would only reach 4.4:1.
            badge: Color::srgb_token("badge", 105, 110, 119, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 255, 255, 255, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 105, 110, 119, 120),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                176,
                180,
                186,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 0, 144, 255, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 105, 110, 119, 64),

            extra: BTreeMap::new(),
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

            // Same blue-9 as the dark variant, same **3.26:1** white
            // label, same fix: Radix blue dark-1, 5.62:1.
            primary: Color::srgb_token("primary", 0, 144, 255, 255),
            primary_foreground: Color::srgb_token("primary-foreground", 13, 21, 32, 255),

            secondary: Color::srgb_token("secondary", 240, 240, 243, 255),
            secondary_foreground: Color::srgb_token("secondary-foreground", 28, 32, 36, 255),

            muted: Color::srgb_token("muted", 240, 240, 243, 255),
            muted_foreground: Color::srgb_token("muted-foreground", 96, 100, 108, 255),

            accent: Color::srgb_token("accent", 230, 244, 254, 255),
            accent_foreground: Color::srgb_token("accent-foreground", 13, 116, 206, 255),

            // Red-9 under a white label measured **3.91:1**. The label
            // takes Radix red **dark-1** (`#191111`) — 4.75:1.
            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 25, 17, 17, 255),

            border: Color::srgb_token("border", 217, 217, 224, 255),
            input: Color::srgb_token("input", 205, 206, 214, 255),
            ring: Color::srgb_token("ring", 0, 144, 255, 255),

            // The light variants used to take green-12 (`#193B2D`)
            // here, one rung short at **3.90:1**. They now take the
            // same green **dark-1** (`#0E1512`) the dark variants use
            // — 5.86:1. Green-9 is one value across the whole stock
            // set, so its label may as well be too.
            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            // Blue-9 under a white label measured **3.26:1**. The label
            // takes Radix blue **dark-1** (`#0D1520`) — 5.62:1.
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 13, 21, 32, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                206,
                44,
                49,
                255,
            ),
            success_tint_foreground: Color::srgb_token("success-tint-foreground", 30, 120, 80, 255),
            warning_tint_foreground: Color::srgb_token("warning-tint-foreground", 160, 93, 0, 255),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 12, 108, 190, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 13, 116, 206, 255),

            // Radix slate light-9 `#8B8D98` — 3.3:1 over the white
            // `card`. The direction flips here: a light-mode step 9 is
            // too pale for white text (3.3:1), so the label takes the
            // ramp's dark end (`foreground`, 5.0:1) instead.
            badge: Color::srgb_token("badge", 139, 141, 152, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 28, 32, 36, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 139, 141, 152, 90),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                96,
                100,
                108,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 0, 144, 255, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 139, 141, 152, 56),

            extra: BTreeMap::new(),
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

            // Red-9 under a white label measured **3.91:1**. The label
            // takes Radix red **dark-1** (`#191111`) — 4.75:1.
            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 25, 17, 17, 255),

            border: Color::srgb_token("border", 59, 58, 55, 255),
            input: Color::srgb_token("input", 59, 58, 55, 255),
            ring: Color::srgb_token("ring", 255, 197, 61, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            // Blue-9 under a white label measured **3.26:1**. The label
            // takes Radix blue **dark-1** (`#0D1520`) — 5.62:1.
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 13, 21, 32, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                255,
                149,
                146,
                255,
            ),
            success_tint_foreground: Color::srgb_token(
                "success-tint-foreground",
                61,
                214,
                140,
                255,
            ),
            warning_tint_foreground: Color::srgb_token(
                "warning-tint-foreground",
                255,
                202,
                22,
                255,
            ),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 112, 184, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 255, 202, 22, 255),

            // Radix sand-9 `#6F6D66` — the warm neutral's solid step,
            // 3.4:1 over `card`. Neutral rather than amber on purpose:
            // a chip is instrumentation, and this ramp spends its
            // luminous amber on `primary` alone. Label is white, 5.2:1.
            badge: Color::srgb_token("badge", 111, 109, 102, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 255, 255, 255, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 111, 109, 102, 120),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                181,
                179,
                173,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 255, 197, 61, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 111, 109, 102, 64),

            extra: BTreeMap::new(),
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

            // Red-9 under a white label measured **3.91:1**. The label
            // takes Radix red **dark-1** (`#191111`) — 4.75:1.
            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 25, 17, 17, 255),

            border: Color::srgb_token("border", 218, 217, 214, 255),
            input: Color::srgb_token("input", 207, 206, 202, 255),
            ring: Color::srgb_token("ring", 255, 197, 61, 255),

            // The light variants used to take green-12 (`#193B2D`)
            // here, one rung short at **3.90:1**. They now take the
            // same green **dark-1** (`#0E1512`) the dark variants use
            // — 5.86:1. Green-9 is one value across the whole stock
            // set, so its label may as well be too.
            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            // Blue-9 under a white label measured **3.26:1**. The label
            // takes Radix blue **dark-1** (`#0D1520`) — 5.62:1.
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 13, 21, 32, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                206,
                44,
                49,
                255,
            ),
            success_tint_foreground: Color::srgb_token("success-tint-foreground", 30, 120, 80, 255),
            warning_tint_foreground: Color::srgb_token("warning-tint-foreground", 160, 93, 0, 255),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 12, 108, 190, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 171, 100, 0, 255),

            // Radix sand light-9 `#8D8D86` — 3.3:1 over the white
            // `card`, dark label (4.9:1), same flip as slate light.
            badge: Color::srgb_token("badge", 141, 141, 134, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 33, 32, 28, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 141, 141, 134, 90),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                99,
                99,
                94,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 255, 197, 61, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 141, 141, 134, 56),

            extra: BTreeMap::new(),
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

            // Red-9 under a white label measured **3.91:1**. The label
            // takes Radix red **dark-1** (`#191111`) — 4.75:1.
            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 25, 17, 17, 255),

            border: Color::srgb_token("border", 60, 57, 63, 255),
            input: Color::srgb_token("input", 60, 57, 63, 255),
            ring: Color::srgb_token("ring", 110, 86, 207, 255),

            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            // Blue-9 under a white label measured **3.26:1**. The label
            // takes Radix blue **dark-1** (`#0D1520`) — 5.62:1.
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 13, 21, 32, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                255,
                149,
                146,
                255,
            ),
            success_tint_foreground: Color::srgb_token(
                "success-tint-foreground",
                61,
                214,
                140,
                255,
            ),
            warning_tint_foreground: Color::srgb_token(
                "warning-tint-foreground",
                255,
                202,
                22,
                255,
            ),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 112, 184, 255, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 204),
            link_foreground: Color::srgb_token("link-foreground", 186, 167, 255, 255),

            // Radix mauve-9 `#6F6D78` — 3.5:1 over `card`, white
            // label at 5.1:1. Neutral, not violet, for the same reason
            // the sand variant stays neutral.
            badge: Color::srgb_token("badge", 111, 109, 120, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 255, 255, 255, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 111, 109, 120, 120),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                181,
                178,
                188,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 110, 86, 207, 96),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 111, 109, 120, 64),

            extra: BTreeMap::new(),
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

            // Red-9 under a white label measured **3.91:1**. The label
            // takes Radix red **dark-1** (`#191111`) — 4.75:1.
            destructive: Color::srgb_token("destructive", 229, 72, 77, 255),
            destructive_foreground: Color::srgb_token("destructive-foreground", 25, 17, 17, 255),

            border: Color::srgb_token("border", 219, 216, 224, 255),
            input: Color::srgb_token("input", 208, 205, 215, 255),
            ring: Color::srgb_token("ring", 110, 86, 207, 255),

            // The light variants used to take green-12 (`#193B2D`)
            // here, one rung short at **3.90:1**. They now take the
            // same green **dark-1** (`#0E1512`) the dark variants use
            // — 5.86:1. Green-9 is one value across the whole stock
            // set, so its label may as well be too.
            success: Color::srgb_token("success", 48, 164, 108, 255),
            success_foreground: Color::srgb_token("success-foreground", 14, 21, 18, 255),
            warning: Color::srgb_token("warning", 255, 197, 61, 255),
            warning_foreground: Color::srgb_token("warning-foreground", 79, 52, 34, 255),
            // Blue-9 under a white label measured **3.26:1**. The label
            // takes Radix blue **dark-1** (`#0D1520`) — 5.62:1.
            info: Color::srgb_token("info", 0, 144, 255, 255),
            info_foreground: Color::srgb_token("info-foreground", 13, 21, 32, 255),
            destructive_tint_foreground: Color::srgb_token(
                "destructive-tint-foreground",
                206,
                44,
                49,
                255,
            ),
            success_tint_foreground: Color::srgb_token("success-tint-foreground", 30, 120, 80, 255),
            warning_tint_foreground: Color::srgb_token("warning-tint-foreground", 160, 93, 0, 255),
            info_tint_foreground: Color::srgb_token("info-tint-foreground", 12, 108, 190, 255),

            overlay_scrim: Color::srgb_token("overlay-scrim", 0, 0, 0, 128),
            link_foreground: Color::srgb_token("link-foreground", 101, 80, 185, 255),

            // Radix mauve light-9 `#8E8C99` — 3.3:1 over the white
            // `card`, dark label (4.9:1), same flip as slate light.
            badge: Color::srgb_token("badge", 142, 140, 153, 255),
            badge_foreground: Color::srgb_token("badge-foreground", 33, 31, 38, 255),

            scrollbar_thumb_fill: Color::srgb_token("scrollbar-thumb", 142, 140, 153, 90),
            scrollbar_thumb_fill_active: Color::srgb_token(
                "scrollbar-thumb-active",
                101,
                99,
                109,
                220,
            ),

            selection_bg: Color::srgb_token("selection-bg", 110, 86, 207, 64),
            selection_bg_unfocused: Color::srgb_token("selection-bg-unfocused", 142, 140, 153, 56),

            extra: BTreeMap::new(),
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

    /// Register an additional token name, so colors carrying it resolve
    /// against this palette instead of painting their baked rgba. This is
    /// how a design system mints its own vocabulary
    /// (`Color::srgb_token("layer-01", …)`) and keeps it swappable.
    ///
    /// The stock token names above are matched first and cannot be
    /// overridden this way; registering one of them stores an entry
    /// [`Palette::lookup`] will never reach. Registering the same extra
    /// name twice keeps the last color.
    pub fn with_token(mut self, name: &'static str, color: Color) -> Self {
        self.extra.insert(name, color);
        self
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

    /// Resolve a token name to its rgba in this palette, checking the
    /// stock vocabulary first and then the names registered with
    /// [`Palette::with_token`]. Returns `None` for theme-invariant tokens
    /// (the renderer falls back to the `Color`'s baked rgba) and for
    /// unknown names.
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
            "destructive-tint-foreground" => self.destructive_tint_foreground,
            "success-tint-foreground" => self.success_tint_foreground,
            "warning-tint-foreground" => self.warning_tint_foreground,
            "info-tint-foreground" => self.info_tint_foreground,
            "overlay-scrim" => self.overlay_scrim,
            "link-foreground" => self.link_foreground,
            "badge" => self.badge,
            "badge-foreground" => self.badge_foreground,
            "scrollbar-thumb" => self.scrollbar_thumb_fill,
            "scrollbar-thumb-active" => self.scrollbar_thumb_fill_active,
            "selection-bg" => self.selection_bg,
            "selection-bg-unfocused" => self.selection_bg_unfocused,
            _ => return self.extra.get(token).copied(),
        })
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::damascene_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens;

    #[test]
    fn dark_lookup_round_trips() {
        let p = Palette::damascene_dark();
        let bg = p.lookup("background").expect("background present");
        assert_eq!(bg, p.background);
    }

    #[test]
    fn damascene_dark_matches_token_fallbacks() {
        let palette = Palette::damascene_dark();
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
            tokens::BADGE,
            tokens::BADGE_FOREGROUND,
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
        let p = Palette::damascene_dark();
        assert!(p.lookup("not-a-token").is_none());
    }

    #[test]
    fn removed_legacy_tokens_not_in_palette() {
        let p = Palette::damascene_dark();
        assert!(p.lookup("bg-app").is_none());
        assert!(p.lookup("bg-card").is_none());
        assert!(p.lookup("bg-muted").is_none());
        assert!(p.lookup("text-on-solid-dark").is_none());
        assert!(p.lookup("text-on-solid-light").is_none());
    }

    #[test]
    fn resolve_passes_through_unrecognized() {
        let p = Palette::damascene_dark();
        let raw = Color::srgb_u8a(1, 2, 3, 4);
        assert_eq!(p.resolve(raw), raw);
        let invariant = Color::srgb_token("text-on-solid-dark", 8, 16, 25, 255);
        assert_eq!(p.resolve(invariant), invariant);
    }

    #[test]
    fn resolve_preserves_alpha_override() {
        let p = Palette::damascene_dark();
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
    fn damascene_light_differs_from_damascene_dark() {
        let dark = Palette::damascene_dark();
        let light = Palette::damascene_light();
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
        let light = Palette::damascene_light();
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
    fn with_token_round_trips_through_lookup() {
        let layer = Color::srgb_u8a(38, 38, 38, 255);
        let p = Palette::damascene_dark().with_token("layer-01", layer);
        assert_eq!(p.lookup("layer-01"), Some(layer));
    }

    #[test]
    fn registered_token_resolves_and_unregistered_passes_through() {
        let layer = Color::srgb_u8a(38, 38, 38, 255);
        let p = Palette::damascene_dark().with_token("layer-01", layer);

        // Registered: rgb comes from the palette, the token name survives.
        let authored = Color::srgb_token("layer-01", 1, 2, 3, 255);
        let resolved = p.resolve(authored);
        assert_eq!(
            (resolved.r, resolved.g, resolved.b),
            (layer.r, layer.g, layer.b)
        );
        assert_eq!(resolved.token, Some("layer-01"));

        // Unregistered: still the documented silent passthrough.
        let invariant = Color::srgb_token("layer-02", 1, 2, 3, 255);
        assert_eq!(p.resolve(invariant), invariant);
    }

    #[test]
    fn stock_names_shadow_extra_entries() {
        let hot_pink = Color::srgb_u8a(255, 0, 255, 255);
        let p = Palette::damascene_dark().with_token("card", hot_pink);
        assert_eq!(p.lookup("card"), Some(p.card));
        assert_ne!(p.lookup("card"), Some(hot_pink));
        assert_eq!(p.resolve(tokens::CARD), tokens::CARD);
    }

    #[test]
    fn palette_constructors_stay_const() {
        // `BTreeMap::new()` is const-stable, so the open namespace must
        // not cost the palette constructors their const-ness.
        const DARK: Palette = Palette::damascene_dark();
        const LIGHT: Palette = Palette::damascene_light();
        assert_eq!(DARK.background, Palette::damascene_dark().background);
        assert!(LIGHT.extra.is_empty());
    }

    /// Every stock palette variant, named for assertion messages.
    fn all_variants() -> [(&'static str, Palette); 8] {
        [
            ("damascene_dark", Palette::damascene_dark()),
            ("damascene_light", Palette::damascene_light()),
            ("radix_slate_blue_dark", Palette::radix_slate_blue_dark()),
            ("radix_slate_blue_light", Palette::radix_slate_blue_light()),
            ("radix_sand_amber_dark", Palette::radix_sand_amber_dark()),
            ("radix_sand_amber_light", Palette::radix_sand_amber_light()),
            ("radix_mauve_violet_dark", Palette::radix_mauve_violet_dark()),
            (
                "radix_mauve_violet_light",
                Palette::radix_mauve_violet_light(),
            ),
        ]
    }

    /// WCAG 2.x relative luminance of an sRGB color.
    fn luminance(c: Color) -> f32 {
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
    fn contrast(a: Color, b: Color) -> f32 {
        let (mut hi, mut lo) = (luminance(a), luminance(b));
        if hi < lo {
            std::mem::swap(&mut hi, &mut lo);
        }
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn badge_slot_is_filled_in_every_stock_variant() {
        // The slot is only useful if a palette swap moves it, which
        // requires every variant to have made a deliberate choice
        // rather than inheriting one variant's grey.
        for (name, p) in all_variants() {
            assert_eq!(
                p.lookup("badge"),
                Some(p.badge),
                "{name}: `badge` must resolve to its own slot"
            );
            assert_eq!(
                p.lookup("badge-foreground"),
                Some(p.badge_foreground),
                "{name}: `badge-foreground` must resolve to its own slot"
            );
            // Opaque: this is a material, not a wash. A translucent
            // entry would be erased by `resolve`, which takes alpha
            // from the requesting color.
            assert_eq!(p.badge.a, 1.0, "{name}: badge must be opaque");
            assert_eq!(
                p.badge_foreground.a, 1.0,
                "{name}: badge-foreground must be opaque"
            );
        }
    }

    #[test]
    fn badge_stands_off_every_surface_it_can_sit_on() {
        // The point of the slot: a chip must read as an object on the
        // panel. 3:1 is the WCAG non-text floor, and the value the
        // workbench chip failed at 1.11:1 before this slot existed.
        for (name, p) in all_variants() {
            for (surface_name, surface) in [
                ("background", p.background),
                ("card", p.card),
                ("popover", p.popover),
            ] {
                let ratio = contrast(p.badge, surface);
                assert!(
                    ratio >= 3.0,
                    "{name}: badge vs {surface_name} is {ratio:.2}:1, under the 3:1 floor"
                );
            }
        }
    }

    #[test]
    fn badge_foreground_is_readable_on_the_badge_fill() {
        // Chip labels are caption-sized, so the small-text floor (4.5)
        // applies rather than the large-text one.
        for (name, p) in all_variants() {
            let ratio = contrast(p.badge_foreground, p.badge);
            assert!(
                ratio >= 4.5,
                "{name}: badge-foreground on badge is {ratio:.2}:1, under the 4.5:1 floor"
            );
        }
    }

    /// The five solid roles: fill slot + the foreground painted on it.
    fn status_pairs(p: &Palette) -> [(&'static str, Color, Color); 5] {
        [
            ("primary", p.primary, p.primary_foreground),
            ("destructive", p.destructive, p.destructive_foreground),
            ("success", p.success, p.success_foreground),
            ("warning", p.warning, p.warning_foreground),
            ("info", p.info, p.info_foreground),
        ]
    }

    #[test]
    fn status_foregrounds_clear_the_small_text_floor() {
        // `button(..).info()`, a tinted chip and a toast header all
        // paint the solid fill and set the label to its paired
        // `*-foreground`. Those labels are caption-to-body sized, so
        // the small-text floor applies — not the 3:1 non-text one the
        // ramps' step-9/-500 rungs are engineered for. Before this
        // was asserted, half the set sat in the 3.2–3.9 band:
        // slate primary/info 3.26, radix destructive 3.91, radix light
        // success 3.90, zinc dark info 3.38, zinc light destructive
        // 3.61. See "Picking a status `*-foreground`" above for how
        // each replacement value is derived.
        for (name, p) in all_variants() {
            for (role, fill, fg) in status_pairs(&p) {
                let ratio = contrast(fg, fill);
                assert!(
                    ratio >= 4.5,
                    "{name}: {role}-foreground on {role} is {ratio:.2}:1, \
                     under the 4.5:1 floor"
                );
            }
        }
    }

    #[test]
    fn status_pairs_are_opaque_so_the_measurement_means_something() {
        // The contrast above is computed on the pair alone. That is
        // only the truth the user sees if neither member is a wash
        // letting a third color through — `resolve` takes alpha from
        // the requesting color, so a translucent entry here would
        // measure one thing and paint another.
        for (name, p) in all_variants() {
            for (role, fill, fg) in status_pairs(&p) {
                assert_eq!(fill.a, 1.0, "{name}: {role} must be opaque");
                assert_eq!(fg.a, 1.0, "{name}: {role}-foreground must be opaque");
            }
        }
    }

    #[test]
    fn badge_is_the_neutral_ramp_step_the_scrollbar_thumb_already_uses() {
        // Documented derivation, asserted so a future palette edit
        // moves the pair together instead of drifting: both are the
        // ramp's opaque neutral step, the thumb at partial alpha.
        for (name, p) in all_variants() {
            let thumb = p.scrollbar_thumb_fill;
            assert_eq!(
                (p.badge.r, p.badge.g, p.badge.b),
                (thumb.r, thumb.g, thumb.b),
                "{name}: badge must be the ramp's solid neutral step"
            );
            assert!(
                thumb.a < 1.0,
                "{name}: the thumb is the same step at partial alpha"
            );
        }
    }

    #[test]
    fn badge_is_not_the_secondary_slot_it_used_to_borrow() {
        // Regression guard for the acceptance complaint: the workbench
        // theme pointed `badge.background` at `secondary`, a
        // near-surface value that made chips invisible. If a variant
        // ever collapses the two, chips go back to unreadable.
        for (name, p) in all_variants() {
            assert_ne!(
                (p.badge.r, p.badge.g, p.badge.b),
                (p.secondary.r, p.secondary.g, p.secondary.b),
                "{name}: badge must not collapse onto secondary"
            );
        }
    }

    #[test]
    fn rgb_ops_strip_token_so_resolve_passes_through() {
        // State animations (hover lighten, press darken) build colors
        // via .darken/.lighten/.mix on token-tagged base colors. Those
        // ops strip the token, so resolve passes them through unchanged
        // and the per-frame derivation wins — which is what we want.
        let p = Palette::damascene_dark();
        let darkened = p.muted.darken(0.5);
        assert_eq!(darkened.token, None);
        let resolved = p.resolve(darkened);
        assert_eq!(resolved, darkened);
    }
}
