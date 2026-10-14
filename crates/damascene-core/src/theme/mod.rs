//! Theme-level shader routing.
//!
//! Damascene widgets expose familiar style knobs (`fill`, `stroke`, `radius`,
//! `shadow`) while the renderer resolves those facts into shader bindings.
//! `Theme` is the indirection layer between those two worlds: an app can
//! keep using stock widgets and globally swap the shader recipe that paints
//! implicit surfaces.
//!
//! This is intentionally shader-first. Token colors are still authored as
//! constants today, but surface appearance can already move from
//! `stock::rounded_rect` to a custom material without rewriting every
//! `button`, `card`, or `text_input`.
//!
//! Sibling modules cover the rest of the appearance system:
//! - [`tokens`] — semantic color / spacing / radius constants.
//! - [`palette`] — runtime swappable color palette layered on the tokens.
//! - [`style`] — style profiles (Solid, TextOnly, …) + variant chainables
//!   (`.primary()`, `.ghost()`, …).

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

pub mod palette;
pub mod style;
pub mod tokens;

use std::collections::BTreeMap;

use crate::metrics::{ComponentSize, ThemeMetrics};
use crate::shader::{ShaderHandle, StockShader, UniformBlock, UniformValue};
use crate::tree::{Color, FontFamily, SurfaceRole};
use crate::vector::IconMaterial;
use palette::Palette;

/// Runtime paint theme for implicit widget visuals.
#[derive(Clone, Debug)]
pub struct Theme {
    palette: Palette,
    metrics: ThemeMetrics,
    surface: SurfaceTheme,
    roles: BTreeMap<SurfaceRole, SurfaceTheme>,
    icon_material: IconMaterial,
    font_family: FontFamily,
    mono_font_family: FontFamily,
}

impl Theme {
    /// Current default: stock rounded-rect surfaces with the Damascene Dark
    /// palette (copied from shadcn/ui zinc dark) and compact desktop
    /// metrics.
    pub fn damascene_dark() -> Self {
        Self::default().with_palette(Palette::damascene_dark())
    }

    /// Stock rounded-rect surfaces with the Damascene Light palette (copied
    /// from shadcn/ui zinc light). Drop-in alternative to
    /// [`Self::damascene_dark`] — token references swap rgba at paint time
    /// without rebuilding the widget tree.
    pub fn damascene_light() -> Self {
        Self::default().with_palette(Palette::damascene_light())
    }

    /// Stock rounded-rect surfaces with a Radix Colors slate + blue
    /// dark palette.
    pub fn radix_slate_blue_dark() -> Self {
        Self::default().with_palette(Palette::radix_slate_blue_dark())
    }

    /// Stock rounded-rect surfaces with a Radix Colors slate + blue
    /// light palette.
    pub fn radix_slate_blue_light() -> Self {
        Self::default().with_palette(Palette::radix_slate_blue_light())
    }

    /// Stock rounded-rect surfaces with a Radix Colors sand + amber
    /// dark palette — warm sepia neutrals with a bright amber accent.
    pub fn radix_sand_amber_dark() -> Self {
        Self::default().with_palette(Palette::radix_sand_amber_dark())
    }

    /// Stock rounded-rect surfaces with a Radix Colors sand + amber
    /// light palette.
    pub fn radix_sand_amber_light() -> Self {
        Self::default().with_palette(Palette::radix_sand_amber_light())
    }

    /// Stock rounded-rect surfaces with a Radix Colors mauve + violet
    /// dark palette — purple-tinged neutrals with a violet accent.
    pub fn radix_mauve_violet_dark() -> Self {
        Self::default().with_palette(Palette::radix_mauve_violet_dark())
    }

    /// Stock rounded-rect surfaces with a Radix Colors mauve + violet
    /// light palette.
    pub fn radix_mauve_violet_light() -> Self {
        Self::default().with_palette(Palette::radix_mauve_violet_light())
    }

    /// Replace the runtime color palette. Token references resolve
    /// through the active palette at paint time, so this swaps surface
    /// rgba without rebuilding the widget tree.
    pub fn with_palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
        self
    }

    /// The active runtime palette.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The active layout metrics used to resolve stock widget defaults.
    pub fn metrics(&self) -> &ThemeMetrics {
        &self.metrics
    }

    /// The default proportional UI font family applied to text nodes
    /// that do not set `.font_family(...)` themselves.
    pub fn font_family(&self) -> FontFamily {
        self.font_family
    }

    /// Set the default proportional UI font family. A registered
    /// brand face is selectable by name via
    /// [`FontFamily::custom`](crate::tree::FontFamily::custom).
    pub fn with_font_family(mut self, family: FontFamily) -> Self {
        self.font_family = family;
        self
    }

    /// The default monospace font family applied to text nodes that
    /// render as code (`font_mono = true`, `TextRole::Code`) and do
    /// not set `.mono_font_family(...)` themselves. Independent of
    /// [`Self::font_family`] — swapping the proportional face leaves
    /// the code face alone, and vice versa.
    pub fn mono_font_family(&self) -> FontFamily {
        self.mono_font_family
    }

    /// Set the default monospace font family for code-tagged text.
    pub fn with_mono_font_family(mut self, family: FontFamily) -> Self {
        self.mono_font_family = family;
        self
    }

    /// Replace the runtime layout metrics.
    pub fn with_metrics(mut self, metrics: ThemeMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Set the default t-shirt size for stock controls.
    pub fn with_default_component_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_default_component_size(size);
        self
    }

    /// Set the t-shirt size for buttons and icon buttons, overriding
    /// the theme default (a per-element `.size(...)` still wins).
    pub fn with_button_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_button_size(size);
        self
    }

    /// Set the t-shirt size for text inputs, overriding the theme
    /// default.
    pub fn with_input_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_input_size(size);
        self
    }

    /// Set the t-shirt size for badges, overriding the theme default.
    pub fn with_badge_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_badge_size(size);
        self
    }

    /// Set the t-shirt size for tab triggers, overriding the theme
    /// default.
    pub fn with_tab_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_tab_size(size);
        self
    }

    /// Set the t-shirt size for checkbox / radio control boxes,
    /// overriding the theme default.
    pub fn with_choice_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_choice_size(size);
        self
    }

    /// Set the size rung for slider track heights, overriding the
    /// theme default.
    pub fn with_slider_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_slider_size(size);
        self
    }

    /// Set the size rung for progress bar heights, overriding the
    /// theme default.
    pub fn with_progress_size(mut self, size: ComponentSize) -> Self {
        self.metrics = self.metrics.with_progress_size(size);
        self
    }

    /// Multiply every theme-default corner radius by `scale` — the
    /// one-line "how round is this app" knob, matching shadcn's
    /// `--radius`. `1.0` (the default) leaves stock radii untouched;
    /// `0.0` squares the app, controls included; `2.0` doubles them.
    ///
    /// The metrics pass applies this to every corner a *theme* chose:
    /// the `default_radius(...)` a widget constructor baked in, and the
    /// radius the pass itself stamps onto buttons and inputs. Three
    /// things survive it:
    ///
    /// - An explicit `.radius(...)`. The author named that value
    ///   (`RadiusOrigin::Fixed`), so it is not a theme default. Switch
    ///   tracks (explicit `RADIUS_PILL`) share the exemption, and card
    ///   header/footer strips inherit the *card's* final corners inside
    ///   the pass, so they track the card whether or not it scaled.
    ///   Widget-stamped silhouettes (`RadiusOrigin::LibraryShape`,
    ///   e.g. `tabs_list` segment triggers) are shape-protected but DO
    ///   scale — they square at `0.0` along with everything else.
    /// - Corners already at `0.0`, so per-corner shapes
    ///   ([`Corners::top`](crate::tree::Corners::top) and friends) keep
    ///   their silhouette instead of going uniform.
    /// - Corners at or above [`tokens::RADIUS_PILL`], because on the web
    ///   `rounded-full` is independent of `--radius` — pills and avatars
    ///   stay round at any scale, including `0.0`. (Badges use a small
    ///   fixed radius, well below the pill threshold, and scale like any
    ///   other control.)
    pub fn with_radius_scale(mut self, scale: f32) -> Self {
        self.metrics = self.metrics.with_radius_scale(scale);
        self
    }

    /// Multiply every theme-default drop shadow by `scale` — the
    /// elevation counterpart of [`Self::with_radius_scale`], matching
    /// how a web theme's shadow variables flatten a whole app at once.
    /// `1.0` (the default) leaves stock shadows untouched; `0.0` makes
    /// the chrome flat.
    ///
    /// The scale reaches both places theme shadows live: the elevation
    /// tier a widget recipe baked in (`card()`'s `SHADOW_SM`,
    /// `popover()`'s `SHADOW_MD`, …) and the surface-role *defaults*
    /// the paint pass fills for `Panel` / `Raised` / `Popover`
    /// surfaces that declared no tier of their own.
    ///
    /// An explicit `.shadow(...)` survives at any scale — the author
    /// named that elevation, like a hardcoded `box-shadow` ignoring
    /// the theme's shadow variables. At `0.0` a scaled-away role
    /// default is *omitted* rather than written as zero, so a
    /// [`Self::with_role_uniform`]`(role, "shadow", …)` can
    /// deliberately re-elevate one role (e.g. keep popovers shadowed
    /// in an otherwise flat app).
    pub fn with_shadow_scale(mut self, scale: f32) -> Self {
        self.metrics = self.metrics.with_shadow_scale(scale);
        self
    }

    /// Multiply every role- and rung-derived font size and line height
    /// by `scale` — the analogue of setting the root font size on the
    /// web, where the whole rem-based type ladder scales together.
    /// `1.0` (the default) leaves stock type untouched; `13.0 / 14.0`
    /// puts 14px body/label text at 13px.
    ///
    /// What scales: everything a role or rung chose — `TextRole`
    /// stamps (`.body()`, `.caption()`, `.title()`, …), the `.small()`
    /// / `.xsmall()` ladder rungs, and default icon sizes — with line
    /// heights scaling alongside so vertical rhythm survives.
    ///
    /// What survives: a raw `.font_size(...)`, `.line_height(...)`, or
    /// `.icon_size(...)` — the author hand-picked those values, like
    /// `text-[15px]` ignoring a rem theme.
    ///
    /// What deliberately does NOT scale: control heights and paddings.
    /// Those are the [`ComponentSize`] ladder's job
    /// ([`Self::with_default_component_size`]), so type density and
    /// control density stay independently tunable.
    pub fn with_type_scale(mut self, scale: f32) -> Self {
        self.metrics = self.metrics.with_type_scale(scale);
        self
    }

    pub(crate) fn apply_metrics(&self, root: &mut crate::El) {
        apply_metrics_tree(&self.metrics, self.font_family, self.mono_font_family, root);
    }

    /// Shorthand for `self.palette().resolve(c)`. Library code that
    /// derives a color from a token (e.g. via `darken`/`lighten`/`mix`)
    /// should resolve through the palette **before** applying the op
    /// so the derivation is computed against the active palette's rgb,
    /// not the token's compile-time fallback.
    pub fn resolve(&self, c: Color) -> Color {
        self.palette.resolve(c)
    }

    /// Route all implicit surfaces through a custom shader.
    ///
    /// The draw-op pass still emits the familiar rounded-rect uniforms
    /// (`fill`, `stroke`, `radius`, `shadow`, `focus_color`, …). When
    /// `rounded_rect_slots` is enabled, those values are also copied into
    /// `vec_a`..`vec_d`, matching the cross-backend [`crate::paint::QuadInstance`]
    /// ABI so custom shaders can be drop-in material replacements.
    pub fn with_surface_shader(mut self, shader: &'static str) -> Self {
        self.surface.handle = ShaderHandle::Custom(shader);
        self.surface.rounded_rect_slots = true;
        self
    }

    /// Add a uniform to every implicit surface draw. Existing node
    /// uniforms win, so a local widget override can still specialize a
    /// shader parameter.
    pub fn with_surface_uniform(mut self, key: &'static str, value: UniformValue) -> Self {
        self.surface.uniforms.insert(key, value);
        self
    }

    /// Route a specific semantic surface role through a custom shader.
    /// Roles without an override use the global surface recipe.
    pub fn with_role_shader(mut self, role: SurfaceRole, shader: &'static str) -> Self {
        self.role_mut(role).handle = ShaderHandle::Custom(shader);
        self.role_mut(role).rounded_rect_slots = true;
        self
    }

    /// Add a uniform to a specific semantic surface role.
    pub fn with_role_uniform(
        mut self,
        role: SurfaceRole,
        key: &'static str,
        value: UniformValue,
    ) -> Self {
        self.role_mut(role).uniforms.insert(key, value);
        self
    }

    /// Select the stock material used by native vector icon painters.
    /// Backends without vector icon support may ignore this while still
    /// preserving the theme value for API parity.
    pub fn with_icon_material(mut self, material: IconMaterial) -> Self {
        self.icon_material = material;
        self
    }

    /// The stock material native vector icon painters use.
    pub fn icon_material(&self) -> IconMaterial {
        self.icon_material
    }

    pub(crate) fn surface_handle(&self, role: SurfaceRole) -> ShaderHandle {
        self.role_theme(role).handle
    }

    pub(crate) fn apply_surface_uniforms(&self, role: SurfaceRole, uniforms: &mut UniformBlock) {
        let surface = self.role_theme(role);
        uniforms
            .entry("surface_role")
            .or_insert(UniformValue::F32(role.uniform_id()));
        apply_role_material(role, uniforms, &self.palette, self.metrics.shadow_scale());
        if surface.rounded_rect_slots {
            add_rounded_rect_slots(uniforms);
        }
        for (key, value) in &surface.uniforms {
            uniforms.entry(*key).or_insert(*value);
        }
    }

    fn role_mut(&mut self, role: SurfaceRole) -> &mut SurfaceTheme {
        self.roles
            .entry(role)
            .or_insert_with(|| self.surface.clone())
    }

    fn role_theme(&self, role: SurfaceRole) -> &SurfaceTheme {
        self.roles.get(&role).unwrap_or(&self.surface)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            palette: Palette::default(),
            metrics: ThemeMetrics::default(),
            surface: SurfaceTheme {
                handle: ShaderHandle::Stock(StockShader::RoundedRect),
                uniforms: UniformBlock::new(),
                rounded_rect_slots: false,
            },
            roles: BTreeMap::new(),
            icon_material: IconMaterial::Flat,
            font_family: FontFamily::default(),
            mono_font_family: FontFamily::JetBrainsMono,
        }
    }
}

/// The metrics walk behind [`Theme::apply_metrics`]. One fused pass:
/// the tree is large and cold (`El` is a wide struct), so traversal
/// count dominates — the three per-node applications (metrics recipes,
/// proportional family, mono family) are node-local and
/// order-independent.
///
/// A virtual list realizes its rows *during the layout pass*, after
/// this walk has already run over the tree — so on visiting a
/// virtual-list node the walk re-wraps its `build_row`: every row it
/// later realizes passes through this same function before sizing,
/// exactly as its non-virtual siblings did. Without the wrap, realized
/// rows silently keep stock metrics — invisible under a default theme,
/// wrong under any theme that moves a scale (first seen as
/// stock-shadcn 36px buttons inside a workbench-density history list).
/// Nested virtual lists compose: wrapping a row wraps the lists inside
/// it, one level per realization.
fn apply_metrics_tree(
    metrics: &crate::metrics::ThemeMetrics,
    font_family: FontFamily,
    mono_font_family: FontFamily,
    el: &mut crate::El,
) {
    metrics.apply_node(el);
    if !el.explicit_font_family {
        el.font_family = font_family;
    }
    if !el.explicit_mono_font_family {
        el.mono_font_family = mono_font_family;
    }
    if let Some(items) = el.virtual_items.as_deref_mut() {
        let metrics = metrics.clone();
        let inner = std::sync::Arc::clone(&items.build_row);
        items.build_row = std::sync::Arc::new(move |i| {
            let mut row = inner(i);
            apply_metrics_tree(&metrics, font_family, mono_font_family, &mut row);
            row
        });
    }
    for child in &mut el.children {
        apply_metrics_tree(metrics, font_family, mono_font_family, child);
    }
}

#[derive(Clone, Debug)]
struct SurfaceTheme {
    handle: ShaderHandle,
    uniforms: UniformBlock,
    rounded_rect_slots: bool,
}

fn add_rounded_rect_slots(uniforms: &mut UniformBlock) {
    if let Some(fill) = uniforms.get("fill").copied() {
        uniforms.entry("vec_a").or_insert(fill);
    }
    if let Some(stroke) = uniforms.get("stroke").copied() {
        uniforms.entry("vec_b").or_insert(stroke);
    }

    let stroke_width = as_f32(uniforms.get("stroke_width")).unwrap_or(0.0);
    let radius = as_f32(uniforms.get("radius")).unwrap_or(0.0);
    let shadow = as_f32(uniforms.get("shadow")).unwrap_or(0.0);
    let focus_width = as_f32(uniforms.get("focus_width")).unwrap_or(0.0);
    uniforms.entry("vec_c").or_insert(UniformValue::Vec4([
        stroke_width,
        radius,
        shadow,
        focus_width,
    ]));

    if let Some(focus_color) = uniforms.get("focus_color").copied() {
        uniforms.entry("vec_d").or_insert(focus_color);
    }
}

fn apply_role_material(
    role: SurfaceRole,
    uniforms: &mut UniformBlock,
    palette: &Palette,
    shadow_scale: f32,
) {
    // Role shadow *defaults* are theme elevation just like the tiers
    // widget recipes bake in, so the theme's shadow scale applies to
    // both — otherwise a `Panel` default would resurrect the very
    // shadow the metrics pass flattened on `card()`. A scaled-away
    // default is omitted (not written as 0.0) so a role-uniform
    // override can still re-elevate the role; the downstream reader
    // treats an absent shadow as none.
    let default_shadow = |uniforms: &mut UniformBlock, tier: f32| {
        let scaled = tier * shadow_scale;
        if scaled > 0.0 {
            default_f32(uniforms, "shadow", scaled);
        }
    };
    // Sunken/Input fill is derived from `muted` by darken, so the
    // base must be palette-resolved *before* the op — otherwise the
    // op runs on the compile-time dark fallback and the surface stays
    // dark even with a light palette active. Same shape for any future
    // role that derives an rgb-modified color from a token.
    match role {
        SurfaceRole::None => {}
        SurfaceRole::Panel => {
            set_color(uniforms, "stroke", tokens::BORDER.with_alpha_u8(210));
            set_f32(uniforms, "stroke_width", 1.0);
            // Shadow is a *default*, not an override: the widget (or
            // app) declares its elevation tier and the role only fills
            // the gap — an override here would silently clobber e.g. a
            // dialog's larger declared shadow (the pre-0.4.7 behavior,
            // which rendered popovers at the dialog tier).
            default_shadow(uniforms, tokens::SHADOW_SM);
        }
        SurfaceRole::Raised => {
            default_color(uniforms, "stroke", tokens::BORDER);
            default_f32(uniforms, "stroke_width", 1.0);
            default_shadow(uniforms, tokens::SHADOW_XS);
        }
        SurfaceRole::Sunken | SurfaceRole::Input => {
            // Defaults, not overrides (see Panel): the input/sunken
            // recipes leave fill and stroke unset so the role provides
            // the trough, and an authored `.fill()`/`.stroke()` wins —
            // through 0.6 these were `set_`, which silently discarded
            // authored paint on every input-role control and made an
            // accent-ringed or transparent field unauthorable. Shadow
            // is simply not written: absent reads as flat, and an
            // author-declared elevation survives.
            default_color(
                uniforms,
                "fill",
                palette.resolve(tokens::MUTED).darken(0.08),
            );
            default_color(uniforms, "stroke", tokens::INPUT.with_alpha_u8(190));
            default_f32(uniforms, "stroke_width", 1.0);
        }
        SurfaceRole::Popover => {
            set_color(uniforms, "stroke", tokens::INPUT);
            set_f32(uniforms, "stroke_width", 1.0);
            // Default (see Panel): the shadcn tier for the popover
            // family is `shadow-md`; dialogs/sheets share this role
            // but declare `SHADOW_LG` themselves and keep it.
            default_shadow(uniforms, tokens::SHADOW_MD);
        }
        SurfaceRole::Selected => {
            default_color(uniforms, "fill", tokens::PRIMARY.with_alpha_u8(28));
            set_color(uniforms, "stroke", tokens::PRIMARY.with_alpha_u8(110));
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
        SurfaceRole::Current => {
            default_color(uniforms, "fill", tokens::ACCENT);
            set_color(uniforms, "stroke", tokens::BORDER.with_alpha_u8(180));
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
        SurfaceRole::Danger => {
            set_color(uniforms, "stroke", tokens::DESTRUCTIVE);
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
    }
}

fn default_color(uniforms: &mut UniformBlock, key: &'static str, color: Color) {
    uniforms.entry(key).or_insert(UniformValue::Color(color));
}

fn set_color(uniforms: &mut UniformBlock, key: &'static str, color: Color) {
    uniforms.insert(key, UniformValue::Color(color));
}

fn default_f32(uniforms: &mut UniformBlock, key: &'static str, value: f32) {
    uniforms.entry(key).or_insert(UniformValue::F32(value));
}

fn set_f32(uniforms: &mut UniformBlock, key: &'static str, value: f32) {
    uniforms.insert(key, UniformValue::F32(value));
}

fn as_f32(value: Option<&UniformValue>) -> Option<f32> {
    match value {
        Some(UniformValue::F32(v)) => Some(*v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::column;
    use crate::widgets::text::text;

    #[test]
    fn theme_can_route_icon_material() {
        let theme = Theme::default().with_icon_material(IconMaterial::Relief);
        assert_eq!(theme.icon_material(), IconMaterial::Relief);
    }

    /// Virtual-list rows are realized by the layout pass, after
    /// `apply_metrics` has walked the tree — the walk must wrap
    /// `build_row` so realized rows still take theme metrics. Regression:
    /// under an `Xs`-sized theme, a button inside a realized row used to
    /// keep the stock `Md` height (first seen as stock-shadcn 36px
    /// buttons inside a workbench-density history list).
    #[test]
    fn virtual_list_rows_take_theme_metrics() {
        use crate::metrics::ComponentSize;
        use crate::widgets::button::button;

        let theme = Theme::default().with_default_component_size(ComponentSize::Xs);
        // A sibling button outside the list, for the expected geometry.
        let mut reference = button("ref");
        theme.apply_metrics(&mut reference);
        let xs_height = reference.height;
        assert_ne!(
            xs_height,
            button("raw").height,
            "Xs must differ from stock for this test to bite"
        );

        let mut root = crate::tree::virtual_list(10, 50.0, |i| {
            column([button(format!("row {i}")).key(format!("btn-{i}"))])
        });
        theme.apply_metrics(&mut root);
        let mut state = crate::state::UiState::new();
        crate::layout::assign_ids(&mut root);
        crate::layout::layout(&mut root, &mut state, crate::Rect::new(0.0, 0.0, 300.0, 200.0));

        assert!(!root.children.is_empty(), "rows should be realized");
        for row in &root.children {
            let btn = &row.children[0];
            assert_eq!(
                btn.height, xs_height,
                "realized row button must carry the theme's control height"
            );
        }
    }

    #[test]
    fn scrollbar_gutter_composes_with_padding_in_any_order() {
        use crate::tree::Sides;

        // gutter before padding
        let mut a = crate::tree::scroll([text("x")])
            .scrollbar_gutter()
            .padding(Sides::all(8.0));
        // padding before gutter
        let mut b = crate::tree::scroll([text("x")])
            .padding(Sides::all(8.0))
            .scrollbar_gutter();
        Theme::default().apply_metrics(&mut a);
        Theme::default().apply_metrics(&mut b);

        let expected = 8.0 + crate::tokens::SCROLLBAR_GUTTER;
        assert_eq!(a.padding.right, expected);
        assert_eq!(b.padding.right, expected);
        // Other sides untouched.
        assert_eq!(a.padding.left, 8.0);
        assert_eq!(b.padding.top, 8.0);
    }

    #[test]
    fn theme_font_family_applies_to_unset_text_nodes() {
        let mut root = column([text("Themed")]);
        Theme::default()
            .with_font_family(FontFamily::Inter)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].font_family, FontFamily::Inter);
    }

    #[test]
    fn explicit_font_family_survives_theme_default() {
        let mut root = column([text("Pinned").roboto()]);
        Theme::default()
            .with_font_family(FontFamily::Inter)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].font_family, FontFamily::Roboto);
    }

    #[test]
    fn theme_mono_font_family_applies_to_unset_text_nodes() {
        let mut root = column([text("code()").code()]);
        Theme::default().apply_metrics(&mut root);

        // Default theme value is JetBrainsMono — propagated through
        // the `apply_mono_font_family` walk to every text-bearing node
        // that didn't pin its own.
        assert_eq!(root.children[0].mono_font_family, FontFamily::JetBrainsMono);
    }

    #[test]
    fn theme_mono_font_family_swap_is_independent_from_proportional() {
        let mut root = column([text("body"), text("code()").code()]);
        Theme::default()
            .with_font_family(FontFamily::Inter)
            .with_mono_font_family(FontFamily::Roboto)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].font_family, FontFamily::Inter);
        assert_eq!(root.children[1].mono_font_family, FontFamily::Roboto);
        // Proportional slot stays Inter even on the code node.
        assert_eq!(root.children[1].font_family, FontFamily::Inter);
    }

    #[test]
    fn explicit_mono_font_family_survives_theme_default() {
        let mut root = column([text("Pinned").code().jetbrains_mono()]);
        Theme::default()
            .with_mono_font_family(FontFamily::Roboto)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].mono_font_family, FontFamily::JetBrainsMono);
    }
}
