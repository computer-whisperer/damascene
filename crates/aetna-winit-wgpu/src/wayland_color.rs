//! Read-only Wayland `wp_color_management_v1` driver, side-loaded onto
//! winit's `wl_surface` to *inspect* the compositor's color state.
//!
//! winit 0.30 exposes no color-management API but does expose the raw
//! `wl_display` and `wl_surface` C pointers via `raw-window-handle 0.6`.
//! So we open a second `wayland_client::Connection` against winit's
//! display (sharing the libwayland connection via
//! [`Backend::from_foreign_display`]), bind `wp_color_manager_v1`, and
//! read two things: the advertised capabilities, and — via
//! `get_surface_feedback` → `get_preferred` → `get_information` — the
//! compositor's *preferred* image description for the surface (reference
//! white, display peak, preferred encoding). Both are surfaced through
//! [`aetna_core::HostDiagnostics`] for the Color Management showcase page.
//!
//! ## Why read-only
//!
//! We deliberately do **not** attach our own image description. Per the
//! protocol a `wl_surface` has exactly one color-management owner
//! (`get_surface` raises `surface_exists` otherwise), and for an
//! accelerated client that owner is the wgpu/Vulkan WSI (Mesa), which
//! already tags the swapchain — proactively on HDR outputs. Because we
//! *share* winit/Mesa's libwayland connection, a second `get_surface`
//! raises a *connection-fatal* protocol error that takes down the whole
//! app (observed on KDE with HDR enabled). `get_surface_feedback`, by
//! contrast, has no exclusivity rule, so reading is always safe.
//!
//! Driving wide-gamut / HDR *output* compliantly is the WSI's job, steered
//! by a swapchain-colorspace knob (`VK_EXT_swapchain_colorspace`) that
//! wgpu does not expose yet — see the color roadmap's wgpu blocker note.
//!
//! All entry points return `Option` and degrade quietly to a "no-op"
//! state on non-wayland hosts, compositors that don't advertise the
//! protocol, or any wire failure. Callers treat absence as normal.
//!
//! ## Lifetimes
//!
//! The connection, event queue, and bound proxy live only for the
//! duration of [`WaylandColorManager::try_new`], which reads the data and
//! drops them; the returned value is plain data. We pass
//! `from_foreign_display` (not `from_owned`), so dropping our Backend does
//! *not* call `wl_display_disconnect` — winit retains ownership.
//!
//! ## Threading
//!
//! `wp_color_management_v1` is bound on a dedicated event queue we create;
//! winit's own dispatch is unaffected. Our roundtrips block the calling
//! thread but run only during setup (`try_new`), never per-frame.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use aetna_core::color::{
    ColorFeature, CompositorColorTargets, HostColorCapabilities, Primaries as APrimaries,
    RenderIntent as ARenderIntent, TransferFunction as ATransferFunction,
};

use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1,
    wp_color_manager_v1::{
        self, Feature as WpFeature, Primaries as WpPrimaries, RenderIntent,
        TransferFunction as WpTransferFunction, WpColorManagerV1,
    },
    wp_image_description_info_v1::{self, WpImageDescriptionInfoV1},
    wp_image_description_v1::{self, WpImageDescriptionV1},
};

/// Read-only side-channel `wp_color_management_v1` driver for one
/// `wl_surface`.
///
/// One instance per window. Cheap to construct (a registry roundtrip plus
/// bind, then one feedback read). Dropping releases our half of the
/// protocol — winit's surface continues uninterrupted.
///
/// **We never attach our own image description.** Per the protocol a
/// `wl_surface` has exactly one color-management owner, and for an
/// accelerated client that owner is the WSI (Mesa), which already tags the
/// swapchain — proactively on HDR outputs. Calling `get_surface` a second
/// time raises a *connection-fatal* `surface_exists` error on the
/// libwayland connection we share with winit/Mesa, which crashes the whole
/// app (observed on KDE with HDR enabled). So this driver only *reads*: it
/// binds the manager, reads the advertised capabilities, and reads the
/// compositor's preferred image description via the feedback object (which
/// has no exclusivity rule). Driving wide / HDR output compliantly needs a
/// swapchain-colorspace knob from wgpu (`VK_EXT_swapchain_colorspace`),
/// which it does not expose yet — see the wgpu blocker note in the color
/// roadmap.
pub struct WaylandColorManager {
    // Data-only: the wayland connection, event queue, bound manager proxy,
    // and dispatch state live only for the duration of `try_new`, which
    // reads the capabilities + preferred description and then drops them.
    // Nothing wayland-side is held past setup, so there's no shared
    // connection to keep alive (or to tear down in a particular order).
    capabilities: HostColorCapabilities,
    /// What the compositor's preferred image description for this surface
    /// reported at setup (reference white, display peak, preferred
    /// encoding). Drives HDR gating + reference-white resolution. All-`None`
    /// when the compositor exposes no usable feedback path.
    preferred_targets: CompositorColorTargets,
}

impl WaylandColorManager {
    /// Try to set up a color-management driver against the supplied
    /// raw `wl_display` + `wl_surface` pointers.
    ///
    /// Returns `None` if any of these are true:
    /// - The pointers are null (caller is on a non-Wayland backend).
    /// - The compositor does not advertise `wp_color_manager_v1` (no
    ///   color-management protocol on this server).
    /// - Any wire-level error during setup (compositor crash mid-handshake,
    ///   permission denied, etc.).
    ///
    /// The caller is expected to treat `None` as "no color management
    /// available" and continue with status-quo sRGB rendering.
    ///
    /// # Safety
    ///
    /// `display_ptr` and `surface_ptr` must point to a live `wl_display`
    /// and `wl_surface` owned by winit (or whoever owns the wayland
    /// connection) for the duration of this call. The returned
    /// [`WaylandColorManager`] is plain data and borrows nothing, so it
    /// may outlive them.
    pub unsafe fn try_new(display_ptr: *mut c_void, surface_ptr: *mut c_void) -> Option<Self> {
        if display_ptr.is_null() || surface_ptr.is_null() {
            return None;
        }

        let backend = unsafe {
            Backend::from_foreign_display(display_ptr as *mut wayland_sys::client::wl_display)
        };
        let connection = Connection::from_backend(backend);

        // `registry_queue_init` does the global registry roundtrip for
        // us on a fresh event queue, returning the global list.
        let (globals, mut event_queue) = registry_queue_init::<State>(&connection).ok()?;
        let qh = event_queue.handle();

        // Find `wp_color_manager_v1`. Bind anywhere in 1..=2 — version
        // 2 is what our wayland-protocols XML defines; older compositors
        // exporting v1 work with the v1 subset we use.
        if !globals.contents().with_list(|list| {
            list.iter()
                .any(|g| g.interface == WpColorManagerV1::interface().name)
        }) {
            return None;
        }
        let color_manager: WpColorManagerV1 = globals
            .bind::<WpColorManagerV1, _, _>(&qh, 1..=2, ())
            .ok()?;

        // Initial dispatch: the compositor fires the burst of
        // `supported_primaries_named` / `supported_tf_named` /
        // `supported_feature` events right after bind, terminated with
        // `done`. roundtrip() ensures we've drained them.
        let mut state = State::default();
        event_queue.roundtrip(&mut state).ok()?;

        // Build the capability set from the events we collected.
        let capabilities = state.collected_capabilities();

        // View-wrap winit's `wl_surface` for use as a request argument
        // (see `view_foreign_surface` for why this isn't `manage_object`).
        // Used only to read the preferred description below via the
        // feedback object; we never call `get_surface` on it (that would
        // raise a connection-fatal `surface_exists` against the WSI's own
        // color-management surface — see the type-level docs).
        let surface_view = unsafe { view_foreign_surface(&connection, surface_ptr) }?;

        // Read the compositor's preferred image description for this
        // surface — reference white, display peak, preferred encoding.
        // Read-only; failures degrade to all-`None` targets.
        let preferred_targets = read_preferred_targets(
            &color_manager,
            &surface_view,
            &qh,
            &mut event_queue,
            &mut state,
            capabilities.parametric_creator(),
        );

        // Keep only the read-out data. `connection`, `event_queue`,
        // `color_manager`, `state`, and `surface_view` drop here: the bound
        // manager proxy is destroyed before its event queue (declaration
        // order), and dropping our `from_foreign_display` connection does
        // not disconnect winit's.
        Some(Self {
            capabilities,
            preferred_targets,
        })
    }

    /// Capabilities the compositor advertised. Pass this into
    /// [`aetna_core::color::ColorPreferences::negotiate`] to pick a
    /// working space the host can actually deliver.
    pub fn capabilities(&self) -> HostColorCapabilities {
        self.capabilities.clone()
    }

    /// What the compositor's *preferred* image description for this
    /// surface reported at setup. The negotiator uses
    /// [`CompositorColorTargets::indicates_hdr`] to gate HDR output and
    /// [`CompositorColorTargets::reference_luminance_nits`] to resolve the
    /// reference white. All-`None` when no usable feedback path exists.
    pub fn preferred_targets(&self) -> CompositorColorTargets {
        self.preferred_targets.clone()
    }
}

/// Read the compositor's preferred image description for `surface_view`
/// and extract its reference white / display peak / preferred encoding.
///
/// Best-effort: a wire error, a `failed` description (e.g. `low_version`),
/// or an ICC-only preferred description (no structured luminance events)
/// all yield the default all-`None` [`CompositorColorTargets`], which the
/// negotiator reads as "no HDR evidence, stay SDR".
///
/// Read once at setup. The feedback object is destroyed immediately
/// afterwards — reacting to runtime `preferred_changed` (output moves,
/// brightness changes) is a future extension that would require live
/// re-negotiation, which the host doesn't do yet.
fn read_preferred_targets(
    color_manager: &WpColorManagerV1,
    surface_view: &WlSurface,
    qh: &QueueHandle<State>,
    event_queue: &mut EventQueue<State>,
    state: &mut State,
    parametric: bool,
) -> CompositorColorTargets {
    let feedback: WpColorManagementSurfaceFeedbackV1 =
        color_manager.get_surface_feedback(surface_view, qh, ());

    // Prefer the parametric form so the info burst carries structured
    // luminance / transfer events. `get_preferred_parametric` requires the
    // same `parametric` feature the caller already checked; without it,
    // `get_preferred` may yield an ICC description we can't introspect —
    // handled below as empty targets.
    let pending = Arc::new(PendingDescription::default());
    state.pending = Some(Arc::clone(&pending));
    let desc: WpImageDescriptionV1 = if parametric {
        feedback.get_preferred_parametric(qh, ())
    } else {
        feedback.get_preferred(qh, ())
    };

    // Wait for the description to resolve (ready / failed) before asking
    // for its information — `get_information` on a failed description is a
    // protocol error.
    while pending.lock().is_none() {
        if event_queue.roundtrip(state).is_err() {
            state.pending = None;
            feedback.destroy();
            return CompositorColorTargets::default();
        }
    }
    let resolution = pending.lock().take();
    state.pending = None;

    let targets = match resolution {
        Some(DescriptionResolution::Ready) => {
            // Drain the info burst into `state.info`, terminated by `done`.
            state.info = CompositorColorTargets::default();
            state.info_done = false;
            let _info: WpImageDescriptionInfoV1 = desc.get_information(qh, ());
            while !state.info_done {
                if event_queue.roundtrip(state).is_err() {
                    break;
                }
            }
            std::mem::take(&mut state.info)
        }
        // Failed / low_version / wire error: no usable hint.
        _ => CompositorColorTargets::default(),
    };

    desc.destroy();
    feedback.destroy();
    targets
}

// ---------------------------------------------------------------------------
// Dispatch state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct State {
    primaries: Vec<APrimaries>,
    transfer_functions: Vec<ATransferFunction>,
    features: Vec<ColorFeature>,
    render_intents: Vec<ARenderIntent>,
    /// Slot the pending image-description's resolution lands in. Set
    /// before `create` is called, cleared once `ready` / `failed` is
    /// observed.
    pending: Option<Arc<PendingDescription>>,
    /// Accumulator for the in-flight `wp_image_description_info_v1` burst.
    /// `read_preferred_targets` resets this before `get_information`, then
    /// reads it once `info_done` flips on the terminating `done` event.
    info: CompositorColorTargets,
    info_done: bool,
}

impl State {
    fn collected_capabilities(&self) -> HostColorCapabilities {
        HostColorCapabilities {
            primaries: self.primaries.clone(),
            transfer_functions: self.transfer_functions.clone(),
            features: self.features.clone(),
            render_intents: self.render_intents.clone(),
        }
    }
}

/// Slot the image-description-creation outcome lands in. Mutex<Option<_>>
/// rather than OnceCell so we can reset it across `apply` calls.
#[derive(Default)]
struct PendingDescription(Mutex<Option<DescriptionResolution>>);

impl PendingDescription {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<DescriptionResolution>> {
        self.0.lock().expect("description-pending mutex poisoned")
    }
}

enum DescriptionResolution {
    /// The preferred description resolved successfully; we then read its
    /// info via the original proxy (no need to carry it here).
    Ready,
    Failed,
}

// ---------------------------------------------------------------------------
// Dispatch impls — boilerplate connecting wire events to State fields.
// ---------------------------------------------------------------------------

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // We only consult the static global list via registry_queue_init;
        // dynamic add/remove during this driver's lifetime is uncommon
        // for color-management and we don't react to it.
    }
}

impl Dispatch<WpColorManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpColorManagerV1,
        event: <WpColorManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::WEnum;
        use wp_color_manager_v1::Event;
        match event {
            Event::SupportedPrimariesNamed {
                primaries: WEnum::Value(p),
            } => {
                if let Some(a) = primaries_from_wp(p) {
                    state.primaries.push(a);
                }
            }
            Event::SupportedTfNamed {
                tf: WEnum::Value(tf),
            } => {
                if let Some(a) = transfer_from_wp(tf) {
                    state.transfer_functions.push(a);
                }
            }
            Event::SupportedFeature {
                feature: WEnum::Value(f),
            } => {
                if let Some(cf) = feature_from_wp(f) {
                    state.features.push(cf);
                }
            }
            Event::SupportedIntent {
                render_intent: WEnum::Value(i),
            } => {
                // Aetna always requests `Perceptual` when applying; the
                // full advertised set is collected for inspection.
                if let Some(ai) = intent_from_wp(i) {
                    state.render_intents.push(ai);
                }
            }
            Event::Done => {
                // Sentinel — no action needed; presence/absence of
                // capability events already populated state.
            }
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpImageDescriptionV1,
        event: <WpImageDescriptionV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wp_image_description_v1::Event;
        let Some(slot) = state.pending.as_ref() else {
            // No one is waiting on a resolution (the preferred-description
            // read isn't in flight) — ignore.
            return;
        };
        match event {
            Event::Ready { .. } | Event::Ready2 { .. } => {
                let mut guard = slot.lock();
                if guard.is_none() {
                    *guard = Some(DescriptionResolution::Ready);
                }
            }
            Event::Failed { .. } => {
                let mut guard = slot.lock();
                if guard.is_none() {
                    *guard = Some(DescriptionResolution::Failed);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WpColorManagementSurfaceFeedbackV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpColorManagementSurfaceFeedbackV1,
        _: <WpColorManagementSurfaceFeedbackV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // `preferred_changed` / `preferred_changed2` only. We read the
        // preferred description once at setup via an explicit
        // `get_preferred`, and don't track runtime changes (output moves,
        // brightness adjustments) yet — so there's nothing to act on.
    }
}

impl Dispatch<WpImageDescriptionInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpImageDescriptionInfoV1,
        event: <WpImageDescriptionInfoV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::WEnum;
        use wp_image_description_info_v1::Event;
        match event {
            Event::Luminances {
                min_lum,
                max_lum,
                reference_lum,
            } => {
                // `min_lum` carries 4 decimals (×10000); `max_lum` and
                // `reference_lum` are unscaled cd/m².
                state.info.min_luminance_nits = Some(min_lum as f32 / 10000.0);
                state.info.max_luminance_nits = Some(max_lum as f32);
                state.info.reference_luminance_nits = Some(reference_lum as f32);
            }
            Event::TargetLuminance { min_lum, max_lum } => {
                // The display's targeted range. `max` is our HDR-headroom
                // signal; `min` carries 4 decimals (×10000).
                state.info.target_min_luminance_nits = Some(min_lum as f32 / 10000.0);
                state.info.target_max_luminance_nits = Some(max_lum as f32);
            }
            Event::TargetMaxCll { max_cll } => {
                state.info.max_content_light_level_nits = Some(max_cll as f32);
            }
            Event::TargetMaxFall { max_fall } => {
                state.info.max_frame_average_light_level_nits = Some(max_fall as f32);
            }
            Event::TfNamed {
                tf: WEnum::Value(tf),
            } => {
                state.info.preferred_transfer = transfer_from_wp(tf);
            }
            Event::PrimariesNamed {
                primaries: WEnum::Value(p),
            } => {
                state.info.preferred_primaries = primaries_from_wp(p);
            }
            Event::IccFile { .. } => {
                // The preferred description is ICC-based — we can't read
                // its primaries/transfer/luminances structurally.
                state.info.preferred_is_icc = true;
            }
            Event::Done => {
                state.info_done = true;
            }
            // primaries (coords), tf_power, target_primaries: not
            // load-bearing for HDR gating or reference-white resolution yet.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Enum mapping aetna_core::color <-> wp_color_management_v1
// ---------------------------------------------------------------------------

fn feature_from_wp(f: WpFeature) -> Option<ColorFeature> {
    Some(match f {
        WpFeature::IccV2V4 => ColorFeature::IccV2V4,
        WpFeature::Parametric => ColorFeature::Parametric,
        WpFeature::SetPrimaries => ColorFeature::SetPrimaries,
        WpFeature::SetTfPower => ColorFeature::SetTfPower,
        WpFeature::SetLuminances => ColorFeature::SetLuminances,
        WpFeature::SetMasteringDisplayPrimaries => ColorFeature::SetMasteringDisplayPrimaries,
        WpFeature::ExtendedTargetVolume => ColorFeature::ExtendedTargetVolume,
        WpFeature::WindowsScrgb => ColorFeature::WindowsScrgb,
        // Forward-compat: a feature added in a future protocol version.
        _ => return None,
    })
}

fn intent_from_wp(i: RenderIntent) -> Option<ARenderIntent> {
    Some(match i {
        RenderIntent::Perceptual => ARenderIntent::Perceptual,
        RenderIntent::Relative => ARenderIntent::Relative,
        RenderIntent::Saturation => ARenderIntent::Saturation,
        RenderIntent::Absolute => ARenderIntent::Absolute,
        RenderIntent::RelativeBpc => ARenderIntent::RelativeBpc,
        RenderIntent::AbsoluteNoAdaptation => ARenderIntent::AbsoluteNoAdaptation,
        _ => return None,
    })
}

fn primaries_from_wp(p: WpPrimaries) -> Option<APrimaries> {
    Some(match p {
        WpPrimaries::Srgb => APrimaries::Srgb,
        WpPrimaries::Bt2020 => APrimaries::Bt2020,
        WpPrimaries::DisplayP3 => APrimaries::DisplayP3,
        WpPrimaries::AdobeRgb => APrimaries::AdobeRgb,
        // Compositor named primaries aetna doesn't model (PAL, NTSC,
        // generic film, CIE 1931 XYZ, DCI-P3 with non-D65 white). We
        // can't author content in these, so leave them out of caps.
        _ => return None,
    })
}

fn transfer_from_wp(tf: WpTransferFunction) -> Option<ATransferFunction> {
    use ATransferFunction::*;
    Some(match tf {
        WpTransferFunction::Bt1886 => Bt1886,
        WpTransferFunction::Gamma22 => Srgb, // close enough for the UI use case
        WpTransferFunction::ExtLinear => Linear,
        WpTransferFunction::St2084Pq => Pq,
        WpTransferFunction::Hlg => Hlg,
        WpTransferFunction::Srgb => Srgb,
        // Other named TFs (ST 240, log_100, log_316, xvYCC, ext_sRGB,
        // ST 428, gamma28) aren't load-bearing for UI work; skipping
        // until we have authored content that needs them.
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Foreign-surface wrapping
// ---------------------------------------------------------------------------

/// Build a typed [`WlSurface`] proxy referencing winit's existing
/// `wl_surface`, *without* taking it under our backend's management.
///
/// Uses [`ObjectId::from_ptr`], which adopts the proxy's interface +
/// id without inserting it into `known_proxies`. That's the crucial
/// difference from `manage_object`: when our connection is dropped,
/// it won't try to call `wl_proxy_destroy` on winit's surface (which
/// would either abort or sever winit's binding). The returned
/// [`WlSurface`] is "view-only" — sending requests through it would
/// be a protocol violation (winit owns the surface), but passing it
/// as an *argument* to other requests (which is all we need) is fine.
///
/// # Safety
///
/// `surface_ptr` must be a live `wl_proxy*` for a `wl_surface` on
/// the same `wl_display` as `connection`'s backend, and must remain
/// alive for as long as the returned proxy is used.
unsafe fn view_foreign_surface(
    connection: &Connection,
    surface_ptr: *mut c_void,
) -> Option<WlSurface> {
    use wayland_sys::client::wl_proxy;
    let object_id =
        unsafe { ObjectId::from_ptr(WlSurface::interface(), surface_ptr as *mut wl_proxy) }.ok()?;
    WlSurface::from_id(connection, object_id).ok()
}
