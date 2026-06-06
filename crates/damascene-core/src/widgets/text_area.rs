//! Multi-line text area widget with selection.
//!
//! `text_area(value, selection, key)` is the multi-line companion to
//! [`crate::widgets::text_input::text_input`]. It shares the same app
//! state shape — a `String` (with embedded `\n`s) and a
//! global [`crate::selection::Selection`] — and delegates its
//! invariants (focusable + capture_keys + paint_overflow + style
//! profile + clipboard helpers) to the same kit primitives.
//!
//! ```ignore
//! use damascene_core::scroll::ScrollRequest;
//! use damascene_core::prelude::*;
//!
//! struct Notes {
//!     body: String,
//!     selection: Selection,
//!     scroll_caret_into_view: bool,
//! }
//!
//! impl App for Notes {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         text_area(&self.body, &self.selection, "body").height(Size::Fixed(180.0))
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent, _cx: &EventCx) {
//!         if e.target_key() == Some("body")
//!             && text_area::apply_event(&mut self.body, &mut self.selection, "body", &e)
//!         {
//!             self.scroll_caret_into_view = true;
//!         } else if let Some(selection) = e.selection.clone() {
//!             self.selection = selection;
//!         }
//!     }
//!
//!     fn drain_scroll_requests(&mut self) -> Vec<ScrollRequest> {
//!         if std::mem::take(&mut self.scroll_caret_into_view) {
//!             text_area::caret_scroll_request_for(&self.body, &self.selection, "body")
//!                 .into_iter()
//!                 .collect()
//!         } else {
//!             Vec::new()
//!         }
//!     }
//!
//!     fn selection(&self) -> Selection {
//!         self.selection.clone()
//!     }
//! }
//! ```
//!
//! Fixed-height text areas scroll internally. If the app wants
//! keyboard navigation (`PageUp` / `PageDown`, arrows, `Home` /
//! `End`, text insertion, paste) to keep the caret visible, set an
//! app-owned flag when [`apply_event`] returns `true` and drain a
//! [`caret_scroll_request_for`] request on the next frame. Do not push
//! that request every frame; doing so would undo a manual wheel scroll
//! that intentionally leaves the caret offscreen.
//!
//! # Differences from `text_input`
//!
//! - The shaped text leaf has `wrap_text()` so cosmic-text wraps lines
//!   to the container's content width.
//! - The selection band is rendered as one rectangle per visual line
//!   covered by the selection (via [`crate::selection_rects`]).
//! - The caret bar is positioned in 2D using [`crate::caret_xy`].
//! - Up/Down arrows navigate by re-hitting `(current_x, current_y ±
//!   line_h)`, which preserves visual column.
//! - `Enter` inserts `"\n"` instead of being consumed by the focus
//!   activation path.
//! - `Home`/`End` operate on the current visual line (line-wise), not
//!   the whole document.
//!
//! Everything else — drag-select, Shift+arrow extension, Ctrl+A,
//! clipboard via [`crate::widgets::text_input::clipboard_request`] —
//! is shared with `text_input` (the helpers `apply_event` calls into
//! also work for multi-line values).

use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::metrics::MetricsRole;
use crate::selection::{Selection, SelectionPoint, SelectionRange};
use crate::style::StyleProfile;
use crate::text::metrics::{self, TextGeometry};
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::widgets::text_input::{
    TextSelection, delete_word_backward, delete_word_forward, replace_selection,
};

pub(crate) const TEXT_AREA_SELECTION_LAYER: &str = "text_area_selection_layer";
pub(crate) const TEXT_AREA_CARET_LAYER: &str = "text_area_caret_layer";

/// Build a multi-line text area that participates in the global
/// [`crate::selection::Selection`]. The widget reads its caret +
/// selection band through `selection.within(key)`; an event-time
/// edit writes back as a single-leaf range under `key` (transferring
/// selection ownership into this area).
///
/// # Layout
///
/// The value is rendered as **one wrapped shaped text leaf** so
/// cosmic-text lays out the entire buffer in one shape pass. The
/// selection bands and caret bar sit on top via overlay layout +
/// paint-time `translate`. Selection / caret pixel positions are
/// derived from [`crate::selection_rects`] and [`crate::caret_xy`].
///
/// # Sizing
///
/// Defaults to `Fill(1.0)` width and a Hug height — the field grows
/// to fit its content, starting at one-line tall when empty. Use the
/// standard `.height(Size::Fixed(...))` builder to give it a fixed
/// shape (typical for forms).
/// Optional knobs for [`text_area_with`]. Mirrors
/// [`crate::widgets::text_input::TextInputOpts`] but only carries the
/// options that make sense for the multi-line variant.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextAreaOpts<'a> {
    /// Hint shown in the input's muted style while the value is empty.
    /// Renders inside the same content rect as the text, so the user
    /// sees it disappear the moment they type.
    pub placeholder: Option<&'a str>,
}

impl<'a> TextAreaOpts<'a> {
    /// Set the placeholder hint.
    pub fn placeholder(mut self, p: &'a str) -> Self {
        self.placeholder = Some(p);
        self
    }
}

#[track_caller]
pub fn text_area(value: &str, selection: &Selection, key: &str) -> El {
    text_area_with(value, selection, key, TextAreaOpts::default())
}

/// Build a [`text_area`] with extra configuration (placeholder, etc.).
/// Same selection-ownership and event-routing contract as
/// [`text_area`]; see [`apply_event`] for the editing model.
#[track_caller]
pub fn text_area_with(value: &str, selection: &Selection, key: &str, opts: TextAreaOpts<'_>) -> El {
    build_text_area(key, value, selection.within(key), opts).key(key)
}

#[track_caller]
fn build_text_area(
    key: &str,
    value: &str,
    view: Option<TextSelection>,
    opts: TextAreaOpts<'_>,
) -> El {
    let mut content_children: Vec<El> = Vec::with_capacity(4);

    if view.is_some_and(|selection| !selection.is_collapsed()) {
        content_children.push(text_area_paint_layer(TEXT_AREA_SELECTION_LAYER, key, value));
    }

    // Placeholder hint — shown only while the value is empty. Sits at
    // the same origin as the (empty) text leaf so it visually fills the
    // gap; mirrors the placement used by `text_input::build_text_input`.
    if value.is_empty()
        && let Some(ph) = opts.placeholder
    {
        content_children.push(
            text(ph)
                .muted()
                .wrap_text()
                .width(Size::Fill(1.0))
                .height(Size::Hug),
        );
    }

    // The value rendered as one wrapped, shaped run. Hug height so the
    // inner content column grows to fit; Fill width so cosmic-text
    // wraps to the available width. When the outer text_area is given
    // a fixed height smaller than this content, the surrounding scroll
    // viewport clips and lets the user wheel through the overflow.
    content_children.push(
        text(value)
            .wrap_text()
            .width(Size::Fill(1.0))
            .height(Size::Hug),
    );

    // Caret bar — emitted only when the active selection actually
    // lives in this area. See the matching gate in
    // `text_input::build_text_input` for the rationale.
    if view.is_some() {
        content_children.push(
            text_area_paint_layer(TEXT_AREA_CARET_LAYER, key, value)
                .alpha_follows_focused_ancestor()
                .blink_when_focused(),
        );
    }

    // Inner overlay groups the bands + text + caret so paint-time
    // `translate` on bands/caret resolves against the same origin as
    // the text leaf's content rect. Hug height so the column its
    // wrapped in measures the real content extent (the scroll
    // viewport's max_offset depends on this).
    let inner = El::new(Kind::Custom("text_area_content"))
        .axis(Axis::Overlay)
        .align(Align::Start)
        .justify(Justify::Start)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .children(content_children);

    // Scroll viewport wraps the content. Provides clip + wheel +
    // scrollbar. Height Fill(1.0) makes it match the outer's content
    // rect — when the outer is `Size::Hug` (default), the scroll
    // hugs the inner content and no scrolling happens; when the
    // outer is `Size::Fixed(h)` for forms, the scroll viewport is
    // clamped to h and the inner content can overflow.
    //
    // Intentionally *unkeyed*: keys make a node a hit-test target,
    // which would steal pointer events from the outer text_area
    // (the outer carries `.focusable() .capture_keys()` and routes
    // by its own key). Offset persistence comes from the scroll's
    // computed_id, which stays stable as long as the scroll remains
    // the only child of text_area — the current shape and the one
    // we intend to keep.
    //
    // Unused while no `.key()` is set, but retained for the
    // upcoming `ScrollRequest::EnsureVisible` helper so the
    // resolver can recognize the request by the outer text_area's
    // key without needing the inner computed_id.
    let viewport = crate::tree::scroll([inner]);

    El::new(Kind::Custom("text_area"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::TextArea)
        .surface_role(SurfaceRole::Input)
        .focusable()
        // Same as text_input: ring stays on click too.
        .always_show_focus_ring()
        .capture_keys()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .hit_overflow(Sides::all(tokens::HIT_OVERFLOW))
        .cursor(Cursor::Text)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        // Single child (the scroll viewport); a Column with stretch
        // alignment makes it fill the content rect cleanly.
        .axis(Axis::Column)
        .align(Align::Stretch)
        .justify(Justify::Start)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .child(viewport)
}

fn text_area_paint_layer(kind: &'static str, key: &str, value: &str) -> El {
    let mut layer = El::new(Kind::Custom(kind))
        .style_profile(StyleProfile::Solid)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));
    layer.text_link = Some(key.to_string());
    layer.tooltip = Some(value.to_string());
    layer
}

fn line_height_px() -> f32 {
    tokens::TEXT_SM.line_height
}

fn text_area_geometry(value: &str, available_width: Option<f32>) -> TextGeometry<'_> {
    TextGeometry::new(
        value,
        tokens::TEXT_SM.size,
        FontWeight::Regular,
        false,
        if available_width.is_some() {
            TextWrap::Wrap
        } else {
            TextWrap::NoWrap
        },
        available_width,
    )
}

/// Build a [`crate::scroll::ScrollRequest::EnsureVisible`] for an in-flight drag
/// whose pointer has moved past the top or bottom edge of the
/// text_area's visible scroll viewport. Returns `None` when the
/// pointer is still inside the viewport, when the event isn't a
/// drag, when the event lacks pointer/target metadata, or when the
/// target isn't this text_area's outer key.
///
/// The returned request asks the runtime to scroll so the
/// pointer's content-space `y` is just inside the viewport. Apps
/// push it to [`crate::event::App::drain_scroll_requests`] the
/// same way [`caret_scroll_request_for`] is pushed.
///
/// Today this fires *only* when the pointer is fully past the edge
/// and only while drag events keep arriving — a perfectly still
/// pointer past the edge won't continue scrolling. Pumping a redraw
/// timer for hold-still autoscroll is a future improvement.
pub fn drag_autoscroll_request_for(
    event: &UiEvent,
    key: &str,
) -> Option<crate::scroll::ScrollRequest> {
    if !matches!(event.kind, UiEventKind::Drag) {
        return None;
    }
    let (_, py) = event.pointer?;
    let target = event.target.as_ref()?;
    if target.key != key {
        return None;
    }
    // Viewport bounds in absolute coords. Padding cuts both the top
    // and bottom of the outer rect.
    let viewport_top = target.rect.y + tokens::SPACE_2;
    let viewport_bottom = target.rect.bottom() - tokens::SPACE_2;
    let line_h = line_height_px();
    if py < viewport_top {
        // Pointer dragged above the visible top — expose the line
        // just above the current top of content.
        let content_y_above = (target.scroll_offset_y - line_h).max(0.0);
        Some(crate::scroll::ScrollRequest::ensure_visible(
            key,
            content_y_above,
            line_h,
        ))
    } else if py > viewport_bottom {
        // Pointer dragged below the visible bottom — expose the line
        // just below the current bottom of content.
        let viewport_h = (viewport_bottom - viewport_top).max(line_h);
        let content_y_below = target.scroll_offset_y + viewport_h;
        Some(crate::scroll::ScrollRequest::ensure_visible(
            key,
            content_y_below,
            line_h,
        ))
    } else {
        None
    }
}

/// Build a [`crate::scroll::ScrollRequest::EnsureVisible`] that keeps the caret of
/// the text_area keyed `key` inside its scroll viewport. Returns
/// `None` when the global selection doesn't currently live in this
/// area (typing in another field shouldn't move us).
///
/// Apps call this from [`crate::event::App::drain_scroll_requests`]
/// for fixed-height text areas, typically gated by a "selection or
/// content just changed" flag set when [`apply_event`] returns
/// `true`:
///
/// ```ignore
/// fn on_event(&mut self, e: UiEvent, _cx: &EventCx) {
///     if e.target_key() == Some("body")
///         && text_area::apply_event(&mut self.body, &mut self.selection, "body", &e)
///     {
///         self.scroll_caret_into_view = true;
///     }
/// }
///
/// fn drain_scroll_requests(&mut self) -> Vec<ScrollRequest> {
///     if std::mem::take(&mut self.scroll_caret_into_view) {
///         text_area::caret_scroll_request_for(&self.body, &self.selection, "body")
///             .into_iter()
///             .collect()
///     } else {
///         Vec::new()
///     }
/// }
/// ```
///
/// The runtime's resolver no-ops if the caret is already inside the
/// visible region. It is fine to request after each accepted text-area
/// event; *don't* push it every frame though — that would snap the
/// scroll back after a manual wheel that moved the caret offscreen.
pub fn caret_scroll_request_for(
    value: &str,
    selection: &Selection,
    key: &str,
) -> Option<crate::scroll::ScrollRequest> {
    let view = selection.within(key)?;
    let head = clamp_to_char_boundary(value, view.head.min(value.len()));
    let geometry = text_area_geometry(value, None);
    let (_, caret_y) = geometry.caret_xy(head);
    Some(crate::scroll::ScrollRequest::ensure_visible(
        key,
        caret_y,
        line_height_px(),
    ))
}

/// Fold a routed [`UiEvent`] into `value` and the global
/// [`Selection`]. Returns `true` when either was mutated.
///
/// Same contract as [`crate::widgets::text_input::apply_event`] plus
/// these multi-line additions:
///
/// - [`UiKey::ArrowUp`] / [`UiKey::ArrowDown`] move the caret one
///   visual line up / down, preserving visual column. With Shift the
///   selection extends; without Shift it collapses to the new caret.
/// - [`UiKey::Enter`] inserts `"\n"` (replaces the selection if
///   non-empty, otherwise inserts at the caret).
/// - `Home` / `End` go to the start / end of the current visual line.
/// - `PageUp` / `PageDown` move by roughly one visible page. With
///   Shift the selection extends; without Shift it collapses to the
///   new caret.
/// - Pointer events use 2D coordinates: clicking inside any line
///   positions the caret at the hit-tested glyph column.
///
/// On any mutation the selection is written back as a single-leaf
/// range under `key`, transferring ownership of the global selection
/// into this area. For fixed-height text areas, use the `true` return
/// value to queue [`caret_scroll_request_for`] from
/// [`crate::event::App::drain_scroll_requests`] so keyboard navigation
/// keeps the caret visible.
pub fn apply_event(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    event: &UiEvent,
) -> bool {
    let mut local = selection.within(key).unwrap_or_default();
    let changed = fold_event_local(value, &mut local, event);
    if changed {
        selection.range = Some(SelectionRange {
            anchor: SelectionPoint::new(key, local.anchor),
            head: SelectionPoint::new(key, local.head),
        });
    }
    changed
}

/// Apply the event to the area's local view. Internal worker behind
/// [`apply_event`]; pure in the sense that it doesn't touch
/// [`Selection`].
fn fold_event_local(value: &mut String, selection: &mut TextSelection, event: &UiEvent) -> bool {
    selection.anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    selection.head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let wrap_width = wrap_width_for_event(event);
    match event.kind {
        UiEventKind::TextInput => {
            let Some(insert) = event.text.as_deref() else {
                return false;
            };
            // See text_input::apply_event for the rationale: drop
            // shortcut-side TextInput emissions (Ctrl/Cmd held) so the
            // 'c' from Ctrl+C doesn't replace the selection after the
            // clipboard handler has already consumed the keystroke.
            if (event.modifiers.ctrl && !event.modifiers.alt) || event.modifiers.logo {
                return false;
            }
            let filtered: String = insert
                .chars()
                .filter(|c| *c == '\n' || !c.is_control())
                .collect();
            if filtered.is_empty() {
                return false;
            }
            replace_selection(value, selection, &filtered);
            true
        }
        UiEventKind::MiddleClick => {
            let Some(byte) = caret_byte_at_with_width(value, event, wrap_width) else {
                return false;
            };
            *selection = TextSelection::caret(byte);
            if let Some(insert) = event.text.as_deref() {
                replace_selection(value, selection, insert);
            }
            true
        }
        UiEventKind::KeyDown => {
            let Some(kp) = event.key_press.as_ref() else {
                return false;
            };
            let mods = kp.modifiers;
            // Ctrl+A: select all.
            if mods.ctrl
                && !mods.alt
                && !mods.logo
                && let UiKey::Character(c) = &kp.key
                && c.eq_ignore_ascii_case("a")
            {
                let len = value.len();
                if selection.anchor == 0 && selection.head == len {
                    return false;
                }
                *selection = TextSelection {
                    anchor: 0,
                    head: len,
                };
                return true;
            }
            // Ctrl+W: delete word backward (Emacs / terminal convention).
            if mods.ctrl
                && !mods.alt
                && !mods.logo
                && !mods.shift
                && let UiKey::Character(c) = &kp.key
                && c.eq_ignore_ascii_case("w")
            {
                return delete_word_backward(value, selection);
            }
            // Ctrl+J: insert newline (Emacs / terminal convention). Mirrors
            // the bare Enter handler below so apps that intercept Enter for
            // a "submit" action still leave Ctrl+J available for newline
            // insertion.
            if mods.ctrl
                && !mods.alt
                && !mods.logo
                && !mods.shift
                && let UiKey::Character(c) = &kp.key
                && c.eq_ignore_ascii_case("j")
            {
                replace_selection(value, selection, "\n");
                return true;
            }
            match kp.key {
                UiKey::Escape => {
                    if selection.is_collapsed() {
                        return false;
                    }
                    selection.anchor = selection.head;
                    true
                }
                UiKey::Enter => {
                    replace_selection(value, selection, "\n");
                    true
                }
                UiKey::Backspace => {
                    if !selection.is_collapsed() {
                        replace_selection(value, selection, "");
                        return true;
                    }
                    if selection.head == 0 {
                        return false;
                    }
                    if mods.ctrl && !mods.alt && !mods.logo {
                        return delete_word_backward(value, selection);
                    }
                    let prev = prev_char_boundary(value, selection.head);
                    value.replace_range(prev..selection.head, "");
                    selection.head = prev;
                    selection.anchor = prev;
                    true
                }
                UiKey::Delete => {
                    if !selection.is_collapsed() {
                        replace_selection(value, selection, "");
                        return true;
                    }
                    if selection.head >= value.len() {
                        return false;
                    }
                    if mods.ctrl && !mods.alt && !mods.logo {
                        return delete_word_forward(value, selection);
                    }
                    let next = next_char_boundary(value, selection.head);
                    value.replace_range(selection.head..next, "");
                    true
                }
                UiKey::ArrowLeft => {
                    let target = if selection.is_collapsed() || mods.shift {
                        if selection.head == 0 {
                            return false;
                        }
                        if mods.ctrl && !mods.alt && !mods.logo {
                            crate::selection::prev_word_boundary(value, selection.head)
                        } else {
                            prev_char_boundary(value, selection.head)
                        }
                    } else if mods.ctrl && !mods.alt && !mods.logo {
                        crate::selection::prev_word_boundary(value, selection.head)
                    } else {
                        selection.ordered().0
                    };
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                UiKey::ArrowRight => {
                    let target = if selection.is_collapsed() || mods.shift {
                        if selection.head >= value.len() {
                            return false;
                        }
                        if mods.ctrl && !mods.alt && !mods.logo {
                            crate::selection::next_word_boundary(value, selection.head)
                        } else {
                            next_char_boundary(value, selection.head)
                        }
                    } else if mods.ctrl && !mods.alt && !mods.logo {
                        crate::selection::next_word_boundary(value, selection.head)
                    } else {
                        selection.ordered().1
                    };
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                UiKey::ArrowUp => {
                    let new = move_caret_vertically(value, selection.head, -1, wrap_width);
                    if new == selection.head {
                        return false;
                    }
                    selection.head = new;
                    if !mods.shift {
                        selection.anchor = new;
                    }
                    true
                }
                UiKey::ArrowDown => {
                    let new = move_caret_vertically(value, selection.head, 1, wrap_width);
                    if new == selection.head {
                        return false;
                    }
                    selection.head = new;
                    if !mods.shift {
                        selection.anchor = new;
                    }
                    true
                }
                UiKey::PageUp => {
                    let new = move_caret_vertically(
                        value,
                        selection.head,
                        page_line_delta_for_event(event, -1),
                        wrap_width,
                    );
                    if new == selection.head {
                        return false;
                    }
                    selection.head = new;
                    if !mods.shift {
                        selection.anchor = new;
                    }
                    true
                }
                UiKey::PageDown => {
                    let new = move_caret_vertically(
                        value,
                        selection.head,
                        page_line_delta_for_event(event, 1),
                        wrap_width,
                    );
                    if new == selection.head {
                        return false;
                    }
                    selection.head = new;
                    if !mods.shift {
                        selection.anchor = new;
                    }
                    true
                }
                UiKey::Home => {
                    let target = if mods.ctrl && !mods.alt && !mods.logo {
                        // Ctrl+Home: jump to start of the whole document.
                        0
                    } else {
                        visual_line_range(value, selection.head, wrap_width).0
                    };
                    if selection.head == target && (mods.shift || selection.anchor == target) {
                        return false;
                    }
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                UiKey::End => {
                    let target = if mods.ctrl && !mods.alt && !mods.logo {
                        // Ctrl+End: jump to end of the whole document.
                        value.len()
                    } else {
                        visual_line_range(value, selection.head, wrap_width).1
                    };
                    if selection.head == target && (mods.shift || selection.anchor == target) {
                        return false;
                    }
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                _ => false,
            }
        }
        UiEventKind::PointerDown => {
            let (Some((px, py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_3;
            // After stage 1 wrapped the content in a scroll viewport,
            // the visible glyph row at viewport-y=N is content-y=N +
            // offset. `UiTarget.scroll_offset_y` carries the nearest
            // descendant scroll's offset so the hit lands on the
            // right content line. Without this, clicks on a scrolled
            // text_area placed the caret on the line that was where
            // the pointer is *now* without scrolling — off by the
            // current offset.
            let local_y = py - target.rect.y - tokens::SPACE_2 + target.scroll_offset_y;
            let pos = caret_from_xy(value, local_x, local_y, wrap_width);
            // Multi-click: 2 = word at caret, ≥3 = line containing
            // caret (delimited by '\n'). Shift+multi-click falls
            // through to the extend behavior, same as text_input.
            if !event.modifiers.shift {
                match event.click_count {
                    2 => {
                        let (lo, hi) = crate::selection::word_range_at(value, pos);
                        selection.anchor = lo;
                        selection.head = hi;
                        return true;
                    }
                    n if n >= 3 => {
                        let (lo, hi) = crate::selection::line_range_at(value, pos);
                        selection.anchor = lo;
                        selection.head = hi;
                        return true;
                    }
                    _ => {}
                }
            }
            selection.head = pos;
            if !event.modifiers.shift {
                selection.anchor = pos;
            }
            true
        }
        UiEventKind::LongPress => {
            let (Some((px, py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_3;
            let local_y = py - target.rect.y - tokens::SPACE_2 + target.scroll_offset_y;
            let pos = caret_from_xy(value, local_x, local_y, wrap_width);
            let (lo, hi) = crate::selection::word_range_at(value, pos);
            selection.anchor = lo;
            selection.head = hi;
            true
        }
        UiEventKind::Drag => {
            let (Some((px, py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_3;
            // See PointerDown above for the scroll_offset_y rationale.
            let local_y = py - target.rect.y - tokens::SPACE_2 + target.scroll_offset_y;
            let pos = caret_from_xy(value, local_x, local_y, wrap_width);
            if !event.modifiers.shift {
                match event.click_count {
                    2 => {
                        extend_word_selection(value, selection, pos);
                        return true;
                    }
                    n if n >= 3 => {
                        extend_line_selection(value, selection, pos);
                        return true;
                    }
                    _ => {}
                }
            }
            selection.head = pos;
            true
        }
        UiEventKind::Click => false,
        _ => false,
    }
}

fn extend_word_selection(value: &str, selection: &mut TextSelection, pos: usize) {
    let (selected_lo, selected_hi) = selection.ordered();
    let (word_lo, word_hi) = crate::selection::word_range_at(value, pos);
    if pos < selected_lo {
        selection.anchor = selected_hi;
        selection.head = word_lo;
    } else {
        selection.anchor = selected_lo;
        selection.head = word_hi;
    }
}

fn extend_line_selection(value: &str, selection: &mut TextSelection, pos: usize) {
    let (selected_lo, selected_hi) = selection.ordered();
    let (line_lo, line_hi) = crate::selection::line_range_at(value, pos);
    if pos < selected_lo {
        selection.anchor = selected_hi;
        selection.head = line_lo;
    } else {
        selection.anchor = selected_lo;
        selection.head = line_hi;
    }
}

/// Return the byte offset of the caret position one visual line in
/// `direction` from `byte_index`; larger magnitudes move multiple
/// visual lines. Returns the input unchanged when there is no line in
/// that direction (already at the first / last visual line).
fn move_caret_vertically(
    value: &str,
    byte_index: usize,
    direction: i32,
    available_width: Option<f32>,
) -> usize {
    let geometry = text_area_geometry(value, available_width);
    let (x, y) = geometry.caret_xy(byte_index);
    let line_h = geometry.line_height();
    let target_y = y + direction as f32 * line_h;
    if target_y < -0.5 {
        // Above the first line — clamp to start of value.
        return 0;
    }
    // Probe slightly inside the target line to make the geometry hit-test find it.
    let probe_y = target_y + line_h * 0.5;
    let Some(byte) = geometry.hit_byte(x, probe_y) else {
        // No line at probe_y — past the bottom of the text. Clamp to end.
        return value.len();
    };
    byte
}

fn page_line_delta_for_event(event: &UiEvent, direction: i32) -> i32 {
    let visible_h = event
        .target
        .as_ref()
        .map(|target| (target.rect.h - 2.0 * tokens::SPACE_2).max(line_height_px()))
        .unwrap_or(line_height_px() * 10.0);
    let lines = (visible_h / line_height_px()).floor().max(1.0) as i32;
    direction * lines
}

/// Resolve the byte offset a pointer event maps to inside a text
/// area's `value`. Mirrors [`crate::widgets::text_input::caret_byte_at`]
/// but accounts for vertical position as well, so the caller lands on
/// the line under the pointer. Returns `None` for events without a
/// pointer or target rect. Used by Linux middle-click paste flows.
#[track_caller]
pub fn caret_byte_at(value: &str, event: &UiEvent) -> Option<usize> {
    caret_byte_at_with_width(value, event, wrap_width_for_event(event))
}

fn caret_byte_at_with_width(
    value: &str,
    event: &UiEvent,
    available_width: Option<f32>,
) -> Option<usize> {
    let (px, py) = event.pointer?;
    let target = event.target.as_ref()?;
    let local_x = px - target.rect.x - tokens::SPACE_3;
    // Same scroll-offset adjustment as the PointerDown / Drag paths
    // in apply_event — `target.scroll_offset_y` is set by hit-test
    // to the nearest descendant scroll's stored offset, so we get
    // content-space y rather than viewport-space y.
    let local_y = py - target.rect.y - tokens::SPACE_2 + target.scroll_offset_y;
    Some(caret_from_xy(value, local_x, local_y, available_width))
}

fn caret_from_xy(value: &str, x: f32, y: f32, available_width: Option<f32>) -> usize {
    let geometry = text_area_geometry(value, available_width);
    let line_h = geometry.line_height();
    let probe_y = y.max(line_h * 0.5);
    let Some(byte) = geometry.hit_byte(x.max(0.0), probe_y) else {
        return value.len();
    };
    byte
}

fn visual_line_range(
    value: &str,
    byte_index: usize,
    available_width: Option<f32>,
) -> (usize, usize) {
    metrics::visual_line_byte_range(
        value,
        byte_index,
        tokens::TEXT_SM.size,
        FontWeight::Regular,
        if available_width.is_some() {
            TextWrap::Wrap
        } else {
            TextWrap::NoWrap
        },
        available_width,
    )
}

fn wrap_width_for_event(event: &UiEvent) -> Option<f32> {
    event
        .target
        .as_ref()
        .map(|target| (target.rect.w - 2.0 * tokens::SPACE_3).max(1.0))
}

fn clamp_to_char_boundary(s: &str, idx: usize) -> usize {
    let mut idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn prev_char_boundary(s: &str, from: usize) -> usize {
    let mut i = from.saturating_sub(1);
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, from: usize) -> usize {
    let mut i = (from + 1).min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, PointerKind};

    /// Test key for the local-view shim. Mirrors the one in
    /// `text_input::tests`; lets the existing test bodies keep using
    /// `apply_event(&mut value, &mut sel, &event)` against the new
    /// `(value, &mut Selection, key, event)` API.
    const TEST_KEY: &str = "ta";

    fn text_area(value: &str, sel: TextSelection) -> El {
        super::text_area(value, &as_selection(sel), TEST_KEY)
    }

    fn apply_event(value: &mut String, sel: &mut TextSelection, event: &UiEvent) -> bool {
        let mut g = as_selection(*sel);
        let changed = super::apply_event(value, &mut g, TEST_KEY, event);
        match g.within(TEST_KEY) {
            Some(view) => *sel = view,
            None => *sel = TextSelection::default(),
        }
        changed
    }

    fn as_selection(sel: TextSelection) -> Selection {
        Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new(TEST_KEY, sel.anchor),
                head: SelectionPoint::new(TEST_KEY, sel.head),
            }),
        }
    }

    fn ev_key(key: UiKey) -> UiEvent {
        ev_key_with_mods(key, KeyModifiers::default())
    }

    fn ev_key_with_mods(key: UiKey, modifiers: KeyModifiers) -> UiEvent {
        UiEvent {
            path: None,
            key: None,
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        }
    }

    fn ev_key_with_target(key: UiKey, target: crate::event::UiTarget) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: None,
            key_press: Some(KeyPress {
                key,
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

    fn ev_key_with_mods_and_target(
        key: UiKey,
        modifiers: KeyModifiers,
        target: crate::event::UiTarget,
    ) -> UiEvent {
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::KeyDown,
        }
    }

    fn ta_target() -> crate::event::UiTarget {
        crate::event::UiTarget {
            key: "ta".to_string(),
            node_id: "/ta".to_string(),
            rect: crate::tree::Rect::new(0.0, 0.0, 200.0, 100.0),
            tooltip: None,
            scroll_offset_y: 0.0,
        }
    }

    fn ev_pointer_down_with_count(
        local: (f32, f32),
        modifiers: KeyModifiers,
        click_count: u8,
    ) -> UiEvent {
        let target = ta_target();
        let pointer = (
            target.rect.x + tokens::SPACE_3 + local.0,
            target.rect.y + tokens::SPACE_2 + local.1,
        );
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            selection: None,
            modifiers,
            click_count,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::PointerDown,
        }
    }

    fn ev_long_press(local: (f32, f32)) -> UiEvent {
        let target = ta_target();
        let pointer = (
            target.rect.x + tokens::SPACE_3 + local.0,
            target.rect.y + tokens::SPACE_2 + local.1,
        );
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: Some(PointerKind::Touch),
            wheel_delta: None,
            kind: UiEventKind::LongPress,
        }
    }

    fn ev_drag_with_count(local: (f32, f32), click_count: u8) -> UiEvent {
        let target = ta_target();
        let pointer = (
            target.rect.x + tokens::SPACE_3 + local.0,
            target.rect.y + tokens::SPACE_2 + local.1,
        );
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::Drag,
        }
    }

    fn ev_middle_click(local: (f32, f32), text: Option<&str>) -> UiEvent {
        let target = ta_target();
        let pointer = (
            target.rect.x + tokens::SPACE_3 + local.0,
            target.rect.y + tokens::SPACE_2 + local.1,
        );
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: text.map(str::to_string),
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::MiddleClick,
        }
    }

    #[test]
    fn text_area_declares_text_cursor() {
        let el = text_area("hello", TextSelection::caret(0));
        assert_eq!(el.cursor, Some(Cursor::Text));
    }

    #[test]
    fn enter_inserts_newline_and_advances_caret() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Enter)));
        assert_eq!(value, "he\nllo");
        assert_eq!(sel, TextSelection::caret(3));
    }

    #[test]
    fn escape_collapses_selection_without_editing() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::range(1, 4);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Escape)));
        assert_eq!(value, "hello");
        assert_eq!(sel, TextSelection::caret(4));
        assert!(!apply_event(&mut value, &mut sel, &ev_key(UiKey::Escape)));
    }

    #[test]
    fn middle_click_inserts_event_text_at_pointer() {
        let mut value = String::from("world");
        let mut sel = TextSelection::caret(value.len());
        let event = ev_middle_click((0.0, tokens::TEXT_SM.line_height * 0.5), Some("hello "));
        assert!(apply_event(&mut value, &mut sel, &event));
        assert_eq!(value, "hello world");
        assert_eq!(sel, TextSelection::caret("hello ".len()));
    }

    #[test]
    fn arrow_down_moves_to_next_line_at_similar_column() {
        let mut value = String::from("alpha\nbravo");
        // Caret at 'p' in alpha (index 2). Down should land near 'a'
        // in bravo (index 8 = 6 + 2).
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowDown)));
        assert!(
            (8..=10).contains(&sel.head),
            "head={} not near column 2 of line 2",
            sel.head
        );
        assert_eq!(sel.anchor, sel.head);
    }

    #[test]
    fn arrow_up_at_top_clamps_to_start() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowUp)));
        assert_eq!(sel, TextSelection::caret(0));
    }

    #[test]
    fn page_up_down_move_by_visible_page() {
        let mut value = String::from("a\nb\nc\nd\ne\nf");
        let mut sel = TextSelection::caret(0);
        let mut target = ta_target();
        target.rect.h = line_height_px() * 3.0 + 2.0 * tokens::SPACE_2;

        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_target(UiKey::PageDown, target.clone())
        ));
        assert_eq!(sel, TextSelection::caret(6));

        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_target(UiKey::PageUp, target)
        ));
        assert_eq!(sel, TextSelection::caret(0));
    }

    #[test]
    fn shift_page_down_extends_selection() {
        let mut value = String::from("a\nb\nc\nd");
        let mut sel = TextSelection::caret(0);
        let mut target = ta_target();
        target.rect.h = line_height_px() * 2.0 + 2.0 * tokens::SPACE_2;
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };

        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods_and_target(UiKey::PageDown, shift, target)
        ));
        assert_eq!(sel, TextSelection::range(0, 4));
    }

    #[test]
    fn home_goes_to_current_line_start() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(8); // 'a' in bravo
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Home)));
        assert_eq!(sel, TextSelection::caret(6));
    }

    #[test]
    fn end_goes_to_current_line_end() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(7); // 'r' in bravo
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::End)));
        assert_eq!(sel, TextSelection::caret(11));
    }

    #[test]
    fn home_and_end_respect_soft_wrapped_visual_lines() {
        let mut value = String::from("alpha beta gamma");
        let gamma = value.find("gamma").unwrap();
        let mut target = ta_target();
        target.rect.w = 80.0;

        let mut sel = TextSelection::caret(gamma + 2);
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_target(UiKey::Home, target.clone())
        ));
        assert!(
            sel.head > 0 && sel.head <= gamma,
            "Home should move to the soft line start near gamma, got {:?}",
            sel
        );

        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_target(UiKey::End, target)
        ));
        assert_eq!(sel, TextSelection::caret(value.len()));
    }

    #[test]
    fn shift_arrow_down_extends_selection_anchor_stays() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(2);
        let mods = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::ArrowDown, mods)
        ));
        assert_eq!(sel.anchor, 2);
        assert!(sel.head > 2);
    }

    #[test]
    fn double_click_selects_word_at_caret() {
        let mut value = String::from("first second\nthird");
        let mut sel = TextSelection::caret(0);
        // Click at top-left → caret_from_xy returns ~0; word_range_at
        // returns "first" → bytes 0..5.
        let down = ev_pointer_down_with_count((1.0, 1.0), KeyModifiers::default(), 2);
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, 5);
    }

    #[test]
    fn long_press_selects_word_at_caret() {
        let mut value = String::from("first second\nthird");
        let mut sel = TextSelection::caret(0);
        let event = ev_long_press((1.0, 1.0));

        assert!(apply_event(&mut value, &mut sel, &event));
        assert_eq!(sel, TextSelection::range(0, 5));
    }

    #[test]
    fn double_click_hold_drag_inside_word_keeps_word_selected() {
        let mut value = String::from("first second\nthird");
        let mut sel = TextSelection::caret(0);
        let down = ev_pointer_down_with_count((1.0, 1.0), KeyModifiers::default(), 2);
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel, TextSelection::range(0, 5));

        let drag = ev_drag_with_count((2.0, 1.0), 2);
        assert!(apply_event(&mut value, &mut sel, &drag));
        assert_eq!(sel, TextSelection::range(0, 5));
    }

    #[test]
    fn triple_click_selects_line_around_caret_not_whole_value() {
        // text_area's triple-click selects the *line* (delimited by
        // '\n'), not the whole value — the user might have a long
        // multi-line note and want to grab a single paragraph.
        let mut value = String::from("first line\nsecond line\nthird");
        let mut sel = TextSelection::caret(0);
        // Click at top-left → first line.
        let down = ev_pointer_down_with_count((1.0, 1.0), KeyModifiers::default(), 3);
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, 10, "selects 'first line' (excludes the \\n)");
    }

    #[test]
    fn ctrl_or_cmd_text_input_is_dropped() {
        // Mirror of the text_input regression: winit can emit
        // TextInput("c") alongside KeyDown(Ctrl+C) on some platforms,
        // and the clipboard wrapper consumes the KeyDown. Without the
        // ctrl/cmd guard, 'c' would replace the selection after the
        // copy.
        let mut value = String::from("first\nsecond");
        let mut sel = TextSelection::range(0, value.len());
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        let ev = UiEvent {
            path: None,
            key: None,
            target: None,
            pointer: None,
            key_press: None,
            text: Some("c".into()),
            selection: None,
            modifiers: ctrl,
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::TextInput,
        };
        assert!(!apply_event(&mut value, &mut sel, &ev));
        assert_eq!(value, "first\nsecond");
    }

    #[test]
    fn text_area_preserves_newlines_for_multiline_paste() {
        let mut value = String::from("alpha");
        let mut sel = TextSelection::caret(value.len());
        let ev = UiEvent {
            path: None,
            key: None,
            target: None,
            pointer: None,
            key_press: None,
            text: Some("\nbeta\n".into()),
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::TextInput,
        };
        assert!(apply_event(&mut value, &mut sel, &ev));
        assert_eq!(value, "alpha\nbeta\n");
        assert_eq!(sel, TextSelection::caret(value.len()));
    }

    #[test]
    fn caret_scroll_request_brings_offscreen_caret_into_view() {
        // Regression for stage 2 of the rework: caret-into-view via
        // `ScrollRequest::EnsureVisible`. We build a tall multi-line
        // value so the inner scroll has plenty of overflow, anchor
        // the caret way past the bottom, then push the request the
        // way an app's `drain_scroll_requests` would, and assert the
        // inner scroll's offset shifted to expose the caret line.
        let value = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
        // Caret near the end of the body, in the bottom third.
        let caret_byte = clamp_to_char_boundary(&value, value.len() - 1);
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new(TEST_KEY, caret_byte),
                head: SelectionPoint::new(TEST_KEY, caret_byte),
            }),
        };
        let mut root = super::text_area(&value, &sel, TEST_KEY)
            .height(Size::Fixed(80.0))
            .width(Size::Fixed(240.0));

        let req = caret_scroll_request_for(&value, &sel, TEST_KEY)
            .expect("selection lives in this area → request emitted");
        let mut ui_state = crate::state::UiState::new();
        ui_state.push_scroll_requests(vec![req]);

        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 240.0, 80.0));

        // Find the inner scroll's computed_id (it's the sole child
        // of the outer text_area) and read its offset.
        let scroll_id = &root.children[0].computed_id;
        let offset = ui_state.scroll_offset(scroll_id);
        assert!(
            offset > 0.0,
            "EnsureVisible should have shifted the scroll past 0 to expose the caret; got {offset}"
        );
        let metrics = ui_state
            .scroll
            .metrics
            .get(scroll_id)
            .expect("metrics written for scroll");
        // The offset must keep the caret line within the viewport —
        // it should sit between `offset` and `offset + viewport_h`.
        let line_h = line_height_px();
        let caret_y = text_area_geometry(&value, None).caret_xy(caret_byte).1;
        assert!(
            caret_y >= offset && caret_y + line_h <= offset + metrics.viewport_h + 0.5,
            "caret y={caret_y} not inside viewport [{offset}, {}]; line_h={line_h}",
            offset + metrics.viewport_h
        );
    }

    #[test]
    fn caret_scroll_request_returns_none_when_selection_lives_elsewhere() {
        // When the global selection points at another widget's key,
        // we mustn't generate a scroll-into-view for this area —
        // typing in widget A shouldn't pull widget B's scroll back to
        // its caret.
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("other", 0),
                head: SelectionPoint::new("other", 0),
            }),
        };
        assert!(caret_scroll_request_for("hello\nworld", &sel, TEST_KEY).is_none());
    }

    #[test]
    fn ensure_visible_skips_when_caret_already_inside_viewport() {
        // The resolver must be idempotent when the caret already
        // lives in the visible region — otherwise pushing the
        // request every frame would snap the scroll back to the
        // caret after a manual wheel.
        let value = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
        // Caret on the first line (always in view at offset 0).
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new(TEST_KEY, 0),
                head: SelectionPoint::new(TEST_KEY, 0),
            }),
        };
        let mut root = super::text_area(&value, &sel, TEST_KEY)
            .height(Size::Fixed(80.0))
            .width(Size::Fixed(240.0));
        let mut ui_state = crate::state::UiState::new();
        // Pre-set an offset that pushes the caret offscreen — a
        // wheel-scroll would leave the system in this state.
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 240.0, 80.0));
        let scroll_id = root.children[0].computed_id.clone();
        ui_state.scroll.offsets.insert(scroll_id.clone(), 300.0);
        // Re-emit the request. Caret is at y=0, viewport is [300,
        // 380]. The resolver must scroll back to expose y=0 —
        // that's the "above viewport" branch, which DOES override.
        let req = caret_scroll_request_for(&value, &sel, TEST_KEY).unwrap();
        ui_state.push_scroll_requests(vec![req]);
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 240.0, 80.0));
        let after = ui_state.scroll_offset(&scroll_id);
        assert!(
            after <= 1.0,
            "caret above viewport → scroll snaps up to expose it; got {after}"
        );

        // Now caret IS in the visible region after layout; emit the
        // request again and confirm offset is unchanged (idempotent).
        let req2 = caret_scroll_request_for(&value, &sel, TEST_KEY).unwrap();
        ui_state.push_scroll_requests(vec![req2]);
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 240.0, 80.0));
        let after2 = ui_state.scroll_offset(&scroll_id);
        assert_eq!(
            after, after2,
            "caret already visible → resolver must leave the offset alone"
        );
    }

    #[test]
    fn pointer_down_after_scroll_lands_on_the_visible_line_not_content_origin() {
        // Regression: stage 1 wrapped contents in a scroll viewport
        // but `apply_event`'s pointer→caret math kept using the
        // outer rect's y, which is unchanged by the inner scroll.
        // So once a user scrolled down and clicked, the caret would
        // land on the line that was visible at that y *before*
        // scrolling — i.e., a content-y of `local_y` instead of
        // `local_y + offset`. The fix is `UiTarget.scroll_offset_y`
        // (populated by hit-test). Verify a click after a scroll
        // lands on the right line.
        let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
        let value = lines.join("\n");
        // Build a text_area with a small fixed height; layout the
        // full pipeline so the inner scroll has metrics.
        let mut root = super::text_area(&value, &Selection::default(), TEST_KEY)
            .height(Size::Fixed(60.0))
            .width(Size::Fixed(200.0));
        let mut ui_state = crate::state::UiState::new();
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 200.0, 60.0));
        let scroll_id = root.children[0].computed_id.clone();
        // Wheel-scroll down by 5 lines' worth.
        let offset = line_height_px() * 5.0;
        ui_state.scroll.offsets.insert(scroll_id.clone(), offset);
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 200.0, 60.0));

        // Synthesize a PointerDown at viewport-y mid-first-visible
        // line. hit_test populates `scroll_offset_y` from the
        // descendant scroll's stored offset.
        let target = crate::hit_test::hit_test_target(
            &root,
            &ui_state,
            (
                tokens::SPACE_3 + 5.0,
                tokens::SPACE_2 + line_height_px() * 0.5,
            ),
        )
        .expect("click inside text_area should hit it");
        assert!(
            (target.scroll_offset_y - offset).abs() < 0.5,
            "UiTarget.scroll_offset_y={} should reflect the inner scroll's {}",
            target.scroll_offset_y,
            offset
        );

        // Now drive a PointerDown through apply_event and assert
        // the caret landed near the byte at line 5 (the first
        // visible line after scrolling), not line 0.
        let ev = UiEvent {
            path: None,
            key: Some(target.key.clone()),
            pointer: Some((
                target.rect.x + tokens::SPACE_3 + 5.0,
                target.rect.y + tokens::SPACE_2 + line_height_px() * 0.5,
            )),
            target: Some(target),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::PointerDown,
        };
        let mut value_mut = value.clone();
        let mut sel = Selection::default();
        super::apply_event(&mut value_mut, &mut sel, TEST_KEY, &ev);
        let view = sel.within(TEST_KEY).expect("apply_event sets selection");
        // line 5 begins after 5 newlines (each "line N" is 6 or 7
        // bytes plus '\n'); use the actual offset from value.
        let line5_start = lines[..5].iter().map(|s| s.len() + 1).sum::<usize>();
        let line5_end = line5_start + lines[5].len();
        assert!(
            view.head >= line5_start && view.head <= line5_end,
            "PointerDown after a 5-line scroll should land on line 5 \
             (bytes [{line5_start}..{line5_end}]); got head={}",
            view.head
        );
    }

    #[test]
    fn drag_past_bottom_edge_emits_autoscroll_request() {
        // Drag-select auto-scroll: when the user drags below the
        // bottom of the visible scroll viewport, the helper
        // returns a request that scrolls the inner scroll to expose
        // the next line down. The app pushes this into
        // `drain_scroll_requests` to advance the offset.
        let target = ta_target();
        let ev = UiEvent {
            path: None,
            key: Some(target.key.clone()),
            pointer: Some((
                target.rect.x + tokens::SPACE_3 + 10.0,
                target.rect.bottom() + 30.0, // 30px below the bottom
            )),
            target: Some(target),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::Drag,
        };
        let req = drag_autoscroll_request_for(&ev, TEST_KEY)
            .expect("drag past bottom edge should produce a scroll request");
        match req {
            crate::scroll::ScrollRequest::EnsureVisible {
                container_key, y, ..
            } => {
                assert_eq!(container_key, TEST_KEY);
                // y should be past the current viewport bottom in
                // content coords. With offset=0 and a 100×100 outer
                // (less 2*SPACE_2 padding), the viewport_h is
                // ~100-2*SPACE_2; the request asks for a y at that
                // bottom edge.
                assert!(y > 0.0);
            }
            other => panic!("expected EnsureVisible, got {other:?}"),
        }
    }

    #[test]
    fn drag_inside_viewport_emits_no_autoscroll_request() {
        // No autoscroll when the pointer is still inside the
        // visible viewport — only continuous-drag past an edge
        // should trigger it.
        let target = ta_target();
        let ev = UiEvent {
            path: None,
            key: Some(target.key.clone()),
            pointer: Some((
                target.rect.x + tokens::SPACE_3 + 10.0,
                target.rect.y + tokens::SPACE_2 + 20.0,
            )),
            target: Some(target),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::Drag,
        };
        assert!(drag_autoscroll_request_for(&ev, TEST_KEY).is_none());
    }

    #[test]
    fn drag_past_top_edge_emits_autoscroll_request_up() {
        // Symmetric: pointer above the top of the visible viewport
        // returns a request that exposes the line just above.
        let mut target = ta_target();
        target.scroll_offset_y = 200.0; // currently scrolled down
        let ev = UiEvent {
            path: None,
            key: Some(target.key.clone()),
            pointer: Some((target.rect.x + tokens::SPACE_3 + 10.0, target.rect.y - 20.0)),
            target: Some(target),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::Drag,
        };
        let req = drag_autoscroll_request_for(&ev, TEST_KEY)
            .expect("drag past top edge should produce a scroll request");
        match req {
            crate::scroll::ScrollRequest::EnsureVisible { y, .. } => {
                assert!(
                    y < 200.0,
                    "should ask to expose a y above the current 200 offset"
                );
            }
            other => panic!("expected EnsureVisible, got {other:?}"),
        }
    }

    #[test]
    fn drag_autoscroll_returns_none_for_non_drag_events() {
        let target = ta_target();
        let ev = UiEvent {
            path: None,
            key: Some(target.key.clone()),
            pointer: Some((target.rect.x, target.rect.bottom() + 50.0)),
            target: Some(target),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
            pointer_kind: None,
            wheel_delta: None,
            kind: UiEventKind::PointerDown,
        };
        assert!(drag_autoscroll_request_for(&ev, TEST_KEY).is_none());
    }

    #[test]
    fn pointer_hit_routes_to_outer_text_area_not_inner_scroll() {
        // Critical invariant: keys make a node a hit-test target, so
        // if the inner scroll accidentally gained a key, pointer
        // events would route to the scroll instead of the outer
        // text_area — breaking `apply_event` (which checks
        // `e.target_key() == Some("ta")`). Lock in that clicks land
        // on the outer key.
        let mut root = super::text_area("body\nwith\nlines", &Selection::default(), TEST_KEY)
            .height(Size::Fixed(60.0))
            .width(Size::Fixed(200.0));
        let mut ui_state = crate::state::UiState::new();
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 200.0, 60.0));
        // Click roughly in the middle of the field — well inside the
        // surface and inside the scroll viewport.
        let hit = crate::hit_test::hit_test(&root, &ui_state, (100.0, 30.0));
        assert_eq!(hit.as_deref(), Some(TEST_KEY));
    }

    #[test]
    fn fixed_height_with_overflow_clips_glyph_run_to_inner_scroll_viewport() {
        // Regression: before stage 1, the text leaf's cosmic-text
        // layout would paint all wrapped lines outside the
        // text_area's surface rect because the outer container
        // didn't clip and there was no scroll viewport in between.
        // Now the inner `scroll(...)` sets `own_scissor` on its
        // descendants — the glyph run's scissor must lie inside the
        // text_area's content rect.
        //
        // We use a long single-line string (no `\n`) so cosmic-text
        // soft-wraps it into many visual rows that obviously exceed
        // the 48 px viewport. Then we read the GlyphRun's scissor
        // from the laid-out draw ops and assert it matches the
        // scroll viewport, not the text leaf's intrinsic full
        // height.
        let long = "lorem ipsum dolor sit amet ".repeat(40);
        let mut root = super::text_area(&long, &Selection::default(), TEST_KEY)
            .height(Size::Fixed(48.0))
            .width(Size::Fixed(200.0));
        let mut ui_state = crate::state::UiState::new();
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 200.0, 48.0));

        let ops = crate::draw_ops::draw_ops(&root, &ui_state);
        let glyph_scissor = ops
            .iter()
            .find_map(|op| {
                if let crate::DrawOp::GlyphRun { scissor, .. } = op {
                    *scissor
                } else {
                    None
                }
            })
            .expect("text_area should emit a GlyphRun for the value");

        // The glyph scissor must be no taller than the text_area's
        // fixed height — if it were the inner content's intrinsic
        // height, it would exceed 48 (many wrapped lines).
        assert!(
            glyph_scissor.h <= 48.0 + 0.5,
            "glyph scissor h={} should be clipped to the outer 48 px content area",
            glyph_scissor.h
        );
    }

    #[test]
    fn selection_band_for_soft_wrapped_text_uses_wrapped_line_y() {
        let value = "alpha beta gamma";
        let start = value.find("gamma").unwrap();
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new(TEST_KEY, start),
                head: SelectionPoint::new(TEST_KEY, value.len()),
            }),
        };
        let mut root = super::text_area(value, &sel, TEST_KEY)
            .height(Size::Fixed(90.0))
            .width(Size::Fixed(80.0));
        let mut ui_state = crate::state::UiState::new();
        ui_state.current_selection = sel;
        crate::layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 80.0, 90.0));

        let ops = crate::draw_ops::draw_ops(&root, &ui_state);
        let band_y = ops
            .iter()
            .find_map(|op| {
                if let crate::DrawOp::Quad { id, rect, .. } = op
                    && id.contains("selection-band")
                {
                    Some(rect.y)
                } else {
                    None
                }
            })
            .expect("wrapped text selection should emit a selection band");
        assert!(
            band_y > tokens::SPACE_2 + 1.0,
            "selection band should be below the first visual line, got y={band_y}"
        );
    }

    #[test]
    fn renders_as_focusable_capture_keys_surface_wrapping_scroll() {
        let el = text_area("foo\nbar", TextSelection::caret(0));
        assert!(matches!(el.kind, Kind::Custom("text_area")));
        assert!(el.focusable);
        assert!(el.capture_keys);
        // Outer is now a column with a single scroll viewport child;
        // the overlay axis lives one level deeper on the content
        // wrapper that hosts the selection bands + text + caret.
        assert!(matches!(el.axis, Axis::Column));
        assert_eq!(el.children.len(), 1, "outer wraps the scroll viewport");
        let scroll = &el.children[0];
        assert!(matches!(scroll.kind, Kind::Scroll));
        assert!(scroll.scrollable);
        // Inner scroll must be unkeyed so the outer text_area
        // remains the hit-test target for pointer events; offset
        // persistence comes from the computed_id, which is stable
        // as long as the scroll stays the sole child of text_area.
        assert!(scroll.key.is_none());
        let content = scroll
            .children
            .first()
            .expect("scroll has the overlay content child");
        assert!(matches!(content.axis, Axis::Overlay));
    }
}
