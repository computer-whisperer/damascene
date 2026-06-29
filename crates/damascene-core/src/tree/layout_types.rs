//! Layout intent enums carried by [`El`](crate::El).

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

/// Sizing intent along one axis.
///
/// - `Fixed(px)` -- exact size.
/// - `Fill(weight)` -- claim a share of leftover space; weights are relative.
/// - `Hug` -- intrinsic size of contents.
/// - `Aspect(ratio)` -- size derived from the other axis: `this = ratio * other`.
///   Use it to lock an El's aspect ratio against a sibling-driven axis, e.g.
///   `width(Size::Fill(1.0)).height(Size::Aspect(nat_h / nat_w))` for an image
///   that fills its column's width and reports a proportional height back so
///   surrounding layout flows around it.
///
///   Resolves on either axis. Inside a flex container, an `Aspect` on the main
///   axis triggers a cross-first ordering for that child only (cross resolves
///   from its own intent, then main = cross × ratio). The opposite pairing
///   (`Aspect` on cross with anything else on main) uses the normal main-then-
///   cross flow. Both axes `Aspect` is degenerate and falls back to intrinsic.
///
///   `min_width` / `max_width` / `min_height` / `max_height` apply to *both*
///   axes: the basis is clamped before being multiplied by `ratio`, and the
///   derived axis is then clamped by its own bounds. A hugging parent will
///   see the clamped intrinsic, so layout stays consistent with paint.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Size {
    /// Exact size in logical px.
    Fixed(f32),
    /// Claim a share of leftover space; weights are relative.
    Fill(f32),
    /// Intrinsic size of contents (the default).
    #[default]
    Hug,
    /// Size derived from the other axis: `this = ratio * other`.
    /// See the enum-level doc for the resolution rules.
    Aspect(f32),
    /// A multiple of the node's `0`-digit advance — the CSS `ch` unit. Sizes
    /// the axis to `n` tabular digit slots in the node's own font, so a
    /// reserved numeric field (`.tabular_numerals().width(Size::Ch(5))`) holds
    /// a fixed width as the value's length changes. Resolved to [`Size::Fixed`]
    /// against the node's resolved font at the start of layout.
    Ch(f32),
}

/// Layout direction for a container's children.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Axis {
    /// No layout; children share the parent's rect.
    #[default]
    Overlay,
    /// Stack children top-to-bottom.
    Column,
    /// Stack children left-to-right.
    Row,
}

/// Cross-axis sizing and alignment of children, mirroring CSS `align-items`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Align {
    /// Pin to the start of the cross axis.
    Start,
    /// Center in the cross extent.
    Center,
    /// Pin to the end of the cross axis.
    End,
    /// Stretch non-`Fixed` children to the container's cross extent.
    #[default]
    Stretch,
}

/// Main-axis distribution when children do not fill the container.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Justify {
    /// Pack children at the start of the main axis (the default).
    #[default]
    Start,
    /// Center children along the main axis.
    Center,
    /// Pack children at the end of the main axis.
    End,
    /// Distribute leftover space evenly between children.
    SpaceBetween,
}

/// Sticky-edge behavior for a scroll viewport. Mirrors egui's
/// `ScrollArea::stick_to_bottom` family.
///
/// - `None` -- the stored offset is the only source of truth; content
///   changes do not shift it.
/// - `Start` -- when engaged, the offset stays glued to `0` so newly
///   added rows at the top stay visible. Engages on first layout and
///   re-engages when the user scrolls back to the head; releases when
///   the user scrolls away.
/// - `End` -- when engaged, the offset stays glued to `max_offset` so
///   newly added rows at the bottom stay visible (chat-log idiom).
///   Engages on first layout, releases on scroll-away, re-engages on
///   return to the tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PinPolicy {
    /// No stickiness; the stored offset is the only source of truth (the default).
    #[default]
    None,
    /// Glue the offset to the head of the content while engaged.
    Start,
    /// Glue the offset to the tail of the content while engaged (chat-log idiom).
    End,
}

/// Which arrow keys move focus inside an arrow-navigable group (see
/// [`crate::tree::El::arrow_nav`]), mirroring `aria-orientation` in the
/// WAI-ARIA patterns the group widgets cite.
///
/// - `Vertical` -- `Up` / `Down` step among the group (menus,
///   `popover_panel`). `Left` / `Right` fall through so a menubar can
///   still move between menus.
/// - `Horizontal` -- `Left` / `Right` step (tab lists, toggle rows).
///   `Up` / `Down` fall through.
/// - `Both` -- all four arrows step linearly (radio groups: ARIA says
///   `Up`/`Left` previous, `Down`/`Right` next regardless of layout).
/// - `Grid` -- 2D month-grid navigation (`calendar_month`): `Left` /
///   `Right` step in tree order, `Up` / `Down` move to the nearest
///   focusable in the row above / below by layout geometry, so
///   disabled (unfocusable) cells are skipped. Unlike the linear
///   modes, the group collects all focusable *descendants* of the
///   flagged node — the cells live inside intermediate row containers.
///
/// `Home` / `End` jump to the group's first / last member in every
/// mode. Steps saturate at the ends (no wrap), matching the menu
/// groups' existing behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowNav {
    /// `Up` / `Down` step among the group; `Left` / `Right` fall through.
    Vertical,
    /// `Left` / `Right` step among the group; `Up` / `Down` fall through.
    Horizontal,
    /// All four arrows step linearly (ARIA radio-group convention).
    Both,
    /// 2D grid navigation: `Left` / `Right` in tree order, `Up` / `Down`
    /// by layout geometry across rows.
    Grid,
}

impl ArrowNav {
    /// Whether this mode consumes `key` for group navigation (`Home` /
    /// `End` always; arrows per orientation). Non-consumed keys fall
    /// through to the default `KeyDown` routing.
    pub(crate) fn handles(self, logical: &crate::event::LogicalKey) -> bool {
        use crate::event::NamedKey;
        match logical.named() {
            Some(NamedKey::Home | NamedKey::End) => true,
            Some(NamedKey::ArrowUp | NamedKey::ArrowDown) => {
                matches!(self, Self::Vertical | Self::Both | Self::Grid)
            }
            Some(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                matches!(self, Self::Horizontal | Self::Both | Self::Grid)
            }
            _ => false,
        }
    }
}
