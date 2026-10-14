//! Code block — surface-tinted monospace block for fenced code.
//!
//! Mirrors HTML's `<pre><code>` shape: a bordered, rounded surface that
//! wraps a multi-line monospace body. The body uses the `mono` font path
//! at `TEXT_SM` size and does not wrap — long lines extend horizontally
//! rather than reflowing, the same convention shadcn's typography uses
//! for fenced code (`overflow-x-auto`). Author wraps the result in
//! `scroll([...])` for horizontal scroll if desired; that integration is
//! out of scope for this primitive.
//!
//! Two entry points: [`code_block`] takes a verbatim string and renders
//! it as one mono text leaf, while [`code_block_chrome`] takes any
//! pre-built body `El` (typically a styled [`text_runs`] paragraph
//! produced by a syntax highlighter) and applies the same surface
//! chrome — used by `damascene-markdown` to render highlighted fenced code
//! without duplicating the chrome tokens.
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! code_block("fn main() {\n    println!(\"hi\");\n}")
//! ```

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// Fenced-code surface (HTML `<pre><code>`) — the verbatim string as a
/// non-wrapping mono leaf inside the standard code-block chrome. Long
/// lines extend horizontally; wrap in `scroll([...])` if needed.
#[track_caller]
pub fn code_block(s: impl Into<String>) -> El {
    let loc = Location::caller();
    // No `.font_size(...)`: `TEXT_SM` is already the El default, and
    // the raw setter would claim author intent — pinning code blocks
    // against `Theme::with_type_scale` when they should track it.
    let body = text(s)
        .at_loc(loc)
        .mono()
        .nowrap_text()
        .width(Size::Hug)
        .height(Size::Hug);
    code_block_chrome_at(body, loc)
}

/// Wrap an author-supplied body `El` in the standard code-block surface
/// chrome (sunken muted fill, themed border, rounded corners,
/// `SPACE_3` padding). Used by syntax-highlighter integrations that
/// need the same chrome around a styled `text_runs([...])` paragraph
/// rather than a plain mono text leaf.
#[track_caller]
pub fn code_block_chrome(body: El) -> El {
    code_block_chrome_at(body, Location::caller())
}

fn code_block_chrome_at(body: El, loc: &'static Location<'static>) -> El {
    column([body])
        .at_loc(loc)
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Sunken)
        // Trough fill + stroke come from the Input/Sunken surface role
        // as *defaults* (theme::apply_role_material), so an authored
        // .fill()/.stroke() on the returned El wins.
        .default_radius(tokens::RADIUS_MD)
        .default_padding(Sides::all(tokens::SPACE_3))
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_block_wraps_mono_body_in_sunken_surface() {
        let block = code_block("fn main() {\n    println!(\"hi\");\n}");

        assert_eq!(block.kind, Kind::Group);
        assert_eq!(block.axis, Axis::Column);
        assert_eq!(block.style_profile, StyleProfile::Surface);
        assert_eq!(block.surface_role, SurfaceRole::Sunken);
        // Trough paint comes from the Sunken role at paint time as a
        // default, not from the recipe — an authored fill/stroke wins.
        assert_eq!(block.fill, None);
        assert_eq!(block.stroke, None);
        assert_eq!(block.padding, Sides::all(tokens::SPACE_3));
        assert_eq!(block.width, Size::Fill(1.0));
        assert_eq!(block.children.len(), 1);

        let body = &block.children[0];
        assert_eq!(body.kind, Kind::Text);
        assert!(body.font_mono);
        assert_eq!(body.font_size, tokens::TEXT_SM.size);
        assert_eq!(body.text_wrap, TextWrap::NoWrap);
        assert_eq!(
            body.text.as_deref(),
            Some("fn main() {\n    println!(\"hi\");\n}")
        );
    }
}
