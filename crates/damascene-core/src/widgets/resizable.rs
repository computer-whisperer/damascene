//! Resizable panel group — a linear row or column of panels separated
//! by drag-to-resize handles. This is the "slicer settings pane",
//! "voice rail", "inspector column" shape: N panels sharing one axis,
//! each boundary optionally carrying a visible handle the user drags
//! (or arrow-keys) to trade size between the two adjacent panels.
//!
//! # Oracle
//!
//! shadcn **Resizable** (`ResizablePanelGroup` / `ResizablePanel` /
//! `ResizableHandle`), which is itself a recipe over
//! **react-resizable-panels** (`PanelGroup` / `Panel` /
//! `PanelResizeHandle`) — per `docs/NAMING_ORACLE.md`'s
//! recipe-over-headless-library policy the citation names both. As
//! with `TableColumn`, we hoist the geometry-and-interaction half and
//! replace the library's internal percentage state with the house
//! controlled pattern: the app owns the sizes as a `Vec<f32>` of
//! fractions and folds routed events back through [`apply_event`].
//!
//! Recorded divergences from the oracle:
//!
//! - `direction="horizontal" | "vertical"` is expressed as
//!   [`Axis::Row`] / [`Axis::Column`] — the house translation already
//!   established by [`crate::widgets::resize_handle`].
//! - `<ResizableHandle withHandle />` is
//!   [`resizable_handle_with_grip`]: the flattened Rust morphology of
//!   `withHandle` would be the degenerate `resizable_handle_with_handle`.
//!   The anatomy is the oracle's — a small rounded chip riding the
//!   seam line, grip dots inside.
//! - Per-panel `minSize` is simplified to one group-wide
//!   [`ResizableOpts::min_fraction`], expressed as a fraction of the
//!   sizes' sum; the default `0.1` matches react-resizable-panels'
//!   default `minSize` of 10%.
//!
//! # Scope: linear only
//!
//! This widget is deliberately the **linear** panel-group shape — one
//! row or one column. The Blender-style recursive split *tree* (split
//! any pane in two, collapse, re-dock) stays app-side, per
//! `docs/WORKBENCH_VISION.md`'s explicit deferral ("Hoisting
//! eutectic's Blender-style split-tree machinery … stays app-side
//! until a second app needs it"). Nesting a `resizable_panel_group`
//! inside another group's panel covers the static two-level layouts in
//! between.
//!
//! # Shape
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Workbench {
//!     sizes: Vec<f32>, // fractions, e.g. vec![0.2, 0.6, 0.2]
//!     drag: ResizableDrag,
//! }
//!
//! impl App for Workbench {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         resizable_panel_group(Axis::Row, "main", [
//!             resizable_panel(self.sizes[0], rail()),
//!             resizable_handle(),
//!             resizable_panel(self.sizes[1], editor()),
//!             resizable_handle_with_grip(),
//!             resizable_panel(self.sizes[2], inspector()),
//!         ])
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent, cx: &EventCx) {
//!         let extent = cx.rect_of_key("main").map_or(0.0, |r| r.w);
//!         resizable::apply_event(
//!             &mut self.sizes,
//!             &mut self.drag,
//!             &event,
//!             "main",
//!             Axis::Row,
//!             extent,
//!             ResizableOpts::default(),
//!         );
//!     }
//! }
//! ```
//!
//! The group element carries the group key, so the app fetches the
//! main-axis extent with [`crate::event::EventCx::rect_of_key`] — the
//! same capture-the-layout pattern as
//! [`crate::widgets::resize_handle::apply_event_weights`].
//!
//! # Routed keys
//!
//! - `{group_key}:handle:{index}` — PointerDown / Drag / PointerUp /
//!   KeyDown on the handle after `index + 1` panels; it trades size
//!   between `sizes[index]` and `sizes[index + 1]`. Format and parse
//!   with [`resizable_handle_key`].
//!
//! # Relationship to `resize_handle`
//!
//! The handles *are* [`crate::widgets::resize_handle::resize_handle`]
//! — same hairline anatomy, cursor, focus ring, and touch-drag opt-in.
//! Reach for `resize_handle` + `apply_event_fixed` directly for the
//! pinned-pixel-width sidebar case; reach for this group when the
//! panels share the space as fractions. One difference: handles built
//! by the group drop their `hit_overflow` band (the panels are flush
//! against them, so an expanded target would invade panel content —
//! `FindingKind::HitOverflowCollision`, the same trade
//! [`crate::widgets::button_group`]'s `join_row` and
//! `numeric_input`'s stacked chevrons make). The handle's grabbable
//! footprint is already hit-widened *by construction*: its
//! [`HANDLE_THICKNESS`] layout strip is 4× the painted hairline.
//! Seam-flush *panel content* keeps its own `hit_overflow`, so give
//! interactive content that must sit flush against a handle some
//! padding (any normal content inset is plenty — the band is
//! `tokens::HIT_OVERFLOW`, under 2px).
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface (`Kind::Custom`
//! wrappers, [`crate::widgets::resize_handle::resize_handle`], `El`
//! sizing). No privileged internals.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::event::{NamedKey, UiEvent, UiEventKind};
use crate::tokens;
use crate::tree::*;
use crate::widgets::resize_handle::{
    HANDLE_THICKNESS, KEYBOARD_PAGE_STEP_PX, KEYBOARD_STEP_PX, resize_handle,
};

/// Main-axis length of the grip chip on a [`resizable_handle_with_grip`]
/// handle, in logical pixels. (Its cross-axis width is the handle's
/// [`HANDLE_THICKNESS`].)
const GRIP_CHIP_LENGTH: f32 = 20.0;

/// Diameter of one grip dot inside the chip.
const GRIP_DOT: f32 = 2.0;

/// One entry in a [`resizable_panel_group`] children list — either a
/// panel ([`resizable_panel`]) or a handle ([`resizable_handle`] /
/// [`resizable_handle_with_grip`]). Opaque by design: the group
/// resolves axis-dependent sizing and derives handle keys, which is
/// what shadcn's `ResizablePanelGroup` context does for its children.
pub struct ResizableChild {
    inner: ChildInner,
    loc: &'static Location<'static>,
}

enum ChildInner {
    // Boxed so the enum doesn't carry `El`'s full inline size in every
    // handle entry (clippy: large_enum_variant).
    Panel { fraction: f32, content: Box<El> },
    Handle { grip: bool },
}

/// A panel occupying `fraction` of the group's shared main-axis space.
///
/// `fraction` is a `Size::Fill` weight — panels split the space left
/// after the fixed-thickness handles proportionally, so any positive
/// scale works, but the natural unit is fractions summing to `1.0`
/// (shadcn's `defaultSize={25}` becomes `resizable_panel(0.25, …)`).
/// Pass the app-owned `sizes[i]` here so drags round-trip.
///
/// The panel clips its content: a panel dragged smaller than its
/// content must not paint over its neighbors (react-resizable-panels'
/// `overflow: hidden`).
#[track_caller]
pub fn resizable_panel(fraction: f32, content: impl Into<El>) -> ResizableChild {
    debug_assert!(
        fraction >= 0.0 && fraction.is_finite(),
        "panel fraction must be a non-negative finite number, got {fraction}"
    );
    ResizableChild {
        inner: ChildInner::Panel {
            fraction,
            content: Box::new(content.into()),
        },
        loc: Location::caller(),
    }
}

/// A draggable handle on the boundary between the two panels around
/// it. The group derives its routed key — `{group_key}:handle:{i}`
/// for the handle after panel `i` (see [`resizable_handle_key`]) —
/// and orients it to the group's axis, exactly as shadcn's
/// `<ResizableHandle />` picks both up from its `ResizablePanelGroup`
/// context.
///
/// A boundary without a handle is legal and stays fixed — the oracle's
/// contract, and why handles are explicit children rather than
/// auto-inserted.
#[track_caller]
pub fn resizable_handle() -> ResizableChild {
    ResizableChild {
        inner: ChildInner::Handle { grip: false },
        loc: Location::caller(),
    }
}

/// [`resizable_handle`] with the oracle's `withHandle` affordance: a
/// small rounded chip riding the seam line with grip dots inside,
/// marking the seam as grabbable at a glance. (Named `_with_grip`
/// because the literal morphology `resizable_handle_with_handle` is
/// degenerate; the anatomy is shadcn's.)
#[track_caller]
pub fn resizable_handle_with_grip() -> ResizableChild {
    ResizableChild {
        inner: ChildInner::Handle { grip: true },
        loc: Location::caller(),
    }
}

/// Format the routed key of the handle after panel `index` in the
/// group keyed `group_key`. [`apply_event`] parses the same format;
/// handle `index` trades size between `sizes[index]` and
/// `sizes[index + 1]`.
pub fn resizable_handle_key(group_key: &str, index: usize) -> String {
    format!("{group_key}:handle:{index}")
}

/// A linear group of resizable panels. `axis` is the split direction —
/// [`Axis::Row`] panels sit side by side (handles drag left/right),
/// [`Axis::Column`] panels stack (handles drag up/down). `key` names
/// the group: the group element itself carries it (so
/// [`crate::event::EventCx::rect_of_key`] answers with the group's
/// extent for [`apply_event`]) and handle keys derive from it.
///
/// `children` interleave [`resizable_panel`] and [`resizable_handle`]
/// entries; every handle must have a panel on both sides.
///
/// The group fills its parent in both axes by default (shadcn's
/// `h-full w-full` flex group); chain `.width(…)` / `.height(…)` to
/// override.
#[track_caller]
pub fn resizable_panel_group<I>(axis: Axis, key: impl Into<String>, children: I) -> El
where
    I: IntoIterator<Item = ResizableChild>,
{
    let caller = Location::caller();
    let group_key = key.into();
    // Overlay isn't a real split direction — degrade to Row like
    // `resize_handle` does rather than panic in a builder.
    let axis = match axis {
        Axis::Overlay => Axis::Row,
        other => other,
    };
    let mut els: Vec<El> = Vec::new();
    let mut panels = 0usize;
    let mut prev_was_handle = false;
    for child in children {
        match child.inner {
            ChildInner::Panel { fraction, content } => {
                let (width, height) = match axis {
                    Axis::Row => (Size::Fill(fraction), Size::Fill(1.0)),
                    Axis::Column | Axis::Overlay => (Size::Fill(1.0), Size::Fill(fraction)),
                };
                els.push(
                    El::new(Kind::Custom("resizable-panel"))
                        .at_loc(child.loc)
                        .width(width)
                        .height(height)
                        // A panel dragged smaller than its content must
                        // not paint over its neighbors.
                        .clip()
                        .child(*content),
                );
                panels += 1;
                prev_was_handle = false;
            }
            ChildInner::Handle { grip } => {
                debug_assert!(
                    panels > 0 && !prev_was_handle,
                    "a resizable_handle needs a resizable_panel on both sides"
                );
                let index = panels.saturating_sub(1);
                // The handle is the stock `resize_handle` — hairline,
                // resize cursor, inside focus ring, touch-drag opt-in —
                // minus its hit_overflow band: the panels are flush
                // against it, so an expanded target would invade panel
                // content (`HitOverflowCollision`; the join_row /
                // stacked-chevron trade). Its 8px layout strip already
                // hit-widens the 2px hairline by construction.
                let mut handle = resize_handle(resizable_handle_key(&group_key, index), axis)
                    .at_loc(child.loc)
                    .hit_overflow(Sides::zero());
                if grip {
                    handle = handle.child(grip_chip(axis));
                }
                els.push(handle);
                prev_was_handle = true;
            }
        }
    }
    debug_assert!(
        !prev_was_handle,
        "a resizable_handle needs a resizable_panel on both sides"
    );
    El::new(Kind::Custom("resizable-panel-group"))
        .at_loc(caller)
        .key(group_key)
        .axis(axis)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .children(els)
}

/// The `withHandle` chip: a small rounded lozenge in the handle's
/// `BORDER` color riding the seam, three grip dots inside. Sized to
/// the handle's own footprint so it never widens the strip.
fn grip_chip(axis: Axis) -> El {
    let dot = || {
        El::new(Kind::Custom("resizable-grip-dot"))
            .width(Size::Fixed(GRIP_DOT))
            .height(Size::Fixed(GRIP_DOT))
            .radius(GRIP_DOT * 0.5)
            .fill(tokens::MUTED_FOREGROUND)
            .state_follows_interactive_ancestor()
    };
    let (width, height, chip_axis) = match axis {
        Axis::Row => (HANDLE_THICKNESS, GRIP_CHIP_LENGTH, Axis::Column),
        Axis::Column | Axis::Overlay => (GRIP_CHIP_LENGTH, HANDLE_THICKNESS, Axis::Row),
    };
    El::new(Kind::Custom("resizable-grip"))
        .axis(chip_axis)
        .width(Size::Fixed(width))
        .height(Size::Fixed(height))
        .radius(tokens::RADIUS_SM)
        .fill(tokens::BORDER)
        .align(Align::Center)
        .justify(Justify::Center)
        .gap(GRIP_DOT)
        .children([dot(), dot(), dot()])
        // Like the hairline underneath: hit-test lands on the handle
        // wrapper, so the chip only lightens on hover / darkens under
        // drag via the cascade.
        .state_follows_interactive_ancestor()
}

/// Drag-anchor state for [`apply_event`]. Lives in the app struct
/// alongside the sizes; default-init it and pass `&mut`.
///
/// `anchor` is the pointer position along the group's axis at
/// PointerDown; `handle` is which handle grabbed it; `initial` is the
/// two adjacent fractions at that moment. Each Drag recomputes
/// absolute targets from the anchor, so long drags don't accumulate
/// float error (same contract as
/// [`crate::widgets::resize_handle::ResizeDrag`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct ResizableDrag {
    /// Pointer position along the group's axis at PointerDown; `None`
    /// when no drag is in progress.
    pub anchor: Option<f32>,
    /// Index of the handle being dragged.
    pub handle: usize,
    /// `[sizes[handle], sizes[handle + 1]]` at the moment the drag
    /// began.
    pub initial: [f32; 2],
}

/// Configuration for [`apply_event`].
///
/// Default: `min_fraction = 0.1` — no panel drags below 10% of the
/// sizes' sum, react-resizable-panels' default `minSize`.
#[derive(Clone, Copy, Debug)]
pub struct ResizableOpts {
    /// Lower bound for each of the two panels a handle trades between,
    /// as a fraction of the *sum* of `sizes` (so it means "10% of the
    /// group" regardless of the app's weight units). Drags and key
    /// nudges clamp here; the neighbor absorbs whatever the dragged
    /// panel gives up.
    pub min_fraction: f32,
}

impl Default for ResizableOpts {
    fn default() -> Self {
        Self { min_fraction: 0.1 }
    }
}

impl ResizableOpts {
    /// Set the per-panel minimum (see [`ResizableOpts::min_fraction`]).
    pub fn min_fraction(mut self, f: f32) -> Self {
        self.min_fraction = f;
        self
    }
}

/// Project a 2D pointer position onto the group's axis.
fn project(pos: (f32, f32), axis: Axis) -> f32 {
    match axis {
        Axis::Row | Axis::Overlay => pos.0,
        Axis::Column => pos.1,
    }
}

/// Pixels of `group_extent` actually shared by the panels: the group's
/// main-axis extent minus one [`HANDLE_THICKNESS`] strip per boundary.
/// Assumes the strict interleave [`resizable_panel_group`] builds (one
/// handle per boundary); a group with handle-less boundaries converts
/// a drag `HANDLE_THICKNESS` px per missing handle too slow — sub-1%
/// on any real layout.
fn panel_extent(group_extent: f32, panel_count: usize) -> f32 {
    group_extent - HANDLE_THICKNESS * panel_count.saturating_sub(1) as f32
}

/// Fold a routed event into the app-owned panel fractions of the group
/// keyed `group_key`. Returns `true` when `sizes` changed.
///
/// Handles the full lifecycle on each `{group_key}:handle:{i}` key:
/// PointerDown anchors the drag, Drag trades size between `sizes[i]`
/// and `sizes[i + 1]` (the two panels adjacent to the handle — all
/// others untouched, the react-resizable-panels default), PointerUp
/// releases, and Arrow / PageUp / PageDown / Home / End on the focused
/// handle nudge by the same pixel steps as
/// [`crate::widgets::resize_handle::apply_event_fixed`]. Both panels
/// clamp at [`ResizableOpts::min_fraction`]; the pair's sum is
/// conserved.
///
/// `axis` and `group_key` must match the group's construction.
/// `group_extent` is the group's current main-axis extent in logical
/// pixels — width for [`Axis::Row`], height for [`Axis::Column`] —
/// captured from the layout the user is looking at via
/// [`crate::event::EventCx::rect_of_key`] on the group key (the group
/// element carries it). With it the helper converts pointer pixels to
/// fractions exactly, so the handle tracks the pointer 1:1.
pub fn apply_event(
    sizes: &mut [f32],
    drag: &mut ResizableDrag,
    event: &UiEvent,
    group_key: &str,
    axis: Axis,
    group_extent: f32,
    opts: ResizableOpts,
) -> bool {
    if sizes.len() < 2 {
        return false;
    }
    let prefix = format!("{group_key}:handle");
    let Some(handle) = event.route_index::<usize>(&prefix) else {
        return false;
    };
    if handle + 1 >= sizes.len() {
        // Stale event from a tree with more panels than the app now
        // owns — ignore rather than index out of bounds.
        return false;
    }
    match event.kind {
        UiEventKind::PointerDown => {
            if let Some(pos) = event.pointer {
                drag.anchor = Some(project(pos, axis));
                drag.handle = handle;
                drag.initial = [sizes[handle], sizes[handle + 1]];
            }
            false
        }
        UiEventKind::Drag => {
            let Some(anchor) = drag.anchor else {
                return false;
            };
            if drag.handle != handle {
                return false;
            }
            let Some(pos) = event.pointer else {
                return false;
            };
            let extent = panel_extent(group_extent, sizes.len());
            if extent <= 0.0 {
                return false;
            }
            let total: f32 = sizes.iter().sum();
            if total <= 0.0 {
                return false;
            }
            // Pointer pixels → fraction units: the whole `total` maps
            // onto the panels' shared extent.
            let delta = (project(pos, axis) - anchor) * (total / extent);
            let pair_total = drag.initial[0] + drag.initial[1];
            let lo = pair_min(opts, total, pair_total);
            let next_left = (drag.initial[0] + delta).clamp(lo, pair_total - lo);
            let next_right = pair_total - next_left;
            let changed = (next_left - sizes[handle]).abs() > f32::EPSILON
                || (next_right - sizes[handle + 1]).abs() > f32::EPSILON;
            sizes[handle] = next_left;
            sizes[handle + 1] = next_right;
            changed
        }
        UiEventKind::PointerUp => {
            drag.anchor = None;
            false
        }
        UiEventKind::KeyDown => apply_key(sizes, handle, event, axis, group_extent, opts),
        _ => false,
    }
}

/// The feasible per-panel floor for one adjacent pair: the configured
/// minimum, capped so two floors always fit inside the pair's sum.
fn pair_min(opts: ResizableOpts, total: f32, pair_total: f32) -> f32 {
    (opts.min_fraction.max(0.0) * total).min(pair_total * 0.5)
}

fn apply_key(
    sizes: &mut [f32],
    handle: usize,
    event: &UiEvent,
    axis: Axis,
    group_extent: f32,
    opts: ResizableOpts,
) -> bool {
    let Some(press) = event.key_press.as_ref() else {
        return false;
    };
    let extent = panel_extent(group_extent, sizes.len());
    if extent <= 0.0 {
        return false;
    }
    let total: f32 = sizes.iter().sum();
    if total <= 0.0 {
        return false;
    }
    let pair_total = sizes[handle] + sizes[handle + 1];
    let lo = pair_min(opts, total, pair_total);
    let step = KEYBOARD_STEP_PX * (total / extent);
    let page = KEYBOARD_PAGE_STEP_PX * (total / extent);
    let left = sizes[handle];
    // ArrowRight/Down move the handle in the +axis direction, growing
    // the panel before it — the `Side::Start` convention of
    // `resize_handle::apply_event_fixed`. `axis` is accepted (and the
    // vertical arrows resolved the same as horizontal) so the call
    // shape stays symmetric with the pointer path.
    let _ = axis;
    let next_left = match press.logical.named() {
        Some(NamedKey::ArrowRight | NamedKey::ArrowDown) => left + step,
        Some(NamedKey::ArrowLeft | NamedKey::ArrowUp) => left - step,
        Some(NamedKey::PageUp) => left + page,
        Some(NamedKey::PageDown) => left - page,
        Some(NamedKey::Home) => lo,
        Some(NamedKey::End) => pair_total - lo,
        _ => return false,
    }
    .clamp(lo, pair_total - lo);
    let next_right = pair_total - next_left;
    let changed = (next_left - sizes[handle]).abs() > f32::EPSILON
        || (next_right - sizes[handle + 1]).abs() > f32::EPSILON;
    sizes[handle] = next_left;
    sizes[handle + 1] = next_right;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, LogicalKey, PhysicalKey, UiTarget};
    use crate::hit_test::hit_test_target;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::widgets::button::button;
    use crate::widgets::text::text;

    fn pointer_event(kind: UiEventKind, key: &str, x: f32, y: f32) -> UiEvent {
        let click_count = match kind {
            UiEventKind::PointerDown | UiEventKind::PointerUp | UiEventKind::Click => 1,
            _ => 0,
        };
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}").into(),
                rect: Rect::new(0.0, 0.0, 8.0, 400.0),
                tooltip: None,
                scroll_offset_y: 0.0,
                content_inset: Sides::zero(),
            }),
            pointer: Some((x, y)),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count,
            pointer_kind: None,
            wheel_delta: None,
            kind,
        }
    }

    fn key_event(key: &str, ui_key: LogicalKey) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(key.to_string()),
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                logical: ui_key,
                physical: PhysicalKey::Unidentified,
                modifiers: KeyModifiers::default(),
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        }
    }

    fn three_panel_row() -> El {
        resizable_panel_group(
            Axis::Row,
            "g",
            [
                resizable_panel(0.25, text("rail")),
                resizable_handle(),
                resizable_panel(0.5, text("content")),
                resizable_handle_with_grip(),
                resizable_panel(0.25, text("inspector")),
            ],
        )
    }

    #[test]
    fn group_anatomy_interleaves_panels_and_keyed_handles() {
        let g = three_panel_row();
        assert_eq!(g.key.as_deref(), Some("g"), "group carries the group key");
        assert_eq!(g.axis, Axis::Row);
        assert_eq!(g.width, Size::Fill(1.0));
        assert_eq!(g.height, Size::Fill(1.0));
        assert_eq!(g.children.len(), 5);

        // Panels: main axis = Fill(fraction), cross axis = Fill(1.0),
        // clipping, unkeyed.
        for (i, fraction) in [(0usize, 0.25f32), (2, 0.5), (4, 0.25)] {
            let p = &g.children[i];
            assert_eq!(p.kind, Kind::Custom("resizable-panel"));
            assert_eq!(p.width, Size::Fill(fraction));
            assert_eq!(p.height, Size::Fill(1.0));
            assert!(p.clip, "panels clip squeezed content");
            assert!(p.key.is_none());
        }

        // Handles: derived keys, stock resize_handle anatomy, zeroed
        // hit_overflow (flush-neighbour idiom).
        let h0 = &g.children[1];
        let h1 = &g.children[3];
        assert_eq!(h0.key.as_deref(), Some("g:handle:0"));
        assert_eq!(h1.key.as_deref(), Some("g:handle:1"));
        for h in [h0, h1] {
            assert!(h.focusable);
            assert_eq!(h.cursor, Some(crate::cursor::Cursor::EwResize));
            assert_eq!(h.width, Size::Fixed(HANDLE_THICKNESS));
            assert_eq!(h.height, Size::Fill(1.0));
            assert_eq!(
                h.hit_overflow,
                Sides::zero(),
                "flush handles must not invade panel content"
            );
        }

        // Grip variant: hairline plus the chip; plain variant: hairline
        // only.
        assert_eq!(h0.children.len(), 1);
        assert_eq!(h1.children.len(), 2);
        let chip = &h1.children[1];
        assert_eq!(chip.kind, Kind::Custom("resizable-grip"));
        assert_eq!(chip.width, Size::Fixed(HANDLE_THICKNESS));
        assert_eq!(chip.children.len(), 3, "three grip dots");
    }

    #[test]
    fn column_group_maps_sizes_to_the_cross_pair() {
        let g = resizable_panel_group(
            Axis::Column,
            "v",
            [
                resizable_panel(0.7, text("top")),
                resizable_handle(),
                resizable_panel(0.3, text("bottom")),
            ],
        );
        assert_eq!(g.axis, Axis::Column);
        let top = &g.children[0];
        assert_eq!(top.width, Size::Fill(1.0));
        assert_eq!(top.height, Size::Fill(0.7));
        let h = &g.children[1];
        assert_eq!(h.key.as_deref(), Some("v:handle:0"));
        assert_eq!(h.cursor, Some(crate::cursor::Cursor::NsResize));
        assert_eq!(h.width, Size::Fill(1.0));
        assert_eq!(h.height, Size::Fixed(HANDLE_THICKNESS));
    }

    #[test]
    fn handle_key_matches_widget_format() {
        assert_eq!(resizable_handle_key("g", 0), "g:handle:0");
        assert_eq!(resizable_handle_key("card:7", 2), "card:7:handle:2");
    }

    #[test]
    fn drag_trades_between_neighbors_and_leaves_others_untouched() {
        // Group 816px wide, 3 panels + 2 handles → panels share 800px.
        // sizes sum 1.0, so 100px of drag = 0.125 of fraction.
        let mut sizes = vec![0.25, 0.5, 0.25];
        let mut drag = ResizableDrag::default();
        let opts = ResizableOpts::default();

        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "g:handle:0", 204.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        assert_eq!(drag.anchor, Some(204.0));
        assert_eq!(drag.handle, 0);
        assert_eq!(drag.initial, [0.25, 0.5]);

        let changed = apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "g:handle:0", 304.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        assert!(changed);
        assert!((sizes[0] - 0.375).abs() < 1e-5, "left grew: {sizes:?}");
        assert!((sizes[1] - 0.375).abs() < 1e-5, "right absorbed: {sizes:?}");
        assert_eq!(sizes[2], 0.25, "non-adjacent panel untouched");
        assert!(
            (sizes.iter().sum::<f32>() - 1.0).abs() < 1e-5,
            "total conserved"
        );

        // Second drag is absolute from the anchor — no accumulation.
        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "g:handle:0", 244.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        assert!((sizes[0] - 0.3).abs() < 1e-5, "absolute, not drifted");

        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::PointerUp, "g:handle:0", 244.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        assert_eq!(drag.anchor, None, "anchor cleared on PointerUp");
    }

    #[test]
    fn drag_clamps_both_panels_at_min_fraction() {
        let mut sizes = vec![0.25, 0.5, 0.25];
        let mut drag = ResizableDrag::default();
        let opts = ResizableOpts::default().min_fraction(0.1);

        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "g:handle:0", 204.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        // Way past the left edge: the left panel floors at 0.1, the
        // right absorbs everything it gave up.
        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "g:handle:0", -10_000.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        assert!((sizes[0] - 0.1).abs() < 1e-5, "{sizes:?}");
        assert!((sizes[1] - 0.65).abs() < 1e-5, "{sizes:?}");
        assert_eq!(sizes[2], 0.25);

        // Way past the right edge: the *right* panel floors at 0.1.
        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "g:handle:0", 10_000.0, 200.0),
            "g",
            Axis::Row,
            816.0,
            opts,
        );
        assert!((sizes[0] - 0.65).abs() < 1e-5, "{sizes:?}");
        assert!((sizes[1] - 0.1).abs() < 1e-5, "{sizes:?}");
    }

    #[test]
    fn events_for_other_keys_or_stale_handles_are_ignored() {
        let mut sizes = vec![0.5, 0.5];
        let mut drag = ResizableDrag::default();
        let opts = ResizableOpts::default();

        // Different widget entirely.
        assert!(!apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "other", 100.0, 100.0),
            "g",
            Axis::Row,
            800.0,
            opts,
        ));
        assert_eq!(drag.anchor, None);

        // A handle index the current sizes can't satisfy (stale tree).
        assert!(!apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "g:handle:1", 100.0, 100.0),
            "g",
            Axis::Row,
            800.0,
            opts,
        ));
        assert_eq!(drag.anchor, None);
        assert_eq!(sizes, vec![0.5, 0.5]);

        // A Drag routed to a different handle than the anchored one.
        apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "g:handle:0", 100.0, 100.0),
            "g",
            Axis::Row,
            800.0,
            opts,
        );
        drag.handle = 7; // simulate a mismatched capture
        assert!(!apply_event(
            &mut sizes,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "g:handle:0", 300.0, 100.0),
            "g",
            Axis::Row,
            800.0,
            opts,
        ));
        assert_eq!(sizes, vec![0.5, 0.5]);
    }

    #[test]
    fn arrow_keys_nudge_the_pair_within_bounds() {
        // 2 panels + 1 handle in 808px → panels share 800px; sum 1.0,
        // so one KEYBOARD_STEP_PX (8px) = 0.01 of fraction.
        let mut sizes = vec![0.5, 0.5];
        let mut drag = ResizableDrag::default();
        let opts = ResizableOpts::default();

        assert!(apply_event(
            &mut sizes,
            &mut drag,
            &key_event("g:handle:0", LogicalKey::Named(NamedKey::ArrowRight)),
            "g",
            Axis::Row,
            808.0,
            opts,
        ));
        assert!((sizes[0] - 0.51).abs() < 1e-5, "{sizes:?}");
        assert!((sizes[1] - 0.49).abs() < 1e-5, "{sizes:?}");

        // Home floors the left panel at min_fraction.
        assert!(apply_event(
            &mut sizes,
            &mut drag,
            &key_event("g:handle:0", LogicalKey::Named(NamedKey::Home)),
            "g",
            Axis::Row,
            808.0,
            opts,
        ));
        assert!((sizes[0] - 0.1).abs() < 1e-5, "{sizes:?}");
        assert!((sizes[1] - 0.9).abs() < 1e-5, "{sizes:?}");

        // ArrowLeft at the floor is a no-op.
        assert!(!apply_event(
            &mut sizes,
            &mut drag,
            &key_event("g:handle:0", LogicalKey::Named(NamedKey::ArrowLeft)),
            "g",
            Axis::Row,
            808.0,
            opts,
        ));
        assert!((sizes[0] - 0.1).abs() < 1e-5);

        // End maxes the left panel (right panel at its floor).
        assert!(apply_event(
            &mut sizes,
            &mut drag,
            &key_event("g:handle:0", LogicalKey::Named(NamedKey::End)),
            "g",
            Axis::Row,
            808.0,
            opts,
        ));
        assert!((sizes[0] - 0.9).abs() < 1e-5, "{sizes:?}");
        assert!((sizes[1] - 0.1).abs() < 1e-5, "{sizes:?}");
    }

    #[test]
    fn hit_test_routes_the_seam_to_the_handle_and_drags_track_the_pointer() {
        // End-to-end: lay the group out, hit-test the seam, feed the
        // resulting target through apply_event, re-lay out, and check
        // the handle landed under the pointer.
        let mut sizes = vec![0.25, 0.5, 0.25];
        let mut drag = ResizableDrag::default();
        let opts = ResizableOpts::default();
        let viewport = Rect::new(0.0, 0.0, 816.0, 400.0);

        let mut tree = resizable_panel_group(
            Axis::Row,
            "g",
            [
                resizable_panel(sizes[0], text("rail")),
                resizable_handle(),
                resizable_panel(sizes[1], text("content")),
                resizable_handle(),
                resizable_panel(sizes[2], text("inspector")),
            ],
        );
        let mut state = UiState::new();
        layout(&mut tree, &mut state, viewport);

        // Panels share 800px → panel 0 is 200px; handle 0 spans
        // 200..208.
        let target = hit_test_target(&tree, &state, (204.0, 200.0))
            .expect("the seam should hit-test to the handle");
        assert_eq!(target.key, "g:handle:0");

        let group_extent = state
            .rect_of_key("g")
            .expect("the group element carries the group key")
            .w;
        assert_eq!(group_extent, 816.0);

        let mut down = pointer_event(UiEventKind::PointerDown, "g:handle:0", 204.0, 200.0);
        down.target = Some(target.clone());
        apply_event(
            &mut sizes, &mut drag, &down, "g", Axis::Row, group_extent, opts,
        );
        let mut dragged = pointer_event(UiEventKind::Drag, "g:handle:0", 304.0, 200.0);
        dragged.target = Some(target);
        assert!(apply_event(
            &mut sizes,
            &mut drag,
            &dragged,
            "g",
            Axis::Row,
            group_extent,
            opts,
        ));

        // Rebuild with the new sizes: the handle's center must sit at
        // the pointer's x — the grab point tracks 1:1 because the
        // px→fraction conversion excludes the handles' own footprints.
        let mut tree = resizable_panel_group(
            Axis::Row,
            "g",
            [
                resizable_panel(sizes[0], text("rail")),
                resizable_handle(),
                resizable_panel(sizes[1], text("content")),
                resizable_handle(),
                resizable_panel(sizes[2], text("inspector")),
            ],
        );
        let mut state = UiState::new();
        layout(&mut tree, &mut state, viewport);
        let handle_rect = state
            .rect_of_key("g:handle:0")
            .expect("handle re-laid out");
        let center = handle_rect.x + handle_rect.w / 2.0;
        assert!(
            (center - 304.0).abs() < 0.5,
            "handle center {center} should track the pointer at 304"
        );
    }

    #[test]
    fn group_with_interactive_panel_content_is_lint_clean() {
        use crate::bundle::lint::FindingKind;

        let panel_content = |label: &str, key: &str| {
            crate::tree::column([button(label).key(key)])
                .padding(Sides::all(tokens::SPACE_2))
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0))
        };
        let mut root = resizable_panel_group(
            Axis::Row,
            "g",
            [
                resizable_panel(0.25, panel_content("Rail", "rail-action")),
                resizable_handle(),
                resizable_panel(0.5, panel_content("Content", "content-action")),
                resizable_handle_with_grip(),
                resizable_panel(0.25, panel_content("Inspect", "inspect-action")),
            ],
        );
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 900.0, 400.0));
        let report = crate::bundle::lint::lint(&root, &state, &crate::theme::Theme::default());
        let relevant: Vec<_> = report
            .findings
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FindingKind::HitOverflowCollision | FindingKind::FocusRingObscured
                )
            })
            .collect();
        assert!(
            relevant.is_empty(),
            "flush handles must not collide with panel content:\n{}",
            report.text(),
        );
    }
}
