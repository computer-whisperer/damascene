//! Overlay and modal primitives.
//!
//! These are ordinary [`El`] trees, not a hidden retained overlay stack.
//! That keeps the agent loop simple: the scrim, panel, buttons, source
//! locations, draw ops, and hit-test keys all appear in the same artifacts
//! as the rest of the UI.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use super::text::h3;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// A full-size overlay layer. Children share the overlay rect and are
/// centered by default; put a full-size scrim first and the floating
/// surface after it.
#[track_caller]
pub fn overlay<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Overlay)
        .at_loc(Location::caller())
        .children(children)
        .fill_size()
        .align(Align::Center)
        .justify(Justify::Center)
        .axis(Axis::Overlay)
        .clip()
}

/// Compose a main view with optional floating layers, filtering out
/// `None`s. A thin sugar wrapper over [`crate::stack`] for the
/// recurring "main + maybe-modal + maybe-popover" pattern at the El
/// root:
///
/// ```ignore
/// overlays(main_view, [
///     self.modal_open.then(|| modal("confirm", "Delete?", [...])),
///     self.menu_open.then(|| dropdown("menu", "trigger", [...])),
/// ])
/// ```
///
/// Equivalent to building a `Vec<El>` by hand and pushing only when
/// each `Option` is `Some`. Layers paint in the order given (last on
/// top); hit-testing visits them in reverse.
///
/// Each `Size::Hug` axis on `main` is promoted to `Size::Fill(1.0)`
/// so the main view spans the full viewport rect — overlay-axis
/// layout sizes a `Hug` child to its intrinsic, which would silently
/// collapse a `virtual_list` or any other content whose intrinsic is
/// `(0, 0)` to zero. Axes the author set to `Fixed` or `Fill` are
/// left untouched. Apps that want a fully `Hug`-sized main view (e.g.
/// a centred card with overlays around it) should compose `stack`
/// directly instead.
#[track_caller]
pub fn overlays<I>(main: impl Into<El>, layers: I) -> El
where
    I: IntoIterator<Item = Option<El>>,
{
    let mut main = main.into();
    if matches!(main.width, Size::Hug) {
        main.width = Size::Fill(1.0);
    }
    if matches!(main.height, Size::Hug) {
        main.height = Size::Fill(1.0);
    }
    let mut children: Vec<El> = Vec::new();
    children.push(main);
    children.extend(layers.into_iter().flatten());
    crate::stack(children)
}

/// A full-size modal scrim. The key should route to dismiss behavior in
/// the app's event handler.
#[track_caller]
pub fn scrim(key: impl Into<String>) -> El {
    El::new(Kind::Scrim)
        .at_loc(Location::caller())
        .key(key)
        .fill(tokens::OVERLAY_SCRIM)
        .enter_fade()
        .fill_size()
}

/// A centered modal with a keyed dismiss scrim.
///
/// Keys:
/// - `{key}:dismiss` — emitted when the user clicks outside the panel.
/// - Child controls keep their own keys, e.g. `button("Delete").key("confirm")`.
#[track_caller]
pub fn modal<I, E>(key: impl Into<String>, title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let key = key.into();
    overlay([
        scrim(format!("{key}:dismiss")),
        modal_panel(title, body).block_pointer(),
    ])
}

/// The floating surface of a [`modal`] without the scrim/overlay wrapper:
/// a fixed-width (420px) popover-styled column with an `h3` title above
/// the body. Use directly when composing a custom overlay arrangement.
#[track_caller]
pub fn modal_panel<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = vec![h3(title)];
    children.extend(body.into_iter().map(Into::into));

    El::new(Kind::Modal)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Popover)
        .children(children)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_LG)
        .enter_transition(crate::anim::EnterTransition::fade().with_slide(0.0, 8.0))
        .shadow(tokens::SHADOW_LG)
        .default_padding(tokens::SPACE_4)
        .default_gap(tokens::SPACE_3)
        .width(Size::Fixed(420.0))
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .clip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::button;

    #[test]
    fn block_pointer_panel_owns_inflated_child_clicks_over_scrim_sibling() {
        // Regression for #33: clicks in the gap between sidebar buttons
        // inside a modal panel must NOT route to the lower-z dismiss
        // scrim. The buttons claim the gap via hit_overflow +
        // min_touch_inflation (distance_sq > 0); the full-screen scrim
        // also contains the point (distance_sq = 0). Without
        // `block_pointer` promoting the panel's hit to dsq=0, the
        // closer scrim beats the inflated button and the modal
        // dismisses on an interior click.
        use crate::Align;
        use crate::Sides;
        use crate::hit_test;
        use crate::layout::layout;
        use crate::state::UiState;
        use crate::tree::{column, divider, row, scroll, stack};
        use crate::widgets::sidebar::{sidebar, sidebar_menu_button};

        let nav = sidebar([
            sidebar_menu_button("Connection", true).key("settings:tabs:tab:connection"),
            sidebar_menu_button("Devices", false).key("settings:tabs:tab:devices"),
            sidebar_menu_button("Voice", false).key("settings:tabs:tab:voice"),
        ])
        .width(Size::Fixed(196.0))
        .height(Size::Fill(1.0))
        .padding(Sides::all(crate::tokens::SPACE_2))
        .gap(crate::tokens::SPACE_1);

        let body_scroll = scroll([column([crate::widgets::text::text("Body")])
            .padding(Sides::xy(12.0, 0.0))
            .width(Size::Fill(1.0))])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));

        let panel = modal_panel(
            "Settings",
            [
                row([nav, body_scroll])
                    .gap(12.0)
                    .width(Size::Fill(1.0))
                    .height(Size::Fill(1.0))
                    .align(Align::Stretch),
                divider(),
                row([
                    button("Close").key("settings:close"),
                    crate::spacer(),
                    button("Save").primary().key("settings:save"),
                ])
                .gap(8.0)
                .width(Size::Fill(1.0))
                .align(Align::Center),
            ],
        )
        .width(Size::Fixed(960.0))
        .height(Size::Fixed(620.0));

        let panel_layer = overlay([scrim("settings:dismiss"), panel.block_pointer()]);
        let mut tree = stack([panel_layer]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 1280.0, 800.0));

        let conn_rect = state
            .rect_of_key("settings:tabs:tab:connection")
            .expect("connection button laid out");
        // Click 2px below the Connection button — squarely in the
        // SPACE_1 gap (4px tall) between Connection and Devices.
        let gap_click = (conn_rect.center_x(), conn_rect.bottom() + 2.0);
        let hit = hit_test::hit_test(&tree, &state, gap_click);
        assert!(
            hit.as_deref() != Some("settings:dismiss"),
            "click at {gap_click:?} in sidebar gap must NOT route to settings:dismiss; \
             got {hit:?} — block_pointer on the modal panel should own this click",
        );

        // Sanity: clicks in the actual scrim region (well outside the
        // panel) still dismiss.
        let scrim_pt = (40.0, 40.0);
        assert_eq!(
            hit_test::hit_test(&tree, &state, scrim_pt).as_deref(),
            Some("settings:dismiss"),
            "click in the dim region outside the panel must still dismiss",
        );
    }

    #[test]
    fn overlays_filters_none_layers_in_order() {
        let main = button("main").key("main");
        let one = button("one").key("one");
        let two = button("two").key("two");
        let stacked = overlays(main, [None, Some(one), None, Some(two)]);
        let keys: Vec<_> = stacked
            .children
            .iter()
            .map(|c| c.key.clone().unwrap_or_default())
            .collect();
        assert_eq!(keys, vec!["main", "one", "two"]);
    }

    #[test]
    fn overlays_with_no_layers_is_just_main_in_a_stack() {
        let stacked = overlays(button("main").key("main"), std::iter::empty::<Option<El>>());
        assert_eq!(stacked.children.len(), 1);
        assert_eq!(stacked.children[0].key.as_deref(), Some("main"));
    }

    #[test]
    fn overlays_promotes_hug_main_to_fill() {
        // Author passes a Hug-sized main column; overlays() must
        // promote both axes to Fill so overlay-axis layout doesn't
        // collapse Fill descendants (e.g. virtual_list, scroll
        // viewports) to zero.
        let main = crate::column(std::iter::empty::<El>());
        assert!(matches!(main.width, Size::Hug));
        assert!(matches!(main.height, Size::Hug));
        let stacked = overlays(main, std::iter::empty::<Option<El>>());
        let promoted = &stacked.children[0];
        assert!(matches!(promoted.width, Size::Fill(_)));
        assert!(matches!(promoted.height, Size::Fill(_)));
    }

    #[test]
    fn overlays_preserves_explicit_main_sizes() {
        // Authors who deliberately set Fixed or Fill on either axis
        // (e.g. a Fixed-height toolbar with overlays at the root) must
        // see their choice preserved. Only Hug → Fill promotion is
        // automatic.
        let main = crate::column(std::iter::empty::<El>())
            .width(Size::Fixed(320.0))
            .height(Size::Fill(1.0));
        let stacked = overlays(main, std::iter::empty::<Option<El>>());
        let kept = &stacked.children[0];
        assert!(matches!(kept.width, Size::Fixed(v) if (v - 320.0).abs() < 0.01));
        assert!(matches!(kept.height, Size::Fill(_)));
    }
}
