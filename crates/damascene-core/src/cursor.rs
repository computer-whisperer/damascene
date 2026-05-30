//! Pointer-cursor model.
//!
//! Widgets opt into a non-default cursor by setting [`crate::tree::El::cursor`].
//! Per-frame the host runner reads [`crate::state::UiState::cursor`] and forwards
//! the resolved value to the windowing backend (winit's
//! `Window::set_cursor_icon`, the browser's `canvas.style.cursor`).
//!
//! The variants line up with the CSS `cursor` property — same names
//! winit's `CursorIcon` already uses — so backend bridges are dumb
//! `From` impls.
//!
//! # Resolution
//!
//! [`crate::state::UiState::cursor`] picks the active cursor each frame:
//!
//! 1. If a press is captured (button drag, slider thumb, scrollbar
//!    drag, text selection, …), the cursor follows the *press target*'s
//!    declared cursor and ignores whatever the pointer is currently
//!    over. Drag off a button onto a text input and the cursor stays
//!    [`Cursor::Pointer`] — matches native press-and-hold behaviour.
//! 2. Else the hovered node and its ancestors are walked root-ward,
//!    returning the first explicit `.cursor(...)`. So a panel that
//!    sets `.cursor(Move)` once propagates to children that don't
//!    override.
//! 3. Else [`Cursor::Default`].
//!
//! Disabled state is *not* auto-mapped to [`Cursor::NotAllowed`] —
//! widgets that want that affordance branch in their build closure.

/// Pointer cursor. Variant names mirror CSS `cursor` so the
/// backend mapping is a 1:1 translation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Cursor {
    /// Platform default arrow.
    #[default]
    Default,
    /// Hand / pointing finger — clickable surfaces (buttons, links,
    /// checkboxes, switches, radios).
    Pointer,
    /// I-beam — text inputs and selectable text regions.
    Text,
    /// Slashed circle — disabled / unavailable affordances.
    NotAllowed,
    /// Open hand — a draggable target at rest.
    Grab,
    /// Closed hand — a draggable target while dragging.
    Grabbing,
    /// Generic "drag in any direction" (pan handles, view-port grabs).
    Move,
    /// Horizontal resize (←→).
    EwResize,
    /// Vertical resize (↑↓).
    NsResize,
    /// Diagonal resize (↖↘).
    NwseResize,
    /// Anti-diagonal resize (↗↙).
    NeswResize,
    /// Column boundary resize (table column dividers).
    ColResize,
    /// Row boundary resize (table row dividers).
    RowResize,
    /// Crosshair — picker / area-select tools.
    Crosshair,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_default_variant() {
        assert_eq!(Cursor::default(), Cursor::Default);
    }
}
