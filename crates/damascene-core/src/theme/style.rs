//! Style modifier methods on [`El`] — kind-aware via [`StyleProfile`].
//!
//! Each component declares its [`StyleProfile`] in its constructor.
//! Style modifiers (`.primary`, `.success`, `.muted`, etc.) dispatch
//! on the profile, not on `Kind`. That means adding a new component
//! is a self-contained file change: declare a profile, the existing
//! modifier vocabulary just works.
//!
//! Profile semantics:
//!
//! - [`StyleProfile::Solid`] — color modifiers produce solid fills
//!   (Button, Toggle thumb, …).
//! - [`StyleProfile::Tinted`] — color modifiers produce tinted alpha
//!   fills with status-colored text (Badge, highlighted Card, …).
//! - [`StyleProfile::Surface`] — color modifiers tint a subtle bg;
//!   `.muted` swaps to a neutral surface (Card, TextField, Select, …).
//! - [`StyleProfile::TextOnly`] — color modifiers only change text color
//!   (Text, Heading, …).
//!
//! Modifier groups in this file:
//!
//! - **Color/status:** `primary`, `success`, `warning`, `destructive`, `info`
//! - **Surface variants:** `secondary`, `ghost`, `outline`, `muted`
//! - **Semantic states:** `selected`, `current`, `disabled`, `invalid`, `loading`
//! - **Typography roles:** `caption`, `label`, `body`, `title`, `heading`, `display`, `code`
//! - **Text shape:** `bold`, `semibold`, `small`, `xsmall`, `color`

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::metrics::ComponentSize;
use crate::tokens;
use crate::tree::*;

/// How a component reacts to style/color modifiers.
///
/// Set once in the component's constructor; the modifier methods dispatch
/// on this rather than on [`Kind`], so adding a new component never
/// requires editing this file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StyleProfile {
    /// Color modifiers produce solid fills with contrasting text
    /// (Button, Toggle thumb, …).
    Solid,
    /// Color modifiers produce tinted low-alpha fills with
    /// status-colored text (Badge, highlighted Card, …).
    Tinted,
    /// Color modifiers tint a subtle background; `.muted()` swaps to a
    /// neutral surface (Card, TextField, Select, …).
    Surface,
    /// Color modifiers only change the text color (Text, Heading, …).
    /// The default profile.
    #[default]
    TextOnly,
}

impl El {
    // ===== Color / status (profile-aware) =====

    /// Primary/brand treatment — applies [`tokens::PRIMARY`] per the
    /// element's [`StyleProfile`].
    pub fn primary(self) -> Self {
        tint(self, tokens::PRIMARY)
    }
    /// Positive status treatment — applies [`tokens::SUCCESS`] per the
    /// element's [`StyleProfile`].
    pub fn success(self) -> Self {
        tint(self, tokens::SUCCESS)
    }
    /// Cautionary status treatment — applies [`tokens::WARNING`] per
    /// the element's [`StyleProfile`].
    pub fn warning(self) -> Self {
        tint(self, tokens::WARNING)
    }
    /// Destructive-action treatment — applies [`tokens::DESTRUCTIVE`]
    /// per the element's [`StyleProfile`].
    pub fn destructive(self) -> Self {
        tint(self, tokens::DESTRUCTIVE)
    }
    /// Informational status treatment — applies [`tokens::INFO`] per
    /// the element's [`StyleProfile`].
    pub fn info(self) -> Self {
        tint(self, tokens::INFO)
    }

    // ===== Surface variants =====

    /// Default-styled secondary surface. This is the default look for
    /// `button(...)`; calling `.secondary()` makes intent explicit.
    pub fn secondary(mut self) -> Self {
        self.fill = Some(tokens::SECONDARY);
        self.stroke = Some(tokens::BORDER);
        self.stroke_width = 1.0;
        set_content_color(&mut self, tokens::SECONDARY_FOREGROUND);
        self.font_weight = FontWeight::Medium;
        self
    }

    /// No fill, no border. Low-emphasis actions like "Cancel" alongside
    /// a primary "Save".
    pub fn ghost(mut self) -> Self {
        self.fill = None;
        self.stroke = None;
        self.stroke_width = 0.0;
        set_content_color(&mut self, tokens::MUTED_FOREGROUND);
        self
    }

    /// Outline-only style: no fill, prominent border.
    pub fn outline(mut self) -> Self {
        self.fill = None;
        self.stroke = Some(tokens::INPUT);
        self.stroke_width = 1.0;
        set_content_color(&mut self, tokens::FOREGROUND);
        self
    }

    /// Muted/neutral emphasis. On surface profiles this swaps to a
    /// neutral background; on text-only profiles it switches the text
    /// color to muted-foreground.
    pub fn muted(mut self) -> Self {
        match self.style_profile {
            StyleProfile::Solid | StyleProfile::Tinted | StyleProfile::Surface => {
                self.fill = Some(tokens::MUTED);
                self.stroke = Some(tokens::BORDER);
                self.stroke_width = 1.0;
                set_content_color(&mut self, tokens::MUTED_FOREGROUND);
            }
            StyleProfile::TextOnly => {
                set_content_color(&mut self, tokens::MUTED_FOREGROUND);
            }
        }
        self
    }

    // ===== Semantic states =====

    /// Selected row/item treatment. Use for the item that is selected
    /// inside a collection, not for transient keyboard focus.
    pub fn selected(mut self) -> Self {
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::PRIMARY);
        } else if matches!(self.kind, Kind::Custom("item")) {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Selected;
            self.fill = Some(tokens::PRIMARY.with_alpha_u8(18));
            self.stroke = Some(tokens::PRIMARY.with_alpha_u8(90));
            self.stroke_width = 1.0;
            set_content_color(&mut self, tokens::FOREGROUND);
            set_item_rail(&mut self, tokens::PRIMARY);
        } else {
            {
                self.style_profile = StyleProfile::Surface;
                self.surface_role = SurfaceRole::Selected;
                self.fill = Some(tokens::PRIMARY.with_alpha_u8(28));
                self.stroke = Some(tokens::PRIMARY.with_alpha_u8(90));
                self.stroke_width = 1.0;
                set_content_color(&mut self, tokens::FOREGROUND);
            }
        }
        self
    }

    /// Current navigation/page treatment. Slightly quieter than
    /// [`Self::selected`] so nav chrome does not compete with content.
    pub fn current(mut self) -> Self {
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        } else if matches!(self.kind, Kind::Custom("item")) {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Current;
            self.fill = Some(tokens::ACCENT.with_alpha_u8(24));
            self.stroke = Some(tokens::BORDER);
            self.stroke_width = 1.0;
            set_content_color(&mut self, tokens::FOREGROUND);
            set_item_rail(&mut self, tokens::PRIMARY);
        } else {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Current;
            self.fill = Some(tokens::ACCENT);
            self.stroke = Some(tokens::BORDER);
            self.stroke_width = 1.0;
            set_content_color(&mut self, tokens::ACCENT_FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        }
        self
    }

    /// Disabled treatment for controls and rows. Also removes the node
    /// from focus order, blocks pointer hits on this element, and
    /// declares [`Cursor::NotAllowed`](crate::cursor::Cursor::NotAllowed)
    /// so hovering a disabled control reads as inert.
    pub fn disabled(mut self) -> Self {
        self.opacity = tokens::DISABLED_ALPHA;
        self.focusable = false;
        self.block_pointer = true;
        self.cursor = Some(crate::cursor::Cursor::NotAllowed);
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::MUTED_FOREGROUND);
        }
        self
    }

    /// Invalid/error treatment for inputs, rows, and validation badges.
    pub fn invalid(mut self) -> Self {
        if !text_only_leaf(&self) {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Danger;
        }
        self.stroke = Some(tokens::DESTRUCTIVE);
        self.stroke_width = 1.0;
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::DESTRUCTIVE);
        }
        self
    }

    /// Loading treatment for a direct text-bearing node. Container
    /// widgets can still use this for opacity even when they do not
    /// have their own label text.
    pub fn loading(mut self) -> Self {
        self.opacity = self.opacity.min(0.78);
        if let Some(label) = &mut self.text {
            label.push_str("...");
        }
        self
    }

    // ===== Typography roles =====

    /// Set the typography role and apply its size/weight/color defaults.
    /// The named shorthands ([`Self::caption`], [`Self::body`], …) are
    /// the usual entry points.
    pub fn text_role(mut self, role: TextRole) -> Self {
        self.text_role = role;
        apply_text_role(&mut self);
        self
    }

    /// Caption role — [`tokens::TEXT_XS`], regular weight, muted-foreground text.
    pub fn caption(self) -> Self {
        self.text_role(TextRole::Caption)
    }

    /// Label role — [`tokens::TEXT_SM`], medium weight.
    pub fn label(self) -> Self {
        self.text_role(TextRole::Label)
    }

    /// Body role — [`tokens::TEXT_SM`], regular weight.
    pub fn body(self) -> Self {
        self.text_role(TextRole::Body)
    }

    /// Title role — [`tokens::TEXT_BASE`], semibold.
    pub fn title(self) -> Self {
        self.text_role(TextRole::Title)
    }

    /// Heading role — [`tokens::TEXT_2XL`], semibold.
    pub fn heading(self) -> Self {
        self.text_role(TextRole::Heading)
    }

    /// Display role — [`tokens::TEXT_3XL`], bold. The largest stock text role.
    pub fn display(self) -> Self {
        self.text_role(TextRole::Display)
    }

    // ===== Text shape =====

    /// Set the font weight to bold.
    pub fn bold(mut self) -> Self {
        self.font_weight = FontWeight::Bold;
        self
    }
    /// Set the font weight to semibold.
    pub fn semibold(mut self) -> Self {
        self.font_weight = FontWeight::Semibold;
        self
    }
    /// Compact sizing: text leaves drop to [`tokens::TEXT_SM`];
    /// components select their small (`Sm`) size variant.
    pub fn small(mut self) -> Self {
        if text_only_leaf(&self) {
            apply_type_token(&mut self, tokens::TEXT_SM);
        } else {
            self.component_size = Some(ComponentSize::Sm);
        }
        self
    }
    /// Extra-compact sizing: text leaves drop to [`tokens::TEXT_XS`];
    /// components select their extra-small (`Xs`) size variant.
    pub fn xsmall(mut self) -> Self {
        if text_only_leaf(&self) {
            apply_type_token(&mut self, tokens::TEXT_XS);
        } else {
            self.component_size = Some(ComponentSize::Xs);
        }
        self
    }
    /// Set an explicit text color.
    pub fn color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }
}

fn text_only_leaf(el: &El) -> bool {
    matches!(el.style_profile, StyleProfile::TextOnly) && el.text.is_some()
}

fn apply_type_token(el: &mut El, token: tokens::TypeToken) {
    el.font_size = token.size;
    el.line_height = token.line_height;
}

fn apply_text_role(el: &mut El) {
    // Non-Code roles default to the proportional face; explicit
    // `.mono()` (which sets `explicit_mono`) wins so the natural
    // reading order `text(s).mono().caption()` keeps the mono family.
    // The Code role intentionally forces mono regardless — that's its
    // whole purpose, and the explicit override would only be set true,
    // never false, so there's no conflict to resolve.
    let clear_mono = |el: &mut El| {
        if !el.explicit_mono {
            el.font_mono = false;
        }
    };
    match el.text_role {
        TextRole::Body => {
            apply_type_token(el, tokens::TEXT_SM);
            el.font_weight = FontWeight::Regular;
            el.text_letter_spacing = 0.0;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Caption => {
            apply_type_token(el, tokens::TEXT_XS);
            el.font_weight = FontWeight::Regular;
            el.text_letter_spacing = 0.0;
            clear_mono(el);
            el.text_color = Some(tokens::MUTED_FOREGROUND);
        }
        TextRole::Label => {
            apply_type_token(el, tokens::TEXT_SM);
            el.font_weight = FontWeight::Medium;
            el.text_letter_spacing = 0.0;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Title => {
            apply_type_token(el, tokens::TEXT_BASE);
            el.font_weight = FontWeight::Semibold;
            el.text_letter_spacing = tokens::TRACKING_TIGHT_EM * tokens::TEXT_BASE.size;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Heading => {
            apply_type_token(el, tokens::TEXT_2XL);
            el.font_weight = FontWeight::Semibold;
            el.text_letter_spacing = tokens::TRACKING_TIGHT_EM * tokens::TEXT_2XL.size;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Display => {
            apply_type_token(el, tokens::TEXT_3XL);
            el.font_weight = FontWeight::Bold;
            el.text_letter_spacing = tokens::TRACKING_TIGHT_EM * tokens::TEXT_3XL.size;
            clear_mono(el);
            el.text_color = Some(tokens::FOREGROUND);
        }
        TextRole::Code => {
            apply_type_token(el, tokens::TEXT_XS);
            el.font_weight = FontWeight::Regular;
            el.text_letter_spacing = 0.0;
            el.font_mono = true;
            el.text_color = Some(tokens::FOREGROUND);
        }
    }
}

fn tint(mut el: El, c: Color) -> El {
    match el.style_profile {
        StyleProfile::Solid => {
            el.fill = Some(c);
            el.stroke = Some(c);
            el.stroke_width = 1.0;
            set_content_color(&mut el, text_on_solid(c));
            el.font_weight = FontWeight::Semibold;
        }
        StyleProfile::Tinted => {
            el.fill = Some(c.with_alpha_u8(38));
            el.stroke = Some(c.with_alpha_u8(120));
            el.stroke_width = 1.0;
            set_content_color(&mut el, c);
        }
        StyleProfile::Surface => {
            el.fill = Some(c.with_alpha_u8(38));
            el.stroke = Some(c.with_alpha_u8(120));
            el.stroke_width = 1.0;
            set_content_color(&mut el, c);
        }
        StyleProfile::TextOnly => {
            set_content_color(&mut el, c);
        }
    }
    el
}

fn set_content_color(el: &mut El, color: Color) {
    el.text_color = Some(color);
    for child in &mut el.children {
        if child.text.is_some() || child.icon.is_some() {
            child.text_color = Some(color);
        }
    }
}

fn set_item_rail(el: &mut El, color: Color) {
    for child in &mut el.children {
        if matches!(child.kind, Kind::Custom("item_rail")) {
            child.fill = Some(color);
            child.opacity = 1.0;
        }
    }
}

/// Pick a contrasting text color for a solid background fill.
///
/// Rec. 601 luminance threshold tuned so light/saturated fills (accent
/// blue, success green, warning yellow) get dark text, and darker
/// saturated fills (destructive red) get light text.
fn text_on_solid(c: Color) -> Color {
    match c.token {
        Some("primary") => return tokens::PRIMARY_FOREGROUND,
        Some("secondary") => return tokens::SECONDARY_FOREGROUND,
        Some("accent") => return tokens::ACCENT_FOREGROUND,
        Some("destructive") => return tokens::DESTRUCTIVE_FOREGROUND,
        Some("success") => return tokens::SUCCESS_FOREGROUND,
        Some("warning") => return tokens::WARNING_FOREGROUND,
        Some("info") => return tokens::INFO_FOREGROUND,
        _ => {}
    }

    let srgb = c.convert_to(crate::color::ColorSpace::SRGB);
    let lum = 0.299 * srgb.r + 0.587 * srgb.g + 0.114 * srgb.b;
    if lum > 150.0 / 255.0 {
        Color::srgb_u8a(8, 16, 25, 255)
    } else {
        Color::srgb_u8a(250, 250, 252, 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, button_with_icon, icon_button, row, text};

    #[test]
    fn text_on_solid_contrast_follows_fill_luminance() {
        // Regression: the threshold once compared 0..1 float luminance
        // against 150.0 (a u8-scale constant), so *every* untokened
        // fill got light text — including near-white ones.
        let on_light = text_on_solid(Color::srgb_u8(240, 240, 240));
        let on_dark = text_on_solid(Color::srgb_u8(30, 30, 30));
        assert!(
            on_light.g < 0.5,
            "light fill must get dark text, got g={}",
            on_light.g
        );
        assert!(
            on_dark.g > 0.5,
            "dark fill must get light text, got g={}",
            on_dark.g
        );
    }

    #[test]
    fn selected_marks_surface_with_accent_treatment() {
        let el = row([text("Selected")]).selected();
        assert_eq!(el.fill, Some(tokens::PRIMARY.with_alpha_u8(28)));
        assert_eq!(el.stroke, Some(tokens::PRIMARY.with_alpha_u8(90)));
        assert_eq!(el.stroke_width, 1.0);
        assert_eq!(el.surface_role, SurfaceRole::Selected);
    }

    #[test]
    fn current_marks_container_as_selected_surface_role() {
        let el = row([text("Current")]).current();
        assert_eq!(el.fill, Some(tokens::ACCENT));
        assert_eq!(el.stroke, Some(tokens::BORDER));
        assert_eq!(el.surface_role, SurfaceRole::Current);
        assert_eq!(el.style_profile, StyleProfile::Surface);
    }

    #[test]
    fn disabled_removes_focus_and_dims_control() {
        let el = button("Disabled").disabled();
        assert!(!el.focusable);
        assert!(el.block_pointer);
        assert_eq!(el.opacity, tokens::DISABLED_ALPHA);
    }

    #[test]
    fn icon_button_uses_same_solid_style_surface_as_button() {
        let el = icon_button("menu").primary();
        assert_eq!(el.icon, Some(crate::IconSource::Builtin(IconName::Menu)));
        assert_eq!(el.fill, Some(tokens::PRIMARY));
        assert_eq!(el.text_color, Some(text_on_solid(tokens::PRIMARY)));
        assert_eq!(el.surface_role, SurfaceRole::Raised);
    }

    #[test]
    fn button_with_icon_propagates_variant_content_color() {
        let el = button_with_icon("upload", "Publish").primary();
        assert_eq!(el.fill, Some(tokens::PRIMARY));
        assert_eq!(
            el.children[0].icon,
            Some(crate::IconSource::Builtin(IconName::Upload))
        );
        let expected = text_on_solid(tokens::PRIMARY);
        assert_eq!(el.children[0].text_color, Some(expected));
        assert_eq!(el.children[1].text.as_deref(), Some("Publish"));
        assert_eq!(el.children[1].text_color, Some(expected));
    }

    #[test]
    fn loading_appends_direct_label_text() {
        let el = button("Save").loading();
        assert_eq!(el.text.as_deref(), Some("Save..."));
        assert_eq!(el.opacity, 0.78);
    }

    #[test]
    fn text_roles_apply_inspectable_typographic_defaults() {
        let caption = text("Caption").caption();
        assert_eq!(caption.text_role, TextRole::Caption);
        assert_eq!(caption.font_size, tokens::TEXT_XS.size);
        assert_eq!(caption.line_height, tokens::TEXT_XS.line_height);
        assert_eq!(caption.text_color, Some(tokens::MUTED_FOREGROUND));

        let label = text("Label").label();
        assert_eq!(label.text_role, TextRole::Label);
        assert_eq!(label.font_size, tokens::TEXT_SM.size);
        assert_eq!(label.line_height, tokens::TEXT_SM.line_height);
        assert_eq!(label.font_weight, FontWeight::Medium);

        let code = text("Code").code();
        assert_eq!(code.text_role, TextRole::Code);
        assert_eq!(code.font_size, tokens::TEXT_XS.size);
        assert_eq!(code.line_height, tokens::TEXT_XS.line_height);
        assert_eq!(code.font_weight, FontWeight::Regular);
        assert_eq!(code.text_color, Some(tokens::FOREGROUND));
        assert!(code.font_mono);
    }

    #[test]
    fn explicit_mono_survives_subsequent_role_modifier() {
        // gh#12. The natural reading order `text(s).mono().caption()`
        // ("small mono caption") used to silently render in the
        // proportional face — `.caption()` reset `font_mono = false`
        // because non-Code roles bake the proportional family in. The
        // `explicit_mono` flag set by `.mono()` now suppresses that
        // reset for every non-Code role.
        let mono_first = text("+2").mono().caption();
        assert!(
            mono_first.font_mono,
            "`.mono()` chained before `.caption()` must keep mono on",
        );
        // Caption's other defaults still apply.
        assert_eq!(mono_first.font_size, tokens::TEXT_XS.size);
        assert_eq!(mono_first.text_color, Some(tokens::MUTED_FOREGROUND));

        // Reversed order — the canonical order — also keeps mono on.
        let role_first = text("+2").caption().mono();
        assert!(role_first.font_mono);

        // Same gating across the rest of the role family.
        for el in [
            text("+1").mono().body(),
            text("+1").mono().label(),
            text("+1").mono().title(),
            text("+1").mono().heading(),
            text("+1").mono().display(),
        ] {
            assert!(
                el.font_mono,
                "explicit .mono() must survive every non-Code role",
            );
        }

        // The Code role is unconditionally mono — no explicit_mono
        // gating needed, but verify nothing regressed.
        assert!(text("x").mono().code().font_mono);
    }
}

#[cfg(test)]
mod tracking_tests {
    use crate::tokens;
    use crate::tree::*;

    #[test]
    fn heading_roles_carry_tracking_tight_and_body_resets_it() {
        let h = crate::text("T").text_role(TextRole::Heading);
        assert_eq!(
            h.text_letter_spacing,
            tokens::TRACKING_TIGHT_EM * tokens::TEXT_2XL.size
        );
        let t = crate::text("T").text_role(TextRole::Title);
        assert_eq!(
            t.text_letter_spacing,
            tokens::TRACKING_TIGHT_EM * tokens::TEXT_BASE.size
        );
        // Switching a heading back to Body must not leak the tracking.
        let b = crate::text("T")
            .text_role(TextRole::Heading)
            .text_role(TextRole::Body);
        assert_eq!(b.text_letter_spacing, 0.0);
    }
}
