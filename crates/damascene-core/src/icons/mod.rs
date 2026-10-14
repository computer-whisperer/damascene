//! Built-in vector icons.
//!
//! The vocabulary intentionally mirrors common shadcn/lucide names.
//! Icons are semantic `El`s that emit vector draw ops for artifact/SVG
//! rendering and a text fallback in GPU backends until the dedicated
//! vector-icon pipeline lands.
//!
//! The path geometry in [`icon_path`] is [Lucide](https://lucide.dev/)'s
//! own (ISC), reproduced verbatim; see `CREDITS.md`. [`icon_strokes`] is
//! that same geometry flattened to line segments.
//!
//! Sibling modules:
//! - [`svg`] — `IconSource`, `SvgIcon`, custom user-supplied icon assets.
//! - [`msdf`] — MSDF (multi-channel signed distance field) generation.
//! - [`msdf_atlas`] — atlas packing of MSDF tiles for GPU consumption.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

pub mod msdf;
pub mod msdf_atlas;
pub mod svg;

use std::panic::Location;
use std::sync::OnceLock;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::vector::{VectorAsset, parse_current_color_svg_asset};
use svg::IntoIconSource;

/// Resolve a string-typed icon name to an [`IconSource`]. A known name
/// becomes [`IconSource::Builtin`]; an unknown name is preserved as
/// [`IconSource::UnknownName`] so the bundle lint can flag it
/// ([`crate::FindingKind::UnknownIconName`]) and the painter can fall
/// back to a visible `AlertCircle` — important when an LLM-typed
/// `icon("abrows-right")` would otherwise silently misrender. A one-line
/// stderr warning is also emitted for runtime builds that don't lint.
pub(crate) fn name_to_source(name: &str) -> svg::IconSource {
    match IconName::parse(name) {
        Some(found) => svg::IconSource::Builtin(found),
        None => {
            eprintln!(
                "damascene: unknown icon name `{name}` — rendering AlertCircle. \
                 See `damascene_core::all_icon_names()` for the available vocabulary."
            );
            svg::IconSource::UnknownName(name.to_string())
        }
    }
}

/// A vector icon — accepts a built-in [`IconName`], a `&str`
/// resolved against the built-in vocabulary
/// (see [`all_icon_names`]), or an app-supplied
/// [`SvgIcon`][crate::SvgIcon] for product-specific glyphs.
///
/// ```ignore
/// use damascene_core::prelude::*;
///
/// // Built-in lucide-shaped vocabulary.
/// icon(IconName::Folder)
/// icon("settings")  // string-typed, with an AlertCircle fallback
///
/// // App-supplied SVG. Parse once (typically as a LazyLock) and
/// // pass the resulting SvgIcon — same `text_color` tinting and
/// // `icon_size` scaling as the built-ins.
/// use damascene_core::SvgIcon;
/// use std::sync::LazyLock;
/// static MY_GLYPH: LazyLock<SvgIcon> = LazyLock::new(|| {
///     SvgIcon::parse_current_color(include_str!("path/to/glyph.svg")).unwrap()
/// });
/// icon(MY_GLYPH.clone()).text_color(tokens::PRIMARY)
/// ```
///
/// Use `.icon_size(...)` to override the default 16 px, and
/// `.text_color(...)` to tint (matters for `parse_current_color`
/// SVGs and the built-in lucide-style monochrome icons; full-color
/// SVGs parsed with [`SvgIcon::parse`][crate::SvgIcon::parse] keep
/// their authored paint).
#[track_caller]
pub fn icon(source: impl IntoIconSource) -> El {
    El::new(Kind::Custom("icon"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .icon_source(source.into_icon_source())
        .icon_size(tokens::ICON_SM)
        .icon_stroke_width(2.0)
        .text_color(tokens::FOREGROUND)
}

/// One straight line segment of a built-in icon, in the 24×24
/// design-grid coordinate system (see [`icon_strokes`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconStroke {
    /// Segment start `[x, y]` in 24×24 design-grid units.
    pub from: [f32; 2],
    /// Segment end `[x, y]` in 24×24 design-grid units.
    pub to: [f32; 2],
}

const fn stroke(x0: f32, y0: f32, x1: f32, y1: f32) -> IconStroke {
    IconStroke {
        from: [x0, y0],
        to: [x1, y1],
    }
}

const ACTIVITY: &[IconStroke] = &[
    stroke(3.0, 12.0, 7.0, 12.0),
    stroke(7.0, 12.0, 10.0, 4.0),
    stroke(10.0, 4.0, 14.0, 20.0),
    stroke(14.0, 20.0, 17.0, 12.0),
    stroke(17.0, 12.0, 21.0, 12.0),
];
const ALERT_CIRCLE: &[IconStroke] = &[
    stroke(12.0, 3.0, 16.5, 4.2),
    stroke(16.5, 4.2, 19.8, 7.5),
    stroke(19.8, 7.5, 21.0, 12.0),
    stroke(21.0, 12.0, 19.8, 16.5),
    stroke(19.8, 16.5, 16.5, 19.8),
    stroke(16.5, 19.8, 12.0, 21.0),
    stroke(12.0, 21.0, 7.5, 19.8),
    stroke(7.5, 19.8, 4.2, 16.5),
    stroke(4.2, 16.5, 3.0, 12.0),
    stroke(3.0, 12.0, 4.2, 7.5),
    stroke(4.2, 7.5, 7.5, 4.2),
    stroke(7.5, 4.2, 12.0, 3.0),
    stroke(12.0, 7.0, 12.0, 13.0),
    stroke(12.0, 17.0, 12.2, 17.0),
];
const ARROW_DOWN: &[IconStroke] = &[
    stroke(12.0, 5.0, 12.0, 19.0),
    stroke(19.0, 12.0, 12.0, 19.0),
    stroke(12.0, 19.0, 5.0, 12.0),
];
const ARROW_LEFT: &[IconStroke] = &[
    stroke(12.0, 19.0, 5.0, 12.0),
    stroke(5.0, 12.0, 12.0, 5.0),
    stroke(19.0, 12.0, 5.0, 12.0),
];
const ARROW_RIGHT: &[IconStroke] = &[
    stroke(5.0, 12.0, 19.0, 12.0),
    stroke(12.0, 5.0, 19.0, 12.0),
    stroke(19.0, 12.0, 12.0, 19.0),
];
const ARROW_UP: &[IconStroke] = &[
    stroke(5.0, 12.0, 12.0, 5.0),
    stroke(12.0, 5.0, 19.0, 12.0),
    stroke(12.0, 19.0, 12.0, 5.0),
];
const BAR_CHART: &[IconStroke] = &[
    stroke(4.0, 20.0, 4.0, 10.0),
    stroke(12.0, 20.0, 12.0, 4.0),
    stroke(20.0, 20.0, 20.0, 13.0),
];
const BELL: &[IconStroke] = &[
    stroke(6.0, 17.0, 18.0, 17.0),
    stroke(6.0, 17.0, 6.0, 8.0),
    stroke(6.0, 8.0, 8.0, 5.0),
    stroke(8.0, 5.0, 12.0, 3.0),
    stroke(12.0, 3.0, 16.0, 5.0),
    stroke(16.0, 5.0, 18.0, 8.0),
    stroke(18.0, 8.0, 18.0, 17.0),
    stroke(10.0, 21.0, 14.0, 21.0),
];
const BOX: &[IconStroke] = &[
    stroke(21.0, 8.0, 20.0, 6.3),
    stroke(20.0, 6.3, 13.0, 2.3),
    stroke(13.0, 2.3, 11.0, 2.3),
    stroke(11.0, 2.3, 4.0, 6.3),
    stroke(4.0, 6.3, 3.0, 8.0),
    stroke(3.0, 8.0, 3.0, 16.0),
    stroke(3.0, 16.0, 4.0, 17.7),
    stroke(4.0, 17.7, 11.0, 21.7),
    stroke(11.0, 21.7, 13.0, 21.7),
    stroke(13.0, 21.7, 20.0, 17.7),
    stroke(20.0, 17.7, 21.0, 16.0),
    stroke(21.0, 16.0, 21.0, 8.0),
    stroke(3.3, 7.0, 12.0, 12.0),
    stroke(12.0, 12.0, 20.7, 7.0),
    stroke(12.0, 22.0, 12.0, 12.0),
];
const CAMERA: &[IconStroke] = &[
    stroke(14.0, 4.0, 15.8, 5.1),
    stroke(15.8, 5.1, 16.2, 6.0),
    stroke(16.2, 6.0, 18.0, 7.0),
    stroke(18.0, 7.0, 20.0, 7.0),
    stroke(20.0, 7.0, 22.0, 9.0),
    stroke(22.0, 9.0, 22.0, 18.0),
    stroke(22.0, 18.0, 20.0, 20.0),
    stroke(20.0, 20.0, 4.0, 20.0),
    stroke(4.0, 20.0, 2.0, 18.0),
    stroke(2.0, 18.0, 2.0, 9.0),
    stroke(2.0, 9.0, 4.0, 7.0),
    stroke(4.0, 7.0, 6.0, 7.0),
    stroke(6.0, 7.0, 7.8, 6.0),
    stroke(7.8, 6.0, 8.2, 5.0),
    stroke(8.2, 5.0, 10.0, 4.0),
    stroke(10.0, 4.0, 14.0, 4.0),
    stroke(15.0, 13.0, 12.0, 16.0),
    stroke(12.0, 16.0, 9.0, 13.0),
    stroke(9.0, 13.0, 12.0, 10.0),
    stroke(12.0, 10.0, 15.0, 13.0),
    stroke(14.8, 13.0, 15.0, 13.0),
];
const CHECK: &[IconStroke] = &[stroke(20.0, 6.0, 9.0, 17.0), stroke(9.0, 17.0, 4.0, 12.0)];
const CHEVRON_DOWN: &[IconStroke] = &[stroke(6.0, 9.0, 12.0, 15.0), stroke(12.0, 15.0, 18.0, 9.0)];
const CHEVRON_LEFT: &[IconStroke] = &[stroke(15.0, 6.0, 9.0, 12.0), stroke(9.0, 12.0, 15.0, 18.0)];
const CHEVRON_RIGHT: &[IconStroke] = &[stroke(9.0, 6.0, 15.0, 12.0), stroke(15.0, 12.0, 9.0, 18.0)];
const CHEVRON_UP: &[IconStroke] = &[stroke(6.0, 15.0, 12.0, 9.0), stroke(12.0, 9.0, 18.0, 15.0)];
const CODE: &[IconStroke] = &[
    stroke(16.0, 18.0, 22.0, 12.0),
    stroke(22.0, 12.0, 16.0, 6.0),
    stroke(8.0, 6.0, 2.0, 12.0),
    stroke(2.0, 12.0, 8.0, 18.0),
];
const COMMAND: &[IconStroke] = &[
    stroke(9.0, 9.0, 15.0, 9.0),
    stroke(15.0, 9.0, 15.0, 15.0),
    stroke(15.0, 15.0, 9.0, 15.0),
    stroke(9.0, 15.0, 9.0, 9.0),
    stroke(9.0, 9.0, 6.0, 9.0),
    stroke(6.0, 9.0, 6.0, 4.0),
    stroke(6.0, 4.0, 9.0, 4.0),
    stroke(9.0, 4.0, 9.0, 9.0),
    stroke(15.0, 9.0, 15.0, 4.0),
    stroke(15.0, 4.0, 18.0, 4.0),
    stroke(18.0, 4.0, 18.0, 9.0),
    stroke(18.0, 9.0, 15.0, 9.0),
    stroke(15.0, 15.0, 18.0, 15.0),
    stroke(18.0, 15.0, 18.0, 20.0),
    stroke(18.0, 20.0, 15.0, 20.0),
    stroke(15.0, 20.0, 15.0, 15.0),
    stroke(9.0, 15.0, 9.0, 20.0),
    stroke(9.0, 20.0, 6.0, 20.0),
    stroke(6.0, 20.0, 6.0, 15.0),
    stroke(6.0, 15.0, 9.0, 15.0),
];
const CONTRAST: &[IconStroke] = &[
    stroke(22.0, 12.0, 21.2, 15.9),
    stroke(21.2, 15.9, 19.1, 19.1),
    stroke(19.1, 19.1, 15.9, 21.2),
    stroke(15.9, 21.2, 12.0, 22.0),
    stroke(12.0, 22.0, 8.1, 21.2),
    stroke(8.1, 21.2, 4.9, 19.1),
    stroke(4.9, 19.1, 2.8, 15.9),
    stroke(2.8, 15.9, 2.0, 12.0),
    stroke(2.0, 12.0, 2.8, 8.1),
    stroke(2.8, 8.1, 4.9, 4.9),
    stroke(4.9, 4.9, 8.1, 2.8),
    stroke(8.1, 2.8, 12.0, 2.0),
    stroke(12.0, 2.0, 15.9, 2.8),
    stroke(15.9, 2.8, 19.1, 4.9),
    stroke(19.1, 4.9, 21.2, 8.1),
    stroke(21.2, 8.1, 22.0, 12.0),
    stroke(21.8, 12.0, 22.0, 12.0),
    stroke(12.0, 18.0, 15.0, 17.2),
    stroke(15.0, 17.2, 17.2, 15.0),
    stroke(17.2, 15.0, 18.0, 12.0),
    stroke(18.0, 12.0, 17.2, 9.0),
    stroke(17.2, 9.0, 15.0, 6.8),
    stroke(15.0, 6.8, 12.0, 6.0),
    stroke(12.0, 6.0, 12.0, 18.0),
    stroke(11.8, 18.0, 12.0, 18.0),
];
const DOWNLOAD: &[IconStroke] = &[
    stroke(12.0, 3.0, 12.0, 15.0),
    stroke(7.0, 10.0, 12.0, 15.0),
    stroke(12.0, 15.0, 17.0, 10.0),
    stroke(5.0, 21.0, 19.0, 21.0),
];
const EXTERNAL_LINK: &[IconStroke] = &[
    stroke(15.0, 3.0, 21.0, 3.0),
    stroke(21.0, 3.0, 21.0, 9.0),
    stroke(10.0, 14.0, 21.0, 3.0),
    stroke(18.0, 13.0, 18.0, 19.0),
    stroke(18.0, 19.0, 16.0, 21.0),
    stroke(16.0, 21.0, 5.0, 21.0),
    stroke(5.0, 21.0, 3.0, 19.0),
    stroke(3.0, 19.0, 3.0, 8.0),
    stroke(3.0, 8.0, 5.0, 6.0),
    stroke(5.0, 6.0, 11.0, 6.0),
];
const EYE: &[IconStroke] = &[
    stroke(2.1, 11.7, 4.4, 8.2),
    stroke(4.4, 8.2, 7.9, 5.8),
    stroke(7.9, 5.8, 12.0, 5.0),
    stroke(12.0, 5.0, 16.1, 5.8),
    stroke(16.1, 5.8, 19.6, 8.2),
    stroke(19.6, 8.2, 21.9, 11.7),
    stroke(21.9, 11.7, 21.9, 12.3),
    stroke(21.9, 12.3, 19.6, 15.8),
    stroke(19.6, 15.8, 16.1, 18.2),
    stroke(16.1, 18.2, 12.0, 19.0),
    stroke(12.0, 19.0, 7.9, 18.2),
    stroke(7.9, 18.2, 4.4, 15.8),
    stroke(4.4, 15.8, 2.1, 12.3),
    stroke(2.1, 12.3, 2.1, 11.7),
    stroke(12.0, 9.0, 15.0, 12.0),
    stroke(15.0, 12.0, 12.0, 15.0),
    stroke(12.0, 15.0, 9.0, 12.0),
    stroke(9.0, 12.0, 12.0, 9.0),
];
const FILE_TEXT: &[IconStroke] = &[
    stroke(6.0, 3.0, 14.0, 3.0),
    stroke(14.0, 3.0, 18.0, 7.0),
    stroke(18.0, 7.0, 18.0, 21.0),
    stroke(18.0, 21.0, 6.0, 21.0),
    stroke(6.0, 21.0, 6.0, 3.0),
    stroke(14.0, 3.0, 14.0, 7.0),
    stroke(14.0, 7.0, 18.0, 7.0),
    stroke(8.0, 12.0, 16.0, 12.0),
    stroke(8.0, 16.0, 14.0, 16.0),
];
const FLIP_HORIZONTAL: &[IconStroke] = &[
    stroke(8.0, 3.0, 5.0, 3.0),
    stroke(5.0, 3.0, 3.0, 5.0),
    stroke(3.0, 5.0, 3.0, 19.0),
    stroke(3.0, 19.0, 5.0, 21.0),
    stroke(5.0, 21.0, 8.0, 21.0),
    stroke(16.0, 3.0, 19.0, 3.0),
    stroke(19.0, 3.0, 21.0, 5.0),
    stroke(21.0, 5.0, 21.0, 19.0),
    stroke(21.0, 19.0, 19.0, 21.0),
    stroke(19.0, 21.0, 16.0, 21.0),
    stroke(12.0, 20.0, 12.0, 22.0),
    stroke(12.0, 14.0, 12.0, 16.0),
    stroke(12.0, 8.0, 12.0, 10.0),
    stroke(12.0, 2.0, 12.0, 4.0),
];
const FOLDER: &[IconStroke] = &[
    stroke(3.0, 6.0, 10.0, 6.0),
    stroke(10.0, 6.0, 12.0, 8.0),
    stroke(12.0, 8.0, 21.0, 8.0),
    stroke(21.0, 8.0, 21.0, 18.0),
    stroke(21.0, 18.0, 19.0, 20.0),
    stroke(19.0, 20.0, 5.0, 20.0),
    stroke(5.0, 20.0, 3.0, 18.0),
    stroke(3.0, 18.0, 3.0, 6.0),
];
const GIT_BRANCH: &[IconStroke] = &[
    stroke(4.0, 5.0, 6.0, 3.0),
    stroke(6.0, 3.0, 8.0, 5.0),
    stroke(8.0, 5.0, 6.0, 7.0),
    stroke(6.0, 7.0, 4.0, 5.0),
    stroke(4.0, 19.0, 6.0, 17.0),
    stroke(6.0, 17.0, 8.0, 19.0),
    stroke(8.0, 19.0, 6.0, 21.0),
    stroke(6.0, 21.0, 4.0, 19.0),
    stroke(16.0, 19.0, 18.0, 17.0),
    stroke(18.0, 17.0, 20.0, 19.0),
    stroke(20.0, 19.0, 18.0, 21.0),
    stroke(18.0, 21.0, 16.0, 19.0),
    stroke(6.0, 7.0, 6.0, 17.0),
    stroke(8.0, 5.0, 12.0, 5.0),
    stroke(12.0, 5.0, 18.0, 11.0),
    stroke(18.0, 11.0, 18.0, 17.0),
];
const GIT_COMMIT: &[IconStroke] = &[
    stroke(3.0, 12.0, 9.0, 12.0),
    stroke(15.0, 12.0, 21.0, 12.0),
    stroke(12.0, 9.0, 15.0, 12.0),
    stroke(15.0, 12.0, 12.0, 15.0),
    stroke(12.0, 15.0, 9.0, 12.0),
    stroke(9.0, 12.0, 12.0, 9.0),
];
const GLOBE: &[IconStroke] = &[
    stroke(22.0, 12.0, 21.2, 15.9),
    stroke(21.2, 15.9, 19.1, 19.1),
    stroke(19.1, 19.1, 15.9, 21.2),
    stroke(15.9, 21.2, 12.0, 22.0),
    stroke(12.0, 22.0, 8.1, 21.2),
    stroke(8.1, 21.2, 4.9, 19.1),
    stroke(4.9, 19.1, 2.8, 15.9),
    stroke(2.8, 15.9, 2.0, 12.0),
    stroke(2.0, 12.0, 2.8, 8.1),
    stroke(2.8, 8.1, 4.9, 4.9),
    stroke(4.9, 4.9, 8.1, 2.8),
    stroke(8.1, 2.8, 12.0, 2.0),
    stroke(12.0, 2.0, 15.9, 2.8),
    stroke(15.9, 2.8, 19.1, 4.9),
    stroke(19.1, 4.9, 21.2, 8.1),
    stroke(21.2, 8.1, 22.0, 12.0),
    stroke(21.8, 12.0, 22.0, 12.0),
    stroke(12.0, 2.0, 9.8, 5.0),
    stroke(9.8, 5.0, 8.4, 8.4),
    stroke(8.4, 8.4, 8.0, 12.0),
    stroke(8.0, 12.0, 8.4, 15.6),
    stroke(8.4, 15.6, 9.8, 19.0),
    stroke(9.8, 19.0, 12.0, 22.0),
    stroke(12.0, 22.0, 14.2, 19.0),
    stroke(14.2, 19.0, 15.6, 15.6),
    stroke(15.6, 15.6, 16.0, 12.0),
    stroke(16.0, 12.0, 15.6, 8.4),
    stroke(15.6, 8.4, 14.2, 5.0),
    stroke(14.2, 5.0, 12.0, 2.0),
    stroke(2.0, 12.0, 22.0, 12.0),
];
const HEADPHONE_OFF: &[IconStroke] = &[
    stroke(21.0, 14.0, 19.7, 14.0),
    stroke(9.1, 3.5, 13.3, 3.1),
    stroke(13.3, 3.1, 17.3, 4.7),
    stroke(17.3, 4.7, 20.0, 7.9),
    stroke(20.0, 7.9, 21.0, 12.0),
    stroke(21.0, 12.0, 21.0, 15.3),
    stroke(2.0, 2.0, 22.0, 22.0),
    stroke(20.4, 20.4, 19.0, 21.0),
    stroke(19.0, 21.0, 18.0, 21.0),
    stroke(18.0, 21.0, 16.0, 19.0),
    stroke(16.0, 19.0, 16.0, 16.0),
    stroke(3.0, 14.0, 6.0, 14.0),
    stroke(6.0, 14.0, 8.0, 16.0),
    stroke(8.0, 16.0, 8.0, 19.0),
    stroke(8.0, 19.0, 6.0, 21.0),
    stroke(6.0, 21.0, 5.0, 21.0),
    stroke(5.0, 21.0, 3.0, 19.0),
    stroke(3.0, 19.0, 3.0, 12.0),
    stroke(3.0, 12.0, 3.7, 8.6),
    stroke(3.7, 8.6, 5.6, 5.6),
];
const HEADPHONES: &[IconStroke] = &[
    stroke(3.0, 14.0, 6.0, 14.0),
    stroke(6.0, 14.0, 8.0, 16.0),
    stroke(8.0, 16.0, 8.0, 19.0),
    stroke(8.0, 19.0, 6.0, 21.0),
    stroke(6.0, 21.0, 5.0, 21.0),
    stroke(5.0, 21.0, 3.0, 19.0),
    stroke(3.0, 19.0, 3.0, 12.0),
    stroke(3.0, 12.0, 3.7, 8.5),
    stroke(3.7, 8.5, 5.6, 5.6),
    stroke(5.6, 5.6, 8.5, 3.7),
    stroke(8.5, 3.7, 12.0, 3.0),
    stroke(12.0, 3.0, 15.5, 3.7),
    stroke(15.5, 3.7, 18.4, 5.6),
    stroke(18.4, 5.6, 20.3, 8.5),
    stroke(20.3, 8.5, 21.0, 12.0),
    stroke(21.0, 12.0, 21.0, 19.0),
    stroke(21.0, 19.0, 19.0, 21.0),
    stroke(19.0, 21.0, 18.0, 21.0),
    stroke(18.0, 21.0, 16.0, 19.0),
    stroke(16.0, 19.0, 16.0, 16.0),
    stroke(16.0, 16.0, 18.0, 14.0),
    stroke(18.0, 14.0, 21.0, 14.0),
];
const INFO: &[IconStroke] = &[
    stroke(12.0, 3.0, 16.5, 4.2),
    stroke(16.5, 4.2, 19.8, 7.5),
    stroke(19.8, 7.5, 21.0, 12.0),
    stroke(21.0, 12.0, 19.8, 16.5),
    stroke(19.8, 16.5, 16.5, 19.8),
    stroke(16.5, 19.8, 12.0, 21.0),
    stroke(12.0, 21.0, 7.5, 19.8),
    stroke(7.5, 19.8, 4.2, 16.5),
    stroke(4.2, 16.5, 3.0, 12.0),
    stroke(3.0, 12.0, 4.2, 7.5),
    stroke(4.2, 7.5, 7.5, 4.2),
    stroke(7.5, 4.2, 12.0, 3.0),
    stroke(12.0, 11.0, 12.0, 17.0),
    stroke(12.0, 7.0, 12.2, 7.0),
];
const KEYBOARD: &[IconStroke] = &[
    stroke(9.8, 8.0, 10.0, 8.0),
    stroke(11.8, 12.0, 12.0, 12.0),
    stroke(13.8, 8.0, 14.0, 8.0),
    stroke(15.8, 12.0, 16.0, 12.0),
    stroke(17.8, 8.0, 18.0, 8.0),
    stroke(5.8, 8.0, 6.0, 8.0),
    stroke(7.0, 16.0, 17.0, 16.0),
    stroke(7.8, 12.0, 8.0, 12.0),
    stroke(4.0, 4.0, 20.0, 4.0),
    stroke(20.0, 4.0, 22.0, 6.0),
    stroke(22.0, 6.0, 22.0, 18.0),
    stroke(22.0, 18.0, 20.0, 20.0),
    stroke(20.0, 20.0, 4.0, 20.0),
    stroke(4.0, 20.0, 2.0, 18.0),
    stroke(2.0, 18.0, 2.0, 6.0),
    stroke(2.0, 6.0, 4.0, 4.0),
    stroke(3.8, 4.0, 4.0, 4.0),
];
const LAYOUT_DASHBOARD: &[IconStroke] = &[
    stroke(3.0, 3.0, 10.0, 3.0),
    stroke(10.0, 3.0, 10.0, 11.0),
    stroke(10.0, 11.0, 3.0, 11.0),
    stroke(3.0, 11.0, 3.0, 3.0),
    stroke(14.0, 3.0, 21.0, 3.0),
    stroke(21.0, 3.0, 21.0, 8.0),
    stroke(21.0, 8.0, 14.0, 8.0),
    stroke(14.0, 8.0, 14.0, 3.0),
    stroke(14.0, 12.0, 21.0, 12.0),
    stroke(21.0, 12.0, 21.0, 21.0),
    stroke(21.0, 21.0, 14.0, 21.0),
    stroke(14.0, 21.0, 14.0, 12.0),
    stroke(3.0, 15.0, 10.0, 15.0),
    stroke(10.0, 15.0, 10.0, 21.0),
    stroke(10.0, 21.0, 3.0, 21.0),
    stroke(3.0, 21.0, 3.0, 15.0),
];
const LOCK: &[IconStroke] = &[
    stroke(5.0, 11.0, 19.0, 11.0),
    stroke(19.0, 11.0, 21.0, 13.0),
    stroke(21.0, 13.0, 21.0, 20.0),
    stroke(21.0, 20.0, 19.0, 22.0),
    stroke(19.0, 22.0, 5.0, 22.0),
    stroke(5.0, 22.0, 3.0, 20.0),
    stroke(3.0, 20.0, 3.0, 13.0),
    stroke(3.0, 13.0, 5.0, 11.0),
    stroke(4.8, 11.0, 5.0, 11.0),
    stroke(7.0, 11.0, 7.0, 7.0),
    stroke(7.0, 7.0, 8.5, 3.5),
    stroke(8.5, 3.5, 12.0, 2.0),
    stroke(12.0, 2.0, 15.5, 3.5),
    stroke(15.5, 3.5, 17.0, 7.0),
    stroke(17.0, 7.0, 17.0, 11.0),
];
const LOG_OUT: &[IconStroke] = &[
    stroke(16.0, 17.0, 21.0, 12.0),
    stroke(21.0, 12.0, 16.0, 7.0),
    stroke(21.0, 12.0, 9.0, 12.0),
    stroke(9.0, 21.0, 5.0, 21.0),
    stroke(5.0, 21.0, 3.0, 19.0),
    stroke(3.0, 19.0, 3.0, 5.0),
    stroke(3.0, 5.0, 5.0, 3.0),
    stroke(5.0, 3.0, 9.0, 3.0),
];
const MENU: &[IconStroke] = &[
    stroke(4.0, 6.0, 20.0, 6.0),
    stroke(4.0, 12.0, 20.0, 12.0),
    stroke(4.0, 18.0, 20.0, 18.0),
];
const MESSAGE_SQUARE: &[IconStroke] = &[
    stroke(22.0, 17.0, 20.0, 19.0),
    stroke(20.0, 19.0, 6.8, 19.0),
    stroke(6.8, 19.0, 5.4, 19.6),
    stroke(5.4, 19.6, 3.2, 21.8),
    stroke(3.2, 21.8, 2.4, 21.9),
    stroke(2.4, 21.9, 2.0, 21.3),
    stroke(2.0, 21.3, 2.0, 5.0),
    stroke(2.0, 5.0, 4.0, 3.0),
    stroke(4.0, 3.0, 20.0, 3.0),
    stroke(20.0, 3.0, 22.0, 5.0),
    stroke(22.0, 5.0, 22.0, 17.0),
];
const MIC: &[IconStroke] = &[
    stroke(12.0, 19.0, 12.0, 22.0),
    stroke(19.0, 10.0, 19.0, 12.0),
    stroke(19.0, 12.0, 18.0, 15.5),
    stroke(18.0, 15.5, 15.5, 18.0),
    stroke(15.5, 18.0, 12.0, 19.0),
    stroke(12.0, 19.0, 8.5, 18.0),
    stroke(8.5, 18.0, 6.0, 15.5),
    stroke(6.0, 15.5, 5.0, 12.0),
    stroke(5.0, 12.0, 5.0, 10.0),
    stroke(11.8, 2.0, 12.0, 2.0),
    stroke(12.0, 2.0, 15.0, 5.0),
    stroke(15.0, 5.0, 15.0, 12.0),
    stroke(15.0, 12.0, 12.0, 15.0),
    stroke(11.8, 15.0, 12.0, 15.0),
    stroke(12.0, 15.0, 9.0, 12.0),
    stroke(9.0, 12.0, 9.0, 5.0),
    stroke(9.0, 5.0, 12.0, 2.0),
    stroke(11.8, 2.0, 12.0, 2.0),
];
const MIC_OFF: &[IconStroke] = &[
    stroke(12.0, 19.0, 12.0, 22.0),
    stroke(15.0, 9.3, 15.0, 5.0),
    stroke(15.0, 5.0, 12.7, 2.1),
    stroke(12.7, 2.1, 9.3, 3.7),
    stroke(17.0, 17.0, 13.4, 18.9),
    stroke(13.4, 18.9, 9.3, 18.5),
    stroke(9.3, 18.5, 6.2, 15.9),
    stroke(6.2, 15.9, 5.0, 12.0),
    stroke(5.0, 12.0, 5.0, 10.0),
    stroke(18.9, 13.2, 19.0, 12.0),
    stroke(19.0, 12.0, 19.0, 10.0),
    stroke(2.0, 2.0, 22.0, 22.0),
    stroke(9.0, 9.0, 9.0, 12.0),
    stroke(9.0, 12.0, 10.9, 14.8),
    stroke(10.9, 14.8, 14.1, 14.1),
];
const MORE_HORIZONTAL: &[IconStroke] = &[
    stroke(6.0, 12.0, 6.2, 12.0),
    stroke(12.0, 12.0, 12.2, 12.0),
    stroke(18.0, 12.0, 18.2, 12.0),
];
const MOVE: &[IconStroke] = &[
    stroke(12.0, 2.0, 12.0, 22.0),
    stroke(15.0, 19.0, 12.0, 22.0),
    stroke(12.0, 22.0, 9.0, 19.0),
    stroke(19.0, 9.0, 22.0, 12.0),
    stroke(22.0, 12.0, 19.0, 15.0),
    stroke(2.0, 12.0, 22.0, 12.0),
    stroke(5.0, 9.0, 2.0, 12.0),
    stroke(2.0, 12.0, 5.0, 15.0),
    stroke(9.0, 5.0, 12.0, 2.0),
    stroke(12.0, 2.0, 15.0, 5.0),
];
const PAPERCLIP: &[IconStroke] = &[
    stroke(16.0, 6.0, 7.6, 14.6),
    stroke(7.6, 14.6, 7.6, 17.4),
    stroke(7.6, 17.4, 10.4, 17.4),
    stroke(10.4, 17.4, 18.8, 8.8),
    stroke(18.8, 8.8, 20.0, 6.0),
    stroke(20.0, 6.0, 18.8, 3.2),
    stroke(18.8, 3.2, 16.0, 2.0),
    stroke(16.0, 2.0, 13.2, 3.2),
    stroke(13.2, 3.2, 4.8, 11.7),
    stroke(4.8, 11.7, 3.2, 14.4),
    stroke(3.2, 14.4, 3.2, 17.5),
    stroke(3.2, 17.5, 4.8, 20.2),
    stroke(4.8, 20.2, 7.5, 21.8),
    stroke(7.5, 21.8, 10.6, 21.8),
    stroke(10.6, 21.8, 13.3, 20.2),
    stroke(13.3, 20.2, 21.7, 11.7),
];
const PLUS: &[IconStroke] = &[stroke(12.0, 5.0, 12.0, 19.0), stroke(5.0, 12.0, 19.0, 12.0)];
const REFRESH_CW: &[IconStroke] = &[
    stroke(20.0, 12.0, 18.0, 17.0),
    stroke(18.0, 17.0, 14.0, 20.0),
    stroke(14.0, 20.0, 9.0, 19.0),
    stroke(9.0, 19.0, 5.5, 16.0),
    stroke(4.0, 12.0, 6.0, 7.0),
    stroke(6.0, 7.0, 10.0, 4.0),
    stroke(10.0, 4.0, 15.0, 5.0),
    stroke(15.0, 5.0, 18.5, 8.0),
    stroke(18.0, 3.0, 18.0, 7.0),
    stroke(18.0, 7.0, 14.0, 7.0),
    stroke(6.0, 21.0, 6.0, 17.0),
    stroke(6.0, 17.0, 10.0, 17.0),
];
const ROTATE_CCW: &[IconStroke] = &[
    stroke(3.0, 12.0, 3.7, 15.5),
    stroke(3.7, 15.5, 5.6, 18.4),
    stroke(5.6, 18.4, 8.5, 20.3),
    stroke(8.5, 20.3, 12.0, 21.0),
    stroke(12.0, 21.0, 15.5, 20.3),
    stroke(15.5, 20.3, 18.4, 18.4),
    stroke(18.4, 18.4, 20.3, 15.5),
    stroke(20.3, 15.5, 21.0, 12.0),
    stroke(21.0, 12.0, 20.3, 8.5),
    stroke(20.3, 8.5, 18.4, 5.6),
    stroke(18.4, 5.6, 15.5, 3.7),
    stroke(15.5, 3.7, 12.0, 3.0),
    stroke(12.0, 3.0, 8.4, 3.7),
    stroke(8.4, 3.7, 5.3, 5.7),
    stroke(5.3, 5.7, 3.0, 8.0),
    stroke(3.0, 3.0, 3.0, 8.0),
    stroke(3.0, 8.0, 8.0, 8.0),
];
const ROTATE_CW: &[IconStroke] = &[
    stroke(21.0, 12.0, 20.3, 15.5),
    stroke(20.3, 15.5, 18.4, 18.4),
    stroke(18.4, 18.4, 15.5, 20.3),
    stroke(15.5, 20.3, 12.0, 21.0),
    stroke(12.0, 21.0, 8.5, 20.3),
    stroke(8.5, 20.3, 5.6, 18.4),
    stroke(5.6, 18.4, 3.7, 15.5),
    stroke(3.7, 15.5, 3.0, 12.0),
    stroke(3.0, 12.0, 3.7, 8.5),
    stroke(3.7, 8.5, 5.6, 5.6),
    stroke(5.6, 5.6, 8.5, 3.7),
    stroke(8.5, 3.7, 12.0, 3.0),
    stroke(12.0, 3.0, 15.6, 3.7),
    stroke(15.6, 3.7, 18.7, 5.7),
    stroke(18.7, 5.7, 21.0, 8.0),
    stroke(21.0, 3.0, 21.0, 8.0),
    stroke(21.0, 8.0, 16.0, 8.0),
];
const RULER: &[IconStroke] = &[
    stroke(21.3, 15.3, 22.0, 17.0),
    stroke(22.0, 17.0, 21.3, 18.7),
    stroke(21.3, 18.7, 18.7, 21.3),
    stroke(18.7, 21.3, 17.0, 22.0),
    stroke(17.0, 22.0, 15.3, 21.3),
    stroke(15.3, 21.3, 2.7, 8.7),
    stroke(2.7, 8.7, 2.7, 5.3),
    stroke(2.7, 5.3, 5.3, 2.7),
    stroke(5.3, 2.7, 8.7, 2.7),
    stroke(8.7, 2.7, 21.3, 15.3),
    stroke(14.5, 12.5, 16.5, 10.5),
    stroke(11.5, 9.5, 13.5, 7.5),
    stroke(8.5, 6.5, 10.5, 4.5),
    stroke(17.5, 15.5, 19.5, 13.5),
];
const SCALING: &[IconStroke] = &[
    stroke(12.0, 3.0, 5.0, 3.0),
    stroke(5.0, 3.0, 3.0, 5.0),
    stroke(3.0, 5.0, 3.0, 19.0),
    stroke(3.0, 19.0, 5.0, 21.0),
    stroke(5.0, 21.0, 19.0, 21.0),
    stroke(19.0, 21.0, 21.0, 19.0),
    stroke(21.0, 19.0, 21.0, 12.0),
    stroke(14.0, 15.0, 9.0, 15.0),
    stroke(9.0, 15.0, 9.0, 10.0),
    stroke(16.0, 3.0, 21.0, 3.0),
    stroke(21.0, 3.0, 21.0, 8.0),
    stroke(21.0, 3.0, 9.0, 15.0),
];
const SCREEN_SHARE: &[IconStroke] = &[
    stroke(13.0, 3.0, 4.0, 3.0),
    stroke(4.0, 3.0, 2.0, 5.0),
    stroke(2.0, 5.0, 2.0, 15.0),
    stroke(2.0, 15.0, 4.0, 17.0),
    stroke(4.0, 17.0, 20.0, 17.0),
    stroke(20.0, 17.0, 22.0, 15.0),
    stroke(22.0, 15.0, 22.0, 12.0),
    stroke(8.0, 21.0, 16.0, 21.0),
    stroke(12.0, 17.0, 12.0, 21.0),
    stroke(17.0, 8.0, 22.0, 3.0),
    stroke(17.0, 3.0, 22.0, 3.0),
    stroke(22.0, 3.0, 22.0, 8.0),
];
const SEARCH: &[IconStroke] = &[
    stroke(11.0, 4.0, 14.5, 5.0),
    stroke(14.5, 5.0, 17.0, 7.5),
    stroke(17.0, 7.5, 18.0, 11.0),
    stroke(18.0, 11.0, 17.0, 14.5),
    stroke(17.0, 14.5, 14.5, 17.0),
    stroke(14.5, 17.0, 11.0, 18.0),
    stroke(11.0, 18.0, 7.5, 17.0),
    stroke(7.5, 17.0, 5.0, 14.5),
    stroke(5.0, 14.5, 4.0, 11.0),
    stroke(4.0, 11.0, 5.0, 7.5),
    stroke(5.0, 7.5, 7.5, 5.0),
    stroke(7.5, 5.0, 11.0, 4.0),
    stroke(16.0, 16.0, 21.0, 21.0),
];
const SEND: &[IconStroke] = &[
    stroke(14.5, 21.7, 15.0, 22.0),
    stroke(15.0, 22.0, 15.5, 21.7),
    stroke(15.5, 21.7, 22.0, 2.7),
    stroke(22.0, 2.7, 21.9, 2.1),
    stroke(21.9, 2.1, 21.3, 2.0),
    stroke(21.3, 2.0, 2.3, 8.5),
    stroke(2.3, 8.5, 2.0, 9.0),
    stroke(2.0, 9.0, 2.3, 9.5),
    stroke(2.3, 9.5, 10.2, 12.6),
    stroke(10.2, 12.6, 11.4, 13.8),
    stroke(11.4, 13.8, 14.5, 21.7),
    stroke(21.9, 2.1, 10.9, 13.1),
];
const SETTINGS: &[IconStroke] = &[
    stroke(12.0, 9.0, 15.0, 12.0),
    stroke(15.0, 12.0, 12.0, 15.0),
    stroke(12.0, 15.0, 9.0, 12.0),
    stroke(9.0, 12.0, 12.0, 9.0),
    stroke(12.0, 3.0, 12.0, 6.0),
    stroke(12.0, 18.0, 12.0, 21.0),
    stroke(3.0, 12.0, 6.0, 12.0),
    stroke(18.0, 12.0, 21.0, 12.0),
    stroke(5.6, 5.6, 7.8, 7.8),
    stroke(16.2, 16.2, 18.4, 18.4),
    stroke(18.4, 5.6, 16.2, 7.8),
    stroke(7.8, 16.2, 5.6, 18.4),
];
const SMILE: &[IconStroke] = &[
    stroke(22.0, 12.0, 21.2, 15.9),
    stroke(21.2, 15.9, 19.1, 19.1),
    stroke(19.1, 19.1, 15.9, 21.2),
    stroke(15.9, 21.2, 12.0, 22.0),
    stroke(12.0, 22.0, 8.1, 21.2),
    stroke(8.1, 21.2, 4.9, 19.1),
    stroke(4.9, 19.1, 2.8, 15.9),
    stroke(2.8, 15.9, 2.0, 12.0),
    stroke(2.0, 12.0, 2.8, 8.1),
    stroke(2.8, 8.1, 4.9, 4.9),
    stroke(4.9, 4.9, 8.1, 2.8),
    stroke(8.1, 2.8, 12.0, 2.0),
    stroke(12.0, 2.0, 15.9, 2.8),
    stroke(15.9, 2.8, 19.1, 4.9),
    stroke(19.1, 4.9, 21.2, 8.1),
    stroke(21.2, 8.1, 22.0, 12.0),
    stroke(21.8, 12.0, 22.0, 12.0),
    stroke(8.0, 14.0, 12.0, 16.0),
    stroke(12.0, 16.0, 16.0, 14.0),
    stroke(8.8, 9.0, 9.0, 9.0),
    stroke(14.8, 9.0, 15.0, 9.0),
];
const TERMINAL: &[IconStroke] = &[
    stroke(12.0, 19.0, 20.0, 19.0),
    stroke(4.0, 17.0, 10.0, 11.0),
    stroke(10.0, 11.0, 4.0, 5.0),
];
const UPLOAD: &[IconStroke] = &[
    stroke(12.0, 21.0, 12.0, 9.0),
    stroke(7.0, 14.0, 12.0, 9.0),
    stroke(12.0, 9.0, 17.0, 14.0),
    stroke(5.0, 3.0, 19.0, 3.0),
];
const USERS: &[IconStroke] = &[
    stroke(6.0, 8.0, 9.0, 5.0),
    stroke(9.0, 5.0, 12.0, 8.0),
    stroke(12.0, 8.0, 9.0, 11.0),
    stroke(9.0, 11.0, 6.0, 8.0),
    stroke(3.0, 21.0, 5.0, 17.0),
    stroke(5.0, 17.0, 9.0, 15.0),
    stroke(9.0, 15.0, 13.0, 17.0),
    stroke(13.0, 17.0, 15.0, 21.0),
    stroke(16.0, 5.0, 18.5, 8.0),
    stroke(18.5, 8.0, 16.0, 11.0),
    stroke(16.0, 16.0, 19.0, 17.5),
    stroke(19.0, 17.5, 21.0, 21.0),
];
const VOLUME_2: &[IconStroke] = &[
    stroke(11.0, 4.7, 10.6, 4.1),
    stroke(10.6, 4.1, 9.8, 4.2),
    stroke(9.8, 4.2, 6.4, 7.6),
    stroke(6.4, 7.6, 5.4, 8.0),
    stroke(5.4, 8.0, 3.0, 8.0),
    stroke(3.0, 8.0, 2.0, 9.0),
    stroke(2.0, 9.0, 2.0, 15.0),
    stroke(2.0, 15.0, 3.0, 16.0),
    stroke(3.0, 16.0, 5.4, 16.0),
    stroke(5.4, 16.0, 6.4, 16.4),
    stroke(6.4, 16.4, 9.8, 19.8),
    stroke(9.8, 19.8, 10.6, 20.0),
    stroke(10.6, 20.0, 11.0, 19.3),
    stroke(11.0, 19.3, 11.0, 4.7),
    stroke(16.0, 9.0, 17.0, 12.0),
    stroke(17.0, 12.0, 16.0, 15.0),
    stroke(19.4, 18.4, 21.3, 15.4),
    stroke(21.3, 15.4, 22.0, 12.0),
    stroke(22.0, 12.0, 21.3, 8.6),
    stroke(21.3, 8.6, 19.4, 5.6),
];
const VOLUME_X: &[IconStroke] = &[
    stroke(11.0, 4.7, 10.6, 4.1),
    stroke(10.6, 4.1, 9.8, 4.2),
    stroke(9.8, 4.2, 6.4, 7.6),
    stroke(6.4, 7.6, 5.4, 8.0),
    stroke(5.4, 8.0, 3.0, 8.0),
    stroke(3.0, 8.0, 2.0, 9.0),
    stroke(2.0, 9.0, 2.0, 15.0),
    stroke(2.0, 15.0, 3.0, 16.0),
    stroke(3.0, 16.0, 5.4, 16.0),
    stroke(5.4, 16.0, 6.4, 16.4),
    stroke(6.4, 16.4, 9.8, 19.8),
    stroke(9.8, 19.8, 10.6, 20.0),
    stroke(10.6, 20.0, 11.0, 19.3),
    stroke(11.0, 19.3, 11.0, 4.7),
    stroke(22.0, 9.0, 16.0, 15.0),
    stroke(16.0, 9.0, 22.0, 15.0),
];
const WIFI: &[IconStroke] = &[
    stroke(11.8, 20.0, 12.0, 20.0),
    stroke(2.0, 8.8, 5.1, 6.7),
    stroke(5.1, 6.7, 8.5, 5.4),
    stroke(8.5, 5.4, 12.0, 5.0),
    stroke(12.0, 5.0, 15.5, 5.4),
    stroke(15.5, 5.4, 18.9, 6.7),
    stroke(18.9, 6.7, 22.0, 8.8),
    stroke(5.0, 12.9, 8.3, 10.7),
    stroke(8.3, 10.7, 12.0, 10.0),
    stroke(12.0, 10.0, 15.7, 10.7),
    stroke(15.7, 10.7, 19.0, 12.9),
    stroke(8.5, 16.4, 12.0, 15.0),
    stroke(12.0, 15.0, 15.5, 16.4),
];
const X: &[IconStroke] = &[stroke(18.0, 6.0, 6.0, 18.0), stroke(6.0, 6.0, 18.0, 18.0)];

/// Flattened line strokes in the same 24x24 coordinate system as
/// [`icon_path`]. This is the first GPU-native icon vocabulary: it is
/// deliberately line-segment based so shader theming can own stroke
/// treatment without parsing SVG paths at frame time.
pub fn icon_strokes(name: IconName) -> &'static [IconStroke] {
    match name {
        IconName::Activity => ACTIVITY,
        IconName::AlertCircle => ALERT_CIRCLE,
        IconName::ArrowDown => ARROW_DOWN,
        IconName::ArrowLeft => ARROW_LEFT,
        IconName::ArrowRight => ARROW_RIGHT,
        IconName::ArrowUp => ARROW_UP,
        IconName::BarChart => BAR_CHART,
        IconName::Bell => BELL,
        IconName::Box => BOX,
        IconName::Camera => CAMERA,
        IconName::Check => CHECK,
        IconName::ChevronDown => CHEVRON_DOWN,
        IconName::ChevronLeft => CHEVRON_LEFT,
        IconName::ChevronRight => CHEVRON_RIGHT,
        IconName::ChevronUp => CHEVRON_UP,
        IconName::Code => CODE,
        IconName::Command => COMMAND,
        IconName::Contrast => CONTRAST,
        IconName::Download => DOWNLOAD,
        IconName::ExternalLink => EXTERNAL_LINK,
        IconName::Eye => EYE,
        IconName::FileText => FILE_TEXT,
        IconName::FlipHorizontal => FLIP_HORIZONTAL,
        IconName::Folder => FOLDER,
        IconName::GitBranch => GIT_BRANCH,
        IconName::GitCommit => GIT_COMMIT,
        IconName::Globe => GLOBE,
        IconName::HeadphoneOff => HEADPHONE_OFF,
        IconName::Headphones => HEADPHONES,
        IconName::Info => INFO,
        IconName::Keyboard => KEYBOARD,
        IconName::LayoutDashboard => LAYOUT_DASHBOARD,
        IconName::Lock => LOCK,
        IconName::LogOut => LOG_OUT,
        IconName::Menu => MENU,
        IconName::MessageSquare => MESSAGE_SQUARE,
        IconName::Mic => MIC,
        IconName::MicOff => MIC_OFF,
        IconName::MoreHorizontal => MORE_HORIZONTAL,
        IconName::Move => MOVE,
        IconName::Paperclip => PAPERCLIP,
        IconName::Plus => PLUS,
        IconName::RefreshCw => REFRESH_CW,
        IconName::RotateCcw => ROTATE_CCW,
        IconName::RotateCw => ROTATE_CW,
        IconName::Ruler => RULER,
        IconName::Scaling => SCALING,
        IconName::ScreenShare => SCREEN_SHARE,
        IconName::Search => SEARCH,
        IconName::Send => SEND,
        IconName::Settings => SETTINGS,
        IconName::Smile => SMILE,
        IconName::Terminal => TERMINAL,
        IconName::Upload => UPLOAD,
        IconName::Users => USERS,
        IconName::Volume2 => VOLUME_2,
        IconName::VolumeX => VOLUME_X,
        IconName::Wifi => WIFI,
        IconName::X => X,
    }
}

/// Parsed vector IR for a built-in icon (24×24 view box, strokes as
/// `currentColor`). Built lazily once per process from the
/// [`icon_path`] SVG markup and cached in a static.
pub fn icon_vector_asset(name: IconName) -> &'static VectorAsset {
    static ASSETS: OnceLock<Vec<VectorAsset>> = OnceLock::new();
    &ASSETS.get_or_init(build_icon_vector_assets)[name_index(name)]
}

/// Every built-in [`IconName`] — the full icon vocabulary, in
/// alphabetical order. String-typed `icon("...")` names resolve
/// against this set.
pub fn all_icon_names() -> &'static [IconName] {
    &[
        IconName::Activity,
        IconName::AlertCircle,
        IconName::ArrowDown,
        IconName::ArrowLeft,
        IconName::ArrowRight,
        IconName::ArrowUp,
        IconName::BarChart,
        IconName::Bell,
        IconName::Box,
        IconName::Camera,
        IconName::Check,
        IconName::ChevronDown,
        IconName::ChevronLeft,
        IconName::ChevronRight,
        IconName::ChevronUp,
        IconName::Code,
        IconName::Command,
        IconName::Contrast,
        IconName::Download,
        IconName::ExternalLink,
        IconName::Eye,
        IconName::FileText,
        IconName::FlipHorizontal,
        IconName::Folder,
        IconName::GitBranch,
        IconName::GitCommit,
        IconName::Globe,
        IconName::HeadphoneOff,
        IconName::Headphones,
        IconName::Info,
        IconName::Keyboard,
        IconName::LayoutDashboard,
        IconName::Lock,
        IconName::LogOut,
        IconName::Menu,
        IconName::MessageSquare,
        IconName::Mic,
        IconName::MicOff,
        IconName::MoreHorizontal,
        IconName::Move,
        IconName::Paperclip,
        IconName::Plus,
        IconName::RefreshCw,
        IconName::RotateCcw,
        IconName::RotateCw,
        IconName::Ruler,
        IconName::Scaling,
        IconName::ScreenShare,
        IconName::Search,
        IconName::Send,
        IconName::Settings,
        IconName::Smile,
        IconName::Terminal,
        IconName::Upload,
        IconName::Users,
        IconName::Volume2,
        IconName::VolumeX,
        IconName::Wifi,
        IconName::X,
    ]
}

fn build_icon_vector_assets() -> Vec<VectorAsset> {
    all_icon_names()
        .iter()
        .map(|name| {
            let svg = format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
                icon_path(*name)
            );
            parse_current_color_svg_asset(&svg)
                .unwrap_or_else(|err| panic!("failed to parse built-in icon {}: {err}", name.name()))
        })
        .collect()
}

fn name_index(name: IconName) -> usize {
    all_icon_names()
        .iter()
        .position(|n| *n == name)
        .expect("IconName missing from all_icon_names")
}

/// SVG path markup in a 24x24 coordinate system. Paths deliberately use
/// `currentColor`; the SVG fallback supplies colour/stroke externally.
pub fn icon_path(name: IconName) -> &'static str {
    match name {
        IconName::Activity => r#"<path d="M3 12h4l3-8 4 16 3-8h4"/>"#,
        IconName::AlertCircle => {
            r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v6"/><path d="M12 17h.01"/>"#
        }
        IconName::ArrowDown => r#"<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>"#,
        IconName::ArrowLeft => r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#,
        IconName::ArrowRight => r#"<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>"#,
        IconName::ArrowUp => r#"<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>"#,
        IconName::BarChart => r#"<path d="M4 20V10"/><path d="M12 20V4"/><path d="M20 20v-7"/>"#,
        IconName::Bell => {
            r#"<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9"/><path d="M10 21h4"/>"#
        }
        IconName::Box => {
            r#"<path d="M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"/><path d="m3.3 7 8.7 5 8.7-5"/><path d="M12 22V12"/>"#
        }
        IconName::Camera => {
            r#"<path d="M13.997 4a2 2 0 0 1 1.76 1.05l.486.9A2 2 0 0 0 18.003 7H20a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2h1.997a2 2 0 0 0 1.759-1.048l.489-.904A2 2 0 0 1 10.004 4z"/><circle cx="12" cy="13" r="3"/>"#
        }
        IconName::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
        IconName::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
        IconName::ChevronLeft => r#"<path d="m15 6-6 6 6 6"/>"#,
        IconName::ChevronRight => r#"<path d="m9 6 6 6-6 6"/>"#,
        IconName::ChevronUp => r#"<path d="m6 15 6-6 6 6"/>"#,
        IconName::Code => r#"<path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/>"#,
        IconName::Command => {
            r#"<path d="M9 9h6v6H9z"/><path d="M9 9H6a3 3 0 1 1 3-3v3Z"/><path d="M15 9V6a3 3 0 1 1 3 3h-3Z"/><path d="M15 15h3a3 3 0 1 1-3 3v-3Z"/><path d="M9 15v3a3 3 0 1 1-3-3h3Z"/>"#
        }
        IconName::Contrast => {
            r#"<circle cx="12" cy="12" r="10"/><path d="M12 18a6 6 0 0 0 0-12v12z"/>"#
        }
        IconName::Download => {
            r#"<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>"#
        }
        IconName::ExternalLink => {
            r#"<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>"#
        }
        IconName::Eye => {
            r#"<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>"#
        }
        IconName::FileText => {
            r#"<path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/><path d="M8 12h8"/><path d="M8 16h6"/>"#
        }
        IconName::FlipHorizontal => {
            r#"<path d="M8 3H5a2 2 0 0 0-2 2v14c0 1.1.9 2 2 2h3"/><path d="M16 3h3a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-3"/><path d="M12 20v2"/><path d="M12 14v2"/><path d="M12 8v2"/><path d="M12 2v2"/>"#
        }
        IconName::Folder => r#"<path d="M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        IconName::GitBranch => {
            r#"<circle cx="6" cy="5" r="2"/><circle cx="18" cy="19" r="2"/><circle cx="6" cy="19" r="2"/><path d="M6 7v10"/><path d="M8 5h4a6 6 0 0 1 6 6v6"/>"#
        }
        IconName::GitCommit => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M3 12h6"/><path d="M15 12h6"/>"#
        }
        IconName::Globe => {
            r#"<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>"#
        }
        IconName::HeadphoneOff => {
            r#"<path d="M21 14h-1.343"/><path d="M9.128 3.47A9 9 0 0 1 21 12v3.343"/><path d="m2 2 20 20"/><path d="M20.414 20.414A2 2 0 0 1 19 21h-1a2 2 0 0 1-2-2v-3"/><path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 2.636-6.364"/>"#
        }
        IconName::Headphones => {
            r#"<path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3"/>"#
        }
        IconName::Info => {
            r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7h.01"/>"#
        }
        IconName::Keyboard => {
            r#"<path d="M10 8h.01"/><path d="M12 12h.01"/><path d="M14 8h.01"/><path d="M16 12h.01"/><path d="M18 8h.01"/><path d="M6 8h.01"/><path d="M7 16h10"/><path d="M8 12h.01"/><rect width="20" height="16" x="2" y="4" rx="2"/>"#
        }
        IconName::LayoutDashboard => {
            r#"<rect x="3" y="3" width="7" height="8"/><rect x="14" y="3" width="7" height="5"/><rect x="14" y="12" width="7" height="9"/><rect x="3" y="15" width="7" height="6"/>"#
        }
        IconName::Lock => {
            r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>"#
        }
        IconName::LogOut => {
            r#"<path d="m16 17 5-5-5-5"/><path d="M21 12H9"/><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/>"#
        }
        IconName::Menu => r#"<path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h16"/>"#,
        IconName::MessageSquare => {
            r#"<path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z"/>"#
        }
        IconName::Mic => {
            r#"<path d="M12 19v3"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><rect x="9" y="2" width="6" height="13" rx="3"/>"#
        }
        IconName::MicOff => {
            r#"<path d="M12 19v3"/><path d="M15 9.34V5a3 3 0 0 0-5.68-1.33"/><path d="M16.95 16.95A7 7 0 0 1 5 12v-2"/><path d="M18.89 13.23A7 7 0 0 0 19 12v-2"/><path d="m2 2 20 20"/><path d="M9 9v3a3 3 0 0 0 5.12 2.12"/>"#
        }
        IconName::MoreHorizontal => {
            r#"<path d="M6 12h.01"/><path d="M12 12h.01"/><path d="M18 12h.01"/>"#
        }
        IconName::Move => {
            r#"<path d="M12 2v20"/><path d="m15 19-3 3-3-3"/><path d="m19 9 3 3-3 3"/><path d="M2 12h20"/><path d="m5 9-3 3 3 3"/><path d="m9 5 3-3 3 3"/>"#
        }
        IconName::Paperclip => {
            r#"<path d="m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551"/>"#
        }
        IconName::Plus => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        IconName::RefreshCw => {
            r#"<path d="M21 12a9 9 0 0 1-15.5 6.2"/><path d="M3 12A9 9 0 0 1 18.5 5.8"/><path d="M18 3v4h-4"/><path d="M6 21v-4h4"/>"#
        }
        IconName::RotateCcw => {
            r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>"#
        }
        IconName::RotateCw => {
            r#"<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>"#
        }
        IconName::Ruler => {
            r#"<path d="M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z"/><path d="m14.5 12.5 2-2"/><path d="m11.5 9.5 2-2"/><path d="m8.5 6.5 2-2"/><path d="m17.5 15.5 2-2"/>"#
        }
        IconName::Scaling => {
            r#"<path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M14 15H9v-5"/><path d="M16 3h5v5"/><path d="M21 3 9 15"/>"#
        }
        IconName::ScreenShare => {
            r#"<path d="M13 3H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-3"/><path d="M8 21h8"/><path d="M12 17v4"/><path d="m17 8 5-5"/><path d="M17 3h5v5"/>"#
        }
        IconName::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m16 16 5 5"/>"#,
        IconName::Send => {
            r#"<path d="M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z"/><path d="m21.854 2.147-10.94 10.939"/>"#
        }
        IconName::Settings => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.5-2.4 1a7 7 0 0 0-1.8-1L14.4 3h-4.8L9.3 6a7 7 0 0 0-1.8 1L5.1 6l-2 3.5 2 1.5a7 7 0 0 0 0 2l-2 1.5 2 3.5 2.4-1a7 7 0 0 0 1.8 1l.3 3h4.8l.3-3a7 7 0 0 0 1.8-1l2.4 1 2-3.5-2-1.5a7 7 0 0 0 .1-1Z"/>"#
        }
        IconName::Smile => {
            r#"<circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><path d="M9 9h.01"/><path d="M15 9h.01"/>"#
        }
        IconName::Terminal => r#"<path d="M12 19h8"/><path d="m4 17 6-6-6-6"/>"#,
        IconName::Upload => r#"<path d="M12 21V9"/><path d="m7 14 5-5 5 5"/><path d="M5 3h14"/>"#,
        IconName::Users => {
            r#"<circle cx="9" cy="8" r="3"/><path d="M3 21a6 6 0 0 1 12 0"/><path d="M16 11a3 3 0 0 0 0-6"/><path d="M21 21a5 5 0 0 0-5-5"/>"#
        }
        IconName::Volume2 => {
            r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>"#
        }
        IconName::VolumeX => {
            r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><path d="m22 9-6 6"/><path d="m16 9 6 6"/>"#
        }
        IconName::Wifi => {
            r#"<path d="M12 20h.01"/><path d="M2 8.82a15 15 0 0 1 20 0"/><path d="M5 12.859a10 10 0 0 1 14 0"/><path d="M8.5 16.429a5 5 0 0 1 7 0"/>"#
        }
        IconName::X => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_names_parse_to_familiar_icon_names() {
        assert_eq!(IconName::parse("search"), Some(IconName::Search));
        assert_eq!(
            IconName::parse("dashboard"),
            Some(IconName::LayoutDashboard)
        );
        assert_eq!(IconName::parse("refresh"), Some(IconName::RefreshCw));
        assert_eq!(IconName::parse("missing"), None);
    }

    #[test]
    fn icon_builder_sets_vector_icon_defaults() {
        use super::svg::IconSource;
        let el = icon("git-branch");
        assert_eq!(el.icon, Some(IconSource::Builtin(IconName::GitBranch)));
        assert_eq!(el.width, Size::Fixed(16.0));
        assert_eq!(el.height, Size::Fixed(16.0));
        assert_eq!(el.text_color, Some(tokens::FOREGROUND));
    }

    /// Unknown string-typed icon names are preserved as
    /// `IconSource::UnknownName` (so the lint can flag them) and still
    /// render the AlertCircle fallback asset rather than panicking —
    /// important so an LLM-generated typo surfaces visibly but doesn't
    /// take the whole program down.
    #[test]
    fn unknown_string_icon_is_preserved_and_falls_back() {
        use super::svg::IconSource;
        let el = icon("not-a-real-icon-name");
        assert_eq!(
            el.icon,
            Some(IconSource::UnknownName("not-a-real-icon-name".to_string()))
        );
        // The fallback still paints AlertCircle's vector.
        let alert = IconSource::Builtin(IconName::AlertCircle);
        assert_eq!(
            el.icon.as_ref().unwrap().vector_asset(),
            alert.vector_asset()
        );
        let el2 = icon(String::from("abrows-right"));
        assert_eq!(
            el2.icon,
            Some(IconSource::UnknownName("abrows-right".to_string()))
        );
    }

    /// Every canonical name round-trips through [`IconName::parse`] and
    /// is unique. The vocabulary lives in four parallel per-icon tables
    /// ([`IconName::name`], [`IconName::parse`], [`icon_path`],
    /// [`icon_strokes`]); the compiler catches a *missing* arm but not a
    /// name typo'd in only one of them, which this does.
    #[test]
    fn every_builtin_icon_name_round_trips_and_is_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for name in all_icon_names() {
            let s = name.name();
            assert_eq!(
                IconName::parse(s),
                Some(*name),
                "`{s}` does not round-trip through IconName::parse"
            );
            assert!(seen.insert(s), "duplicate canonical icon name `{s}`");
        }
    }

    /// [`all_icon_names`] is documented as alphabetical, and it is the
    /// index space behind [`icon_vector_asset`]'s asset cache — so a
    /// variant inserted out of order is a real (if subtle) bug.
    #[test]
    fn all_icon_names_is_alphabetical() {
        for pair in all_icon_names().windows(2) {
            assert!(
                pair[0] < pair[1],
                "all_icon_names() out of order: {} before {}",
                pair[0].name(),
                pair[1].name()
            );
        }
    }

    #[test]
    fn every_builtin_icon_has_gpu_strokes() {
        for name in all_icon_names() {
            assert!(
                !icon_strokes(*name).is_empty(),
                "{} has no GPU strokes",
                name.name()
            );
        }
    }

    #[test]
    fn every_builtin_icon_parses_as_svg_vector_asset() {
        for name in all_icon_names() {
            let asset = icon_vector_asset(*name);
            assert_eq!(asset.view_box, [0.0, 0.0, 24.0, 24.0]);
            assert!(
                !asset.paths.is_empty(),
                "{} has no vector paths",
                name.name()
            );
        }
    }
}
