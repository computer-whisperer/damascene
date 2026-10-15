//! Built-in vector icons: the whole [Lucide](https://lucide.dev/) set.
//!
//! [`IconName`] is Lucide at a pinned tag, wholesale — 1,890 names and
//! their markup verbatim (ISC; see `CREDITS.md`), generated into
//! [`lucide`] by `tools/src/bin/gen_lucide_icons.rs` and maintained by
//! re-running it rather than by hand. Ruled 2026-10-14
//! (`docs/POLISH_REVIEW_2026_10.md`): the hand-picked set of 59 was the
//! single largest fidelity ceiling every validation round named, and
//! without icons every dense action set degrades into a wall of
//! labelled buttons.
//!
//! Icons are semantic `El`s that emit vector draw ops for artifact/SVG
//! rendering and MSDF-rasterised glyphs on the GPU backends; both come
//! from the same markup, parsed once per icon on first use.
//!
//! Sibling modules:
//! - [`lucide`] — the generated vocabulary (`IconName`, names, markup,
//!   aliases).
//! - [`svg`] — `IconSource`, `SvgIcon`, custom user-supplied icon assets.
//! - [`msdf`] — MSDF (multi-channel signed distance field) generation.
//! - [`msdf_atlas`] — atlas packing of MSDF tiles for GPU consumption.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

pub mod lucide;
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
/// One stderr line per unknown name, not one per frame: a misspelled
/// icon in a tree rebuilt every frame used to scroll the log.
fn warn_unknown_once(name: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen
        .get_or_insert_with(HashSet::new)
        .insert(name.to_string())
    {
        eprintln!(
            "damascene: unknown icon name `{name}` — rendering AlertCircle. \
             See `damascene_core::all_icon_names()` for the available vocabulary."
        );
    }
}

pub(crate) fn name_to_source(name: &str) -> svg::IconSource {
    match IconName::parse(name) {
        Some(found) => svg::IconSource::Builtin(found),
        None => {
            warn_unknown_once(name);
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
        .default_icon_size(tokens::ICON_SM)
        .icon_stroke_width(2.0)
        .text_color(tokens::FOREGROUND)
}

impl IconName {
    /// A single-character text stand-in for the icon, used where the
    /// vector glyph cannot be drawn (text-only rendering paths and
    /// experimental backends). The common navigation and status glyphs
    /// have a recognisable character; the rest of the set shares one
    /// placeholder, since a hand table over 1,890 icons would be rot
    /// the moment the generator moved the tag.
    pub fn fallback_glyph(self) -> &'static str {
        match self {
            Self::ArrowDown | Self::Download => "↓",
            Self::ArrowLeft => "←",
            Self::ArrowRight => "→",
            Self::ArrowUp | Self::Upload => "↑",
            Self::Check => "✓",
            Self::ChevronDown => "⌄",
            Self::ChevronLeft => "‹",
            Self::ChevronRight => "›",
            Self::ChevronUp => "⌃",
            Self::CircleAlert | Self::TriangleAlert => "!",
            Self::Command => "⌘",
            Self::Ellipsis => "…",
            Self::EllipsisVertical => "⋮",
            Self::ExternalLink => "↗",
            Self::Info => "i",
            Self::Menu => "≡",
            Self::Minus => "−",
            Self::Plus => "+",
            Self::Search => "⌕",
            Self::Settings => "⚙",
            Self::X => "×",
            _ => "◌",
        }
    }
}

/// Parsed vector IR for a built-in icon (24×24 view box, strokes as
/// `currentColor`). Built lazily, once per icon on first use, from its
/// [`icon_path`] markup, and cached for the process.
pub fn icon_vector_asset(name: IconName) -> &'static VectorAsset {
    static ASSETS: [OnceLock<VectorAsset>; IconName::COUNT] =
        [const { OnceLock::new() }; IconName::COUNT];
    ASSETS[name as usize].get_or_init(|| {
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"##,
            icon_path(name)
        );
        parse_current_color_svg_asset(&svg)
            .unwrap_or_else(|err| panic!("failed to parse built-in icon {}: {err}", name.name()))
    })
}

/// Every built-in [`IconName`] — the full Lucide vocabulary, in
/// alphabetical order. String-typed `icon("...")` names resolve
/// against this set (plus Lucide's deprecated aliases and the house
/// short forms; see [`IconName::parse`]).
pub fn all_icon_names() -> &'static [IconName] {
    IconName::ALL
}

/// SVG markup for a built-in icon in Lucide's 24×24 view box — the
/// upstream file's elements verbatim, without the `<svg>` wrapper.
/// Paint (`currentColor`, stroke width) comes from the rendering
/// context, which is what lets one markup serve both the SVG artifact
/// and the GPU glyph.
pub fn icon_path(name: IconName) -> &'static str {
    name.markup()
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

    /// Lucide renames glyphs and keeps the old name as a deprecated
    /// alias; both the string and the Rust side follow. `alert-circle`
    /// became `circle-alert` upstream, so `IconName::AlertCircle` (a
    /// generated alias constant) is the live `CircleAlert`, and the
    /// fallback glyph for unknown names is that one icon under either
    /// name.
    #[test]
    fn upstream_aliases_resolve_on_both_sides() {
        assert_eq!(IconName::parse("alert-circle"), Some(IconName::CircleAlert));
        assert_eq!(IconName::AlertCircle, IconName::CircleAlert);
        assert_eq!(IconName::parse("more-horizontal"), Some(IconName::Ellipsis));
        assert_eq!(IconName::MoreHorizontal, IconName::Ellipsis);
        assert_eq!(IconName::CircleAlert.name(), "circle-alert");
    }

    /// The tables are indexed by discriminant: the enum, `ALL`, `NAMES`
    /// and `MARKUP` are all emitted in one alphabetical order.
    #[test]
    fn discriminants_index_the_generated_tables() {
        assert_eq!(IconName::ALL.len(), IconName::COUNT);
        for (i, name) in IconName::ALL.iter().enumerate() {
            assert_eq!(*name as usize, i, "{} out of place", name.name());
        }
        const {
            assert!(
                IconName::COUNT > 1500,
                "the whole set, not a hand-picked one"
            )
        };
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
    /// is unique — the generated tables agree with each other.
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
