//! The default series colour cycle — the colours auto-assigned to marks
//! that don't set their own (`line(&h)` with no `.color(...)`).
//!
//! A small, categorical, reasonably colour-blind-aware set in the spirit of
//! the well-worn Tableau-10 / Observable-Plot defaults. Marks index into it
//! by their order in the [`PlotSpec`](crate::plot::PlotSpec); an app that
//! wants exact control sets `.color(...)` per mark.

#![warn(missing_docs)]

use crate::color::Color;

/// The categorical series colours, cycled by mark index.
const SERIES: [Color; 8] = [
    Color::srgb_u8(99, 164, 255),  // blue
    Color::srgb_u8(255, 145, 77),  // orange
    Color::srgb_u8(95, 200, 130),  // green
    Color::srgb_u8(232, 106, 138), // pink/red
    Color::srgb_u8(178, 142, 255), // purple
    Color::srgb_u8(120, 200, 210), // teal
    Color::srgb_u8(214, 196, 96),  // yellow
    Color::srgb_u8(168, 168, 178), // grey
];

/// The default series colour for the mark at index `i`, cycling the
/// `SERIES` palette.
pub fn series_color(i: usize) -> Color {
    SERIES[i % SERIES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_and_is_stable() {
        assert_eq!(series_color(0), series_color(8));
        assert_ne!(series_color(0), series_color(1));
    }
}
