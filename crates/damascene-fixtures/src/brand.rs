//! Raster embeds of the project logo (`assets/damascene_badge_icon.svg`).
//!
//! These exist because the vector icon pipeline ignores SVG
//! `clip-path` (issue #150) and the badge clips its gold wave inlay to
//! the letterform with one — `SvgIcon::parse` paints the waves
//! unclipped. Until that closes, the fixtures embed pre-rasterized
//! pixels (straight-alpha sRGB, written by `tools/regen_logo_rasters.sh`)
//! at the two sizes the fixtures draw: 96px covers the hero's
//! 32-logical-px slot at up to 3x DPI, 192px the About page's 64.

use std::sync::LazyLock;

use damascene_core::prelude::*;

/// Badge logo at 96x96, for slots up to 32 logical pixels.
pub(crate) static LOGO_96: LazyLock<Image> = LazyLock::new(|| {
    Image::from_rgba8(
        96,
        96,
        include_bytes!("../assets/badge_icon_96.rgba").to_vec(),
    )
});

/// Badge logo at 192x192, for slots up to 64 logical pixels.
pub(crate) static LOGO_192: LazyLock<Image> = LazyLock::new(|| {
    Image::from_rgba8(
        192,
        192,
        include_bytes!("../assets/badge_icon_192.rgba").to_vec(),
    )
});
