//! Built-in vector icons.
//!
//! The vocabulary intentionally mirrors common shadcn/lucide names.
//! Icons are semantic `El`s that emit vector draw ops for artifact/SVG
//! rendering and a text fallback in GPU backends until the dedicated
//! vector-icon pipeline lands.
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
const CHECK: &[IconStroke] = &[stroke(20.0, 6.0, 9.0, 17.0), stroke(9.0, 17.0, 4.0, 12.0)];
const CHEVRON_DOWN: &[IconStroke] = &[stroke(6.0, 9.0, 12.0, 15.0), stroke(12.0, 15.0, 18.0, 9.0)];
const CHEVRON_LEFT: &[IconStroke] = &[stroke(15.0, 6.0, 9.0, 12.0), stroke(9.0, 12.0, 15.0, 18.0)];
const CHEVRON_RIGHT: &[IconStroke] = &[stroke(9.0, 6.0, 15.0, 12.0), stroke(15.0, 12.0, 9.0, 18.0)];
const CHEVRON_UP: &[IconStroke] = &[stroke(6.0, 15.0, 12.0, 9.0), stroke(12.0, 9.0, 18.0, 15.0)];
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
const DOWNLOAD: &[IconStroke] = &[
    stroke(12.0, 3.0, 12.0, 15.0),
    stroke(7.0, 10.0, 12.0, 15.0),
    stroke(12.0, 15.0, 17.0, 10.0),
    stroke(5.0, 21.0, 19.0, 21.0),
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
const MENU: &[IconStroke] = &[
    stroke(4.0, 6.0, 20.0, 6.0),
    stroke(4.0, 12.0, 20.0, 12.0),
    stroke(4.0, 18.0, 20.0, 18.0),
];
const MORE_HORIZONTAL: &[IconStroke] = &[
    stroke(6.0, 12.0, 6.2, 12.0),
    stroke(12.0, 12.0, 12.2, 12.0),
    stroke(18.0, 12.0, 18.2, 12.0),
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
const X: &[IconStroke] = &[stroke(18.0, 6.0, 6.0, 18.0), stroke(6.0, 6.0, 18.0, 18.0)];

/// Flattened line strokes in the same 24x24 coordinate system as
/// [`icon_path`]. This is the first GPU-native icon vocabulary: it is
/// deliberately line-segment based so shader theming can own stroke
/// treatment without parsing SVG paths at frame time.
pub fn icon_strokes(name: IconName) -> &'static [IconStroke] {
    match name {
        IconName::Activity => ACTIVITY,
        IconName::AlertCircle => ALERT_CIRCLE,
        IconName::BarChart => BAR_CHART,
        IconName::Bell => BELL,
        IconName::Check => CHECK,
        IconName::ChevronDown => CHEVRON_DOWN,
        IconName::ChevronLeft => CHEVRON_LEFT,
        IconName::ChevronRight => CHEVRON_RIGHT,
        IconName::ChevronUp => CHEVRON_UP,
        IconName::Command => COMMAND,
        IconName::Download => DOWNLOAD,
        IconName::FileText => FILE_TEXT,
        IconName::Folder => FOLDER,
        IconName::GitBranch => GIT_BRANCH,
        IconName::GitCommit => GIT_COMMIT,
        IconName::Info => INFO,
        IconName::LayoutDashboard => LAYOUT_DASHBOARD,
        IconName::Menu => MENU,
        IconName::MoreHorizontal => MORE_HORIZONTAL,
        IconName::Plus => PLUS,
        IconName::RefreshCw => REFRESH_CW,
        IconName::Search => SEARCH,
        IconName::Settings => SETTINGS,
        IconName::Upload => UPLOAD,
        IconName::Users => USERS,
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
        IconName::BarChart,
        IconName::Bell,
        IconName::Check,
        IconName::ChevronDown,
        IconName::ChevronLeft,
        IconName::ChevronRight,
        IconName::ChevronUp,
        IconName::Command,
        IconName::Download,
        IconName::FileText,
        IconName::Folder,
        IconName::GitBranch,
        IconName::GitCommit,
        IconName::Info,
        IconName::LayoutDashboard,
        IconName::Menu,
        IconName::MoreHorizontal,
        IconName::Plus,
        IconName::RefreshCw,
        IconName::Search,
        IconName::Settings,
        IconName::Upload,
        IconName::Users,
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
        IconName::BarChart => r#"<path d="M4 20V10"/><path d="M12 20V4"/><path d="M20 20v-7"/>"#,
        IconName::Bell => {
            r#"<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9"/><path d="M10 21h4"/>"#
        }
        IconName::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
        IconName::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
        IconName::ChevronLeft => r#"<path d="m15 6-6 6 6 6"/>"#,
        IconName::ChevronRight => r#"<path d="m9 6 6 6-6 6"/>"#,
        IconName::ChevronUp => r#"<path d="m6 15 6-6 6 6"/>"#,
        IconName::Command => {
            r#"<path d="M9 9h6v6H9z"/><path d="M9 9H6a3 3 0 1 1 3-3v3Z"/><path d="M15 9V6a3 3 0 1 1 3 3h-3Z"/><path d="M15 15h3a3 3 0 1 1-3 3v-3Z"/><path d="M9 15v3a3 3 0 1 1-3-3h3Z"/>"#
        }
        IconName::Download => {
            r#"<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>"#
        }
        IconName::FileText => {
            r#"<path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/><path d="M8 12h8"/><path d="M8 16h6"/>"#
        }
        IconName::Folder => r#"<path d="M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        IconName::GitBranch => {
            r#"<circle cx="6" cy="5" r="2"/><circle cx="18" cy="19" r="2"/><circle cx="6" cy="19" r="2"/><path d="M6 7v10"/><path d="M8 5h4a6 6 0 0 1 6 6v6"/>"#
        }
        IconName::GitCommit => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M3 12h6"/><path d="M15 12h6"/>"#
        }
        IconName::Info => {
            r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7h.01"/>"#
        }
        IconName::LayoutDashboard => {
            r#"<rect x="3" y="3" width="7" height="8"/><rect x="14" y="3" width="7" height="5"/><rect x="14" y="12" width="7" height="9"/><rect x="3" y="15" width="7" height="6"/>"#
        }
        IconName::Menu => r#"<path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h16"/>"#,
        IconName::MoreHorizontal => {
            r#"<path d="M6 12h.01"/><path d="M12 12h.01"/><path d="M18 12h.01"/>"#
        }
        IconName::Plus => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        IconName::RefreshCw => {
            r#"<path d="M21 12a9 9 0 0 1-15.5 6.2"/><path d="M3 12A9 9 0 0 1 18.5 5.8"/><path d="M18 3v4h-4"/><path d="M6 21v-4h4"/>"#
        }
        IconName::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m16 16 5 5"/>"#,
        IconName::Settings => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.5-2.4 1a7 7 0 0 0-1.8-1L14.4 3h-4.8L9.3 6a7 7 0 0 0-1.8 1L5.1 6l-2 3.5 2 1.5a7 7 0 0 0 0 2l-2 1.5 2 3.5 2.4-1a7 7 0 0 0 1.8 1l.3 3h4.8l.3-3a7 7 0 0 0 1.8-1l2.4 1 2-3.5-2-1.5a7 7 0 0 0 .1-1Z"/>"#
        }
        IconName::Upload => r#"<path d="M12 21V9"/><path d="m7 14 5-5 5 5"/><path d="M5 3h14"/>"#,
        IconName::Users => {
            r#"<circle cx="9" cy="8" r="3"/><path d="M3 21a6 6 0 0 1 12 0"/><path d="M16 11a3 3 0 0 0 0-6"/><path d="M21 21a5 5 0 0 0-5-5"/>"#
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
