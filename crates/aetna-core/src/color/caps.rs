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

/// A `wp_color_manager_v1` feature — the `supported_feature` events.
/// Mirrors the protocol's `feature` enum so callers reason about exactly
/// which requests the compositor accepts rather than a single
/// coarse "parametric or not" flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorFeature {
    /// `create_icc_creator` — ICC-profile image descriptions.
    IccV2V4,
    /// `create_parametric_creator` — primaries + TF image descriptions.
    /// The prerequisite for everything beyond implicit sRGB.
    Parametric,
    /// Parametric `set_primaries` — arbitrary CIE-xy primaries.
    SetPrimaries,
    /// Parametric `set_tf_power` — arbitrary power-curve exponent.
    SetTfPower,
    /// Parametric `set_luminances` — reference white + min/max luminance.
    /// Needed to declare HDR reference white (Stage 2).
    SetLuminances,
    /// Parametric `set_mastering_display_primaries`.
    SetMasteringDisplayPrimaries,
    /// Target color volume may exceed the primary color volume.
    ExtendedTargetVolume,
    /// `create_windows_scrgb` — the Windows scRGB convenience description.
    WindowsScrgb,
}

/// A `wp_color_manager_v1` render intent — the `supported_intent` events.
/// Mirrors the protocol's `render_intent` enum. Aetna always requests
/// `Perceptual` today; the rest are surfaced for inspection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderIntent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
    RelativeBpc,
    AbsoluteNoAdaptation,
}

/// Where the negotiation ended up on this host — what backends pass
/// to [`crate::HostDiagnostics`] so apps can inspect the wire state.
#[derive(Clone, Debug, Default)]
pub enum ColorManagementStatus {
    /// No color-management protocol available on this host (X11, plain
    /// Wayland without `wp_color_management_v1`, macOS / Windows today,
    /// Android, iOS, headless render bins). The surface goes out with
    /// the host's implicit interpretation, which is sRGB everywhere
    /// aetna runs.
    #[default]
    Unavailable,
    /// The host's color-management protocol is available. `capabilities`
    /// is what the host advertised; `attached` is the [`ColorSpace`]
    /// whose image description was attached to the surface, or `None`
    /// if the negotiator chose [`ColorSpace::SRGB`] and the host's
    /// implicit handling was used (no description attached).
    ///
    /// `targets` is what the compositor's *preferred* image description
    /// for this surface reports (reference white, display peak, preferred
    /// encoding) — read once at setup. All-`None` when the host exposes no
    /// feedback path; the negotiator treats that as "no HDR evidence".
    Available {
        capabilities: HostColorCapabilities,
        attached: Option<ColorSpace>,
        targets: CompositorColorTargets,
    },
}

/// What the compositor's *preferred* image description reports for a
/// surface — obtained via `wp_color_management_surface_feedback_v1`'s
/// `get_preferred` → `wp_image_description_v1.get_information` path.
///
/// Every field is optional: a host with no feedback path, an SDR-only
/// compositor, or an ICC-based preferred description leaves the relevant
/// fields `None`. The negotiator reads these as evidence — see
/// [`Self::indicates_hdr`] — and never *requires* them, so absence
/// degrades cleanly to SDR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompositorColorTargets {
    /// Reference white luminance the compositor wants surfaces to target
    /// on this output, in nits (cd/m²). On HDR setups this is the
    /// SDR-white-equivalent the user configured — the value HDR UI white
    /// should sit at so it matches surrounding SDR content.
    pub reference_luminance_nits: Option<f32>,
    /// Primary color volume max luminance (nits), if reported.
    pub max_luminance_nits: Option<f32>,
    /// Primary color volume min luminance (nits), if reported.
    pub min_luminance_nits: Option<f32>,
    /// The display's targeted peak luminance (nits) — the headroom
    /// available above reference white. The strongest single signal for
    /// "is this surface actually on an HDR output".
    pub target_max_luminance_nits: Option<f32>,
    /// The display's targeted minimum (black) luminance (nits), if reported.
    pub target_min_luminance_nits: Option<f32>,
    /// Targeted maximum content light level (`max_cll`, nits), if reported.
    pub max_content_light_level_nits: Option<f32>,
    /// Targeted maximum frame-average light level (`max_fall`, nits).
    pub max_frame_average_light_level_nits: Option<f32>,
    /// Transfer function of the preferred description. An HDR transfer
    /// (PQ / HLG) here is direct evidence the output is HDR.
    pub preferred_transfer: Option<TransferFunction>,
    /// Primaries of the preferred description.
    pub preferred_primaries: Option<Primaries>,
    /// True when the preferred description is ICC-based (an `icc_file`
    /// event arrived) rather than parametric — we can't introspect its
    /// primaries/transfer/luminances structurally.
    pub preferred_is_icc: bool,
}

impl CompositorColorTargets {
    /// Does the compositor's preferred encoding indicate a genuine HDR
    /// output? `true` when the preferred transfer is HDR (PQ / HLG), or
    /// the display's target peak sits meaningfully above the reference
    /// white (≥1.5×). Gates HDR output so we never emit bright PQ content
    /// into an environment we only *guessed* was HDR — when this is
    /// `false` (including the no-feedback all-`None` case) the negotiator
    /// stays on the SDR / wide-gamut-SDR path.
    pub fn indicates_hdr(&self) -> bool {
        if matches!(
            self.preferred_transfer,
            Some(TransferFunction::Pq | TransferFunction::Hlg)
        ) {
            return true;
        }
        match (
            self.target_max_luminance_nits,
            self.reference_luminance_nits,
        ) {
            (Some(peak), Some(reference)) => peak >= reference * 1.5,
            _ => false,
        }
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
    /// Features the compositor advertised (`supported_feature`).
    /// [`ColorFeature::Parametric`] is the one negotiation requires today;
    /// the rest gate finer requests (e.g. [`ColorFeature::SetLuminances`]
    /// for HDR reference-white declaration) and are surfaced for inspection.
    pub features: Vec<ColorFeature>,
    /// Render intents the compositor advertised (`supported_intent`).
    pub render_intents: Vec<RenderIntent>,
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
            features: vec![],
            render_intents: vec![],
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

    /// Does this host advertise the given feature?
    pub fn has_feature(&self, feature: ColorFeature) -> bool {
        self.features.contains(&feature)
    }

    /// Whether the compositor can build parametric image descriptions
    /// ([`ColorFeature::Parametric`]) — the prerequisite for any space
    /// beyond implicit sRGB. `false` means it can only forward whatever
    /// the buffer format implies; the negotiator treats that as "sRGB only".
    pub fn parametric_creator(&self) -> bool {
        self.has_feature(ColorFeature::Parametric)
    }

    /// Can the host deliver an arbitrary [`ColorSpace`] end-to-end?
    /// `SRGB` always returns `true` (universal baseline). Anything else
    /// requires the parametric creator + primaries + TF all to match.
    pub fn supports(&self, space: ColorSpace) -> bool {
        if space == ColorSpace::SRGB {
            return true;
        }
        self.parametric_creator()
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
            features: vec![ColorFeature::Parametric],
            render_intents: vec![],
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
            features: vec![],
            render_intents: vec![],
        };
        assert_eq!(
            ColorPreferences::hdr_broad().negotiate(&host),
            ColorSpace::SRGB,
        );
    }

    #[test]
    fn no_feedback_does_not_indicate_hdr() {
        // All-`None` (no usable feedback path) must read as SDR so we
        // never emit HDR into an environment we only guessed about.
        assert!(!CompositorColorTargets::default().indicates_hdr());
    }

    #[test]
    fn pq_preferred_transfer_indicates_hdr() {
        let t = CompositorColorTargets {
            preferred_transfer: Some(TransferFunction::Pq),
            ..Default::default()
        };
        assert!(t.indicates_hdr());
    }

    #[test]
    fn headroom_above_reference_indicates_hdr() {
        // SDR transfer, but the display peak sits well above reference
        // white → genuine HDR output.
        let t = CompositorColorTargets {
            preferred_transfer: Some(TransferFunction::Srgb),
            reference_luminance_nits: Some(203.0),
            target_max_luminance_nits: Some(1000.0),
            ..Default::default()
        };
        assert!(t.indicates_hdr());
    }

    #[test]
    fn sdr_peak_near_reference_is_not_hdr() {
        // A typical SDR display: peak ≈ reference white, no headroom.
        let t = CompositorColorTargets {
            preferred_transfer: Some(TransferFunction::Srgb),
            reference_luminance_nits: Some(203.0),
            target_max_luminance_nits: Some(240.0),
            ..Default::default()
        };
        assert!(!t.indicates_hdr());
    }

    #[test]
    fn default_preferences_are_sdr_only() {
        let caps = HostColorCapabilities {
            primaries: vec![Primaries::Srgb, Primaries::Bt2020],
            transfer_functions: vec![TransferFunction::Srgb, TransferFunction::Pq],
            features: vec![ColorFeature::Parametric],
            render_intents: vec![],
        };
        assert_eq!(
            ColorPreferences::default().negotiate(&caps),
            ColorSpace::SRGB,
        );
    }
}
