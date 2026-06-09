//! Blockquote — left-rule + indent surface for quoted prose.
//!
//! Mirrors HTML's `<blockquote>` and shadcn's typography blockquote
//! shape: a vertical accent rule on the left, content indented to the
//! right of it. Children compose freely — `paragraph(...)`, `text_runs([...])`,
//! a nested `bullet_list([...])`, even another `blockquote([...])`.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! blockquote([
//!     paragraph("After all, everyone enjoys a good joke."),
//!     paragraph("So it follows that everyone is a joker.").muted(),
//! ])
//! ```
//!
//! Author chrome (italic emphasis, muted color, etc.) is applied at the
//! call site rather than baked in — the widget is purely structural.
//!
//! Implemented as an overlay (`stack`) of the rule and a content column
//! rather than as a `row([rule, column(...)])`. The overlay axis
//! propagates the available width into intrinsic measurement, so a
//! wrapped paragraph inside the blockquote sizes correctly without the
//! row-axis chicken-and-egg between cross-axis hug and child wrap-width.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::tokens;
use crate::tree::*;

/// Width of the accent rule on the left edge. Matches shadcn's
/// `border-l-2` (= 2 px).
const RULE_WIDTH: f32 = 2.0;

/// Total indent applied to the content column: rule width plus the
/// shadcn `pl-6` typography offset, so glyphs sit clear of the rule.
const CONTENT_INDENT: f32 = RULE_WIDTH + tokens::SPACE_4;

/// Quoted-prose block (HTML `<blockquote>`) — a 2px accent rule on the
/// left with the children indented beside it in a column. Purely
/// structural; apply italics/muting at the call site.
#[track_caller]
pub fn blockquote<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let loc = Location::caller();
    let rule = El::new(Kind::Divider)
        .at_loc(loc)
        .width(Size::Fixed(RULE_WIDTH))
        .height(Size::Fill(1.0))
        .fill(tokens::BORDER);
    let body = column(children)
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(Sides {
            left: CONTENT_INDENT,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        })
        .default_gap(tokens::SPACE_3);

    stack([rule, body])
        .at_loc(loc)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::text::paragraph;

    #[test]
    fn blockquote_overlays_rule_and_indented_body_column() {
        let q = blockquote([paragraph("Quoted prose")]);

        assert_eq!(q.kind, Kind::Group);
        assert_eq!(q.axis, Axis::Overlay);
        assert_eq!(q.width, Size::Fill(1.0));
        assert_eq!(q.children.len(), 2);

        let rule = &q.children[0];
        assert_eq!(rule.kind, Kind::Divider);
        assert_eq!(rule.width, Size::Fixed(RULE_WIDTH));
        assert_eq!(rule.height, Size::Fill(1.0));
        assert_eq!(rule.fill, Some(tokens::BORDER));

        let body = &q.children[1];
        assert_eq!(body.kind, Kind::Group);
        assert_eq!(body.axis, Axis::Column);
        assert_eq!(body.width, Size::Fill(1.0));
        assert_eq!(body.padding.left, CONTENT_INDENT);
        assert_eq!(body.padding.top, 0.0);
        assert_eq!(body.padding.right, 0.0);
        assert_eq!(body.padding.bottom, 0.0);
        assert_eq!(body.children.len(), 1);
    }

    #[test]
    fn blockquote_padding_overrideable_at_call_site() {
        let q = blockquote([paragraph("x")]).pl(0.0);
        // Override applies to the outer stack's padding, not the inner
        // column's — same convention as other stock widgets.
        assert_eq!(q.padding.left, 0.0);
        assert!(q.explicit_padding);
    }
}
