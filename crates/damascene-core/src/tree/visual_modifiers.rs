//! Visual, cursor, and paint-transform modifiers for [`El`].
//!
//! Every value-setting modifier here is **last-write-wins**: calling
//! it again replaces the earlier value silently. That's load-bearing
//! for the catalog — widgets bake a recipe (`button` sets a cursor,
//! `card_content` sets padding) and callers override per-call. The one
//! exception with a debug-build guard is [`El::tooltip`]: no stock
//! widget pre-sets a tooltip, so a re-set is always two user calls
//! racing for the same slot — usually one of them on the wrong node.

use crate::anim::Timing;
use crate::shader::ShaderBinding;
use crate::style::StyleProfile;

use super::geometry::{Corners, Sides};
use super::node::{El, FocusRingPlacement};
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
    pub fn fill(mut self, c: Color) -> Self {
        self.fill = Some(c);
        self
    }

    /// Fill applied when the nearest focusable ancestor isn't focused;
    /// the painter lerps from `dim_fill` toward `fill` as the focus
    /// envelope rises from 0 to 1. See [`Self::dim_fill`] field doc.
    pub fn dim_fill(mut self, c: Color) -> Self {
        self.dim_fill = Some(c);
        self
    }

    pub fn stroke(mut self, c: Color) -> Self {
        self.stroke = Some(c);
        if self.stroke_width == 0.0 {
            self.stroke_width = 1.0;
        }
        self
    }

    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }

    /// Set the element's corner radii. A scalar (e.g.
    /// `.radius(tokens::RADIUS_MD)`) sets all four corners uniformly
    /// via [`Corners::from`]; pass [`Corners::top`] / [`Corners::bottom`]
    /// / [`Corners::left`] / [`Corners::right`], or a directly-built
    /// [`Corners`], to round only a subset of corners.
    pub fn radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
        self.explicit_radius = true;
        self
    }

    pub fn shadow(mut self, s: f32) -> Self {
        self.shadow = s;
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
    /// **The node must also have a [`key`](Self::key).** Tooltips fire
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
        if let Some(prev) = &self.tooltip
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
        self.tooltip = Some(text);
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

    /// Uniformly scale this element's paint around its rect centre.
    pub fn scale(mut self, v: f32) -> Self {
        self.scale = v.max(0.0);
        self
    }

    /// Opt this element into app-driven prop interpolation.
    pub fn animate(mut self, timing: Timing) -> Self {
        self.animate = Some(timing);
        self
    }

    /// Bind a shader for the surface paint, replacing the implicit
    /// `stock::rounded_rect`.
    pub fn shader(mut self, binding: ShaderBinding) -> Self {
        self.shader_override = Some(binding);
        self
    }

    // ---- Internal: style profile ----
    pub fn style_profile(mut self, p: StyleProfile) -> Self {
        self.style_profile = p;
        self
    }

    pub(crate) fn default_radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
        self.explicit_radius = false;
        self
    }
}
