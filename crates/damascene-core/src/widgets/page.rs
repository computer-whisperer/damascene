//! Page scaffold — the window-level "body" container for an app root.

use std::panic::Location;

use crate::tokens;
use crate::tree::*;

/// Root scaffold for an app screen — what `<body>` plus a screen
/// container is to an HTML page.
///
/// Window chrome needs two things that no bare `column`/`row` root
/// provides, and `page` bakes both:
///
/// - window padding ([`tokens::SPACE_4`]) on the content, so toolbars
///   and text never sit flush against window edges or get clipped by
///   rounded window corners;
/// - an overlay root ([`Axis::Overlay`]), so layer-synthesizing state
///   like `.tooltip()` has somewhere to mount (the same requirement
///   [`crate::overlays`] satisfies — wrap `page(...)` in `overlays`
///   when the app also drives modals/dropdowns).
///
/// (No background fill: the host already clears the frame to the
/// theme's [`tokens::BACKGROUND`] — an explicit root fill would paint
/// the same pixels twice.)
///
/// The minimal real app root reads:
///
/// ```
/// use damascene_core::prelude::*;
///
/// fn build() -> El {
///     page([
///         toolbar([toolbar_title("Library"), spacer(), button("Import")]),
///         text("content"),
///     ])
/// }
/// # let _ = build();
/// ```
///
/// Children flow vertically with a [`tokens::SPACE_4`] gap, stretched
/// to the content width. Apps that want different page anatomy (no
/// padding for a full-bleed canvas, a `Hug`-sized centered card)
/// should compose `stack([...])` directly — see
/// `damascene-fixtures/src/hero.rs` for an expanded workbench root.
#[track_caller]
pub fn page<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let loc = Location::caller();
    stack([
        column(children)
            .at_loc(loc)
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .padding(tokens::SPACE_4)
            .fill_size(),
    ])
    .at_loc(loc)
    .fill_size()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::toolbar::{toolbar, toolbar_title};

    #[test]
    fn page_bakes_padding_and_overlay_root() {
        let p = page([toolbar([toolbar_title("Documents")])]);

        // Overlay root: .tooltip() layers can mount without a separate
        // overlays() wrapper.
        assert_eq!(p.axis, Axis::Overlay);
        assert_eq!(p.width, Size::Fill(1.0));
        assert_eq!(p.height, Size::Fill(1.0));

        // Content layer: padded vertical flow. No background layer —
        // the host clears to the theme background.
        assert_eq!(p.children.len(), 1);
        let content = &p.children[0];
        assert_eq!(content.axis, Axis::Column);
        assert_eq!(content.padding, Sides::all(tokens::SPACE_4));
        assert_eq!(content.gap, tokens::SPACE_4);
        assert_eq!(content.align, Align::Stretch);
        assert!(content.fill.is_none());
    }

    #[test]
    fn page_root_is_lint_clean_and_tooltip_capable() {
        // The whole point of the scaffold: the minimal app root with a
        // toolbar and a tooltip passes render_bundle with no findings.
        let mut root = page([
            toolbar([toolbar_title("Library")]),
            crate::text("cell").key("cell").tooltip("a tooltip"),
        ]);
        let bundle =
            crate::bundle::artifact::render_bundle(&mut root, Rect::new(0.0, 0.0, 640.0, 480.0));
        assert!(
            bundle.lint.findings.is_empty(),
            "page() root must lint clean:\n{}",
            bundle.lint.text()
        );
    }
}
