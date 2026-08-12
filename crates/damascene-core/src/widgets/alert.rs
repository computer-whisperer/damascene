//! Alert — bordered callout surface with shadcn-style anatomy.
//!
//! The common shape is:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! alert([
//!     alert_title("Heads up"),
//!     alert_description("This action changes project defaults."),
//! ])
//! ```
//!
//! Apply the existing status modifiers to the root for variants:
//! `.destructive()`, `.warning()`, `.info()`, `.success()`, or `.muted()`.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::a11y::Role;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// Bordered callout surface (shadcn's `Alert`) — a full-width column
/// for [`alert_title`] and [`alert_description`]. Chain a status
/// modifier (`.destructive()`, `.warning()`, …) on the root for
/// tinted variants.
#[track_caller]
pub fn alert<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("alert"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Panel)
        .surface_role(SurfaceRole::Panel)
        .role(Role::Alert)
        .children(children)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .default_padding(Sides::all(tokens::SPACE_3))
        .default_gap(tokens::SPACE_1)
}

/// Single-line alert heading (shadcn's `AlertTitle`) — semibold,
/// ellipsized.
#[track_caller]
pub fn alert_title(title: impl Into<String>) -> El {
    text(title)
        .at_loc(Location::caller())
        .label()
        .font_weight(FontWeight::Semibold)
        .ellipsis()
        .width(Size::Fill(1.0))
}

/// Muted, wrapping body text under the title (shadcn's
/// `AlertDescription`).
#[track_caller]
pub fn alert_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .fill_width()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_uses_panel_surface_and_density_role() {
        let a = alert([alert_title("Heads up"), alert_description("Details")]);

        assert_eq!(a.kind, Kind::Custom("alert"));
        assert_eq!(a.style_profile, StyleProfile::Surface);
        assert_eq!(a.metrics_role, Some(MetricsRole::Panel));
        assert_eq!(a.surface_role, SurfaceRole::Panel);
        assert_eq!(a.axis, Axis::Column);
        assert_eq!(a.width, Size::Fill(1.0));
        assert_eq!(a.fill, Some(tokens::CARD));
        assert_eq!(a.stroke, Some(tokens::BORDER));
        assert_eq!(a.children.len(), 2);
    }

    #[test]
    fn alert_anatomy_matches_text_roles() {
        let title = alert_title("Danger");
        let description = alert_description("This cannot be undone.");

        assert_eq!(title.text.as_deref(), Some("Danger"));
        assert_eq!(title.text_role, TextRole::Label);
        assert_eq!(title.font_weight, FontWeight::Semibold);
        assert_eq!(description.text_role, TextRole::Body);
        assert_eq!(description.text_color, Some(tokens::MUTED_FOREGROUND));
        assert_eq!(description.text_wrap, TextWrap::Wrap);
    }

    #[test]
    fn status_modifiers_tint_direct_alert_text() {
        let a = alert([alert_title("Invalid"), alert_description("Fix this")]).destructive();

        assert_eq!(a.fill, Some(tokens::DESTRUCTIVE.with_alpha_u8(38)));
        assert_eq!(a.stroke, Some(tokens::DESTRUCTIVE.with_alpha_u8(120)));
        assert_eq!(a.children[0].text_color, Some(tokens::DESTRUCTIVE));
        assert_eq!(a.children[1].text_color, Some(tokens::DESTRUCTIVE));
    }
}
