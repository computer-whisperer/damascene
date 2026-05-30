//! Anchored popovers — floating surfaces positioned relative to another
//! keyed element or to a logical-pixel point.
//!
//! # The shape
//!
//! Apps own the open/closed state and render the popover only while
//! open. Compose at the root using `stack`:
//!
//! ```ignore
//! fn build(&self, _cx: &BuildCx) -> El {
//!     let mut layers = vec![self.main_view()];
//!     if self.menu_open {
//!         layers.push(popover(
//!             "color-menu",
//!             Anchor::below_key("color-trigger"),
//!             [menu_item("Red"), menu_item("Blue"), menu_item("Green")],
//!         ));
//!     }
//!     stack(layers)
//! }
//! ```
//!
//! The popover layer fills the viewport, paints a transparent dismiss
//! scrim under the panel, and anchors the panel via [`anchor_rect`] —
//! which flips to the opposite side if the requested placement would
//! clip against the viewport. Click outside the panel emits
//! `{key}:dismiss`; `Escape` is delivered as a `UiEventKind::Escape`
//! event whose target is the focused element (apps route both to close
//! the popover).
//!
//! # Why explicit composition (no portal hoist)
//!
//! Damascene's grain is "every visible thing is in the El tree at the
//! source location it was authored." A portal mechanism (where a node
//! lives in one place in the tree but paints somewhere else) would
//! subvert clip escape, focus order, hit-test recursion, and lint
//! output for one feature, and force every future feature to think
//! about portaling. Composing at the root keeps the contract uniform:
//! the popover is a sibling of main content, paints last, hit-tests
//! first. See `widget_kit.md`.
//!
//! # Dogfood
//!
//! `popover` is a composition of [`crate::overlay`], a keyed
//! [`crate::scrim`], and a custom-laid-out container that uses
//! [`crate::layout::LayoutCtx::rect_of_key`] to position its panel
//! relative to the trigger. An app crate can write an equivalent
//! floating layer against the same public surface.

use std::panic::Location;

use crate::event::{PointerKind, UiEvent};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// Minimum comfortable row height for touch-opened action menus.
pub const TOUCH_MENU_ITEM_HEIGHT: f32 = 48.0;

/// Density applied to action menu rows.
///
/// Menus opened by a precise pointer stay compact. Menus opened from
/// touch should use the larger variant so each action has a platform-
/// sized tap target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MenuDensity {
    /// Desktop / pointer density.
    #[default]
    Compact,
    /// Touch density with larger row targets.
    Touch,
}

impl MenuDensity {
    /// Resolve a menu density from the pointer modality that opened it.
    pub fn from_pointer_kind(kind: Option<PointerKind>) -> Self {
        if matches!(kind, Some(PointerKind::Touch)) {
            Self::Touch
        } else {
            Self::Compact
        }
    }

    /// Resolve a menu density from the event that opened it.
    pub fn from_event(event: &UiEvent) -> Self {
        Self::from_pointer_kind(event.pointer_kind)
    }
}

/// Apply a density to all stock menu rows in a subtree.
///
/// This lets higher-level menu constructors keep accepting ordinary
/// `menu_item(...)` / `dropdown_menu_item(...)` children while adapting
/// the final menu surface for touch.
pub fn apply_menu_density(mut el: El, density: MenuDensity) -> El {
    apply_menu_density_to_tree(&mut el, density);
    el
}

fn apply_menu_density_to_tree(el: &mut El, density: MenuDensity) {
    if matches!(density, MenuDensity::Touch)
        && matches!(
            el.kind,
            Kind::Custom("menu_item") | Kind::Custom("dropdown_menu_item")
        )
    {
        el.height = Size::Fixed(TOUCH_MENU_ITEM_HEIGHT);
        el.padding = Sides::xy(tokens::SPACE_4, 0.0);
        el.explicit_height = true;
        el.explicit_padding = true;
    }

    for child in &mut el.children {
        apply_menu_density_to_tree(child, density);
    }
}
use crate::widgets::overlay::overlay;

/// Default spacing between a tooltip / floating surface and its
/// anchor. Right-click menus and dropdowns flush their panel against
/// the trigger (gap = 0) — they pass `0.0` to [`anchor_rect`]
/// directly. This constant is the breathing-room value tooltips and
/// other non-menu floating panels use.
pub const ANCHOR_GAP: f32 = tokens::SPACE_1;

/// Where a popover sits relative to its anchor.
///
/// `Below` / `Above` align the panel's left edge with the anchor's
/// left edge and stack on the cross-axis. `Right` / `Left` align the
/// top edges and stack on the main-axis. `AtPoint` places the panel's
/// top-left corner at the anchor (used by context menus).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Side {
    Below,
    Above,
    Right,
    Left,
    AtPoint,
}

/// What a popover anchors to.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Anchor {
    /// Stick to another keyed element's laid-out rect. The library
    /// reads `LayoutCtx::rect_of_key(key)` at layout time; if the key
    /// isn't found (trigger scrolled out of view, removed by a
    /// rebuild, etc.) the panel falls back to the viewport origin.
    Key { key: String, side: Side },
    /// Anchor by a node's `computed_id`. Used by runtime-synthesized
    /// layers (tooltips) that already know the trigger by id and
    /// don't need (or have) a key. Caller looks up via
    /// `LayoutCtx::rect_of_id`.
    Id { id: String, side: Side },
    /// Anchor at an absolute logical-pixel point. Used for context
    /// menus (anchor at right-click position) and any popup that
    /// follows a position the app already computed.
    Point { x: f32, y: f32, side: Side },
}

impl Anchor {
    pub fn below_key(key: impl Into<String>) -> Self {
        Anchor::Key {
            key: key.into(),
            side: Side::Below,
        }
    }
    pub fn above_key(key: impl Into<String>) -> Self {
        Anchor::Key {
            key: key.into(),
            side: Side::Above,
        }
    }
    pub fn right_of_key(key: impl Into<String>) -> Self {
        Anchor::Key {
            key: key.into(),
            side: Side::Right,
        }
    }
    pub fn left_of_key(key: impl Into<String>) -> Self {
        Anchor::Key {
            key: key.into(),
            side: Side::Left,
        }
    }
    pub fn at_point(x: f32, y: f32) -> Self {
        Anchor::Point {
            x,
            y,
            side: Side::AtPoint,
        }
    }
    pub fn below_id(id: impl Into<String>) -> Self {
        Anchor::Id {
            id: id.into(),
            side: Side::Below,
        }
    }
}

/// Compute the laid-out rect for a popover panel of `panel_size`
/// anchored by `anchor` inside `viewport`.
///
/// `gap` is the breathing-room spacing between the anchor and the
/// panel along the primary axis (Below/Above/Right/Left). Right-click
/// menus and dropdowns flush their panel against the trigger by
/// passing `0.0`; tooltips pass [`ANCHOR_GAP`] for visual separation
/// from the trigger.
///
/// Behavior:
///
/// - **Side::Below / Above** — panel's left edge aligns with the
///   anchor's left edge; the cross-axis side flips to the opposite
///   side if the requested side would extend past the viewport edge.
/// - **Side::Right / Left** — panel's top aligns with the anchor's
///   top; flips horizontally on overflow.
/// - **Side::AtPoint** — top-left corner at the anchor point;
///   `gap` is ignored (the point is the placement).
/// - After placement, the rect is shifted (not flipped) so it stays
///   within the viewport on the secondary axis. Panels larger than
///   the viewport are pinned to the top-left.
/// - **Missing key** — when `Anchor::Key` and `lookup` returns `None`,
///   the panel lands at the viewport top-left at its requested size.
///
/// Pure function — the caller (the popover's `layout_override`) is
/// responsible for invoking it with the panel's intrinsic size and
/// the popover layer's own container rect.
pub fn anchor_rect(
    anchor: &Anchor,
    panel_size: (f32, f32),
    viewport: Rect,
    lookup: &dyn Fn(&str) -> Option<Rect>,
    gap: f32,
) -> Rect {
    let (w, h) = panel_size;
    // Reduce both anchor variants to a single `(anchor_rect, side)`
    // pair. `Anchor::Point` becomes a zero-size rect at the point.
    let (anchor_rect, side) = match anchor {
        Anchor::Key { key, side } => match lookup(key) {
            Some(r) => (r, *side),
            None => return Rect::new(viewport.x, viewport.y, w, h),
        },
        Anchor::Id { id, side } => match lookup(id) {
            Some(r) => (r, *side),
            None => return Rect::new(viewport.x, viewport.y, w, h),
        },
        Anchor::Point { x, y, side } => (Rect::new(*x, *y, 0.0, 0.0), *side),
    };

    let (mut x, mut y) = match side {
        Side::Below => (anchor_rect.x, anchor_rect.bottom() + gap),
        Side::Above => (anchor_rect.x, anchor_rect.y - gap - h),
        Side::Right => (anchor_rect.right() + gap, anchor_rect.y),
        Side::Left => (anchor_rect.x - gap - w, anchor_rect.y),
        Side::AtPoint => (anchor_rect.x, anchor_rect.y),
    };

    // Flip to opposite side when the primary side would clip. Only the
    // primary axis flips; the secondary axis is shifted (below).
    match side {
        Side::Below if y + h > viewport.bottom() => {
            let flipped = anchor_rect.y - gap - h;
            if flipped >= viewport.y {
                y = flipped;
            }
        }
        Side::Above if y < viewport.y => {
            let flipped = anchor_rect.bottom() + gap;
            if flipped + h <= viewport.bottom() {
                y = flipped;
            }
        }
        Side::Right if x + w > viewport.right() => {
            let flipped = anchor_rect.x - gap - w;
            if flipped >= viewport.x {
                x = flipped;
            }
        }
        Side::Left if x < viewport.x => {
            let flipped = anchor_rect.right() + gap;
            if flipped + w <= viewport.right() {
                x = flipped;
            }
        }
        _ => {}
    }

    // Secondary-axis clamp so menus near a viewport edge don't paint
    // off-screen. Panels wider/taller than the viewport pin to the
    // top-left edge.
    if x + w > viewport.right() {
        x = viewport.right() - w;
    }
    if x < viewport.x {
        x = viewport.x;
    }
    if y + h > viewport.bottom() {
        y = viewport.bottom() - h;
    }
    if y < viewport.y {
        y = viewport.y;
    }

    Rect::new(x, y, w, h)
}

/// A floating element anchored to another element or to a point,
/// sitting over a transparent dismiss scrim. Compose at the root via
/// `stack`; the app owns open/closed state and renders this only
/// while open.
///
/// `panel` is the single El to position — typically a
/// [`popover_panel`], but any El works. The panel is wrapped in a
/// `block_pointer` layer so clicks on it don't fall through to the
/// dismiss scrim.
///
/// Keys:
/// - `{key}:dismiss` — emitted when the user clicks outside the panel.
///   The app's event handler matches this and clears its open flag.
/// - The panel keeps its own keyed children for routing button clicks etc.
///
/// Anchoring fallback: when `Anchor::Key` references a key that isn't
/// present (e.g. the trigger has been scrolled out of view), the panel
/// lays out at the viewport origin. Apps that don't want this should
/// avoid rendering the popover when the trigger isn't visible.
#[track_caller]
pub fn popover(key: impl Into<String>, anchor: Anchor, panel: impl Into<El>) -> El {
    let key = key.into();
    let dismiss_key = format!("{key}:dismiss");
    overlay([
        // Transparent dismiss scrim — full-viewport, keyed so the app
        // can route click-outside to close. No fill so the main view
        // stays visible behind the popover.
        El::new(Kind::Scrim)
            .at_loc(Location::caller())
            .key(dismiss_key)
            .fill_size(),
        // Custom-laid-out container that positions the panel against
        // the anchor. Fills the viewport so its `container` rect is
        // the placement region; the `layout_override` reads the
        // panel's intrinsic size and asks `anchor_rect` where to put
        // it. The panel itself (not the layer) carries
        // `block_pointer` — applied inside `anchored_panel` — so
        // clicks INSIDE the panel rect don't fall through to the
        // scrim, but clicks OUTSIDE still reach the scrim and emit
        // dismiss.
        anchored_panel(anchor, panel.into()),
    ])
}

/// The bare popover panel (a card with shadow + border + radius)
/// without the dismiss scrim. Useful for tooltips and any non-modal
/// floating surface where outside clicks should NOT be intercepted.
///
/// Compose at the root the same way as [`popover`]; the panel itself
/// won't fill the viewport, so wrap it in a layer that does (e.g.
/// [`crate::overlay`]) when the host needs the panel to escape its
/// trigger's clip.
#[track_caller]
pub fn popover_panel<I, E>(body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let children: Vec<El> = body.into_iter().map(Into::into).collect();
    El::new(Kind::Custom("popover_panel"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Popover)
        .arrow_nav_siblings()
        .children(children)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .radius(0.0)
        .shadow(tokens::SHADOW_MD)
        .padding(Sides::zero())
        .gap(0.0)
        .width(Size::Hug)
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}

/// A single menu row. A container with a child text node — composing
/// the label as a child (rather than `.text(...)` on the row itself)
/// is what makes `padding` actually offset the label from the row's
/// left edge. With `.text(...)` on the row, `draw_ops` paints the
/// glyph run at the row's full layout rect and ignores padding when
/// `text_align == Start`; using a child node positions the label via
/// layout instead.
///
/// Items render flat — no per-row stroke, shadow, or radius — so a
/// menu reads as a continuous "receipt" of full-width rows rather
/// than a stack of independent buttons. The rest `fill` matches the
/// panel surface so it's visually invisible at rest, but it's
/// required for the hover-lighten / press-darken envelopes
/// (`apply_state` in `draw_ops`) to have a colour to mix against.
/// Without a rest fill, `fill.map(...)` is a no-op and the item
/// produces no hover visual.
///
/// Apps key these with the action they route to; clicks emit
/// `UiEventKind::Click` to that key.
#[track_caller]
pub fn menu_item(label: impl Into<String>) -> El {
    let label = El::new(Kind::Text)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .text(label)
        .text_role(TextRole::Label)
        .text_color(tokens::FOREGROUND)
        .font_weight(FontWeight::Regular)
        .hug();
    El::new(Kind::Custom("menu_item"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::MenuItem)
        .focusable()
        .focus_ring_inside()
        .child(label)
        .fill(tokens::POPOVER)
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .default_gap(0.0)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(28.0))
        .axis(Axis::Row)
        .align(Align::Center)
        .justify(Justify::Start)
}

#[track_caller]
pub fn menu_item_with_density(label: impl Into<String>, density: MenuDensity) -> El {
    apply_menu_density(menu_item(label), density).at_loc(Location::caller())
}

/// Right-click context menu — popover anchored at a logical-pixel
/// point with menu items inside a stock [`popover_panel`]. Apps
/// capture the click position from `UiEvent.pointer` on a
/// `SecondaryClick` and stash it alongside the `open` flag.
#[track_caller]
pub fn context_menu<I, E>(key: impl Into<String>, point: (f32, f32), items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    popover(
        key,
        Anchor::at_point(point.0, point.1),
        popover_panel(items),
    )
}

#[track_caller]
pub fn context_menu_with_density<I, E>(
    key: impl Into<String>,
    point: (f32, f32),
    density: MenuDensity,
    items: I,
) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    context_menu(
        key,
        point,
        items
            .into_iter()
            .map(Into::into)
            .map(|item| apply_menu_density(item, density)),
    )
    .at_loc(Location::caller())
}

/// Dropdown menu — popover anchored below a trigger by key, with menu
/// items inside a stock [`popover_panel`]. The trigger element must
/// be present in the laid-out tree (it's looked up via
/// `LayoutCtx::rect_of_key`).
#[track_caller]
pub fn dropdown<I, E>(key: impl Into<String>, trigger_key: impl Into<String>, items: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    popover(key, Anchor::below_key(trigger_key), popover_panel(items))
}

/// Internal: a `Kind::Custom("popover_layer")` that fills the viewport
/// and uses `layout_override` to anchor its single child (the panel)
/// via [`anchor_rect`]. Stamps `block_pointer` onto the panel so
/// clicks on the panel rect don't fall through to the dismiss scrim
/// — *only* the panel's rect blocks; the layer itself is hit-test
/// transparent so clicks outside the panel reach the scrim.
#[track_caller]
fn anchored_panel(anchor: Anchor, panel: El) -> El {
    let panel = panel.block_pointer();
    El::new(Kind::Custom("popover_layer"))
        .at_loc(Location::caller())
        .child(panel)
        .fill_size()
        .layout(move |ctx| {
            let (w, h) = (ctx.measure)(&ctx.children[0]);
            // Menus flush against the trigger — no breathing-room gap.
            let rect = anchor_rect(&anchor, (w, h), ctx.container, ctx.rect_of_key, 0.0);
            vec![rect]
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_lookup() -> impl Fn(&str) -> Option<Rect> {
        |_: &str| None
    }

    fn lookup_one(key: &'static str, rect: Rect) -> impl Fn(&str) -> Option<Rect> {
        move |k: &str| if k == key { Some(rect) } else { None }
    }

    fn vp() -> Rect {
        Rect::new(0.0, 0.0, 400.0, 300.0)
    }

    #[test]
    fn anchor_rect_below_key_aligns_left_edge_and_drops_below() {
        let trig = Rect::new(50.0, 40.0, 80.0, 24.0);
        let r = anchor_rect(
            &Anchor::below_key("t"),
            (120.0, 60.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        assert_eq!(r.x, 50.0);
        assert_eq!(r.y, 40.0 + 24.0 + ANCHOR_GAP);
        assert_eq!((r.w, r.h), (120.0, 60.0));
    }

    #[test]
    fn anchor_rect_above_key_aligns_above_with_gap() {
        let trig = Rect::new(60.0, 200.0, 80.0, 24.0);
        let r = anchor_rect(
            &Anchor::above_key("t"),
            (120.0, 50.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        assert_eq!(r.x, 60.0);
        assert_eq!(r.y, 200.0 - ANCHOR_GAP - 50.0);
    }

    #[test]
    fn anchor_rect_below_flips_to_above_when_overflow_bottom() {
        // Trigger near the bottom of the viewport — Below would clip,
        // Above fits. Expect a flip to Above.
        let trig = Rect::new(50.0, 270.0, 80.0, 24.0);
        let r = anchor_rect(
            &Anchor::below_key("t"),
            (120.0, 60.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        // Above placement: y = trig.y - GAP - h = 270 - 4 - 60 = 206
        assert_eq!(r.y, 270.0 - ANCHOR_GAP - 60.0);
    }

    #[test]
    fn anchor_rect_below_does_not_flip_when_above_also_overflows() {
        // Both sides overflow → keep the requested side and clamp
        // (not flip into a worse position).
        let trig = Rect::new(50.0, 280.0, 80.0, 12.0);
        let r = anchor_rect(
            &Anchor::below_key("t"),
            (120.0, 320.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        // Panel taller than the whole viewport → pinned to top.
        assert_eq!(r.y, 0.0);
    }

    #[test]
    fn anchor_rect_above_flips_to_below_when_overflow_top() {
        let trig = Rect::new(50.0, 10.0, 80.0, 24.0);
        let r = anchor_rect(
            &Anchor::above_key("t"),
            (120.0, 60.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        assert_eq!(r.y, 10.0 + 24.0 + ANCHOR_GAP);
    }

    #[test]
    fn anchor_rect_right_flips_to_left_when_overflow_right() {
        let trig = Rect::new(360.0, 100.0, 30.0, 30.0);
        let r = anchor_rect(
            &Anchor::right_of_key("t"),
            (80.0, 50.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        // Right placement: x = 390 + 4 = 394 + 80 = 474 > 400 → flip
        // to Left: x = 360 - 4 - 80 = 276
        assert_eq!(r.x, 360.0 - ANCHOR_GAP - 80.0);
    }

    #[test]
    fn anchor_rect_left_flips_to_right_when_overflow_left() {
        let trig = Rect::new(10.0, 100.0, 30.0, 30.0);
        let r = anchor_rect(
            &Anchor::left_of_key("t"),
            (80.0, 50.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        // Left placement: x = 10 - 4 - 80 = -74 → flip to Right.
        assert_eq!(r.x, 10.0 + 30.0 + ANCHOR_GAP);
    }

    #[test]
    fn anchor_rect_at_point_pins_top_left_to_point() {
        let r = anchor_rect(
            &Anchor::at_point(120.0, 80.0),
            (60.0, 40.0),
            vp(),
            &no_lookup(),
            ANCHOR_GAP,
        );
        assert_eq!((r.x, r.y), (120.0, 80.0));
    }

    #[test]
    fn anchor_rect_at_point_clamps_into_viewport_on_overflow() {
        let r = anchor_rect(
            &Anchor::at_point(380.0, 280.0),
            (60.0, 40.0),
            vp(),
            &no_lookup(),
            ANCHOR_GAP,
        );
        // Top-left at (380, 280) puts bottom-right at (440, 320),
        // outside (400, 300). Clamp to (340, 260).
        assert_eq!((r.x, r.y), (340.0, 260.0));
    }

    #[test]
    fn anchor_rect_below_clamps_x_when_panel_overflows_right() {
        // Trigger near the right edge — the panel's left edge would
        // align with the trigger but extend past the viewport.
        let trig = Rect::new(380.0, 50.0, 20.0, 20.0);
        let r = anchor_rect(
            &Anchor::below_key("t"),
            (100.0, 40.0),
            vp(),
            &lookup_one("t", trig),
            ANCHOR_GAP,
        );
        // Right edge clamped: x = 400 - 100 = 300.
        assert_eq!(r.x, 300.0);
    }

    #[test]
    fn anchor_rect_missing_key_falls_back_to_viewport_origin() {
        let r = anchor_rect(
            &Anchor::below_key("missing"),
            (60.0, 40.0),
            vp(),
            &no_lookup(),
            ANCHOR_GAP,
        );
        assert_eq!((r.x, r.y), (vp().x, vp().y));
    }

    #[test]
    fn popover_exposes_dismiss_scrim_and_block_pointer_on_panel() {
        // A popover with key "menu" exposes a scrim keyed
        // "menu:dismiss" so the app can route click-outside to close.
        // The panel (not the layer) carries `block_pointer` — the
        // layer fills the viewport, so blocking on the layer would
        // also block clicks meant for the scrim. Regression for
        // "click outside doesn't dismiss the menu."
        let p = popover(
            "menu",
            Anchor::at_point(100.0, 100.0),
            popover_panel([menu_item("Copy"), menu_item("Paste")]),
        );
        let scrim = &p.children[0];
        assert_eq!(scrim.key.as_deref(), Some("menu:dismiss"));
        assert_eq!(scrim.kind, Kind::Scrim);

        let layer = &p.children[1];
        assert!(
            !layer.block_pointer,
            "the popover layer must be hit-test transparent — block_pointer belongs on the panel"
        );
        let panel = &layer.children[0];
        assert!(
            panel.block_pointer,
            "the panel itself must block_pointer so clicks on it don't fall through"
        );
    }

    #[test]
    fn click_outside_panel_routes_to_dismiss_scrim() {
        // End-to-end hit-test regression: lay out a popover anchored
        // at a point and verify that a click well outside the panel
        // returns the dismiss key, not Blocked. This is the actual
        // user-visible bug the layout test alone wouldn't catch.
        use crate::layout::layout;
        use crate::state::UiState;
        use crate::tree::stack;
        let panel_anchor_pt = (40.0, 40.0);
        let mut tree = stack([popover(
            "ctx",
            Anchor::at_point(panel_anchor_pt.0, panel_anchor_pt.1),
            popover_panel([menu_item("A"), menu_item("B")]),
        )]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));

        // A click far from the anchor point should land on the scrim.
        let click_far = (350.0, 250.0);
        let hit = crate::hit_test::hit_test(&tree, &state, click_far);
        assert_eq!(hit.as_deref(), Some("ctx:dismiss"));
    }

    #[test]
    fn click_inside_panel_does_not_route_to_dismiss_scrim() {
        // Inverse of the regression above: a click on the panel
        // (specifically: in the panel's padding/gap area, not on a
        // keyed item) must NOT emit dismiss. block_pointer on the
        // panel makes this a Hit::Blocked, which never reaches the
        // app — exactly what we want.
        use crate::layout::layout;
        use crate::state::UiState;
        use crate::tree::stack;
        let pt = (40.0, 40.0);
        let mut tree = stack([popover(
            "ctx",
            Anchor::at_point(pt.0, pt.1),
            popover_panel([menu_item("A"), menu_item("B")]),
        )]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        // Probe the very corner of the panel — gap area, not on any
        // keyed menu_item. Should be Blocked (= None from hit_test).
        let panel_corner = (pt.0 + 1.0, pt.1 + 1.0);
        let hit = crate::hit_test::hit_test(&tree, &state, panel_corner);
        assert!(
            hit.is_none() || hit.as_deref() != Some("ctx:dismiss"),
            "click on panel must not route to dismiss; got {hit:?}",
        );
    }

    #[test]
    fn dropdown_anchors_below_trigger_key() {
        let dd = dropdown("colors", "trig", [menu_item("Red"), menu_item("Blue")]);
        // Dismiss scrim's key is `{key}:dismiss`.
        let scrim = &dd.children[0];
        assert_eq!(scrim.key.as_deref(), Some("colors:dismiss"));
        // Layer's single child is the popover_panel containing the items.
        let layer = &dd.children[1];
        assert_eq!(layer.children.len(), 1);
        let panel = &layer.children[0];
        assert_eq!(panel.kind, Kind::Custom("popover_panel"));
        assert_eq!(panel.children.len(), 2);
        assert_eq!(
            panel.children[0].focus_ring_placement,
            crate::tree::FocusRingPlacement::Inside
        );
    }

    #[test]
    fn menu_item_with_touch_density_expands_row() {
        let item = menu_item_with_density("Copy", MenuDensity::Touch);

        assert_eq!(item.height, Size::Fixed(TOUCH_MENU_ITEM_HEIGHT));
        assert_eq!(item.padding.left, tokens::SPACE_4);
        assert_eq!(item.padding.right, tokens::SPACE_4);
    }

    #[test]
    fn context_menu_anchors_at_click_point() {
        let cm = context_menu("ctx", (120.0, 80.0), [menu_item("Cut"), menu_item("Copy")]);
        let scrim = &cm.children[0];
        assert_eq!(scrim.key.as_deref(), Some("ctx:dismiss"));
    }

    #[test]
    fn context_menu_with_touch_density_expands_rows() {
        let cm = context_menu_with_density(
            "ctx",
            (120.0, 80.0),
            MenuDensity::Touch,
            [menu_item("Cut"), menu_item("Copy")],
        );
        let panel = &cm.children[1].children[0];

        assert_eq!(
            panel.children[0].height,
            Size::Fixed(TOUCH_MENU_ITEM_HEIGHT)
        );
        assert_eq!(
            panel.children[1].height,
            Size::Fixed(TOUCH_MENU_ITEM_HEIGHT)
        );
    }
}
