//! App-side scroll request types.
//!
//! Apps push [`ScrollRequest`]s via [`crate::App::drain_scroll_requests`];
//! the layout pass resolves each one against the matching scroll
//! container's live viewport rect and writes the resulting offset into
//! the scroll state so the same frame's render reflects the new position.
//!
//! Mirrors [`crate::toast`]: the App produces fire-and-forget descriptors
//! and the runtime resolves them with state that's only fully known
//! mid-frame.

/// Where in the viewport a row-targeted [`ScrollRequest`] should land
/// its target row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAlignment {
    /// Top of the row aligns with the top of the viewport.
    Start,
    /// Centre of the row aligns with the centre of the viewport.
    Center,
    /// Bottom of the row aligns with the bottom of the viewport.
    End,
    /// No-op if the row already fits entirely inside the viewport;
    /// otherwise scroll the minimum amount that brings it into view
    /// (i.e., align to the nearer edge).
    Visible,
}

/// What the app produces from [`crate::App::drain_scroll_requests`].
/// Three shapes today:
///
/// - [`ScrollRequest::ToRow`] — "scroll the virtual list keyed
///   `list_key` so row `row` lands per `align`." Resolved during
///   layout of the matching `virtual_list` / `virtual_list_dyn` using
///   the live viewport and the row-height cache.
/// - [`ScrollRequest::ToRowKey`] — same operation, but targets the
///   virtual-list row by stable row identity instead of current row
///   index. Prefer this for `virtual_list_dyn` when the app already
///   has message/thread/commit ids.
/// - [`ScrollRequest::EnsureVisible`] — "scroll the nearest scroll
///   container under the node keyed `container_key` so the content-
///   space rect `y..y+h` is visible." Resolved during layout of the
///   matching `scroll(...)` container; minimal-displacement (top edge
///   if above viewport, bottom edge if below, no-op if already
///   visible). Used by [`crate::widgets::text_area`] for
///   caret-into-view on keyboard navigation, and available for any
///   widget that needs to keep an inner anchor on screen.
#[derive(Clone, Debug)]
pub enum ScrollRequest {
    /// Bring `row` of the virtual list keyed `list_key` into view per
    /// `align`.
    ToRow {
        list_key: String,
        row: usize,
        align: ScrollAlignment,
    },
    /// Bring the row identified by `row_key` in the virtual list keyed
    /// `list_key` into view per `align`.
    ToRowKey {
        list_key: String,
        row_key: String,
        align: ScrollAlignment,
    },
    /// Ensure the content-space rect at `y..y+h` is visible inside
    /// the scroll container under the node keyed `container_key`.
    /// `container_key` is the outer widget's key (e.g. the text_area's
    /// key) — the resolver descends to find the nearest `Kind::Scroll`
    /// inside that node.
    EnsureVisible {
        container_key: String,
        y: f32,
        h: f32,
    },
}

impl ScrollRequest {
    /// Construct a [`ScrollRequest::ToRow`]. Kept for source-compat
    /// with callers that predate the enum — `ScrollRequest::new(...)`
    /// has always meant "scroll a virtual list to a row."
    pub fn new(list_key: impl Into<String>, row: usize, align: ScrollAlignment) -> Self {
        ScrollRequest::ToRow {
            list_key: list_key.into(),
            row,
            align,
        }
    }

    /// Construct a [`ScrollRequest::ToRowKey`]. Dynamic virtual lists
    /// resolve this against the same stable row identities passed to
    /// [`crate::virtual_list_dyn`].
    pub fn to_row_key(
        list_key: impl Into<String>,
        row_key: impl Into<String>,
        align: ScrollAlignment,
    ) -> Self {
        ScrollRequest::ToRowKey {
            list_key: list_key.into(),
            row_key: row_key.into(),
            align,
        }
    }

    /// Construct a [`ScrollRequest::EnsureVisible`] for the widget
    /// keyed `container_key`, asking the resolver to keep
    /// `y..y+h` (in the scroll container's content coordinates)
    /// inside the viewport.
    pub fn ensure_visible(container_key: impl Into<String>, y: f32, h: f32) -> Self {
        ScrollRequest::EnsureVisible {
            container_key: container_key.into(),
            y,
            h,
        }
    }
}
