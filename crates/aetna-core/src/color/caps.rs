//! Host color capabilities + app color preferences + their negotiation.
//!
//! Aetna apps run on a wide range of hosts — bare X11, plain Wayland
//! compositors with no color management, modern Wayland compositors that
//! advertise `wp_color_management_v1`, and (in the future) macOS / Windows
//! / Android. Each host can deliver a different subset of [`ColorSpace`]s
//! to the wire. The app states a *preference order*; the host states what
//! it can *actually* support; this module picks the highest-preference
//! mutual match and never fails — [`ColorSpace::SRGB`] is the universal
//! baseline that every host can deliver.
//!
//! The vocabulary mirrors `wp_color_management_v1`'s
//! `supported_primaries_named` / `supported_tf_named` / `supported_feature`
//! events, but the negotiation itself is host-agnostic.

use super::space::{ColorSpace, Primaries, TransferFunction};

/// Where the negotiation ended up on this host — what backends pass
/// to [`crate::HostDiagnostics`] so apps can inspect the wire state.
#[derive(Clone, Debug)]
pub enum ColorManagementStatus {
    /// No color-management protocol available on this host (X11, plain
    /// Wayland without `wp_color_management_v1`, macOS / Windows today,
    /// Android, iOS, headless render bins). The surface goes out with
    /// the host's implicit interpretation, which is sRGB everywhere
    /// aetna runs.
    Unavailable,
    /// The host's color-management protocol is available. `capabilities`
    /// is what the host advertised; `attached` is the [`ColorSpace`]
    /// whose image description was attached to the surface, or `None`
    /// if the negotiator chose [`ColorSpace::SRGB`] and the host's
    /// implicit handling was used (no description attached).
    Available {
        capabilities: HostColorCapabilities,
        attached: Option<ColorSpace>,
    },
}

impl Default for ColorManagementStatus {
    fn default() -> Self {
        Self::Unavailable
    }
}

/// What the host can advertise upstream — the intersection of "what the
/// compositor told us it supports" and "what shapes the renderer knows
/// how to drive."
///
/// Every host implicitly supports [`ColorSpace::SRGB`] regardless of what
/// is listed here; that fallback is enforced by [`ColorPreferences::negotiate`].
#[derive(Clone, Debug, Default)]
pub struct HostColorCapabilities {
    /// Named primaries the compositor accepts (`wp_color_manager_v1`
    /// `supported_primaries_named` events).
    pub primaries: Vec<Primaries>,
    /// Named transfer functions the compositor accepts (`supported_tf_named`).
    /// `GammaExponent` values are matched by `(primaries, gamma×100)`
    /// equality, so callers populating this list should use the same
    /// `GammaExponent` constants the app's preferences will reference.
    pub transfer_functions: Vec<TransferFunction>,
    /// Whether the compositor exposed the `parametric` feature — required
    /// to build an image description from primaries + TF rather than ICC.
    /// `false` here means the host can only forward whatever the buffer
    /// format implies; the negotiator treats this as "sRGB only".
    pub parametric_creator: bool,
}

impl HostColorCapabilities {
    /// The implicit baseline — the only space a no-color-management host
    /// can be trusted to display correctly is sRGB. Returned when the
    /// host has no color manager at all (X11, older compositors, macOS
    /// today, etc.).
    pub fn srgb_only() -> Self {
        Self {
            primaries: vec![Primaries::Srgb],
            transfer_functions: vec![TransferFunction::Srgb],
            parametric_creator: false,
        }
    }

    /// Does this host advertise the named primaries?
    pub fn supports_primaries(&self, p: Primaries) -> bool {
        self.primaries.contains(&p)
    }

    /// Does this host advertise the named transfer function?
    pub fn supports_transfer(&self, tf: TransferFunction) -> bool {
        self.transfer_functions.contains(&tf)
    }

    /// Can the host deliver an arbitrary [`ColorSpace`] end-to-end?
    /// `SRGB` always returns `true` (universal baseline). Anything else
    /// requires the parametric creator + primaries + TF all to match.
    pub fn supports(&self, space: ColorSpace) -> bool {
        if space == ColorSpace::SRGB {
            return true;
        }
        self.parametric_creator
            && self.supports_primaries(space.primaries)
            && self.supports_transfer(space.transfer)
    }
}

/// What the app *wants* the renderer working space to be, in order of
/// preference. The negotiator walks the list and returns the first space
/// the host can deliver, falling back to [`ColorSpace::SRGB`] if none
/// match.
///
/// The list is *additive*: there is no need to include `SRGB` at the end
/// — it is always the final fallback. Listing it earlier short-circuits
/// to sRGB even on capable hosts.
#[derive(Clone, Debug)]
pub struct ColorPreferences {
    pub working_spaces: Vec<ColorSpace>,
}

impl Default for ColorPreferences {
    fn default() -> Self {
        Self::sdr_only()
    }
}

impl ColorPreferences {
    /// Explicit constructor — equivalent to `ColorPreferences { working_spaces: list }`.
    pub fn new(list: Vec<ColorSpace>) -> Self {
        Self {
            working_spaces: list,
        }
    }

    /// `[SRGB]` — the conservative baseline. Identical pixels on every
    /// host. The default for [`ColorPreferences`].
    pub fn sdr_only() -> Self {
        Self {
            working_spaces: vec![ColorSpace::SRGB],
        }
    }

    /// `[DISPLAY_P3, SRGB]` — opt into wider primaries on capable hosts
    /// without changing transfer characteristic semantics.
    pub fn wide_gamut() -> Self {
        Self {
            working_spaces: vec![ColorSpace::DISPLAY_P3, ColorSpace::SRGB],
        }
    }

    /// `[SCRGB_LINEAR, DISPLAY_P3_LINEAR, DISPLAY_P3, SRGB]` — extended-
    /// range linear HDR when available, gracefully degrading.
    pub fn hdr_extended() -> Self {
        Self {
            working_spaces: vec![
                ColorSpace::SCRGB_LINEAR,
                ColorSpace::DISPLAY_P3_LINEAR,
                ColorSpace::DISPLAY_P3,
                ColorSpace::SRGB,
            ],
        }
    }

    /// `[BT2020_PQ, BT2020_LINEAR, SCRGB_LINEAR, DISPLAY_P3_LINEAR,
    /// DISPLAY_P3, SRGB]` — broadest HDR ladder.
    ///
    /// PQ leads for HDR10-style output, but it requires the backend to
    /// encode linear → PQ before submit; backends that can't perform that
    /// pass skip it (the `aetna-wgpu` host does, today). `BT2020_LINEAR`
    /// sits right behind so a BT.2020-capable compositor still gets the
    /// wide gamut via an extended-range float surface, then the ladder
    /// degrades through scRGB-linear / Display-P3 down to sRGB.
    pub fn hdr_broad() -> Self {
        Self {
            working_spaces: vec![
                ColorSpace::BT2020_PQ,
                ColorSpace::BT2020_LINEAR,
                ColorSpace::SCRGB_LINEAR,
                ColorSpace::DISPLAY_P3_LINEAR,
                ColorSpace::DISPLAY_P3,
                ColorSpace::SRGB,
            ],
        }
    }

    /// Pick the highest-preference space the host can deliver. Always
    /// returns *something* — [`ColorSpace::SRGB`] is the universal
    /// fallback, since any host that can render at all can render sRGB.
    pub fn negotiate(&self, caps: &HostColorCapabilities) -> ColorSpace {
        for s in &self.working_spaces {
            if caps.supports(*s) {
                return *s;
            }
        }
        ColorSpace::SRGB
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_only_host_always_negotiates_srgb() {
        let host = HostColorCapabilities::srgb_only();
        assert_eq!(
            ColorPreferences::hdr_broad().negotiate(&host),
            ColorSpace::SRGB,
        );
    }

    #[test]
    fn negotiation_picks_highest_match() {
        let host = HostColorCapabilities {
            primaries: vec![Primaries::Srgb, Primaries::DisplayP3],
            transfer_functions: vec![TransferFunction::Srgb, TransferFunction::Linear],
            parametric_creator: true,
        };
        // hdr_extended is [scRGB-linear, DP3-linear, DP3, sRGB]
        // host has DP3 + linear -> DISPLAY_P3_LINEAR wins (scRGB-linear
        // also matches — sRGB primaries + linear — and is listed first).
        assert_eq!(
            ColorPreferences::hdr_extended().negotiate(&host),
            ColorSpace::SCRGB_LINEAR,
        );
    }

    #[test]
    fn missing_parametric_creator_blocks_non_srgb() {
        let host = HostColorCapabilities {
            primaries: vec![Primaries::Srgb, Primaries::DisplayP3, Primaries::Bt2020],
            transfer_functions: vec![
                TransferFunction::Srgb,
                TransferFunction::Linear,
                TransferFunction::Pq,
            ],
            parametric_creator: false,
        };
        assert_eq!(
            ColorPreferences::hdr_broad().negotiate(&host),
            ColorSpace::SRGB,
        );
    }

    #[test]
    fn default_preferences_are_sdr_only() {
        let caps = HostColorCapabilities {
            primaries: vec![Primaries::Srgb, Primaries::Bt2020],
            transfer_functions: vec![TransferFunction::Srgb, TransferFunction::Pq],
            parametric_creator: true,
        };
        assert_eq!(
            ColorPreferences::default().negotiate(&caps),
            ColorSpace::SRGB,
        );
    }
}
