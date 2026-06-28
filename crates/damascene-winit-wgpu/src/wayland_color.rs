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
//! [`damascene_core::HostDiagnostics`] for the Color Management showcase page.
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
//! by the swapchain format we pick: on an HDR output the host selects
//! `Rgba16Float`, which wgpu's Vulkan backend tags as scRGB
//! (`EXTENDED_SRGB_LINEAR_EXT`). The preferred description this driver reads
//! is what gates that choice (`CompositorColorTargets::indicates_hdr`).
//!
//! All entry points return `Option` and degrade quietly to a "no-op"
//! state on non-wayland hosts, compositors that don't advertise the
//! protocol, or any wire failure. Callers treat absence as normal.
//!
//! ## Lifetimes
//!
//! The connection, event queue, and bound proxies stay alive for the
//! manager's lifetime so [`WaylandColorManager::poll`] can observe
//! `preferred_changed` / `preferred_changed2` and re-read the preferred
//! description when the surface moves between outputs or the output's
//! HDR configuration changes. We pass `from_foreign_display` (not
//! `from_owned`), so dropping our Backend does *not* call
//! `wl_display_disconnect` — winit retains ownership. The manager must
//! not outlive winit's display; the host stores it in `Gfx`, which drops
//! before the window.
//!
//! ## Threading
//!
//! `wp_color_management_v1` is bound on a dedicated event queue we create;
//! winit's own dispatch is unaffected. Setup (`try_new`) and a dirty
//! re-read inside `poll` do blocking roundtrips on the calling thread;
//! the steady-state `poll` path is a non-blocking `dispatch_pending` —
//! winit's event loop reads the shared socket and libwayland demuxes
//! events onto our queue.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use damascene_core::color::{
    ColorFeature, CompositorColorTargets, HostColorCapabilities, OutputColorCapability,
    Primaries as APrimaries, RenderIntent as ARenderIntent, TransferFunction as ATransferFunction,
};

use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_output::WlOutput, wl_registry::WlRegistry, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};

use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_output_v1::{self, WpColorManagementOutputV1},
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
/// has no exclusivity rule). The host turns that read into compliant HDR
/// output by *format selection* — picking an `Rgba16Float` swapchain, which
/// wgpu's Vulkan backend tags scRGB (`EXTENDED_SRGB_LINEAR_EXT`) — not by
/// attaching a description here.
pub struct WaylandColorManager {
    capabilities: HostColorCapabilities,
    /// What the compositor's preferred image description for this surface
    /// most recently reported (reference white, display peak, preferred
    /// encoding). Drives HDR gating + reference-white resolution. All-`None`
    /// when the compositor exposes no usable feedback path. Refreshed by
    /// [`Self::poll`] when the compositor signals `preferred_changed`.
    preferred_targets: CompositorColorTargets,
    /// HDR capability aggregated across the connected outputs' own image
    /// descriptions ("any HDR output" heuristic). Out-of-band evidence that
    /// lets HDR engage on compositors which SDR-clamp the per-surface
    /// preferred description until a surface opts in (KWin). Merged into the
    /// [`CompositorColorTargets`] returned by [`Self::preferred_targets`].
    /// `None` when no output color descriptions were readable. Refreshed by
    /// [`Self::poll`] on an output's `image_description_changed`.
    output_capability: Option<OutputColorCapability>,
    /// Whether the compositor advertises the parametric creator — decides
    /// `get_preferred_parametric` vs `get_preferred` on each re-read.
    parametric: bool,

    // Live wire state. Held for the manager's lifetime so `poll` can
    // dispatch `preferred_changed(2)` / output `image_description_changed`
    // and re-read the relevant description.
    // Declaration order = drop order: proxies before their event queue,
    // queue before the connection. The connection wraps winit's display
    // via `from_foreign_display`, so dropping it never disconnects.
    feedback: WpColorManagementSurfaceFeedbackV1,
    color_manager: WpColorManagerV1,
    /// Per-output color-management proxies, kept alive so `poll` can observe
    /// `image_description_changed` (an output toggling its HDR config) and
    /// re-scan. Destroyed in `Drop`.
    output_managers: Vec<WpColorManagementOutputV1>,
    /// The bound `wl_output` proxies the above were created from. Held to
    /// keep them alive for the manager's lifetime and `release`d in `Drop`
    /// (v3+).
    outputs: Vec<WlOutput>,
    state: State,
    event_queue: EventQueue<State>,
    _connection: Connection,
}

impl Drop for WaylandColorManager {
    fn drop(&mut self) {
        // Release our half of the protocol. Best-effort: if the
        // compositor is already gone the requests just fail to send.
        self.feedback.destroy();
        for cmo in &self.output_managers {
            cmo.destroy();
        }
        // `wl_output.release` is a v3+ destructor; we bind at the server's
        // version (capped at 4), so guard older outputs that lack it.
        for output in &self.outputs {
            if output.version() >= 3 {
                output.release();
            }
        }
        self.color_manager.destroy();
        let _ = self.event_queue.flush();
    }
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

        if std::env::var("DAMASCENE_COLOR_DEBUG").is_ok() {
            eprintln!(
                "damascene color: bound wp_color_manager_v1 at version {}",
                color_manager.version(),
            );
        }

        // View-wrap winit's `wl_surface` for use as a request argument
        // (see `view_foreign_surface` for why this isn't `manage_object`).
        // Used only to create the feedback object below; we never call
        // `get_surface` on it (that would raise a connection-fatal
        // `surface_exists` against the WSI's own color-management
        // surface — see the type-level docs).
        let surface_view = unsafe { view_foreign_surface(&connection, surface_ptr) }?;

        // The feedback object is the live half of the driver: it fires
        // `preferred_changed(2)` when the surface's preferred description
        // changes (output move, HDR toggle), and `poll` re-reads through
        // it. Created once, destroyed in `Drop`.
        let feedback: WpColorManagementSurfaceFeedbackV1 =
            color_manager.get_surface_feedback(&surface_view, &qh, ());

        // Read the compositor's preferred image description for this
        // surface — reference white, display peak, preferred encoding.
        // Read-only; failures degrade to all-`None` targets.
        let parametric = capabilities.parametric_creator();
        let preferred_targets =
            read_preferred_targets(&feedback, &qh, &mut event_queue, &mut state, parametric);
        // The initial read may itself have raced a `preferred_changed`
        // burst; the value we just read is current, so start clean.
        state.preferred_dirty = false;

        // Output color capability ("any HDR output" heuristic). Some
        // compositors (KWin / Plasma 6.7) SDR-clamp the per-surface
        // preferred description above for a surface that hasn't attached an
        // HDR description, so it shows no HDR headroom even on a genuine HDR
        // panel. Each output's own `wp_color_management_output_v1` description
        // still advertises the panel's true encoding (PQ / real peak), so we
        // read every connected output and let an HDR-capable one drive HDR
        // engagement — a window can be dragged to any output, so any HDR one
        // counts. We keep the proxies alive for `image_description_changed`
        // (an output toggling HDR at runtime). New-output *hotplug* isn't
        // tracked — we don't re-bind globals that appear mid-session.
        let outputs: Vec<WlOutput> = globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == WlOutput::interface().name)
                .map(|g| {
                    globals
                        .registry()
                        .bind::<WlOutput, _, _>(g.name, g.version.min(4), &qh, ())
                })
                .collect()
        });
        let output_managers: Vec<WpColorManagementOutputV1> = outputs
            .iter()
            .map(|o| color_manager.get_output(o, &qh, ()))
            .collect();
        let output_capability =
            read_output_capability(&output_managers, &qh, &mut event_queue, &mut state);
        state.outputs_dirty = false;

        // `surface_view` drops here (it's a view, not ownership). The
        // connection / queue / proxies live on in the manager so `poll`
        // can track preferred-description and output changes.
        Some(Self {
            capabilities,
            preferred_targets,
            output_capability,
            parametric,
            feedback,
            color_manager,
            output_managers,
            outputs,
            state,
            event_queue,
            _connection: connection,
        })
    }

    /// Capabilities the compositor advertised. Pass this into
    /// [`damascene_core::color::ColorPreferences::negotiate`] to pick a
    /// working space the host can actually deliver.
    pub fn capabilities(&self) -> HostColorCapabilities {
        self.capabilities.clone()
    }

    /// What the compositor's *preferred* image description for this
    /// surface most recently reported, with the connected outputs' HDR
    /// capability merged in. The negotiator uses
    /// [`CompositorColorTargets::indicates_hdr`] to gate HDR output (which
    /// consults the merged output capability) and
    /// [`CompositorColorTargets::reference_luminance_nits`] to resolve the
    /// reference white. All-`None` when no usable feedback path exists.
    pub fn preferred_targets(&self) -> CompositorColorTargets {
        let mut targets = self.preferred_targets.clone();
        targets.output_capability = self.output_capability.clone();
        targets
    }

    /// Process any pending wayland events for this driver's queue and,
    /// if the compositor signalled `preferred_changed` /
    /// `preferred_changed2` since the last call, re-read the preferred
    /// description. Returns the fresh targets on a change, `None` when
    /// nothing changed (the common per-frame case).
    ///
    /// The steady-state path is non-blocking: winit's event loop reads
    /// the shared display socket and libwayland routes our events onto
    /// this queue; `dispatch_pending` just drains them. Only an actual
    /// change pays the blocking `get_preferred` → info-burst roundtrips
    /// (same cost as the setup read).
    ///
    /// Call once per event-loop wake (e.g. before rendering). The caller
    /// re-negotiates format / working space / white scale from the
    /// returned targets — see the host's `poll_color_management`.
    pub fn poll(&mut self) -> Option<CompositorColorTargets> {
        if self.event_queue.dispatch_pending(&mut self.state).is_err() {
            // Wire error (compositor gone mid-session). Degrade to the
            // last-known targets; the connection-level error will surface
            // through winit shortly anyway.
            return None;
        }
        let surface_dirty = self.state.preferred_dirty;
        let outputs_dirty = self.state.outputs_dirty;
        if !surface_dirty && !outputs_dirty {
            return None;
        }
        let qh = self.event_queue.handle();
        // A re-read can race the *next* change; the dispatcher may re-set
        // either dirty flag during our roundtrips, so the next poll picks it
        // up. We clear before re-reading and don't stomp a re-set flag.
        if surface_dirty {
            self.state.preferred_dirty = false;
            self.preferred_targets = read_preferred_targets(
                &self.feedback,
                &qh,
                &mut self.event_queue,
                &mut self.state,
                self.parametric,
            );
        }
        if outputs_dirty {
            self.state.outputs_dirty = false;
            self.output_capability = read_output_capability(
                &self.output_managers,
                &qh,
                &mut self.event_queue,
                &mut self.state,
            );
        }
        Some(self.preferred_targets())
    }
}

/// Read the compositor's current preferred image description through
/// `feedback` and extract its reference white / display peak / preferred
/// encoding.
///
/// Best-effort: a wire error, a `failed` description (e.g. `low_version`),
/// or an ICC-only preferred description (no structured luminance events)
/// all yield the default all-`None` [`CompositorColorTargets`], which the
/// negotiator reads as "no HDR evidence, stay SDR".
///
/// Called at setup and again from [`WaylandColorManager::poll`] whenever
/// the compositor signals `preferred_changed(2)`. Blocking (roundtrips
/// until the description resolves and its info burst completes); the
/// feedback object is owned by the caller and survives the read.
fn read_preferred_targets(
    feedback: &WpColorManagementSurfaceFeedbackV1,
    qh: &QueueHandle<State>,
    event_queue: &mut EventQueue<State>,
    state: &mut State,
    parametric: bool,
) -> CompositorColorTargets {
    // Prefer the parametric form so the info burst carries structured
    // luminance / transfer events. `get_preferred_parametric` requires the
    // same `parametric` feature the caller already checked; without it,
    // `get_preferred` may yield an ICC description we can't introspect —
    // handled below as empty targets.
    let desc: WpImageDescriptionV1 = if parametric {
        feedback.get_preferred_parametric(qh, ())
    } else {
        feedback.get_preferred(qh, ())
    };
    read_description_targets(desc, qh, event_queue, state, "surface preferred")
}

/// Resolve a `wp_image_description_v1` (from a surface feedback's
/// `get_preferred(_parametric)` or an output's `get_image_description`) and
/// extract its structured luminance / transfer / primaries into a
/// [`CompositorColorTargets`].
///
/// Best-effort: a wire error, a `failed` description (`low_version` /
/// `no_output`), or an ICC-only description (no structured luminance events)
/// all yield the default all-`None` value. Blocking — roundtrips until the
/// description resolves and its info burst completes — and consumes (destroys)
/// the passed description.
fn read_description_targets(
    desc: WpImageDescriptionV1,
    qh: &QueueHandle<State>,
    event_queue: &mut EventQueue<State>,
    state: &mut State,
    debug_label: &str,
) -> CompositorColorTargets {
    let debug = std::env::var("DAMASCENE_COLOR_DEBUG").is_ok();
    let pending = Arc::new(PendingDescription::default());
    state.pending = Some(Arc::clone(&pending));

    // Wait for the description to resolve (ready / failed) before asking
    // for its information — `get_information` on a failed description is a
    // protocol error.
    while pending.lock().is_none() {
        if event_queue.roundtrip(state).is_err() {
            state.pending = None;
            desc.destroy();
            if debug {
                eprintln!("damascene color: {debug_label} description: wire error during resolve");
            }
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
            let t = std::mem::take(&mut state.info);
            if debug {
                eprintln!(
                    "damascene color: {debug_label} description ready — tf={:?} primaries={:?} \
                     ref_white={:?} target_peak={:?} icc={}",
                    t.preferred_transfer,
                    t.preferred_primaries,
                    t.reference_luminance_nits,
                    t.target_max_luminance_nits,
                    t.preferred_is_icc,
                );
            }
            t
        }
        Some(DescriptionResolution::Failed { cause, msg }) => {
            if debug {
                eprintln!(
                    "damascene color: {debug_label} description failed — cause={cause} msg={msg:?}"
                );
            }
            CompositorColorTargets::default()
        }
        // Unreachable: the loop above only exits on a resolution or a wire
        // error (handled). Treated as no usable hint.
        None => CompositorColorTargets::default(),
    };

    desc.destroy();
    targets
}

/// Read every connected output's `wp_color_management_output_v1` image
/// description and aggregate them into an [`OutputColorCapability`] — the
/// "any HDR output" heuristic that lets HDR engage on compositors which
/// SDR-clamp the per-surface preferred description.
///
/// Each output's description is parsed with the same path as the surface
/// preferred one, then run through [`CompositorColorTargets::indicates_hdr`]
/// (a PQ / HLG transfer or peak ≥1.5× reference reads as HDR). Returns
/// `None` when there are no outputs to read; `Some` with `indicates_hdr =
/// false` when outputs were read but none are HDR — so the caller can tell
/// "saw SDR outputs" from "saw no output color descriptions". When more than
/// one output is HDR, the highest advertised peak is carried.
fn read_output_capability(
    output_managers: &[WpColorManagementOutputV1],
    qh: &QueueHandle<State>,
    event_queue: &mut EventQueue<State>,
    state: &mut State,
) -> Option<OutputColorCapability> {
    if output_managers.is_empty() {
        return None;
    }
    if std::env::var("DAMASCENE_COLOR_DEBUG").is_ok() {
        eprintln!(
            "damascene color: reading {} output description(s) (color-mgmt output v{})",
            output_managers.len(),
            output_managers.first().map(|o| o.version()).unwrap_or(0),
        );
    }
    let mut cap = OutputColorCapability::default();
    for (i, cmo) in output_managers.iter().enumerate() {
        let desc = cmo.get_image_description(qh, ());
        let label = format!("output[{i}]");
        let targets = read_description_targets(desc, qh, event_queue, state, &label);
        if !targets.indicates_hdr() {
            continue;
        }
        cap.indicates_hdr = true;
        // Carry the most-capable output's peak (and its paired reference).
        let peak = targets.target_max_luminance_nits.unwrap_or(0.0);
        if peak >= cap.target_max_luminance_nits.unwrap_or(0.0) {
            cap.target_max_luminance_nits =
                targets.target_max_luminance_nits.or(cap.target_max_luminance_nits);
            cap.reference_luminance_nits =
                targets.reference_luminance_nits.or(cap.reference_luminance_nits);
        }
    }
    Some(cap)
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
    /// Set by `preferred_changed` / `preferred_changed2` on the feedback
    /// object; consumed by [`WaylandColorManager::poll`], which re-reads
    /// the preferred description when it's up.
    preferred_dirty: bool,
    /// Set by `image_description_changed` on any output's color-management
    /// object (an output toggling its HDR config); consumed by
    /// [`WaylandColorManager::poll`], which re-scans the output capability.
    outputs_dirty: bool,
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
    /// The description failed to form — carries the `cause` enum name and
    /// the compositor's ad-hoc message for diagnostics (KWin's output
    /// descriptions failing is exactly what we're chasing for #113).
    Failed { cause: String, msg: String },
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
                // Damascene always requests `Perceptual` when applying; the
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
            Event::Failed { cause, msg } => {
                use wayland_client::WEnum;
                let mut guard = slot.lock();
                if guard.is_none() {
                    let cause = match cause {
                        WEnum::Value(c) => format!("{c:?}"),
                        WEnum::Unknown(v) => format!("unknown({v})"),
                    };
                    *guard = Some(DescriptionResolution::Failed { cause, msg });
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WpColorManagementSurfaceFeedbackV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpColorManagementSurfaceFeedbackV1,
        _: <WpColorManagementSurfaceFeedbackV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The feedback interface has exactly two events —
        // `preferred_changed` (v1) and `preferred_changed2` (v2, adds the
        // description identity). Both mean the same thing for us: the
        // preferred description is stale, re-read it. We don't compare
        // identities — `poll` coalesces any burst into one re-read.
        state.preferred_dirty = true;
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: <WlOutput as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // We bind `wl_output`s only to create their color-management objects;
        // their geometry/mode/name events aren't load-bearing for HDR gating.
    }
}

impl Dispatch<WpColorManagementOutputV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpColorManagementOutputV1,
        event: <WpColorManagementOutputV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The only event is `image_description_changed` — the output's HDR
        // configuration changed (e.g. the user toggled HDR). Mark the output
        // capability stale; `poll` re-scans and re-negotiates.
        if matches!(
            event,
            wp_color_management_output_v1::Event::ImageDescriptionChanged
        ) {
            state.outputs_dirty = true;
        }
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
// Enum mapping damascene_core::color <-> wp_color_management_v1
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
        // Compositor named primaries damascene doesn't model (PAL, NTSC,
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
