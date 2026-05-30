//! App-supplied SVG icons.
//!
//! Apps parse an SVG once (typically as a `LazyLock` over an
//! `include_str!` payload) and pass the resulting [`SvgIcon`] to any
//! API that accepts a built-in [`IconName`]:
//!
//! ```
//! use std::sync::LazyLock;
//! use damascene_core::prelude::*;
//! use damascene_core::SvgIcon;
//!
//! // Real apps usually do `include_str!("path/to/logo.svg")`. Inlined
//! // here so the doctest compiles without a fixture file.
//! const LOGO_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/></svg>"##;
//!
//! static MY_LOGO: LazyLock<SvgIcon> = LazyLock::new(|| {
//!     SvgIcon::parse_current_color(LOGO_SVG).unwrap()
//! });
//!
//! fn header() -> El {
//!     icon(MY_LOGO.clone()).icon_size(24.0)
//! }
//! ```
//!
//! Identity is content-hashed: two `SvgIcon`s parsed from the same
//! source and paint mode share backend cache entries. Cloning is a cheap
//! `Arc` bump.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::tree::IconName;
use crate::vector::{
    VectorAsset, VectorParseError, parse_current_color_svg_asset, parse_svg_asset,
};

/// An SVG icon supplied by the application.
///
/// Construct with [`Self::parse`] (paint preserved as authored) or
/// [`Self::parse_current_color`] (paint replaced with `currentColor`,
/// so the element's `text_color` tints the icon and `stroke_width`
/// modulates strokes — matches the lucide-style monochrome icons in
/// the built-in set).
#[derive(Clone)]
pub struct SvgIcon {
    inner: Arc<SvgIconInner>,
}

struct SvgIconInner {
    asset: VectorAsset,
    content_hash: u64,
    paint_mode: SvgIconPaintMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgIconPaintMode {
    Authored,
    CurrentColorMask,
}

impl SvgIcon {
    /// Parse an SVG, preserving every fill and stroke as authored. The
    /// element's `text_color` and `stroke_width` settings do not affect
    /// this icon. Use this for full-color art (logos, illustrations).
    pub fn parse(svg: &str) -> Result<Self, VectorParseError> {
        let asset = parse_svg_asset(svg)?;
        Ok(Self::from_asset(
            asset,
            hash_svg(svg, false),
            SvgIconPaintMode::Authored,
        ))
    }

    /// Parse an SVG and treat every fill/stroke as `currentColor`. The
    /// element's `text_color` tints the icon and `stroke_width`
    /// modulates strokes — matches how the built-in lucide icons work.
    pub fn parse_current_color(svg: &str) -> Result<Self, VectorParseError> {
        let asset = parse_current_color_svg_asset(svg)?;
        Ok(Self::from_asset(
            asset,
            hash_svg(svg, true),
            SvgIconPaintMode::CurrentColorMask,
        ))
    }

    fn from_asset(asset: VectorAsset, content_hash: u64, paint_mode: SvgIconPaintMode) -> Self {
        Self {
            inner: Arc::new(SvgIconInner {
                asset,
                content_hash,
                paint_mode,
            }),
        }
    }

    /// Parsed IR — same shape the built-in icons use, so any backend
    /// that can render an [`IconName`] can render this.
    pub fn vector_asset(&self) -> &VectorAsset {
        &self.inner.asset
    }

    /// Stable per-process identity. Two `SvgIcon`s parsed from the
    /// same input and paint mode share this value, so backend caches
    /// dedup them automatically.
    pub fn content_hash(&self) -> u64 {
        self.inner.content_hash
    }

    pub fn paint_mode(&self) -> SvgIconPaintMode {
        self.inner.paint_mode
    }
}

impl PartialEq for SvgIcon {
    fn eq(&self, other: &Self) -> bool {
        self.inner.content_hash == other.inner.content_hash
    }
}

impl Eq for SvgIcon {}

impl std::fmt::Debug for SvgIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvgIcon")
            .field(
                "content_hash",
                &format_args!("{:016x}", self.inner.content_hash),
            )
            .field("paths", &self.inner.asset.paths.len())
            .field("paint_mode", &self.inner.paint_mode)
            .finish()
    }
}

/// Source for an icon draw — either a built-in [`IconName`] or an
/// app-supplied [`SvgIcon`]. APIs accept this via [`IntoIconSource`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSource {
    Builtin(IconName),
    Custom(SvgIcon),
}

impl IconSource {
    /// The vector IR for this icon — built-ins are looked up in the
    /// process-wide static cache; custom icons hold their own.
    pub fn vector_asset(&self) -> &VectorAsset {
        match self {
            IconSource::Builtin(name) => crate::icons::icon_vector_asset(*name),
            IconSource::Custom(svg) => svg.vector_asset(),
        }
    }

    pub fn paint_mode(&self) -> SvgIconPaintMode {
        match self {
            IconSource::Builtin(_) => SvgIconPaintMode::CurrentColorMask,
            IconSource::Custom(svg) => svg.paint_mode(),
        }
    }

    /// Short human-readable label, useful for inspection/dump output.
    /// Built-ins use their `kebab-case` name; custom icons report
    /// `"custom:<short hash>"`.
    pub fn label(&self) -> String {
        match self {
            IconSource::Builtin(name) => name.name().to_string(),
            IconSource::Custom(svg) => format!("custom:{:08x}", svg.content_hash() as u32),
        }
    }
}

impl From<IconName> for IconSource {
    fn from(name: IconName) -> Self {
        IconSource::Builtin(name)
    }
}

impl From<SvgIcon> for IconSource {
    fn from(svg: SvgIcon) -> Self {
        IconSource::Custom(svg)
    }
}

/// Conversion into an [`IconSource`]. Implemented for [`IconName`],
/// [`SvgIcon`], and string types (resolved against the built-in
/// vocabulary, with an `AlertCircle` fallback for unknown names).
pub trait IntoIconSource {
    fn into_icon_source(self) -> IconSource;
}

impl IntoIconSource for IconSource {
    fn into_icon_source(self) -> IconSource {
        self
    }
}

impl IntoIconSource for IconName {
    fn into_icon_source(self) -> IconSource {
        IconSource::Builtin(self)
    }
}

impl IntoIconSource for SvgIcon {
    fn into_icon_source(self) -> IconSource {
        IconSource::Custom(self)
    }
}

impl IntoIconSource for &SvgIcon {
    fn into_icon_source(self) -> IconSource {
        IconSource::Custom(self.clone())
    }
}

impl IntoIconSource for &str {
    fn into_icon_source(self) -> IconSource {
        IconSource::Builtin(crate::icons::name_or_fallback(self))
    }
}

impl IntoIconSource for String {
    fn into_icon_source(self) -> IconSource {
        IconSource::Builtin(crate::icons::name_or_fallback(&self))
    }
}

fn hash_svg(svg: &str, current_color: bool) -> u64 {
    let mut h = DefaultHasher::new();
    (current_color as u8).hash(&mut h);
    svg.as_bytes().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="#ff0000"/></svg>"##;
    const BLUE_CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="#0000ff"/></svg>"##;

    #[test]
    fn parse_extracts_view_box_and_paths() {
        let icon = SvgIcon::parse(RED_CIRCLE).unwrap();
        assert_eq!(icon.vector_asset().view_box, [0.0, 0.0, 24.0, 24.0]);
        assert!(!icon.vector_asset().paths.is_empty());
    }

    #[test]
    fn same_source_dedups_to_same_hash() {
        let a = SvgIcon::parse(RED_CIRCLE).unwrap();
        let b = SvgIcon::parse(RED_CIRCLE).unwrap();
        assert_eq!(a.content_hash(), b.content_hash());
        assert_eq!(a, b);
    }

    #[test]
    fn different_sources_have_different_hashes() {
        let a = SvgIcon::parse(RED_CIRCLE).unwrap();
        let b = SvgIcon::parse(BLUE_CIRCLE).unwrap();
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn parse_mode_is_part_of_identity() {
        // Same bytes, two parse modes → two distinct atlas keys.
        let a = SvgIcon::parse(RED_CIRCLE).unwrap();
        let b = SvgIcon::parse_current_color(RED_CIRCLE).unwrap();
        assert_ne!(a.content_hash(), b.content_hash());
        assert_eq!(a.paint_mode(), SvgIconPaintMode::Authored);
        assert_eq!(b.paint_mode(), SvgIconPaintMode::CurrentColorMask);
        assert_eq!(
            IconSource::Builtin(IconName::Settings).paint_mode(),
            SvgIconPaintMode::CurrentColorMask
        );
    }

    #[test]
    fn malformed_svg_returns_error() {
        let err = SvgIcon::parse("<not-svg/>");
        assert!(err.is_err(), "expected parse error, got {err:?}");
    }

    #[test]
    fn into_icon_source_for_iconname() {
        assert_eq!(
            IconName::Settings.into_icon_source(),
            IconSource::Builtin(IconName::Settings)
        );
    }

    #[test]
    fn into_icon_source_for_str_uses_builtin_vocab() {
        assert_eq!(
            "settings".into_icon_source(),
            IconSource::Builtin(IconName::Settings)
        );
    }

    #[test]
    fn into_icon_source_for_unknown_str_falls_back() {
        assert_eq!(
            "not-a-real-icon".into_icon_source(),
            IconSource::Builtin(IconName::AlertCircle)
        );
    }

    #[test]
    fn into_icon_source_for_svg_icon() {
        let svg = SvgIcon::parse(RED_CIRCLE).unwrap();
        match svg.clone().into_icon_source() {
            IconSource::Custom(c) => assert_eq!(c, svg),
            other => panic!("expected Custom, got {other:?}"),
        }
    }
}
