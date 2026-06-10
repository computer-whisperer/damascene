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

impl Cursor {
    /// The CSS `cursor` property value for this cursor (`"default"`,
    /// `"pointer"`, `"ew-resize"`, …).
    ///
    /// This is the backend-neutral bridge for custom hosts:
    /// - Web hosts assign it to `canvas.style.cursor` directly.
    /// - winit hosts parse it into a `CursorIcon` without needing any
    ///   damascene host crate (winit's `cursor-icon` types implement
    ///   `FromStr` over exactly these CSS names):
    ///   `cursor.css_name().parse::<CursorIcon>().unwrap_or_default()`.
    pub const fn css_name(self) -> &'static str {
        match self {
            Cursor::Default => "default",
            Cursor::Pointer => "pointer",
            Cursor::Text => "text",
            Cursor::NotAllowed => "not-allowed",
            Cursor::Grab => "grab",
            Cursor::Grabbing => "grabbing",
            Cursor::Move => "move",
            Cursor::EwResize => "ew-resize",
            Cursor::NsResize => "ns-resize",
            Cursor::NwseResize => "nwse-resize",
            Cursor::NeswResize => "nesw-resize",
            Cursor::ColResize => "col-resize",
            Cursor::RowResize => "row-resize",
            Cursor::Crosshair => "crosshair",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_default_variant() {
        assert_eq!(Cursor::default(), Cursor::Default);
    }

    #[test]
    fn css_names_match_the_css_cursor_property() {
        // Spot-check the kebab-case conversions; the simple one-word
        // names are self-evidently CSS values.
        assert_eq!(Cursor::NotAllowed.css_name(), "not-allowed");
        assert_eq!(Cursor::EwResize.css_name(), "ew-resize");
        assert_eq!(Cursor::NwseResize.css_name(), "nwse-resize");
        assert_eq!(Cursor::ColResize.css_name(), "col-resize");
    }
}
