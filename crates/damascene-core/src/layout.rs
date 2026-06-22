//! Flex-style layout pass over the [`El`] tree.
//!
//! Sizing per axis:
//! - `Fixed(px)` — exact size on its axis.
//! - `Hug` — intrinsic size (text width, sum of children, etc.). Default.
//! - `Fill(weight)` — share leftover main-axis space proportionally.
//!
//! Defaults match CSS flex's `flex: 0 1 auto`: children content-size
//! on the main axis, defer to the parent's [`Align`] on the cross
//! axis. `Align::Stretch` (the column / scroll default) stretches both
//! `Hug` and `Fill` children to the container's full cross extent —
//! the analog of CSS `align-items: stretch`. `Align::Center | Start |
//! End` shrinks them to intrinsic so the alignment can actually
//! position them — matching CSS's behavior when align-items is
//! non-stretch. Main-axis distribution is governed by [`Justify`] (or
//! insert a [`spacer`]).
//!
//! The layout pass also assigns each node a stable path-based
//! [`El::computed_id`]: `root.0.card[account].2.button` — a node's ID is
//! parent-id + dot + role-or-key + sibling-index. IDs survive minor
//! refactors and are usable as patch / lint / draw-op targets.
//!
//! Rects do not live on `El` — the layout pass writes them to
//! `UiState`'s computed-rect side map, keyed by `computed_id`. The
//! container rect flows down the recursion as a parameter; child rects
//! are computed per-axis and inserted into the side map. Scroll offsets
//! likewise read/write `UiState`'s scroll-offset side map directly.
//!
//! Text intrinsic measurement uses bundled-font glyph advances via
//! [`crate::text::metrics`]. Full shaping still belongs to the renderer
//! for now; this keeps layout/lint/SVG close enough to glyphon output
//! without committing to the final text stack.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::scroll::{ScrollAlignment, ScrollRequest};
use crate::state::dyn_height::DynHeightIndex;
use crate::state::{ScrollAnchor, UiState, VirtualAnchor};
use crate::text::metrics as text_metrics;
use crate::tree::*;

/// Second escape hatch: author-supplied layout function.
///
/// When set on a node via [`El::layout`], the layout pass calls this
/// function instead of running the column/row/overlay distribution for
/// that node's direct children. The function returns one [`Rect`] per
/// child (in source order), positioned anywhere inside the container.
/// The library still recurses into each child (so descendants lay out
/// normally) and still drives hit-test, focus, animation, scroll —
/// those all read from `UiState`'s computed-rect side map, which receives the
/// rects this function produces.
///
/// Authors typically write a free `fn(LayoutCtx) -> Vec<Rect>` and
/// pass it directly: `column(children).layout(my_layout)`.
///
/// ## What you get
///
/// - [`LayoutCtx::container`] — the rect available for placement
///   (parent rect minus this node's padding).
/// - [`LayoutCtx::children`] — read-only slice of the node's children;
///   index here matches the index in your returned `Vec<Rect>`.
/// - [`LayoutCtx::measure`] — call to get a child's intrinsic
///   `(width, height)` if you need it for sizing decisions.
///
/// ## Scope limits (will panic)
///
/// - The custom-layout node itself must size with [`Size::Fixed`] or
///   [`Size::Fill`] on both axes. `Size::Hug` would require a separate
///   intrinsic callback and is not yet supported.
/// - The returned `Vec<Rect>` length must equal `children.len()`.
#[derive(Clone)]
pub struct LayoutFn(pub Arc<dyn Fn(LayoutCtx) -> Vec<Rect> + Send + Sync>);

impl LayoutFn {
    /// Wrap a closure as a [`LayoutFn`]. Equivalent to passing a free
    /// `fn` directly to [`El::layout`].
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(LayoutCtx) -> Vec<Rect> + Send + Sync + 'static,
    {
        LayoutFn(Arc::new(f))
    }
}

impl std::fmt::Debug for LayoutFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LayoutFn(<fn>)")
    }
}

/// Hit / miss counters for the per-pass intrinsic-measurement cache.
/// Recorded during each layout pass; retrieve the latest pass's numbers
/// with [`take_intrinsic_cache_stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutIntrinsicCacheStats {
    /// Intrinsic measurements served from the cache.
    pub hits: u64,
    /// Intrinsic measurements computed fresh.
    pub misses: u64,
}

/// Counters for the scroll-layout prune optimization, which skips
/// recursing into scroll-container children that lie far outside the
/// visible range. Recorded during each layout pass; retrieve the latest
/// pass's numbers with [`take_prune_stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutPruneStats {
    /// Off-screen child subtrees whose recursion was skipped.
    pub subtrees: u64,
    /// Total descendant nodes inside those skipped subtrees.
    pub nodes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct IntrinsicCacheKey {
    computed_id: String,
    available_width_bits: Option<u32>,
}

#[derive(Default)]
struct IntrinsicCache {
    measurements: FxHashMap<IntrinsicCacheKey, (f32, f32)>,
    stats: LayoutIntrinsicCacheStats,
    prune: LayoutPruneStats,
}

thread_local! {
    static INTRINSIC_CACHE: RefCell<Option<IntrinsicCache>> = const { RefCell::new(None) };
    static LAST_INTRINSIC_CACHE_STATS: RefCell<LayoutIntrinsicCacheStats> =
        const { RefCell::new(LayoutIntrinsicCacheStats { hits: 0, misses: 0 }) };
    static LAST_PRUNE_STATS: RefCell<LayoutPruneStats> =
        const { RefCell::new(LayoutPruneStats { subtrees: 0, nodes: 0 }) };
}

struct IntrinsicCacheGuard {
    previous: Option<IntrinsicCache>,
}

impl Drop for IntrinsicCacheGuard {
    fn drop(&mut self) {
        INTRINSIC_CACHE.with(|cell| {
            cell.replace(self.previous.take());
        });
    }
}

fn with_intrinsic_cache(f: impl FnOnce()) {
    let previous = INTRINSIC_CACHE.with(|cell| cell.replace(Some(IntrinsicCache::default())));
    let mut guard = IntrinsicCacheGuard { previous };
    f();
    let finished = INTRINSIC_CACHE.with(|cell| cell.replace(guard.previous.take()));
    if let Some(cache) = finished {
        LAST_INTRINSIC_CACHE_STATS.with(|stats| {
            *stats.borrow_mut() = cache.stats;
        });
        LAST_PRUNE_STATS.with(|stats| {
            *stats.borrow_mut() = cache.prune;
        });
    }
    std::mem::forget(guard);
}

/// Take (and reset) the intrinsic-cache stats recorded by this thread's
/// most recent layout pass. The runtime drains this once per frame into
/// its timing diagnostics.
pub fn take_intrinsic_cache_stats() -> LayoutIntrinsicCacheStats {
    LAST_INTRINSIC_CACHE_STATS.with(|stats| std::mem::take(&mut *stats.borrow_mut()))
}

/// Take (and reset) the scroll-prune stats recorded by this thread's
/// most recent layout pass. The runtime drains this once per frame into
/// its timing diagnostics.
pub fn take_prune_stats() -> LayoutPruneStats {
    LAST_PRUNE_STATS.with(|stats| std::mem::take(&mut *stats.borrow_mut()))
}

/// Virtualized list state attached to a [`Kind::VirtualList`] node.
/// Holds the row count, the row-height policy, and the closure that
/// realizes a row by global index. Set via [`crate::virtual_list`] or
/// [`crate::virtual_list_dyn`]; the layout pass calls `build_row(i)`
/// only for indices whose rect intersects the viewport.
///
/// ## Row-height policies
///
/// - [`VirtualMode::Fixed`] — every row is the same logical-pixel
///   height. Scroll → visible-range is O(1).
/// - [`VirtualMode::Dynamic`] — rows vary in height. The library uses
///   `estimated_row_height` as a placeholder for unmeasured rows,
///   measures visible rows at the current layout width, and preserves a
///   row anchor on screen while estimates become measurements.
///
/// ## Other current scope
///
/// - **Vertical only** — feed/chat-log-shaped lists are the target.
///   A horizontal variant can come later.
/// - **No row pooling** — visible rows are rebuilt from scratch each
///   layout pass. Fine for thousands of items; if it bottlenecks we
///   add a pool keyed by stable row keys.
#[derive(Clone, Debug)]
pub enum VirtualMode {
    /// Every row is exactly `row_height` logical pixels tall.
    Fixed {
        /// Uniform row height in logical pixels. Must be > 0.
        row_height: f32,
    },
    /// Rows have variable heights. `estimated_row_height` seeds the
    /// content-height total and the visible-range walk for rows that
    /// haven't been measured yet.
    Dynamic {
        /// Placeholder height (logical pixels) for not-yet-measured
        /// rows. Must be > 0.
        estimated_row_height: f32,
        /// Opt-in contract: across frames the row sequence changes
        /// **only** by appending rows at the tail and/or dropping a
        /// contiguous prefix at the head (an append-at-bottom feed or a
        /// capped ring buffer). When set, layout maintains an
        /// incremental per-row height index so the per-frame cost is
        /// O(visible) instead of several O(n) walks — the difference
        /// that lets a 100k-row chat log stay inside frame budget while
        /// scrolling (issue #107). Reordering, mid-list insertion, or
        /// re-keying a surviving row violates the contract: debug builds
        /// assert, release self-heals with a one-frame O(n) rebuild.
        /// Set via [`VirtualItems::append_only`] /
        /// [`crate::tree::El::append_only`].
        append_only: bool,
    },
}

/// Policy used to pick the next dynamic virtual-list anchor after each
/// layout pass. The previous anchor solves the current frame; this
/// policy rebases the next frame onto a coherent in-viewport row point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VirtualAnchorPolicy {
    /// Pick the row point nearest `y_fraction` through the viewport.
    /// `0.0` is the top, `1.0` is the bottom. Good default for feeds.
    ViewportFraction {
        /// Fraction of the viewport height (clamped to `0.0..=1.0`)
        /// where the anchor point is sampled.
        y_fraction: f32,
    },
    /// Prefer the first fully visible row; fall back to the first
    /// partially visible row.
    FirstVisible,
    /// Prefer the last fully visible row; fall back to the last
    /// partially visible row.
    LastVisible,
}

impl Default for VirtualAnchorPolicy {
    fn default() -> Self {
        Self::ViewportFraction { y_fraction: 0.25 }
    }
}

/// Virtualized list state attached to a [`Kind::VirtualList`] node:
/// row count, row-height policy, and the closures that identify and
/// realize rows. Built via [`Self::new`] / [`Self::new_dyn`] or,
/// more commonly, the [`crate::virtual_list`] /
/// [`crate::virtual_list_dyn`] builders. See [`VirtualMode`] for the
/// scope and policy discussion.
#[derive(Clone)]
#[non_exhaustive]
pub struct VirtualItems {
    /// Total number of rows in the list (realized or not).
    pub count: usize,
    /// Row-height policy — see [`VirtualMode`].
    pub mode: VirtualMode,
    /// How the next frame's scroll anchor is chosen after each layout
    /// pass (dynamic mode only).
    pub anchor_policy: VirtualAnchorPolicy,
    /// Stable identity for a row by global index. Keys measured heights
    /// and [`ScrollRequest::ToRowKey`] targets across reorders /
    /// insertions; defaults to the index itself in fixed mode.
    pub row_key: Arc<dyn Fn(usize) -> String + Send + Sync>,
    /// Realize the row at a global index. Called only for rows whose
    /// rect intersects the viewport.
    pub build_row: Arc<dyn Fn(usize) -> El + Send + Sync>,
}

impl VirtualItems {
    /// Fixed-height list: every row is `row_height` logical pixels.
    /// Row keys default to the stringified index. Panics if
    /// `row_height <= 0.0`. Prefer the [`crate::virtual_list`] builder.
    pub fn new<F>(count: usize, row_height: f32, build_row: F) -> Self
    where
        F: Fn(usize) -> El + Send + Sync + 'static,
    {
        assert!(
            row_height > 0.0,
            "VirtualItems::new requires row_height > 0.0 (got {row_height})"
        );
        VirtualItems {
            count,
            mode: VirtualMode::Fixed { row_height },
            anchor_policy: VirtualAnchorPolicy::default(),
            row_key: Arc::new(|i| i.to_string()),
            build_row: Arc::new(build_row),
        }
    }

    /// Variable-height list: rows start at `estimated_row_height`
    /// logical pixels and are replaced by real measurements as they
    /// become visible. `row_key` must give each row a stable identity.
    /// Panics if `estimated_row_height <= 0.0`. Prefer the
    /// [`crate::virtual_list_dyn`] builder.
    pub fn new_dyn<K, F>(count: usize, estimated_row_height: f32, row_key: K, build_row: F) -> Self
    where
        K: Fn(usize) -> String + Send + Sync + 'static,
        F: Fn(usize) -> El + Send + Sync + 'static,
    {
        assert!(
            estimated_row_height > 0.0,
            "VirtualItems::new_dyn requires estimated_row_height > 0.0 (got {estimated_row_height})"
        );
        VirtualItems {
            count,
            mode: VirtualMode::Dynamic {
                estimated_row_height,
                append_only: false,
            },
            anchor_policy: VirtualAnchorPolicy::default(),
            row_key: Arc::new(row_key),
            build_row: Arc::new(build_row),
        }
    }

    /// Declare the append-only contract on a dynamic list — see
    /// [`VirtualMode::Dynamic::append_only`]. No-op (and harmless) on a
    /// fixed list, whose range math is already O(1).
    pub fn append_only(mut self) -> Self {
        if let VirtualMode::Dynamic {
            ref mut append_only,
            ..
        } = self.mode
        {
            *append_only = true;
        }
        self
    }

    /// Replace the default anchor policy
    /// ([`VirtualAnchorPolicy::ViewportFraction`] at 0.25).
    pub fn anchor_policy(mut self, policy: VirtualAnchorPolicy) -> Self {
        self.anchor_policy = policy;
        self
    }
}

impl std::fmt::Debug for VirtualItems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualItems")
            .field("count", &self.count)
            .field("mode", &self.mode)
            .field("anchor_policy", &self.anchor_policy)
            .field("row_key", &"<fn>")
            .field("build_row", &"<fn>")
            .finish()
    }
}

/// Context handed to a [`LayoutFn`]. Marked `#[non_exhaustive]` so
/// future fields (intrinsic-at-width, scroll context, …) can be added
/// without breaking author code that currently reads `container` /
/// `children` / `measure`.
#[non_exhaustive]
pub struct LayoutCtx<'a> {
    /// Inner rect of the parent (after padding) — the area available
    /// for child placement. Children may be positioned anywhere; the
    /// library does not clamp returned rects to this region.
    pub container: Rect,
    /// Direct children of the node, in source order. Read-only — return
    /// positions through your `Vec<Rect>`.
    pub children: &'a [El],
    /// Intrinsic `(width, height)` for any child. Wrapped text returns
    /// its unwrapped width here; if you need width-dependent wrapping
    /// you'll need to size the child with `Fixed` / `Fill` instead.
    pub measure: &'a dyn Fn(&El) -> (f32, f32),
    /// Look up any keyed node's laid-out rect. Returns `None` when the
    /// key is absent from the tree, when the node hasn't been laid out
    /// yet (siblings later in source order), or when the key was used
    /// on a node without a recorded rect. Used by widgets like
    /// [`crate::widgets::popover::popover`] to position children
    /// relative to elements outside their own subtree.
    pub rect_of_key: &'a dyn Fn(&str) -> Option<Rect>,
    /// Look up a node's laid-out rect by its `computed_id`. Same
    /// semantics as [`Self::rect_of_key`] but skips the `key →
    /// computed_id` translation — useful for runtime-synthesized
    /// layers (tooltips, focus rings) that anchor to a node the
    /// library already knows by id.
    pub rect_of_id: &'a dyn Fn(&str) -> Option<Rect>,
}

/// Lay out the whole tree into the given viewport rect. Assigns
/// `computed_id`s, rebuilds the key index, and runs the layout walk.
///
/// Hosts that drive their own pipeline (the Damascene runtime does this in
/// [`crate::runtime::RunnerCore::prepare_layout`]) typically call
/// [`assign_ids`] before synthesizing floating layers (tooltips,
/// toasts), then route the laid-out call through
/// [`layout_post_assign`] so the id walk doesn't run twice per frame.
pub fn layout(root: &mut El, ui_state: &mut UiState, viewport: Rect) {
    {
        crate::profile_span!("layout::assign_ids");
        assign_id(root, "root");
    }
    layout_post_assign(root, ui_state, viewport);
}

/// Like [`layout`], but skips the recursive `assign_id` walk. Callers
/// are responsible for ensuring every node's `computed_id` is already
/// set — typically by invoking [`assign_ids`] earlier in the pipeline,
/// then having any per-frame floating-layer synthesis pass call
/// [`assign_id_appended`] on its newly pushed layer.
pub fn layout_post_assign(root: &mut El, ui_state: &mut UiState, viewport: Rect) {
    with_intrinsic_cache(|| {
        {
            crate::profile_span!("layout::root_setup");
            ui_state
                .layout
                .computed_rects
                .insert(root.computed_id.clone(), viewport);
            rebuild_key_index(root, ui_state);
            // Per-scrollable scratch is rebuilt every layout — entries for
            // scrollables that disappeared mid-frame must not leave stale
            // thumb rects behind for hit-test or paint to find.
            ui_state.scroll.metrics.clear();
            ui_state.scroll.thumb_rects.clear();
            ui_state.scroll.thumb_tracks.clear();
            ui_state.scroll.visible_ranges.clear();
            ui_state.resize.bands.clear();
            // `.user_resizable()` panes with a user-dragged size get
            // their declared size rewritten before the layout walk so
            // every sizing path (measure, fill redistribution) sees
            // the override as an ordinary `Fixed`.
            apply_resize_overrides(root, ui_state);
        }
        crate::profile_span!("layout::children");
        layout_children(root, viewport, ui_state);
        // Band geometry needs final rects, so it runs as a post-pass.
        publish_resize_bands(root, ui_state);
    });
}

/// Drag clamp for a `.user_resizable()` pane along its parent's
/// `axis`, from the pane's CSS-shaped min/max fields. Missing bounds
/// fall back to `[0, ∞)`; a min above max resolves to the lower bound,
/// matching the documented `min_width`/`max_width` conflict rule.
fn resize_clamp(child: &El, axis: Axis) -> (f32, f32) {
    let (min, max) = match axis {
        Axis::Column => (child.min_height, child.max_height),
        _ => (child.min_width, child.max_width),
    };
    let min = min.unwrap_or(0.0).max(0.0);
    (min, max.unwrap_or(f32::INFINITY).max(min))
}

/// Pre-pass for `.user_resizable()` panes (issue #106): where the user
/// has dragged a size, rewrite the pane's declared main-axis size to
/// `Fixed(override)` — clamped to the pane's min/max — before the
/// layout walk. The declared size remains the default until the first
/// drag writes an override.
fn apply_resize_overrides(node: &mut El, ui_state: &UiState) {
    let axis = node.axis;
    for child in &mut node.children {
        if child.user_resizable && !matches!(axis, Axis::Overlay) {
            let id = child.key.as_deref().unwrap_or(&child.computed_id);
            if let Some(&px) = ui_state.resize.overrides.get(id) {
                let (min, max) = resize_clamp(child, axis);
                let px = px.clamp(min, max);
                match axis {
                    Axis::Column => child.height = Size::Fixed(px),
                    _ => child.width = Size::Fixed(px),
                }
            }
        }
        apply_resize_overrides(child, ui_state);
    }
}

/// Post-pass for `.user_resizable()` panes: publish each pane's grab
/// band — an invisible [`crate::state::resize::RESIZE_BAND_THICKNESS`]
/// strip straddling the pane's seam edge — into
/// `ui_state.resize.bands` for the runtime's pointer pre-emption and
/// cursor resolution. The band sits on the trailing edge along the
/// parent's axis when a sibling follows, and on the leading edge when
/// the pane is the last child (a right- or bottom-anchored pane).
fn publish_resize_bands(node: &El, ui_state: &mut UiState) {
    use crate::state::resize::{RESIZE_BAND_THICKNESS as T, ResizeBand};
    let axis = node.axis;
    if !matches!(axis, Axis::Overlay) && node.children.iter().any(|c| c.user_resizable) {
        let parent_rect = ui_state
            .layout
            .computed_rects
            .get(&node.computed_id)
            .copied();
        if let Some(parent_rect) = parent_rect {
            let inner_main = match axis {
                Axis::Column => parent_rect.h - node.padding.top - node.padding.bottom,
                _ => parent_rect.w - node.padding.left - node.padding.right,
            };
            let count = node.children.len();
            for (idx, child) in node.children.iter().enumerate() {
                if !child.user_resizable {
                    continue;
                }
                let Some(rect) = ui_state
                    .layout
                    .computed_rects
                    .get(&child.computed_id)
                    .copied()
                else {
                    continue;
                };
                // Trailing edge when a sibling follows (or the pane is
                // alone); leading edge for a last child with siblings
                // before it.
                let trailing = idx + 1 < count || count == 1;
                let band = match (axis, trailing) {
                    (Axis::Column, true) => Rect::new(rect.x, rect.y + rect.h - T / 2.0, rect.w, T),
                    (Axis::Column, false) => Rect::new(rect.x, rect.y - T / 2.0, rect.w, T),
                    (_, true) => Rect::new(rect.x + rect.w - T / 2.0, rect.y, T, rect.h),
                    (_, false) => Rect::new(rect.x - T / 2.0, rect.y, T, rect.h),
                };
                let (min, max) = resize_clamp(child, axis);
                // The seam must stay inside the parent: cap the drag at
                // the parent's inner extent so the band can't be pushed
                // out of reach.
                let max = max.min(inner_main.max(min));
                ui_state.resize.bands.push(ResizeBand {
                    id: child
                        .key
                        .clone()
                        .unwrap_or_else(|| child.computed_id.clone()),
                    key: child.key.clone(),
                    container_id: node.computed_id.clone(),
                    band,
                    axis,
                    sign: if trailing { 1.0 } else { -1.0 },
                    current: match axis {
                        Axis::Column => rect.h,
                        _ => rect.w,
                    },
                    min,
                    max,
                });
            }
        }
    }
    for child in &node.children {
        publish_resize_bands(child, ui_state);
    }
}

/// Assign `computed_id`s to a child that was just appended to an
/// already-id-assigned `parent`. Companion to [`layout_post_assign`]:
/// floating-layer synthesis (tooltip, toast) pushes one new child onto
/// the root and uses this to give the new subtree the same path-style
/// ids the recursive `assign_id` would have, without re-walking the
/// rest of the tree.
pub fn assign_id_appended(parent_id: &str, child: &mut El, child_index: usize) {
    let role = role_token(&child.kind);
    let suffix = match (&child.key, role) {
        (Some(k), r) => format!("{r}[{k}]"),
        (None, r) => format!("{r}.{child_index}"),
    };
    assign_id(child, &format!("{parent_id}.{suffix}"));
}

/// Walk the tree once and refresh `ui_state.layout.key_index` so
/// `LayoutCtx::rect_of_key` can resolve `key → computed_id` without
/// re-scanning the tree per lookup. First key wins — duplicate keys
/// are an author bug, but we don't want to crash layout over it.
fn rebuild_key_index(root: &El, ui_state: &mut UiState) {
    ui_state.layout.key_index.clear();
    // Count computed_ids in the same walk so the duplicate-id check
    // (issue #64) costs no extra traversal. Two same-role siblings
    // sharing a key collide on computed_id (`assign_id` produces
    // `parent.role[key]` for both) — the second then overwrites the
    // first's `computed_rects` entry (both hit-test/paint with one
    // rect) and the intrinsic cache returns one sibling's measurement
    // for the other. The failure is silent and non-local.
    let mut id_counts: rustc_hash::FxHashMap<&str, u32> = Default::default();
    fn visit<'a>(
        node: &'a El,
        index: &mut rustc_hash::FxHashMap<String, String>,
        id_counts: &mut rustc_hash::FxHashMap<&'a str, u32>,
    ) {
        if let Some(key) = &node.key {
            index
                .entry(key.clone())
                .or_insert_with(|| node.computed_id.clone());
        }
        *id_counts.entry(node.computed_id.as_str()).or_insert(0) += 1;
        for c in &node.children {
            visit(c, index, id_counts);
        }
    }
    visit(root, &mut ui_state.layout.key_index, &mut id_counts);
    warn_duplicate_ids(&id_counts, &mut ui_state.layout.warned_duplicate_ids);
}

/// Emit a once-per-id warning for every colliding `computed_id`. The
/// bundle lint already catches this as [`crate::bundle::lint::
/// FindingKind::DuplicateId`], but runtime-only apps never run it — so
/// this reuses the same finding vocabulary at log-warn level. Deduped
/// via the persistent `warned` set so a standing duplicate warns once,
/// not every layout (issue #64).
fn warn_duplicate_ids(
    id_counts: &rustc_hash::FxHashMap<&str, u32>,
    warned: &mut rustc_hash::FxHashSet<String>,
) {
    for (&id, &n) in id_counts {
        if n > 1 && warned.insert(id.to_string()) {
            log::warn!(
                "DuplicateId: {n} nodes share id {id} — duplicate sibling key; \
                 their rects and intrinsic-cache entries collide silently (issue #64)"
            );
        }
    }
}

/// Assign every node's `computed_id` without positioning anything else.
/// Useful when callers need to read or seed side-map entries (e.g.,
/// scroll offsets) before `layout` runs.
pub fn assign_ids(root: &mut El) {
    assign_id(root, "root");
}

fn assign_id(node: &mut El, path: &str) {
    node.computed_id = path.to_string();
    for (i, c) in node.children.iter_mut().enumerate() {
        let role = role_token(&c.kind);
        let suffix = match (&c.key, role) {
            (Some(k), r) => format!("{r}[{k}]"),
            (None, r) => format!("{r}.{i}"),
        };
        let child_path = format!("{path}.{suffix}");
        assign_id(c, &child_path);
    }
}

fn role_token(k: &Kind) -> &'static str {
    match k {
        Kind::Group => "group",
        Kind::Card => "card",
        Kind::Button => "button",
        Kind::Badge => "badge",
        Kind::Text => "text",
        Kind::Heading => "heading",
        Kind::Spacer => "spacer",
        Kind::Divider => "divider",
        Kind::Overlay => "overlay",
        Kind::Scrim => "scrim",
        Kind::Modal => "modal",
        Kind::Scroll => "scroll",
        Kind::VirtualList => "virtual_list",
        Kind::Inlines => "inlines",
        Kind::HardBreak => "hard_break",
        Kind::Math => "math",
        Kind::Image => "image",
        Kind::Surface => "surface",
        Kind::Vector => "vector",
        Kind::Scene3D => "scene3d",
        Kind::Viewport => "viewport",
        Kind::Custom(name) => name,
    }
}

fn layout_children(node: &mut El, node_rect: Rect, ui_state: &mut UiState) {
    if matches!(node.kind, Kind::Inlines) {
        // The paragraph paints as a single AttributedText DrawOp;
        // child Text/HardBreak nodes are aggregated by draw_ops::
        // push_node and don't paint independently. Give each child a
        // zero-size rect so the rest of the engine (hit-test, focus,
        // animation, lint) treats them as non-paint pseudo-nodes. The
        // paragraph's hit-test target is the Inlines node itself,
        // sized by node_rect.
        for c in &mut node.children {
            ui_state.layout.computed_rects.insert(
                c.computed_id.clone(),
                Rect::new(node_rect.x, node_rect.y, 0.0, 0.0),
            );
            // Recurse so descendants of Text/HardBreak nodes (rare —
            // these are leaves in practice — but keeping the invariant
            // simple) still get their rects assigned.
            layout_children(c, Rect::new(node_rect.x, node_rect.y, 0.0, 0.0), ui_state);
        }
        return;
    }
    if let Some(items) = node.virtual_items.clone() {
        layout_virtual(node, node_rect, items, ui_state);
        return;
    }
    if let Some(layout_fn) = node.layout_override.clone() {
        layout_custom(node, node_rect, layout_fn, ui_state);
        if node.scrollable {
            apply_scroll_offset(node, node_rect, ui_state);
        }
        if node.viewport.is_some() {
            apply_viewport_transform(node, node_rect, ui_state);
        }
        return;
    }
    match node.axis {
        Axis::Overlay => {
            let inner = node_rect.inset(node.padding);
            for c in &mut node.children {
                let c_rect = overlay_rect(c, inner, node.align, node.justify);
                ui_state
                    .layout
                    .computed_rects
                    .insert(c.computed_id.clone(), c_rect);
                layout_children(c, c_rect, ui_state);
            }
        }
        Axis::Column => layout_axis(node, node_rect, true, ui_state),
        Axis::Row => layout_axis(node, node_rect, false, ui_state),
    }
    if node.scrollable {
        apply_scroll_offset(node, node_rect, ui_state);
    }
    if node.viewport.is_some() {
        apply_viewport_transform(node, node_rect, ui_state);
    }
}

fn layout_custom(node: &mut El, node_rect: Rect, layout_fn: LayoutFn, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    let measure = |c: &El| intrinsic(c);
    // Split-borrow `ui_state` so the `rect_of_key` closure reads the
    // key index + computed rects while the surrounding function still
    // holds the mutable borrow needed to insert this node's children
    // back into `computed_rects` afterwards.
    let key_index = &ui_state.layout.key_index;
    let computed_rects = &ui_state.layout.computed_rects;
    let rect_of_key = |key: &str| -> Option<Rect> {
        let id = key_index.get(key)?;
        computed_rects.get(id).copied()
    };
    let rect_of_id = |id: &str| -> Option<Rect> { computed_rects.get(id).copied() };
    let rects = (layout_fn.0)(LayoutCtx {
        container: inner,
        children: &node.children,
        measure: &measure,
        rect_of_key: &rect_of_key,
        rect_of_id: &rect_of_id,
    });
    assert_eq!(
        rects.len(),
        node.children.len(),
        "LayoutFn for {:?} returned {} rects for {} children",
        node.computed_id,
        rects.len(),
        node.children.len(),
    );
    for (c, c_rect) in node.children.iter_mut().zip(rects) {
        ui_state
            .layout
            .computed_rects
            .insert(c.computed_id.clone(), c_rect);
        layout_children(c, c_rect, ui_state);
    }
}

/// Virtualized list realization. Dispatches by [`VirtualMode`] —
/// `Fixed` uses an O(1) division to find the visible range; `Dynamic`
/// walks measured-or-estimated heights, measures each visible row's
/// natural intrinsic height, and writes the result back to the height
/// cache on `UiState` so subsequent frames have it available.
fn layout_virtual(node: &mut El, node_rect: Rect, items: VirtualItems, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    match items.mode {
        VirtualMode::Fixed { row_height } => layout_virtual_fixed(
            node,
            inner,
            items.count,
            row_height,
            items.build_row,
            ui_state,
        ),
        VirtualMode::Dynamic {
            estimated_row_height,
            append_only,
        } => {
            let fns = DynamicVirtualFns {
                anchor_policy: items.anchor_policy,
                row_key: items.row_key,
                build_row: items.build_row,
            };
            if append_only {
                layout_virtual_dynamic_incremental(
                    node,
                    inner,
                    items.count,
                    estimated_row_height,
                    fns,
                    ui_state,
                );
            } else {
                layout_virtual_dynamic(
                    node,
                    inner,
                    items.count,
                    estimated_row_height,
                    fns,
                    ui_state,
                );
            }
        }
    }
}

/// Consume any pending [`ScrollRequest`]s targeting this list's `key`,
/// resolving each into a target offset using the live viewport rect and
/// the caller-supplied row-extent function. Writes the resolved offset
/// directly into `scroll.offsets`; the immediately-following
/// `write_virtual_scroll_state` call clamps it to `[0, max_offset]`.
///
/// Requests for other lists are left in the queue for sibling lists in
/// the same layout pass. Anything still queued after layout completes is
/// dropped by the runtime (see `prepare_layout`).
fn resolve_scroll_requests<F, K>(
    node: &El,
    inner: Rect,
    count: usize,
    row_extent: F,
    row_for_key: K,
    ui_state: &mut UiState,
) -> bool
where
    F: Fn(usize) -> (f32, f32),
    K: Fn(&str) -> Option<usize>,
{
    if ui_state.scroll.pending_requests.is_empty() {
        return false;
    }
    let Some(key) = node.key.as_deref() else {
        return false;
    };
    let pending = std::mem::take(&mut ui_state.scroll.pending_requests);
    let (matched, remaining): (Vec<ScrollRequest>, Vec<ScrollRequest>) =
        pending.into_iter().partition(|req| match req {
            ScrollRequest::ToRow { list_key, .. } => list_key == key,
            ScrollRequest::ToRowKey { list_key, .. } => list_key == key,
            // EnsureVisible isn't a virtual-list-row request; let the
            // non-virtual scroll resolver pick it up downstream.
            ScrollRequest::EnsureVisible { .. } => false,
        });
    ui_state.scroll.pending_requests = remaining;

    let mut wrote = false;
    for req in matched {
        let (row, align) = match req {
            ScrollRequest::ToRow { row, align, .. } => (row, align),
            ScrollRequest::ToRowKey { row_key, align, .. } => {
                let Some(row) = row_for_key(&row_key) else {
                    continue;
                };
                (row, align)
            }
            ScrollRequest::EnsureVisible { .. } => continue,
        };
        if row >= count {
            continue;
        }
        let (row_top, row_h) = row_extent(row);
        let row_bottom = row_top + row_h;
        let viewport_h = inner.h;
        let current = ui_state
            .scroll
            .offsets
            .get(&node.computed_id)
            .copied()
            .unwrap_or(0.0);
        let new_offset = match align {
            ScrollAlignment::Start => row_top,
            ScrollAlignment::End => row_bottom - viewport_h,
            ScrollAlignment::Center => row_top + (row_h - viewport_h) / 2.0,
            ScrollAlignment::Visible => {
                if row_top < current {
                    row_top
                } else if row_bottom > current + viewport_h {
                    row_bottom - viewport_h
                } else {
                    continue;
                }
            }
        };
        ui_state
            .scroll
            .offsets
            .insert(node.computed_id.clone(), new_offset);
        wrote = true;
    }
    wrote
}

/// Clamp the stored scroll offset, write the metrics + thumb rect, and
/// return the clamped offset. Shared scaffold for both virtual modes.
fn write_virtual_scroll_state(node: &El, inner: Rect, total_h: f32, ui_state: &mut UiState) -> f32 {
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let stored = resolve_pin(node, stored, max_offset, ui_state);
    let offset = stored.clamp(0.0, max_offset);
    ui_state
        .scroll
        .offsets
        .insert(node.computed_id.clone(), offset);
    write_virtual_scroll_metrics(node, inner, total_h, max_offset, offset, ui_state);
    offset
}

fn write_virtual_scroll_metrics(
    node: &El,
    inner: Rect,
    total_h: f32,
    max_offset: f32,
    offset: f32,
    ui_state: &mut UiState,
) {
    ui_state.scroll.metrics.insert(
        node.computed_id.clone(),
        crate::state::ScrollMetrics {
            viewport_h: inner.h,
            content_h: total_h,
            max_offset,
        },
    );
    write_thumb_rect(node, inner, total_h, max_offset, offset, ui_state);
}

/// Assign the realized row a path-style `computed_id` matching the
/// regular tree's role/key/index convention so hit-test, focus, and
/// state lookups remain stable across scrolls.
fn assign_virtual_row_id(child: &mut El, parent_id: &str, global_i: usize) {
    let role = role_token(&child.kind);
    let suffix = match (&child.key, role) {
        (Some(k), r) => format!("{r}[{k}]"),
        (None, r) => format!("{r}.{global_i}"),
    };
    assign_id(child, &format!("{parent_id}.{suffix}"));
}

fn layout_virtual_fixed(
    node: &mut El,
    inner: Rect,
    count: usize,
    row_height: f32,
    build_row: Arc<dyn Fn(usize) -> El + Send + Sync>,
    ui_state: &mut UiState,
) {
    let gap = node.gap.max(0.0);
    let pitch = row_height + gap;
    let total_h = virtual_total_height(count, count as f32 * row_height, gap);
    resolve_scroll_requests(
        node,
        inner,
        count,
        |i| (i as f32 * pitch, row_height),
        |row_key| row_key.parse::<usize>().ok().filter(|row| *row < count),
        ui_state,
    );
    let offset = write_virtual_scroll_state(node, inner, total_h, ui_state);

    if count == 0 {
        node.children.clear();
        return;
    }

    // Visible index range — `start` floors, `end` ceils, both clamped.
    // Include one extra candidate because a large gap can make the
    // pitch-based ceil land on the gap before the next visible row.
    let start = (offset / pitch).floor() as usize;
    let end = ((((offset + inner.h) / pitch).ceil() as usize) + 1).min(count);

    let mut realized: Vec<El> = Vec::new();
    let mut realized_range: Option<(usize, usize)> = None;
    for global_i in start..end {
        let row_top = global_i as f32 * pitch;
        if row_top >= offset + inner.h || row_top + row_height <= offset {
            continue;
        }
        let mut child = (build_row)(global_i);
        assign_virtual_row_id(&mut child, &node.computed_id, global_i);

        let row_y = inner.y + row_top - offset;
        let c_rect = Rect::new(inner.x, row_y, inner.w, row_height);
        ui_state
            .layout
            .computed_rects
            .insert(child.computed_id.clone(), c_rect);
        layout_children(&mut child, c_rect, ui_state);
        realized.push(child);
        realized_range = Some(match realized_range {
            None => (global_i, global_i + 1),
            Some((s, _)) => (s, global_i + 1),
        });
    }
    if let Some((s, e)) = realized_range {
        ui_state
            .scroll
            .visible_ranges
            .insert(node.computed_id.clone(), (s, e));
    }
    node.children = realized;
}

/// Append-only fast path for `VirtualMode::Dynamic { append_only: true }`.
/// Behaviourally identical to [`layout_virtual_dynamic`] (anchoring,
/// pinning, scroll-request resolution, measurement, the offset-correction
/// pass) — it reuses the same semantic helpers — but it never
/// materializes the full `Vec` of keys or heights. A persistent
/// [`DynHeightIndex`] holds the per-row heights across frames; this pass
/// reconciles it trim-then-append and answers every height / row-top /
/// visible-range query off the index, so per-frame cost is O(visible)
/// (plus O(√n) range queries) instead of the general path's several O(n)
/// walks (issue #107). See the dispatch in [`layout_virtual`] and the
/// contract on [`VirtualMode::Dynamic::append_only`].
fn layout_virtual_dynamic_incremental(
    node: &mut El,
    inner: Rect,
    count: usize,
    estimated_row_height: f32,
    fns: DynamicVirtualFns,
    ui_state: &mut UiState,
) {
    let gap = node.gap.max(0.0);
    let width_bucket = virtual_width_bucket(inner.w);

    if count == 0 {
        ui_state.scroll.virtual_anchors.remove(&node.computed_id);
        ui_state.scroll.dyn_height_index.remove(&node.computed_id);
        let offset = write_virtual_scroll_state(node, inner, 0.0, ui_state);
        debug_assert_eq!(offset, 0.0);
        node.children.clear();
        return;
    }

    // Pull the persistent height index out of `ui_state` so it is an
    // owned local for the rest of the pass (sidestepping the borrow
    // against `&mut ui_state` the helpers need). Reconcile it against
    // this frame as trim-then-append, or cold-rebuild when that's not
    // possible (first frame, width change, or a contract violation —
    // self-healing to correct geometry at an O(n) cost for that frame).
    let existing = ui_state.scroll.dyn_height_index.remove(&node.computed_id);
    let head_key = (fns.row_key)(0);
    let mut trimmed_keys: Vec<String> = Vec::new();
    let reconciled = existing.and_then(|mut ix| {
        let ok = ix.reconcile(
            width_bucket,
            estimated_row_height,
            count,
            &head_key,
            |i| (fns.row_key)(i),
            |_i, key| {
                cached_row_height(
                    ui_state,
                    &node.computed_id,
                    key,
                    width_bucket,
                    estimated_row_height,
                )
            },
            &mut trimmed_keys,
        );
        ok.then_some(ix)
    });
    let mut index = match reconciled {
        Some(ix) => ix,
        None => DynHeightIndex::build(width_bucket, estimated_row_height, count, |i| {
            let key = (fns.row_key)(i);
            let h = cached_row_height(
                ui_state,
                &node.computed_id,
                &key,
                width_bucket,
                estimated_row_height,
            );
            (key, h)
        }),
    };
    // Evict measurement-cache entries for rows trimmed off the head, so
    // the cache stays bounded over a long ring-buffer feed. This is the
    // incremental analogue of the general path's per-frame O(n)
    // `prune_dynamic_measurements`; here it's O(trimmed).
    if !trimmed_keys.is_empty() {
        if let Some(measured) = ui_state
            .scroll
            .measured_row_heights
            .get_mut(&node.computed_id)
        {
            for key in &trimmed_keys {
                measured.remove(key);
            }
            if measured.is_empty() {
                ui_state
                    .scroll
                    .measured_row_heights
                    .remove(&node.computed_id);
            }
        }
    }

    // Skip the cache snapshot entirely when nothing in the queue targets
    // this list (see the general path for the rationale).
    let has_request = node.key.as_deref().is_some_and(|k| {
        ui_state.scroll.pending_requests.iter().any(|r| match r {
            ScrollRequest::ToRow { list_key, .. } => list_key == k,
            ScrollRequest::ToRowKey { list_key, .. } => list_key == k,
            ScrollRequest::EnsureVisible { .. } => false,
        })
    });
    let mut request_wrote = false;
    if has_request {
        request_wrote = resolve_scroll_requests(
            node,
            inner,
            count,
            |target| (index.row_top(target, gap), index.height(target)),
            |row_key| index.index_for_key(row_key),
            ui_state,
        );
    }

    let total_h = virtual_total_height(count, index.heights_sum(), gap);
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let pin_active = pin_would_be_active(node, stored, max_offset, ui_state).unwrap_or(false);
    let provisional_offset = if pin_active {
        match node.pin_policy {
            crate::tree::PinPolicy::End => max_offset,
            crate::tree::PinPolicy::Start => 0.0,
            crate::tree::PinPolicy::None => unreachable!(),
        }
    } else if request_wrote {
        stored
    } else {
        dynamic_anchor_offset_indexed(node, &index, gap, stored, ui_state).unwrap_or(stored)
    }
    .clamp(0.0, max_offset);

    let (measure_start, _, measure_end) = index.visible_range(gap, provisional_offset, inner.h);
    measure_dynamic_range(
        node,
        DynamicRangeCtx {
            inner,
            keys: KeySource::Func(&*fns.row_key),
            width_bucket,
            build_row: &fns.build_row,
        },
        measure_start,
        measure_end,
        ui_state,
    );
    refresh_index_range(
        &mut index,
        &node.computed_id,
        width_bucket,
        measure_start,
        measure_end,
        &*fns.row_key,
        estimated_row_height,
        ui_state,
    );

    let total_h = virtual_total_height(count, index.heights_sum(), gap);
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let pin_resolved = resolve_pin(node, stored, max_offset, ui_state);
    let pin_active = !matches!(node.pin_policy, crate::tree::PinPolicy::None)
        && ui_state
            .scroll
            .pin_active
            .get(&node.computed_id)
            .copied()
            .unwrap_or(false);
    let mut offset = if pin_active {
        pin_resolved
    } else if request_wrote {
        stored
    } else {
        dynamic_anchor_offset_indexed(node, &index, gap, stored, ui_state).unwrap_or(stored)
    }
    .clamp(0.0, max_offset);

    ui_state
        .scroll
        .offsets
        .insert(node.computed_id.clone(), offset);

    let (start, start_y, end) = index.visible_range(gap, offset, inner.h);
    let mut realized_rows = layout_dynamic_range(
        node,
        DynamicRangeCtx {
            inner,
            keys: KeySource::Func(&*fns.row_key),
            width_bucket,
            build_row: &fns.build_row,
        },
        offset,
        start,
        start_y,
        end,
        ui_state,
    );
    refresh_index_range(
        &mut index,
        &node.computed_id,
        width_bucket,
        start,
        end,
        &*fns.row_key,
        estimated_row_height,
        ui_state,
    );

    let total_h = virtual_total_height(count, index.heights_sum(), gap);
    let max_offset = (total_h - inner.h).max(0.0);
    let corrected_offset = if pin_active {
        match node.pin_policy {
            crate::tree::PinPolicy::End => max_offset,
            crate::tree::PinPolicy::Start => 0.0,
            crate::tree::PinPolicy::None => unreachable!(),
        }
    } else if request_wrote {
        offset
    } else {
        dynamic_anchor_offset_indexed(node, &index, gap, stored, ui_state).unwrap_or(offset)
    }
    .clamp(0.0, max_offset);
    if (corrected_offset - offset).abs() > 0.01 {
        let dy = offset - corrected_offset;
        for child in &node.children {
            shift_subtree_y(child, dy, ui_state);
        }
        for row in &mut realized_rows {
            row.rect.y += dy;
        }
        offset = corrected_offset;
        ui_state
            .scroll
            .offsets
            .insert(node.computed_id.clone(), offset);
    }
    if matches!(node.pin_policy, crate::tree::PinPolicy::End) {
        ui_state
            .scroll
            .pin_prev_max
            .insert(node.computed_id.clone(), max_offset);
    }
    write_virtual_scroll_metrics(node, inner, total_h, max_offset, offset, ui_state);

    if let Some(anchor) = choose_dynamic_anchor(fns.anchor_policy, inner, offset, &realized_rows) {
        ui_state
            .scroll
            .virtual_anchors
            .insert(node.computed_id.clone(), anchor);
    } else {
        ui_state.scroll.virtual_anchors.remove(&node.computed_id);
    }

    ui_state
        .scroll
        .dyn_height_index
        .insert(node.computed_id.clone(), index);
}

/// Measured-or-estimated height for one row, read from the persistent
/// measurement cache exactly as the general path's `dynamic_row_heights`
/// does (keyed by list id → row key → width bucket).
fn cached_row_height(
    ui_state: &UiState,
    id: &str,
    key: &str,
    width_bucket: u32,
    estimated_row_height: f32,
) -> f32 {
    ui_state
        .scroll
        .measured_row_heights
        .get(id)
        .and_then(|m| m.get(key))
        .and_then(|by_width| by_width.get(&width_bucket))
        .copied()
        .unwrap_or(estimated_row_height)
}

/// Fold the freshly measured heights for the realized window `[start,
/// end)` back into the height index, so the next query sees real
/// measurements instead of the estimate. O(visible).
#[allow(clippy::too_many_arguments)]
fn refresh_index_range(
    index: &mut DynHeightIndex,
    id: &str,
    width_bucket: u32,
    start: usize,
    end: usize,
    row_key: &(dyn Fn(usize) -> String + Send + Sync),
    estimated_row_height: f32,
    ui_state: &UiState,
) {
    for idx in start..end {
        let key = row_key(idx);
        let h = cached_row_height(ui_state, id, &key, width_bucket, estimated_row_height);
        index.set_height(idx, h);
    }
}

/// Index-backed equivalent of [`dynamic_anchor_offset`]: resolve the
/// stored anchor's row through the height index (O(1) key→index, O(√n)
/// row-top) instead of a linear scan over a materialized key slice.
fn dynamic_anchor_offset_indexed(
    node: &El,
    index: &DynHeightIndex,
    gap: f32,
    stored: f32,
    ui_state: &UiState,
) -> Option<f32> {
    let anchor = ui_state.scroll.virtual_anchors.get(&node.computed_id)?;
    let idx = index.index_for_key(&anchor.row_key)?;
    let row_h = index.height(idx).max(0.0);
    let row_point = row_h * anchor.row_fraction.clamp(0.0, 1.0);
    let scroll_delta = stored - anchor.resolved_offset;
    let viewport_y = anchor.viewport_y - scroll_delta;
    Some(index.row_top(idx, gap) + row_point - viewport_y)
}

fn layout_virtual_dynamic(
    node: &mut El,
    inner: Rect,
    count: usize,
    estimated_row_height: f32,
    fns: DynamicVirtualFns,
    ui_state: &mut UiState,
) {
    let gap = node.gap.max(0.0);
    let width_bucket = virtual_width_bucket(inner.w);
    let row_keys = (0..count).map(|i| (fns.row_key)(i)).collect::<Vec<_>>();
    prune_dynamic_measurements(node, &row_keys, ui_state);

    if count == 0 {
        ui_state.scroll.virtual_anchors.remove(&node.computed_id);
        let offset = write_virtual_scroll_state(node, inner, 0.0, ui_state);
        debug_assert_eq!(offset, 0.0);
        node.children.clear();
        return;
    }

    let mut row_heights = dynamic_row_heights(
        node,
        &row_keys,
        width_bucket,
        estimated_row_height,
        ui_state,
    );

    // Skip the cache snapshot entirely when nothing in the queue
    // targets this list — a hot path on dynamic lists with warm
    // caches (potentially thousands of entries) that would otherwise
    // pay a per-frame HashMap clone for an operation that fires
    // maybe once a minute.
    let has_request = node.key.as_deref().is_some_and(|k| {
        ui_state.scroll.pending_requests.iter().any(|r| match r {
            ScrollRequest::ToRow { list_key, .. } => list_key == k,
            ScrollRequest::ToRowKey { list_key, .. } => list_key == k,
            ScrollRequest::EnsureVisible { .. } => false,
        })
    });
    let mut request_wrote = false;
    if has_request {
        request_wrote = resolve_scroll_requests(
            node,
            inner,
            count,
            |target| {
                (
                    dynamic_row_top(&row_heights, gap, target),
                    row_heights[target],
                )
            },
            |row_key| row_keys.iter().position(|key| key == row_key),
            ui_state,
        );
    }

    let total_h = virtual_total_height(count, row_heights.iter().sum(), gap);
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let pin_active = pin_would_be_active(node, stored, max_offset, ui_state).unwrap_or(false);
    let provisional_offset = if pin_active {
        match node.pin_policy {
            crate::tree::PinPolicy::End => max_offset,
            crate::tree::PinPolicy::Start => 0.0,
            crate::tree::PinPolicy::None => unreachable!(),
        }
    } else if request_wrote {
        stored
    } else {
        dynamic_anchor_offset(node, &row_keys, &row_heights, gap, stored, ui_state)
            .unwrap_or(stored)
    }
    .clamp(0.0, max_offset);

    let (measure_start, _, measure_end) =
        dynamic_visible_range(&row_heights, gap, provisional_offset, inner.h);
    measure_dynamic_range(
        node,
        DynamicRangeCtx {
            inner,
            keys: KeySource::Slice(&row_keys),
            width_bucket,
            build_row: &fns.build_row,
        },
        measure_start,
        measure_end,
        ui_state,
    );

    row_heights = dynamic_row_heights(
        node,
        &row_keys,
        width_bucket,
        estimated_row_height,
        ui_state,
    );
    let total_h = virtual_total_height(count, row_heights.iter().sum(), gap);
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let pin_resolved = resolve_pin(node, stored, max_offset, ui_state);
    let pin_active = !matches!(node.pin_policy, crate::tree::PinPolicy::None)
        && ui_state
            .scroll
            .pin_active
            .get(&node.computed_id)
            .copied()
            .unwrap_or(false);
    let mut offset = if pin_active {
        pin_resolved
    } else if request_wrote {
        stored
    } else {
        dynamic_anchor_offset(node, &row_keys, &row_heights, gap, stored, ui_state)
            .unwrap_or(stored)
    }
    .clamp(0.0, max_offset);

    ui_state
        .scroll
        .offsets
        .insert(node.computed_id.clone(), offset);

    let (start, start_y, end) = dynamic_visible_range(&row_heights, gap, offset, inner.h);
    let mut realized_rows = layout_dynamic_range(
        node,
        DynamicRangeCtx {
            inner,
            keys: KeySource::Slice(&row_keys),
            width_bucket,
            build_row: &fns.build_row,
        },
        offset,
        start,
        start_y,
        end,
        ui_state,
    );

    row_heights = dynamic_row_heights(
        node,
        &row_keys,
        width_bucket,
        estimated_row_height,
        ui_state,
    );
    let total_h = virtual_total_height(count, row_heights.iter().sum(), gap);
    let max_offset = (total_h - inner.h).max(0.0);
    let corrected_offset = if pin_active {
        match node.pin_policy {
            crate::tree::PinPolicy::End => max_offset,
            crate::tree::PinPolicy::Start => 0.0,
            crate::tree::PinPolicy::None => unreachable!(),
        }
    } else if request_wrote {
        offset
    } else {
        dynamic_anchor_offset(node, &row_keys, &row_heights, gap, stored, ui_state)
            .unwrap_or(offset)
    }
    .clamp(0.0, max_offset);
    if (corrected_offset - offset).abs() > 0.01 {
        let dy = offset - corrected_offset;
        for child in &node.children {
            shift_subtree_y(child, dy, ui_state);
        }
        for row in &mut realized_rows {
            row.rect.y += dy;
        }
        offset = corrected_offset;
        ui_state
            .scroll
            .offsets
            .insert(node.computed_id.clone(), offset);
    }
    if matches!(node.pin_policy, crate::tree::PinPolicy::End) {
        ui_state
            .scroll
            .pin_prev_max
            .insert(node.computed_id.clone(), max_offset);
    }
    write_virtual_scroll_metrics(node, inner, total_h, max_offset, offset, ui_state);

    if let Some(anchor) = choose_dynamic_anchor(fns.anchor_policy, inner, offset, &realized_rows) {
        ui_state
            .scroll
            .virtual_anchors
            .insert(node.computed_id.clone(), anchor);
    } else {
        ui_state.scroll.virtual_anchors.remove(&node.computed_id);
    }
}

struct DynamicVirtualFns {
    anchor_policy: VirtualAnchorPolicy,
    row_key: Arc<dyn Fn(usize) -> String + Send + Sync>,
    build_row: Arc<dyn Fn(usize) -> El + Send + Sync>,
}

#[derive(Clone, Copy)]
struct DynamicRangeCtx<'a> {
    inner: Rect,
    keys: KeySource<'a>,
    width_bucket: u32,
    build_row: &'a Arc<dyn Fn(usize) -> El + Send + Sync>,
}

/// Source of stable row keys over a realized range. The general dynamic
/// path already has every key materialized as a slice; the append-only
/// incremental path resolves keys on demand for the visible window only
/// (it never materializes all `n`). Both feed the same range
/// realization code (`measure_dynamic_range` / `layout_dynamic_range`).
#[derive(Clone, Copy)]
enum KeySource<'a> {
    Slice(&'a [String]),
    Func(&'a (dyn Fn(usize) -> String + Send + Sync)),
}

impl KeySource<'_> {
    fn key(&self, idx: usize) -> String {
        match self {
            KeySource::Slice(keys) => keys[idx].clone(),
            KeySource::Func(f) => f(idx),
        }
    }
}

fn virtual_width_bucket(width: f32) -> u32 {
    width.max(0.0).round().min(u32::MAX as f32) as u32
}

fn prune_dynamic_measurements(node: &El, row_keys: &[String], ui_state: &mut UiState) {
    let Some(measurements) = ui_state
        .scroll
        .measured_row_heights
        .get_mut(&node.computed_id)
    else {
        return;
    };
    let live_keys = row_keys
        .iter()
        .map(String::as_str)
        .collect::<FxHashSet<_>>();
    measurements.retain(|key, widths| {
        let live = live_keys.contains(key.as_str());
        if live {
            widths.retain(|_, h| h.is_finite() && *h >= 0.0);
        }
        live && !widths.is_empty()
    });
    if measurements.is_empty() {
        ui_state
            .scroll
            .measured_row_heights
            .remove(&node.computed_id);
    }
}

fn dynamic_row_heights(
    node: &El,
    row_keys: &[String],
    width_bucket: u32,
    estimated_row_height: f32,
    ui_state: &UiState,
) -> Vec<f32> {
    let measurements = ui_state.scroll.measured_row_heights.get(&node.computed_id);
    row_keys
        .iter()
        .map(|key| {
            measurements
                .and_then(|m| m.get(key))
                .and_then(|by_width| by_width.get(&width_bucket))
                .copied()
                .unwrap_or(estimated_row_height)
        })
        .collect()
}

fn dynamic_row_top(row_heights: &[f32], gap: f32, target: usize) -> f32 {
    row_heights
        .iter()
        .take(target)
        .fold(0.0, |y, h| y + *h + gap)
}

fn dynamic_visible_range(
    row_heights: &[f32],
    gap: f32,
    offset: f32,
    viewport_h: f32,
) -> (usize, f32, usize) {
    let count = row_heights.len();
    let mut start = 0;
    let mut y = 0.0_f32;
    while start < count {
        let h = row_heights[start];
        if y + h > offset {
            break;
        }
        y += h + gap;
        start += 1;
    }

    let mut end = start;
    let mut cursor = y;
    let viewport_bottom = offset + viewport_h;
    while end < count && cursor < viewport_bottom {
        let h = row_heights[end];
        end += 1;
        cursor += h + gap;
    }
    (start, y, end)
}

fn dynamic_anchor_offset(
    node: &El,
    row_keys: &[String],
    row_heights: &[f32],
    gap: f32,
    stored: f32,
    ui_state: &UiState,
) -> Option<f32> {
    let anchor = ui_state.scroll.virtual_anchors.get(&node.computed_id)?;
    let idx = if anchor.row_index < row_keys.len() && row_keys[anchor.row_index] == anchor.row_key {
        anchor.row_index
    } else {
        row_keys.iter().position(|key| key == &anchor.row_key)?
    };
    let row_h = row_heights.get(idx).copied().unwrap_or(0.0).max(0.0);
    let row_point = row_h * anchor.row_fraction.clamp(0.0, 1.0);
    let scroll_delta = stored - anchor.resolved_offset;
    let viewport_y = anchor.viewport_y - scroll_delta;
    Some(dynamic_row_top(row_heights, gap, idx) + row_point - viewport_y)
}

fn measure_dynamic_range(
    node: &El,
    ctx: DynamicRangeCtx<'_>,
    start: usize,
    end: usize,
    ui_state: &mut UiState,
) {
    if start >= end {
        return;
    }
    let mut new_measurements = Vec::new();
    for idx in start..end {
        let key = ctx.keys.key(idx);
        let mut child = (ctx.build_row)(idx);
        // Assign the same id `layout_dynamic_range` will use so this
        // measurement lands in the per-pass intrinsic cache — without
        // it the cache key is `None` (empty computed_id) and the
        // layout pass re-measures every row from scratch, doubling
        // per-frame shaping work for dynamic lists (issue #59).
        assign_virtual_row_id(&mut child, &node.computed_id, idx);
        let actual_h = measure_dynamic_row(node, idx, ctx.inner.w, &child);
        new_measurements.push((key, actual_h));
    }
    store_dynamic_measurements(node, ctx.width_bucket, new_measurements, ui_state);
}

fn measure_dynamic_row(node: &El, idx: usize, width: f32, child: &El) -> f32 {
    match child.height {
        Size::Fixed(v) => v.max(0.0),
        Size::Hug => intrinsic_constrained(child, Some(width)).1.max(0.0),
        Size::Aspect(r) => (width * r).max(0.0),
        Size::Fill(_) => panic!(
            "virtual_list_dyn row {idx} on {:?} must size with Size::Fixed, Size::Hug, \
             or Size::Aspect; Size::Fill would absorb the viewport's height and break \
             virtualization",
            node.computed_id,
        ),
    }
}

/// Width buckets retained per measured row. Buckets are 1-logical-px
/// granular ([`virtual_width_bucket`]), so a user dragging a window
/// edge sweeps through hundreds of distinct buckets; without a cap
/// every one is retained for the list's lifetime (issue #57). Only the
/// buckets nearest the current width matter — heights at distant
/// widths are stale guesses anyway once content rewraps.
const MAX_WIDTH_BUCKETS_PER_ROW: usize = 8;

fn store_dynamic_measurements(
    node: &El,
    width_bucket: u32,
    measurements: Vec<(String, f32)>,
    ui_state: &mut UiState,
) {
    if measurements.is_empty() {
        return;
    }
    let entry = ui_state
        .scroll
        .measured_row_heights
        .entry(node.computed_id.clone())
        .or_default();
    for (row_key, h) in measurements {
        let buckets = entry.entry(row_key).or_default();
        buckets.insert(width_bucket, h);
        if buckets.len() > MAX_WIDTH_BUCKETS_PER_ROW {
            // Keep the buckets closest to the width we're laying out
            // at; drop the farthest.
            let mut widths: Vec<u32> = buckets.keys().copied().collect();
            widths.sort_unstable_by_key(|w| (i64::from(*w) - i64::from(width_bucket)).abs());
            for w in widths.drain(MAX_WIDTH_BUCKETS_PER_ROW..) {
                buckets.remove(&w);
            }
        }
    }
}

#[derive(Clone, Debug)]
struct DynamicRealizedRow {
    index: usize,
    key: String,
    rect: Rect,
}

fn layout_dynamic_range(
    node: &mut El,
    ctx: DynamicRangeCtx<'_>,
    offset: f32,
    start: usize,
    start_y: f32,
    end: usize,
    ui_state: &mut UiState,
) -> Vec<DynamicRealizedRow> {
    let gap = node.gap.max(0.0);
    let mut cursor_y = start_y;
    let mut realized = Vec::new();
    let mut realized_rows = Vec::new();
    let mut new_measurements = Vec::new();

    for idx in start..end {
        let key = ctx.keys.key(idx);
        let mut child = (ctx.build_row)(idx);
        assign_virtual_row_id(&mut child, &node.computed_id, idx);
        let actual_h = measure_dynamic_row(node, idx, ctx.inner.w, &child);
        new_measurements.push((key.clone(), actual_h));

        let row_y = ctx.inner.y + cursor_y - offset;
        let c_rect = Rect::new(ctx.inner.x, row_y, ctx.inner.w, actual_h);
        ui_state
            .layout
            .computed_rects
            .insert(child.computed_id.clone(), c_rect);
        layout_children(&mut child, c_rect, ui_state);

        realized_rows.push(DynamicRealizedRow {
            index: idx,
            key,
            rect: c_rect,
        });
        realized.push(child);
        cursor_y += actual_h + gap;
    }

    store_dynamic_measurements(node, ctx.width_bucket, new_measurements, ui_state);
    if let (Some(first), Some(last)) = (realized_rows.first(), realized_rows.last()) {
        ui_state
            .scroll
            .visible_ranges
            .insert(node.computed_id.clone(), (first.index, last.index + 1));
    }
    node.children = realized;
    realized_rows
}

fn choose_dynamic_anchor(
    policy: VirtualAnchorPolicy,
    inner: Rect,
    offset: f32,
    rows: &[DynamicRealizedRow],
) -> Option<VirtualAnchor> {
    let visible = rows
        .iter()
        .filter(|row| row.rect.bottom() > inner.y && row.rect.y < inner.bottom())
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return None;
    }

    let chosen = match policy {
        VirtualAnchorPolicy::ViewportFraction { y_fraction } => {
            let target_y = inner.y + inner.h * y_fraction.clamp(0.0, 1.0);
            visible
                .iter()
                .min_by(|a, b| {
                    let ad = distance_to_interval(target_y, a.rect.y, a.rect.bottom());
                    let bd = distance_to_interval(target_y, b.rect.y, b.rect.bottom());
                    ad.total_cmp(&bd)
                })
                .copied()
                .map(|row| {
                    let anchor_y = target_y.clamp(row.rect.y, row.rect.bottom());
                    (row.clone(), anchor_y)
                })
        }
        VirtualAnchorPolicy::FirstVisible => {
            let row = visible
                .iter()
                .find(|row| row.rect.y >= inner.y && row.rect.bottom() <= inner.bottom())
                .or_else(|| visible.first())
                .copied()?;
            let anchor_y = row.rect.y.max(inner.y);
            Some((row.clone(), anchor_y))
        }
        VirtualAnchorPolicy::LastVisible => {
            let row = visible
                .iter()
                .rev()
                .find(|row| row.rect.y >= inner.y && row.rect.bottom() <= inner.bottom())
                .or_else(|| visible.last())
                .copied()?;
            let anchor_y = row.rect.bottom().min(inner.bottom());
            Some((row.clone(), anchor_y))
        }
    }?;

    let (row, anchor_y) = chosen;
    let row_h = row.rect.h.max(0.0);
    let row_fraction = if row_h > 0.0 {
        ((anchor_y - row.rect.y) / row_h).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(VirtualAnchor {
        row_key: row.key.clone(),
        row_index: row.index,
        row_fraction,
        viewport_y: anchor_y - inner.y,
        resolved_offset: offset,
    })
}

fn distance_to_interval(y: f32, top: f32, bottom: f32) -> f32 {
    if y < top {
        top - y
    } else if y > bottom {
        y - bottom
    } else {
        0.0
    }
}

fn virtual_total_height(count: usize, row_sum: f32, gap: f32) -> f32 {
    if count == 0 {
        0.0
    } else {
        row_sum + gap * count.saturating_sub(1) as f32
    }
}

/// Scrollable post-pass: measure content height from the laid-out
/// children's stored rects, clamp the scroll offset to the available
/// range, and shift every descendant rect by `-offset`.
///
/// Children should size with `Hug` or `Fixed` on the main axis —
/// `Fill` children would absorb the viewport's height and there would
/// be nothing to scroll.
fn apply_scroll_offset(node: &El, node_rect: Rect, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    if node.children.is_empty() {
        ui_state
            .scroll
            .offsets
            .insert(node.computed_id.clone(), 0.0);
        ui_state.scroll.scroll_anchors.remove(&node.computed_id);
        ui_state.scroll.metrics.insert(
            node.computed_id.clone(),
            crate::state::ScrollMetrics {
                viewport_h: inner.h,
                content_h: 0.0,
                max_offset: 0.0,
            },
        );
        return;
    }
    let content_bottom = node
        .children
        .iter()
        .map(|c| ui_state.rect(&c.computed_id).bottom())
        .fold(f32::NEG_INFINITY, f32::max);
    let content_h = (content_bottom - inner.y).max(0.0);
    let max_offset = (content_h - inner.h).max(0.0);

    // Resolve any matching `ScrollRequest::EnsureVisible` against
    // this scroll BEFORE reading the stored offset, so the request's
    // chosen offset wins (and gets clamped below, just like
    // wheel-driven offsets do). A request matches when the node
    // keyed `container_key` is an ancestor of this scroll —
    // `key_index` resolves the key to a computed_id and a
    // prefix-match on `node.computed_id` tells us we're inside.
    let request_wrote = resolve_ensure_visible_for_scroll(node, inner, content_h, ui_state);

    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let stored = resolve_pin(node, stored, max_offset, ui_state);
    let pin_active = !matches!(node.pin_policy, crate::tree::PinPolicy::None)
        && ui_state
            .scroll
            .pin_active
            .get(&node.computed_id)
            .copied()
            .unwrap_or(false);
    let stored = if pin_active || request_wrote {
        stored
    } else {
        scroll_anchor_offset(node, inner, stored, ui_state).unwrap_or(stored)
    };
    let clamped = stored.clamp(0.0, max_offset);
    if clamped > 0.0 {
        for c in &node.children {
            shift_subtree_y(c, -clamped, ui_state);
        }
    }
    ui_state
        .scroll
        .offsets
        .insert(node.computed_id.clone(), clamped);
    ui_state.scroll.metrics.insert(
        node.computed_id.clone(),
        crate::state::ScrollMetrics {
            viewport_h: inner.h,
            content_h,
            max_offset,
        },
    );

    write_thumb_rect(node, inner, content_h, max_offset, clamped, ui_state);

    if let Some(anchor) = choose_scroll_anchor(node, inner, clamped, ui_state) {
        ui_state
            .scroll
            .scroll_anchors
            .insert(node.computed_id.clone(), anchor);
    } else {
        ui_state.scroll.scroll_anchors.remove(&node.computed_id);
    }
}

/// Resolve a [`viewport()`](crate::tree::viewport)'s pan/zoom and bake it
/// into its descendants' rects — the 2D pan + uniform-zoom analogue of
/// [`apply_scroll_offset`]. Runs after the viewport's children have been
/// laid out in un-transformed *content space*; measures the content
/// bounding box, consumes any matching
/// [`ViewportRequest`](crate::viewport::ViewportRequest), clamps the
/// result, then maps every descendant rect through the transform. Because
/// the transform lands in `computed_rects`, hit-test / links / selection
/// follow it automatically (paint additionally scales the descendants'
/// scalar visuals; see `draw_ops`).
fn apply_viewport_transform(node: &El, node_rect: Rect, ui_state: &mut UiState) {
    use crate::viewport::ViewportView;
    let cfg = node
        .viewport
        .expect("apply_viewport_transform called on a non-viewport node");
    let inner = node_rect.inset(node.padding);
    let origin = (inner.x, inner.y);
    let content = viewport_content_bbox(node, ui_state);

    // Start from the stored view, then fold in any programmatic requests
    // that name this viewport's key (consuming them).
    let mut view = ui_state
        .viewport
        .views
        .get(&node.computed_id)
        .copied()
        .unwrap_or_default();
    if let Some(key) = node.key.as_deref() {
        let mut i = 0;
        while i < ui_state.viewport.pending_requests.len() {
            if ui_state.viewport.pending_requests[i].key() == key {
                let req = ui_state.viewport.pending_requests.remove(i);
                view = apply_viewport_request(req, cfg, inner, origin, content, view);
            } else {
                i += 1;
            }
        }
    }

    // Clamp zoom to the configured range, then clamp pan so the content
    // can't be dragged out of the viewport.
    view.zoom = view.zoom.clamp(cfg.min_zoom, cfg.max_zoom);
    if let Some(c) = content {
        clamp_viewport_pan(&mut view, inner, origin, c);
    }

    // Bake into descendant rects. Identity is a no-op — skip the walk.
    if view != ViewportView::default() {
        transform_viewport_subtree(node, view, origin, ui_state);
    }

    ui_state
        .viewport
        .views
        .insert(node.computed_id.clone(), view);
    ui_state.viewport.metrics.insert(
        node.computed_id.clone(),
        crate::state::ViewportMetrics {
            inner,
            content,
            cfg,
        },
    );
}

/// Bounding box of all of `node`'s descendant rects, in content space
/// (the rects as laid out before the viewport transform). `None` when the
/// viewport has no children with rects.
fn viewport_content_bbox(node: &El, ui_state: &UiState) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for c in &node.children {
        if let Some(r) = ui_state.layout.computed_rects.get(&c.computed_id) {
            acc = Some(acc.map_or(*r, |a| union_rect(a, *r)));
        }
        if let Some(bb) = viewport_content_bbox(c, ui_state) {
            acc = Some(acc.map_or(bb, |a| union_rect(a, bb)));
        }
    }
    acc
}

/// Smallest rect covering both inputs.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = a.right().max(b.right());
    let bot = a.bottom().max(b.bottom());
    Rect::new(x, y, r - x, bot - y)
}

/// Map every descendant rect of a viewport through `view` about `origin`.
/// The viewport node's own rect is left untouched (it is the window).
fn transform_viewport_subtree(
    node: &El,
    view: crate::viewport::ViewportView,
    origin: (f32, f32),
    ui_state: &mut UiState,
) {
    for c in &node.children {
        if let Some(rect) = ui_state.layout.computed_rects.get_mut(&c.computed_id) {
            let (nx, ny) = view.project((rect.x, rect.y), origin);
            *rect = Rect::new(nx, ny, rect.w * view.zoom, rect.h * view.zoom);
        }
        transform_viewport_subtree(c, view, origin, ui_state);
    }
}

/// Apply one programmatic request to a viewport view, given the live
/// inner rect / origin / content bbox. `current` carries the zoom for
/// `CenterOn` and is the fallback when `FitContent` has nothing to frame.
fn apply_viewport_request(
    req: crate::viewport::ViewportRequest,
    cfg: crate::viewport::ViewportConfig,
    inner: Rect,
    origin: (f32, f32),
    content: Option<Rect>,
    current: crate::viewport::ViewportView,
) -> crate::viewport::ViewportView {
    use crate::viewport::{ViewportRequest, ViewportView};
    match req {
        ViewportRequest::ResetView { .. } => ViewportView::default(),
        ViewportRequest::CenterOn { point, .. } => {
            viewport_center_on(inner, origin, current.zoom, point)
        }
        ViewportRequest::FitContent { padding, .. } => match content {
            Some(c) if c.w > 0.0 || c.h > 0.0 => {
                let avail_w = (inner.w - 2.0 * padding).max(1.0);
                let avail_h = (inner.h - 2.0 * padding).max(1.0);
                let mut zoom = f32::INFINITY;
                if c.w > 0.0 {
                    zoom = zoom.min(avail_w / c.w);
                }
                if c.h > 0.0 {
                    zoom = zoom.min(avail_h / c.h);
                }
                if !zoom.is_finite() {
                    return current;
                }
                let zoom = zoom.clamp(cfg.min_zoom, cfg.max_zoom);
                viewport_center_on(inner, origin, zoom, (c.center_x(), c.center_y()))
            }
            _ => current,
        },
    }
}

/// A view at `zoom` whose pan places content-space `point` at the center
/// of the viewport `inner` rect.
fn viewport_center_on(
    inner: Rect,
    origin: (f32, f32),
    zoom: f32,
    point: (f32, f32),
) -> crate::viewport::ViewportView {
    crate::viewport::ViewportView {
        pan: (
            inner.center_x() - origin.0 - zoom * (point.0 - origin.0),
            inner.center_y() - origin.1 - zoom * (point.1 - origin.1),
        ),
        zoom,
    }
}

/// Clamp `view.pan` so the transformed content bbox can't be dragged out
/// of the viewport: when the content is larger than the viewport on an
/// axis, no empty gutter is allowed (you can't scroll past the content
/// edges); when smaller, the content stays fully inside.
fn clamp_viewport_pan(
    view: &mut crate::viewport::ViewportView,
    inner: Rect,
    origin: (f32, f32),
    content: Rect,
) {
    let (lx, ty) = view.project((content.x, content.y), origin);
    let w = content.w * view.zoom;
    let h = content.h * view.zoom;
    view.pan.0 += clamp_axis_delta(lx, lx + w, inner.x, inner.right(), w, inner.w);
    view.pan.1 += clamp_axis_delta(ty, ty + h, inner.y, inner.bottom(), h, inner.h);
}

/// Pan delta on one axis that brings `[lo, hi]` into the allowed relation
/// with the viewport `[vlo, vhi]`. Returns `0.0` when already valid.
fn clamp_axis_delta(lo: f32, hi: f32, vlo: f32, vhi: f32, size: f32, vsize: f32) -> f32 {
    if size <= vsize {
        // Content fits: keep it inside the viewport.
        if lo < vlo {
            vlo - lo
        } else if hi > vhi {
            vhi - hi
        } else {
            0.0
        }
    } else {
        // Content overflows: don't allow a gutter past either edge.
        if lo > vlo {
            vlo - lo
        } else if hi < vhi {
            vhi - hi
        } else {
            0.0
        }
    }
}

fn scroll_anchor_offset(node: &El, inner: Rect, stored: f32, ui_state: &UiState) -> Option<f32> {
    let anchor = ui_state.scroll.scroll_anchors.get(&node.computed_id)?;
    let rect = ui_state.layout.computed_rects.get(&anchor.node_id)?;
    if rect.h <= 0.0 {
        return None;
    }
    let rect_point = rect.h * anchor.rect_fraction.clamp(0.0, 1.0);
    let scroll_delta = stored - anchor.resolved_offset;
    let viewport_y = anchor.viewport_y - scroll_delta;
    Some(rect.y - inner.y + rect_point - viewport_y)
}

fn choose_scroll_anchor(
    node: &El,
    inner: Rect,
    offset: f32,
    ui_state: &UiState,
) -> Option<ScrollAnchor> {
    if inner.h <= 0.0 {
        return None;
    }
    let target_y = inner.y + inner.h * 0.25;
    let mut best = None;
    for child in &node.children {
        choose_scroll_anchor_in_subtree(child, inner, target_y, 1, ui_state, &mut best);
    }
    let candidate = best?;
    let anchor_y = target_y.clamp(candidate.rect.y, candidate.rect.bottom());
    let rect_fraction = if candidate.rect.h > 0.0 {
        ((anchor_y - candidate.rect.y) / candidate.rect.h).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(ScrollAnchor {
        node_id: candidate.node_id,
        rect_fraction,
        viewport_y: anchor_y - inner.y,
        resolved_offset: offset,
    })
}

#[derive(Clone, Debug)]
struct ScrollAnchorCandidate {
    node_id: String,
    rect: Rect,
    distance: f32,
    depth: usize,
}

fn choose_scroll_anchor_in_subtree(
    node: &El,
    inner: Rect,
    target_y: f32,
    depth: usize,
    ui_state: &UiState,
    best: &mut Option<ScrollAnchorCandidate>,
) {
    let Some(rect) = ui_state
        .layout
        .computed_rects
        .get(&node.computed_id)
        .copied()
    else {
        return;
    };
    if rect.w > 0.0 && rect.h > 0.0 && rect.bottom() > inner.y && rect.y < inner.bottom() {
        let distance = distance_to_interval(target_y, rect.y, rect.bottom());
        let candidate = ScrollAnchorCandidate {
            node_id: node.computed_id.clone(),
            rect,
            distance,
            depth,
        };
        let replace = best.as_ref().is_none_or(|current| {
            candidate.distance < current.distance
                || (candidate.distance == current.distance && candidate.depth > current.depth)
                || (candidate.distance == current.distance
                    && candidate.depth == current.depth
                    && candidate.rect.h < current.rect.h)
        });
        if replace {
            *best = Some(candidate);
        }
    }

    if node.scrollable {
        return;
    }
    for child in &node.children {
        choose_scroll_anchor_in_subtree(child, inner, target_y, depth + 1, ui_state, best);
    }
}

/// Stored offset within this much of the pinned edge counts as "at
/// the edge" for [`crate::tree::PinPolicy`] activation. Wheel deltas
/// are integer pixels, so a half-pixel slack absorbs floating-point
/// rounding without admitting any deliberate user scroll.
const PIN_EPSILON: f32 = 0.5;

/// Decide whether the pin should be engaged this frame, given the
/// previous frame's active flag and reference offset.
///
/// `policy` selects the edge: `End` references the previous frame's
/// `max_offset` (stored in `scroll.pin_prev_max`) and engages when
/// `stored ≈ prev_max`; `Start` references `0` and engages when
/// `stored ≈ 0`. `None` returns `None` so callers can short-circuit.
fn pin_would_be_active(
    node: &El,
    stored: f32,
    _max_offset: f32,
    ui_state: &UiState,
) -> Option<bool> {
    let prev_active = ui_state.scroll.pin_active.get(&node.computed_id).copied();
    match node.pin_policy {
        crate::tree::PinPolicy::None => None,
        crate::tree::PinPolicy::End => {
            let prev_max = ui_state.scroll.pin_prev_max.get(&node.computed_id).copied();
            Some(match prev_active {
                None => true,
                Some(prev) => {
                    let prev_max = prev_max.unwrap_or(0.0);
                    if prev && stored < prev_max - PIN_EPSILON {
                        false
                    } else if !prev && prev_max > 0.0 && stored >= prev_max - PIN_EPSILON {
                        true
                    } else {
                        prev
                    }
                }
            })
        }
        crate::tree::PinPolicy::Start => Some(match prev_active {
            None => true,
            Some(prev) => {
                if prev && stored > PIN_EPSILON {
                    false
                } else if !prev && stored <= PIN_EPSILON {
                    true
                } else {
                    prev
                }
            }
        }),
    }
}

/// Apply this node's [`crate::tree::PinPolicy`] to `stored`. Reads the
/// previous frame's bookkeeping from `scroll.pin_active` /
/// `scroll.pin_prev_max` to decide whether the stored offset has moved
/// off the pinned edge since last frame (user wheel / drag /
/// programmatic write), and updates those maps accordingly. Returns
/// the offset that should be clamped + written downstream — the
/// pinned edge (`0` for `Start`, `max_offset` for `End`) when engaged,
/// the input `stored` otherwise.
///
/// First frame for an opted-in container starts pinned, so a freshly
/// mounted `scroll([...]).pin_end()` paints with its tail visible and
/// `scroll([...]).pin_start()` paints (still) with its head visible.
fn resolve_pin(node: &El, stored: f32, max_offset: f32, ui_state: &mut UiState) -> f32 {
    if matches!(node.pin_policy, crate::tree::PinPolicy::None) {
        ui_state.scroll.pin_active.remove(&node.computed_id);
        ui_state.scroll.pin_prev_max.remove(&node.computed_id);
        return stored;
    }
    let active = pin_would_be_active(node, stored, max_offset, ui_state).unwrap_or(false);
    ui_state
        .scroll
        .pin_active
        .insert(node.computed_id.clone(), active);
    match node.pin_policy {
        crate::tree::PinPolicy::End => {
            ui_state
                .scroll
                .pin_prev_max
                .insert(node.computed_id.clone(), max_offset);
            if active { max_offset } else { stored }
        }
        crate::tree::PinPolicy::Start => {
            // `pin_prev_max` is unused for Start — drop any stale entry
            // (e.g. if the policy was just flipped from End to Start
            // for the same computed_id).
            ui_state.scroll.pin_prev_max.remove(&node.computed_id);
            if active { 0.0 } else { stored }
        }
        crate::tree::PinPolicy::None => unreachable!(),
    }
}

/// Walk pending `ScrollRequest::EnsureVisible` requests and pop any
/// whose `container_key` resolves to an ancestor of `node`. For each
/// match, write a stored offset that brings the request's content-
/// space `y..y+h` range into the viewport using minimal-displacement
/// semantics (top edge if above, bottom edge if below, leave alone if
/// already inside). The clamp + shift downstream of this call ensures
/// the resulting offset stays inside `[0, max_offset]`.
///
/// Matching is by computed-id prefix on the keyed ancestor — a
/// scroll is "inside" the keyed widget when its id starts with the
/// ancestor's id followed by `.`, the same rule used by
/// [`crate::state::query::target_in_subtree`].
fn resolve_ensure_visible_for_scroll(
    node: &El,
    inner: Rect,
    content_h: f32,
    ui_state: &mut UiState,
) -> bool {
    if ui_state.scroll.pending_requests.is_empty() {
        return false;
    }
    let pending = std::mem::take(&mut ui_state.scroll.pending_requests);
    let mut remaining: Vec<ScrollRequest> = Vec::with_capacity(pending.len());
    let mut wrote = false;
    for req in pending {
        let ScrollRequest::EnsureVisible {
            container_key,
            y,
            h,
        } = &req
        else {
            remaining.push(req);
            continue;
        };
        let Some(ancestor_id) = ui_state.layout.key_index.get(container_key) else {
            // Container hasn't been laid out yet (or its key isn't
            // in this tree). Keep the request for a future frame —
            // dropped at end-of-frame like row requests for
            // missing lists.
            remaining.push(req);
            continue;
        };
        // Match this scroll only if it sits inside the keyed widget.
        // Same prefix rule as `target_in_subtree`.
        let inside = node.computed_id == *ancestor_id
            || node
                .computed_id
                .strip_prefix(ancestor_id.as_str())
                .is_some_and(|rest| rest.starts_with('.'));
        if !inside {
            remaining.push(req);
            continue;
        }
        let current = ui_state
            .scroll
            .offsets
            .get(&node.computed_id)
            .copied()
            .unwrap_or(0.0);
        let target_top = *y;
        let target_bottom = *y + *h;
        let viewport_h = inner.h;
        // Minimal-displacement: if the range is fully visible, no
        // change. If it's above the viewport top, scroll up to it.
        // If it's below the viewport bottom, scroll just enough to
        // expose the bottom edge — but never less than 0 or more
        // than `content_h - viewport_h` (the clamp downstream will
        // do that anyway).
        let new_offset = if target_top < current {
            target_top
        } else if target_bottom > current + viewport_h {
            target_bottom - viewport_h
        } else {
            // Already visible: don't override an in-progress
            // manual scroll just because the caret happens to be
            // mid-viewport. Skip this request without disturbing
            // the offset.
            continue;
        };
        // Clamp against the live content extent so we don't write
        // a wildly-out-of-range offset when the request races a
        // layout pass that hasn't yet measured all rows.
        let max = (content_h - viewport_h).max(0.0);
        let new_offset = new_offset.clamp(0.0, max);
        ui_state
            .scroll
            .offsets
            .insert(node.computed_id.clone(), new_offset);
        wrote = true;
    }
    ui_state.scroll.pending_requests = remaining;
    wrote
}

/// Compute and store the scrollbar thumb + track rects for `node`
/// when the author opted into a visible scrollbar AND content
/// overflows. Both rects are anchored to the right edge of `inner`.
/// The visible thumb is `SCROLLBAR_THUMB_WIDTH` wide and tracks the
/// scroll offset; the track is `SCROLLBAR_HITBOX_WIDTH` wide and
/// covers the full inner height so a press above/below the thumb
/// can page-scroll.
fn write_thumb_rect(
    node: &El,
    inner: Rect,
    content_h: f32,
    max_offset: f32,
    offset: f32,
    ui_state: &mut UiState,
) {
    if !node.scrollbar || max_offset <= 0.0 || inner.h <= 0.0 || content_h <= 0.0 {
        return;
    }
    let thumb_w = crate::tokens::SCROLLBAR_THUMB_WIDTH;
    let track_w = crate::tokens::SCROLLBAR_HITBOX_WIDTH;
    let track_inset = crate::tokens::SCROLLBAR_TRACK_INSET;
    let min_thumb_h = crate::tokens::SCROLLBAR_THUMB_MIN_H;
    let thumb_h = ((inner.h * inner.h / content_h).max(min_thumb_h)).min(inner.h);
    let track_remaining = (inner.h - thumb_h).max(0.0);
    let thumb_y = inner.y + track_remaining * (offset / max_offset);
    // `scrollbar_gutter` reserved a content-free band on the right via
    // extra padding (metrics pass). The thumb belongs *inside* that
    // band, not at the now-narrower content edge — anchor it where the
    // content edge would be without the gutter, i.e. the band's right.
    let edge = if node.scrollbar_gutter {
        inner.right() + crate::tokens::SCROLLBAR_GUTTER
    } else {
        inner.right()
    };
    let thumb_x = edge - thumb_w - track_inset;
    let track_x = edge - track_w - track_inset;
    ui_state.scroll.thumb_rects.insert(
        node.computed_id.clone(),
        Rect::new(thumb_x, thumb_y, thumb_w, thumb_h),
    );
    ui_state.scroll.thumb_tracks.insert(
        node.computed_id.clone(),
        Rect::new(track_x, inner.y, track_w, inner.h),
    );
}

fn shift_subtree_y(node: &El, dy: f32, ui_state: &mut UiState) {
    if let Some(rect) = ui_state.layout.computed_rects.get_mut(&node.computed_id) {
        rect.y += dy;
    }
    if let Some(thumb) = ui_state.scroll.thumb_rects.get_mut(&node.computed_id) {
        thumb.y += dy;
    }
    if let Some(track) = ui_state.scroll.thumb_tracks.get_mut(&node.computed_id) {
        track.y += dy;
    }
    for c in &node.children {
        shift_subtree_y(c, dy, ui_state);
    }
}

fn layout_axis(node: &mut El, node_rect: Rect, vertical: bool, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    let n = node.children.len();
    if n == 0 {
        return;
    }

    let total_gap = node.gap * n.saturating_sub(1) as f32;
    let main_extent = if vertical { inner.h } else { inner.w };
    let cross_extent = if vertical { inner.w } else { inner.h };

    let intrinsics: Vec<(f32, f32)> = {
        crate::profile_span!("layout::axis::intrinsics");
        if vertical {
            // Column layout: each child measures at the parent's cross
            // (width) extent, so wrap-text children see the width they
            // will actually paint at. `child_intrinsic` already threads
            // this through.
            node.children
                .iter()
                .map(|c| child_intrinsic(c, vertical, cross_extent, node.align))
                .collect()
        } else {
            // Row layout: mirror the two-pass measurement in
            // `intrinsic_constrained_uncached`'s Row branch so a `Fill`
            // child with `wrap_text` descendants reports the height it
            // will actually paint at — not its single-line unwrapped
            // intrinsic. Without this, e.g. a `row([column([paragraph,
            // paragraph]).fill_width(), fixed])` shape sizes the
            // column rect at the unwrapped height, and the wrapped
            // text inside overflows the column vertically (the
            // `Overflow B=N` shape that motivated this fix).
            row_child_intrinsics(node, main_extent)
        }
    };

    // `main_size_of` resolves main-axis size directly from each child's
    // own sizing intent and its intrinsic. `Size::Aspect` on the main
    // axis is the one case that needs more context: it derives main
    // from the *resolved cross size*, which means we have to compute
    // cross first for that child only (an inversion of the normal
    // main-then-cross ordering). Cross resolution doesn't depend on
    // main except when cross is also Aspect — that's the degenerate
    // both-Aspect case and falls back to intrinsic via main_size_of.
    let resolve_main = |c: &El, iw: f32, ih: f32| -> MainSize {
        let main_intent = if vertical { c.height } else { c.width };
        if let Size::Aspect(r) = main_intent {
            let cross_intent = if vertical { c.width } else { c.height };
            if !matches!(cross_intent, Size::Aspect(_)) {
                let cross_intrinsic = if vertical { iw } else { ih };
                let cross_size = match cross_intent {
                    Size::Fixed(v) => v,
                    Size::Hug | Size::Fill(_) => match node.align {
                        Align::Stretch => cross_extent,
                        Align::Start | Align::Center | Align::End => cross_intrinsic,
                    },
                    Size::Aspect(_) => unreachable!(),
                };
                let cross_size = if vertical {
                    clamp_w(c, cross_size)
                } else {
                    clamp_h(c, cross_size)
                };
                let main = cross_size * r.max(0.0);
                let clamped = if vertical {
                    clamp_h(c, main)
                } else {
                    clamp_w(c, main)
                };
                return MainSize::Resolved(clamped);
            }
        }
        main_size_of(c, iw, ih, vertical)
    };

    // Resolve every child's main size up front. Fill children share
    // the free space by weight, with CSS-flex parity for min/max
    // clamps: a fill whose share violates its clamp is frozen at the
    // clamped size, and the space it freed (max) or stole (min) is
    // re-distributed among the still-flexible fills, iterating until
    // a distribution sticks. Each round computes every hypothetical
    // share against the same `remaining` / weight total, then freezes
    // all of that round's violators at once — freezing as it scans
    // would let an early max-freeze spuriously min-freeze a later
    // sibling against a half-updated distribution.
    let mut main_sizes: Vec<f32> = Vec::with_capacity(n);
    // `Some(weight)` = still-flexible fill; cleared as fills freeze.
    let mut fill_weights: Vec<Option<f32>> = Vec::with_capacity(n);
    let mut consumed = 0.0;
    for (c, (iw, ih)) in node.children.iter().zip(intrinsics.iter()) {
        match resolve_main(c, *iw, *ih) {
            MainSize::Resolved(v) => {
                consumed += v;
                main_sizes.push(v);
                fill_weights.push(None);
            }
            MainSize::Fill(w) => {
                main_sizes.push(0.0);
                fill_weights.push(Some(w.max(0.001)));
            }
        }
    }
    let mut remaining = (main_extent - consumed - total_gap).max(0.0);
    loop {
        let flexible_weight: f32 = fill_weights.iter().flatten().sum();
        if flexible_weight == 0.0 {
            break;
        }
        let mut frozen_any = false;
        let mut newly_frozen = 0.0;
        for (i, c) in node.children.iter().enumerate() {
            let Some(w) = fill_weights[i] else { continue };
            let raw = remaining * w / flexible_weight;
            let clamped = if vertical {
                clamp_h(c, raw)
            } else {
                clamp_w(c, raw)
            };
            main_sizes[i] = clamped;
            if clamped != raw {
                fill_weights[i] = None;
                frozen_any = true;
                newly_frozen += clamped;
            }
        }
        if !frozen_any {
            // The flexible fills absorbed all the free space.
            remaining = 0.0;
            break;
        }
        remaining = (remaining - newly_frozen).max(0.0);
    }

    // Free space after children + gaps that the fills could not absorb
    // (no fills, or every fill frozen at its max). This is what
    // justify distributes — mirroring flexbox's leftover free space.
    let free_after_used = remaining;
    let mut cursor = match node.justify {
        Justify::Start => 0.0,
        Justify::Center => free_after_used * 0.5,
        Justify::End => free_after_used,
        Justify::SpaceBetween => 0.0,
    };
    let between_extra = if matches!(node.justify, Justify::SpaceBetween) && n > 1 {
        free_after_used / (n - 1) as f32
    } else {
        0.0
    };
    let scroll_visible = scroll_visible_content_rect(node, inner, vertical, ui_state);

    crate::profile_span!("layout::axis::place");
    for (i, (c, (iw, ih))) in node.children.iter_mut().zip(intrinsics).enumerate() {
        let main_size = main_sizes[i];

        let cross_intent = if vertical { c.width } else { c.height };
        let cross_intrinsic = if vertical { iw } else { ih };
        // CSS-flex parity for cross-axis sizing: `Size::Fixed` is an
        // explicit author override and always wins. Otherwise the
        // parent's `Align` decides — `Stretch` (the column default)
        // stretches non-fixed children to the container, `Center` /
        // `Start` / `End` shrink to intrinsic so the alignment can
        // actually position them. This collapses Hug and Fill on the
        // cross axis (both are "follow align-items"), the same way
        // CSS flex doesn't distinguish between them on the cross axis.
        // `Aspect` derives cross from the already-resolved main. The
        // symmetric case (Aspect on main) is handled by `resolve_main`
        // above, which inverts the ordering for that child only.
        let cross_size = match cross_intent {
            Size::Fixed(v) => v,
            Size::Aspect(r) => main_size * r,
            Size::Hug | Size::Fill(_) => match node.align {
                Align::Stretch => cross_extent,
                Align::Start | Align::Center | Align::End => cross_intrinsic,
            },
        };
        let cross_size = if vertical {
            clamp_w(c, cross_size)
        } else {
            clamp_h(c, cross_size)
        };

        let cross_off = match node.align {
            Align::Start | Align::Stretch => 0.0,
            Align::Center => (cross_extent - cross_size) * 0.5,
            Align::End => cross_extent - cross_size,
        };

        let c_rect = if vertical {
            Rect::new(inner.x + cross_off, inner.y + cursor, cross_size, main_size)
        } else {
            Rect::new(inner.x + cursor, inner.y + cross_off, main_size, cross_size)
        };
        ui_state
            .layout
            .computed_rects
            .insert(c.computed_id.clone(), c_rect);
        if can_prune_scroll_child(c, c_rect, scroll_visible) {
            let nodes = zero_descendant_rects(c, c_rect, ui_state);
            record_pruned_subtree(nodes);
        } else {
            layout_children(c, c_rect, ui_state);
        }

        cursor += main_size + node.gap + if i + 1 < n { between_extra } else { 0.0 };
    }
}

const SCROLL_LAYOUT_PRUNE_OVERSCAN: f32 = 256.0;

fn scroll_visible_content_rect(
    node: &El,
    inner: Rect,
    vertical: bool,
    ui_state: &UiState,
) -> Option<Rect> {
    if !vertical || !node.scrollable || !matches!(node.pin_policy, crate::tree::PinPolicy::None) {
        return None;
    }
    let offset = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    Some(Rect::new(
        inner.x,
        inner.y + offset - SCROLL_LAYOUT_PRUNE_OVERSCAN,
        inner.w,
        inner.h + 2.0 * SCROLL_LAYOUT_PRUNE_OVERSCAN,
    ))
}

fn can_prune_scroll_child(child: &El, child_rect: Rect, visible: Option<Rect>) -> bool {
    let Some(visible) = visible else {
        return false;
    };
    child_rect.intersect(visible).is_none() && subtree_is_layout_confined(child)
}

fn subtree_is_layout_confined(node: &El) -> bool {
    if node.translate != (0.0, 0.0)
        || node.scale != 1.0
        || node.shadow > 0.0
        || node.paint_overflow != Sides::zero()
        || node.hit_overflow != Sides::zero()
        || node.layout_override.is_some()
        || node.virtual_items.is_some()
    {
        return false;
    }
    node.children.iter().all(subtree_is_layout_confined)
}

fn zero_descendant_rects(node: &El, rect: Rect, ui_state: &mut UiState) -> u64 {
    let mut count = 0;
    let zero = Rect::new(rect.x, rect.y, 0.0, 0.0);
    for child in &node.children {
        ui_state
            .layout
            .computed_rects
            .insert(child.computed_id.clone(), zero);
        count += 1 + zero_descendant_rects(child, zero, ui_state);
    }
    count
}

fn record_pruned_subtree(nodes: u64) {
    INTRINSIC_CACHE.with(|cell| {
        if let Some(cache) = cell.borrow_mut().as_mut() {
            cache.prune.subtrees += 1;
            cache.prune.nodes += nodes;
        }
    });
}

enum MainSize {
    Resolved(f32),
    Fill(f32),
}

fn main_size_of(c: &El, iw: f32, ih: f32, vertical: bool) -> MainSize {
    let s = if vertical { c.height } else { c.width };
    let intr = if vertical { ih } else { iw };
    let clamp = |v: f32| {
        if vertical {
            clamp_h(c, v)
        } else {
            clamp_w(c, v)
        }
    };
    match s {
        Size::Fixed(v) => MainSize::Resolved(clamp(v)),
        Size::Hug => MainSize::Resolved(clamp(intr)),
        Size::Fill(w) => MainSize::Fill(w),
        // Main-axis Aspect needs the resolved cross size to compute
        // `cross * r`. That requires inverting the normal main-then-
        // cross order and is handled by `layout_axis`'s `resolve_main`
        // closure. If we ever reach this arm (e.g. from a path that
        // bypasses `resolve_main`), fall back to intrinsic so the El
        // still has a finite measure.
        Size::Aspect(_) => MainSize::Resolved(clamp(intr)),
    }
}

fn child_intrinsic(
    c: &El,
    vertical: bool,
    parent_cross_extent: f32,
    parent_align: Align,
) -> (f32, f32) {
    if !vertical {
        return intrinsic(c);
    }
    let available_width = match c.width {
        Size::Fixed(v) => Some(v),
        Size::Fill(_) => Some(parent_cross_extent),
        Size::Hug => match parent_align {
            Align::Stretch => Some(parent_cross_extent),
            Align::Start | Align::Center | Align::End => Some(parent_cross_extent),
        },
        // Aspect width derives from height; we don't know height yet
        // at intrinsic time. Cap text wrap at the parent's cross
        // extent so wrappable content doesn't unwrap unnecessarily;
        // the Aspect post-step in `intrinsic_constrained` overrides
        // the returned width with `ih * r` anyway.
        Size::Aspect(_) => Some(parent_cross_extent),
    };
    intrinsic_constrained(c, available_width)
}

/// Per-child intrinsics for a horizontal-axis (row) parent, using the
/// same two-pass distribution as `intrinsic_constrained_uncached`'s
/// Row branch. First pass measures Fixed and Hug widths unconstrained
/// (Hug naturally takes its intrinsic; Fixed self-resolves); second
/// pass distributes leftover main-axis space across Fill children and
/// re-measures each with its allocated width so wrap-text descendants
/// shape at the width they will actually paint at, not their unwrapped
/// intrinsic. `inner_main_extent` is the row's padded inner width.
fn row_child_intrinsics(node: &El, inner_main_extent: f32) -> Vec<(f32, f32)> {
    let n = node.children.len();
    let total_gap = node.gap * n.saturating_sub(1) as f32;

    let mut first: Vec<Option<(f32, f32)>> = Vec::with_capacity(n);
    let mut consumed: f32 = 0.0;
    let mut fill_weight_total: f32 = 0.0;
    for c in &node.children {
        match c.width {
            Size::Fill(w) => {
                fill_weight_total += w.max(0.001);
                first.push(None);
            }
            _ => {
                let (iw, ih) = intrinsic(c);
                consumed += iw;
                first.push(Some((iw, ih)));
            }
        }
    }

    let fill_remaining = (inner_main_extent - consumed - total_gap).max(0.0);

    node.children
        .iter()
        .zip(first)
        .map(|(c, slot)| match slot {
            Some(rc) => rc,
            None => {
                let weight = match c.width {
                    Size::Fill(w) => w.max(0.001),
                    _ => 1.0,
                };
                let av = if fill_weight_total > 0.0 {
                    fill_remaining * weight / fill_weight_total
                } else {
                    fill_remaining
                };
                intrinsic_constrained(c, Some(av))
            }
        })
        .collect()
}

fn overlay_rect(c: &El, parent: Rect, align: Align, justify: Justify) -> Rect {
    // Wrap-text height depends on width, so constrain the intrinsic
    // measurement to the width the child will actually be laid out at
    // — same shape as `child_intrinsic` does for column/row children.
    // Without this, a Fixed-width modal with a wrappable paragraph
    // measures as a single-line block and the modal's Hug height ends
    // up shorter than the actual content needs, eating bottom padding.
    let constrained_width = match c.width {
        Size::Fixed(v) => Some(v),
        Size::Fill(_) | Size::Hug => Some(parent.w),
        // Width derives from height — let the intrinsic post-step
        // override iw; don't pre-constrain text wrap to parent.w.
        Size::Aspect(_) => None,
    };
    let (iw, ih) = intrinsic_constrained(c, constrained_width);
    // Overlay isn't main/cross-asymmetric, so Aspect can resolve on
    // either axis here. Resolve the non-Aspect axis first, then derive
    // the Aspect axis. If both are Aspect (degenerate), fall back to
    // intrinsic for both.
    let (w, h) = match (c.width, c.height) {
        (Size::Aspect(_), Size::Aspect(_)) => (iw.min(parent.w), ih.min(parent.h)),
        (Size::Aspect(r), _) => {
            let h = match c.height {
                Size::Fixed(v) => v,
                Size::Hug => ih.min(parent.h),
                Size::Fill(_) => parent.h,
                Size::Aspect(_) => unreachable!(),
            };
            (h * r, h)
        }
        (_, Size::Aspect(r)) => {
            let w = match c.width {
                Size::Fixed(v) => v,
                Size::Hug => iw.min(parent.w),
                Size::Fill(_) => parent.w,
                Size::Aspect(_) => unreachable!(),
            };
            (w, w * r)
        }
        _ => {
            let w = match c.width {
                Size::Fixed(v) => v,
                Size::Hug => iw.min(parent.w),
                Size::Fill(_) => parent.w,
                Size::Aspect(_) => unreachable!(),
            };
            let h = match c.height {
                Size::Fixed(v) => v,
                Size::Hug => ih.min(parent.h),
                Size::Fill(_) => parent.h,
                Size::Aspect(_) => unreachable!(),
            };
            (w, h)
        }
    };
    let w = clamp_w(c, w);
    let h = clamp_h(c, h);
    let x = match align {
        Align::Start | Align::Stretch => parent.x,
        Align::Center => parent.x + (parent.w - w) * 0.5,
        Align::End => parent.right() - w,
    };
    let y = match justify {
        Justify::Start | Justify::SpaceBetween => parent.y,
        Justify::Center => parent.y + (parent.h - h) * 0.5,
        Justify::End => parent.bottom() - h,
    };
    Rect::new(x, y, w, h)
}

/// Intrinsic (width, height) for hugging layouts.
pub fn intrinsic(c: &El) -> (f32, f32) {
    intrinsic_constrained(c, None)
}

fn intrinsic_constrained(c: &El, available_width: Option<f32>) -> (f32, f32) {
    // A node's own Fixed width beats whatever the ancestor chain has
    // available: layout will resolve the node at exactly that width,
    // so measuring at any other width makes Hug ancestors disagree
    // with the final wrap of text descendants (issue #47). Applied
    // before the cache key so all callers unify on one entry.
    let available_width = match c.width {
        Size::Fixed(v) => Some(v),
        _ => available_width,
    };
    let key = intrinsic_cache_key(c, available_width);
    if let Some(key) = &key
        && let Some(cached) = INTRINSIC_CACHE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let cache = slot.as_mut()?;
            let cached = cache.measurements.get(key).copied();
            if cached.is_some() {
                cache.stats.hits += 1;
            }
            cached
        })
    {
        return cached;
    }

    if key.is_some() {
        INTRINSIC_CACHE.with(|cell| {
            if let Some(cache) = cell.borrow_mut().as_mut() {
                cache.stats.misses += 1;
            }
        });
    }

    let measured = apply_aspect(
        c,
        available_width,
        intrinsic_constrained_uncached(c, available_width),
    );

    if let Some(key) = key {
        INTRINSIC_CACHE.with(|cell| {
            if let Some(cache) = cell.borrow_mut().as_mut() {
                cache.measurements.insert(key, measured);
            }
        });
    }

    measured
}

/// Apply `Size::Aspect` to a freshly-measured intrinsic by deriving the
/// aspect-locked axis from the other axis. Runs after the inner intrinsic
/// pass so it composes with any content type (image, text, container).
///
/// When the *other* axis is `Fill`, the layout-time size of that axis is
/// the parent's available extent, not the El's inner intrinsic. Using the
/// inner intrinsic would let a hugging parent under-size and the Aspect-
/// derived axis would then overflow at paint. Prefer `available_width`
/// for Fill width; we don't currently plumb available_height, so a Fill
/// height + Aspect width pairing falls back to inner intrinsic.
///
/// Both axes Aspect is degenerate — fall back to the inner intrinsic so
/// the El still has a finite measure. Negative ratios are clamped to zero
/// for the same reason.
fn apply_aspect(c: &El, available_width: Option<f32>, (iw, ih): (f32, f32)) -> (f32, f32) {
    match (c.width, c.height) {
        (Size::Aspect(_), Size::Aspect(_)) => (iw, ih),
        (Size::Aspect(r), _) => {
            // Basis axis is height; ih is already clamped by apply_min.
            // Clamp the derived width against the El's own min/max so
            // a hugging parent sees the intrinsic that layout will
            // actually paint at.
            (clamp_w(c, ih * r.max(0.0)), ih)
        }
        (_, Size::Aspect(r)) => {
            let raw_basis = match c.width {
                Size::Fixed(v) => v,
                Size::Fill(_) => available_width.unwrap_or(iw),
                Size::Hug | Size::Aspect(_) => iw,
            };
            // Mirror the layout-time ordering in `resolve_main`: clamp
            // the basis by the *basis* axis's min/max first, then derive
            // the other axis and clamp by its own min/max.
            let basis = clamp_w(c, raw_basis);
            (iw, clamp_h(c, basis * r.max(0.0)))
        }
        _ => (iw, ih),
    }
}

fn intrinsic_cache_key(c: &El, available_width: Option<f32>) -> Option<IntrinsicCacheKey> {
    if INTRINSIC_CACHE.with(|cell| cell.borrow().is_none()) {
        return None;
    }
    if c.computed_id.is_empty() {
        return None;
    }
    Some(IntrinsicCacheKey {
        computed_id: c.computed_id.clone(),
        available_width_bits: available_width.map(f32::to_bits),
    })
}

fn intrinsic_constrained_uncached(c: &El, available_width: Option<f32>) -> (f32, f32) {
    if c.layout_override.is_some() {
        // Custom-layout nodes don't define an intrinsic. Authors must
        // size them with `Fixed` or `Fill` on both axes; the returned
        // (0.0, 0.0) is replaced by `apply_min` for `Fixed` and is
        // unread for `Fill` (parent's distribution decides).
        if matches!(c.width, Size::Hug) || matches!(c.height, Size::Hug) {
            panic!(
                "layout_override on {:?} requires Size::Fixed or Size::Fill on both axes; \
                 Size::Hug is not supported for custom layouts",
                c.computed_id,
            );
        }
        return apply_min(c, 0.0, 0.0);
    }
    if c.virtual_items.is_some() {
        // VirtualList sizes the whole viewport (the parent decides) and
        // realizes only on-screen rows. Hug-sizing it would mean
        // "shrink to fit all rows", defeating virtualization. Same
        // shape as the layout_override guard.
        if matches!(c.width, Size::Hug) || matches!(c.height, Size::Hug) {
            panic!(
                "virtual_list on {:?} requires Size::Fixed or Size::Fill on both axes; \
                 Size::Hug would defeat virtualization",
                c.computed_id,
            );
        }
        return apply_min(c, 0.0, 0.0);
    }
    if matches!(c.kind, Kind::Inlines) {
        return inline_paragraph_intrinsic(c, available_width);
    }
    if matches!(c.kind, Kind::HardBreak) {
        // HardBreak is meaningful only inside Inlines (where draw_ops
        // encodes it as `\n` in the attributed text). Outside Inlines
        // it's a no-op layout-wise.
        return apply_min(c, 0.0, 0.0);
    }
    if matches!(c.kind, Kind::Math) {
        if let Some(expr) = &c.math {
            let layout = crate::math::layout_math(expr, c.font_size, c.math_display);
            return apply_min(
                c,
                layout.width + c.padding.left + c.padding.right,
                layout.height() + c.padding.top + c.padding.bottom,
            );
        }
        return apply_min(c, 0.0, 0.0);
    }
    if c.icon.is_some() {
        return apply_min(
            c,
            c.font_size + c.padding.left + c.padding.right,
            c.font_size + c.padding.top + c.padding.bottom,
        );
    }
    if let Some(img) = &c.image {
        // Natural pixel size as a logical-pixel intrinsic. Authors who
        // want a different sized box set `.width()` / `.height()`;
        // the projection inside that box is decided by `image_fit`.
        let w = img.width() as f32 + c.padding.left + c.padding.right;
        let h = img.height() as f32 + c.padding.top + c.padding.bottom;
        return apply_min(c, w, h);
    }
    if let Some(text) = &c.text {
        let content_available = match c.text_wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => available_width
                .or(match c.width {
                    Size::Fixed(v) => Some(v),
                    // Aspect-on-text would be circular (text height
                    // depends on wrap width which would depend on
                    // text height). Treat like Hug — no wrap cap.
                    Size::Fill(_) | Size::Hug | Size::Aspect(_) => None,
                })
                .map(|w| (w - c.padding.left - c.padding.right).max(1.0)),
        };
        let display = display_text_for_measure(c, text, content_available);
        let layout = text_metrics::layout_text_with_line_height_and_family(
            &display,
            c.font_size,
            c.line_height,
            c.font_family,
            c.font_weight,
            c.font_mono,
            c.text_tabular_numerals,
            c.text_wrap,
            content_available,
        );
        let w = match (content_available, c.width) {
            (Some(available), Size::Hug | Size::Aspect(_)) => {
                let unwrapped = text_metrics::layout_text_with_family(
                    text,
                    c.font_size,
                    c.font_family,
                    c.font_weight,
                    c.font_mono,
                    c.text_tabular_numerals,
                    TextWrap::NoWrap,
                    None,
                );
                unwrapped.width.min(available) + c.padding.left + c.padding.right
            }
            (Some(available), Size::Fixed(_) | Size::Fill(_)) => {
                available + c.padding.left + c.padding.right
            }
            (None, _) => layout.width + c.padding.left + c.padding.right,
        };
        let h = layout.height + c.padding.top + c.padding.bottom;
        return apply_min(c, w, h);
    }
    match c.axis {
        Axis::Overlay => {
            let mut w: f32 = 0.0;
            let mut h: f32 = 0.0;
            for ch in &c.children {
                let child_available =
                    available_width.map(|w| (w - c.padding.left - c.padding.right).max(0.0));
                let (cw, chh) = intrinsic_constrained(ch, child_available);
                w = w.max(cw);
                h = h.max(chh);
            }
            apply_min(
                c,
                w + c.padding.left + c.padding.right,
                h + c.padding.top + c.padding.bottom,
            )
        }
        Axis::Column => {
            let mut w: f32 = 0.0;
            let mut h: f32 = c.padding.top + c.padding.bottom;
            let n = c.children.len();
            let child_available =
                available_width.map(|w| (w - c.padding.left - c.padding.right).max(0.0));
            for (i, ch) in c.children.iter().enumerate() {
                let (cw, chh) = intrinsic_constrained(ch, child_available);
                w = w.max(cw);
                h += chh;
                if i + 1 < n {
                    h += c.gap;
                }
            }
            apply_min(c, w + c.padding.left + c.padding.right, h)
        }
        Axis::Row => {
            // Two-pass measurement so that wrappable Fill children see
            // the width they will actually be laid out at. Without
            // this, a `Size::Fill` paragraph inside a row falls through
            // `inline_paragraph_intrinsic`'s `available_width` fallback
            // with `None` and reports its unwrapped single-line height
            // — the row then under-reserves vertical space and the
            // wrapped text overflows downward into the next row. This
            // mirrors how `layout_axis` (the runtime pass) already
            // splits Resolved vs. Fill main-axis sizing.
            let n = c.children.len();
            let total_gap = c.gap * n.saturating_sub(1) as f32;
            let inner_available = available_width
                .map(|w| (w - c.padding.left - c.padding.right - total_gap).max(0.0));

            // First pass: Fixed and Hug children measure unconstrained.
            // Fixed-width wrappable children self-resolve their wrap
            // width via `inline_paragraph_intrinsic`'s own Fixed
            // fallback; Hug children take their natural width. We only
            // need to feed an explicit available width to Fill.
            let mut consumed: f32 = 0.0;
            let mut fill_weight_total: f32 = 0.0;
            let mut sizes: Vec<Option<(f32, f32)>> = Vec::with_capacity(n);
            for ch in &c.children {
                match ch.width {
                    Size::Fill(w) => {
                        fill_weight_total += w.max(0.001);
                        sizes.push(None);
                    }
                    _ => {
                        let (cw, chh) = intrinsic(ch);
                        consumed += cw;
                        sizes.push(Some((cw, chh)));
                    }
                }
            }

            // Second pass: distribute the leftover among Fill children
            // by weight and remeasure each with its share. Without an
            // available_width hint (row inside a Hug ancestor with no
            // outer constraint) we fall back to unconstrained
            // measurement — same lossy shape as the prior code, but
            // limited to the case where there's genuinely no width to
            // distribute.
            let fill_remaining = inner_available.map(|av| (av - consumed).max(0.0));
            let mut w_total: f32 = c.padding.left + c.padding.right;
            let mut h_max: f32 = 0.0;
            for (i, (ch, slot)) in c.children.iter().zip(sizes).enumerate() {
                let (cw, chh) = match slot {
                    Some(rc) => rc,
                    None => match (fill_remaining, fill_weight_total > 0.0) {
                        (Some(av), true) => {
                            let weight = match ch.width {
                                Size::Fill(w) => w.max(0.001),
                                _ => 1.0,
                            };
                            intrinsic_constrained(ch, Some(av * weight / fill_weight_total))
                        }
                        _ => intrinsic(ch),
                    },
                };
                w_total += cw;
                if i + 1 < n {
                    w_total += c.gap;
                }
                h_max = h_max.max(chh);
            }
            apply_min(c, w_total, h_max + c.padding.top + c.padding.bottom)
        }
    }
}

pub(crate) fn text_layout(
    c: &El,
    available_width: Option<f32>,
) -> Option<text_metrics::TextLayout> {
    let text = c.text.as_ref()?;
    let content_available = match c.text_wrap {
        TextWrap::NoWrap => None,
        TextWrap::Wrap => available_width
            .or(match c.width {
                Size::Fixed(v) => Some(v),
                Size::Fill(_) | Size::Hug | Size::Aspect(_) => None,
            })
            .map(|w| (w - c.padding.left - c.padding.right).max(1.0)),
    };
    let display = display_text_for_measure(c, text, content_available);
    Some(text_metrics::layout_text_with_line_height_and_family(
        &display,
        c.font_size,
        c.line_height,
        c.font_family,
        c.font_weight,
        c.font_mono,
        c.text_tabular_numerals,
        c.text_wrap,
        content_available,
    ))
}

fn display_text_for_measure(c: &El, text: &str, available_width: Option<f32>) -> String {
    if let (TextWrap::Wrap, Some(max_lines), Some(width)) =
        (c.text_wrap, c.text_max_lines, available_width)
    {
        text_metrics::clamp_text_to_lines_with_family(
            text,
            c.font_size,
            c.font_family,
            c.font_weight,
            c.font_mono,
            width,
            max_lines,
        )
    } else {
        text.to_string()
    }
}

fn apply_min(c: &El, mut w: f32, mut h: f32) -> (f32, f32) {
    if let Size::Fixed(v) = c.width {
        w = v;
    }
    if let Size::Fixed(v) = c.height {
        h = v;
    }
    (clamp_w(c, w), clamp_h(c, h))
}

/// Apply [`El::min_width`] / [`El::max_width`] to a resolved width,
/// matching CSS's `min-width` over `max-width` precedence (when both
/// constraints conflict, the lower bound wins). Also clamps to a
/// non-negative result so a zero-padding Hug never reports a negative
/// intrinsic.
pub(crate) fn clamp_w(c: &El, mut w: f32) -> f32 {
    if let Some(max_w) = c.max_width {
        w = w.min(max_w);
    }
    if let Some(min_w) = c.min_width {
        w = w.max(min_w);
    }
    w.max(0.0)
}

/// Height-axis companion to [`clamp_w`].
pub(crate) fn clamp_h(c: &El, mut h: f32) -> f32 {
    if let Some(max_h) = c.max_height {
        h = h.min(max_h);
    }
    if let Some(min_h) = c.min_height {
        h = h.max(min_h);
    }
    h.max(0.0)
}

/// Approximate intrinsic measurement for `Kind::Inlines` paragraphs.
///
/// The paragraph paints through cosmic-text's rich-text shaping (which
/// resolves bold/italic/mono runs against fontdb), but layout needs a
/// width and height *before* we get to the renderer. We concatenate
/// the runs' text into one string and call `text_metrics::layout_text`
/// at the dominant font size — same approximation the lint pass uses
/// for single-style text. Bold/italic widths are slightly different
/// from regular; for body-text paragraphs that difference is well
/// under one wrap-line and we accept it. If a fixture wraps within
/// 1-2 characters of a boundary the rendered glyphs may straddle the
/// laid-out rect by a fraction of a glyph.
fn inline_paragraph_intrinsic(node: &El, available_width: Option<f32>) -> (f32, f32) {
    if node.children.iter().any(|c| matches!(c.kind, Kind::Math)) {
        return inline_mixed_intrinsic(node, available_width);
    }
    let concat = concat_inline_text(&node.children);
    let size = inline_paragraph_size(node);
    let line_height = inline_paragraph_line_height(node);
    let content_available = match node.text_wrap {
        TextWrap::NoWrap => None,
        TextWrap::Wrap => available_width
            .or(match node.width {
                Size::Fixed(v) => Some(v),
                Size::Fill(_) | Size::Hug | Size::Aspect(_) => None,
            })
            .map(|w| (w - node.padding.left - node.padding.right).max(1.0)),
    };
    let layout = text_metrics::layout_text_with_line_height_and_family(
        &concat,
        size,
        line_height,
        node.font_family,
        FontWeight::Regular,
        false,
        false,
        node.text_wrap,
        content_available,
    );
    let w = match (content_available, node.width) {
        (Some(available), Size::Hug | Size::Aspect(_)) => {
            let unwrapped = text_metrics::layout_text_with_line_height_and_family(
                &concat,
                size,
                line_height,
                node.font_family,
                FontWeight::Regular,
                false,
                false,
                TextWrap::NoWrap,
                None,
            );
            unwrapped.width.min(available) + node.padding.left + node.padding.right
        }
        (Some(available), Size::Fixed(_) | Size::Fill(_)) => {
            available + node.padding.left + node.padding.right
        }
        (None, _) => layout.width + node.padding.left + node.padding.right,
    };
    let h = layout.height + node.padding.top + node.padding.bottom;
    apply_min(node, w, h)
}

fn inline_mixed_intrinsic(node: &El, available_width: Option<f32>) -> (f32, f32) {
    let wrap_width = match node.text_wrap {
        TextWrap::Wrap => available_width.or(match node.width {
            Size::Fixed(v) => Some(v),
            Size::Fill(_) | Size::Hug | Size::Aspect(_) => None,
        }),
        TextWrap::NoWrap => None,
    }
    .map(|w| (w - node.padding.left - node.padding.right).max(1.0));

    let mut breaker = crate::text::inline_mixed::MixedInlineBreaker::new(
        node.text_wrap,
        wrap_width,
        node.font_size * 0.82,
        node.font_size * 0.22,
        node.line_height,
    );

    for child in &node.children {
        match child.kind {
            Kind::HardBreak => {
                breaker.finish_line();
                continue;
            }
            Kind::Text => {
                let text = child.text.as_deref().unwrap_or("");
                for chunk in inline_text_chunks(text) {
                    let is_space = chunk.chars().all(char::is_whitespace);
                    if breaker.skips_leading_space(is_space) {
                        continue;
                    }
                    let (w, ascent, descent) = inline_text_chunk_metrics(child, chunk);
                    if breaker.wraps_before(is_space, w) {
                        breaker.finish_line();
                    }
                    if breaker.skips_overflowing_space(is_space, w) {
                        continue;
                    }
                    breaker.push(w, ascent, descent);
                }
                continue;
            }
            _ => {}
        }
        let (w, ascent, descent) = inline_child_metrics(child);
        if breaker.wraps_before(false, w) {
            breaker.finish_line();
        }
        breaker.push(w, ascent, descent);
    }
    let measurement = breaker.finish();
    let w = measurement.width + node.padding.left + node.padding.right;
    let h = measurement.height + node.padding.top + node.padding.bottom;
    apply_min(node, w, h)
}

fn inline_text_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut last_space = None;
    for (i, ch) in text.char_indices() {
        let is_space = ch.is_whitespace();
        match last_space {
            None => last_space = Some(is_space),
            Some(prev) if prev != is_space => {
                chunks.push(&text[start..i]);
                start = i;
                last_space = Some(is_space);
            }
            _ => {}
        }
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

fn inline_text_chunk_metrics(child: &El, text: &str) -> (f32, f32, f32) {
    let layout = text_metrics::layout_text_with_line_height_and_family(
        text,
        child.font_size,
        child.line_height,
        child.font_family,
        child.font_weight,
        child.font_mono,
        child.text_tabular_numerals,
        TextWrap::NoWrap,
        None,
    );
    (layout.width, child.font_size * 0.82, child.font_size * 0.22)
}

fn inline_child_metrics(child: &El) -> (f32, f32, f32) {
    match child.kind {
        Kind::Text => inline_text_chunk_metrics(child, child.text.as_deref().unwrap_or("")),
        Kind::Math => {
            if let Some(expr) = &child.math {
                let layout = crate::math::layout_math(expr, child.font_size, child.math_display);
                (layout.width, layout.ascent, layout.descent)
            } else {
                (0.0, 0.0, 0.0)
            }
        }
        _ => (0.0, 0.0, 0.0),
    }
}

/// Walk an Inlines paragraph's children and produce the source-order
/// concatenation that draw_ops will hand to the atlas. `Kind::Text`
/// contributes its `text` field; `Kind::HardBreak` contributes a
/// newline; anything else contributes nothing (an unsupported child
/// kind inside Inlines is a programmer error elsewhere — measurement
/// silently ignores it).
fn concat_inline_text(children: &[El]) -> String {
    let mut s = String::new();
    for c in children {
        match c.kind {
            Kind::Text => {
                if let Some(t) = &c.text {
                    s.push_str(t);
                }
            }
            Kind::HardBreak => s.push('\n'),
            _ => {}
        }
    }
    s
}

/// Pick the font size that drives the paragraph's measurement. We use
/// the maximum across text children rather than the parent's own
/// `font_size`, because builders set sizes on the leaf text nodes.
fn inline_paragraph_size(node: &El) -> f32 {
    let mut size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) {
            size = size.max(c.font_size);
        }
    }
    size
}

fn inline_paragraph_line_height(node: &El) -> f32 {
    let mut line_height: f32 = node.line_height;
    let mut max_size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) && c.font_size >= max_size {
            max_size = c.font_size;
            line_height = c.line_height;
        }
    }
    line_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;

    /// Issue #64: two same-role siblings sharing a key collide on
    /// `computed_id`. Runtime-only apps never run the bundle lint, so
    /// the layout pass flags the collision once per id (reusing the
    /// `DuplicateId` finding vocabulary) and dedupes across frames.
    #[test]
    fn duplicate_sibling_keys_flagged_once() {
        let mut root = column([
            crate::widgets::text::text("a").key("dup"),
            crate::widgets::text::text("b").key("dup"),
        ]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        // Both siblings collapse onto one computed_id...
        assert_eq!(root.children[0].computed_id, root.children[1].computed_id);
        let id = root.children[0].computed_id.clone();
        // ...and that id is flagged.
        assert!(state.layout.warned_duplicate_ids.contains(&id));

        // A standing duplicate must not re-flag on the next layout.
        let n_before = state.layout.warned_duplicate_ids.len();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(state.layout.warned_duplicate_ids.len(), n_before);
    }

    /// A tree with unique sibling keys must not flag anything.
    #[test]
    fn distinct_sibling_keys_not_flagged() {
        let mut root = column([
            crate::widgets::text::text("a").key("one"),
            crate::widgets::text::text("b").key("two"),
        ]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert!(state.layout.warned_duplicate_ids.is_empty());
    }

    /// CSS-flex parity: a `Size::Fill` child of a column with
    /// `align(Center)` should shrink to its intrinsic cross-axis size
    /// and be horizontally centered, matching `align-items: center`
    /// in CSS flex (which causes flex items to lose their stretch).
    #[test]
    fn align_center_shrinks_fill_child_to_intrinsic() {
        // Column with align(Center). Inner row has the default
        // El::new width = Fill(1.0); without Proposal B it would
        // claim the full 200px and align would be a no-op.
        let mut root = column([crate::row([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])])
        .align(Align::Center)
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let row_rect = state.rect(&root.children[0].computed_id);
        // Row's intrinsic width = 40 (single fixed child). 200 - 40 = 160
        // leftover; centered → row starts at x=80.
        assert!(
            (row_rect.x - 80.0).abs() < 0.5,
            "expected x≈80 (centered), got {}",
            row_rect.x
        );
        assert!(
            (row_rect.w - 40.0).abs() < 0.5,
            "expected w≈40 (shrunk to intrinsic), got {}",
            row_rect.w
        );
    }

    /// CSS-flex parity: when a `Fill` sibling hits its `max_width`,
    /// the space it frees is re-distributed to the still-flexible
    /// fills instead of becoming trailing dead space.
    #[test]
    fn max_clamped_fill_frees_space_to_sibling_fills() {
        let mut root = crate::row([
            El::new(Kind::Group).width(Size::Fill(1.0)).max_width(50.0),
            El::new(Kind::Group).width(Size::Fill(1.0)),
        ])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let a = state.rect(&root.children[0].computed_id);
        let b = state.rect(&root.children[1].computed_id);
        assert!((a.w - 50.0).abs() < 0.5, "capped fill w={}", a.w);
        assert!(
            (b.w - 350.0).abs() < 0.5,
            "sibling fill should absorb the freed 150px, got w={}",
            b.w
        );
    }

    /// A `min_width` violation steals space symmetrically: the frozen
    /// fill takes its floor and the flexible sibling absorbs what's
    /// left, rather than both overflowing the row.
    #[test]
    fn min_clamped_fill_steals_space_from_sibling_fills() {
        let mut root = crate::row([
            El::new(Kind::Group).width(Size::Fill(1.0)).min_width(300.0),
            El::new(Kind::Group).width(Size::Fill(1.0)),
        ])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let a = state.rect(&root.children[0].computed_id);
        let b = state.rect(&root.children[1].computed_id);
        assert!((a.w - 300.0).abs() < 0.5, "floored fill w={}", a.w);
        assert!(
            (b.w - 100.0).abs() < 0.5,
            "sibling fill should shrink to the remaining 100px, got w={}",
            b.w
        );
    }

    /// When every fill freezes at its max, the leftover becomes free
    /// space for `justify` — a centered capped column actually centers
    /// instead of leaving all the slack trailing.
    #[test]
    fn justify_distributes_space_left_by_fully_capped_fills() {
        let mut root = crate::row([El::new(Kind::Group).width(Size::Fill(1.0)).max_width(100.0)])
            .justify(Justify::Center)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let a = state.rect(&root.children[0].computed_id);
        assert!((a.w - 100.0).abs() < 0.5, "capped fill w={}", a.w);
        assert!(
            (a.x - 150.0).abs() < 0.5,
            "capped fill should be centered (x≈150), got x={}",
            a.x
        );
    }

    /// `align(Stretch)` (the default) preserves Fill stretching: a
    /// Fill-width child still claims the full cross axis.
    #[test]
    fn align_stretch_preserves_fill_stretch() {
        let mut root = column([crate::row([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])])
        .align(Align::Stretch)
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let row_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (row_rect.x - 0.0).abs() < 0.5 && (row_rect.w - 200.0).abs() < 0.5,
            "expected stretched (x=0, w=200), got x={} w={}",
            row_rect.x,
            row_rect.w
        );
    }

    /// Issue #47: a Hug ancestor must measure a wrap-text descendant
    /// at the width layout will resolve for it — the node's own Fixed
    /// width — not at whatever wider width the ancestor chain has
    /// available. Inverted precedence here made measure wrap at the
    /// card's inner width while layout wrapped at the text's fixed
    /// width, leaving every Hug ancestor short by the difference.
    #[test]
    fn hug_ancestor_measures_wrap_text_at_its_own_fixed_width() {
        let long = "The quick brown fox jumps over the lazy dog, then \
                    does it again and again until the line is long \
                    enough to wrap several times.";
        // "card": Fill-wide, Hug-tall → measures its subtree. Inner
        // column Fixed(240); wrap text Fixed(200) — measure must use
        // 200, not the card's much wider inner width.
        let mut root = column([column([column([crate::widgets::text::text(long)
            .wrap_text()
            .width(Size::Fixed(200.0))])
        .width(Size::Fixed(240.0))])
        .width(Size::Fill(1.0))]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));
        let card = state.rect(&root.children[0].computed_id);
        let text_rect = state.rect(&root.children[0].children[0].children[0].computed_id);
        assert!(
            text_rect.h > 25.0,
            "text should wrap to multiple lines at 200px, got h={}",
            text_rect.h
        );
        assert!(
            (card.h - text_rect.h).abs() < 0.5,
            "Hug card height {} must match wrapped text height {}",
            card.h,
            text_rect.h
        );
    }

    /// When all children are Hug-sized, `Justify::Center` should split
    /// the leftover space symmetrically across the main axis.
    #[test]
    fn justify_center_centers_hug_children() {
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .justify(Justify::Center)
        .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        // Expected: 100 - 20 = 80 leftover; centered → starts at y=40.
        assert!(
            (child_rect.y - 40.0).abs() < 0.5,
            "expected y≈40, got {}",
            child_rect.y
        );
    }

    #[test]
    fn justify_end_pushes_to_bottom() {
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .justify(Justify::End)
        .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.y - 80.0).abs() < 0.5,
            "expected y≈80, got {}",
            child_rect.y
        );
    }

    /// CSS `justify-content: space-between`: when no main-axis Fill
    /// children claim the slack, the leftover space is distributed
    /// evenly *between* (not around) the children — outer edges flush.
    #[test]
    fn justify_space_between_distributes_evenly() {
        let row_child = || {
            crate::widgets::text::text("x")
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0))
        };
        let mut root = column([row_child(), row_child(), row_child()])
            .justify(Justify::SpaceBetween)
            .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 200.0));
        // Used main = 3 * 20 = 60. Leftover = 140 over (n-1) = 2 gaps
        // → 70 between. Positions: 0, 90, 180.
        let y0 = state.rect(&root.children[0].computed_id).y;
        let y1 = state.rect(&root.children[1].computed_id).y;
        let y2 = state.rect(&root.children[2].computed_id).y;
        assert!(
            y0.abs() < 0.5,
            "first child should be flush at y=0, got {y0}"
        );
        assert!(
            (y1 - 90.0).abs() < 0.5,
            "middle child should be at y≈90, got {y1}"
        );
        assert!(
            (y2 - 180.0).abs() < 0.5,
            "last child should be flush at y≈180, got {y2}"
        );
    }

    /// CSS `flex: <weight>`: when multiple `Size::Fill` children share
    /// a container, the available space is distributed in proportion
    /// to their weights.
    #[test]
    fn fill_weight_distributes_proportionally() {
        let big = crate::widgets::text::text("big")
            .width(Size::Fixed(40.0))
            .height(Size::Fill(2.0));
        let small = crate::widgets::text::text("small")
            .width(Size::Fixed(40.0))
            .height(Size::Fill(1.0));
        let mut root = column([big, small]).height(Size::Fixed(300.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 300.0));
        // Total weight = 3, available = 300. Big = 200, small = 100.
        let big_h = state.rect(&root.children[0].computed_id).h;
        let small_h = state.rect(&root.children[1].computed_id).h;
        assert!(
            (big_h - 200.0).abs() < 0.5,
            "Fill(2.0) should claim 2/3 of 300 ≈ 200, got {big_h}"
        );
        assert!(
            (small_h - 100.0).abs() < 0.5,
            "Fill(1.0) should claim 1/3 of 300 ≈ 100, got {small_h}"
        );
    }

    /// `padding` on a `Hug`-sized container is included in the
    /// container's intrinsic — matching CSS `box-sizing: content-box`
    /// where padding adds to the rendered size.
    #[test]
    fn padding_on_hug_includes_in_intrinsic() {
        let root = column([crate::widgets::text::text("x")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(40.0))])
        .padding(Sides::all(20.0));
        let (w, h) = intrinsic(&root);
        // 40 content + 2*20 padding on each axis = 80.
        assert!((w - 80.0).abs() < 0.5, "expected intrinsic w≈80, got {w}");
        assert!((h - 80.0).abs() < 0.5, "expected intrinsic h≈80, got {h}");
    }

    /// Cross-axis `Align::End` on a row pins children to the bottom
    /// edge — CSS `align-items: flex-end`. Mirror of `justify_end`
    /// but on the cross axis instead of the main axis.
    #[test]
    fn align_end_pins_to_cross_axis_far_edge() {
        let mut root = crate::row([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .align(Align::End)
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        // Row cross axis = height. End → child y = 100 - 20 = 80.
        assert!(
            (child_rect.y - 80.0).abs() < 0.5,
            "expected y≈80 (pinned to bottom), got {}",
            child_rect.y
        );
    }

    #[test]
    fn overlay_can_center_hug_child() {
        let mut root = stack([crate::titled_card("Dialog", [crate::text("Body")])
            .width(Size::Fixed(200.0))
            .height(Size::Hug)])
        .align(Align::Center)
        .justify(Justify::Center);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 400.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.x - 200.0).abs() < 0.5,
            "expected x≈200, got {}",
            child_rect.x
        );
        assert!(
            child_rect.y > 100.0 && child_rect.y < 200.0,
            "expected centered y, got {}",
            child_rect.y
        );
    }

    #[test]
    fn scroll_offset_translates_children_and_clamps_to_content() {
        // Six 50px-tall rows in a 200px-tall scroll viewport.
        // Content height = 6 * 50 + 5 * 12 (gap) = 360 px. Visible
        // viewport (no padding) = 200 px → max_offset = 160.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .key("list")
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        assign_ids(&mut root);
        state.scroll.offsets.insert(root.computed_id.clone(), 80.0);

        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        // Offset is in range, applied verbatim.
        let stored = state
            .scroll
            .offsets
            .get(&root.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - 80.0).abs() < 0.01,
            "offset clamped unexpectedly: {stored}"
        );
        // First child shifted up by 80.
        let c0 = state.rect(&root.children[0].computed_id);
        assert!(
            (c0.y - (-80.0)).abs() < 0.01,
            "child 0 y = {} (expected -80)",
            c0.y
        );
        // Now overshoot — should clamp to max_offset=160.
        state
            .scroll
            .offsets
            .insert(root.computed_id.clone(), 9999.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stored = state
            .scroll
            .offsets
            .get(&root.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - 160.0).abs() < 0.01,
            "overshoot clamped to {stored}"
        );
        // Content fits → offset clamps to 0.
        let mut tiny =
            scroll([crate::widgets::text::text("just one row").height(Size::Fixed(20.0))])
                .height(Size::Fixed(200.0));
        let mut tiny_state = UiState::new();
        assign_ids(&mut tiny);
        tiny_state
            .scroll
            .offsets
            .insert(tiny.computed_id.clone(), 50.0);
        layout(
            &mut tiny,
            &mut tiny_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert_eq!(
            tiny_state
                .scroll
                .offsets
                .get(&tiny.computed_id)
                .copied()
                .unwrap_or(0.0),
            0.0
        );
    }

    #[test]
    fn scroll_layout_prunes_far_offscreen_descendants() {
        let far = column([crate::widgets::text::text("far row body").key("far-text")])
            .height(Size::Fixed(40.0));
        let mut root = scroll([
            column([crate::widgets::text::text("near row body")]).height(Size::Fixed(40.0)),
            crate::tree::spacer().height(Size::Fixed(400.0)),
            far,
        ])
        .key("list")
        .height(Size::Fixed(80.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 80.0));
        let stats = take_prune_stats();

        assert!(
            stats.subtrees >= 1,
            "expected at least one far scroll child to be pruned, got {stats:?}"
        );
        assert!(
            stats.nodes >= 1,
            "expected pruned descendants to be zeroed, got {stats:?}"
        );
        let far_text = state
            .rect_of_key("far-text")
            .expect("far text keeps a zero rect while pruned");
        assert_eq!(far_text.w, 0.0);
        assert_eq!(far_text.h, 0.0);
    }

    #[test]
    fn plain_scroll_preserves_visible_anchor_when_width_reflows_content() {
        let make_root = || {
            let paragraph_text = "Variable width text wraps into a different number of lines when \
                                  the viewport narrows, which used to make a plain scroll box lose \
                                  the item the user was reading.";
            scroll([column((0..30).map(|i| {
                crate::widgets::text::paragraph(format!("{i}: {paragraph_text}"))
                    .key(format!("paragraph-{i}"))
            }))
            .gap(8.0)])
            .key("article")
            .height(Size::Fixed(180.0))
        };

        let mut root = make_root();
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 320.0, 180.0));

        state.scroll.offsets.insert(root.computed_id.clone(), 520.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 320.0, 180.0));

        let anchor = state
            .scroll
            .scroll_anchors
            .get(&root.computed_id)
            .cloned()
            .expect("plain scroll should store a visible descendant anchor");
        let before_rect = state.rect(&anchor.node_id);
        let before_anchor_y = before_rect.y + before_rect.h * anchor.rect_fraction;
        let before_offset = state.scroll_offset(&root.computed_id);

        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 180.0));

        let after_rect = state.rect(&anchor.node_id);
        let after_anchor_y = after_rect.y + after_rect.h * anchor.rect_fraction;
        let after_offset = state.scroll_offset(&root.computed_id);
        assert!(
            (after_anchor_y - before_anchor_y).abs() < 0.5,
            "anchor point should stay at y={before_anchor_y}, got {after_anchor_y}"
        );
        assert!(
            (after_offset - before_offset).abs() > 20.0,
            "offset should absorb height changes above the anchor"
        );
    }

    #[test]
    fn scrollbar_thumb_size_and_position_track_overflow() {
        // 6 rows x 50px + 5 gaps x 12 = 360 content; 200 viewport.
        // viewport/content = 200/360 ≈ 0.555 → thumb_h ≈ 111.1.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let metrics = state
            .scroll
            .metrics
            .get(&root.computed_id)
            .copied()
            .expect("scrollable should have metrics");
        assert!((metrics.viewport_h - 200.0).abs() < 0.01);
        assert!((metrics.content_h - 360.0).abs() < 0.01);
        assert!((metrics.max_offset - 160.0).abs() < 0.01);

        let thumb = state
            .scroll
            .thumb_rects
            .get(&root.computed_id)
            .copied()
            .expect("scrollable with scrollbar() and overflow gets a thumb");
        // viewport^2 / content_h = 200^2 / 360 = 111.11..
        assert!((thumb.h - 111.111).abs() < 0.5, "thumb h = {}", thumb.h);
        assert!((thumb.w - crate::tokens::SCROLLBAR_THUMB_WIDTH).abs() < 0.01);
        // At offset 0, thumb sits at the top of the inner rect.
        assert!(thumb.y.abs() < 0.01);
        // Right-anchored: thumb_x + thumb_w + track_inset == viewport_right.
        assert!(
            (thumb.x + thumb.w + crate::tokens::SCROLLBAR_TRACK_INSET - 300.0).abs() < 0.01,
            "thumb anchored at {} (expected {})",
            thumb.x,
            300.0 - thumb.w - crate::tokens::SCROLLBAR_TRACK_INSET
        );

        // Slide to half — thumb should be at half the track_remaining.
        state.scroll.offsets.insert(root.computed_id.clone(), 80.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let thumb = state
            .scroll
            .thumb_rects
            .get(&root.computed_id)
            .copied()
            .unwrap();
        let track_remaining = 200.0 - thumb.h;
        let expected_y = track_remaining * (80.0 / 160.0);
        assert!(
            (thumb.y - expected_y).abs() < 0.5,
            "thumb at half-scroll y = {} (expected {expected_y})",
            thumb.y,
        );
    }

    #[test]
    fn scrollbar_track_is_wider_than_thumb_and_full_height() {
        // The track is the click hitbox: wider than the visible
        // thumb (Fitts's law) and tall enough to detect track
        // clicks above and below the thumb for paging.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let thumb = state
            .scroll
            .thumb_rects
            .get(&root.computed_id)
            .copied()
            .unwrap();
        let track = state
            .scroll
            .thumb_tracks
            .get(&root.computed_id)
            .copied()
            .unwrap();
        // Track wider than thumb on the same right edge.
        assert!(track.w > thumb.w, "track.w {} thumb.w {}", track.w, thumb.w);
        assert!(
            (track.right() - thumb.right()).abs() < 0.01,
            "track and thumb must share the right edge",
        );
        // Track spans the full inner viewport (so above/below thumb
        // are both inside it for click-to-page).
        assert!(
            (track.h - 200.0).abs() < 0.01,
            "track height = {} (expected 200)",
            track.h,
        );
    }

    #[test]
    fn scrollbar_thumb_absent_when_disabled_or_no_overflow() {
        // Same scrollable, but author opted out — no thumb_rect.
        let mut suppressed = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .no_scrollbar()
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(
            &mut suppressed,
            &mut state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert!(
            !state
                .scroll
                .thumb_rects
                .contains_key(&suppressed.computed_id)
        );

        // Same scrollable, content fits → no thumb either.
        let mut tiny = scroll([crate::widgets::text::text("one row").height(Size::Fixed(20.0))])
            .height(Size::Fixed(200.0));
        let mut tiny_state = UiState::new();
        layout(
            &mut tiny,
            &mut tiny_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert!(
            !tiny_state
                .scroll
                .thumb_rects
                .contains_key(&tiny.computed_id)
        );
    }

    #[test]
    fn nested_scrollbar_thumb_moves_with_outer_scroll_content() {
        let make_root = || {
            scroll([
                crate::tree::spacer().height(Size::Fixed(80.0)),
                scroll((0..6).map(|i| {
                    crate::widgets::text::text(format!("inner row {i}")).height(Size::Fixed(50.0))
                }))
                .key("inner")
                .height(Size::Fixed(120.0)),
                crate::tree::spacer().height(Size::Fixed(260.0)),
            ])
            .key("outer")
            .height(Size::Fixed(220.0))
        };

        let mut root = make_root();
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 220.0));
        let inner = root
            .children
            .iter()
            .find(|child| child.key.as_deref() == Some("inner"))
            .expect("inner scroll");
        let inner_id = inner.computed_id.clone();
        let inner_rect = state.rect(&inner_id);
        let thumb = state
            .scroll
            .thumb_rects
            .get(&inner_id)
            .copied()
            .expect("inner scroll should have a thumb");
        let track = state
            .scroll
            .thumb_tracks
            .get(&inner_id)
            .copied()
            .expect("inner scroll should have a track");
        let thumb_rel_y = thumb.y - inner_rect.y;
        let track_rel_y = track.y - inner_rect.y;

        state.scroll.offsets.insert(root.computed_id.clone(), 60.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 220.0));
        let inner_rect_after = state.rect(&inner_id);
        let thumb_after = state.scroll.thumb_rects.get(&inner_id).copied().unwrap();
        let track_after = state.scroll.thumb_tracks.get(&inner_id).copied().unwrap();

        assert!(
            (inner_rect_after.y - (inner_rect.y - 60.0)).abs() < 0.5,
            "outer scroll should shift the inner viewport"
        );
        assert!(
            (thumb_after.y - inner_rect_after.y - thumb_rel_y).abs() < 0.5,
            "inner thumb should stay fixed relative to its viewport"
        );
        assert!(
            (track_after.y - inner_rect_after.y - track_rel_y).abs() < 0.5,
            "inner track should stay fixed relative to its viewport"
        );
    }

    #[test]
    fn layout_override_places_children_at_returned_rects() {
        // A custom layout that just stacks children diagonally inside the container.
        let mut root = column((0..3).map(|i| {
            crate::widgets::text::text(format!("dot {i}"))
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0))
        }))
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| {
            ctx.children
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let off = i as f32 * 30.0;
                    Rect::new(ctx.container.x + off, ctx.container.y + off, 20.0, 20.0)
                })
                .collect()
        });
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r0 = state.rect(&root.children[0].computed_id);
        let r1 = state.rect(&root.children[1].computed_id);
        let r2 = state.rect(&root.children[2].computed_id);
        assert_eq!((r0.x, r0.y), (0.0, 0.0));
        assert_eq!((r1.x, r1.y), (30.0, 30.0));
        assert_eq!((r2.x, r2.y), (60.0, 60.0));
    }

    #[test]
    fn layout_override_rect_of_key_resolves_earlier_sibling() {
        // The popover-anchor pattern: a custom-laid-out node positions
        // its child by reading another keyed node's rect via the new
        // LayoutCtx::rect_of_key callback. The trigger lives in an
        // earlier sibling so its rect is already in `computed_rects`
        // by the time the popover layer's layout_override runs.
        use crate::tree::stack;
        let trigger_x = 40.0;
        let trigger_y = 20.0;
        let trigger_w = 60.0;
        let trigger_h = 30.0;
        let mut root = stack([
            // Earlier sibling: the trigger.
            crate::widgets::button::button("Open")
                .key("trig")
                .width(Size::Fixed(trigger_w))
                .height(Size::Fixed(trigger_h)),
            // Later sibling: a custom-laid-out container that reads
            // the trigger's rect to position its single child.
            stack([crate::widgets::text::text("popover")
                .width(Size::Fixed(80.0))
                .height(Size::Fixed(20.0))])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .layout(|ctx| {
                let trig = (ctx.rect_of_key)("trig").expect("trigger laid out");
                vec![Rect::new(trig.x, trig.bottom() + 4.0, 80.0, 20.0)]
            }),
        ])
        .padding(Sides::xy(trigger_x, trigger_y));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let popover_layer = &root.children[1];
        let panel_id = &popover_layer.children[0].computed_id;
        let panel_rect = state.rect(panel_id);
        // Anchored to (trigger.x, trigger.bottom() + 4.0). With padding
        // (40, 20) and trigger height 30 → expect (40, 54).
        assert!(
            (panel_rect.x - trigger_x).abs() < 0.01,
            "popover x = {} (expected {trigger_x})",
            panel_rect.x,
        );
        assert!(
            (panel_rect.y - (trigger_y + trigger_h + 4.0)).abs() < 0.01,
            "popover y = {} (expected {})",
            panel_rect.y,
            trigger_y + trigger_h + 4.0,
        );
    }

    #[test]
    fn layout_override_rect_of_key_returns_none_for_missing_key() {
        let mut root = column([crate::widgets::text::text("inner")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| {
            assert!((ctx.rect_of_key)("nope").is_none());
            vec![Rect::new(ctx.container.x, ctx.container.y, 40.0, 20.0)]
        });
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn layout_override_rect_of_key_returns_none_for_later_sibling() {
        // First-frame contract: a custom layout running before its
        // target's sibling has been laid out should see `None`, not a
        // zero rect or a panic. This is what makes the popover pattern
        // (trigger first, popover layer second in source order) the
        // supported shape — the reverse direction simply gets `None`.
        use crate::tree::stack;
        let mut root = stack([
            stack([crate::widgets::text::text("panel")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(20.0))])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .layout(|ctx| {
                assert!(
                    (ctx.rect_of_key)("later").is_none(),
                    "later sibling's rect must not be available yet"
                );
                vec![Rect::new(ctx.container.x, ctx.container.y, 40.0, 20.0)]
            }),
            crate::widgets::button::button("after").key("later"),
        ]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn layout_override_measure_returns_intrinsic() {
        // The custom layout reads `measure` to size each child.
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| {
            let (w, h) = (ctx.measure)(&ctx.children[0]);
            assert!((w - 40.0).abs() < 0.01, "measured width {w}");
            assert!((h - 20.0).abs() < 0.01, "measured height {h}");
            vec![Rect::new(ctx.container.x, ctx.container.y, w, h)]
        });
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r = state.rect(&root.children[0].computed_id);
        assert_eq!((r.w, r.h), (40.0, 20.0));
    }

    #[test]
    #[should_panic(expected = "returned 1 rects for 2 children")]
    fn layout_override_length_mismatch_panics() {
        let mut root = column([
            crate::widgets::text::text("a")
                .width(Size::Fixed(10.0))
                .height(Size::Fixed(10.0)),
            crate::widgets::text::text("b")
                .width(Size::Fixed(10.0))
                .height(Size::Fixed(10.0)),
        ])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| vec![Rect::new(ctx.container.x, ctx.container.y, 10.0, 10.0)]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    #[should_panic(expected = "Size::Hug is not supported for custom layouts")]
    fn layout_override_hug_panics() {
        // Hug check fires when the parent's layout pass measures the
        // custom-layout child for sizing — i.e. when a layout_override
        // node is a child of a column/row, not when it's the root.
        let mut root = column([column([crate::widgets::text::text("c")])
            .width(Size::Hug)
            .height(Size::Fixed(200.0))
            .layout(|ctx| vec![Rect::new(ctx.container.x, ctx.container.y, 10.0, 10.0)])])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn fit_contain_letterboxes_and_centers() {
        // 16:9 child in a 400×400 container: width binds → 400×225,
        // centered vertically.
        let child = crate::tree::column([crate::widgets::text::text("c")]).key("fitted");
        let mut root = crate::tree::fit_contain(child, 16.0 / 9.0)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));

        let r = state.rect_of_key("fitted").expect("fitted rect");
        assert_eq!((r.w, r.h), (400.0, 225.0));
        assert_eq!(r.x, 0.0);
        assert!((r.y - 87.5).abs() < 0.01, "centered: y = {}", r.y);

        // Tall container, same ratio: height binds → pillarboxed.
        let child = crate::tree::column([crate::widgets::text::text("c")]).key("fitted2");
        let mut root = crate::tree::fit_contain(child, 16.0 / 9.0)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));
        let r = state.rect_of_key("fitted2").expect("fitted2 rect");
        assert!((r.w - 100.0 * 16.0 / 9.0).abs() < 0.01);
        assert_eq!(r.h, 100.0);
        assert!((r.x - (400.0 - r.w) / 2.0).abs() < 0.01);
    }

    #[test]
    fn fit_cover_overflows_the_slack_axis_and_clips() {
        // 1:1 child covering a 400×200 container: width binds the
        // cover → 400×400, vertically centered (overflowing top and
        // bottom), and the container clips.
        let child = crate::tree::column([crate::widgets::text::text("c")]).key("covered");
        let mut root = crate::tree::fit_cover(child, 1.0)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(200.0));
        assert!(root.clip, "fit_cover must clip its overflow");
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let r = state.rect_of_key("covered").expect("covered rect");
        assert_eq!((r.w, r.h), (400.0, 400.0));
        assert!(
            (r.y - -100.0).abs() < 0.01,
            "centered overflow: y = {}",
            r.y
        );
    }

    #[test]
    fn fit_contain_intrinsic_uses_the_child_measure() {
        // A fixed 100×50 child (2:1) in a 400×400 container → 400×200.
        let child = crate::tree::column::<Vec<El>, El>(vec![])
            .width(Size::Fixed(100.0))
            .height(Size::Fixed(50.0))
            .key("intrinsic");
        let mut root = crate::tree::fit_contain_intrinsic(child)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let r = state.rect_of_key("intrinsic").expect("intrinsic rect");
        assert_eq!((r.w, r.h), (400.0, 200.0));
    }

    #[test]
    fn virtual_list_realizes_only_visible_rows() {
        // 100 rows × 50px each in a 200px viewport, offset = 120.
        // Visible range: rows whose y in [-50, 200) → start = floor(120/50) = 2,
        // end = ceil((120+200)/50) = ceil(6.4) = 7. Five rows realized.
        let mut root = crate::tree::virtual_list(100, 50.0, |i| {
            crate::widgets::text::text(format!("row {i}")).key(format!("row-{i}"))
        });
        let mut state = UiState::new();
        assign_ids(&mut root);
        state.scroll.offsets.insert(root.computed_id.clone(), 120.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        assert_eq!(
            root.children.len(),
            5,
            "expected 5 realized rows, got {}",
            root.children.len()
        );
        // Identity check: the first realized row should be the row keyed "row-2".
        assert_eq!(root.children[0].key.as_deref(), Some("row-2"));
        assert_eq!(root.children[4].key.as_deref(), Some("row-6"));
        // Position check: first realized row's y = inner.y + 2*50 - 120 = -20.
        let r0 = state.rect(&root.children[0].computed_id);
        assert!(
            (r0.y - (-20.0)).abs() < 0.5,
            "row 2 expected y≈-20, got {}",
            r0.y
        );
    }

    #[test]
    fn virtual_list_gap_contributes_to_row_positions_and_content_height() {
        let mut root = crate::tree::virtual_list(10, 40.0, |i| {
            crate::widgets::text::text(format!("row {i}")).key(format!("row-{i}"))
        })
        .gap(10.0);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));

        assert_eq!(
            root.children.len(),
            3,
            "rows 0, 1, and 2 should intersect a 120px viewport with 40px rows and 10px gaps"
        );
        let row_1 = root
            .children
            .iter()
            .find(|c| c.key.as_deref() == Some("row-1"))
            .expect("row 1 should be realized");
        assert!(
            (state.rect(&row_1.computed_id).y - 50.0).abs() < 0.5,
            "gap should place row 1 at y=50"
        );
        let metrics = state
            .scroll
            .metrics
            .get(&root.computed_id)
            .expect("virtual list writes scroll metrics");
        assert!(
            (metrics.content_h - 490.0).abs() < 0.5,
            "10 rows x 40 plus 9 gaps x 10 should be 490, got {}",
            metrics.content_h
        );
    }

    #[test]
    fn virtual_list_keyed_rows_have_stable_computed_id_across_scroll() {
        let make_root = || {
            crate::tree::virtual_list(50, 50.0, |i| {
                crate::widgets::text::text(format!("row {i}")).key(format!("row-{i}"))
            })
        };

        let mut state = UiState::new();
        let mut root_a = make_root();
        assign_ids(&mut root_a);
        // Scroll so row 5 is visible.
        state
            .scroll
            .offsets
            .insert(root_a.computed_id.clone(), 250.0);
        layout(&mut root_a, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let id_at_offset_a = root_a
            .children
            .iter()
            .find(|c| c.key.as_deref() == Some("row-5"))
            .unwrap()
            .computed_id
            .clone();

        // Re-layout with a different offset — row 5 is still visible.
        let mut root_b = make_root();
        assign_ids(&mut root_b);
        state
            .scroll
            .offsets
            .insert(root_b.computed_id.clone(), 200.0);
        layout(&mut root_b, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let id_at_offset_b = root_b
            .children
            .iter()
            .find(|c| c.key.as_deref() == Some("row-5"))
            .unwrap()
            .computed_id
            .clone();

        assert_eq!(
            id_at_offset_a, id_at_offset_b,
            "row-5's computed_id changed when scroll offset moved"
        );
    }

    #[test]
    fn virtual_list_clamps_overshoot_offset() {
        // 10 rows × 50 = 500 content height; viewport 200; max offset = 300.
        let mut root =
            crate::tree::virtual_list(10, 50.0, |i| crate::widgets::text::text(format!("r{i}")));
        let mut state = UiState::new();
        assign_ids(&mut root);
        state
            .scroll
            .offsets
            .insert(root.computed_id.clone(), 9999.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stored = state
            .scroll
            .offsets
            .get(&root.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - 300.0).abs() < 0.01,
            "expected clamp to 300, got {stored}"
        );
    }

    #[test]
    fn virtual_list_empty_count_realizes_no_children() {
        let mut root =
            crate::tree::virtual_list(0, 50.0, |i| crate::widgets::text::text(format!("r{i}")));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        assert_eq!(root.children.len(), 0);
    }

    #[test]
    #[should_panic(expected = "row_height > 0.0")]
    fn virtual_list_zero_row_height_panics() {
        let _ = crate::tree::virtual_list(10, 0.0, |i| crate::widgets::text::text(format!("r{i}")));
    }

    #[test]
    #[should_panic(expected = "Size::Hug would defeat virtualization")]
    fn virtual_list_hug_panics() {
        let mut root = column([crate::tree::virtual_list(10, 50.0, |i| {
            crate::widgets::text::text(format!("r{i}"))
        })
        .height(Size::Hug)])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn grid_packs_rows_and_aligns_the_partial_tail() {
        // 3 cells in 2 columns at gap 10 inside 210px: two rows; each
        // column is (210 - 10) / 2 = 100 wide. The tail row's single
        // real cell must align with column 0 above (filler holds col 1).
        let cell = |i: usize| {
            crate::tree::column::<Vec<El>, El>(vec![])
                .key(format!("cell-{i}"))
                .height(Size::Fixed(50.0))
        };
        let mut root = crate::tree::grid(2, 10.0, (0..3).map(cell))
            .width(Size::Fixed(210.0))
            .height(Size::Fixed(300.0));
        assert_eq!(root.arrow_nav, Some(crate::tree::ArrowNav::Grid));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 210.0, 300.0));

        let r0 = state.rect_of_key("cell-0").unwrap();
        let r1 = state.rect_of_key("cell-1").unwrap();
        let r2 = state.rect_of_key("cell-2").unwrap();
        assert_eq!((r0.x, r0.w), (0.0, 100.0));
        assert_eq!((r1.x, r1.w), (110.0, 100.0));
        // Tail cell: same column geometry as cell-0, next row (50 + 10 gap).
        assert_eq!((r2.x, r2.w), (0.0, 100.0));
        assert_eq!(r2.y, 60.0);
    }

    #[test]
    fn virtual_grid_realizes_rows_of_packed_cells() {
        // 10 items, 3 columns, 50px cells, 120px viewport: rows 0..3
        // realize (ceil(120/50)+1 candidates, clamped by visibility).
        let mut root = crate::tree::virtual_grid(10, 3, 50.0, 0.0, |i| {
            crate::widgets::text::text(format!("item {i}")).key(format!("item-{i}"))
        })
        .key("vgrid");
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));

        assert_eq!(root.arrow_nav, Some(crate::tree::ArrowNav::Grid));
        // Realized rows are children of the list; cells are children of
        // each packed row. (Keys inside virtual rows resolve through
        // hit-test/event routing, not the pre-layout key index, so the
        // test reads rects by computed_id.)
        let rect_of = |state: &UiState, row: usize, col: usize| {
            state.rect(&root.children[row].children[col].computed_id)
        };
        // First realized row holds items 0..3 packed across the width.
        let i0 = rect_of(&state, 0, 0);
        let i2 = rect_of(&state, 0, 2);
        assert_eq!(i0.w, 100.0);
        assert_eq!(i2.x, 200.0);
        // Second row starts the next item triplet.
        let i3 = rect_of(&state, 1, 0);
        assert_eq!((i3.x, i3.y), (0.0, 50.0));
        // Realized row range is queryable for eviction logic.
        assert!(state.visible_range("vgrid").is_some());
        // Identity check: the first realized cell is item-0.
        assert_eq!(root.children[0].children[0].key.as_deref(), Some("item-0"));
    }

    #[test]
    fn visible_range_tracks_realized_rows() {
        // Same scenario as `virtual_list_realizes_only_visible_rows`:
        // 100 rows x 50px in a 200px viewport at offset 120 realizes
        // rows 2..7. The keyed query must report exactly that range.
        let mut root = crate::tree::virtual_list(100, 50.0, |i| {
            crate::widgets::text::text(format!("row {i}")).key(format!("row-{i}"))
        })
        .key("list");
        let mut state = UiState::new();
        assign_ids(&mut root);
        state.scroll.offsets.insert(root.computed_id.clone(), 120.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        assert_eq!(state.visible_range("list"), Some(2..7));
        assert_eq!(state.visible_range("not-a-list"), None);

        // Dynamic variant: four rows realize at offset 0 (see
        // `virtual_list_dyn_respects_per_row_fixed_heights`).
        let mut dyn_root = crate::tree::virtual_list_dyn(
            20,
            50.0,
            |i| format!("row-{i}"),
            |i| {
                let h = if i % 2 == 0 { 40.0 } else { 80.0 };
                crate::tree::column([crate::widgets::text::text(format!("r{i}"))])
                    .key(format!("row-{i}"))
                    .height(Size::Fixed(h))
            },
        )
        .key("dyn-list");
        let mut dyn_state = UiState::new();
        layout(
            &mut dyn_root,
            &mut dyn_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert_eq!(dyn_state.visible_range("dyn-list"), Some(0..4));

        // Per-frame scratch: an empty re-layout clears stale ranges.
        let mut empty =
            crate::tree::virtual_list(0, 50.0, |_| crate::widgets::text::text("never")).key("list");
        layout(&mut empty, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        assert_eq!(state.visible_range("list"), None);
    }

    #[test]
    fn virtual_list_dyn_respects_per_row_fixed_heights() {
        // Alternating 40px / 80px rows. With a 200px viewport and offset 0,
        // accumulated y goes 0, 40, 120, 160, 240 — the fifth row starts
        // past the viewport, so four rows are realized.
        let mut root = crate::tree::virtual_list_dyn(
            20,
            50.0,
            |i| format!("row-{i}"),
            |i| {
                let h = if i % 2 == 0 { 40.0 } else { 80.0 };
                crate::tree::column([crate::widgets::text::text(format!("r{i}"))])
                    .key(format!("row-{i}"))
                    .height(Size::Fixed(h))
            },
        );
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        assert_eq!(
            root.children.len(),
            4,
            "expected 4 realized rows, got {}",
            root.children.len()
        );
        // y positions: row 0 → 0, row 1 → 40, row 2 → 120, row 3 → 160.
        let ys: Vec<f32> = root
            .children
            .iter()
            .map(|c| state.rect(&c.computed_id).y)
            .collect();
        assert!(
            (ys[0] - 0.0).abs() < 0.5,
            "row 0 expected y≈0, got {}",
            ys[0]
        );
        assert!(
            (ys[1] - 40.0).abs() < 0.5,
            "row 1 expected y≈40, got {}",
            ys[1]
        );
        assert!(
            (ys[2] - 120.0).abs() < 0.5,
            "row 2 expected y≈120, got {}",
            ys[2]
        );
        assert!(
            (ys[3] - 160.0).abs() < 0.5,
            "row 3 expected y≈160, got {}",
            ys[3]
        );
    }

    #[test]
    fn virtual_list_dyn_gap_contributes_to_row_positions_and_content_height() {
        let mut root = crate::tree::virtual_list_dyn(
            10,
            40.0,
            |i| format!("row-{i}"),
            |i| {
                crate::tree::column([crate::widgets::text::text(format!("row {i}"))])
                    .key(format!("row-{i}"))
                    .height(Size::Fixed(40.0))
            },
        )
        .gap(10.0);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 120.0));

        assert_eq!(
            root.children.len(),
            3,
            "rows 0, 1, and 2 should intersect a 120px viewport with 40px rows and 10px gaps"
        );
        let row_1 = root
            .children
            .iter()
            .find(|c| c.key.as_deref() == Some("row-1"))
            .expect("row 1 should be realized");
        assert!(
            (state.rect(&row_1.computed_id).y - 50.0).abs() < 0.5,
            "gap should place row 1 at y=50"
        );
        let metrics = state
            .scroll
            .metrics
            .get(&root.computed_id)
            .expect("virtual list writes scroll metrics");
        assert!(
            (metrics.content_h - 490.0).abs() < 0.5,
            "10 rows x 40 plus 9 gaps x 10 should be 490, got {}",
            metrics.content_h
        );
    }

    #[test]
    fn virtual_list_dyn_caches_measured_heights() {
        // Build a list where the first frame realizes rows 0..k, measuring
        // each. After layout the cache should hold those measurements and
        // the next frame should read them.
        let mut root = crate::tree::virtual_list_dyn(
            50,
            50.0,
            |i| format!("row-{i}"),
            |i| {
                crate::tree::column([crate::widgets::text::text(format!("r{i}"))])
                    .key(format!("row-{i}"))
                    .height(Size::Fixed(30.0))
            },
        );
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let measured = state
            .scroll
            .measured_row_heights
            .get(&root.computed_id)
            .expect("dynamic virtual list should populate the height cache");
        // The first pass measures the estimate-derived window, then
        // the anchored final pass can extend it with newly revealed
        // rows. At least six rows are visible/cached here.
        assert!(
            measured.len() >= 6,
            "expected ≥ 6 cached row heights, got {}",
            measured.len()
        );
        for by_width in measured.values() {
            let h = by_width
                .get(&300)
                .copied()
                .expect("measurement should be keyed at the 300px width bucket");
            assert!(
                (h - 30.0).abs() < 0.5,
                "expected cached height ≈ 30, got {h}"
            );
        }
    }

    #[test]
    fn virtual_list_dyn_measure_pass_seeds_intrinsic_cache_for_layout_pass() {
        // Regression for #59: the measure pass assigns the same row ids
        // the layout pass will use, so its intrinsic measurements land
        // in the per-pass cache and the layout pass re-measures the
        // same Hug rows as cache hits instead of reshaping every
        // visible row a second time.
        let mut root = crate::tree::virtual_list_dyn(
            50,
            20.0,
            |i| format!("row-{i}"),
            |i| {
                crate::tree::column([crate::widgets::text::text(format!("row body {i}"))])
                    .key(format!("row-{i}"))
                    .height(Size::Hug)
            },
        );
        let mut state = UiState::new();
        let _ = take_intrinsic_cache_stats();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stats = take_intrinsic_cache_stats();
        let realized = root.children.len() as u64;
        assert!(realized > 0, "test requires realized rows");
        assert!(
            stats.hits >= realized,
            "layout pass should re-measure realized rows from the intrinsic cache \
             (hits {} < realized {realized})",
            stats.hits
        );
    }

    #[test]
    fn virtual_list_dyn_preserves_visible_anchor_when_above_measurement_changes() {
        let make_root = || {
            crate::tree::virtual_list_dyn(
                100,
                40.0,
                |i| format!("row-{i}"),
                |i| {
                    crate::tree::column([crate::widgets::text::text(format!("r{i}"))])
                        .key(format!("row-{i}"))
                        .height(Size::Fixed(40.0))
                },
            )
        };
        let mut root = make_root();
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        state.scroll.offsets.insert(root.computed_id.clone(), 400.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let anchor = state
            .scroll
            .virtual_anchors
            .get(&root.computed_id)
            .cloned()
            .expect("dynamic list should store a visible anchor");
        let before_y = root
            .children
            .iter()
            .find(|child| child.key.as_deref() == Some(anchor.row_key.as_str()))
            .map(|child| state.rect(&child.computed_id).y)
            .expect("anchor row should be realized");
        let before_offset = state.scroll_offset(&root.computed_id);

        state
            .scroll
            .measured_row_heights
            .entry(root.computed_id.clone())
            .or_default()
            .entry("row-0".to_string())
            .or_default()
            .insert(300, 120.0);

        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let after_y = root
            .children
            .iter()
            .find(|child| child.key.as_deref() == Some(anchor.row_key.as_str()))
            .map(|child| state.rect(&child.computed_id).y)
            .expect("anchor row should remain realized");
        let after_offset = state.scroll_offset(&root.computed_id);

        assert!(
            (after_y - before_y).abs() < 0.5,
            "anchor row should stay at y={before_y}, got {after_y}"
        );
        assert!(
            (after_offset - (before_offset + 80.0)).abs() < 0.5,
            "offset should absorb the 80px measurement delta above anchor"
        );
    }

    #[test]
    fn virtual_list_dyn_height_cache_is_width_bucketed() {
        let mut root = crate::tree::virtual_list_dyn(
            20,
            50.0,
            |i| format!("row-{i}"),
            |i| {
                crate::tree::column([crate::widgets::text::text(format!("r{i}"))])
                    .key(format!("row-{i}"))
                    .height(Size::Fixed(30.0))
            },
        );
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 240.0, 200.0));

        let row_0 = state
            .scroll
            .measured_row_heights
            .get(&root.computed_id)
            .and_then(|m| m.get("row-0"))
            .expect("row 0 should be measured");
        assert!(
            row_0.contains_key(&300) && row_0.contains_key(&240),
            "expected width buckets 300 and 240, got {:?}",
            row_0.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn virtual_list_dyn_total_height_uses_measured_plus_estimate() {
        // Measured rows use their cached fixed 30px height; rows that
        // have not been seen at this width still use the 50px estimate.
        // An overshoot offset must clamp to the mixed measured/estimated
        // content height after the final visible measurements land.
        let make_root = || {
            crate::tree::virtual_list_dyn(
                20,
                50.0,
                |i| format!("row-{i}"),
                |i| {
                    crate::tree::column([crate::widgets::text::text(format!("r{i}"))])
                        .key(format!("row-{i}"))
                        .height(Size::Fixed(30.0))
                },
            )
        };
        let mut state = UiState::new();
        let mut root = make_root();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        state
            .scroll
            .offsets
            .insert(root.computed_id.clone(), 9999.0);
        let mut root2 = make_root();
        layout(&mut root2, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let measured = state
            .scroll
            .measured_row_heights
            .get(&root2.computed_id)
            .expect("dynamic virtual list should populate the height cache");
        let measured_sum = measured
            .values()
            .filter_map(|by_width| by_width.get(&300))
            .sum::<f32>();
        let measured_count = measured
            .values()
            .filter(|by_width| by_width.contains_key(&300))
            .count();
        let expected_total = measured_sum + (20 - measured_count) as f32 * 50.0;
        let expected_max_offset = expected_total - 200.0;

        let stored = state
            .scroll
            .offsets
            .get(&root2.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - expected_max_offset).abs() < 0.5,
            "expected offset clamped to {expected_max_offset}, got {stored}"
        );
    }

    // ---- append_only parity ---------------------------------------
    //
    // The append-only incremental path must produce byte-identical
    // geometry to the general dynamic path for any trim-then-append
    // frame sequence. These tests drive both variants through the same
    // script on independent `UiState`s and compare realized rows, stored
    // offsets, and visible ranges every frame (issue #107).

    /// A chat-shaped row whose height depends only on the *stable*
    /// message id (`first + i`), so a given message keeps its height as
    /// it shifts indices under head-trim. Heights are integer-valued so
    /// both paths' prefix sums are bit-identical regardless of summation
    /// order.
    fn parity_list(first: usize, count: usize, append_only: bool, pin: bool) -> El {
        let mut el = crate::tree::virtual_list_dyn(
            count,
            20.0,
            move |i| format!("m{}", first + i),
            move |i| {
                let id = first + i;
                let h = 30.0 + (id % 7) as f32 * 10.0;
                crate::tree::column([crate::widgets::text::text(format!("m{id}"))])
                    .key(format!("m{id}"))
                    .height(Size::Fixed(h))
            },
        )
        .key("chat")
        .gap(4.0);
        if pin {
            el = el.pin_end();
        }
        if append_only {
            el = el.append_only();
        }
        el
    }

    /// Realized rows as `(key, rect)`, in render order.
    fn parity_rows(root: &El, state: &UiState) -> Vec<(String, Rect)> {
        root.children
            .iter()
            .map(|c| {
                (
                    c.key.clone().unwrap_or_default(),
                    state.rect(&c.computed_id),
                )
            })
            .collect()
    }

    struct Frame {
        first: usize,
        count: usize,
        seed_offset: Option<f32>,
        request: Option<ScrollRequest>,
    }

    fn run_parity(frames: &[Frame], pin: bool) {
        let mut sg = UiState::new(); // general path
        let mut si = UiState::new(); // incremental (append_only) path

        for (n, f) in frames.iter().enumerate() {
            let viewport = Rect::new(0.0, 0.0, 300.0, 200.0);
            let step = |state: &mut UiState, append_only: bool| {
                let mut root = parity_list(f.first, f.count, append_only, pin);
                assign_ids(&mut root);
                if let Some(o) = f.seed_offset {
                    state.scroll.offsets.insert(root.computed_id.clone(), o);
                }
                if let Some(req) = &f.request {
                    state.push_scroll_requests(vec![req.clone()]);
                }
                layout(&mut root, state, viewport);
                root
            };

            let root_g = step(&mut sg, false);
            let root_i = step(&mut si, true);

            let rows_g = parity_rows(&root_g, &sg);
            let rows_i = parity_rows(&root_i, &si);
            assert_eq!(
                rows_g.len(),
                rows_i.len(),
                "frame {n}: realized row count differs (general {} vs incremental {})",
                rows_g.len(),
                rows_i.len()
            );
            for ((kg, rg), (ki, ri)) in rows_g.iter().zip(&rows_i) {
                assert_eq!(kg, ki, "frame {n}: realized key order differs");
                assert!(
                    (rg.x - ri.x).abs() < 1e-2
                        && (rg.y - ri.y).abs() < 1e-2
                        && (rg.w - ri.w).abs() < 1e-2
                        && (rg.h - ri.h).abs() < 1e-2,
                    "frame {n}: rect for {kg} differs: general {rg:?} vs incremental {ri:?}"
                );
            }

            let off_g = sg.scroll.offsets.get(&root_g.computed_id).copied();
            let off_i = si.scroll.offsets.get(&root_i.computed_id).copied();
            assert!(
                (off_g.unwrap_or(0.0) - off_i.unwrap_or(0.0)).abs() < 1e-2,
                "frame {n}: stored offset differs: general {off_g:?} vs incremental {off_i:?}"
            );
            assert_eq!(
                sg.visible_range("chat"),
                si.visible_range("chat"),
                "frame {n}: visible range differs"
            );
        }
    }

    #[test]
    fn append_only_matches_general_top_append_scroll_trim() {
        run_parity(
            &[
                // Start at the top.
                Frame {
                    first: 0,
                    count: 30,
                    seed_offset: Some(0.0),
                    request: None,
                },
                // Append 12 rows at the tail.
                Frame {
                    first: 0,
                    count: 42,
                    seed_offset: None,
                    request: None,
                },
                // Scroll to the middle.
                Frame {
                    first: 0,
                    count: 42,
                    seed_offset: Some(400.0),
                    request: None,
                },
                // Trim 8 off the head, append 16 at the tail.
                Frame {
                    first: 8,
                    count: 50,
                    seed_offset: None,
                    request: None,
                },
                // Append more while scrolled mid-list (anchor preservation).
                Frame {
                    first: 8,
                    count: 62,
                    seed_offset: None,
                    request: None,
                },
                // Trim again.
                Frame {
                    first: 20,
                    count: 62,
                    seed_offset: None,
                    request: None,
                },
            ],
            false,
        );
    }

    #[test]
    fn append_only_matches_general_to_row_key_request() {
        run_parity(
            &[
                Frame {
                    first: 0,
                    count: 40,
                    seed_offset: Some(0.0),
                    request: None,
                },
                // Programmatic jump to a specific message by key.
                Frame {
                    first: 0,
                    count: 40,
                    seed_offset: None,
                    request: Some(ScrollRequest::ToRowKey {
                        list_key: "chat".into(),
                        row_key: "m30".into(),
                        align: ScrollAlignment::Start,
                    }),
                },
                // Trim past the anchored row, then append.
                Frame {
                    first: 12,
                    count: 55,
                    seed_offset: None,
                    request: None,
                },
            ],
            false,
        );
    }

    /// A horizontal resize changes the width bucket, which the index
    /// can't reconcile — it must cold-rebuild for the new bucket and
    /// still match the general path (which re-reads the new bucket's
    /// measurements). Drives both variants through width changes
    /// interleaved with append/trim.
    #[test]
    fn append_only_matches_general_across_width_change() {
        let mut sg = UiState::new();
        let mut si = UiState::new();
        // (first, count, viewport_w)
        let script = [
            (0usize, 30usize, 300.0f32),
            (0, 40, 300.0),
            (0, 40, 240.0), // resize → new width bucket
            (6, 52, 240.0), // append + trim at the new width
            (6, 52, 360.0), // resize wider
        ];
        for (n, &(first, count, w)) in script.iter().enumerate() {
            let viewport = Rect::new(0.0, 0.0, w, 200.0);
            let step = |state: &mut UiState, append_only: bool| {
                let mut root = parity_list(first, count, append_only, false);
                assign_ids(&mut root);
                layout(&mut root, state, viewport);
                root
            };
            let rg = step(&mut sg, false);
            let ri = step(&mut si, true);
            let rows_g = parity_rows(&rg, &sg);
            let rows_i = parity_rows(&ri, &si);
            assert_eq!(rows_g.len(), rows_i.len(), "frame {n}: row count differs");
            for ((kg, a), (ki, b)) in rows_g.iter().zip(&rows_i) {
                assert_eq!(kg, ki, "frame {n}: key order differs");
                assert!(
                    (a.x - b.x).abs() < 1e-2
                        && (a.y - b.y).abs() < 1e-2
                        && (a.w - b.w).abs() < 1e-2
                        && (a.h - b.h).abs() < 1e-2,
                    "frame {n}: rect for {kg} differs: {a:?} vs {b:?}"
                );
            }
            assert_eq!(
                sg.visible_range("chat"),
                si.visible_range("chat"),
                "frame {n}: visible range differs"
            );
        }
    }

    #[test]
    fn append_only_matches_general_pin_end_stick_to_bottom() {
        run_parity(
            &[
                Frame {
                    first: 0,
                    count: 20,
                    seed_offset: None,
                    request: None,
                },
                // Tail grows: a pinned list must stay at the bottom.
                Frame {
                    first: 0,
                    count: 35,
                    seed_offset: None,
                    request: None,
                },
                // Ring-buffer: trim head while appending tail, still pinned.
                Frame {
                    first: 10,
                    count: 50,
                    seed_offset: None,
                    request: None,
                },
                Frame {
                    first: 25,
                    count: 70,
                    seed_offset: None,
                    request: None,
                },
            ],
            true,
        );
    }

    #[test]
    fn virtual_list_dyn_empty_count_realizes_no_children() {
        let mut root = crate::tree::virtual_list_dyn(
            0,
            50.0,
            |i| format!("row-{i}"),
            |i| crate::widgets::text::text(format!("r{i}")),
        );
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        assert_eq!(root.children.len(), 0);
    }

    #[test]
    #[should_panic(expected = "estimated_row_height > 0.0")]
    fn virtual_list_dyn_zero_estimate_panics() {
        let _ = crate::tree::virtual_list_dyn(
            10,
            0.0,
            |i| format!("row-{i}"),
            |i| crate::widgets::text::text(format!("r{i}")),
        );
    }

    #[test]
    fn text_runs_constructor_shape_smoke() {
        let el = crate::tree::text_runs([
            crate::widgets::text::text("Hello, "),
            crate::widgets::text::text("world").bold(),
            crate::tree::hard_break(),
            crate::widgets::text::text("of text").italic(),
        ]);
        assert_eq!(el.kind, Kind::Inlines);
        assert_eq!(el.children.len(), 4);
        assert!(matches!(
            el.children[1].font_weight,
            FontWeight::Bold | FontWeight::Semibold
        ));
        assert_eq!(el.children[2].kind, Kind::HardBreak);
        assert!(el.children[3].text_italic);
    }

    #[test]
    fn wrapped_text_hugs_multiline_height_from_available_width() {
        let mut root = column([crate::paragraph(
            "A longer sentence should wrap into multiple measured lines.",
        )])
        .width(Size::Fill(1.0))
        .height(Size::Hug);

        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 180.0, 200.0));

        let child_rect = state.rect(&root.children[0].computed_id);
        assert_eq!(child_rect.w, 180.0);
        assert!(
            child_rect.h > crate::tokens::TEXT_SM.size * 1.4,
            "expected multiline paragraph height, got {}",
            child_rect.h
        );
    }

    #[test]
    fn overlay_child_with_wrapped_text_measures_against_its_resolved_width() {
        // Regression: overlay_rect used to call `intrinsic(c)` with no
        // width hint, so a Fixed-width modal containing a wrappable
        // paragraph measured the paragraph as a single line — leaving
        // the modal's Hug height short by the wrapped lines and
        // crowding the buttons against the bottom edge of the panel
        // (rumble cert-pending modal showed this).
        //
        // The fix: pass the child's resolved width as the available
        // width for intrinsic measurement, mirroring what column/row
        // already do.
        const PANEL_W: f32 = 240.0;
        const PADDING: f32 = 18.0;
        const GAP: f32 = 12.0;

        let panel = column([
            crate::paragraph(
                "A long enough warning paragraph that it has to wrap onto a second line \
                 inside this narrow panel.",
            ),
            crate::widgets::button::button("OK").key("ok"),
        ])
        .width(Size::Fixed(PANEL_W))
        .height(Size::Hug)
        .padding(Sides::all(PADDING))
        .gap(GAP)
        .align(Align::Stretch);

        let mut root = crate::stack([panel])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));

        let panel_rect = state.rect(&root.children[0].computed_id);
        assert_eq!(panel_rect.w, PANEL_W, "panel keeps its Fixed width");

        let para_rect = state.rect(&root.children[0].children[0].computed_id);
        let button_rect = state.rect(&root.children[0].children[1].computed_id);

        // Paragraph wrapped to ≥ 2 lines (exact line count depends on
        // glyph metrics; just guard against the single-line bug).
        assert!(
            para_rect.h > crate::tokens::TEXT_SM.size * 1.4,
            "paragraph should wrap to multiple lines inside the Fixed-width panel; \
             got h={}",
            para_rect.h
        );

        // Panel height must accommodate top padding + paragraph +
        // gap + button + bottom padding. The bug was that the panel
        // came out exactly `padding + gap + 1-line-paragraph + button`
        // — short by the second wrap line — and the button overshot
        // the inner area, leaving zero pixels of bottom padding.
        let bottom_padding = (panel_rect.y + panel_rect.h) - (button_rect.y + button_rect.h);
        assert!(
            (bottom_padding - PADDING).abs() < 0.5,
            "expected {PADDING}px between button and panel bottom, got {bottom_padding}",
        );
    }

    #[test]
    fn row_with_fill_paragraph_propagates_height_to_parent_column() {
        // Regression: the Row branch of `intrinsic_constrained` called
        // `intrinsic(ch)` unconstrained, so a wrappable Fill child
        // (paragraph) measured as a single unwrapped line. Two such rows
        // in a column then got one-line-tall allocations and the second
        // row's gutter rect overlapped the first row's wrapped text
        // (chat-port event-log recipe in damascene-core/README.md hit this).
        //
        // The fix mirrors `layout_axis`: the Row intrinsic distributes
        // its available width across Fill children before measuring,
        // so wrappable Fill children see the width they will actually
        // be laid out at.
        const COL_W: f32 = 600.0;
        const GUTTER_W: f32 = 3.0;

        let long = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, \
                    sed do eiusmod tempor incididunt ut labore et dolore magna \
                    aliqua. Ut enim ad minim veniam, quis nostrud exercitation \
                    ullamco laboris nisi ut aliquip ex ea commodo consequat.";

        let make_row = || {
            let gutter = El::new(Kind::Custom("gutter"))
                .width(Size::Fixed(GUTTER_W))
                .height(Size::Fill(1.0));
            let body = crate::paragraph(long).width(Size::Fill(1.0));
            crate::row([gutter, body]).width(Size::Fill(1.0))
        };

        let mut root = column([make_row(), make_row()])
            .width(Size::Fixed(COL_W))
            .height(Size::Hug)
            .align(Align::Stretch);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, COL_W, 2000.0));

        let row0_rect = state.rect(&root.children[0].computed_id);
        let row1_rect = state.rect(&root.children[1].computed_id);
        let para0_rect = state.rect(&root.children[0].children[1].computed_id);

        // Both the paragraph rect and the row rect must reflect the
        // wrapped (multi-line) height. The bug pinned them to a single
        // line (~`TEXT_SM.line_height` = 20px), so the wrapped text
        // painted outside the row's allocated rect.
        let line_height = crate::tokens::TEXT_SM.line_height;
        assert!(
            para0_rect.h > line_height * 1.5,
            "paragraph should wrap to multiple lines at ~597px wide; \
             got h={} (line_height={})",
            para0_rect.h,
            line_height,
        );
        assert!(
            row0_rect.h > line_height * 1.5,
            "row 0 should accommodate the wrapped paragraph height; \
             got h={} (line_height={})",
            row0_rect.h,
            line_height,
        );

        // Sanity: row 1 sits below row 0's allocated rect, not above it.
        assert!(
            row1_rect.y >= row0_rect.y + row0_rect.h - 0.5,
            "row 1 starts at y={} but row 0 occupies y={}..{}",
            row1_rect.y,
            row0_rect.y,
            row0_rect.y + row0_rect.h,
        );
    }

    /// `min_width` floors a child whose resolved cross-axis size is
    /// below the floor. Tests against an `align(Start)` column so
    /// `Size::Fixed` doesn't get widened by the default Stretch
    /// alignment before clamping has a chance to apply.
    #[test]
    fn min_width_floors_resolved_cross_axis_size() {
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))
            .min_width(120.0)])
        .align(Align::Start)
        .width(Size::Fixed(500.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 500.0, 200.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.w - 120.0).abs() < 0.5,
            "expected child clamped up to 120 (intrinsic 40 < min 120), got w={}",
            child_rect.w,
        );
    }

    /// `max_width` caps a `Size::Fill` child even when the surrounding
    /// row offers more space.
    #[test]
    fn max_width_caps_fill_child() {
        let mut root = crate::row([crate::widgets::text::text("body")
            .width(Size::Fill(1.0))
            .height(Size::Fixed(20.0))
            .max_width(160.0)])
        .width(Size::Fixed(800.0))
        .height(Size::Fixed(40.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 40.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.w - 160.0).abs() < 0.5,
            "expected Fill child capped at 160, got w={}",
            child_rect.w,
        );
    }

    /// When `min_width` and `max_width` conflict, the lower bound wins
    /// (CSS `min-width` precedence over `max-width`).
    #[test]
    fn min_width_wins_over_max_width_when_conflicting() {
        let mut root = column([crate::widgets::text::text("x")
            .width(Size::Fixed(50.0))
            .height(Size::Fixed(20.0))
            .max_width(80.0)
            .min_width(120.0)]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 500.0, 200.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.w - 120.0).abs() < 0.5,
            "expected min_width (120) to win over max_width (80), got w={}",
            child_rect.w,
        );
    }

    /// `min_height` floors a Hug child column whose children sum to
    /// less than the floor. Tested through a fixed-size parent so the
    /// resolved rect of the inner column reflects the clamp.
    #[test]
    fn min_height_floors_hug_column_inside_fixed_parent() {
        let inner = column([crate::widgets::text::text("a")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .width(Size::Fixed(80.0))
        .height(Size::Hug)
        .min_height(200.0);
        let mut root = column([inner])
            .align(Align::Start)
            .width(Size::Fixed(800.0))
            .height(Size::Fixed(600.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));
        let inner_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (inner_rect.h - 200.0).abs() < 0.5,
            "expected inner column floored to min_height=200 (intrinsic ~20), got h={}",
            inner_rect.h,
        );
    }

    /// Row laying out a `Fill` Hug-column with a wrap-text child must
    /// measure the column's height at the column's allocated width, not
    /// unconstrained. Repro for the lint regression that fires on a
    /// `row([column([wrap_text(...).fill_width()]).fill_width(), fixed])`
    /// shape: without the constrained measurement, the column reports
    /// its single-line unwrapped height to the row, the row sizes the
    /// column rect at that height, and the wrapped text then overflows
    /// the column vertically (Overflow `B=N` finding).
    #[test]
    fn row_passes_allocated_width_to_hug_column_with_wrap_text_child() {
        // 200px-wide row. The fixed child takes 40; the Fill column gets
        // 200 - 40 - 12 (gap) = 148. The paragraph wraps at 148px to two
        // lines; the column's intrinsic height should reflect that.
        let mut root = crate::row([
            column([crate::widgets::text::paragraph(
                "A long enough description that must wrap to two lines at 148px",
            )])
            .width(Size::Fill(1.0)),
            crate::widgets::text::text("ok")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(20.0)),
        ])
        .gap(12.0)
        .align(Align::Center)
        .width(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 600.0));
        // Find the column child (root.children[0]) and its paragraph leaf.
        let col_rect = state.rect(&root.children[0].computed_id);
        let para_rect = state.rect(&root.children[0].children[0].computed_id);
        assert!(
            (col_rect.h - para_rect.h).abs() < 0.5,
            "column height ({}) should track its wrapped child's height ({})",
            col_rect.h,
            para_rect.h,
        );
    }

    /// `Size::Aspect` on the main axis (height inside a Column) derives
    /// from the resolved cross size. Width fills its column's 200px;
    /// height should be 200 * 0.5 = 100.
    #[test]
    fn aspect_on_column_main_axis_derives_from_cross() {
        let mut root = column([El::new(Kind::Group)
            .width(Size::Fill(1.0))
            .height(Size::Aspect(0.5))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 400.0));
        let r = state.rect(&root.children[0].computed_id);
        assert!(
            (r.w - 200.0).abs() < 0.5,
            "expected w≈200 (Fill), got {}",
            r.w,
        );
        assert!(
            (r.h - 100.0).abs() < 0.5,
            "expected h≈100 (Aspect 0.5 of 200), got {}",
            r.h,
        );
    }

    /// Surrounding layout flows around an Aspect-sized image: a Hug
    /// column containing an Aspect-height El + a fixed-height sibling
    /// must have an outer height equal to derived height + sibling.
    #[test]
    fn aspect_height_pushes_siblings_in_column() {
        let mut root = column([
            El::new(Kind::Group)
                .width(Size::Fill(1.0))
                .height(Size::Aspect(0.25)),
            crate::widgets::text::text("caption")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(20.0)),
        ])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(500.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 500.0));
        let img = state.rect(&root.children[0].computed_id);
        let cap = state.rect(&root.children[1].computed_id);
        assert!(
            (img.h - 100.0).abs() < 0.5,
            "expected aspect-derived height ≈100, got {}",
            img.h,
        );
        assert!(
            (cap.y - 100.0).abs() < 0.5,
            "caption should sit immediately below the aspect-sized El (y≈100), got y={}",
            cap.y,
        );
    }

    /// `Size::Aspect` on the cross axis (width inside a Row) derives
    /// from the resolved main (height). Height fills 200; width should
    /// be 200 * 2.0 = 400.
    #[test]
    fn aspect_on_row_cross_axis_derives_from_main() {
        let mut root = crate::row([El::new(Kind::Group)
            .height(Size::Fill(1.0))
            .width(Size::Aspect(2.0))])
        .width(Size::Fixed(800.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 200.0));
        let r = state.rect(&root.children[0].computed_id);
        assert!(
            (r.h - 200.0).abs() < 0.5,
            "expected h≈200 (Fill), got {}",
            r.h,
        );
        assert!(
            (r.w - 400.0).abs() < 0.5,
            "expected w≈400 (Aspect 2.0 of 200), got {}",
            r.w,
        );
    }

    /// Both axes `Aspect` is degenerate — falls back to intrinsic so
    /// the El still has a finite measure.
    #[test]
    fn aspect_on_both_axes_falls_back_to_intrinsic() {
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Aspect(1.0))
            .height(Size::Aspect(1.0))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r = state.rect(&root.children[0].computed_id);
        assert!(
            r.w > 0.0 && r.h > 0.0,
            "expected finite size for both-Aspect fallback, got {}x{}",
            r.w,
            r.h,
        );
    }

    /// `max_height` and `min_height` cap the Aspect-derived axis, and
    /// the hugging parent's intrinsic agrees with the layout-time size
    /// (no overflow, no gap).
    #[test]
    fn aspect_respects_min_and_max_on_derived_axis() {
        // Case 1: max_height caps a too-tall derived height.
        // Fill(1.0) width inside Fixed(400) → 400 wide; Aspect(1.0) →
        // 400 tall; max_height=120 → clamped to 120.
        let mut root = column([column([El::new(Kind::Group)
            .width(Size::Fill(1.0))
            .height(Size::Aspect(1.0))
            .max_height(120.0)])
        .width(Size::Hug)
        .height(Size::Hug)])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(600.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 600.0));
        let panel = state.rect(&root.children[0].computed_id);
        let img = state.rect(&root.children[0].children[0].computed_id);
        assert!(
            (img.h - 120.0).abs() < 0.5,
            "max_height should clamp aspect-derived height to 120, got {}",
            img.h,
        );
        assert!(
            (panel.h - 120.0).abs() < 0.5,
            "hugging panel should match clamped child (120), got {}",
            panel.h,
        );

        // Case 2: min_height pushes a too-short derived height up.
        // Aspect(0.1) of 400 = 40; min_height=200 → bumped to 200.
        let mut root = column([column([El::new(Kind::Group)
            .width(Size::Fill(1.0))
            .height(Size::Aspect(0.1))
            .min_height(200.0)])
        .width(Size::Hug)
        .height(Size::Hug)])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(600.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 600.0));
        let panel = state.rect(&root.children[0].computed_id);
        let img = state.rect(&root.children[0].children[0].computed_id);
        assert!(
            (img.h - 200.0).abs() < 0.5,
            "min_height should bump aspect-derived height to 200, got {}",
            img.h,
        );
        assert!(
            (panel.h - 200.0).abs() < 0.5,
            "hugging panel should match bumped child (200), got {}",
            panel.h,
        );
    }

    /// `max_width` on the basis axis caps the Fill basis *before* the
    /// Aspect-derived axis is computed, matching the layout-time path.
    #[test]
    fn aspect_basis_is_clamped_before_deriving() {
        // Fill width in 400-wide column, but max_width=100 → basis=100.
        // Aspect(0.5) → height=50, not 200.
        // Align::Stretch (default) so Fill claims the column's cross
        // extent; with Align::Start a Fill child would shrink to its
        // intrinsic (0 for a bare Group), defeating the test.
        let mut root = column([El::new(Kind::Group)
            .width(Size::Fill(1.0))
            .height(Size::Aspect(0.5))
            .max_width(100.0)])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let img = state.rect(&root.children[0].computed_id);
        assert!(
            (img.w - 100.0).abs() < 0.5,
            "max_width should cap Fill width at 100, got {}",
            img.w,
        );
        assert!(
            (img.h - 50.0).abs() < 0.5,
            "aspect-derived height should follow clamped width (100 * 0.5 = 50), got {}",
            img.h,
        );
    }

    /// Regression: when a `Fill+Aspect` child sits inside a Hug column,
    /// the hugging column must size itself to the Aspect-derived height
    /// derived against the *parent's* available width, not the El's
    /// own natural intrinsic. Otherwise the column hugs too small and
    /// the child overflows downward at paint.
    #[test]
    fn hug_column_around_fill_aspect_child_does_not_overflow() {
        // Outer column is Fixed(400, 400) — the available width handed
        // down. Middle column hugs (the panel/card surrogate); inner
        // image has width=Fill, height=Aspect(0.5). At layout time the
        // image should be (400, 200), so the hugging panel must also
        // hug to (400, 200) — not to (nat_w, nat_w * 0.5) which would
        // be (1, 0.5) for a default-pixel-less El.
        let mut root = column([column([El::new(Kind::Group)
            .width(Size::Fill(1.0))
            .height(Size::Aspect(0.5))])
        .width(Size::Hug)
        .height(Size::Hug)])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let panel = state.rect(&root.children[0].computed_id);
        let img = state.rect(&root.children[0].children[0].computed_id);
        assert!(
            (panel.h - 200.0).abs() < 0.5,
            "hugging panel should hug to aspect-derived height 200, got {}",
            panel.h,
        );
        assert!(
            (img.h - 200.0).abs() < 0.5,
            "image should layout to height 200, got {}",
            img.h,
        );
        assert!(
            img.bottom() <= panel.bottom() + 0.5,
            "image (bottom={}) must fit within hugging panel (bottom={})",
            img.bottom(),
            panel.bottom(),
        );
    }

    /// When a parent hugs to its child and the child has `Aspect`, the
    /// hugging parent reports a size that matches what the child will
    /// actually paint at — the intrinsic post-step ensures consistency.
    #[test]
    fn hugging_parent_sees_aspect_corrected_intrinsic() {
        // Inner El has width=Fixed(80), height=Aspect(0.5) → intrinsic
        // height should derive to 40. The hugging column wrapping it
        // should size to (80, 40), not (80, 0) or (80, natural).
        let mut root = column([column([El::new(Kind::Group)
            .width(Size::Fixed(80.0))
            .height(Size::Aspect(0.5))])
        .width(Size::Hug)
        .height(Size::Hug)])
        .width(Size::Fixed(400.0))
        .height(Size::Fixed(400.0))
        .align(Align::Start);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 400.0));
        let hugger = state.rect(&root.children[0].computed_id);
        assert!(
            (hugger.w - 80.0).abs() < 0.5 && (hugger.h - 40.0).abs() < 0.5,
            "hugging parent should be 80x40 (matching aspect-corrected intrinsic), got {}x{}",
            hugger.w,
            hugger.h,
        );
    }

    /// `max_height` caps a `Hug` overlay child below its intrinsic.
    #[test]
    fn max_height_caps_overlay_child_below_intrinsic() {
        // Overlay parent sized 600x600; child Hug column whose intrinsic
        // height is 300 (single 300-tall fixed leaf), capped at 100.
        let mut root = crate::tree::stack([column([crate::widgets::text::text("tall")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(300.0))])
        .width(Size::Hug)
        .height(Size::Hug)
        .max_height(100.0)])
        .width(Size::Fixed(600.0))
        .height(Size::Fixed(600.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 600.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.h - 100.0).abs() < 0.5,
            "expected child height capped at 100, got h={}",
            child_rect.h,
        );
    }

    /// `.user_resizable()` (issue #106): layout publishes a grab band
    /// straddling the pane's trailing edge, sized off the pane's rect,
    /// with the clamp range from `min_width`/`max_width`.
    #[test]
    fn user_resizable_publishes_trailing_edge_band() {
        use crate::state::resize::RESIZE_BAND_THICKNESS as T;
        let mut root = crate::tree::row([
            column(Vec::<El>::new())
                .key("nav")
                .user_resizable()
                .width(Size::Fixed(200.0))
                .min_width(120.0)
                .max_width(420.0),
            column(Vec::<El>::new()).width(Size::Fill(1.0)),
        ])
        .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 400.0));

        assert_eq!(state.resize.bands.len(), 1);
        let band = &state.resize.bands[0];
        assert_eq!(band.id, "nav");
        assert_eq!(band.key.as_deref(), Some("nav"));
        assert_eq!(band.sign, 1.0, "trailing edge: drag-right grows");
        assert!((band.current - 200.0).abs() < 0.5);
        assert_eq!((band.min, band.max), (120.0, 420.0));
        // The band straddles the seam at x=200, full pane height.
        assert!((band.band.x - (200.0 - T / 2.0)).abs() < 0.5);
        assert!((band.band.w - T).abs() < 0.5);
        assert!((band.band.h - 400.0).abs() < 0.5);
    }

    /// A last-child resizable pane (right-anchored inspector) gets a
    /// leading-edge band with the drag direction flipped, and a
    /// missing `max_width` caps at the parent's inner extent so the
    /// seam can't be dragged out of reach.
    #[test]
    fn user_resizable_last_child_gets_leading_edge_band() {
        use crate::state::resize::RESIZE_BAND_THICKNESS as T;
        let mut root = crate::tree::row([
            column(Vec::<El>::new()).width(Size::Fill(1.0)),
            column(Vec::<El>::new())
                .key("inspector")
                .user_resizable()
                .width(Size::Fixed(240.0)),
        ])
        .height(Size::Fixed(400.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 400.0));

        assert_eq!(state.resize.bands.len(), 1);
        let band = &state.resize.bands[0];
        assert_eq!(band.sign, -1.0, "leading edge: drag-left grows");
        // Pane spans [560, 800]; the band straddles its left edge.
        assert!((band.band.x - (560.0 - T / 2.0)).abs() < 0.5);
        assert_eq!(band.min, 0.0);
        assert!(
            (band.max - 800.0).abs() < 0.5,
            "no max_width → capped at the parent's inner extent, got {}",
            band.max,
        );
    }

    /// A stored override rewrites the pane's declared size as `Fixed`
    /// on the next layout, clamped to min/max — including overrides
    /// pre-seeded via `set_user_size` before any drag.
    #[test]
    fn user_resizable_override_applies_and_clamps() {
        let build = || {
            crate::tree::row([
                column(Vec::<El>::new())
                    .key("nav")
                    .user_resizable()
                    .width(Size::Fixed(200.0))
                    .min_width(120.0)
                    .max_width(420.0),
                column(Vec::<El>::new()).key("main").width(Size::Fill(1.0)),
            ])
            .height(Size::Fixed(400.0))
        };
        let mut state = UiState::new();
        state.set_user_size("nav", 300.0);
        let mut root = build();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 400.0));
        let nav = state.rect_of_key("nav").unwrap();
        assert!((nav.w - 300.0).abs() < 0.5, "override wins, got {}", nav.w);
        let main = state.rect_of_key("main").unwrap();
        assert!(
            (main.w - 500.0).abs() < 0.5,
            "the Fill sibling absorbs the change, got {}",
            main.w,
        );

        // Out-of-range overrides clamp to the pane's declared bounds.
        state.set_user_size("nav", 9999.0);
        let mut root = build();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 400.0));
        assert!((state.rect_of_key("nav").unwrap().w - 420.0).abs() < 0.5);

        // Clearing returns the pane to its declared size.
        state.clear_user_size("nav");
        let mut root = build();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 400.0));
        assert!((state.rect_of_key("nav").unwrap().w - 200.0).abs() < 0.5);
    }

    /// A resizable pane in a `column` resizes its *height*: the band
    /// lies along the bottom edge and the clamp comes from the height
    /// bounds.
    #[test]
    fn user_resizable_in_column_resizes_height() {
        use crate::state::resize::RESIZE_BAND_THICKNESS as T;
        let mut root = column([
            crate::tree::row(Vec::<El>::new())
                .key("topbar")
                .user_resizable()
                .height(Size::Fixed(100.0))
                .min_height(40.0),
            crate::tree::row(Vec::<El>::new()).height(Size::Fill(1.0)),
        ])
        .width(Size::Fixed(800.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));

        assert_eq!(state.resize.bands.len(), 1);
        let band = &state.resize.bands[0];
        assert!((band.band.y - (100.0 - T / 2.0)).abs() < 0.5);
        assert!((band.band.w - 800.0).abs() < 0.5);
        assert_eq!(band.min, 40.0);
        assert!((band.current - 100.0).abs() < 0.5);

        state.set_user_size("topbar", 250.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert!((state.rect_of_key("topbar").unwrap().h - 250.0).abs() < 0.5);
    }
}
