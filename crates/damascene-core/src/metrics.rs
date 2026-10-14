//! Component sizing vocabulary.
//!
//! Stock controls (button / input / badge / tab / choice / slider /
//! progress) carry a t-shirt `size`. `Xs` … `Lg` map 1:1 to shadcn's
//! `size` prop; [`ComponentSize::Xxs`] extends the ladder one rung
//! *below* shadcn for dense chrome strips, which the web genre has no
//! prop for. Container surfaces (card / form / list / menu / panel)
//! bake their padding / gap / height / radius recipes directly in their
//! constructors — there is no global density knob, the way Tailwind /
//! shadcn picks padding per component class.
//!
//! **Table cells are the one container surface on the ladder**
//! (`table_cell` / `table_head`), because a table's row pitch is the
//! single number that decides whether a data view reads as dense — see
//! `table_cell_metrics` for the derivation, and
//! `docs/VOCABULARY_PARITY.md` §"Rejected: container density / size
//! props" (the "Partially reversed 2026-07-28" box) for why that ruling
//! was reopened for this one metric — and for what stays rejected: no
//! per-role container size prop, no global density knob.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::tree::{El, RadiusOrigin, Sides, Size};

/// T-shirt size for stock controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[non_exhaustive]
pub enum ComponentSize {
    /// Extra-extra small — the dense-chrome rung (22 px control
    /// height). Below [`Xs`](Self::Xs), and below shadcn's ladder
    /// entirely.
    ///
    /// # What it is for
    ///
    /// The strips a workbench frames its content with — title bars,
    /// status bars, pane headers, toolbars — where the *whole strip* is
    /// 22–30 px tall. An [`Xs`](Self::Xs) control is 28 px and simply
    /// does not fit in one; `Xxs` is the rung that does. It is a chrome
    /// rung, not an app baseline: reach for it per element
    /// (`.size(ComponentSize::Xxs)`) or per role
    /// ([`ThemeMetrics::with_button_size`]), not as a theme-wide default
    /// (see [`ThemeMetrics::with_default_component_size`]).
    ///
    /// # Evidence
    ///
    /// Workbench validation (2026-07) found that agents building a 30 px
    /// title bar could not place a 28 px `Xs` control in it and
    /// hardcoded 22 px heights instead; the reference corpus measured
    /// real chrome controls at 22–26 px, and even shadcn-fluent agents
    /// reached for `h-7!`-style overrides. Both
    /// `damascene_workbench::tokens::STATUS_BAR_HEIGHT` and
    /// `PANE_HEADER_HEIGHT` are 22 px, which is where the control height
    /// comes from: a chrome control is exactly as tall as the bar it
    /// sits in, and 22 px clears a 30 px title strip with room for the
    /// 2 px focus ring (`tokens::RING_WIDTH`) on both sides.
    ///
    /// # Derivation
    ///
    /// The control height is the measured 22 px. Every other value
    /// continues the step this ladder already takes from `Sm` down to
    /// `Xs`, so the rung's internal proportions are the ladder's, not a
    /// fresh set of hand-picked numbers:
    ///
    /// | metric | `Sm` | `Xs` | `Xxs` | step |
    /// |---|---|---|---|---|
    /// | control height | 32 | 28 | **22** | pinned to the 22 px bar height |
    /// | control padding-x | 10 | 8 | **6** | −2 |
    /// | control radius | 6 | 5 | **4** | −1 |
    /// | control gap | 6 | 4 | **2** | −2 |
    /// | badge height | 20 | 18 | **16** | −2 |
    /// | badge padding-x | 8 | 6 | **4** | −2 |
    /// | choice box edge | 16 | 14 | **12** | −2 |
    /// | switch track | 20 | 16 | **14** | −2; also keeps the +2 px it runs above the choice box at `Xs` |
    /// | slider track | 16 | 14 | **12** | −2 |
    /// | progress height | 6 | 4 | **2** | −2 |
    ///
    /// No type rung comes with it, because `Xs` introduces none either —
    /// control heights and type density are separate knobs
    /// ([`crate::Theme::with_type_scale`] is the type one). And one
    /// deliberate *non*-continuation: [`MetricsRole::Input`]'s 10 px
    /// horizontal-padding floor still applies, so an `Xxs` field is
    /// 22 px tall but keeps a button-unlike 10 px text gutter — that
    /// floor is about the caret, not about the rung.
    Xxs,
    /// Extra small — shadcn's densest rung (28 px control height). For
    /// chrome denser than shadcn goes, see [`Xxs`](Self::Xxs).
    Xs,
    /// Small (32 px control height) — [`ThemeMetrics`]' baseline default.
    Sm,
    /// Medium (36 px control height) — this enum's `Default`, matching
    /// shadcn's web baseline.
    #[default]
    Md,
    /// Large (40 px control height; text inputs get 44 px).
    Lg,
}

/// Theme-facing stock metrics role for a widget surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MetricsRole {
    /// Push button. The metrics pass stamps height, horizontal padding,
    /// corner radius, and gap from the resolved [`ComponentSize`].
    Button,
    /// Square icon-only button — width is forced equal to the height,
    /// with no horizontal padding.
    IconButton,
    /// Single-line text field. Sized like a button, with a slightly
    /// wider padding floor and a taller `Lg` height (44 px).
    Input,
    /// Multi-line text field. Bakes its padding + radius recipe in the
    /// constructor (`widgets/text_area.rs`); the metrics pass leaves it
    /// alone.
    TextArea,
    /// Status badge. The metrics pass stamps height and horizontal
    /// padding from the resolved [`ComponentSize`].
    Badge,
    /// Card surface. Padding / gap / radius are baked into the
    /// constructors (`widgets/card.rs`); the metrics pass only
    /// propagates the card's corner radii onto a filled leading header
    /// / trailing footer child so the strip doesn't poke past the
    /// card's rounded corners.
    Card,
    /// Card header slot — recipe baked in the constructor; may inherit
    /// the card's top corner radii (see [`MetricsRole::Card`]).
    CardHeader,
    /// Card body slot — recipe baked in the constructor; untouched by
    /// the metrics pass.
    CardContent,
    /// Card footer slot — recipe baked in the constructor; may inherit
    /// the card's bottom corner radii (see [`MetricsRole::Card`]).
    CardFooter,
    /// Form container — recipe baked in the constructor; untouched by
    /// the metrics pass.
    Form,
    /// Form item (label + control + hint) — recipe baked in the
    /// constructor; untouched by the metrics pass.
    FormItem,
    /// Generic panel surface — recipe baked in the constructor;
    /// untouched by the metrics pass.
    Panel,
    /// Menu row — recipe baked in the constructor; untouched by the
    /// metrics pass.
    MenuItem,
    /// List row — recipe baked in the constructor; untouched by the
    /// metrics pass.
    ListItem,
    /// Settings / preference row — recipe baked in the constructor;
    /// untouched by the metrics pass.
    PreferenceRow,
    /// Table header row. The row's own recipe (gap, radius, stretch) is
    /// baked in the constructor; the metrics pass stamps the resolved
    /// [`ComponentSize`]'s cell padding onto its `table_head` children —
    /// see [`MetricsRole::TableRow`].
    TableHeader,
    /// Table body row. The row's own recipe is baked in the constructor,
    /// but the metrics pass stamps the resolved [`ComponentSize`]'s
    /// **cell padding** onto every child cell that has no explicit
    /// `.padding(...)` — the row pitch is the ladder's one container
    /// metric (`table_cell_metrics`).
    TableRow,
    /// Tab trigger button — stamped with the same control metrics as
    /// [`MetricsRole::Button`].
    TabTrigger,
    /// Tab strip container — recipe baked in `tabs_list()`; the metrics
    /// pass only propagates an explicit [`ComponentSize`] down to
    /// [`MetricsRole::TabTrigger`] children.
    TabList,
    /// Square checkbox / radio control box — width and height are set
    /// to the scale's edge length (12–18 px).
    ChoiceControl,
    /// Checkbox / radio row — recipe baked in the constructor; the
    /// metrics pass only propagates an explicit [`ComponentSize`] down
    /// to the [`MetricsRole::ChoiceControl`] child.
    ChoiceItem,
    /// Slider track — the metrics pass stamps the height (12–22 px)
    /// from the resolved [`ComponentSize`].
    Slider,
    /// Switch control — the metrics pass scales the whole track
    /// (width, height, and the thumb's slide translate) proportionally
    /// from the resolved [`ComponentSize`], governed by the same
    /// choice-size knob as checkbox / radio.
    Switch,
    /// Progress bar — the metrics pass stamps the height (2–10 px)
    /// from the resolved [`ComponentSize`].
    Progress,
}

/// Theme-owned layout metrics for stock widgets.
#[derive(Clone, Debug)]
pub struct ThemeMetrics {
    default_component_size: ComponentSize,
    button_size: Option<ComponentSize>,
    input_size: Option<ComponentSize>,
    badge_size: Option<ComponentSize>,
    tab_size: Option<ComponentSize>,
    choice_size: Option<ComponentSize>,
    slider_size: Option<ComponentSize>,
    progress_size: Option<ComponentSize>,
    radius_scale: f32,
    shadow_scale: f32,
    type_scale: f32,
}

impl ThemeMetrics {
    /// Same as `Default`: [`ComponentSize::Sm`] baseline, no per-role
    /// overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// The size applied when neither the element (`.size(...)`) nor a
    /// per-role override specifies one. [`ComponentSize::Sm`] by
    /// default.
    pub fn default_component_size(&self) -> ComponentSize {
        self.default_component_size
    }

    /// Set the theme-wide default [`ComponentSize`] for all stock
    /// controls. Per-role overrides and per-element `.size(...)` still
    /// win.
    ///
    /// Any rung is accepted, [`ComponentSize::Xxs`] included, but that
    /// one is a *chrome* rung — 22 px controls with 2 px gaps read as
    /// instrumentation, and an app whose forms and dialogs are all
    /// `Xxs` looks broken rather than dense. Set it per element
    /// (`.size(ComponentSize::Xxs)`) or per role
    /// ([`Self::with_button_size`]) on the bars that need it, and leave
    /// the app default at [`ComponentSize::Xs`] or above.
    pub fn with_default_component_size(mut self, size: ComponentSize) -> Self {
        self.default_component_size = size;
        self
    }

    /// Override the size for buttons and icon buttons (beats the theme
    /// default; a per-element `.size(...)` still wins).
    pub fn with_button_size(mut self, size: ComponentSize) -> Self {
        self.button_size = Some(size);
        self
    }

    /// Override the size for text inputs (beats the theme default; a
    /// per-element `.size(...)` still wins).
    pub fn with_input_size(mut self, size: ComponentSize) -> Self {
        self.input_size = Some(size);
        self
    }

    /// Override the size for badges (beats the theme default; a
    /// per-element `.size(...)` still wins).
    pub fn with_badge_size(mut self, size: ComponentSize) -> Self {
        self.badge_size = Some(size);
        self
    }

    /// Override the size for tab triggers (beats the theme default; a
    /// per-element `.size(...)` still wins).
    pub fn with_tab_size(mut self, size: ComponentSize) -> Self {
        self.tab_size = Some(size);
        self
    }

    /// Override the size for checkbox / radio control boxes and
    /// switches (beats the theme default; a per-element `.size(...)`
    /// still wins).
    pub fn with_choice_size(mut self, size: ComponentSize) -> Self {
        self.choice_size = Some(size);
        self
    }

    /// Override the slider track height's size rung (beats the theme
    /// default; a per-element `.size(...)` still wins).
    pub fn with_slider_size(mut self, size: ComponentSize) -> Self {
        self.slider_size = Some(size);
        self
    }

    /// Override the progress bar height's size rung (beats the theme
    /// default; a per-element `.size(...)` still wins).
    pub fn with_progress_size(mut self, size: ComponentSize) -> Self {
        self.progress_size = Some(size);
        self
    }

    /// The multiplier applied to every theme-default corner radius.
    /// `1.0` by default — see [`Self::with_radius_scale`].
    pub fn radius_scale(&self) -> f32 {
        self.radius_scale
    }

    /// Scale every theme-default corner radius in the tree — shadcn's
    /// one-line `--radius` knob. See [`crate::Theme::with_radius_scale`]
    /// for the exemptions.
    pub fn with_radius_scale(mut self, scale: f32) -> Self {
        // Negative / NaN would paint garbage corners; `f32::max` maps
        // both to the square end of the range.
        self.radius_scale = scale.max(0.0);
        self
    }

    /// The multiplier applied to every theme-default drop shadow.
    /// `1.0` by default — see [`Self::with_shadow_scale`].
    pub fn shadow_scale(&self) -> f32 {
        self.shadow_scale
    }

    /// Scale every theme-default drop shadow in the tree — the
    /// elevation counterpart of [`Self::with_radius_scale`]. See
    /// [`crate::Theme::with_shadow_scale`] for what survives.
    pub fn with_shadow_scale(mut self, scale: f32) -> Self {
        // Same clamp rationale as the radius knob.
        self.shadow_scale = scale.max(0.0);
        self
    }

    /// The multiplier applied to every role- and rung-derived type
    /// size. `1.0` by default — see [`Self::with_type_scale`].
    pub fn type_scale(&self) -> f32 {
        self.type_scale
    }

    /// Scale every role- and rung-derived font size and line height in
    /// the tree — the rem analogue. See
    /// [`crate::Theme::with_type_scale`] for what survives.
    pub fn with_type_scale(mut self, scale: f32) -> Self {
        // A zero or negative type scale would erase all text; clamp to
        // a floor that keeps glyphs renderable instead of silently
        // blanking the app.
        self.type_scale = scale.max(0.05);
        self
    }

    /// Tree-walking form, retained for the unit tests below; the
    /// production path is `Theme::apply_metrics`'s fused walk.
    #[cfg(test)]
    pub(crate) fn apply_to_tree(&self, root: &mut El) {
        self.apply_node(root);
        for child in &mut root.children {
            self.apply_to_tree(child);
        }
    }

    /// Node-local half of the metrics pass; `Theme::apply_metrics`
    /// drives the tree walk so it can fuse the font-family stamps into
    /// the same traversal.
    pub(crate) fn apply_node(&self, el: &mut El) {
        self.apply_to_el(el);
        // Resolve `El::scrollbar_gutter` after any role recipe has
        // stamped its padding: the gutter is additive on top of the
        // node's final padding, which is what makes it compose with
        // `.padding(...)` in any call order (and with density-driven
        // padding on un-explicit nodes).
        if el.scrollbar_gutter {
            el.padding.right += crate::tokens::SCROLLBAR_GUTTER;
        }
        // The role recipes above stamp theme-default radii of their own
        // (`apply_control`), and those are as much a theme default as a
        // constructor's `default_radius(...)` — so they scale too.
        apply_radius_scale(el, self.radius_scale);
        apply_shadow_scale(el, self.shadow_scale);
        apply_type_scale(el, self.type_scale);
        // Card corner inheritance runs on the card's FINAL corners —
        // after the scale, so an explicit (scale-exempt) card and a
        // scaled card both hand their strips exactly the curve they
        // will paint. The stamped strips are marked exempt from their
        // own later `apply_radius_scale` visit (see the propagation
        // body), which is what keeps them in sync in both cases.
        if el.metrics_role == Some(MetricsRole::Card) {
            propagate_card_corner_radii(el);
        }
    }

    fn apply_to_el(&self, el: &mut El) {
        match el.metrics_role {
            Some(MetricsRole::Button) => {
                let size = el
                    .component_size
                    .or(self.button_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Button));
            }
            Some(MetricsRole::IconButton) => {
                let size = el
                    .component_size
                    .or(self.button_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::IconButton));
            }
            Some(MetricsRole::Input) => {
                let size = el
                    .component_size
                    .or(self.input_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Input));
            }
            Some(MetricsRole::TextArea) => {
                // TextArea bakes its padding + radius recipe directly
                // in the constructor (`widgets/text_area.rs`). The
                // metrics pass leaves it alone.
            }
            Some(MetricsRole::Badge) => {
                let size = el
                    .component_size
                    .or(self.badge_size)
                    .unwrap_or(self.default_component_size);
                apply_badge(el, badge_metrics(size));
            }
            Some(MetricsRole::Card) => {
                // Card surfaces do not participate in the metrics-driven
                // density override. Padding, gap, and radius are baked
                // into the constructors in `widgets/card.rs` (shadcn's
                // stock recipe). Override per-call with `.padding(...)`,
                // `.pt(...)` / `.px(...)` / etc.
                //
                // What the metrics pass *does* do for a card: propagate
                // the card's top corner radii onto a leading
                // `card_header` child's *fill* (and symmetric for a
                // trailing `card_footer`). Without this, a
                // `card_header([...]).fill(MUTED)` strip paints sharp
                // top corners that poke past the card's rounded curve;
                // see `propagate_card_corner_radii` (called from
                // `apply_node` after the radius scale, so the strips
                // inherit the card's final corners).
                restore_headerless_card_content_padding(el);
            }
            Some(MetricsRole::CardHeader | MetricsRole::CardContent | MetricsRole::CardFooter) => {
                // See above: padding / gap / radius baked into the
                // constructors. Corner-radii inheritance is stamped by
                // the parent `Card` branch above.
            }
            Some(
                MetricsRole::Form
                | MetricsRole::FormItem
                | MetricsRole::Panel
                | MetricsRole::MenuItem
                | MetricsRole::ListItem
                | MetricsRole::PreferenceRow,
            ) => {
                // These surfaces bake their padding / gap / height /
                // radius recipe directly in their constructors (see
                // `widgets/{form,alert,dialog,sheet,overlay,popover,
                // dropdown_menu,accordion,sidebar,command}.rs`).
                // The metrics pass does not touch them. Override per
                // call with `.padding(...)` / `.height(...)` / etc.
            }
            Some(MetricsRole::TableHeader | MetricsRole::TableRow) => {
                // The row's own recipe (zero gap, square corners,
                // stretch, inside focus ring) is the constructor's; the
                // rung owns the *cells'* padding, which is what sets the
                // row pitch. A row may name its own rung with
                // `.size(...)`, the way a `tabs_list` does.
                let size = el.component_size.unwrap_or(self.default_component_size);
                apply_table_cell_padding(el, table_cell_metrics(size, self.type_scale));
            }
            Some(MetricsRole::TabTrigger) => {
                let size = el
                    .component_size
                    .or(self.tab_size)
                    .unwrap_or(self.default_component_size);
                apply_control(el, control_metrics(size, ControlKind::Button));
            }
            Some(MetricsRole::TabList) => {
                // Padding, gap, and radius are baked into
                // `tabs_list()`. The metrics pass only propagates the
                // optional `ComponentSize` down to TabTrigger children.
                if let Some(size) = el.component_size {
                    apply_tab_trigger_size_to_children(el, size);
                }
            }
            Some(MetricsRole::ChoiceControl) => {
                let size = el
                    .component_size
                    .or(self.choice_size)
                    .unwrap_or(self.default_component_size);
                apply_choice_control(el, choice_control_metrics(size));
            }
            Some(MetricsRole::ChoiceItem) => {
                // Padding, gap, and radius are baked into `radio_item()`.
                // The metrics pass only propagates `ComponentSize` down
                // to the ChoiceControl child.
                if let Some(size) = el.component_size {
                    apply_choice_control_size_to_children(el, size);
                }
            }
            Some(MetricsRole::Slider) => {
                let size = el
                    .component_size
                    .or(self.slider_size)
                    .unwrap_or(self.default_component_size);
                apply_single_axis_height(el, slider_metrics(size));
            }
            Some(MetricsRole::Switch) => {
                let size = el
                    .component_size
                    .or(self.choice_size)
                    .unwrap_or(self.default_component_size);
                apply_switch(el, switch_metrics(size));
            }
            Some(MetricsRole::Progress) => {
                let size = el
                    .component_size
                    .or(self.progress_size)
                    .unwrap_or(self.default_component_size);
                apply_single_axis_height(el, progress_metrics(size));
            }
            None => {}
        }
    }
}

impl Default for ThemeMetrics {
    fn default() -> Self {
        Self {
            // Damascene's baseline component size is `Sm` so desktop apps
            // land in a denser-than-web baseline. Bump everything one
            // rung with `Theme::with_default_component_size(Md)`, or
            // override per-call with `.size(...)` / `.medium()` /
            // `.large()`.
            default_component_size: ComponentSize::Sm,
            button_size: None,
            input_size: None,
            badge_size: None,
            tab_size: None,
            choice_size: None,
            slider_size: None,
            progress_size: None,
            // Identity: the metrics pass skips the rescale entirely at
            // 1.0, so stock radii, shadows, and type stay bit-identical.
            radius_scale: 1.0,
            shadow_scale: 1.0,
            type_scale: 1.0,
        }
    }
}

/// Rescale a node's theme-default corner radii by the theme's radius
/// scale.
///
/// Three carve-outs, each load-bearing:
/// - [`RadiusOrigin::Fixed`] — the value is final (author `.radius()`,
///   or an in-pass finalization like card corner inheritance) and the
///   scale must not touch it. [`RadiusOrigin::LibraryShape`] values DO
///   scale: a widget-stamped silhouette (tab-strip edge triggers) is
///   shape-owned but magnitude-themed, so it squares at `0.0` along
///   with everything else — matching the web, where shadcn's trigger
///   radius derives from `--radius`.
/// - Zero corners stay square, so per-corner silhouettes built with
///   [`crate::tree::Corners::top`] and friends survive the rescale as
///   shapes rather than collapsing to a uniform radius.
/// - Corners at or above [`crate::tokens::RADIUS_PILL`] are exempt,
///   matching the web: `rounded-full` does not read `--radius`.
fn apply_radius_scale(el: &mut El, scale: f32) {
    if scale == 1.0 || el.radius_origin == RadiusOrigin::Fixed || !el.radius.any_nonzero() {
        return;
    }
    el.radius.tl = scale_corner(el.radius.tl, scale);
    el.radius.tr = scale_corner(el.radius.tr, scale);
    el.radius.br = scale_corner(el.radius.br, scale);
    el.radius.bl = scale_corner(el.radius.bl, scale);
}

fn scale_corner(radius: f32, scale: f32) -> f32 {
    if radius <= 0.0 || radius >= crate::tokens::RADIUS_PILL {
        radius
    } else {
        radius * scale
    }
}

/// Rescale a node's theme-default drop shadow by the theme's shadow
/// scale. Only [`El::shadow`] values a widget recipe baked in via
/// `default_shadow` scale; an author's explicit `.shadow(...)` is not
/// a theme default and survives untouched. The paint-side counterpart
/// — the surface-role shadow *defaults* (`Panel` / `Raised` /
/// `Popover` in `apply_role_material`) — scales by the same factor at
/// uniform-build time, so a role default can't resurrect a shadow this
/// pass flattened.
fn apply_shadow_scale(el: &mut El, scale: f32) {
    if scale == 1.0 || el.explicit_shadow || el.shadow == 0.0 {
        return;
    }
    el.shadow *= scale;
}

/// Rescale a node's role- and rung-derived type metrics by the theme's
/// type scale — the rem analogue: on the web the whole ladder scales
/// with the root font size while hand-picked px values don't. Roles
/// and rungs stamp raw `f32`s at construction, so by pass time the
/// token identity is gone; `explicit_font_size` is exactly the
/// remaining bit that distinguishes a themed size from an author's.
/// Line height scales with size — scaling size alone would wreck
/// vertical rhythm. Control heights do NOT scale with type; they are
/// the [`ComponentSize`] ladder's job.
fn apply_type_scale(el: &mut El, scale: f32) {
    if scale == 1.0 || el.explicit_font_size {
        return;
    }
    el.font_size *= scale;
    el.line_height *= scale;
}

#[derive(Clone, Copy)]
enum ControlKind {
    Button,
    IconButton,
    Input,
}

#[derive(Clone, Copy)]
struct ControlMetrics {
    height: f32,
    padding_x: f32,
    radius: f32,
    gap: f32,
}

fn control_metrics(size: ComponentSize, kind: ControlKind) -> ControlMetrics {
    let (mut height, padding_x, radius, gap): (f32, f32, f32, f32) = match size {
        // 22 px is the workbench bar height; the rest continue the
        // `Sm` → `Xs` steps (−2 padding, −1 radius, −2 gap). See the
        // `ComponentSize::Xxs` docs for the full derivation.
        ComponentSize::Xxs => (22.0, 6.0, 4.0, 2.0),
        ComponentSize::Xs => (28.0, 8.0, 5.0, 4.0),
        ComponentSize::Sm => (32.0, 10.0, 6.0, 6.0),
        ComponentSize::Md => (36.0, 12.0, 7.0, 8.0),
        ComponentSize::Lg => (40.0, 14.0, 8.0, 8.0),
    };
    if matches!(kind, ControlKind::Input) && matches!(size, ComponentSize::Lg) {
        height = 44.0;
    }
    match kind {
        ControlKind::IconButton => ControlMetrics {
            height,
            padding_x: 0.0,
            radius,
            gap,
        },
        ControlKind::Input => ControlMetrics {
            height,
            padding_x: padding_x.max(10.0),
            radius,
            gap,
        },
        ControlKind::Button => ControlMetrics {
            height,
            padding_x,
            radius,
            gap,
        },
    }
}

fn apply_control(el: &mut El, metrics: ControlMetrics) {
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.height);
    }
    if matches!(el.metrics_role, Some(MetricsRole::IconButton)) && !el.explicit_width {
        el.width = Size::Fixed(metrics.height);
    }
    if !el.explicit_padding && !matches!(el.metrics_role, Some(MetricsRole::IconButton)) {
        el.padding = Sides::xy(metrics.padding_x, 0.0);
    }
    if el.radius_origin == RadiusOrigin::ThemeDefault {
        el.radius = crate::tree::Corners::all(metrics.radius);
    }
    if !el.explicit_gap {
        el.gap = metrics.gap;
    }
}

#[derive(Clone, Copy)]
struct BadgeMetrics {
    height: f32,
    padding_x: f32,
}

fn badge_metrics(size: ComponentSize) -> BadgeMetrics {
    match size {
        // −2 on both axes from `Xs`, which lands the chrome rung on
        // 16 px — the height dense-tool status chips measure at.
        ComponentSize::Xxs => BadgeMetrics {
            height: 16.0,
            padding_x: 4.0,
        },
        ComponentSize::Xs => BadgeMetrics {
            height: 18.0,
            padding_x: 6.0,
        },
        ComponentSize::Sm => BadgeMetrics {
            height: 20.0,
            padding_x: 8.0,
        },
        ComponentSize::Md => BadgeMetrics {
            height: 24.0,
            padding_x: 8.0,
        },
        ComponentSize::Lg => BadgeMetrics {
            height: 28.0,
            padding_x: 10.0,
        },
    }
}

fn apply_badge(el: &mut El, metrics: BadgeMetrics) {
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.height);
    }
    if !el.explicit_padding {
        el.padding = Sides::xy(metrics.padding_x, 0.0);
    }
}

/// Padding for one table cell (`table_cell` / `table_head`) at a rung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TableCellMetrics {
    /// Horizontal gutter on each side of the cell's content.
    pub(crate) padding_x: f32,
    /// Vertical padding above and below the cell's content — the half
    /// that decides the row pitch.
    pub(crate) padding_y: f32,
}

/// The rung's table-cell padding.
///
/// # The invariant
///
/// **A table row's content box is exactly as tall as a control of the
/// same rung.** A row of text and a row holding a `button` / `select` /
/// `text_input` are then the same height, and a table under
/// `with_default_component_size(Xs)` is as dense as the controls around
/// it. The painted pitch is one pixel more, because `table_body` gives
/// every row but the last a 1 px `.border_b()` rule.
///
/// So the vertical padding is the *residue* after the nominal line box,
/// not a hand-picked number:
///
/// ```text
/// padding_y = (control_height(rung) − TEXT_SM.line_height × type_scale) / 2
/// ```
///
/// Taking the type scale into account is what keeps the invariant true:
/// [`crate::Theme::with_type_scale`] shrinks the line box, and the
/// padding grows back into it so the pitch stays pinned to the rung.
/// (Control heights themselves are deliberately type-scale-independent —
/// that is the whole point of having both knobs.) A type scale large
/// enough to overflow the rung clamps the padding at zero and lets the
/// row grow; text is never clipped to hold a pitch.
///
/// The horizontal gutter is simply the rung's **control padding-x**, so
/// a cell's text starts where a button's label would.
///
/// | rung | control height | `padding_x` | `padding_y` @ type scale 1.0 | row content box | painted pitch |
/// |---|---|---|---|---|---|
/// | `Xxs` | 22 | 6 | 1 | 22 | 23 |
/// | `Xs` | 28 | 8 | 4 | 28 | 29 |
/// | `Sm` | 32 | 10 | 6 | 32 | 33 |
/// | `Md` | 36 | **12** | **8** | **36** | 37 |
/// | `Lg` | 40 | 14 | 10 | 40 | 41 |
///
/// # Why `Md` is the pin
///
/// `Md` reproduces the constructors' historical hardcode exactly —
/// `Sides::xy(SPACE_3, SPACE_2)` = `xy(12, 8)` — so shadcn's own rung
/// still paints shadcn's own table, bit for bit. Every other rung is the
/// formula above, not a second set of picks.
///
/// # Evidence
///
/// The workbench reference corpus
/// (`references/workbench-validation/parts/`) declares its parts-index
/// rows as `tbody tr { height: 28px }` — the `Xs` control height, on the
/// nose — and renders them at a 30 px pitch (measured off
/// `reference.png`'s zebra seams: rules at y = 164, 194, 224, 254, 284).
/// The workbench theme's `Xs` default lands a stock table at 29 px, one
/// pixel under the reference's rendering and exactly on the box the
/// reference author wrote. Against the pre-ladder stock table's ~36 px
/// (measured 35.57 px under the workbench type scale), that is the
/// densification the corpus was asking for.
///
/// The damascene-side validation apps agree independently: both
/// `examples/parts.rs` and `examples/parts_v2.rs` hand-tightened cells
/// to `Sides::xy(SPACE_2, 5.0)` = `xy(8, 5)`, against this table's
/// `xy(8, 4.71)` at `Xs` under the workbench type scale. Those local
/// `ROW_PAD_Y` constants are what this ladder retires.
pub(crate) fn table_cell_metrics(size: ComponentSize, type_scale: f32) -> TableCellMetrics {
    let control = control_metrics(size, ControlKind::Button);
    let line_box = crate::tokens::TEXT_SM.line_height * type_scale;
    TableCellMetrics {
        padding_x: control.padding_x,
        // `max(0.0)` also maps NaN to zero, which is the safe end: a
        // garbage type scale must not paint negative padding.
        padding_y: ((control.height - line_box) / 2.0).max(0.0),
    }
}

/// Stamp the rung's cell padding onto a table row's cells.
///
/// Two deliberate details:
///
/// - **`explicit_padding` is the opt-out.** A cell the author padded
///   (`table_cell(x).padding(...)`, `.py(...)`) is skipped entirely, the
///   same contract every other rung-stamped metric honours.
/// - **The stamp claims the cell.** `table_cell` styles its content *in
///   place* rather than wrapping it, so `table_cell(badge("3"))` is a
///   node that already carries [`MetricsRole::Badge`]. Without the
///   claim, the badge's own recipe — visited later in this same
///   pre-order walk — would overwrite the cell chrome with badge
///   padding. This is the same "the value is final now" marking
///   `propagate_card_corner_radii` does with [`RadiusOrigin::Fixed`].
fn apply_table_cell_padding(row: &mut El, metrics: TableCellMetrics) {
    for cell in &mut row.children {
        if cell.explicit_padding {
            continue;
        }
        cell.padding = Sides::xy(metrics.padding_x, metrics.padding_y);
        cell.explicit_padding = true;
    }
}

fn apply_tab_trigger_size_to_children(el: &mut El, size: ComponentSize) {
    for child in &mut el.children {
        if matches!(child.metrics_role, Some(MetricsRole::TabTrigger))
            && child.component_size.is_none()
        {
            child.component_size = Some(size);
        }
    }
}

#[derive(Clone, Copy)]
struct ChoiceControlMetrics {
    edge: f32,
}

fn choice_control_metrics(size: ComponentSize) -> ChoiceControlMetrics {
    let edge = match size {
        // −2 from `Xs`. At this rung the check glyph
        // (`widgets::checkbox::CHECK_ICON_SIZE`, a rung-independent
        // 12 px) fills the box edge to edge, which is the intended
        // chrome look: a tick, not a tick inside a frame.
        ComponentSize::Xxs => 12.0,
        ComponentSize::Xs => 14.0,
        ComponentSize::Sm => 16.0,
        ComponentSize::Md => 16.0,
        ComponentSize::Lg => 18.0,
    };
    ChoiceControlMetrics { edge }
}

fn apply_choice_control(el: &mut El, metrics: ChoiceControlMetrics) {
    if !el.explicit_width {
        el.width = Size::Fixed(metrics.edge);
    }
    if !el.explicit_height {
        el.height = Size::Fixed(metrics.edge);
    }
}

fn apply_choice_control_size_to_children(el: &mut El, size: ComponentSize) {
    for child in &mut el.children {
        if matches!(child.metrics_role, Some(MetricsRole::ChoiceControl))
            && child.component_size.is_none()
        {
            child.component_size = Some(size);
        }
    }
}

/// Switch track height per scale. `Sm` (the baseline) equals the
/// widget's unscaled [`crate::widgets::switch::TRACK_HEIGHT`].
fn switch_metrics(size: ComponentSize) -> f32 {
    match size {
        // −2 from `Xs`, which also preserves the +2 px a switch track
        // runs above the choice box at that rung (16 vs 14 → 14 vs 12).
        ComponentSize::Xxs => 14.0,
        ComponentSize::Xs => 16.0,
        ComponentSize::Sm => crate::widgets::switch::TRACK_HEIGHT,
        ComponentSize::Md => 22.0,
        ComponentSize::Lg => 26.0,
    }
}

/// Scale a switch proportionally: track width/height from the scale's
/// track height, and the thumb child's ON-position translate by the
/// same ratio (the builder computed it at the default size). Explicit
/// `.width(...)` / `.height(...)` opt the control out entirely — the
/// builder's defaults stay self-consistent.
fn apply_switch(el: &mut El, track_height: f32) {
    use crate::widgets::switch::{TRACK_HEIGHT, TRACK_WIDTH};
    if el.explicit_width || el.explicit_height {
        return;
    }
    let ratio = track_height / TRACK_HEIGHT;
    el.width = Size::Fixed(TRACK_WIDTH * ratio);
    el.height = Size::Fixed(track_height);
    for child in &mut el.children {
        child.translate.0 *= ratio;
    }
}

fn slider_metrics(size: ComponentSize) -> f32 {
    match size {
        // −2 from `Xs`, the ladder's own step.
        ComponentSize::Xxs => 12.0,
        ComponentSize::Xs => 14.0,
        ComponentSize::Sm => 16.0,
        ComponentSize::Md => 18.0,
        ComponentSize::Lg => 22.0,
    }
}

fn progress_metrics(size: ComponentSize) -> f32 {
    match size {
        // −2 from `Xs` — the uniform step of this whole table — landing
        // on the 2 px hairline bar dense tools run inside chrome.
        ComponentSize::Xxs => 2.0,
        ComponentSize::Xs => 4.0,
        ComponentSize::Sm => 6.0,
        ComponentSize::Md => 8.0,
        ComponentSize::Lg => 10.0,
    }
}

fn apply_single_axis_height(el: &mut El, height: f32) {
    if !el.explicit_height {
        el.height = Size::Fixed(height);
    }
}

/// Propagate the parent card's top/bottom corner radii onto a leading
/// `card_header` / trailing `card_footer` child whose `.fill(...)` would
/// otherwise paint sharp corners over the card's rounded curve.
///
/// Triggers only when:
/// - The card has a non-zero corner radius (the only case the strip
///   pokes through).
/// - The card has zero padding on the corresponding edge (the slot's
///   own padding is inside it; the slot's outer rect is the card's
///   inner rect). If the card has top padding, the header's top is
///   inset from the card edge — no leak, no inheritance.
/// - The slot has `.fill(...)` set. A no-fill `card_header` doesn't
///   draw anything in the corner band, so corner inheritance would be
///   invisible (and could surprise authors who later add stroke).
/// - The slot has no explicit `.radius(...)` — author overrides win.
///
/// Top inherits from `card.radius.tl` / `tr`; bottom from `bl` / `br`.
/// The matching opposite corners are zeroed so the strip's interior
/// edge stays straight against the body slot.
fn propagate_card_corner_radii(card: &mut El) {
    if !card.radius.any_nonzero() || card.children.is_empty() {
        return;
    }
    let card_radius = card.radius;
    let pad_top = card.padding.top;
    let pad_bottom = card.padding.bottom;
    let last_idx = card.children.len() - 1;
    for (idx, child) in card.children.iter_mut().enumerate() {
        if child.fill.is_none() || child.radius_origin != RadiusOrigin::ThemeDefault {
            continue;
        }
        match child.metrics_role {
            Some(MetricsRole::CardHeader) if idx == 0 && pad_top == 0.0 => {
                child.radius = crate::tree::Corners {
                    tl: card_radius.tl,
                    tr: card_radius.tr,
                    br: 0.0,
                    bl: 0.0,
                };
                // The inherited corners are the card's FINAL (already
                // radius-scaled, or scale-exempt) curve; mark them
                // `Fixed` so the strip's own `apply_radius_scale`
                // visit doesn't rescale them out of sync with the card.
                // (`LibraryShape` would be wrong here: the walk is
                // pre-order, so the strip's scale visit comes AFTER
                // this stamp, and a scalable origin would double-scale
                // an already-final value.)
                child.radius_origin = RadiusOrigin::Fixed;
            }
            Some(MetricsRole::CardFooter) if idx == last_idx && pad_bottom == 0.0 => {
                child.radius = crate::tree::Corners {
                    tl: 0.0,
                    tr: 0.0,
                    br: card_radius.br,
                    bl: card_radius.bl,
                };
                child.radius_origin = RadiusOrigin::Fixed;
            }
            _ => {}
        }
    }
}

/// Restore a leading `card_content`'s top padding when it sits directly
/// under the card with no `card_header` above it.
///
/// `card_content` bakes shadcn's `p-6 pt-0` on the assumption that a
/// `card_header` supplies the gap above the body. A header-less
/// `card([card_content([...])])` — a natural thing to write — would
/// otherwise leave its first child flush against the card's top edge
/// (the `UnpaddedSurfacePanel` lint). When `card_content` is the leading
/// slot, restore the top padding so the common header-less card is
/// correct by default. Only the constructor default is touched: an
/// explicit `.padding(...)` / `.pt(...)` (e.g. the full-bleed
/// `card_content([scroll(...)]).padding(0.0)` recipe) sets
/// `explicit_padding` and wins.
fn restore_headerless_card_content_padding(card: &mut El) {
    let Some(first) = card.children.first_mut() else {
        return;
    };
    if first.metrics_role == Some(MetricsRole::CardContent)
        && !first.explicit_padding
        && first.padding.top == 0.0
    {
        first.padding.top = crate::tokens::SPACE_6;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, tabs_list, text_input, titled_card, tokens};

    #[test]
    fn choice_size_scales_switch_proportionally() {
        use crate::widgets::switch::{THUMB_SLIDE, TRACK_HEIGHT, TRACK_WIDTH, switch};
        let mut el = switch("s", true);
        ThemeMetrics::default()
            .with_choice_size(ComponentSize::Lg)
            .apply_to_tree(&mut el);
        let ratio = 26.0 / TRACK_HEIGHT;
        assert_eq!(el.height, Size::Fixed(26.0));
        assert_eq!(el.width, Size::Fixed(TRACK_WIDTH * ratio));
        // The thumb's ON-position translate rescales with the control
        // so it still lands exactly at the end of the track.
        let thumb = &el.children[1];
        assert!(
            (thumb.translate.0 - THUMB_SLIDE * ratio).abs() < 1e-3,
            "thumb translate {} should scale to {}",
            thumb.translate.0,
            THUMB_SLIDE * ratio,
        );
    }

    #[test]
    fn switch_with_default_metrics_keeps_its_documented_size() {
        use crate::widgets::switch::{TRACK_HEIGHT, TRACK_WIDTH, switch};
        let mut el = switch("s", false);
        ThemeMetrics::default().apply_to_tree(&mut el);
        assert_eq!(el.width, Size::Fixed(TRACK_WIDTH));
        assert_eq!(el.height, Size::Fixed(TRACK_HEIGHT));
    }

    #[test]
    fn explicit_size_opts_switch_out_of_metric_scaling() {
        use crate::widgets::switch::switch;
        let mut el = switch("s", true).width(Size::Fixed(50.0));
        let before_translate = el.children[1].translate.0;
        ThemeMetrics::default()
            .with_choice_size(ComponentSize::Lg)
            .apply_to_tree(&mut el);
        assert_eq!(el.width, Size::Fixed(50.0), "explicit width preserved");
        assert_eq!(
            el.children[1].translate.0, before_translate,
            "translate untouched when the author owns the size"
        );
    }

    #[test]
    fn theme_default_component_size_applies_to_stock_control() {
        let mut el = button("Save");

        ThemeMetrics::default()
            .with_default_component_size(ComponentSize::Lg)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(40.0));
    }

    #[test]
    fn local_component_size_overrides_theme_default() {
        let mut el = button("Save").large();

        ThemeMetrics::default()
            .with_default_component_size(ComponentSize::Xs)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(40.0));
    }

    #[test]
    fn input_uses_spacious_field_height_at_large_size() {
        let mut el = text_input("search", "Search", &crate::Selection::default()).large();

        ThemeMetrics::default().apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(44.0));
    }

    #[test]
    fn explicit_height_overrides_component_metrics() {
        let mut el = button("Save").height(Size::Fixed(44.0));

        ThemeMetrics::default()
            .with_default_component_size(ComponentSize::Sm)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(44.0));
    }

    #[test]
    fn card_slot_defaults_match_shadcn_stock() {
        // card_header / card_content / card_footer bake shadcn's `p-6`
        // / `p-6 pt-0` recipe directly via `default_padding(...)` in
        // the constructor. The metrics pass leaves those slots alone.
        let mut t = titled_card("Settings", [crate::text("Body")]);
        ThemeMetrics::default().apply_to_tree(&mut t);

        // Outer card is unpadded; the slots own all the spacing.
        assert_eq!(t.padding, Sides::zero());
        // Header: SPACE_6 on all four sides.
        assert_eq!(t.children[0].padding, Sides::all(tokens::SPACE_6));
        // Content: SPACE_6 on left / right / bottom, 0 on top (`p-6 pt-0`).
        assert_eq!(
            t.children[1].padding,
            Sides {
                left: tokens::SPACE_6,
                right: tokens::SPACE_6,
                top: 0.0,
                bottom: tokens::SPACE_6,
            }
        );
    }

    #[test]
    fn headerless_card_content_regains_top_padding() {
        use crate::{card, card_content, card_header, text};

        // No header: the leading card_content should regain its top
        // padding so the body isn't flush against the card edge.
        let mut headerless = card([card_content([text("Body")])]);
        ThemeMetrics::default().apply_to_tree(&mut headerless);
        assert_eq!(headerless.children[0].padding.top, tokens::SPACE_6);

        // With a header above it, card_content keeps the `pt-0` seam.
        let mut with_header = card([card_header([text("Header")]), card_content([text("Body")])]);
        ThemeMetrics::default().apply_to_tree(&mut with_header);
        assert_eq!(with_header.children[1].padding.top, 0.0);

        // An explicit full-bleed override wins — no restoration.
        let mut full_bleed = card([card_content([text("Body")]).padding(0.0)]);
        ThemeMetrics::default().apply_to_tree(&mut full_bleed);
        assert_eq!(full_bleed.children[0].padding.top, 0.0);
    }

    #[test]
    fn card_header_with_fill_inherits_card_top_corner_radii() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_header, text};
        // The canonical "tinted strip" recipe blessed by the
        // `card_header` doc comment. Without inheritance the strip
        // paints sharp top corners that poke past the card's curve.
        let mut tree = card([
            card_header([text("Header")]).fill(tokens::MUTED),
            card_content([text("Body")]),
        ]);
        ThemeMetrics::default().apply_to_tree(&mut tree);

        assert_eq!(
            tree.children[0].radius,
            Corners {
                tl: tokens::RADIUS_LG,
                tr: tokens::RADIUS_LG,
                br: 0.0,
                bl: 0.0,
            },
            "header strip should adopt the card's top corner radii"
        );
        // Body slot has no fill → no inheritance, no surprise.
        assert_eq!(tree.children[1].radius, Corners::ZERO);
    }

    #[test]
    fn card_footer_with_fill_inherits_card_bottom_corner_radii() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_footer, text};
        let mut tree = card([
            card_content([text("Body")]),
            card_footer([text("Footer")]).fill(tokens::MUTED),
        ]);
        ThemeMetrics::default().apply_to_tree(&mut tree);

        let footer = tree.children.last().expect("footer slot");
        assert_eq!(
            footer.radius,
            Corners {
                tl: 0.0,
                tr: 0.0,
                br: tokens::RADIUS_LG,
                bl: tokens::RADIUS_LG,
            }
        );
    }

    #[test]
    fn card_header_explicit_radius_wins_over_inheritance() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_header, text};
        let mut tree = card([
            card_header([text("Header")])
                .fill(tokens::MUTED)
                .radius(Corners::ZERO),
            card_content([text("Body")]),
        ]);
        ThemeMetrics::default().apply_to_tree(&mut tree);

        assert_eq!(
            tree.children[0].radius,
            Corners::ZERO,
            "author override must win over auto-inheritance"
        );
    }

    #[test]
    fn card_header_without_fill_does_not_inherit() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_header, text};
        let mut tree = card([card_header([text("Header")]), card_content([text("Body")])]);
        ThemeMetrics::default().apply_to_tree(&mut tree);
        assert_eq!(
            tree.children[0].radius,
            Corners::ZERO,
            "no fill means no corner stackup to fix"
        );
    }

    #[test]
    fn card_with_top_padding_skips_header_inheritance() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_header, text};
        // Explicit padding on the card insets the header from the
        // card's edge, so there's no corner stackup to inherit away.
        let mut tree = card([
            card_header([text("Header")]).fill(tokens::MUTED),
            card_content([text("Body")]),
        ])
        .padding(tokens::SPACE_2);
        ThemeMetrics::default().apply_to_tree(&mut tree);
        assert_eq!(tree.children[0].radius, Corners::ZERO);
    }

    #[test]
    fn theme_tab_size_applies_to_tab_triggers() {
        let mut el = tabs_list("settings", &"account", [("account", "Account")]);

        ThemeMetrics::default()
            .with_tab_size(ComponentSize::Lg)
            .apply_to_tree(&mut el);

        assert_eq!(el.children[0].height, Size::Fixed(40.0));
    }

    #[test]
    fn local_tab_list_size_applies_to_tab_triggers() {
        let mut el =
            tabs_list("settings", &"account", [("account", "Account")]).size(ComponentSize::Lg);

        ThemeMetrics::default().apply_to_tree(&mut el);

        assert_eq!(el.children[0].height, Size::Fixed(40.0));
    }

    #[test]
    fn local_choice_item_size_applies_to_child_control() {
        let control =
            El::new(crate::Kind::Custom("choice-control")).metrics_role(MetricsRole::ChoiceControl);
        let mut el = El::new(crate::Kind::Custom("choice"))
            .metrics_role(MetricsRole::ChoiceItem)
            .child(control)
            .size(ComponentSize::Lg);

        ThemeMetrics::default().apply_to_tree(&mut el);

        assert_eq!(el.children[0].width, Size::Fixed(18.0));
        assert_eq!(el.children[0].height, Size::Fixed(18.0));
    }

    #[test]
    fn progress_size_uses_component_scale() {
        let mut el = El::new(crate::Kind::Custom("progress")).metrics_role(MetricsRole::Progress);

        ThemeMetrics::default()
            .with_progress_size(ComponentSize::Sm)
            .apply_to_tree(&mut el);

        assert_eq!(el.height, Size::Fixed(6.0));
    }

    #[test]
    fn raw_metrics_role_tags_no_longer_override_widget_defaults() {
        // After the density removal, surfaces like Form / FormItem /
        // ListItem / MenuItem / TableRow / PreferenceRow / ChoiceItem /
        // TextArea / TabList / Panel bake their padding / gap / height /
        // radius recipes into their constructors. The metrics pass does
        // not stamp anything onto bare-tagged Els (it only reaches
        // *children* — ComponentSize down to TabTrigger / ChoiceControl,
        // cell padding down to a TableRow's cells). This test asserts
        // the absence — a bare El tagged with one of those roles comes
        // out with zero padding, zero gap, and Hug height, exactly as if
        // the role was unset.
        for role in [
            MetricsRole::Form,
            MetricsRole::FormItem,
            MetricsRole::ListItem,
            MetricsRole::MenuItem,
            MetricsRole::PreferenceRow,
            MetricsRole::TableRow,
            MetricsRole::TableHeader,
            MetricsRole::ChoiceItem,
            MetricsRole::TextArea,
            MetricsRole::TabList,
            MetricsRole::Panel,
        ] {
            let mut el = El::new(crate::Kind::Custom("bare")).metrics_role(role);
            ThemeMetrics::default().apply_to_tree(&mut el);
            assert_eq!(el.padding, Sides::zero(), "role {role:?} stamped padding");
            assert_eq!(el.gap, 0.0, "role {role:?} stamped gap");
            assert_eq!(el.height, Size::Hug, "role {role:?} stamped height");
            assert_eq!(
                el.radius,
                crate::tree::Corners::ZERO,
                "role {role:?} stamped radius"
            );
        }
    }

    #[test]
    fn form_constructor_bakes_default_gap() {
        // Smoke test for the constructor-baked recipe: form() picks up
        // SPACE_3 between items, form_item() picks up SPACE_2.
        let mut f = crate::form([crate::form_item([crate::text("body")])]);
        ThemeMetrics::default().apply_to_tree(&mut f);
        assert_eq!(f.gap, tokens::SPACE_3);
        assert_eq!(f.children[0].gap, tokens::SPACE_2);
    }

    /// A themed tree with no radius scale set must be bit-identical to
    /// the pre-`with_radius_scale` library. These are the stock radii
    /// as of that change: a card's `default_radius(RADIUS_LG)`, a
    /// badge's `BADGE_RADIUS`, an avatar's pill, and the 6 px the
    /// metrics pass stamps on an `Sm` button.
    #[test]
    fn default_radius_scale_leaves_stock_radii_unchanged() {
        use crate::tree::Corners;
        use crate::{avatar_initials, badge, card, column};

        let mut root = column([
            card([button("Save"), badge("New"), avatar_initials("BK")]),
            text_input("q", "Search", &crate::Selection::default()),
        ]);
        crate::Theme::default().apply_metrics(&mut root);

        let card_el = &root.children[0];
        assert_eq!(card_el.radius, Corners::all(tokens::RADIUS_LG));
        assert_eq!(card_el.children[0].radius, Corners::all(6.0));
        assert_eq!(
            card_el.children[1].radius,
            Corners::all(crate::widgets::badge::BADGE_RADIUS)
        );
        assert_eq!(
            card_el.children[2].radius,
            Corners::all(tokens::RADIUS_PILL)
        );
        assert_eq!(root.children[1].radius, Corners::all(6.0));
    }

    #[test]
    fn radius_scale_zero_squares_stock_controls_and_cards() {
        use crate::tree::Corners;
        use crate::{badge, card, column};

        let mut root = column([card([
            button("Save"),
            text_input("q", "Search", &crate::Selection::default()),
            badge("New"),
        ])]);
        crate::Theme::default()
            .with_radius_scale(0.0)
            .apply_metrics(&mut root);

        let card_el = &root.children[0];
        assert_eq!(card_el.radius, Corners::ZERO, "card surface");
        for (idx, child) in card_el.children.iter().enumerate() {
            assert_eq!(child.radius, Corners::ZERO, "control {idx}");
        }
    }

    #[test]
    fn tab_triggers_square_with_radius_scale_zero() {
        use crate::tree::{Corners, RadiusOrigin};
        use crate::{column, tabs_list};

        // shadcn's tab-trigger radius derives from `--radius` via the
        // calc ladder, so a web-trained agent setting the radius to
        // zero expects tabs to square with everything else. The
        // triggers' per-corner segment silhouette is `LibraryShape` —
        // protected from `apply_control`, but magnitude-themed.
        let mut root = column([tabs_list(
            "settings",
            &"account",
            [("account", "Account"), ("password", "Password")],
        )]);
        let list = &root.children[0];
        assert!(
            list.children
                .iter()
                .all(|t| t.radius_origin == RadiusOrigin::LibraryShape && t.radius.any_nonzero()),
            "precondition: triggers carry a library-stamped rounded silhouette"
        );

        crate::Theme::default()
            .with_radius_scale(0.0)
            .apply_metrics(&mut root);

        let list = &root.children[0];
        assert_eq!(list.radius, Corners::ZERO, "the strip squares");
        for (idx, trigger) in list.children.iter().enumerate() {
            assert_eq!(trigger.radius, Corners::ZERO, "trigger {idx} squares");
        }
    }

    #[test]
    fn tab_trigger_silhouette_scales_proportionally() {
        use crate::tree::RadiusOrigin;
        use crate::{column, tabs_list};

        // At a nonzero scale the segment shape survives: rounded outer
        // corners shrink, square inner corners stay square.
        let mut root = column([tabs_list(
            "settings",
            &"account",
            [("account", "Account"), ("password", "Password")],
        )]);
        let unscaled: Vec<_> = root.children[0].children.iter().map(|t| t.radius).collect();

        crate::Theme::default()
            .with_radius_scale(0.5)
            .apply_metrics(&mut root);

        for (trigger, before) in root.children[0].children.iter().zip(&unscaled) {
            assert_eq!(trigger.radius_origin, RadiusOrigin::LibraryShape);
            for (after, before) in trigger
                .radius
                .to_array()
                .iter()
                .zip(before.to_array().iter())
            {
                assert_eq!(*after, before * 0.5, "corners scale, zeros stay zero");
            }
        }
    }

    #[test]
    fn shadow_scale_zero_flattens_stock_recipe_shadows() {
        use crate::{card, column, text};

        let mut root = column([card([text("Body")])]);
        assert_eq!(
            root.children[0].shadow,
            tokens::SHADOW_SM,
            "precondition: card bakes shadow-sm"
        );
        assert!(
            !root.children[0].explicit_shadow,
            "precondition: a recipe shadow is a theme default, not author intent"
        );

        crate::Theme::default()
            .with_shadow_scale(0.0)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].shadow, 0.0, "card chrome goes flat");
    }

    #[test]
    fn explicit_shadow_survives_shadow_scale_zero() {
        use crate::{card, column, text};

        let mut root = column([card([text("Body")]).shadow(tokens::SHADOW_LG)]);
        crate::Theme::default()
            .with_shadow_scale(0.0)
            .apply_metrics(&mut root);

        assert_eq!(
            root.children[0].shadow,
            tokens::SHADOW_LG,
            "an author-declared elevation is not a theme default"
        );
    }

    #[test]
    fn shadow_scale_default_is_identity() {
        use crate::{card, column, text};

        let mut root = column([card([text("Body")])]);
        crate::Theme::default().apply_metrics(&mut root);
        assert_eq!(
            root.children[0].shadow,
            tokens::SHADOW_SM,
            "no knob set: recipe shadows stay bit-identical"
        );
    }

    #[test]
    fn type_scale_scales_roles_rungs_and_line_heights() {
        use crate::{column, text};

        let scale = 13.0 / 14.0;
        let mut root = column([
            text("body"),
            text("caption").caption(),
            text("title").title(),
            text("small").small(),
        ]);
        let before: Vec<_> = root
            .children
            .iter()
            .map(|t| (t.font_size, t.line_height))
            .collect();

        crate::Theme::default()
            .with_type_scale(scale)
            .apply_metrics(&mut root);

        for (child, (size, lh)) in root.children.iter().zip(&before) {
            assert_eq!(child.font_size, size * scale, "sizes scale");
            assert_eq!(child.line_height, lh * scale, "line heights track");
        }
        assert_eq!(
            root.children[0].font_size,
            tokens::TEXT_SM.size * scale,
            "14px body lands at 13px with the workbench ratio"
        );
    }

    #[test]
    fn explicit_type_metrics_survive_type_scale() {
        use crate::{column, text};

        let mut root = column([
            text("picked").font_size(15.0),
            text("leaded").line_height(28.0),
        ]);
        crate::Theme::default()
            .with_type_scale(0.5)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].font_size, 15.0);
        assert_eq!(root.children[1].line_height, 28.0);
        assert_eq!(
            root.children[1].font_size,
            tokens::TEXT_SM.size,
            "claiming line height pins the node's type metrics wholesale"
        );
    }

    #[test]
    fn type_scale_default_is_identity() {
        use crate::{column, text};

        let mut root = column([text("body"), text("title").title()]);
        crate::Theme::default().apply_metrics(&mut root);
        assert_eq!(root.children[0].font_size, tokens::TEXT_SM.size);
        assert_eq!(root.children[1].font_size, tokens::TEXT_BASE.size);
        assert_eq!(root.children[1].line_height, tokens::TEXT_BASE.line_height);
    }

    #[test]
    fn explicit_radius_survives_radius_scale_zero() {
        use crate::tree::Corners;
        use crate::{card, column, text};

        let mut root = column([
            button("Save").radius(tokens::RADIUS_LG),
            card([text("Body")]).radius(Corners::top(tokens::RADIUS_MD)),
        ]);
        crate::Theme::default()
            .with_radius_scale(0.0)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].radius, Corners::all(tokens::RADIUS_LG));
        assert_eq!(root.children[1].radius, Corners::top(tokens::RADIUS_MD));
    }

    #[test]
    fn pill_radius_survives_radius_scale_zero() {
        use crate::tree::Corners;
        use crate::{avatar_initials, badge, column};

        // `rounded-full` on the web is independent of `--radius`, so a
        // pill stays a pill however square the rest of the app goes —
        // even when the pill is a theme default, not an author's pick.
        let mut root = column([
            badge("New").default_radius(tokens::RADIUS_PILL),
            avatar_initials("BK"),
        ]);
        crate::Theme::default()
            .with_radius_scale(0.0)
            .apply_metrics(&mut root);

        assert_eq!(root.children[0].radius, Corners::all(tokens::RADIUS_PILL));
        assert_eq!(root.children[1].radius, Corners::all(tokens::RADIUS_PILL));
    }

    #[test]
    fn per_corner_default_radii_scale_proportionally() {
        use crate::tree::Corners;

        // A `Corners::top(...)` silhouette must come out of the rescale
        // still a top-rounded shape, not a uniform one.
        let mut el = El::new(crate::Kind::Custom("strip")).default_radius(Corners {
            tl: tokens::RADIUS_LG,
            tr: tokens::RADIUS_SM,
            br: 0.0,
            bl: 0.0,
        });
        crate::Theme::default()
            .with_radius_scale(0.5)
            .apply_metrics(&mut el);

        assert_eq!(
            el.radius,
            Corners {
                tl: tokens::RADIUS_LG * 0.5,
                tr: tokens::RADIUS_SM * 0.5,
                br: 0.0,
                bl: 0.0,
            }
        );
    }

    #[test]
    fn card_header_strip_tracks_the_scaled_card_corners() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_header, text};

        // The header strip inherits the card's corners inside the same
        // pass; it must land on the *scaled* curve or it pokes through.
        let mut tree = card([
            card_header([text("Header")]).fill(tokens::MUTED),
            card_content([text("Body")]),
        ]);
        crate::Theme::default()
            .with_radius_scale(0.5)
            .apply_metrics(&mut tree);

        assert_eq!(tree.radius, Corners::all(tokens::RADIUS_LG * 0.5));
        assert_eq!(
            tree.children[0].radius,
            Corners::top(tokens::RADIUS_LG * 0.5)
        );
    }

    #[test]
    fn card_header_strip_tracks_an_explicit_scale_exempt_card() {
        use crate::tree::Corners;
        use crate::{card, card_content, card_header, text};

        // An explicit `.radius(...)` card is exempt from the scale; the
        // header strip it stamps must stay on the card's (unscaled)
        // curve rather than being rescaled out of sync at its own
        // visit — the sharp-corner poke-through the propagation exists
        // to prevent.
        let mut tree = card([
            card_header([text("Header")]).fill(tokens::MUTED),
            card_content([text("Body")]),
        ])
        .radius(tokens::RADIUS_LG);
        crate::Theme::default()
            .with_radius_scale(0.5)
            .apply_metrics(&mut tree);

        assert_eq!(tree.radius, Corners::all(tokens::RADIUS_LG));
        assert_eq!(tree.children[0].radius, Corners::top(tokens::RADIUS_LG));
    }

    #[test]
    fn default_metrics_are_compact_desktop_defaults() {
        let metrics = ThemeMetrics::default();

        assert_eq!(metrics.default_component_size(), ComponentSize::Sm);
    }

    // ===== The size ladder =====

    /// Every rung, densest first. `Ord` is derived from this order, so
    /// the declaration order and this list must agree.
    const LADDER: [ComponentSize; 5] = [
        ComponentSize::Xxs,
        ComponentSize::Xs,
        ComponentSize::Sm,
        ComponentSize::Md,
        ComponentSize::Lg,
    ];

    /// Every metric the ladder keys, as `(name, value)` pairs in a
    /// stable order — one row of the ladder table.
    fn rung_metrics(size: ComponentSize) -> Vec<(&'static str, f32)> {
        let button = control_metrics(size, ControlKind::Button);
        let icon = control_metrics(size, ControlKind::IconButton);
        let input = control_metrics(size, ControlKind::Input);
        let badge = badge_metrics(size);
        vec![
            ("button height", button.height),
            ("button padding_x", button.padding_x),
            ("button radius", button.radius),
            ("button gap", button.gap),
            ("icon button edge", icon.height),
            ("input height", input.height),
            ("input padding_x", input.padding_x),
            ("badge height", badge.height),
            ("badge padding_x", badge.padding_x),
            ("choice edge", choice_control_metrics(size).edge),
            ("switch track", switch_metrics(size)),
            ("slider track", slider_metrics(size)),
            ("progress height", progress_metrics(size)),
            // The one container metric on the ladder; measured at the
            // identity type scale so the row is a table of constants.
            (
                "table cell padding_x",
                table_cell_metrics(size, 1.0).padding_x,
            ),
            (
                "table cell padding_y",
                table_cell_metrics(size, 1.0).padding_y,
            ),
        ]
    }

    #[test]
    fn the_ladder_never_grows_as_the_rung_shrinks() {
        // Monotonic, not strict: the ladder deliberately plateaus in
        // places (Lg and Md share an 8 px gap; Md and Sm share a 16 px
        // choice box and 8 px badge padding). What must never happen is
        // a *smaller* rung with a *larger* value in any metric.
        for pair in LADDER.windows(2) {
            let (small, large) = (pair[0], pair[1]);
            for ((name, s), (_, l)) in rung_metrics(small).into_iter().zip(rung_metrics(large)) {
                assert!(
                    s <= l,
                    "{name}: {small:?} ({s}) must not exceed {large:?} ({l})"
                );
            }
        }
    }

    #[test]
    fn xxs_is_strictly_denser_than_xs_on_every_metric() {
        // The rung only earns its place if it is smaller in every
        // dimension, not just in control height — a 22 px control with
        // Xs padding and gaps would not fit its own contents.
        // Exception: the Input horizontal-padding floor is a caret
        // gutter, deliberately shared with Xs (see `ComponentSize::Xxs`).
        for ((name, xxs), (_, xs)) in rung_metrics(ComponentSize::Xxs)
            .into_iter()
            .zip(rung_metrics(ComponentSize::Xs))
        {
            if name == "input padding_x" {
                assert_eq!(xxs, xs, "the input padding floor is rung-independent");
                continue;
            }
            assert!(
                xxs < xs,
                "{name}: Xxs ({xxs}) must be denser than Xs ({xs})"
            );
        }
    }

    #[test]
    fn xxs_metrics_are_the_documented_chrome_values() {
        let button = control_metrics(ComponentSize::Xxs, ControlKind::Button);
        assert_eq!(button.height, 22.0, "the workbench bar height");
        assert_eq!(button.padding_x, 6.0);
        assert_eq!(button.radius, 4.0);
        assert_eq!(button.gap, 2.0);

        // Icon buttons stay square and lose their horizontal padding.
        let icon = control_metrics(ComponentSize::Xxs, ControlKind::IconButton);
        assert_eq!((icon.height, icon.padding_x), (22.0, 0.0));

        // The input keeps the 10 px caret gutter (its floor bites here,
        // as it already does at Xs) and is not given the Lg field bump.
        let input = control_metrics(ComponentSize::Xxs, ControlKind::Input);
        assert_eq!((input.height, input.padding_x), (22.0, 10.0));

        let badge = badge_metrics(ComponentSize::Xxs);
        assert_eq!((badge.height, badge.padding_x), (16.0, 4.0));

        assert_eq!(choice_control_metrics(ComponentSize::Xxs).edge, 12.0);
        assert_eq!(switch_metrics(ComponentSize::Xxs), 14.0);
        assert_eq!(slider_metrics(ComponentSize::Xxs), 12.0);
        assert_eq!(progress_metrics(ComponentSize::Xxs), 2.0);
    }

    #[test]
    fn an_xxs_control_fits_a_thirty_pixel_title_strip() {
        // The rung's whole reason to exist: a 30 px title bar has to
        // hold the control *and* its focus ring, which paints
        // RING_WIDTH outside the control bounds on every side.
        let height = control_metrics(ComponentSize::Xxs, ControlKind::Button).height;
        assert!(
            height + 2.0 * tokens::RING_WIDTH <= 30.0,
            "22 px control + 2 px ring per side must clear a 30 px strip, got {}",
            height + 2.0 * tokens::RING_WIDTH
        );
        // And Xs, the rung that used to be the floor, does not — this
        // is the measurement that motivated Xxs.
        assert!(
            control_metrics(ComponentSize::Xs, ControlKind::Button).height
                + 2.0 * tokens::RING_WIDTH
                > 30.0
        );
    }

    #[test]
    fn xxs_stamps_through_the_metrics_pass_onto_real_widgets() {
        use crate::widgets::checkbox::checkbox;
        use crate::widgets::progress::progress;
        use crate::widgets::select::select_trigger;
        use crate::widgets::slider::slider;
        use crate::widgets::switch::switch;
        use crate::{badge, row};

        // End to end: the pass, not the table.
        let mut chrome = row([
            button("Run").size(ComponentSize::Xxs),
            select_trigger("branch", "main").size(ComponentSize::Xxs),
            badge("3").size(ComponentSize::Xxs),
            checkbox("wrap", true).size(ComponentSize::Xxs),
            switch("live", true).size(ComponentSize::Xxs),
            slider("zoom", 0.5).size(ComponentSize::Xxs),
            progress(0.4).size(ComponentSize::Xxs),
        ]);
        crate::Theme::default().apply_metrics(&mut chrome);

        let kids = &chrome.children;
        assert_eq!(kids[0].height, Size::Fixed(22.0), "button");
        assert_eq!(kids[0].padding, Sides::xy(6.0, 0.0));
        assert_eq!(kids[0].gap, 2.0);
        assert_eq!(kids[1].height, Size::Fixed(22.0), "select trigger");
        assert_eq!(
            kids[1].padding,
            Sides::xy(10.0, 0.0),
            "the input caret gutter survives the rung"
        );
        assert_eq!(kids[2].height, Size::Fixed(16.0), "badge");
        assert_eq!(kids[2].padding, Sides::xy(4.0, 0.0));
        assert_eq!(kids[3].width, Size::Fixed(12.0), "checkbox box");
        assert_eq!(kids[3].height, Size::Fixed(12.0));
        assert_eq!(kids[4].height, Size::Fixed(14.0), "switch track");
        assert_eq!(kids[5].height, Size::Fixed(12.0), "slider track");
        assert_eq!(kids[6].height, Size::Fixed(2.0), "progress bar");
    }

    /// `icon_button` is what a compact strip is actually built from, and
    /// its rustdoc now points agents at `.size(ComponentSize::Xxs)` for
    /// the chrome rung. This is the claim that doc makes, measured end
    /// to end through the metrics pass: a 22 px *square*, no horizontal
    /// padding, still clearing a 30 px title strip with the focus ring.
    #[test]
    fn an_xxs_icon_button_stamps_a_22px_square() {
        use crate::row;
        use crate::widgets::button::icon_button;

        let mut strip = row([icon_button("eye").ghost().size(ComponentSize::Xxs)]);
        crate::Theme::default().apply_metrics(&mut strip);

        let button = &strip.children[0];
        assert_eq!(button.height, Size::Fixed(22.0));
        assert_eq!(
            button.width,
            Size::Fixed(22.0),
            "IconButton forces width == height"
        );
        assert_eq!(button.padding, Sides::xy(0.0, 0.0));
        // That 22 px is what clears a 30 px strip — see
        // `an_xxs_control_fits_a_thirty_pixel_title_strip` for the
        // focus-ring arithmetic.
    }

    // ===== Table cell padding: the ladder's one container metric =====

    /// Lay out a stock table under `theme` and return
    /// `(painted row pitch, row content box)` for its body rows.
    #[cfg(test)]
    fn measure_table(theme: &crate::Theme) -> (f32, f32) {
        use crate::widgets::table::{
            TableColumn, table, table_body, table_header, table_header_cells, table_row_cells,
        };
        use crate::{Rect, text};

        const COLS: &[TableColumn] = &[TableColumn::fill(1.0), TableColumn::fill(1.0)];
        let mut root = crate::column([table([
            table_header([table_header_cells(COLS, ["Reference", "Stock"])]),
            table_body((0..4).map(|i| {
                table_row_cells(format!("row:{i}"), COLS, [text("RC0603FR"), text("4,812")])
            })),
        ])]);
        crate::bundle::artifact::render_bundle_themed(
            &mut root,
            Rect::new(0.0, 0.0, 640.0, 400.0),
            theme,
        );
        let body = &root.children[0].children[1];
        let pitch = body.children[1].computed_rect.y - body.children[0].computed_rect.y;
        // The last row carries no `border_b`, so its box is the content
        // box the rung asked for, undisturbed by the rule.
        let content = body.children.last().unwrap().computed_rect.h;
        (pitch, content)
    }

    #[test]
    fn table_cell_padding_is_the_documented_rung_table() {
        // The table in `table_cell_metrics`' rustdoc, at the identity
        // type scale. `Md` is the pin: `xy(SPACE_3, SPACE_2)`, the value
        // `table_cell` hardcoded before the ladder reached it.
        let want = [
            (ComponentSize::Xxs, 6.0, 1.0),
            (ComponentSize::Xs, 8.0, 4.0),
            (ComponentSize::Sm, 10.0, 6.0),
            (ComponentSize::Md, tokens::SPACE_3, tokens::SPACE_2),
            (ComponentSize::Lg, 14.0, 10.0),
        ];
        for (size, px, py) in want {
            let m = table_cell_metrics(size, 1.0);
            assert_eq!((m.padding_x, m.padding_y), (px, py), "{size:?}");
        }
    }

    #[test]
    fn a_table_row_is_as_tall_as_a_control_of_the_same_rung() {
        // The invariant the vertical padding is derived from, checked at
        // the identity scale *and* under the workbench's 13/14 — the
        // whole reason `table_cell_metrics` takes the type scale.
        for scale in [1.0_f32, 13.0 / 14.0, 0.5] {
            for size in LADDER {
                let control = control_metrics(size, ControlKind::Button).height;
                let m = table_cell_metrics(size, scale);
                let row = tokens::TEXT_SM.line_height * scale + 2.0 * m.padding_y;
                assert!(
                    (row - control).abs() < 1e-4,
                    "{size:?} @ {scale}: row box {row} should equal the control height {control}",
                );
            }
        }
    }

    #[test]
    fn table_cell_padding_clamps_at_zero_rather_than_clipping_text() {
        // A type scale big enough to overflow the rung must let the row
        // grow, not paint negative padding.
        let m = table_cell_metrics(ComponentSize::Xxs, 4.0);
        assert_eq!(m.padding_y, 0.0);
        assert_eq!(m.padding_x, 6.0, "the horizontal gutter is scale-free");
    }

    #[test]
    fn the_default_component_size_densifies_a_table_end_to_end() {
        use crate::text;
        use crate::widgets::table::{table_cell, table_row};

        for (size, px, py) in [
            (ComponentSize::Md, tokens::SPACE_3, tokens::SPACE_2),
            (ComponentSize::Xs, 8.0, 4.0),
        ] {
            let mut row = table_row([table_cell(text("Ada")), table_cell(text("dev"))]);
            crate::Theme::default()
                .with_default_component_size(size)
                .apply_metrics(&mut row);
            for (i, cell) in row.children.iter().enumerate() {
                assert_eq!(cell.padding, Sides::xy(px, py), "{size:?} cell {i}");
            }
        }
    }

    #[test]
    fn a_header_row_takes_the_same_rung_as_a_body_row() {
        use crate::widgets::table::{table_head, table_header, table_row};

        let mut header = table_header([table_row([table_head("Name"), table_head("Role")])]);
        crate::Theme::default()
            .with_default_component_size(ComponentSize::Xs)
            .apply_metrics(&mut header);
        for cell in &header.children[0].children {
            assert_eq!(cell.padding, Sides::xy(8.0, 4.0));
        }
        // And the promotion to `TableHeader` is what carried it — a row
        // that never saw `table_header` is stamped by the TableRow arm.
        let mut body_row = table_row([table_head("Name")]);
        crate::Theme::default()
            .with_default_component_size(ComponentSize::Xs)
            .apply_metrics(&mut body_row);
        assert_eq!(body_row.children[0].padding, Sides::xy(8.0, 4.0));
    }

    #[test]
    fn explicit_cell_padding_survives_the_rung_stamp() {
        use crate::text;
        use crate::widgets::table::{table_cell, table_row};

        // Both spellings of "the author owns this cell": whole-Sides and
        // one-axis. Either marks the padding explicit.
        let mut row = table_row([
            table_cell(text("a")).padding(Sides::xy(2.0, 1.0)),
            table_cell(text("b")).py(3.0),
            table_cell(text("c")),
        ]);
        crate::Theme::default()
            .with_default_component_size(ComponentSize::Lg)
            .apply_metrics(&mut row);

        assert_eq!(row.children[0].padding, Sides::xy(2.0, 1.0));
        assert_eq!(
            row.children[1].padding,
            Sides {
                left: tokens::SPACE_3,
                right: tokens::SPACE_3,
                top: 3.0,
                bottom: 3.0,
            },
            "`.py()` claims the node: the untouched sides keep the constructor default"
        );
        assert_eq!(
            row.children[2].padding,
            Sides::xy(14.0, 10.0),
            "the un-claimed cell still takes the rung"
        );
    }

    #[test]
    fn a_control_inside_a_cell_keeps_the_cell_chrome_not_its_own() {
        use crate::badge;
        use crate::widgets::table::{table_cell, table_row};

        // `table_cell` styles its content in place, so this node carries
        // `MetricsRole::Badge` *and* is a cell. The cell padding has to
        // win, or the badge recipe (visited later in the same pre-order
        // walk) would overwrite the row's pitch away.
        let mut row = table_row([table_cell(badge("3"))]);
        crate::Theme::default()
            .with_default_component_size(ComponentSize::Md)
            .apply_metrics(&mut row);

        let cell = &row.children[0];
        assert_eq!(cell.metrics_role, Some(MetricsRole::Badge));
        assert_eq!(
            cell.padding,
            Sides::xy(tokens::SPACE_3, tokens::SPACE_2),
            "cell chrome, not `badge_metrics`' xy(8, 0)"
        );
    }

    #[test]
    fn a_row_may_name_its_own_rung() {
        use crate::text;
        use crate::widgets::table::{table_cell, table_row};

        let mut row = table_row([table_cell(text("a"))]).size(ComponentSize::Xxs);
        crate::Theme::default()
            .with_default_component_size(ComponentSize::Lg)
            .apply_metrics(&mut row);
        assert_eq!(row.children[0].padding, Sides::xy(6.0, 1.0));
    }

    #[test]
    fn the_stock_table_still_paints_shadcns_geometry_at_md() {
        // The pin, measured rather than asserted on the table: at `Md`
        // and the identity type scale a stock table is bit-identical to
        // the pre-ladder library — 36 px rows, 37 px painted pitch.
        let (pitch, content) =
            measure_table(&crate::Theme::default().with_default_component_size(ComponentSize::Md));
        assert_eq!(content, 36.0);
        assert_eq!(pitch, 37.0);
    }

    #[test]
    fn the_xs_rung_lands_a_table_on_the_reference_row_box() {
        // `references/workbench-validation/parts/index.html` declares
        // `tbody tr { height: 28px }` and renders at a 30 px pitch. The
        // `Xs` rung — what `damascene_workbench::theme` ships — puts a
        // stock table's content box on that declared 28 px under the
        // workbench's 13/14 type scale, i.e. a 29 px painted pitch.
        let workbenchish = crate::Theme::default()
            .with_default_component_size(ComponentSize::Xs)
            .with_type_scale(13.0 / 14.0);
        let (pitch, content) = measure_table(&workbenchish);
        assert_eq!(content, 28.0, "the reference's declared row box");
        assert_eq!(pitch, 29.0);
        // And it is a real densification: the same table at the stock
        // `Md` rung is a third taller.
        let (md_pitch, _) =
            measure_table(&crate::Theme::default().with_default_component_size(ComponentSize::Md));
        assert!(pitch < md_pitch * 0.85, "{pitch} vs {md_pitch}");
    }

    #[test]
    fn the_row_pitch_holds_across_type_scales() {
        // The invariant, end to end: the type scale moves the glyphs,
        // not the pitch. This is what makes the workbench's 13/14 land
        // on the reference instead of 2 px under it.
        for scale in [1.0_f32, 13.0 / 14.0, 0.75] {
            let (_, content) = measure_table(
                &crate::Theme::default()
                    .with_default_component_size(ComponentSize::Xs)
                    .with_type_scale(scale),
            );
            assert_eq!(content, 28.0, "type scale {scale} moved the row box");
        }
    }

    #[test]
    fn xxs_is_the_ladders_floor_by_ord() {
        // `Ord` is derived, so a future rung inserted in the wrong place
        // would silently reorder comparisons.
        assert!(LADDER.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(LADDER.iter().min(), Some(&ComponentSize::Xxs));
        assert_eq!(
            ComponentSize::default(),
            ComponentSize::Md,
            "still shadcn's"
        );
    }
}
