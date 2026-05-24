//! Wayland `wp_color_management_v1` driver, side-loaded onto winit's
//! `wl_surface`.
//!
//! winit 0.30 exposes no color-management API but does expose the raw
//! `wl_display` and `wl_surface` C pointers via `raw-window-handle 0.6`.
//! Mesa's WSI doesn't drive the color-management extension either. So we
//! open a second `wayland_client::Connection` against winit's display
//! (sharing the libwayland connection via [`Backend::from_foreign_display`]),
//! bind `wp_color_manager_v1` ourselves, and attach the appropriate
//! image description to the same `wl_surface` winit owns.
//!
//! The compositor doesn't care which client object attached the image
//! description; what matters is that one is attached when the surface
//! commits — and winit commits per frame.
//!
//! All entry points return `Option` / `Result` and degrade quietly to a
//! "no-op" state on non-wayland hosts, compositors that don't advertise
//! the protocol, or any wire failure. Callers should treat absence as
//! the normal case, not an error.
//!
//! ## Lifetimes
//!
//! winit owns the `wl_display` for the lifetime of the `EventLoop`. The
//! [`WaylandColorManager`] is created inside `Host::resumed` and dropped
//! before the window, so the backend pointer it holds is always valid.
//! We pass `from_foreign_display` (not `from_owned`), so dropping our
//! Backend does *not* call `wl_display_disconnect` — winit retains
//! ownership.
//!
//! ## Threading
//!
//! `wp_color_management_v1` is bound on a dedicated event queue we
//! create. winit's event dispatch on its own queue is unaffected. Our
//! roundtrips block the calling thread but only run during setup
//! (`try_new`) and per-working-space-change (`apply`); they do not run
//! per-frame.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use aetna_core::color::{
    ColorSpace, HostColorCapabilities, Primaries as APrimaries,
    TransferFunction as ATransferFunction,
};

use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_v1::WpColorManagementSurfaceV1,
    wp_color_manager_v1::{
        self, Feature as WpFeature, Primaries as WpPrimaries, RenderIntent,
        TransferFunction as WpTransferFunction, WpColorManagerV1,
    },
    wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1,
    wp_image_description_v1::{self, WpImageDescriptionV1},
};

/// Failure mode for [`WaylandColorManager::apply`]. None of these
/// should be fatal — callers fall back to leaving no image description
/// on the surface, which is identical to the pre-color-management
/// behavior.
#[derive(Debug)]
pub enum ApplyError {
    /// The chosen [`ColorSpace`] uses a primaries / transfer function
    /// the compositor didn't advertise. The caller should re-negotiate
    /// against [`WaylandColorManager::capabilities`] before retrying.
    Unsupported(&'static str),
    /// The compositor reported a `failed` event on the image
    /// description (typically: parameters out of range for its
    /// implementation). The caller should fall back to sRGB.
    DescriptionFailed,
    /// A wire-level error during dispatch.
    Wire(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Unsupported(what) => write!(f, "compositor does not support {what}"),
            ApplyError::DescriptionFailed => {
                write!(f, "compositor rejected the image description parameters")
            }
            ApplyError::Wire(s) => write!(f, "wayland dispatch error: {s}"),
        }
    }
}

impl std::error::Error for ApplyError {}

/// Side-channel `wp_color_management_v1` driver for one `wl_surface`.
///
/// One instance per window. Cheap to construct (a registry roundtrip
/// + bind); cheap to call `apply` (one roundtrip to validate the
/// description). Dropping releases our half of the protocol — winit's
/// surface continues uninterrupted.
pub struct WaylandColorManager {
    // Field drop order matters. Our wp_* proxies must be destroyed
    // before the Connection that owns their backend; the foreign
    // `_surface_view` is `ObjectId::from_ptr`-built and not in
    // `known_proxies`, so it incurs no protocol traffic on drop.
    cm_surface: WpColorManagementSurfaceV1,
    color_manager: WpColorManagerV1,
    /// View-only proxy aliasing winit's `wl_surface`. Kept around so
    /// `wp_color_management_surface_v1` can be reused across `apply`
    /// calls (the protocol creates one cm_surface per wl_surface).
    _surface_view: WlSurface,
    capabilities: HostColorCapabilities,
    event_queue: EventQueue<State>,
    state: State,
    _connection: Connection,
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
    /// connection). The returned [`WaylandColorManager`] must be
    /// dropped before that owner shuts down the connection.
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
        let surface_view = unsafe { view_foreign_surface(&connection, surface_ptr) }?;

        let cm_surface: WpColorManagementSurfaceV1 =
            color_manager.get_surface(&surface_view, &qh, ());

        // Drain any errors from get_surface eagerly so we surface them
        // here rather than at first apply.
        event_queue.roundtrip(&mut state).ok()?;

        Some(Self {
            cm_surface,
            color_manager,
            _surface_view: surface_view,
            capabilities,
            event_queue,
            state,
            _connection: connection,
        })
    }

    /// Capabilities the compositor advertised. Pass this into
    /// [`aetna_core::color::ColorPreferences::negotiate`] to pick a
    /// working space the host can actually deliver.
    pub fn capabilities(&self) -> HostColorCapabilities {
        self.capabilities.clone()
    }

    /// Build a parametric image description for `space` and attach it
    /// to the surface. The effect lands on the next `wl_surface.commit`
    /// (which winit will perform on the next frame).
    ///
    /// Returns [`ApplyError::Unsupported`] if `space` requires features
    /// or named TF/primaries the compositor didn't advertise — caller
    /// should re-negotiate. Returns [`ApplyError::DescriptionFailed`]
    /// if the compositor rejects the parameters at validation time
    /// (rare; usually means luminance values it can't accept).
    pub fn apply(&mut self, space: ColorSpace) -> Result<(), ApplyError> {
        if !self.capabilities.parametric_creator {
            return Err(ApplyError::Unsupported("create_parametric_creator"));
        }
        let wp_primaries = map_primaries(space.primaries)
            .filter(|_| self.capabilities.primaries.contains(&space.primaries))
            .ok_or(ApplyError::Unsupported("primaries"))?;
        let wp_tf = map_transfer(space.transfer)
            .filter(|_| {
                self.capabilities
                    .transfer_functions
                    .contains(&space.transfer)
            })
            .ok_or(ApplyError::Unsupported("transfer function"))?;

        let qh = self.event_queue.handle();

        // Build a parametric creator, populate it, then `create` to get
        // the immutable description. The creator is consumed by create.
        let creator: WpImageDescriptionCreatorParamsV1 =
            self.color_manager.create_parametric_creator(&qh, ());
        creator.set_primaries_named(wp_primaries);
        creator.set_tf_named(wp_tf);

        // `create` is a destructor request that returns a new
        // `wp_image_description_v1`. The description is not usable
        // until `ready` / `ready2` fires.
        let pending = Arc::new(PendingDescription::default());
        self.state.pending = Some(Arc::clone(&pending));
        let _desc: WpImageDescriptionV1 = creator.create(&qh, ());

        // Drive the queue until the compositor reports ready or failed.
        // The compositor responds promptly to `create`; loop in case the
        // first roundtrip drains intermediate events without resolution.
        while pending.lock().is_none() {
            self.event_queue
                .roundtrip(&mut self.state)
                .map_err(|e| ApplyError::Wire(e.to_string()))?;
        }
        let resolution = pending.lock().take().expect("we just observed Some");
        self.state.pending = None;

        match resolution {
            DescriptionResolution::Failed => return Err(ApplyError::DescriptionFailed),
            DescriptionResolution::Ready(desc) => {
                self.cm_surface
                    .set_image_description(&desc, RenderIntent::Perceptual);
                // Flush so the request hits the wire before winit's
                // next commit. We don't roundtrip — set_image_description
                // has no response; the description takes effect on the
                // surface's next commit (winit's next frame).
                self.event_queue
                    .flush()
                    .map_err(|e| ApplyError::Wire(e.to_string()))?;
                // Drop our local reference to the description proxy —
                // the compositor holds the binding via set_image_description.
                desc.destroy();
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Dispatch state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct State {
    primaries: Vec<APrimaries>,
    transfer_functions: Vec<ATransferFunction>,
    features: Vec<WpFeature>,
    /// Slot the pending image-description's resolution lands in. Set
    /// before `create` is called, cleared once `ready` / `failed` is
    /// observed.
    pending: Option<Arc<PendingDescription>>,
}

impl State {
    fn collected_capabilities(&self) -> HostColorCapabilities {
        HostColorCapabilities {
            primaries: self.primaries.clone(),
            transfer_functions: self.transfer_functions.clone(),
            parametric_creator: self.features.contains(&WpFeature::Parametric),
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
    Ready(WpImageDescriptionV1),
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
        use wp_color_manager_v1::Event;
        match event {
            Event::SupportedPrimariesNamed { primaries } => {
                if let wayland_client::WEnum::Value(p) = primaries {
                    if let Some(a) = primaries_from_wp(p) {
                        state.primaries.push(a);
                    }
                }
            }
            Event::SupportedTfNamed { tf } => {
                if let wayland_client::WEnum::Value(tf) = tf {
                    if let Some(a) = transfer_from_wp(tf) {
                        state.transfer_functions.push(a);
                    }
                }
            }
            Event::SupportedFeature { feature } => {
                if let wayland_client::WEnum::Value(f) = feature {
                    state.features.push(f);
                }
            }
            Event::SupportedIntent { .. } => {
                // We always request `Perceptual`, which compositors
                // are required to support; ignore the rest.
            }
            Event::Done => {
                // Sentinel — no action needed; presence/absence of
                // capability events already populated state.
            }
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionCreatorParamsV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpImageDescriptionCreatorParamsV1,
        _: <WpImageDescriptionCreatorParamsV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The creator has no events.
    }
}

impl Dispatch<WpImageDescriptionV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WpImageDescriptionV1,
        event: <WpImageDescriptionV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wp_image_description_v1::Event;
        let Some(slot) = state.pending.as_ref() else {
            // No one is waiting on a resolution — descriptor created
            // outside of `apply`, ignore.
            return;
        };
        match event {
            Event::Ready { .. } | Event::Ready2 { .. } => {
                let mut guard = slot.lock();
                if guard.is_none() {
                    *guard = Some(DescriptionResolution::Ready(proxy.clone()));
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

impl Dispatch<WpColorManagementSurfaceV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &WpColorManagementSurfaceV1,
        _: <WpColorManagementSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // No events on this interface.
    }
}

// ---------------------------------------------------------------------------
// Enum mapping aetna_core::color <-> wp_color_management_v1
// ---------------------------------------------------------------------------

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

fn map_primaries(p: APrimaries) -> Option<WpPrimaries> {
    Some(match p {
        APrimaries::Srgb => WpPrimaries::Srgb,
        APrimaries::DisplayP3 => WpPrimaries::DisplayP3,
        APrimaries::Bt2020 => WpPrimaries::Bt2020,
        APrimaries::AdobeRgb => WpPrimaries::AdobeRgb,
    })
}

fn map_transfer(tf: ATransferFunction) -> Option<WpTransferFunction> {
    Some(match tf {
        ATransferFunction::Srgb => WpTransferFunction::Srgb,
        ATransferFunction::Linear => WpTransferFunction::ExtLinear,
        ATransferFunction::Bt1886 => WpTransferFunction::Bt1886,
        ATransferFunction::Pq => WpTransferFunction::St2084Pq,
        ATransferFunction::Hlg => WpTransferFunction::Hlg,
        ATransferFunction::Gamma(g) if (g.to_f32() - 2.2).abs() < 0.01 => {
            WpTransferFunction::Gamma22
        }
        ATransferFunction::Gamma(g) if (g.to_f32() - 2.8).abs() < 0.01 => {
            WpTransferFunction::Gamma28
        }
        // tf_power for arbitrary exponents requires `set_tf_power`
        // feature support and a different code path; not in this cut.
        ATransferFunction::Gamma(_) => return None,
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
