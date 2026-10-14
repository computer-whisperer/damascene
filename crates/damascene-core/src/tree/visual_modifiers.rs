//! Visual, cursor, and paint-transform modifiers for [`El`].
//!
//! Every value-setting modifier here is **last-write-wins**: calling
//! it again replaces the earlier value silently. That's load-bearing
//! for the catalog — widgets bake a recipe (`button` sets a cursor,
//! `card_content` sets padding) and callers override per-call. The one
//! exception with a debug-build guard is [`El::tooltip`]: no stock
//! widget pre-sets a tooltip, so a re-set is always two user calls
//! racing for the same slot — usually one of them on the wrong node.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::anim::Timing;
use crate::shader::ShaderBinding;
use crate::style::StyleProfile;

use super::geometry::{Corners, Sides};
use super::node::{El, FocusRingPlacement, RadiusOrigin};
use super::semantics::SurfaceRole;
use crate::color::Color;

/// Debug-build stderr warning, deduplicated per callsite so a warning
/// inside `App::build` prints once, not once per frame.
#[cfg(debug_assertions)]
fn warn_once(loc: &'static std::panic::Location<'static>, msg: impl FnOnce() -> String) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SEEN: Mutex<Option<HashSet<(&'static str, u32)>>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap();
    if seen
        .get_or_insert_with(HashSet::new)
        .insert((loc.file(), loc.line()))
    {
        eprintln!("{}", msg());
    }
}

impl El {
    // ---- Visual ----
    /// Set the element's background fill color.
    pub fn fill(mut self, c: Color) -> Self {
        self.fill = Some(c);
        self
    }

    /// Fill applied when the nearest focusable ancestor isn't focused;
    /// the painter lerps from `dim_fill` toward `fill` as the focus
    /// envelope rises from 0 to 1. See [`Self::dim_fill`] field doc.
    pub fn dim_fill(mut self, c: Color) -> Self {
        self.dim_fill = Some(Box::new(c));
        self
    }

    /// Set the element's border color. Also sets the stroke width to
    /// 1 logical px if it's still 0 (so `.stroke(c)` alone draws a
    /// hairline border).
    ///
    /// On a joined group ([`crate::button_group`] /
    /// [`toggle_group`](crate::widgets::toggle::toggle_group)) this is
    /// also the color of the seams between its segments: frame and
    /// dividers are one authored line. See the
    /// [`button_group`](crate::widgets::button_group) module docs.
    pub fn stroke(mut self, c: Color) -> Self {
        self.stroke = Some(c);
        if self.stroke_width == 0.0 {
            self.stroke_width = 1.0;
        }
        // The group's segments were seamed when it was built, before
        // this authored color existed; re-point them now. A `Kind`
        // discriminant test for everything else.
        crate::widgets::button_group::restroke_seams(&mut self);
        self
    }

    /// Set the border width in logical pixels (default 1.0 once a
    /// stroke color is set).
    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }

    /// Mutate this node's [`super::node::BorderSpec`] in place,
    /// installing the default (all-zero) spec first if the node has no
    /// border yet. Lets the per-side chainables compose in any order.
    fn edit_border(mut self, f: impl FnOnce(&mut super::node::BorderSpec)) -> Self {
        f(self.border.get_or_insert_default());
        self
    }

    /// Enable a 1px top border — Tailwind's `border-t`. The color is
    /// [`Self::border_color`] when set, else `tokens::BORDER` (the
    /// shadcn preflight default).
    ///
    /// Per-side borders are CSS semantics, distinct from
    /// [`Self::stroke`]: they sit *inside* the rect and join padding
    /// in the layout content inset, so a `Size::Hug` box grows by the
    /// border width and a fixed-size box keeps its outer size while
    /// content insets inward. On a rounded rect each edge quad spans
    /// the side minus the adjacent corner radii — an approximation of
    /// the CSS border curve, exact at radius 0. Children painted after
    /// the surface can cover a border they overlap (same z-order as
    /// CSS in-flow children).
    ///
    /// Because the border joins the inset, a fixed-height row spends the
    /// border width out of its content box — a `Size::Fixed(22.0)` row
    /// with `.border_t()` leaves 21px for content and reads a pixel
    /// short of its unbordered neighbours. Add the border width back to
    /// fixed row heights (`22.0 + 1.0`) to stay on the rhythm.
    pub fn border_t(self) -> Self {
        self.edit_border(|b| b.widths.top = 1.0)
    }

    /// Enable a 1px bottom border — Tailwind's `border-b`. See
    /// [`Self::border_t`] for the shared semantics.
    pub fn border_b(self) -> Self {
        self.edit_border(|b| b.widths.bottom = 1.0)
    }

    /// Enable a 1px left border — Tailwind's `border-l`. See
    /// [`Self::border_t`] for the shared semantics.
    pub fn border_l(self) -> Self {
        self.edit_border(|b| b.widths.left = 1.0)
    }

    /// Enable a 1px right border — Tailwind's `border-r`. See
    /// [`Self::border_t`] for the shared semantics.
    pub fn border_r(self) -> Self {
        self.edit_border(|b| b.widths.right = 1.0)
    }

    /// Set the per-side border color (all sides share one color, like
    /// CSS `border-color`). Without this, bordered sides paint in
    /// `tokens::BORDER`. Composes with the side chainables in any
    /// order; a color with no bordered side paints nothing.
    pub fn border_color(self, c: Color) -> Self {
        self.edit_border(|b| b.color = Some(c))
    }

    /// Set all four border widths at once, in logical pixels — the
    /// width-override companion to the 1px side chainables, mirroring
    /// how [`Self::padding`] relates to [`Self::pt`] / [`Self::pb`].
    /// `Sides::bottom(2.0)` is Tailwind's `border-b-2`; a scalar
    /// (`.border_widths(1.0)`) borders all four sides. Replaces any
    /// previously set widths; the color is left as-is.
    pub fn border_widths(self, w: impl Into<Sides>) -> Self {
        let w = w.into();
        self.edit_border(|b| b.widths = w)
    }

    /// Set the element's corner radii. A scalar (e.g.
    /// `.radius(tokens::RADIUS_MD)`) sets all four corners uniformly
    /// via [`Corners::from`]; pass [`Corners::top`] / [`Corners::bottom`]
    /// / [`Corners::left`] / [`Corners::right`], or a directly-built
    /// [`Corners`], to round only a subset of corners.
    pub fn radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
        self.radius_origin = RadiusOrigin::Fixed;
        self
    }

    /// Set the drop-shadow strength in logical pixels (0.0 = no
    /// shadow). An explicit shadow is the author's elevation choice
    /// and survives [`crate::Theme::with_shadow_scale`] — like a
    /// hardcoded `box-shadow` ignoring a theme's shadow variables.
    pub fn shadow(mut self, s: f32) -> Self {
        self.shadow = s;
        self.explicit_shadow = true;
        self
    }

    /// Widget-recipe shadow: sets the elevation tier without claiming
    /// author intent, so the theme's shadow scale applies. The
    /// `default_radius` of shadows.
    pub(crate) fn default_shadow(mut self, s: f32) -> Self {
        self.shadow = s;
        self.explicit_shadow = false;
        self
    }

    /// Tag this node with a semantic [`SurfaceRole`] so the theme can
    /// route it through the appropriate paint recipe. Most app code
    /// should not call this directly: the catalog widgets (`card()`,
    /// `sidebar()`, `dialog()`, `popover()`, `tabs_list()`, etc.) set
    /// the right role *and* the matching fill / stroke / radius /
    /// shadow together, while the `.selected()` and `.current()`
    /// chainables wrap the corresponding state recipes.
    ///
    /// Reach for the raw chainable when authoring a new widget or when
    /// composing a custom container that the catalog doesn't cover —
    /// and remember that decorative roles (`Panel`, `Raised`, `Popover`,
    /// `Danger`) require you to supply a fill yourself; see the
    /// [`SurfaceRole`] doc for the per-variant contract. The bundle
    /// lint pass flags `Panel` without a fill as
    /// [`crate::bundle::lint::FindingKind::MissingSurfaceFill`].
    pub fn surface_role(mut self, role: SurfaceRole) -> Self {
        self.surface_role = role;
        self
    }

    /// Permit paint to extend beyond this element's layout bounds by
    /// `outset` on each side. Layout-neutral; siblings don't move and
    /// hit-testing still uses the layout rect.
    pub fn paint_overflow(mut self, outset: impl Into<Sides>) -> Self {
        self.paint_overflow = outset.into();
        self
    }

    /// Draw the stock focus ring just inside this node's layout rect.
    ///
    /// The default focus ring is outside the rect so it does not reduce
    /// usable control area. Inside rings are for dense, flush stacks such as
    /// menu rows, where adding gaps would change the intended visual recipe.
    pub fn focus_ring_inside(mut self) -> Self {
        self.focus_ring_placement = FocusRingPlacement::Inside;
        self
    }

    /// Draw the stock focus ring outside this node's layout rect.
    pub fn focus_ring_outside(mut self) -> Self {
        self.focus_ring_placement = FocusRingPlacement::Outside;
        self
    }

    /// Attach a hover tooltip to this element. The runtime synthesizes
    /// a floating tooltip layer when the pointer rests on the node for
    /// the configured delay.
    ///
    /// # Two preconditions, both cheap
    ///
    /// **1. Your `App::build` root must be an overlay container.** The
    /// synthesized layer is pushed as the root's *last child*, so it
    /// only floats if the root stacks its children. Under a plain
    /// `column`/`row` root it becomes an ordinary flow child that
    /// steals main-axis space from your UI —
    /// [`crate::tooltip::synthesize_tooltip`] `debug_assert`s on the
    /// first hover to stop that reaching a release build. One line
    /// fixes it — wrap whatever you already return:
    ///
    /// ```ignore
    /// overlays(root, [])   // `root` = your existing build() value
    /// ```
    ///
    /// `page([...])` and any `stack(...)` root already satisfy this.
    /// The bundle lint catches it statically, at `render_bundle` time
    /// rather than at first hover — and in a release build, where the
    /// `debug_assert` is compiled out, the lint is the *only* warning
    /// you get. It reports as
    /// [`crate::bundle::lint::FindingKind::TooltipWithoutOverlayRoot`].
    ///
    /// Damascene does not auto-wrap the root. Wrapping is not free and
    /// not invisible: it pushes every node one path segment deeper, so
    /// every `computed_id` changes — and with them the scroll offsets,
    /// focus, and animation state keyed by id. That is a fine cost to
    /// opt into once, at a call you can see, and a bad one for the
    /// library to impose silently on a tree that never asked.
    ///
    /// A full-bleed shell with no overlay root is *not* a reason to
    /// drop tooltips from icon-only chrome — add the `overlays(...)`
    /// root (it is layout-neutral), and reach for
    /// [`aria_label`][method@Self::aria_label] as well, which labels a
    /// graphic control with no root requirement at all.
    ///
    /// **2. The node must also have a [`key`](Self::key).** Tooltips fire
    /// through the hit-test pipeline, and `crate::hit_test` only
    /// returns keyed nodes — an unkeyed leaf with `.tooltip()` is
    /// silently dead, because hover skips past it to the nearest
    /// keyed ancestor (which has a different `computed_id` and a
    /// different tooltip). The bundle lint flags this case as
    /// [`crate::bundle::lint::FindingKind::DeadTooltip`].
    ///
    /// For info-only chrome inside list rows (sha cells, timestamps,
    /// chips, identicon avatars) the usual key is a synthetic one
    /// like `"row:{idx}.<part>"` — its only purpose is to make the
    /// tooltip's hover land. The tooltip text is snapshotted onto the
    /// hit target at hit-test time, so tooltips fire correctly even
    /// on `virtual_list_dyn` rows whose children are realized only
    /// during layout.
    ///
    /// Like every modifier, last-write-wins — but unlike `fill` or
    /// `padding`, no stock widget pre-sets a tooltip, so a second
    /// `.tooltip()` on the same element is always two app calls racing
    /// for one slot (usually one belongs on a different node). Debug
    /// builds print a once-per-callsite warning when a re-set replaces
    /// different text.
    #[track_caller]
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        #[cfg(debug_assertions)]
        if let Some(prev) = self.tooltip_text()
            && *prev != text
        {
            let loc = std::panic::Location::caller();
            warn_once(loc, || {
                format!(
                    "damascene: .tooltip({text:?}) at {file}:{line} replaces the earlier \
                     .tooltip({prev:?}) on the same element — last value wins. If one of \
                     these belongs on a different node, move it; tooltips are looked up \
                     by the hovered node's id.",
                    file = loc.file(),
                    line = loc.line(),
                )
            });
        }
        self.tooltip = Some(text.into_boxed_str());
        self
    }

    /// Declare the pointer cursor when the pointer is over this
    /// element.
    pub fn cursor(mut self, cursor: crate::cursor::Cursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Declare the cursor shown only while a press is captured at this
    /// exact node.
    pub fn cursor_pressed(mut self, cursor: crate::cursor::Cursor) -> Self {
        self.cursor_pressed = Some(cursor);
        self
    }

    // ---- Paint-time transforms (animatable via `.animate()`) ----
    /// Multiply this element's paint alpha by `v` (clamped to `[0, 1]`).
    pub fn opacity(mut self, v: f32) -> Self {
        self.opacity = v.clamp(0.0, 1.0);
        self
    }

    /// Offset this element's paint and its descendants by `(x, y)` in
    /// logical pixels.
    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.translate = (x, y);
        self
    }

    /// Uniformly scale this element's paint — and its descendants' —
    /// around its rect centre (CSS `transform: scale()` semantics).
    /// Layout is untouched: siblings don't move, hit-testing keeps
    /// the layout rects.
    pub fn scale(mut self, v: f32) -> Self {
        self.scale = v.max(0.0);
        self
    }

    /// Opt this element into app-driven prop interpolation.
    pub fn animate(mut self, timing: Timing) -> Self {
        self.motion.get_or_insert_with(Default::default).animate = Some(timing);
        self
    }

    /// Animate this element's first mounted frame — seed the app-prop
    /// trackers at the transition's `from` values and ease to the
    /// built values (`fade-in-0 zoom-in-95 slide-in-from-*` as a
    /// value; see [`crate::anim::EnterTransition`]). Implies app-prop
    /// ticking; no separate [`Self::animate`] opt-in needed.
    pub fn enter_transition(mut self, t: crate::anim::EnterTransition) -> Self {
        self.motion.get_or_insert_with(Default::default).enter = Some(t);
        self
    }

    /// [`Self::enter_transition`] with a plain fade-in.
    pub fn enter_fade(self) -> Self {
        self.enter_transition(crate::anim::EnterTransition::fade())
    }

    /// [`Self::enter_transition`] with the shadcn overlay entrance —
    /// fade-in + zoom from 95%.
    pub fn enter_zoom(self) -> Self {
        self.enter_transition(crate::anim::EnterTransition::zoom())
    }

    /// [`Self::enter_transition`] sliding in from `(dx, dy)` px away.
    pub fn enter_slide(self, dx: f32, dy: f32) -> Self {
        self.enter_transition(crate::anim::EnterTransition::slide(dx, dy))
    }

    /// Bind a shader for the surface paint, replacing the implicit
    /// `stock::rounded_rect`.
    pub fn shader(mut self, binding: ShaderBinding) -> Self {
        self.shader_override = Some(Box::new(binding));
        self
    }

    // ---- Internal: style profile ----
    /// Set the node's [`StyleProfile`], which routes the theme's paint
    /// recipe selection. Stock widget constructors set this; app code
    /// rarely needs it.
    pub fn style_profile(mut self, p: StyleProfile) -> Self {
        self.style_profile = p;
        self
    }

    pub(crate) fn default_radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
        self.radius_origin = RadiusOrigin::ThemeDefault;
        self
    }
}
