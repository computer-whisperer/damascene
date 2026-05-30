//! Dialog anatomy — shadcn-shaped modal content helpers.
//!
//! `modal(...)` remains the compact convenience API. Reach for this
//! module when you want the familiar component structure:
//! `dialog(key, [dialog_header([...]), ..., dialog_footer([...])])`.

use std::panic::Location;

use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::overlay::{overlay, scrim};
use crate::widgets::text::{h3, text};

/// A blocking dialog layer with a keyed dismiss scrim.
///
/// Keys:
/// - `{key}:dismiss` — emitted when the user clicks outside the content.
#[track_caller]
pub fn dialog<I, E>(key: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let key = key.into();
    overlay([
        scrim(format!("{key}:dismiss")),
        dialog_content(body).block_pointer(),
    ])
}

/// The floating dialog surface.
#[track_caller]
pub fn dialog_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Modal)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Popover)
        .children(children)
        .fill(tokens::POPOVER)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_LG)
        .shadow(tokens::SHADOW_LG)
        .default_padding(tokens::SPACE_4)
        .default_gap(tokens::SPACE_4)
        .width(Size::Fixed(420.0))
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .clip()
}

#[track_caller]
pub fn dialog_header<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_1)
}

#[track_caller]
pub fn dialog_footer<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .justify(Justify::End)
}

#[track_caller]
pub fn dialog_title(title: impl Into<String>) -> El {
    h3(title)
        .at_loc(Location::caller())
        .line_height(tokens::TEXT_BASE.size)
}

#[track_caller]
pub fn dialog_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .fill_width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::button;

    #[test]
    fn dialog_layer_exposes_dismiss_scrim_and_blocks_panel() {
        let d = dialog("prefs", [dialog_title("Preferences")]);

        assert_eq!(d.kind, Kind::Overlay);
        assert_eq!(d.children[0].kind, Kind::Scrim);
        assert_eq!(d.children[0].key.as_deref(), Some("prefs:dismiss"));
        assert!(d.children[1].block_pointer);
    }

    #[test]
    fn dialog_content_uses_panel_metrics_and_modal_surface() {
        let content = dialog_content([dialog_header([
            dialog_title("Delete project"),
            dialog_description("This action cannot be undone."),
        ])]);

        assert_eq!(content.kind, Kind::Modal);
        assert_eq!(content.metrics_role, Some(MetricsRole::Panel));
        assert_eq!(content.surface_role, SurfaceRole::Popover);
        assert_eq!(content.width, Size::Fixed(420.0));
        assert_eq!(content.axis, Axis::Column);
        assert_eq!(content.align, Align::Stretch);
    }

    #[test]
    fn dialog_header_and_footer_match_expected_anatomy() {
        let header = dialog_header([dialog_title("Title"), dialog_description("Description")]);
        let footer = dialog_footer([button("Cancel"), button("Save").primary()]);

        assert_eq!(header.axis, Axis::Column);
        assert_eq!(header.gap, tokens::SPACE_1);
        assert_eq!(footer.axis, Axis::Row);
        assert_eq!(footer.gap, tokens::SPACE_2);
        assert_eq!(footer.justify, Justify::End);
        assert_eq!(footer.align, Align::Center);
    }
}
