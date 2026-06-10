//! The host's color-negotiation stack, exposed for custom hosts.
//!
//! Everything here is per-surface — nothing assumes one window per
//! process. A custom multi-window host creates one [`SurfaceColor`]
//! per window and polls each one from its event loop.
//!
//! Three layers, from most to least packaged:
//!
//! - [`SurfaceColor`] — the per-window driver. Owns the live
//!   `wp_color_management_v1` client and the negotiated state;
//!   [`SurfaceColor::poll`] surfaces live re-negotiations
//!   (`preferred_changed(2)`: output moves, HDR toggles) as a
//!   [`Renegotiation`] plan the host applies. This is what the
//!   built-in run loop uses.
//! - [`negotiate_color`] / [`ColorSetup`] — one-shot startup
//!   negotiation against a winit `Window` (wayland builds only).
//! - The pure pieces: [`negotiate_output`] (the preference-ladder
//!   walk), [`deliver_space`], [`output_luminance`], the format
//!   pickers [`srgb_format`] / [`wide_format`], and the diagnostics
//!   builders [`build_surface_color_info`] /
//!   [`classify_surface_format`].
//!
//! The negotiation rationale — why the WSI owns the surface's color
//! tag, why we never attach a description, how reference white is
//! anchored — lives in `docs/COLOR_MANAGEMENT.md`.

use damascene_core::color::{
    ColorManagementStatus, ColorPreferences, ColorSpace, CompositorColorTargets,
    HostColorCapabilities,
};
use winit::window::Window;

#[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
pub use crate::wayland_color::WaylandColorManager;

/// Conservative sRGB swapchain format — the universal fallback.
///
/// `formats` is the surface's advertised list
/// (`wgpu::SurfaceCapabilities::formats`), as for every helper here
/// that picks formats.
pub fn srgb_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(formats[0])
}

/// Extended-range float swapchain format for HDR output, if the surface
/// offers it.
///
/// `Rgba16Float` is the one format wgpu's Vulkan backend pairs with
/// `VK_COLOR_SPACE_EXTENDED_SRGB_LINEAR_EXT` (scRGB) — see
/// `wgpu-hal/src/vulkan/{conv.rs,swapchain/native.rs}`. Configuring the
/// surface with it yields a linear, extended-range swapchain that the WSI
/// tags and the compositor encodes; our linear working-space values go out
/// verbatim, with SDR content in `[0,1]` unchanged and `>1.0` emitting HDR.
/// The WSI still owns the surface's color tag — we attach nothing.
///
/// `None` when the surface doesn't advertise it: an SDR output, a
/// compositor without `extended_target_volume`, or no color management at
/// all. Callers fall back to [`srgb_format`].
pub fn wide_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|f| *f == wgpu::TextureFormat::Rgba16Float)
}

/// Walk the app's color-space preference ladder and return the first
/// `(swapchain format, renderer working space)` the host can actually
/// deliver — the intersection of three sets: the app's *preferences* (the
/// ladder), the *compositor's capabilities* (`caps.supports`), and *what
/// the wgpu swapchain can carry* ([`deliver_space`]). Falls back to the
/// 8-bit sRGB baseline, which any host can present.
///
/// This is the constrained form of
/// [`damascene_core::color::ColorPreferences::negotiate`]: that method
/// intersects only the first two sets and would over-promise, since a
/// compositor may advertise PQ / BT.2020 while the wgpu swapchain can build
/// only scRGB or sRGB. See docs/COLOR_MANAGEMENT.md.
pub fn negotiate_output(
    preferences: &ColorPreferences,
    caps: &HostColorCapabilities,
    formats: &[wgpu::TextureFormat],
    targets: &CompositorColorTargets,
) -> (wgpu::TextureFormat, ColorSpace) {
    for &space in &preferences.working_spaces {
        if caps.supports(space) {
            if let Some(delivered) = deliver_space(space, formats, targets) {
                return delivered;
            }
        }
    }
    (srgb_format(formats), ColorSpace::SRGB_LINEAR)
}

/// Map an agreed output color space to a concrete wgpu swapchain format +
/// renderer working space, or `None` when the wgpu swapchain can't carry
/// it. The working space is always linear; the swapchain format is what
/// carries the encoding + dynamic range to the WSI.
pub fn deliver_space(
    space: ColorSpace,
    formats: &[wgpu::TextureFormat],
    targets: &CompositorColorTargets,
) -> Option<(wgpu::TextureFormat, ColorSpace)> {
    use damascene_core::color::{Primaries, TransferFunction};
    match (space.primaries, space.transfer) {
        // Plain sRGB: an 8-bit sRGB-encoded swapchain; the GPU does the
        // linear → sRGB encode on store. Always available.
        (Primaries::Srgb, TransferFunction::Srgb) => {
            Some((srgb_format(formats), ColorSpace::SRGB_LINEAR))
        }
        // scRGB (== SRGB_LINEAR): linear sRGB primaries, extended range.
        // wgpu carries this as an `Rgba16Float` swapchain tagged
        // `EXTENDED_SRGB_LINEAR_EXT`. Deliverable only on a genuinely HDR
        // output that offers the float format — on SDR we fall through to
        // the cheaper 8-bit baseline (the extended range would only clamp).
        (Primaries::Srgb, TransferFunction::Linear) => {
            if targets.indicates_hdr() {
                wide_format(formats).map(|f| (f, ColorSpace::SRGB_LINEAR))
            } else {
                None
            }
        }
        // Wider gamut (Display-P3, BT.2020) or HDR transfers (PQ / HLG): the
        // wgpu Vulkan backend maps only the scRGB pair, so its swapchain
        // can't carry these. Skipped — see docs/COLOR_MANAGEMENT.md.
        _ => None,
    }
}

/// Derive the renderer's output luminance frame — `(headroom,
/// reference_nits)` for `Runner::set_output_luminance` — from the
/// compositor's preferred targets and the negotiated swapchain format.
///
/// Headroom is the usable range above reference white, in multiples of
/// it. On an 8-bit swapchain it is 1.0 regardless of the panel (the
/// encoding clips at reference, so HDR images tonemap down to SDR
/// rather than hard-clipping). On scRGB it is `target_max / reference`;
/// when the output declares no maximum there is nothing to remaster
/// against, so it is unbounded and image content passes through
/// unchanged (the compositor's own mapping is the only backstop —
/// matches the pre-remaster behavior).
pub fn output_luminance(
    targets: &CompositorColorTargets,
    format: wgpu::TextureFormat,
) -> (f32, f32) {
    let reference = targets
        .reference_luminance_nits
        .filter(|&r| r > 0.0)
        .unwrap_or(damascene_core::color::BT2408_REFERENCE_WHITE_NITS);
    if format != wgpu::TextureFormat::Rgba16Float {
        return (1.0, reference);
    }
    let headroom = match targets.target_max_luminance_nits {
        Some(max) if max > 0.0 => (max / reference).max(1.0),
        _ => f32::INFINITY,
    };
    (headroom, reference)
}

/// Summarize the wgpu/WSI side of color negotiation for
/// [`HostDiagnostics::surface_color`](damascene_core::HostDiagnostics::surface_color)
/// — what the swapchain can represent, which is half of what the
/// negotiator can pick (the compositor caps are the other half).
pub fn build_surface_color_info(
    adapter: &wgpu::Adapter,
    surface_caps: &wgpu::SurfaceCapabilities,
    chosen_format: wgpu::TextureFormat,
    present_mode: wgpu::PresentMode,
    alpha_mode: wgpu::CompositeAlphaMode,
) -> damascene_core::SurfaceColorInfo {
    let info = adapter.get_info();
    let driver = match (info.driver.is_empty(), info.driver_info.is_empty()) {
        (false, false) => format!("{} ({})", info.driver, info.driver_info),
        (false, true) => info.driver.clone(),
        (true, false) => info.driver_info.clone(),
        (true, true) => String::new(),
    };
    damascene_core::SurfaceColorInfo {
        adapter: info.name,
        driver,
        formats: surface_caps
            .formats
            .iter()
            .map(|f| classify_surface_format(*f))
            .collect(),
        chosen_format: format!("{chosen_format:?}"),
        present_mode: format!("{present_mode:?}"),
        alpha_mode: format!("{alpha_mode:?}"),
    }
}

/// Classify one surface format by how it can carry color output.
pub fn classify_surface_format(f: wgpu::TextureFormat) -> damascene_core::SurfaceFormatInfo {
    use wgpu::TextureFormat::{Rgb10a2Unorm, Rgba16Float, Rgba32Float};
    damascene_core::SurfaceFormatInfo {
        name: format!("{f:?}"),
        srgb: f.is_srgb(),
        // Float (linear-direct — the compositor encodes) or ≥10-bit (a
        // PQ-encode target) can carry wide-gamut / HDR; 8-bit unorm is
        // SDR-only.
        wide: matches!(f, Rgba16Float | Rgba32Float | Rgb10a2Unorm),
    }
}

/// Color setup for a freshly-created surface. We consult
/// `wp_color_management_v1` for the compositor's capabilities and its
/// preferred image description (for the Color Management showcase /
/// `HostDiagnostics`), but we do **not** attach our own description.
///
/// Per the protocol a `wl_surface` has exactly one color-management owner,
/// and for an accelerated client that owner is the WSI (Mesa), which tags
/// the swapchain. A second `get_surface` raises a connection-fatal
/// `surface_exists` error on the libwayland connection we share with
/// winit/Mesa, crashing the app (seen on KDE with HDR enabled) — so we
/// never attach. We *do* steer the WSI the compliant way: on a genuinely
/// HDR output we select an `Rgba16Float` swapchain, which wgpu's Vulkan
/// backend pairs with scRGB (`EXTENDED_SRGB_LINEAR_EXT`), letting `>1.0`
/// reach the display. SDR outputs stay on the 8-bit sRGB baseline. See
/// [`wide_format`] for the format mechanism and the color roadmap.
///
/// Linux + `wayland-color-management`: consults `wp_color_management_v1`.
#[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
pub fn negotiate_color(
    window: &Window,
    preferences: &ColorPreferences,
    surface_caps: &wgpu::SurfaceCapabilities,
) -> ColorSetup {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

    // Wayland raw handles — absent on X11 / other backends.
    let handles = (
        window.display_handle().ok().map(|h| h.as_raw()),
        window.window_handle().ok().map(|h| h.as_raw()),
    );
    let (display_ptr, surface_ptr) = match handles {
        (Some(RawDisplayHandle::Wayland(d)), Some(RawWindowHandle::Wayland(w))) => {
            (d.display.as_ptr(), w.surface.as_ptr())
        }
        _ => return ColorSetup::srgb_unavailable(surface_caps),
    };

    let mgr = unsafe { WaylandColorManager::try_new(display_ptr, surface_ptr) };
    let compositor_caps = mgr
        .as_ref()
        .map(|m| m.capabilities())
        .unwrap_or_else(HostColorCapabilities::srgb_only);
    let targets = mgr
        .as_ref()
        .map(|m| m.preferred_targets())
        .unwrap_or_default();

    // Negotiate the swapchain format + working space from the app's color
    // preferences, the compositor's capabilities, and what the wgpu
    // swapchain can actually carry. On a genuinely HDR output an app that
    // asks for extended-range linear (scRGB) gets an `Rgba16Float`
    // swapchain — wgpu tags it scRGB, the compositor encodes, our linear
    // values go out verbatim (SDR ≤1.0 unchanged, >1.0 = HDR). We attach no
    // description; the WSI owns the surface tag (compliant — float-format
    // selection is a normal client knob, not a second `get_surface`). Apps
    // that don't ask for HDR (the default `sdr_only`) stay on the cheaper
    // 8-bit sRGB baseline. See docs/COLOR_MANAGEMENT.md.
    let (format, working_space) =
        negotiate_output(preferences, &compositor_caps, &surface_caps.formats, &targets);

    // Diagnostic: DAMASCENE_COLOR_DEBUG=1 dumps the wgpu surface formats (what
    // Mesa's WSI advertises), the compositor's reported state, and the
    // swapchain format we settled on.
    if std::env::var("DAMASCENE_COLOR_DEBUG").is_ok() {
        eprintln!(
            "damascene color: surface formats = {:?}",
            surface_caps.formats
        );
        eprintln!(
            "damascene color: compositor primaries={:?} transfers={:?} parametric={}",
            compositor_caps.primaries,
            compositor_caps.transfer_functions,
            compositor_caps.parametric_creator(),
        );
        eprintln!(
            "damascene color: preferred targets ref_white={:?} display_peak={:?} preferred_tf={:?} preferred_primaries={:?} indicates_hdr={}",
            targets.reference_luminance_nits,
            targets.target_max_luminance_nits,
            targets.preferred_transfer,
            targets.preferred_primaries,
            targets.indicates_hdr(),
        );
        let wide = format == wgpu::TextureFormat::Rgba16Float;
        eprintln!(
            "damascene color: WSI owns surface color (no attach) — chose {format:?} ({})",
            if wide {
                "scRGB extended-range HDR"
            } else {
                "sRGB baseline"
            },
        );
    }

    // We never attach a description, so there is nothing for the compositor
    // to interpret differently from the swapchain tag. We still report the
    // protocol as Available (with the read-only targets) when the manager
    // bound, so the showcase can inspect the host. The manager stays alive
    // in the caller's per-window state: its `poll` watches
    // `preferred_changed(2)` so the host can re-negotiate live when the
    // surface moves between outputs or the output's HDR configuration
    // changes.
    let status = if mgr.is_some() {
        ColorManagementStatus::Available {
            capabilities: compositor_caps,
            attached: None,
            targets,
        }
    } else {
        ColorManagementStatus::Unavailable
    };
    // `working_space` comes from negotiation. Today every deliverable space
    // is sRGB-primaries (sRGB or scRGB), so it resolves to `SRGB_LINEAR`
    // either way — the swapchain format, not the working space, is what
    // differs (8-bit sRGB HW-encoded vs fp16 extended-linear verbatim).
    // Wider working spaces would flow through here once wgpu can deliver a
    // wider-gamut swapchain to pair with them.
    ColorSetup {
        format,
        working_space,
        status,
        manager: mgr,
    }
}

/// Result of color negotiation for a surface.
#[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
pub struct ColorSetup {
    pub format: wgpu::TextureFormat,
    pub working_space: ColorSpace,
    pub status: ColorManagementStatus,
    /// Live color-management driver — kept in the caller's per-window
    /// state so the host can poll `preferred_changed(2)` and
    /// re-negotiate. `None` on non-wayland backends or compositors
    /// without the protocol.
    pub manager: Option<WaylandColorManager>,
}

#[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
impl ColorSetup {
    fn srgb_unavailable(surface_caps: &wgpu::SurfaceCapabilities) -> Self {
        Self {
            format: srgb_format(&surface_caps.formats),
            working_space: ColorSpace::SRGB_LINEAR,
            status: ColorManagementStatus::Unavailable,
            manager: None,
        }
    }
}

/// Per-window color-management driver: negotiated swapchain format +
/// working space, live `wp_color_management_v1` client, and the
/// diagnostics apps see via
/// [`HostDiagnostics::color_management`](damascene_core::HostDiagnostics::color_management).
///
/// This struct exists on every platform; off Linux (or without the
/// `wayland-color-management` feature) [`negotiate`](Self::negotiate)
/// settles on the sRGB baseline and [`poll`](Self::poll) never fires.
/// A custom host therefore doesn't need its own cfg dance.
///
/// # Drop order
///
/// The wayland client shares winit's libwayland connection, so a
/// `SurfaceColor` **must be dropped before the `Window` it was
/// negotiated against** (in a struct, declare it before the window —
/// fields drop in declaration order).
pub struct SurfaceColor {
    format: wgpu::TextureFormat,
    working_space: ColorSpace,
    status: ColorManagementStatus,
    /// Preference ladder snapshot — re-walked on every live
    /// re-negotiation.
    #[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
    preferences: ColorPreferences,
    /// Surface format snapshot from startup — the list a live
    /// re-negotiation chooses from. WSI format offerings don't change
    /// at runtime (they're per-device); only the compositor's
    /// preferred description does.
    #[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
    formats: Vec<wgpu::TextureFormat>,
    /// Live `wp_color_management_v1` driver; `None` on non-wayland
    /// backends or compositors without the protocol.
    #[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
    manager: Option<WaylandColorManager>,
}

/// What changed in a live color re-negotiation — returned by
/// [`SurfaceColor::poll`], applied by the host.
///
/// Always: feed `headroom` / `reference_nits` to
/// `Runner::set_output_luminance` (the output's luminance frame can
/// change without a format flip — e.g. a peak-luminance
/// reconfiguration on the same HDR output), refresh any cached
/// diagnostics from [`SurfaceColor::status`], and request a redraw.
///
/// When `new_format` is `Some` (SDR ↔ HDR output move / toggle):
/// reconfigure the surface with it, call `Runner::set_target_format`
/// (rebuilds only the format-bound pipelines — interaction state,
/// atlases, and texture caches survive) and
/// `Runner::set_working_color_space(working_space)`, and reallocate
/// any MSAA target. No white-scale change on a format flip: reference
/// white sits at signal 1.0 on both encodings here (8-bit sRGB by
/// definition; the float swapchain via Mesa's parametric ext-linear
/// tag + compositor anchoring — see docs/COLOR_MANAGEMENT.md).
#[derive(Debug, Clone, Copy)]
pub struct Renegotiation {
    /// `Some` iff the negotiated swapchain format flipped.
    pub new_format: Option<wgpu::TextureFormat>,
    /// The (re-)negotiated renderer working space.
    pub working_space: ColorSpace,
    /// Usable range above reference white, in multiples of it — see
    /// [`output_luminance`].
    pub headroom: f32,
    /// Reference white in nits.
    pub reference_nits: f32,
}

impl SurfaceColor {
    /// Negotiate color for a freshly-created window surface: intersect
    /// the app's preferences with what the display server can
    /// color-manage and what the wgpu surface can represent. Silent
    /// sRGB fallback on any mismatch (and always off
    /// Linux/wayland-color-management).
    ///
    /// Configure the swapchain with [`format`](Self::format), the
    /// renderer with [`working_space`](Self::working_space) and
    /// [`output_luminance`](Self::output_luminance).
    pub fn negotiate(
        window: &Window,
        preferences: &ColorPreferences,
        surface_caps: &wgpu::SurfaceCapabilities,
    ) -> Self {
        #[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
        {
            let setup = negotiate_color(window, preferences, surface_caps);
            Self {
                format: setup.format,
                working_space: setup.working_space,
                status: setup.status,
                preferences: preferences.clone(),
                formats: surface_caps.formats.clone(),
                manager: setup.manager,
            }
        }
        #[cfg(not(all(target_os = "linux", feature = "wayland-color-management")))]
        {
            let _ = (window, preferences);
            Self {
                format: srgb_format(&surface_caps.formats),
                working_space: ColorSpace::SRGB_LINEAR,
                status: ColorManagementStatus::Unavailable,
            }
        }
    }

    /// The negotiated swapchain format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The negotiated renderer working space (always linear).
    pub fn working_space(&self) -> ColorSpace {
        self.working_space
    }

    /// Negotiated color-management state for
    /// [`HostDiagnostics::color_management`](damascene_core::HostDiagnostics::color_management).
    /// Refreshed live by [`poll`](Self::poll).
    pub fn status(&self) -> &ColorManagementStatus {
        &self.status
    }

    /// The output's luminance frame for `Runner::set_output_luminance`,
    /// or `None` when the compositor reports no color management (the
    /// renderer's defaults already match that case).
    pub fn output_luminance(&self) -> Option<(f32, f32)> {
        #[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
        if let ColorManagementStatus::Available { targets, .. } = &self.status {
            return Some(output_luminance(targets, self.format));
        }
        None
    }

    /// Drive the live color-management client: drain its wayland queue
    /// and, when the compositor changed this surface's preferred
    /// description (output move, HDR toggle), re-negotiate.
    ///
    /// Cheap in the steady state (one non-blocking `dispatch_pending`);
    /// only an actual change pays the description re-read and returns a
    /// [`Renegotiation`] for the host to apply. Call once per event-loop
    /// wake, per window.
    pub fn poll(&mut self) -> Option<Renegotiation> {
        #[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
        {
            let mgr = self.manager.as_mut()?;
            let targets = mgr.poll()?;
            let capabilities = mgr.capabilities();

            let (format, working_space) =
                negotiate_output(&self.preferences, &capabilities, &self.formats, &targets);

            if std::env::var("DAMASCENE_COLOR_DEBUG").is_ok() {
                eprintln!(
                    "damascene color: preferred changed — ref_white={:?} display_peak={:?} \
                     indicates_hdr={} → format {:?} ({})",
                    targets.reference_luminance_nits,
                    targets.target_max_luminance_nits,
                    targets.indicates_hdr(),
                    format,
                    if format == self.format {
                        "unchanged"
                    } else {
                        "switching"
                    },
                );
            }

            let (headroom, reference) = output_luminance(&targets, format);
            let new_format = (format != self.format).then_some(format);
            self.format = format;
            self.working_space = working_space;
            self.status = ColorManagementStatus::Available {
                capabilities,
                attached: None,
                targets,
            };
            Some(Renegotiation {
                new_format,
                working_space,
                headroom,
                reference_nits: reference,
            })
        }
        #[cfg(not(all(target_os = "linux", feature = "wayland-color-management")))]
        {
            None
        }
    }
}
