//! Layout and child-list modifiers for [`El`].

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use crate::layout::{LayoutCtx, LayoutFn, VirtualAnchorPolicy};
use crate::metrics::{ComponentSize, MetricsRole};

use super::geometry::{Rect, Sides};
use super::layout_types::{Align, Axis, Justify, Size};
use super::node::El;

impl El {
    // ---- Sizing ----
    /// Set the width intent (see [`Size`]).
    pub fn width(mut self, w: Size) -> Self {
        self.width = w;
        self.explicit_width = true;
        self
    }

    /// Set the height intent (see [`Size`]).
    pub fn height(mut self, h: Size) -> Self {
        self.height = h;
        self.explicit_height = true;
        self
    }

    /// Size both axes to the intrinsic size of contents
    /// (`Size::Hug` on width and height).
    pub fn hug(mut self) -> Self {
        self.width = Size::Hug;
        self.height = Size::Hug;
        self.explicit_width = true;
        self.explicit_height = true;
        self
    }

    /// Fill the available space on both axes (`Size::Fill(1.0)` on
    /// width and height).
    pub fn fill_size(mut self) -> Self {
        self.width = Size::Fill(1.0);
        self.height = Size::Fill(1.0);
        self.explicit_width = true;
        self.explicit_height = true;
        self
    }

    /// Shorthand for `.width(Size::Fill(1.0))` — fill the available width
    /// of the parent (or, in a column, span its inner content box). Mirrors
    /// CSS `width: 100%`. For a non-default weight, use `.width(Size::Fill(w))`.
    pub fn fill_width(mut self) -> Self {
        self.width = Size::Fill(1.0);
        self.explicit_width = true;
        self
    }

    /// Shorthand for `.height(Size::Fill(1.0))` — fill the available height
    /// of the parent (or, in a row, span its inner content box). Mirrors
    /// CSS `height: 100%`. For a non-default weight, use `.height(Size::Fill(w))`.
    pub fn fill_height(mut self) -> Self {
        self.height = Size::Fill(1.0);
        self.explicit_height = true;
        self
    }

    /// Lower-bound the resolved width in logical pixels. Composes with
    /// any [`Size`] choice — `Hug` won't shrink below the floor, `Fill`
    /// won't lose space below it. See [`El::min_width`] for the full
    /// semantic.
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = Some(w);
        self
    }

    /// Upper-bound the resolved width in logical pixels. Pairs naturally
    /// with `Size::Fill` to cap a column at a readable measure.
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }

    /// Lower-bound the resolved height in logical pixels. See
    /// [`Self::min_width`] for the semantic.
    pub fn min_height(mut self, h: f32) -> Self {
        self.min_height = Some(h);
        self
    }

    /// Upper-bound the resolved height in logical pixels. See
    /// [`Self::max_width`] for the semantic.
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = Some(h);
        self
    }

    /// Set the t-shirt size for stock controls.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.component_size = Some(size);
        self
    }

    /// Shorthand for `.size(ComponentSize::Md)`.
    pub fn medium(self) -> Self {
        self.size(ComponentSize::Md)
    }

    /// Shorthand for `.size(ComponentSize::Lg)`.
    pub fn large(self) -> Self {
        self.size(ComponentSize::Lg)
    }

    /// Set the theme-facing stock metrics role for this widget.
    pub fn metrics_role(mut self, role: MetricsRole) -> Self {
        self.metrics_role = Some(role);
        self
    }

    // ---- Layout (container) ----
    /// Inner padding in logical px — a uniform value or per-side
    /// [`Sides`]. Marks the padding as explicit, so the metrics pass
    /// will not stamp a density-driven value over it.
    pub fn padding(mut self, p: impl Into<Sides>) -> Self {
        self.padding = p.into();
        self.explicit_padding = true;
        self
    }

    /// Override only the top padding side, preserving the other three
    /// sides at their current value (whether from a constructor's
    /// `default_padding` or a previous explicit `.padding(...)`).
    /// Mirrors Tailwind's `pt-N`. Marks the padding as explicit, so
    /// the metrics pass will not stamp a density-driven value over it.
    pub fn pt(mut self, v: f32) -> Self {
        self.padding.top = v;
        self.explicit_padding = true;
        self
    }

    /// Override only the bottom padding side. Mirrors Tailwind's `pb-N`.
    /// See [`Self::pt`] for composition semantics.
    pub fn pb(mut self, v: f32) -> Self {
        self.padding.bottom = v;
        self.explicit_padding = true;
        self
    }

    /// Override only the left padding side. Mirrors Tailwind's `pl-N`.
    /// See [`Self::pt`] for composition semantics.
    pub fn pl(mut self, v: f32) -> Self {
        self.padding.left = v;
        self.explicit_padding = true;
        self
    }

    /// Override only the right padding side. Mirrors Tailwind's `pr-N`.
    /// See [`Self::pt`] for composition semantics.
    pub fn pr(mut self, v: f32) -> Self {
        self.padding.right = v;
        self.explicit_padding = true;
        self
    }

    /// Override the horizontal padding sides (left + right), preserving
    /// `top` and `bottom`. Mirrors Tailwind's `px-N`.
    /// See [`Self::pt`] for composition semantics.
    pub fn px(mut self, v: f32) -> Self {
        self.padding.left = v;
        self.padding.right = v;
        self.explicit_padding = true;
        self
    }

    /// Override the vertical padding sides (top + bottom), preserving
    /// `left` and `right`. Mirrors Tailwind's `py-N`.
    /// See [`Self::pt`] for composition semantics.
    pub fn py(mut self, v: f32) -> Self {
        self.padding.top = v;
        self.padding.bottom = v;
        self.explicit_padding = true;
        self
    }

    /// Spacing between adjacent children along the main axis, in
    /// logical px.
    pub fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self.explicit_gap = true;
        self
    }

    /// Cross-axis sizing/alignment of children (see [`Align`];
    /// mirrors CSS `align-items`).
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    /// Main-axis distribution of children (see [`Justify`]).
    pub fn justify(mut self, j: Justify) -> Self {
        self.justify = j;
        self
    }

    /// Clip children's paint to this node's rect.
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }

    /// Make this node a vertical scroll viewport for its overflowing
    /// content.
    pub fn scrollable(mut self) -> Self {
        self.scrollable = true;
        self
    }

    /// Show a draggable vertical scrollbar thumb when this scrollable
    /// node's content overflows.
    pub fn scrollbar(mut self) -> Self {
        self.scrollbar = true;
        self
    }

    /// Suppress the default scrollbar thumb on this scrollable node.
    pub fn no_scrollbar(mut self) -> Self {
        self.scrollbar = false;
        self
    }

    /// Reserve a right-edge gutter for the scrollbar thumb — the CSS
    /// `scrollbar-gutter: stable` shape. The thumb overlays content by
    /// default; when the content holds focusable rows their hit
    /// targets and focus rings end up underneath it (the
    /// `ScrollbarObscuresFocusable` lint). This reserves
    /// [`crate::tokens::SCROLLBAR_GUTTER`] of extra right padding so
    /// content and thumb never overlap.
    ///
    /// The gutter is resolved by the metrics pass *on top of* the
    /// node's padding, so it composes with `.padding(...)` /
    /// `.pr(...)` in any call order. Apply it to the scroll node
    /// itself:
    ///
    /// ```ignore
    /// scroll(rows).key("sidebar").scrollbar_gutter()
    /// ```
    pub fn scrollbar_gutter(mut self) -> Self {
        self.scrollbar_gutter = true;
        self
    }

    /// Internal: install (or replace) this node's
    /// [`ViewportConfig`](crate::viewport::ViewportConfig), marking it a
    /// pan/zoom viewport. The [`viewport()`](crate::tree::viewport)
    /// constructor calls this with the defaults; the per-field builders
    /// below mutate the installed config.
    pub(crate) fn with_viewport_cfg(mut self, cfg: crate::viewport::ViewportConfig) -> Self {
        self.viewport = Some(cfg);
        self
    }

    /// Mutate this viewport's config in place, installing the default
    /// config first if the node is not yet a viewport. Lets the
    /// per-field builders be called in any order, on or off the
    /// `viewport()` constructor.
    fn edit_viewport_cfg(mut self, f: impl FnOnce(&mut crate::viewport::ViewportConfig)) -> Self {
        let mut cfg = self.viewport.unwrap_or_default();
        f(&mut cfg);
        self.viewport = Some(cfg);
        self
    }

    /// Smallest zoom factor the [`viewport`](crate::tree::viewport) may
    /// reach by wheel or [`ViewportRequest`](crate::viewport::ViewportRequest)
    /// (default `0.2`). `0.5` means the content can shrink to half size.
    pub fn min_zoom(self, z: f32) -> Self {
        self.edit_viewport_cfg(|c| c.min_zoom = z)
    }

    /// Largest zoom factor the [`viewport`](crate::tree::viewport) may
    /// reach (default `5.0`).
    pub fn max_zoom(self, z: f32) -> Self {
        self.edit_viewport_cfg(|c| c.max_zoom = z)
    }

    /// Which pointer button begins a pan drag in this
    /// [`viewport`](crate::tree::viewport) (default
    /// [`PointerButton::Primary`](crate::event::PointerButton::Primary)).
    /// Set [`PointerButton::Middle`](crate::event::PointerButton::Middle)
    /// for a CAD/Figma-style middle-drag pan that leaves left-click free
    /// for content.
    pub fn pan_button(self, button: crate::event::PointerButton) -> Self {
        self.edit_viewport_cfg(|c| c.pan_trigger.button = button)
    }

    /// Which modifier mask must be held to begin a pan drag (default:
    /// none). The match is exact — extra modifiers held beyond this mask
    /// do not trigger a pan. Use this for a space-/alt-drag pan that
    /// keeps the plain drag available to content.
    pub fn pan_modifier(self, modifiers: crate::event::KeyModifiers) -> Self {
        self.edit_viewport_cfg(|c| c.pan_trigger.modifiers = modifiers)
    }

    /// How far the [`viewport`](crate::tree::viewport) may be panned
    /// relative to its content (default
    /// [`PanBounds::Contain`](crate::viewport::PanBounds::Contain)). Use
    /// [`PanBounds::Center`](crate::viewport::PanBounds::Center) for a
    /// node graph / DAG canvas where any node should be parkable at the
    /// center of the frame, or
    /// [`PanBounds::Free`](crate::viewport::PanBounds::Free) to drop
    /// clamping entirely.
    pub fn pan_bounds(self, bounds: crate::viewport::PanBounds) -> Self {
        self.edit_viewport_cfg(|c| c.pan_bounds = bounds)
    }

    /// Let the user drag this pane's seam edge to resize it — the CSS
    /// `resize` shape adapted to pane seams, with no divider widget and
    /// no app-side state:
    ///
    /// ```ignore
    /// row([
    ///     sidebar([...]).key("nav").user_resizable()
    ///         .min_width(180.0).max_width(480.0),
    ///     main_pane().width(Size::Fill(1.0)),
    /// ])
    /// ```
    ///
    /// The runtime hit-tests an invisible ~8px grab band straddling
    /// the pane's seam edge (no layout space consumed), flips the
    /// cursor to the resize arrows over it, and keeps the dragged size
    /// in [`crate::UiState`] the way scroll offsets are kept. Layout
    /// then treats the stored size as `Fixed`. Everything is derived:
    /// the resize axis is the parent's layout axis (Row → width,
    /// Column → height), the band sits on the trailing edge when a
    /// sibling follows and on the leading edge when this pane is the
    /// last child (right-anchored inspector), and the drag clamps to
    /// [`Self::min_width`]/[`Self::max_width`] (or the height pair) —
    /// like CSS `min-width`/`max-width` clamp CSS `resize`. The drag
    /// is additionally capped so the seam never leaves the parent's
    /// inner rect.
    ///
    /// The declared size is the default until the first drag; once
    /// dragged, the stored size wins (a dragged `Fill` pane pins to
    /// fixed pixels and its `Fill` siblings absorb the change). Apps
    /// that persist the size read [`crate::UiState::user_size`] (also
    /// on `BuildCx`/`EventCx`), pre-seed with
    /// [`crate::UiState::set_user_size`], and hear a
    /// [`crate::UiEventKind::Resized`] routed to the pane's key when a
    /// drag releases — none of which is required for the common case.
    ///
    /// Pointer-only, like CSS `resize`. For a keyboard-accessible
    /// divider or a weighted 50/50 split, use
    /// [`crate::widgets::resize_handle`] instead.
    pub fn user_resizable(mut self) -> Self {
        self.user_resizable = true;
        self
    }

    /// Stick this scroll viewport's offset to the tail of its content
    /// the way chat logs and activity feeds do — when new children land
    /// below the current bottom, the offset follows them; when the user
    /// scrolls up, the pin releases; when the user scrolls back to the
    /// bottom, it re-engages. Mirrors `egui::ScrollArea::stick_to_bottom`.
    ///
    /// On first layout the offset starts at `max_offset`, so a freshly
    /// mounted `scroll([...]).pin_end()` paints with its tail visible
    /// rather than its head. Programmatic
    /// [`crate::scroll::ScrollRequest::EnsureVisible`] requests that
    /// resolve away from the tail also release the pin, so a
    /// "jump-to-message N" action behaves as the user expects.
    pub fn pin_end(mut self) -> Self {
        self.pin_policy = crate::tree::PinPolicy::End;
        self
    }

    /// Stick this scroll viewport's offset to the head of its content —
    /// useful for virtual lists whose newest rows arrive at the top
    /// (commit logs, reverse-chronological activity feeds). When the
    /// pin is engaged the offset stays at `0` so the new rows stay
    /// visible; the user scrolling down releases the pin, and
    /// scrolling back to the top re-engages it. The symmetric
    /// counterpart to [`Self::pin_end`].
    ///
    /// On first layout the offset is `0` (which is also the default
    /// without any pin), so the visible effect of `pin_start` only
    /// kicks in across rebuilds — in particular it overrides a
    /// dynamic virtual list's anchor preservation so newly prepended
    /// rows don't get hidden by the anchor that was preserving the
    /// previously-visible row.
    pub fn pin_start(mut self) -> Self {
        self.pin_policy = crate::tree::PinPolicy::Start;
        self
    }

    /// Override how a dynamic virtual list chooses the in-viewport row
    /// point that anchors the next frame.
    pub fn virtual_anchor_policy(mut self, policy: VirtualAnchorPolicy) -> Self {
        if let Some(items) = self.virtual_items.take() {
            self.virtual_items = Some(items.anchor_policy(policy));
        }
        self
    }

    /// Declare the append-only contract on a dynamic virtual list: the
    /// row sequence only ever grows at the tail and/or drops a prefix at
    /// the head. Switches layout to an O(visible) incremental height
    /// index instead of the general O(n)-per-frame walk — see
    /// [`crate::layout::VirtualMode::Dynamic::append_only`]. No-op on a
    /// fixed list.
    pub fn append_only(mut self) -> Self {
        if let Some(items) = self.virtual_items.take() {
            self.virtual_items = Some(items.append_only());
        }
        self
    }

    /// Treat this element's focusable children as a single
    /// arrow-navigable group with `Up` / `Down` stepping — the menu
    /// shape. Shorthand for `arrow_nav(ArrowNav::Vertical)`.
    pub fn arrow_nav_siblings(self) -> Self {
        self.arrow_nav(crate::tree::ArrowNav::Vertical)
    }

    /// Treat this element's focusable children as a single
    /// arrow-navigable group with the given orientation (mirrors
    /// `aria-orientation`; see [`crate::tree::ArrowNav`] for which
    /// keys each mode covers).
    pub fn arrow_nav(mut self, mode: crate::tree::ArrowNav) -> Self {
        self.arrow_nav = Some(mode);
        self
    }

    /// Replace the column/row/overlay distribution for this node with
    /// a custom child layout function.
    pub fn layout<F>(mut self, f: F) -> Self
    where
        F: Fn(LayoutCtx) -> Vec<Rect> + Send + Sync + 'static,
    {
        self.layout_override = Some(LayoutFn::new(f));
        self
    }

    // ---- Children ----
    /// Append a single child.
    pub fn child(mut self, c: impl Into<El>) -> Self {
        self.children.push(c.into());
        self
    }

    /// Append every element of an iterator as children.
    pub fn children<I, E>(mut self, cs: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        self.children.extend(cs.into_iter().map(Into::into));
        self
    }

    /// Set the layout axis directly.
    pub fn axis(mut self, a: Axis) -> Self {
        self.axis = a;
        self
    }

    // ---- Internal stock defaults ----
    pub(crate) fn default_width(mut self, w: Size) -> Self {
        self.width = w;
        self.explicit_width = false;
        self
    }

    pub(crate) fn default_height(mut self, h: Size) -> Self {
        self.height = h;
        self.explicit_height = false;
        self
    }

    pub(crate) fn default_padding(mut self, p: impl Into<Sides>) -> Self {
        self.padding = p.into();
        self.explicit_padding = false;
        self
    }

    pub(crate) fn default_gap(mut self, g: f32) -> Self {
        self.gap = g;
        self.explicit_gap = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Kind;

    fn fresh() -> El {
        El::new(Kind::Group)
    }

    #[test]
    fn pt_sets_only_top_and_marks_explicit() {
        let el = fresh().pt(7.0);
        assert_eq!(
            el.padding,
            Sides {
                left: 0.0,
                right: 0.0,
                top: 7.0,
                bottom: 0.0
            }
        );
        assert!(el.explicit_padding);
    }

    #[test]
    fn px_py_set_only_their_axis() {
        let el = fresh().px(4.0).py(2.0);
        assert_eq!(
            el.padding,
            Sides {
                left: 4.0,
                right: 4.0,
                top: 2.0,
                bottom: 2.0
            }
        );
        assert!(el.explicit_padding);
    }

    #[test]
    fn pt_overrides_only_top_when_following_padding() {
        // Tailwind shape: `p-4 pt-0` keeps left/right/bottom at 4 and zeros only top.
        let el = fresh().padding(4.0).pt(0.0);
        assert_eq!(
            el.padding,
            Sides {
                left: 4.0,
                right: 4.0,
                top: 0.0,
                bottom: 4.0
            }
        );
        assert!(el.explicit_padding);
    }

    #[test]
    fn pt_after_default_padding_preserves_other_sides_and_marks_explicit() {
        // Constructor default of all-4, then author overrides just the top to 0.
        // Other sides keep the default's value; explicit_padding flips so the
        // metrics pass cannot stamp over the override.
        let el = fresh().default_padding(4.0).pt(0.0);
        assert_eq!(
            el.padding,
            Sides {
                left: 4.0,
                right: 4.0,
                top: 0.0,
                bottom: 4.0
            }
        );
        assert!(el.explicit_padding);
    }

    #[test]
    fn per_side_chainables_compose() {
        let el = fresh().pl(1.0).pr(2.0).pt(3.0).pb(4.0);
        assert_eq!(
            el.padding,
            Sides {
                left: 1.0,
                right: 2.0,
                top: 3.0,
                bottom: 4.0
            }
        );
        assert!(el.explicit_padding);
    }

    #[test]
    fn fill_width_sets_only_width_to_fill_one() {
        let el = fresh().fill_width();
        assert_eq!(el.width, Size::Fill(1.0));
        assert!(el.explicit_width);
        assert!(!el.explicit_height);
    }

    #[test]
    fn fill_height_sets_only_height_to_fill_one() {
        let el = fresh().fill_height();
        assert_eq!(el.height, Size::Fill(1.0));
        assert!(el.explicit_height);
        assert!(!el.explicit_width);
    }

    #[test]
    fn fill_width_and_fill_height_compose_to_fill_size() {
        let combined = fresh().fill_width().fill_height();
        let either = fresh().fill_size();
        assert_eq!(combined.width, either.width);
        assert_eq!(combined.height, either.height);
        assert_eq!(combined.explicit_width, either.explicit_width);
        assert_eq!(combined.explicit_height, either.explicit_height);
    }

    #[test]
    fn sides_x_and_y_constructors_only_populate_one_axis() {
        assert_eq!(
            Sides::x(5.0),
            Sides {
                left: 5.0,
                right: 5.0,
                top: 0.0,
                bottom: 0.0
            }
        );
        assert_eq!(
            Sides::y(5.0),
            Sides {
                left: 0.0,
                right: 0.0,
                top: 5.0,
                bottom: 5.0
            }
        );
    }
}
