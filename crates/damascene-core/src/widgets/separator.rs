//! Separator — shadcn-shaped divider naming over Damascene's divider primitive.
//!
//! Use `separator()` for a horizontal rule between stacked sections and
//! `vertical_separator()` between inline toolbar/sidebar items. These are
//! intentionally tiny wrappers around [`crate::divider`] so authors can
//! reach for the familiar shadcn vocabulary without learning a second
//! primitive.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::tokens;
use crate::tree::*;

/// Horizontal rule between stacked sections — a 1px full-width divider in [`tokens::BORDER`].
#[track_caller]
pub fn separator() -> El {
    crate::divider()
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Fixed(1.0))
        .fill(tokens::BORDER)
}

/// Vertical rule between inline items (toolbars, sidebars) — a 1px full-height divider in [`tokens::BORDER`].
#[track_caller]
pub fn vertical_separator() -> El {
    crate::divider()
        .at_loc(Location::caller())
        .width(Size::Fixed(1.0))
        .height(Size::Fill(1.0))
        .fill(tokens::BORDER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separator_is_horizontal_by_default() {
        let s = separator();

        assert_eq!(s.kind, Kind::Divider);
        assert_eq!(s.width, Size::Fill(1.0));
        assert_eq!(s.height, Size::Fixed(1.0));
        assert_eq!(s.fill, Some(tokens::BORDER));
    }

    #[test]
    fn vertical_separator_flips_axes() {
        let s = vertical_separator();

        assert_eq!(s.kind, Kind::Divider);
        assert_eq!(s.width, Size::Fixed(1.0));
        assert_eq!(s.height, Size::Fill(1.0));
        assert_eq!(s.fill, Some(tokens::BORDER));
    }
}
