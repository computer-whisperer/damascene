//! Optional desktop host for running [`App`]s against a real `wgpu`
//! surface in a `winit` window.
//!
//! Most native apps should use this crate instead of calling
//! `damascene-wgpu` directly:
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
//!     damascene_winit_wgpu::run("My Damascene App", viewport, MyApp::default())
//! }
//! ```
//!
//! The host owns the event loop, window, device/queue, surface
//! configuration, render pass boundaries, input mapping, IME forwarding,
//! and animation redraw cadence. Your code owns the [`App`]: application
//! state, [`App::build`], [`App::on_event`], optional hotkeys, custom
//! shaders, and theme.
//!
//! [`run`] takes an [`App`] and runs an event loop that:
//!
//! - Calls [`App::build`] on every redraw, applying current hover/press
//!   visuals automatically before paint.
//! - Routes `winit` pointer events through the renderer's hit-tester
//!   and dispatches events back via [`App::on_event`].
//! - Routes Tab/Shift-Tab through focus traversal and Enter/Space/Escape
//!   through keyboard events.
//! - Copies the current Damascene text selection to the native clipboard
//!   on Ctrl/Cmd+C.
//! - Requests a redraw whenever interaction state changes (mouse move,
//!   button down/up) so hover/press visuals are immediate.
//!
//! Use [`run_with_config`] when an app has external live state. Put
//! per-frame state refresh in [`App::before_build`], then pick the
//! redraw driver that matches the data (see the README's meter-class
//! vs event-class discussion): a fixed cadence via
//! [`HostConfig::with_redraw_interval`] for continuously-changing
//! meters, or push-driven wakes via
//! [`HostConfig::with_external_wakeup`] for sparse events, so the
//! idle app renders at 0 fps. For fully custom render-loop
//! integration, bypass this crate and call `damascene_wgpu::Runner`
//! directly.
//!
//! # Environment variables
//!
//! - `DAMASCENE_COLOR_DEBUG=1` — dump the color negotiation to stderr:
//!   the surface formats the WSI advertises, the compositor's
//!   capabilities, the preferred-description targets (reference white,
//!   display peak, `indicates_hdr`), and the swapchain format the
//!   ladder settled on; re-dumped on every `preferred_changed2`
//!   re-negotiation. The first stop for "why didn't I get HDR?" —
//!   see `docs/COLOR_MANAGEMENT.md`. Apps query the same state at
//!   runtime via `HostDiagnostics::hdr_active()`.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use damascene_core::color::ColorPreferences;
use damascene_core::widgets::text_input::{self, ClipboardKind};
use damascene_core::{
    App, Cursor, FrameTrigger, HostDiagnostics, KeyModifiers, LogicalKey, PhysicalKey, Pointer,
    PointerButton, Rect, Sides, UiEvent, UiEventKind, clipboard,
};
use damascene_wgpu::Runner;

pub mod host;
#[cfg(all(target_os = "linux", feature = "wayland-color-management"))]
mod wayland_color;

use host::input::{
    key_modifiers, map_key, map_physical, pointer_button, touch_pressure, winit_cursor,
};

const DEFAULT_SAMPLE_COUNT: u32 = 4;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
type PlatformClipboard = Option<arboard::Clipboard>;
#[cfg(target_os = "android")]
struct PlatformClipboard {
    app: AndroidApp,
}
#[cfg(target_os = "ios")]
#[derive(Default)]
struct PlatformClipboard;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
#[cfg(target_os = "android")]
use winit::platform::android::{EventLoopExtAndroid, WindowExtAndroid, activity::AndroidApp};
use winit::window::{Window, WindowId};

/// `Send + Clone` handle that wakes the running host loop from any
/// thread and schedules one redraw.
///
/// This is the push path for **event-class** live data (see the crate
/// README): application code that learns about a change off the UI
/// thread — a message on a channel, a background task advancing state —
/// calls [`Wakeup::wake`] and the host builds + renders one frame.
/// Between wakes the host sits fully idle; no polling cadence required.
///
/// Obtain one via [`HostConfig::with_external_wakeup`].
#[derive(Clone, Debug)]
pub struct Wakeup {
    proxy: winit::event_loop::EventLoopProxy<()>,
}

impl Wakeup {
    /// Ask the host loop to build + render one frame.
    ///
    /// Safe to call from any thread, before the first frame, and after
    /// the loop has exited (then it's a no-op). Wakes coalesce: any
    /// number of calls before the next frame produce a single redraw,
    /// so callers don't need their own burst-collapsing — though
    /// deciding *which* events warrant a frame stays on the app side.
    ///
    /// The resulting frame takes the full path (rebuild + layout +
    /// paint), since the host must assume app data changed.
    pub fn wake(&self) {
        let _ = self.proxy.send_event(());
    }
}

/// External-wakeup hook stored in [`HostConfig`]. Wraps the closure so
/// `HostConfig` can keep deriving `Clone` and `Debug`.
#[derive(Clone)]
pub struct WakeupHook(Arc<dyn Fn(Wakeup) + Send + Sync>);

impl std::fmt::Debug for WakeupHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WakeupHook(..)")
    }
}

/// Configuration for the optional native winit + wgpu host.
#[derive(Clone, Debug)]
pub struct HostConfig {
    /// MSAA sample count used for Damascene's SDF surfaces. The default is
    /// 4, matching the demo and validation app paths.
    pub sample_count: u32,
    /// Optional fixed redraw cadence for apps with external live data
    /// sources such as audio meters. Animation-driven redraws still
    /// come from `Runner::prepare().needs_redraw`; this is only for
    /// host-owned clocks.
    pub redraw_interval: Option<Duration>,
    /// Prefer the lowest-latency wgpu present mode the surface
    /// advertises (`Mailbox`, falling back to `Fifo`). Default is
    /// `Fifo`, which is vsync-locked and conservative on power.
    ///
    /// Why this exists: with `Fifo`, every submit queues a frame for
    /// the next vsync; if the app submits faster than the display
    /// refresh, the compositor pulls the *oldest* queued frame at
    /// each vsync. On Wayland/Mesa during an interactive resize this
    /// shows up as the window content trailing the cursor in slow
    /// motion — by the time the latest size we rendered reaches the
    /// screen, several more compositor `configure` events have
    /// arrived. `Mailbox` replaces the pending frame on each submit,
    /// so the next vsync always shows the most recent render.
    ///
    /// Cost: with `Mailbox`, render cadence is no longer naturally
    /// vsync-bounded — an animation that calls `request_redraw` from
    /// `prepare.needs_redraw` will render at GPU speed. Pair this
    /// with `redraw_interval` (or accept the cycles) if that's not
    /// what you want.
    pub low_latency_present: bool,
    /// Stable identifier used by the windowing system / compositor /
    /// desktop services to group windows under this application.
    ///
    /// - **Wayland**: sets `xdg_toplevel.app_id`. Should match the
    ///   basename of the `.desktop` file the app ships (reverse-DNS
    ///   by convention, e.g. `com.example.MyApp`).
    /// - **X11**: sets both fields of `WM_CLASS` to the same value.
    /// - **Windows / macOS / mobile**: ignored.
    ///
    /// When `None`, windowing-system defaults apply — typically the
    /// process name on Wayland, which several compositors render as
    /// a generic placeholder (e.g. `surface-transient`) in their
    /// config UIs and XDG-portal-backed system dialogs.
    pub app_id: Option<String>,
    /// App's color-space preferences.
    ///
    /// **Mostly advisory.** We never attach an image description to the
    /// surface — per `wp_color_management_v1` a surface has a single
    /// color-management owner, and for an accelerated client that is the
    /// wgpu/Vulkan WSI, not us. We do read the compositor's color-management
    /// state (for the Color Management showcase page) and, on a genuinely
    /// HDR output, select an extended-range float swapchain (`Rgba16Float` →
    /// scRGB via the WSI) so `>1.0` values reach the display; SDR outputs
    /// stay on the 8-bit sRGB baseline. The default is
    /// `ColorPreferences::sdr_only()`.
    pub color_preferences: ColorPreferences,
    /// Hook invoked once with a [`Wakeup`] handle for the host loop,
    /// just before the loop starts. See
    /// [`HostConfig::with_external_wakeup`].
    pub external_wakeup: Option<WakeupHook>,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            sample_count: DEFAULT_SAMPLE_COUNT,
            redraw_interval: None,
            low_latency_present: false,
            app_id: None,
            color_preferences: ColorPreferences::default(),
            external_wakeup: None,
        }
    }
}

impl HostConfig {
    pub fn with_redraw_interval(mut self, interval: Duration) -> Self {
        self.redraw_interval = Some(interval);
        self
    }

    pub fn with_sample_count(mut self, sample_count: u32) -> Self {
        self.sample_count = sample_count.max(1);
        self
    }

    pub fn with_low_latency_present(mut self, low_latency_present: bool) -> Self {
        self.low_latency_present = low_latency_present;
        self
    }

    pub fn with_app_id(mut self, app_id: impl Into<String>) -> Self {
        self.app_id = Some(app_id.into());
        self
    }

    pub fn with_color_preferences(mut self, color_preferences: ColorPreferences) -> Self {
        self.color_preferences = color_preferences;
        self
    }

    /// Register a hook that receives a [`Wakeup`] handle for the host
    /// loop. The hook runs once on the UI thread, just before the
    /// event loop starts; hand the handle to whatever owns your
    /// event-class data source.
    ///
    /// This is the push-driven complement to
    /// [`with_redraw_interval`](Self::with_redraw_interval): instead of
    /// the host polling on a fixed clock, app code schedules a frame
    /// exactly when something changed, and the idle app renders at
    /// 0 fps. The two compose — a fixed cadence for meter-class data
    /// and pushed wakes for event-class data don't conflict — but most
    /// apps with conditional meters are better served by
    /// `redraw_within` on the meter widget plus this hook for events.
    ///
    /// ```no_run
    /// use damascene_winit_wgpu::HostConfig;
    ///
    /// let (tx, rx) = std::sync::mpsc::channel();
    /// let config = HostConfig::default().with_external_wakeup(move |wakeup| {
    ///     let _ = tx.send(wakeup);
    /// });
    /// // A backend thread receives the handle and pokes the UI per event:
    /// std::thread::spawn(move || {
    ///     let wakeup = rx.recv().unwrap();
    ///     // for each interesting backend event:
    ///     wakeup.wake();
    /// });
    /// ```
    pub fn with_external_wakeup(mut self, hook: impl Fn(Wakeup) + Send + Sync + 'static) -> Self {
        self.external_wakeup = Some(WakeupHook(Arc::new(hook)));
        self
    }
}

/// Compatibility extension point for apps that use this host crate.
///
/// New apps should prefer [`App::before_build`]. This trait remains for
/// code that wants to name a winit-host-specific app type while still
/// using the same core lifecycle, and as a place to hang wgpu-specific
/// hooks that the backend-neutral [`App`] trait can't carry — see
/// [`Self::gpu_setup`] and [`Self::before_paint`].
pub trait WinitWgpuApp: App {
    fn before_build(&mut self) {
        App::before_build(self);
    }

    /// Called once after the host has created its `wgpu::Device` and
    /// before the first frame is drawn. Apps that need to allocate
    /// app-owned GPU textures (typically for use with
    /// [`damascene_core::surface::AppTexture`] / `surface()` widgets)
    /// initialize them here.
    ///
    /// Default: no-op. App authors who don't touch wgpu directly can
    /// ignore this hook.
    fn gpu_setup(&mut self, _device: &wgpu::Device, _queue: &wgpu::Queue) {}

    /// Called each frame just before [`App::build`] runs. Apps update
    /// their app-owned GPU textures here — typically by
    /// `queue.write_texture(...)` of the next animation frame so the
    /// composite the runner draws this frame samples fresh pixels.
    ///
    /// Default: no-op.
    fn before_paint(&mut self, _queue: &wgpu::Queue) {}
}

struct BasicApp<A>(A);

impl<A: App> App for BasicApp<A> {
    fn before_build(&mut self) {
        self.0.before_build();
    }

    fn build(&self, cx: &damascene_core::BuildCx) -> damascene_core::El {
        self.0.build(cx)
    }

    fn on_event(&mut self, event: damascene_core::UiEvent, cx: &damascene_core::EventCx) {
        self.0.on_event(event, cx);
    }

    fn on_wheel_event(
        &mut self,
        event: damascene_core::UiEvent,
        cx: &damascene_core::EventCx,
    ) -> bool {
        self.0.on_wheel_event(event, cx)
    }

    fn hotkeys(&self) -> Vec<(damascene_core::KeyChord, String)> {
        self.0.hotkeys()
    }

    fn drain_toasts(&mut self) -> Vec<damascene_core::toast::ToastSpec> {
        self.0.drain_toasts()
    }

    fn drain_announcements(&mut self) -> Vec<damascene_core::announce::Announcement> {
        self.0.drain_announcements()
    }

    fn drain_focus_requests(&mut self) -> Vec<String> {
        self.0.drain_focus_requests()
    }

    fn drain_scroll_requests(&mut self) -> Vec<damascene_core::scroll::ScrollRequest> {
        self.0.drain_scroll_requests()
    }

    fn drain_viewport_requests(&mut self) -> Vec<damascene_core::viewport::ViewportRequest> {
        self.0.drain_viewport_requests()
    }

    fn drain_plot_requests(&mut self) -> Vec<damascene_core::plot::PlotRequest> {
        self.0.drain_plot_requests()
    }

    fn drain_link_opens(&mut self) -> Vec<String> {
        self.0.drain_link_opens()
    }

    fn shaders(&self) -> Vec<damascene_core::AppShader> {
        self.0.shaders()
    }

    fn theme(&self) -> damascene_core::Theme {
        self.0.theme()
    }

    fn selection(&self) -> damascene_core::Selection {
        self.0.selection()
    }
}

impl<A: App> WinitWgpuApp for BasicApp<A> {}

/// Run a windowed app. Blocks until the user closes the window.
///
/// The `App` is owned by the runner; its `&mut self` is updated in
/// response to routed events and read on every `build` call.
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, BasicApp(app), HostConfig::default())
}

/// Run a windowed app with host-specific configuration.
///
/// Use this when a plain [`App`] wants a host cadence
/// (`redraw_interval`) or non-default MSAA. For fully custom
/// render-loop integration, bypass this crate and call
/// `damascene_wgpu::Runner` directly.
pub fn run_with_config<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, BasicApp(app), config)
}

/// Run a plain [`App`] using a caller-created winit event loop.
///
/// This is primarily for platform hosts that need to configure the
/// event loop before Damascene owns it. Android, for example, must attach
/// the `AndroidApp` received by `android_main` before `build()`.
pub fn run_on_event_loop<A: App + 'static>(
    event_loop: EventLoop<()>,
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host_on_event_loop(event_loop, title, viewport, BasicApp(app), config)
}

/// Run a windowed app with host-specific configuration.
///
/// Prefer [`run_with_config`] for new apps; [`App::before_build`] is
/// available there as well.
pub fn run_host_app_with_config<A: WinitWgpuApp + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, app, config)
}

/// Run a host-specific [`WinitWgpuApp`] using a caller-created winit
/// event loop.
pub fn run_host_app_on_event_loop<A: WinitWgpuApp + 'static>(
    event_loop: EventLoop<()>,
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host_on_event_loop(event_loop, title, viewport, app, config)
}

/// Run a windowed app with default host configuration.
///
/// Prefer [`run`] for new apps; [`App::before_build`] is available
/// there as well.
pub fn run_host_app<A: WinitWgpuApp + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_host(title, viewport, app, HostConfig::default())
}

fn run_host<A: WinitWgpuApp + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    run_host_on_event_loop(event_loop, title, viewport, app, config)
}

fn run_host_on_event_loop<A: WinitWgpuApp + 'static>(
    event_loop: EventLoop<()>,
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
    // Hand out the external-wakeup handle before the loop starts so
    // app threads can wake it from frame zero. Wakes that land before
    // the surface exists are covered by `resumed`'s initial redraw.
    if let Some(WakeupHook(hook)) = config.external_wakeup.as_ref() {
        hook(Wakeup {
            proxy: event_loop.create_proxy(),
        });
    }
    #[cfg(target_os = "android")]
    let android_app = event_loop.android_app().clone();
    #[cfg(not(target_os = "android"))]
    let clipboard = new_clipboard();
    #[cfg(target_os = "android")]
    let clipboard = new_clipboard(&android_app);
    let mut host = Host {
        title,
        viewport,
        config,
        app,
        #[cfg(target_os = "android")]
        android_app,
        instance: None,
        gfx: None,
        #[cfg(feature = "accessibility")]
        a11y: None,
        env_a11y_prefs: Default::default(),
        setup_error: None,
        last_pointer: None,
        modifiers: KeyModifiers::default(),
        next_periodic_redraw: None,
        last_cursor: Cursor::Default,
        #[cfg(any(target_os = "android", target_os = "ios"))]
        ime_allowed: false,
        pending_resize: None,
        next_layout_redraw: None,
        next_paint_redraw: None,
        next_trigger: FrameTrigger::Initial,
        last_frame_at: None,
        last_build: Duration::ZERO,
        last_prepare: Duration::ZERO,
        last_layout: Duration::ZERO,
        last_layout_intrinsic_cache_hits: 0,
        last_layout_intrinsic_cache_misses: 0,
        last_layout_pruned_subtrees: 0,
        last_layout_pruned_nodes: 0,
        last_draw_ops: Duration::ZERO,
        last_draw_ops_culled_text_ops: 0,
        last_paint: Duration::ZERO,
        last_paint_culled_ops: 0,
        last_gpu_upload: Duration::ZERO,
        last_snapshot: Duration::ZERO,
        last_submit: Duration::ZERO,
        last_text_layout_cache_hits: 0,
        last_text_layout_cache_misses: 0,
        last_text_layout_cache_evictions: 0,
        last_text_layout_shaped_bytes: 0,
        frame_index: 0,
        backend: "?",
        clipboard,
        last_primary: String::new(),
        last_diagnostics: None,
    };
    event_loop.run_app(&mut host)?;
    // GPU setup happens lazily inside `resumed()`, which cannot return
    // an error through winit — it records the failure and exits the
    // loop instead. Surface it to the caller here.
    if let Some(message) = host.setup_error {
        return Err(message.into());
    }
    Ok(())
}

struct Host<A: WinitWgpuApp> {
    title: &'static str,
    viewport: Rect,
    config: HostConfig,
    app: A,
    #[cfg(target_os = "android")]
    android_app: AndroidApp,
    /// The wgpu instance the surface was created from, retained so
    /// the redraw path can recreate the surface when an acquire
    /// reports `Lost` (wgpu's contract: `create_surface()` then
    /// `configure()`). `None` until the first `resumed()`.
    instance: Option<wgpu::Instance>,
    gfx: Option<host::WindowGfx>,
    /// AccessKit adapter + shared handler state. `None` until
    /// `resumed()` creates the window (and again after Android
    /// suspend drops it).
    #[cfg(feature = "accessibility")]
    a11y: Option<HostA11y>,
    /// Env-derived base accessibility preferences, cached at resume so
    /// per-frame `screen_reader_active` flips re-push without
    /// re-reading the environment.
    env_a11y_prefs: damascene_core::a11y::AccessibilityPreferences,
    /// Fatal GPU-setup failure recorded by `resumed()`. Adapter and
    /// device acquisition legitimately fail on real platforms (no
    /// Vulkan driver on a GLES-only Android device, no GPU in a
    /// container, …) — `resumed` can't return an error through winit,
    /// so it records the message here and exits the loop;
    /// `run_host_on_event_loop` converts it into the `Err` that
    /// `run()` callers see.
    setup_error: Option<String>,
    /// Last pointer position in logical pixels (winit reports physical;
    /// we divide by the window's scale factor before storing).
    last_pointer: Option<(f32, f32)>,
    modifiers: KeyModifiers,
    next_periodic_redraw: Option<Instant>,
    /// Last cursor pushed to `Window::set_cursor`. Avoids redundant
    /// per-frame calls when the resolved cursor hasn't changed —
    /// `set_cursor` is cheap but goes through a syscall on most
    /// platforms.
    last_cursor: Cursor,
    /// Last Android soft-keyboard visibility state mirrored from
    /// `Runner::focused_captures_keys`.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    ime_allowed: bool,
    /// Latest size from `WindowEvent::Resized` not yet applied to the
    /// surface. Compositors (Wayland especially) deliver a burst of
    /// resize events during an interactive drag; coalescing them so
    /// `surface.configure()` + MSAA realloc run once per frame
    /// instead of once per event keeps the window content from
    /// trailing the cursor.
    pending_resize: Option<PhysicalSize<u32>>,
    /// Wall-clock deadline for the next redraw that needs a full
    /// rebuild + layout pass — animations settling, widget
    /// `redraw_within` requests, pending tooltip / toast fades.
    /// Derived from `prepare.next_layout_redraw_in`. `None` means no
    /// layout-driven future frame is pending. Cleared after firing.
    next_layout_redraw: Option<Instant>,
    /// Wall-clock deadline for the next paint-only redraw — a
    /// time-driven shader (spinner / skeleton / progress / custom
    /// `samples_time=true`) needs another frame but layout state is
    /// unchanged. Serviced via `Renderer::repaint`, which reuses the
    /// cached ops and only advances `frame.time`. Derived from
    /// `prepare.next_paint_redraw_in`. Cleared after firing.
    next_paint_redraw: Option<Instant>,
    /// Reason the next redraw is being requested. Each event handler
    /// that calls `request_redraw` sets this beforehand; RedrawRequested
    /// consumes it and resets to `Other`. Drives [`HostDiagnostics::trigger`]
    /// for apps that surface a debug overlay.
    next_trigger: FrameTrigger,
    /// Wall clock at the start of the previous redraw. Diff with the
    /// next frame's start gives `last_frame_dt`.
    last_frame_at: Option<Instant>,
    /// Timing breakdown from the last completed rendered frame.
    last_build: Duration,
    last_prepare: Duration,
    last_layout: Duration,
    last_layout_intrinsic_cache_hits: u64,
    last_layout_intrinsic_cache_misses: u64,
    last_layout_pruned_subtrees: u64,
    last_layout_pruned_nodes: u64,
    last_draw_ops: Duration,
    last_draw_ops_culled_text_ops: u64,
    last_paint: Duration,
    last_paint_culled_ops: u64,
    last_gpu_upload: Duration,
    last_snapshot: Duration,
    last_submit: Duration,
    last_text_layout_cache_hits: u64,
    last_text_layout_cache_misses: u64,
    last_text_layout_cache_evictions: u64,
    last_text_layout_shaped_bytes: u64,
    /// Counts redraws actually rendered (not requested). Surfaced via
    /// [`HostDiagnostics::frame_index`].
    frame_index: u64,
    /// Adapter backend tag (`"Vulkan"`, `"Metal"`, `"DX12"`, `"GL"`,
    /// `"WebGPU"`). Captured once at adapter selection and surfaced in
    /// the diagnostic overlay.
    backend: &'static str,
    /// Best-effort native clipboard. Initialization can fail in
    /// display-less/headless environments; the host simply leaves copy
    /// shortcuts as no-ops in that case.
    clipboard: PlatformClipboard,
    /// Last text mirrored into Linux's primary selection.
    last_primary: String,
    /// Diagnostics snapshot from the last built frame, retained so
    /// event dispatch can attach it to [`damascene_core::EventCx`] —
    /// handlers branch on negotiated output state (HDR, working color
    /// space) without mirroring it through app state.
    last_diagnostics: Option<damascene_core::HostDiagnostics>,
}

#[cfg(target_os = "android")]
fn safe_area_for_window(window: &Window, surface_size: (u32, u32), scale_factor: f32) -> Sides {
    let rect = window.content_rect();
    if rect.right <= rect.left || rect.bottom <= rect.top || scale_factor <= 0.0 {
        return Sides::default();
    }
    let (surface_w, surface_h) = (surface_size.0 as i32, surface_size.1 as i32);
    Sides {
        left: rect.left.max(0) as f32 / scale_factor,
        top: rect.top.max(0) as f32 / scale_factor,
        right: (surface_w - rect.right).max(0) as f32 / scale_factor,
        bottom: (surface_h - rect.bottom).max(0) as f32 / scale_factor,
    }
}

#[cfg(not(target_os = "android"))]
fn safe_area_for_window(_window: &Window, _surface_size: (u32, u32), _scale_factor: f32) -> Sides {
    Sides::default()
}

/// AccessKit integration state: the platform adapter plus the shared
/// flags its `Send` handlers write from whatever thread the platform
/// calls them on (Windows UIA notably calls from its own thread).
/// Created with the window in `resumed()`; dropped with it on Android
/// suspend so a fresh adapter binds the recreated window.
#[cfg(feature = "accessibility")]
struct HostA11y {
    #[cfg(not(target_os = "android"))]
    adapter: accesskit_winit::Adapter,
    /// accesskit_winit's Android backend binds GameActivity's
    /// `mSurfaceView` and this host runs in a NativeActivity, so
    /// Android drives `accesskit_android` directly — see [`android_a11y`].
    #[cfg(target_os = "android")]
    adapter: android_a11y::Adapter,
    /// An assistive technology requested the tree and has not
    /// deactivated since. Gates the per-frame lowering so idle frames
    /// pay nothing.
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Action requests queued by the platform handler, drained at the
    /// top of each redraw and routed through the runner.
    actions: std::sync::Arc<std::sync::Mutex<Vec<damascene_core::accesskit::ActionRequest>>>,
    /// Last activation state pushed into the accessibility
    /// preferences, so `screen_reader_active` re-pushes only on flips.
    reported_active: Option<bool>,
}

#[cfg(feature = "accessibility")]
mod a11y_handlers {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    #[cfg(not(target_os = "android"))]
    use damascene_core::accesskit::DeactivationHandler;
    use damascene_core::accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
    use winit::window::Window;

    /// Marks the tree requested and wakes the loop. Returning `None`
    /// is the contract's async path: the next redraw pushes a full
    /// tree via `Adapter::update_if_active` (the platform adapter
    /// serves a placeholder until then). Building the real tree here
    /// is impossible — this can run off the main thread, away from
    /// the runner.
    pub(super) struct Activation {
        pub active: Arc<AtomicBool>,
        pub window: Arc<Window>,
    }
    impl ActivationHandler for Activation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            self.active.store(true, Ordering::Relaxed);
            self.window.request_redraw();
            None
        }
    }

    pub(super) struct Actions {
        pub queue: Arc<Mutex<Vec<ActionRequest>>>,
        pub window: Arc<Window>,
    }
    impl ActionHandler for Actions {
        fn do_action(&mut self, request: ActionRequest) {
            self.queue.lock().unwrap().push(request);
            self.window.request_redraw();
        }
    }

    /// Not constructed on Android: `accesskit_android` has no
    /// deactivation callback, so `active` latches on until suspend
    /// drops the adapter (worst case: idle tree lowering continues
    /// after the AT disconnects, until the next suspend).
    #[cfg(not(target_os = "android"))]
    pub(super) struct Deactivation {
        pub active: Arc<AtomicBool>,
    }
    #[cfg(not(target_os = "android"))]
    impl DeactivationHandler for Deactivation {
        fn deactivate_accessibility(&mut self) {
            self.active.store(false, Ordering::Relaxed);
        }
    }
}

/// Android AccessKit adapter for this crate's `NativeActivity` host.
///
/// `accesskit_winit`'s Android backend is hard-wired to GameActivity —
/// it reads the activity's `mSurfaceView` field and panics on anything
/// else — while this host runs inside `NativeActivity` (see
/// `damascene-android`). `accesskit_android`'s `InjectingAdapter` itself is
/// view-generic: it installs the AccessKit accessibility delegate
/// (loaded from a dex embedded in the crate, so the APK needs no
/// Java-side changes) on any `View`. We resolve NativeActivity's
/// content view over JNI and drive it directly.
///
/// All JNI types here come from `accesskit_android`'s own `jni`
/// re-export — the host's direct `jni` dep (clipboard/link bridge)
/// tracks a different major and the types don't mix.
#[cfg(all(feature = "accessibility", target_os = "android"))]
mod android_a11y {
    use accesskit_android::InjectingAdapter;
    use accesskit_android::jni::{
        JNIEnv, JavaVM,
        errors::{Error as JniError, Result as JniResult},
        objects::{JObject, JValue},
    };
    use winit::event::WindowEvent;
    use winit::platform::android::activity::AndroidApp;
    use winit::window::Window;

    pub(super) struct Adapter {
        inner: InjectingAdapter,
    }

    impl Adapter {
        /// Wire AccessKit into the activity's content view. Returns
        /// `None` (logged) when the JNI wiring fails — accessibility
        /// degrades to absent rather than killing the app, the same
        /// policy as GPU-setup degradation elsewhere in this host.
        pub(super) fn new(
            app: &AndroidApp,
            activation: super::a11y_handlers::Activation,
            actions: super::a11y_handlers::Actions,
        ) -> Option<Self> {
            match Self::try_new(app, activation, actions) {
                Ok(adapter) => Some(adapter),
                Err(err) => {
                    log::warn!(
                        "damascene-winit-wgpu: AccessKit unavailable — could not bind \
                         the NativeActivity content view: {err}"
                    );
                    None
                }
            }
        }

        fn try_new(
            app: &AndroidApp,
            activation: super::a11y_handlers::Activation,
            actions: super::a11y_handlers::Actions,
        ) -> JniResult<Self> {
            let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
            // android_main runs on its own native thread; make sure it
            // is attached before any JNI call (a no-op when
            // android-activity already attached it). The adapter's
            // later calls happen on this same thread.
            let mut env = vm.attach_current_thread_permanently()?;
            let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
            let view = Self::content_view(&mut env, &activity)?;
            let inner = InjectingAdapter::new(&mut env, &view, activation, actions);
            Ok(Self { inner })
        }

        /// NativeActivity's `onCreate` installs a single focused
        /// `NativeContentView` as the content child; that view spans
        /// the window and holds input focus, so it hosts the virtual
        /// tree (the analog of GameActivity's surface view). Falls
        /// back to the content frame itself if the child is missing.
        fn content_view<'e>(env: &mut JNIEnv<'e>, activity: &JObject) -> JniResult<JObject<'e>> {
            // android.R.id.content — public and stable since API 1.
            const ANDROID_R_ID_CONTENT: i32 = 0x0102_0002;
            let content = env
                .call_method(
                    activity,
                    "findViewById",
                    "(I)Landroid/view/View;",
                    &[JValue::Int(ANDROID_R_ID_CONTENT)],
                )?
                .l()?;
            if content.is_null() {
                return Err(JniError::NullPtr("android.R.id.content view"));
            }
            let child = env
                .call_method(
                    &content,
                    "getChildAt",
                    "(I)Landroid/view/View;",
                    &[JValue::Int(0)],
                )?
                .l()?;
            Ok(if child.is_null() { content } else { child })
        }

        pub(super) fn update_if_active(
            &mut self,
            updater: impl FnOnce() -> damascene_core::accesskit::TreeUpdate,
        ) {
            self.inner.update_if_active(updater);
        }

        /// Signature parity with `accesskit_winit::Adapter`; Android's
        /// adapter needs no winit window events.
        pub(super) fn process_event(&mut self, _window: &Window, _event: &WindowEvent) {}
    }
}

/// Read accessibility-preference overrides from the environment — the
/// interim host-side detection until native platform sniffing lands
/// (the XDG-portal read arrives with the AccessKit arc; see
/// `docs/ACCESSIBILITY_PLAN.md`). Unset variables leave the
/// corresponding preference unknown, so a future platform reader can
/// fill them without fighting the override.
///
/// - `DAMASCENE_REDUCED_MOTION` / `DAMASCENE_REDUCED_TRANSPARENCY` —
///   truthy (`1` / `true` / `reduce`) reports the preference on; falsy
///   (`0` / `false` / `no-preference`) reports it explicitly off.
/// - `DAMASCENE_COLOR_SCHEME` — `dark` / `light`.
/// - `DAMASCENE_CONTRAST` — `more` (alias `high`) / `less` (alias `low`).
fn accessibility_preferences_from_env() -> damascene_core::a11y::AccessibilityPreferences {
    use damascene_core::a11y::{AccessibilityPreferences, ColorScheme, Contrast};
    fn norm(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
    }
    fn flag(name: &str) -> Option<bool> {
        norm(name).map(|v| !matches!(v.as_str(), "" | "0" | "false" | "no" | "no-preference"))
    }
    AccessibilityPreferences {
        reduced_motion: flag("DAMASCENE_REDUCED_MOTION"),
        color_scheme: norm("DAMASCENE_COLOR_SCHEME").and_then(|v| match v.as_str() {
            "dark" => Some(ColorScheme::Dark),
            "light" => Some(ColorScheme::Light),
            _ => None,
        }),
        contrast: norm("DAMASCENE_CONTRAST").and_then(|v| match v.as_str() {
            "more" | "high" => Some(Contrast::More),
            "less" | "low" => Some(Contrast::Less),
            _ => None,
        }),
        reduced_transparency: flag("DAMASCENE_REDUCED_TRANSPARENCY"),
        // Testing override; when set it wins over the AccessKit
        // adapter's live activation state.
        screen_reader_active: flag("DAMASCENE_SCREEN_READER"),
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn sync_mobile_ime(window: &Window, renderer: &Runner, ime_allowed: &mut bool) {
    let allowed = renderer.focused_captures_keys();
    if allowed != *ime_allowed {
        window.set_ime_allowed(allowed);
        *ime_allowed = allowed;
    }
}

impl<A: WinitWgpuApp> Host<A> {
    /// Drive the live color-management driver: drain its wayland queue
    /// and, when the compositor changed this surface's preferred
    /// description (output move, HDR toggle), re-negotiate.
    ///
    /// Cheap in the steady state (one non-blocking `dispatch_pending`);
    /// only an actual change pays the description re-read — see
    /// [`host::color::SurfaceColor::poll`], which produces the
    /// [`host::color::Renegotiation`] plan this method applies. Two
    /// tiers of reaction:
    /// - **Targets changed, format holds** — refresh
    ///   [`HostDiagnostics::color_management`] and redraw so e.g. the
    ///   showcase's Color Management page tracks the move live.
    /// - **Negotiated format flips** (SDR ↔ HDR) — additionally
    ///   reconfigure the surface, rebuild the renderer's format-bound
    ///   pipelines in place (interaction state, atlases, and texture
    ///   caches survive — see `Runner::set_target_format`), refresh the
    ///   working space + white scale, and reallocate the MSAA target.
    fn poll_color_management(&mut self) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let Some(plan) = gfx.color.poll() else {
            return;
        };
        gfx.apply_renegotiation(&plan);
        self.next_trigger = FrameTrigger::External;
        gfx.window.request_redraw();
    }
}

impl<A: WinitWgpuApp> Host<A> {
    /// Record a fatal GPU-setup failure and stop the loop. The
    /// message is logged immediately (the only channel on Android,
    /// where there is no terminal — it lands in logcat) and returned
    /// as the `Err` of `run()` / `run_with_config` once the loop
    /// unwinds.
    fn fail_setup(&mut self, event_loop: &ActiveEventLoop, message: String) {
        log::error!("damascene-winit-wgpu: {message}");
        self.setup_error = Some(message);
        event_loop.exit();
    }
}

impl<A: WinitWgpuApp> ApplicationHandler for Host<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(PhysicalSize::new(
                self.viewport.w as u32,
                self.viewport.h as u32,
            ));
        #[cfg(target_os = "linux")]
        let attrs = if let Some(app_id) = self.config.app_id.as_deref() {
            // Fully-qualified — both extension traits define `with_name`.
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            let a = WindowAttributesExtWayland::with_name(attrs, app_id, "");
            WindowAttributesExtX11::with_name(a, app_id, app_id)
        } else {
            attrs
        };
        // accesskit_winit requires its adapter to exist before the
        // window first becomes visible; create hidden, wire the
        // adapter, then show.
        #[cfg(feature = "accessibility")]
        let attrs = attrs.with_visible(false);
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        #[cfg(feature = "accessibility")]
        {
            use std::sync::atomic::AtomicBool;
            use std::sync::{Arc, Mutex};
            let active = Arc::new(AtomicBool::new(false));
            let actions = Arc::new(Mutex::new(Vec::new()));
            #[cfg(not(target_os = "android"))]
            let adapter = Some(accesskit_winit::Adapter::with_direct_handlers(
                event_loop,
                &window,
                a11y_handlers::Activation {
                    active: active.clone(),
                    window: window.clone(),
                },
                a11y_handlers::Actions {
                    queue: actions.clone(),
                    window: window.clone(),
                },
                a11y_handlers::Deactivation {
                    active: active.clone(),
                },
            ));
            // Android: accesskit_android against NativeActivity's
            // content view; may degrade to None (logged) on JNI
            // failure, leaving the app running without accessibility.
            #[cfg(target_os = "android")]
            let adapter = android_a11y::Adapter::new(
                &self.android_app,
                a11y_handlers::Activation {
                    active: active.clone(),
                    window: window.clone(),
                },
                a11y_handlers::Actions {
                    queue: actions.clone(),
                    window: window.clone(),
                },
            );
            if let Some(adapter) = adapter {
                self.a11y = Some(HostA11y {
                    adapter,
                    active,
                    actions,
                    reported_active: None,
                });
            }
            window.set_visible(true);
        }

        // Adapter / device acquisition fails on real platforms — a
        // GLES-only Android device with no Vulkan driver, a container
        // or CI box with no GPU and no lavapipe, a denylisted driver.
        // Those are environment outcomes, not bugs: record + exit so
        // `run()` returns the error instead of panicking.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // Retained for surface recreation on acquire `Lost` (instances
        // are window-independent, so this survives Android
        // suspend/resume cycles; each resume overwrites it anyway).
        self.instance = Some(instance.clone());
        let surface = match instance.create_surface(window.clone()) {
            Ok(surface) => surface,
            Err(err) => {
                self.fail_setup(
                    event_loop,
                    format!("could not create a rendering surface for the window: {err}"),
                );
                return;
            }
        };

        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })) {
                Ok(adapter) => adapter,
                Err(err) => {
                    self.fail_setup(
                        event_loop,
                        format!(
                            "no compatible GPU adapter ({err}) — Damascene's native host needs a \
                         Vulkan, Metal, or DX12 driver (on a headless Linux box, installing \
                         lavapipe/llvmpipe provides a software Vulkan adapter; on Android the \
                         device must support Vulkan)"
                        ),
                    );
                    return;
                }
            };
        self.backend = backend_label(adapter.get_info().backend);

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("damascene_winit_wgpu::device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })) {
                Ok(pair) => pair,
                Err(err) => {
                    self.fail_setup(
                        event_loop,
                        format!("GPU device creation failed on the selected adapter: {err}"),
                    );
                    return;
                }
            };

        // Per-window GPU bring-up — surface config, color negotiation,
        // Runner construction, MSAA target. `with_surface` because the
        // surface above already anchored adapter selection; a custom
        // multi-window host calls `WindowGfx::new` for further windows
        // on the same device/queue.
        let mut gfx =
            host::WindowGfx::with_surface(&adapter, &device, &queue, window, surface, &self.config);
        gfx.renderer.set_theme(self.app.theme());
        self.env_a11y_prefs = accessibility_preferences_from_env();
        gfx.renderer
            .set_accessibility_preferences(self.env_a11y_prefs);
        // Register any custom shaders the app declared. Done once at
        // startup; pipelines are cached for the runner's lifetime.
        // Backdrop-sampling shaders need the surface to support the
        // mid-frame snapshot copy; when it doesn't (GLES surfaces
        // without COPY_SRC — issue #143), skip them so their draws
        // drop cleanly instead of splitting passes around a copy the
        // surface can't service.
        for s in self.app.shaders() {
            if s.samples_backdrop && !gfx.backdrop_capable() {
                log::warn!(
                    "damascene-winit-wgpu: skipping backdrop-sampling shader `{}` — surface \
                     lacks COPY_SRC, so the snapshot copy it samples cannot run",
                    s.name
                );
                continue;
            }
            gfx.renderer.register_shader_with(
                &device,
                s.name,
                s.wgsl,
                s.samples_backdrop,
                s.samples_time,
            );
        }
        self.gfx = Some(gfx);
        // Hand the app the device + queue so it can allocate any GPU
        // textures it intends to display via `surface()` widgets. Runs
        // whenever a host GPU context is created; on Android this can
        // happen again after Activity suspend/resume recreates the
        // native window.
        let gfx = self.gfx.as_ref().unwrap();
        self.app.gpu_setup(&gfx.device, &gfx.queue);
        self.next_periodic_redraw = self
            .config
            .redraw_interval
            .map(|interval| Instant::now() + interval);
        gfx.window.request_redraw();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "android")]
        {
            // Android destroys the native window while keeping the Rust
            // process alive. Any surface/window handles derived from
            // that native window must be dropped and recreated on the
            // next `resumed`, otherwise returning from Home can leave a
            // live process presenting to a dead surface.
            self.gfx.take();
            // The AccessKit adapter is bound to the destroyed window;
            // the next `resumed` creates a fresh one alongside it.
            #[cfg(feature = "accessibility")]
            {
                self.a11y = None;
            }
            self.pending_resize = None;
            self.last_pointer = None;
            self.last_frame_at = None;
            self.next_periodic_redraw = None;
            self.ime_allowed = false;
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // External wakeup (`Wakeup::wake`): app code reports that data
        // outside the tree changed, so the frame must take the full
        // rebuild + layout path — `about_to_wait` guards this trigger
        // against being downgraded to paint-only by a shader deadline
        // expiring on the same loop turn. If the surface isn't alive
        // yet (before the first `resumed`, or while suspended on
        // Android), drop the poke: `resumed` unconditionally requests
        // an initial redraw, which covers it.
        if let Some(gfx) = self.gfx.as_ref() {
            self.next_trigger = FrameTrigger::External;
            gfx.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.gfx.take();
                event_loop.exit();
            }

            event => {
                let Some(gfx) = self.gfx.as_mut() else {
                    return;
                };
                let scale = gfx.window.scale_factor() as f32;
                // The platform accessibility adapter observes every
                // window event (focus, geometry) before the app does.
                #[cfg(feature = "accessibility")]
                if let Some(a11y) = self.a11y.as_mut() {
                    a11y.adapter.process_event(&gfx.window, &event);
                }

                match event {
                    WindowEvent::Resized(size) => {
                        let w = size.width.max(1);
                        let h = size.height.max(1);
                        // Drop no-op resizes the compositor sometimes
                        // re-sends with the same dimensions — running
                        // surface.configure() for them just stalls the
                        // GPU pipeline without changing anything.
                        let already_pending = self
                            .pending_resize
                            .map(|s| s.width == w && s.height == h)
                            .unwrap_or(false);
                        let same_as_current = self.pending_resize.is_none()
                            && w == gfx.config.width
                            && h == gfx.config.height;
                        if already_pending || same_as_current {
                            return;
                        }
                        self.pending_resize = Some(PhysicalSize::new(w, h));
                        self.next_trigger = FrameTrigger::Resize;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::Occluded(false) => {
                        // Back to visible — repaint. The redraw
                        // handler skips frames (without scheduling a
                        // retry) while the surface reports itself
                        // occluded, so this event is what restarts
                        // painting afterwards.
                        gfx.window.request_redraw();
                    }

                    WindowEvent::CursorMoved { position, .. } => {
                        let lx = position.x as f32 / scale;
                        let ly = position.y as f32 / scale;
                        self.last_pointer = Some((lx, ly));
                        let moved = gfx.renderer.pointer_moved(Pointer::moving(lx, ly));
                        for event in moved.events {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        // Wayland and most X11 compositors deliver
                        // CursorMoved at high frequency while the
                        // cursor is over the surface — only redraw
                        // when the move actually changed something
                        // (hovered identity, scrollbar drag, drag
                        // event), per `PointerMove`.
                        if moved.needs_redraw {
                            self.next_trigger = FrameTrigger::Pointer;
                            gfx.window.request_redraw();
                        }
                    }

                    WindowEvent::CursorLeft { .. } => {
                        self.last_pointer = None;
                        for event in gfx.renderer.pointer_left() {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::HoveredFile(path) => {
                        // File hover routes at the current pointer
                        // position; winit keeps firing CursorMoved
                        // alongside the file events so `last_pointer`
                        // tracks the drag in real time.
                        let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                        for event in gfx.renderer.file_hovered(path, lx, ly) {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::HoveredFileCancelled => {
                        for event in gfx.renderer.file_hover_cancelled() {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::DroppedFile(path) => {
                        let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                        for event in gfx.renderer.file_dropped(path, lx, ly) {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::MouseInput { state, button, .. } => {
                        let Some(button) = pointer_button(button) else {
                            return;
                        };
                        let Some((lx, ly)) = self.last_pointer else {
                            return;
                        };
                        match state {
                            ElementState::Pressed => {
                                for event in
                                    gfx.renderer.pointer_down(Pointer::mouse(lx, ly, button))
                                {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                #[cfg(any(target_os = "android", target_os = "ios"))]
                                sync_mobile_ime(&gfx.window, &gfx.renderer, &mut self.ime_allowed);
                                self.next_trigger = FrameTrigger::Pointer;
                                gfx.window.request_redraw();
                            }
                            ElementState::Released => {
                                for event in gfx.renderer.pointer_up(Pointer::mouse(lx, ly, button))
                                {
                                    let event =
                                        attach_primary_selection_text(event, &mut self.clipboard);
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                self.next_trigger = FrameTrigger::Pointer;
                                gfx.window.request_redraw();
                            }
                        }
                    }

                    WindowEvent::MouseWheel { delta, .. } => {
                        let Some((lx, ly)) = self.last_pointer else {
                            return;
                        };
                        // Convert wheel ticks to logical pixels. Line-based
                        // deltas come from notched mouse wheels; pixel-based
                        // from trackpads. ~50 px/line matches typical OS feel.
                        let (dx, dy) = match delta {
                            MouseScrollDelta::LineDelta(x, y) => (-x * 50.0, -y * 50.0),
                            MouseScrollDelta::PixelDelta(p) => {
                                (-(p.x as f32) / scale, -(p.y as f32) / scale)
                            }
                        };
                        let mut needs_redraw = false;
                        let consumed =
                            if let Some(event) = gfx.renderer.pointer_wheel_event(lx, ly, dx, dy) {
                                needs_redraw = true;
                                dispatch_app_wheel_event(
                                    &mut self.app,
                                    event,
                                    gfx,
                                    self.last_diagnostics.as_ref(),
                                    &mut self.clipboard,
                                    &mut self.last_primary,
                                )
                            } else {
                                false
                            };
                        if !consumed && gfx.renderer.pointer_wheel(lx, ly, dy) {
                            needs_redraw = true;
                        }
                        if needs_redraw {
                            self.next_trigger = FrameTrigger::Pointer;
                            gfx.window.request_redraw();
                        }
                    }

                    WindowEvent::ModifiersChanged(modifiers) => {
                        self.modifiers = key_modifiers(modifiers.state());
                        gfx.renderer.set_modifiers(self.modifiers);
                    }

                    WindowEvent::KeyboardInput {
                        event:
                            key_event @ winit::event::KeyEvent {
                                state: ElementState::Pressed,
                                ..
                            },
                        is_synthetic: false,
                        ..
                    } => {
                        let logical = map_key(&key_event.logical_key);
                        let physical = map_physical(key_event.physical_key);
                        // Dispatch when either facet is meaningful — a key
                        // with no logical identity can still drive a
                        // physical-facet hotkey, and vice versa.
                        if logical != LogicalKey::Unidentified
                            || physical != PhysicalKey::Unidentified
                        {
                            for event in gfx.renderer.key_down(
                                logical,
                                physical,
                                self.modifiers,
                                key_event.repeat,
                            ) {
                                match text_input::clipboard_request(&event) {
                                    Some(ClipboardKind::Copy) => {
                                        copy_current_selection(&gfx.renderer, &mut self.clipboard);
                                        dispatch_app_event(
                                            &mut self.app,
                                            event,
                                            gfx,
                                            self.last_diagnostics.as_ref(),
                                            &mut self.clipboard,
                                            &mut self.last_primary,
                                        );
                                    }
                                    Some(ClipboardKind::Cut) => {
                                        copy_current_selection(&gfx.renderer, &mut self.clipboard);
                                        let delete = clipboard::delete_selection_event(event);
                                        dispatch_app_event(
                                            &mut self.app,
                                            delete,
                                            gfx,
                                            self.last_diagnostics.as_ref(),
                                            &mut self.clipboard,
                                            &mut self.last_primary,
                                        );
                                    }
                                    Some(ClipboardKind::Paste) => {
                                        if let Some(paste) = paste_text_from_clipboard(
                                            event.clone(),
                                            &mut self.clipboard,
                                        ) {
                                            dispatch_app_event(
                                                &mut self.app,
                                                paste,
                                                gfx,
                                                self.last_diagnostics.as_ref(),
                                                &mut self.clipboard,
                                                &mut self.last_primary,
                                            );
                                        } else {
                                            dispatch_app_event(
                                                &mut self.app,
                                                event,
                                                gfx,
                                                self.last_diagnostics.as_ref(),
                                                &mut self.clipboard,
                                                &mut self.last_primary,
                                            );
                                        }
                                    }
                                    None => dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    ),
                                }
                            }
                        }
                        // Composed text payload (handles Shift+a → "A", dead
                        // keys, etc). winit attaches this on the same press
                        // event for non-IME input; IME composition arrives
                        // separately via `WindowEvent::Ime`.
                        if let Some(text) = &key_event.text
                            && let Some(event) = gfx.renderer.text_input(text.to_string())
                        {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        self.next_trigger = FrameTrigger::Keyboard;
                        gfx.window.request_redraw();
                    }
                    WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                        if let Some(event) = gfx.renderer.text_input(text) {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                gfx,
                                self.last_diagnostics.as_ref(),
                                &mut self.clipboard,
                                &mut self.last_primary,
                            );
                        }
                        self.next_trigger = FrameTrigger::Keyboard;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::Touch(touch) => {
                        let lx = touch.location.x as f32 / scale;
                        let ly = touch.location.y as f32 / scale;
                        self.last_pointer = Some((lx, ly));
                        let mut pointer = Pointer::touch(
                            lx,
                            ly,
                            PointerButton::Primary,
                            damascene_core::PointerId(touch.id as u32),
                        );
                        pointer.pressure = touch_pressure(touch.force);
                        match touch.phase {
                            TouchPhase::Started => {
                                for event in gfx.renderer.pointer_down(pointer) {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                #[cfg(any(target_os = "android", target_os = "ios"))]
                                sync_mobile_ime(&gfx.window, &gfx.renderer, &mut self.ime_allowed);
                            }
                            TouchPhase::Moved => {
                                let moved = gfx.renderer.pointer_moved(pointer);
                                for event in moved.events {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                if !moved.needs_redraw {
                                    return;
                                }
                            }
                            TouchPhase::Ended => {
                                for event in gfx.renderer.pointer_up(pointer) {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                self.last_pointer = None;
                            }
                            TouchPhase::Cancelled => {
                                // A real cancel, not a leave: abandons
                                // in-flight gesture captures (viewport
                                // pan, camera drag) that pointer_left
                                // deliberately keeps alive for mouse
                                // drags crossing the window edge.
                                for event in gfx.renderer.pointer_cancelled() {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        gfx,
                                        self.last_diagnostics.as_ref(),
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                self.last_pointer = None;
                            }
                        }
                        self.next_trigger = FrameTrigger::Pointer;
                        gfx.window.request_redraw();
                    }

                    WindowEvent::RedrawRequested => {
                        // Drain time-driven input events (touch
                        // long-press today) before this frame's
                        // build. The runtime folds the long-press
                        // deadline into `next_redraw_in`, so by the
                        // time RedrawRequested fires the deadline may
                        // have just elapsed; dispatching here ensures
                        // the synthesized LongPress event is visible
                        // to the App's `build` for this frame.
                        // Route assistive-technology actions queued
                        // since the last frame — before input polling
                        // and build, so their effects (focus moves,
                        // activations) land in this frame like any
                        // other input.
                        #[cfg(feature = "accessibility")]
                        {
                            let queued: Vec<_> = match self.a11y.as_ref() {
                                Some(a11y) => std::mem::take(&mut *a11y.actions.lock().unwrap()),
                                None => Vec::new(),
                            };
                            for request in queued {
                                for event in gfx.renderer.accessibility_action(request) {
                                    let cx = event_cx(gfx, self.last_diagnostics.as_ref());
                                    self.app.on_event(event, &cx);
                                }
                            }
                        }
                        for event in gfx.renderer.poll_input(Instant::now()) {
                            let cx = event_cx(gfx, self.last_diagnostics.as_ref());
                            self.app.on_event(event, &cx);
                        }
                        // Apply the latest coalesced resize, if any,
                        // before acquiring the next surface texture so
                        // the frame we render matches the size the
                        // compositor is asking for.
                        if let Some(size) = self.pending_resize.take() {
                            gfx.resize(size.width, size.height);
                        }
                        let frame = match gfx.surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(t)
                            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                            wgpu::CurrentSurfaceTexture::Outdated => {
                                // Reconfigure and ask for another redraw —
                                // skipping `request_redraw` here would leave
                                // the compositor's stale frame on screen
                                // until some other event (resize, periodic
                                // tick, layout deadline) happened to wake
                                // us up, which is exactly the lag we're
                                // trying to avoid during an interactive
                                // drag on Wayland.
                                //
                                // Logged with a running count: each
                                // reconfigure allocates a fresh set of
                                // swapchain buffers, and this arm looping
                                // silently during a compositor stall is
                                // what made #133's buffer churn
                                // undiagnosable from the client side.
                                gfx.reconfigures = gfx.reconfigures.wrapping_add(1);
                                log::warn!(
                                    "damascene-winit-wgpu: surface outdated; reconfiguring \
                                     (reconfigure #{})",
                                    gfx.reconfigures
                                );
                                gfx.surface.configure(&gfx.device, &gfx.config);
                                gfx.window.request_redraw();
                                return;
                            }
                            wgpu::CurrentSurfaceTexture::Lost => {
                                // wgpu's documented recovery for `Lost`
                                // is a *fresh surface* (`create_surface`
                                // then `configure`) — unlike `Outdated`,
                                // reconfiguring the lost surface is not
                                // guaranteed to revive it. Fall back to
                                // a plain reconfigure only if recreation
                                // fails. Redraw request for the same
                                // reason as `Outdated` above.
                                let recreated = self.instance.as_ref().is_some_and(|instance| {
                                    gfx.recreate_surface(instance)
                                        .inspect_err(|err| {
                                            log::error!(
                                                "damascene-winit-wgpu: surface recreation \
                                                     failed: {err}; falling back to reconfigure"
                                            );
                                        })
                                        .is_ok()
                                });
                                if !recreated {
                                    gfx.reconfigures = gfx.reconfigures.wrapping_add(1);
                                    gfx.surface.configure(&gfx.device, &gfx.config);
                                }
                                log::warn!(
                                    "damascene-winit-wgpu: surface lost; {} (reconfigure #{})",
                                    if recreated {
                                        "recreated"
                                    } else {
                                        "reconfigured"
                                    },
                                    gfx.reconfigures
                                );
                                gfx.window.request_redraw();
                                return;
                            }
                            wgpu::CurrentSurfaceTexture::Timeout => {
                                // Retry, or an event-driven app stalls
                                // black until the next input: nothing
                                // else will ever request a frame. Seen
                                // on Android cold start, where the
                                // first acquire can time out before
                                // the surface is fully live (#139).
                                // The retry cannot spin hot — the
                                // acquire itself just blocked for the
                                // driver's full timeout before
                                // returning this variant.
                                log::warn!(
                                    "damascene-winit-wgpu: surface acquire timed out; retrying"
                                );
                                gfx.window.request_redraw();
                                return;
                            }
                            wgpu::CurrentSurfaceTexture::Occluded => {
                                // Minimized/hidden — skip the frame.
                                // No retry here: this variant returns
                                // immediately, so a redraw loop would
                                // spin hot for as long as the window
                                // stays hidden. `Occluded(false)`
                                // below repaints when the compositor
                                // reports the window visible again.
                                // That pairing holds because only the
                                // Metal backend emits this variant as
                                // of wgpu 30, and macOS is a platform
                                // where winit delivers Occluded
                                // events — re-audit if other backends
                                // start returning it.
                                return;
                            }
                            other => {
                                log::error!("damascene-winit-wgpu: surface unavailable: {other:?}");
                                return;
                            }
                        };
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        // Per-frame GPU update hook — apps writing to
                        // their own AppTextures (animated content,
                        // 3D viewports, video frames) push pixels to
                        // the queue here, before paint records draws
                        // that sample those textures.
                        // Snapshot diagnostics for this frame: trigger
                        // (consumed once — next defaults back to Other),
                        // wall-clock since previous frame, surface size,
                        // backend tag. Apps read this via `cx.diagnostics()`.
                        let frame_start = Instant::now();
                        let last_frame_dt = self
                            .last_frame_at
                            .map(|t| frame_start.duration_since(t))
                            .unwrap_or(Duration::ZERO);
                        self.last_frame_at = Some(frame_start);
                        let trigger = std::mem::take(&mut self.next_trigger);
                        let scale_factor = gfx.window.scale_factor() as f32;
                        let viewport = Rect::new(
                            0.0,
                            0.0,
                            gfx.config.width as f32 / scale_factor,
                            gfx.config.height as f32 / scale_factor,
                        );
                        // Paint-only path: a time-driven shader's deadline
                        // fired but no input / layout signal is queued for
                        // this frame, so we skip rebuild + layout and reuse
                        // the cached ops. `pending_resize` was applied above
                        // and would have set `Resize` instead — but defend
                        // against trigger-overwrite races by also requiring
                        // it to be empty here.
                        let paint_only =
                            trigger == FrameTrigger::ShaderPaint && self.pending_resize.is_none();

                        let (prepare, palette, t_after_build, t_after_prepare) = if paint_only {
                            damascene_core::profile_span!("frame::repaint");
                            // No build pass on paint-only frames — reuse
                            // the renderer's already-set theme palette
                            // (set on the prior full prepare).
                            let palette = gfx.renderer.theme().palette().clone();
                            let t_after_build = Instant::now();
                            let prepare = gfx.renderer.repaint(
                                &gfx.device,
                                &gfx.queue,
                                viewport,
                                scale_factor,
                            );
                            let t_after_prepare = Instant::now();
                            (prepare, palette, t_after_build, t_after_prepare)
                        } else {
                            let msaa_samples =
                                gfx.msaa.as_ref().map(|m| m.sample_count).unwrap_or(1);
                            self.frame_index = self.frame_index.wrapping_add(1);
                            let diagnostics = HostDiagnostics {
                                backend: self.backend,
                                surface_size: (gfx.config.width, gfx.config.height),
                                scale_factor,
                                msaa_samples,
                                frame_index: self.frame_index,
                                surface_reconfigures: gfx.reconfigures,
                                last_frame_dt,
                                last_build: self.last_build,
                                last_prepare: self.last_prepare,
                                last_layout: self.last_layout,
                                last_layout_intrinsic_cache_hits: self
                                    .last_layout_intrinsic_cache_hits,
                                last_layout_intrinsic_cache_misses: self
                                    .last_layout_intrinsic_cache_misses,
                                last_layout_pruned_subtrees: self.last_layout_pruned_subtrees,
                                last_layout_pruned_nodes: self.last_layout_pruned_nodes,
                                last_draw_ops: self.last_draw_ops,
                                last_draw_ops_culled_text_ops: self.last_draw_ops_culled_text_ops,
                                last_paint: self.last_paint,
                                last_paint_culled_ops: self.last_paint_culled_ops,
                                last_gpu_upload: self.last_gpu_upload,
                                last_snapshot: self.last_snapshot,
                                last_submit: self.last_submit,
                                last_text_layout_cache_hits: self.last_text_layout_cache_hits,
                                last_text_layout_cache_misses: self.last_text_layout_cache_misses,
                                last_text_layout_cache_evictions: self
                                    .last_text_layout_cache_evictions,
                                last_text_layout_shaped_bytes: self.last_text_layout_shaped_bytes,
                                trigger,
                                working_color_space: gfx.renderer.working_color_space(),
                                color_management: gfx.color.status().clone(),
                                surface_color: Some(gfx.surface_color.clone()),
                            };
                            // Retained for event dispatch: handlers read the
                            // last built frame's snapshot via EventCx.
                            self.last_diagnostics = Some(diagnostics.clone());
                            let (tree, palette) = {
                                damascene_core::profile_span!("frame::build");
                                self.app.before_paint(&gfx.queue);
                                WinitWgpuApp::before_build(&mut self.app);
                                let theme = self.app.theme();
                                let palette = theme.palette().clone();
                                let cx = damascene_core::BuildCx::new(&theme)
                                    .with_ui_state(gfx.renderer.ui_state())
                                    .with_diagnostics(&diagnostics)
                                    .with_viewport(viewport.w, viewport.h)
                                    .with_safe_area(safe_area_for_window(
                                        &gfx.window,
                                        (gfx.config.width, gfx.config.height),
                                        scale_factor,
                                    ));
                                let tree = self.app.build(&cx);
                                gfx.renderer.set_theme(theme);
                                gfx.renderer.set_hotkeys(self.app.hotkeys());
                                gfx.renderer.set_selection(self.app.selection());
                                gfx.renderer.push_toasts(self.app.drain_toasts());
                                gfx.renderer
                                    .push_announcements(self.app.drain_announcements());
                                gfx.renderer
                                    .push_focus_requests(self.app.drain_focus_requests());
                                gfx.renderer
                                    .push_scroll_requests(self.app.drain_scroll_requests());
                                gfx.renderer
                                    .push_viewport_requests(self.app.drain_viewport_requests());
                                gfx.renderer
                                    .push_plot_requests(self.app.drain_plot_requests());
                                for url in self.app.drain_link_opens() {
                                    #[cfg(target_os = "android")]
                                    open_link(&self.android_app, &url);
                                    #[cfg(not(any(target_os = "android", target_os = "ios")))]
                                    open_link(&url);
                                    #[cfg(target_os = "ios")]
                                    open_link(&url);
                                }
                                (tree, palette)
                            };
                            let t_after_build = Instant::now();
                            let prepare = {
                                damascene_core::profile_span!("frame::prepare");
                                gfx.renderer.prepare(
                                    &gfx.device,
                                    &gfx.queue,
                                    tree,
                                    viewport,
                                    scale_factor,
                                )
                            };
                            #[cfg(any(target_os = "android", target_os = "ios"))]
                            sync_mobile_ime(&gfx.window, &gfx.renderer, &mut self.ime_allowed);
                            let t_after_prepare = Instant::now();
                            // Cursor resolution depends on the laid-out tree
                            // and the hovered key derived from layout ids,
                            // so it only updates on the full-prepare path.
                            // Paint-only frames inherit the previous cursor.
                            let cursor = gfx.renderer.snapshot_cursor();
                            if cursor != self.last_cursor {
                                gfx.window.set_cursor(winit_cursor(cursor));
                                self.last_cursor = cursor;
                            }
                            // Push the freshly laid-out tree to the
                            // platform adapter while an assistive
                            // technology is connected — the `active`
                            // flag gates the lowering so idle frames
                            // skip it entirely — and surface
                            // activation flips through the
                            // preferences (`screen_reader_active`).
                            #[cfg(feature = "accessibility")]
                            if let Some(a11y) = self.a11y.as_mut() {
                                let active = a11y.active.load(std::sync::atomic::Ordering::Relaxed);
                                if active
                                    && let Some(update) =
                                        gfx.renderer.accessibility_tree_update(scale_factor)
                                {
                                    a11y.adapter.update_if_active(move || update);
                                }
                                if a11y.reported_active != Some(active) {
                                    a11y.reported_active = Some(active);
                                    let mut prefs = self.env_a11y_prefs;
                                    // An env override wins over the
                                    // adapter's live state (testing).
                                    prefs.screen_reader_active =
                                        prefs.screen_reader_active.or(Some(active));
                                    gfx.renderer.set_accessibility_preferences(prefs);
                                }
                            }
                            (prepare, palette, t_after_build, t_after_prepare)
                        };

                        {
                            damascene_core::profile_span!("frame::submit");
                            let mut encoder = gfx.device.create_command_encoder(
                                &wgpu::CommandEncoderDescriptor {
                                    label: Some("damascene_winit_wgpu::encoder"),
                                },
                            );
                            // `render()` owns pass lifetimes itself so it can split
                            // around `BackdropSnapshot` boundaries when the app
                            // uses backdrop-sampling shaders. With no boundary it
                            // collapses to a single pass — same behaviour as the
                            // old `draw(pass)` path.
                            gfx.renderer.render(
                                &gfx.device,
                                &mut encoder,
                                &frame.texture,
                                &view,
                                gfx.msaa.as_ref().map(|msaa| &msaa.view),
                                wgpu::LoadOp::Clear(bg_color(
                                    &palette,
                                    gfx.renderer.working_color_space(),
                                )),
                            );
                            gfx.queue.submit(Some(encoder.finish()));
                            gfx.queue.present(frame);
                            let t_after_submit = Instant::now();
                            self.last_build = t_after_build - frame_start;
                            self.last_prepare = t_after_prepare - t_after_build;
                            self.last_submit = t_after_submit - t_after_prepare;
                            self.last_layout = prepare.timings.layout;
                            self.last_layout_intrinsic_cache_hits =
                                prepare.timings.layout_intrinsic_cache.hits;
                            self.last_layout_intrinsic_cache_misses =
                                prepare.timings.layout_intrinsic_cache.misses;
                            self.last_layout_pruned_subtrees =
                                prepare.timings.layout_prune.subtrees;
                            self.last_layout_pruned_nodes = prepare.timings.layout_prune.nodes;
                            self.last_draw_ops = prepare.timings.draw_ops;
                            self.last_draw_ops_culled_text_ops =
                                prepare.timings.draw_ops_culled_text_ops;
                            self.last_paint = prepare.timings.paint;
                            self.last_paint_culled_ops = prepare.timings.paint_culled_ops;
                            self.last_gpu_upload = prepare.timings.gpu_upload;
                            self.last_snapshot = prepare.timings.snapshot;
                            self.last_text_layout_cache_hits =
                                prepare.timings.text_layout_cache.hits;
                            self.last_text_layout_cache_misses =
                                prepare.timings.text_layout_cache.misses;
                            self.last_text_layout_cache_evictions =
                                prepare.timings.text_layout_cache.evictions;
                            self.last_text_layout_shaped_bytes =
                                prepare.timings.text_layout_cache.shaped_bytes;
                        }

                        // Two-lane redraw scheduling: split widget /
                        // animation deadlines (require rebuild +
                        // layout) from time-driven shader deadlines
                        // (paint-only is sufficient). Each lane parks
                        // its own wake-up; `about_to_wait` chooses the
                        // earlier and `RedrawRequested` dispatches to
                        // either the full prepare path or the
                        // paint-only `repaint` path based on which
                        // deadline fired (input handlers naturally
                        // upgrade to full by overwriting the trigger).
                        //
                        // On a paint-only frame, only the paint lane
                        // is updated — `repaint` deliberately reports
                        // `next_layout_redraw_in = None` because it
                        // didn't re-evaluate that signal, so we leave
                        // the host's previously-parked layout
                        // deadline alone.
                        let now = Instant::now();
                        if !paint_only {
                            match prepare.next_layout_redraw_in {
                                None => self.next_layout_redraw = None,
                                Some(d) if d.is_zero() => {
                                    self.next_layout_redraw = None;
                                    self.next_trigger = FrameTrigger::Animation;
                                    gfx.window.request_redraw();
                                }
                                Some(d) => self.next_layout_redraw = Some(now + d),
                            }
                        }
                        match prepare.next_paint_redraw_in {
                            None => self.next_paint_redraw = None,
                            Some(d) if d.is_zero() => {
                                // Don't override an Animation trigger
                                // we already set above — layout takes
                                // precedence when both fire this turn.
                                self.next_paint_redraw = None;
                                if !matches!(self.next_trigger, FrameTrigger::Animation) {
                                    self.next_trigger = FrameTrigger::ShaderPaint;
                                }
                                gfx.window.request_redraw();
                            }
                            Some(d) => self.next_paint_redraw = Some(now + d),
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Drain the color-management queue once per loop wake. Steady
        // state is a non-blocking dispatch; a compositor-side preferred-
        // description change (output move, HDR toggle) re-negotiates and
        // requests a redraw. The wayland socket becoming readable is
        // itself a loop wake, so changes are picked up promptly even
        // when the app is otherwise idle. A no-op off
        // Linux/wayland-color-management.
        self.poll_color_management();

        let Some(gfx) = self.gfx.as_ref() else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        let now = Instant::now();

        // Refresh the periodic-config wake-up. This is the legacy
        // host-config knob; with widgets adopting `redraw_within` it
        // becomes unnecessary, but keep it as a manual override for
        // hosts that want to force a cadence regardless of what the
        // tree asks.
        if let Some(interval) = self.config.redraw_interval {
            let next = self
                .next_periodic_redraw
                .get_or_insert_with(|| now + interval);
            if now >= *next {
                self.next_trigger = FrameTrigger::Periodic;
                gfx.window.request_redraw();
                *next = now + interval;
            }
        }

        // Pick the earlier wake-up across all three sources: the
        // periodic-config knob, the layout deadline (rebuild + full
        // prepare), and the paint deadline (paint-only via repaint).
        // If a deadline has already passed, fire `request_redraw` and
        // clear it; the dispatcher in RedrawRequested reads the
        // trigger to decide layout vs paint-only path.
        let mut wake_up = self.next_periodic_redraw;
        if let Some(t) = self.next_layout_redraw {
            if now >= t {
                self.next_trigger = FrameTrigger::Animation;
                gfx.window.request_redraw();
                self.next_layout_redraw = None;
            } else {
                wake_up = Some(match wake_up {
                    Some(p) => p.min(t),
                    None => t,
                });
            }
        }
        if let Some(t) = self.next_paint_redraw {
            if now >= t {
                // Layout always wins: if a layout redraw is also queued
                // for this turn — an animation deadline above, or an
                // external wakeup delivered earlier this loop turn —
                // take that path and let it re-derive the paint
                // deadline from the fresh prepare.
                if !matches!(
                    self.next_trigger,
                    FrameTrigger::Animation | FrameTrigger::External
                ) {
                    self.next_trigger = FrameTrigger::ShaderPaint;
                }
                gfx.window.request_redraw();
                self.next_paint_redraw = None;
            } else {
                wake_up = Some(match wake_up {
                    Some(p) => p.min(t),
                    None => t,
                });
            }
        }

        match wake_up {
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn new_clipboard() -> PlatformClipboard {
    arboard::Clipboard::new().ok()
}

#[cfg(target_os = "ios")]
fn new_clipboard() -> PlatformClipboard {
    PlatformClipboard
}

#[cfg(target_os = "android")]
fn new_clipboard(app: &AndroidApp) -> PlatformClipboard {
    PlatformClipboard { app: app.clone() }
}

/// Open a URL surfaced by `App::drain_link_opens` through the OS's
/// default URL handler — `xdg-open` on Linux, `start` on Windows,
/// `open` on macOS — via the `open` crate. Failures (no handler
/// installed, sandboxed environment) are logged rather than panicking.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn open_link(url: &str) {
    if let Err(err) = open::that_detached(url) {
        log::error!("damascene-winit-wgpu: failed to open {url}: {err}");
    }
}

#[cfg(target_os = "ios")]
fn open_link(url: &str) {
    log::error!("damascene-winit-wgpu: opening links is not wired on iOS yet: {url}");
}

#[cfg(target_os = "android")]
fn open_link(app: &AndroidApp, url: &str) {
    let app_for_thread = app.clone();
    let url = url.to_string();
    app.run_on_java_main_thread(Box::new(move || {
        if let Err(err) = open_link_jni(&app_for_thread, &url) {
            log::error!("damascene-winit-wgpu: failed to open link on Android: {err}");
        }
    }));
}

/// JNI body of [`open_link`], run on the Java main thread:
/// `Uri.parse(url)` → `Intent(ACTION_VIEW, uri)` → `activity.startActivity(intent)`.
#[cfg(target_os = "android")]
fn open_link_jni(app: &AndroidApp, url: &str) -> jni::errors::Result<()> {
    let jvm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    jvm.attach_current_thread(|env| {
        let url = env.new_string(url)?;
        let uri = env
            .call_static_method(
                jni::jni_str!("android/net/Uri"),
                jni::jni_str!("parse"),
                jni::jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                &[jni::JValue::Object(url.as_ref())],
            )?
            .l()?;
        let action = env
            .get_static_field(
                jni::jni_str!("android/content/Intent"),
                jni::jni_str!("ACTION_VIEW"),
                jni::jni_sig!("Ljava/lang/String;"),
            )?
            .l()?;
        let intent = env.new_object(
            jni::jni_str!("android/content/Intent"),
            jni::jni_sig!("(Ljava/lang/String;Landroid/net/Uri;)V"),
            &[jni::JValue::Object(&action), jni::JValue::Object(&uri)],
        )?;
        let activity = unsafe {
            jni::objects::JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject)
        };
        env.call_method(
            &activity,
            jni::jni_str!("startActivity"),
            jni::jni_sig!("(Landroid/content/Intent;)V"),
            &[jni::JValue::Object(&intent)],
        )?;
        Ok(())
    })
}

/// Clear color for the surface: the background token converted into the
/// renderer's negotiated working space, exactly like every painted fill.
/// Routing through [`damascene_core::paint::rgba_f32_in`] keeps the clear
/// in lockstep with the paint stream — no separate transfer-function math
/// to drift (issue #45).
fn bg_color(
    palette: &damascene_core::Palette,
    working: damascene_core::color::ColorSpace,
) -> wgpu::Color {
    let [r, g, b, a] = damascene_core::paint::rgba_f32_in(palette.background, working);
    wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    }
}

fn copy_current_selection(renderer: &Runner, clipboard: &mut PlatformClipboard) {
    // Read the selection out of `last_tree` (via the runtime helper) —
    // see `RunnerCore::selected_text` for why a build-only path would
    // miss selections inside a virtual list.
    let Some(text) = renderer.selected_text() else {
        return;
    };
    set_clipboard_text(clipboard, text);
}

/// Logical-pixel viewport currently configured on `gfx`'s surface —
/// the same value the next `build` would see, so event-time layout
/// math (grid navigation, breakpoints) agrees with build-time.
fn logical_viewport(gfx: &host::WindowGfx) -> (f32, f32) {
    let scale = gfx.window.scale_factor() as f32;
    (
        gfx.config.width as f32 / scale,
        gfx.config.height as f32 / scale,
    )
}

fn event_cx<'a>(
    gfx: &'a host::WindowGfx,
    diagnostics: Option<&'a damascene_core::HostDiagnostics>,
) -> damascene_core::EventCx<'a> {
    let (w, h) = logical_viewport(gfx);
    let cx = damascene_core::EventCx::new()
        .with_ui_state(gfx.renderer.ui_state())
        .with_viewport(w, h);
    match diagnostics {
        Some(d) => cx.with_diagnostics(d),
        None => cx,
    }
}

fn dispatch_app_event<A: App>(
    app: &mut A,
    event: UiEvent,
    gfx: &host::WindowGfx,
    diagnostics: Option<&damascene_core::HostDiagnostics>,
    clipboard: &mut PlatformClipboard,
    last_primary: &mut String,
) {
    let before = app.selection();
    let cx = event_cx(gfx, diagnostics);
    app.on_event(event, &cx);
    if app.selection() != before {
        sync_primary_selection(&app.selection(), &gfx.renderer, clipboard, last_primary);
    }
}

fn dispatch_app_wheel_event<A: App>(
    app: &mut A,
    event: UiEvent,
    gfx: &host::WindowGfx,
    diagnostics: Option<&damascene_core::HostDiagnostics>,
    clipboard: &mut PlatformClipboard,
    last_primary: &mut String,
) -> bool {
    let before = app.selection();
    let cx = event_cx(gfx, diagnostics);
    let consumed = app.on_wheel_event(event, &cx);
    if app.selection() != before {
        sync_primary_selection(&app.selection(), &gfx.renderer, clipboard, last_primary);
    }
    consumed
}

fn sync_primary_selection(
    selection: &damascene_core::selection::Selection,
    renderer: &Runner,
    clipboard: &mut PlatformClipboard,
    last_primary: &mut String,
) {
    let text = renderer
        .selected_text_for(selection)
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if text == *last_primary {
        return;
    }
    if !text.is_empty() {
        primary::set(clipboard, &text);
    }
    *last_primary = text;
}

fn paste_text_from_clipboard(event: UiEvent, clipboard: &mut PlatformClipboard) -> Option<UiEvent> {
    let text = get_clipboard_text(clipboard)?;
    Some(clipboard::paste_text_event(event, text))
}

fn attach_primary_selection_text(mut event: UiEvent, clipboard: &mut PlatformClipboard) -> UiEvent {
    if event.kind == UiEventKind::MiddleClick {
        event.text = primary::get(clipboard);
    }
    event
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn set_clipboard_text(clipboard: &mut PlatformClipboard, text: String) {
    if let Some(cb) = clipboard {
        let _ = cb.set_text(text);
    }
}

#[cfg(target_os = "ios")]
fn set_clipboard_text(_clipboard: &mut PlatformClipboard, _text: String) {}

#[cfg(target_os = "android")]
fn set_clipboard_text(clipboard: &mut PlatformClipboard, text: String) {
    if let Err(err) = set_android_clipboard_text(&clipboard.app, &text) {
        log::error!("damascene-winit-wgpu: failed to set Android clipboard: {err}");
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn get_clipboard_text(clipboard: &mut PlatformClipboard) -> Option<String> {
    clipboard.as_mut()?.get_text().ok()
}

#[cfg(target_os = "ios")]
fn get_clipboard_text(_clipboard: &mut PlatformClipboard) -> Option<String> {
    None
}

#[cfg(target_os = "android")]
fn get_clipboard_text(clipboard: &mut PlatformClipboard) -> Option<String> {
    match get_android_clipboard_text(&clipboard.app) {
        Ok(text) => text,
        Err(err) => {
            log::error!("damascene-winit-wgpu: failed to read Android clipboard: {err}");
            None
        }
    }
}

#[cfg(target_os = "android")]
fn set_android_clipboard_text(app: &AndroidApp, text: &str) -> jni::errors::Result<()> {
    use jni::refs::Reference as _;

    let jvm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    jvm.attach_current_thread(|env| {
        let activity = unsafe {
            jni::objects::JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject)
        };
        let service_name = env.new_string("clipboard")?;
        let clipboard = env
            .call_method(
                &activity,
                jni::jni_str!("getSystemService"),
                jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[jni::JValue::Object(service_name.as_ref())],
            )?
            .l()?;
        if clipboard.is_null() {
            return Ok(());
        }

        let label = env.new_string("Damascene")?;
        let text = env.new_string(text)?;
        let clip = env
            .call_static_method(
                jni::jni_str!("android/content/ClipData"),
                jni::jni_str!("newPlainText"),
                jni::jni_sig!(
                    "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"
                ),
                &[
                    jni::JValue::Object(label.as_ref()),
                    jni::JValue::Object(text.as_ref()),
                ],
            )?
            .l()?;
        env.call_method(
            &clipboard,
            jni::jni_str!("setPrimaryClip"),
            jni::jni_sig!("(Landroid/content/ClipData;)V"),
            &[jni::JValue::Object(&clip)],
        )?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn get_android_clipboard_text(app: &AndroidApp) -> jni::errors::Result<Option<String>> {
    use jni::refs::Reference as _;

    let jvm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    jvm.attach_current_thread(|env| {
        let activity = unsafe {
            jni::objects::JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject)
        };
        let service_name = env.new_string("clipboard")?;
        let clipboard = env
            .call_method(
                &activity,
                jni::jni_str!("getSystemService"),
                jni::jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[jni::JValue::Object(service_name.as_ref())],
            )?
            .l()?;
        if clipboard.is_null() {
            return Ok(None);
        }

        let clip = env
            .call_method(
                &clipboard,
                jni::jni_str!("getPrimaryClip"),
                jni::jni_sig!("()Landroid/content/ClipData;"),
                &[],
            )?
            .l()?;
        if clip.is_null() {
            return Ok(None);
        }

        let item_count = env
            .call_method(
                &clip,
                jni::jni_str!("getItemCount"),
                jni::jni_sig!("()I"),
                &[],
            )?
            .i()?;
        if item_count <= 0 {
            return Ok(None);
        }

        let item = env
            .call_method(
                &clip,
                jni::jni_str!("getItemAt"),
                jni::jni_sig!("(I)Landroid/content/ClipData$Item;"),
                &[jni::JValue::Int(0)],
            )?
            .l()?;
        if item.is_null() {
            return Ok(None);
        }

        let text = env
            .call_method(
                &item,
                jni::jni_str!("coerceToText"),
                jni::jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                &[jni::JValue::Object(&activity)],
            )?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }

        let text = env
            .call_method(
                &text,
                jni::jni_str!("toString"),
                jni::jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }

        let text = env.cast_local::<jni::objects::JString>(text)?;
        Ok(Some(text.try_to_string(env)?))
    })
}

mod primary {
    #[cfg(target_os = "linux")]
    pub fn set(clipboard: &mut super::PlatformClipboard, text: &str) {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        if let Some(cb) = clipboard {
            let _ = cb.set().clipboard(LinuxClipboardKind::Primary).text(text);
        }
    }

    #[cfg(target_os = "linux")]
    pub fn get(clipboard: &mut super::PlatformClipboard) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        let cb = clipboard.as_mut()?;
        cb.get().clipboard(LinuxClipboardKind::Primary).text().ok()
    }

    #[cfg(not(target_os = "linux"))]
    pub fn set(_clipboard: &mut super::PlatformClipboard, _text: &str) {}

    #[cfg(not(target_os = "linux"))]
    pub fn get(_clipboard: &mut super::PlatformClipboard) -> Option<String> {
        None
    }
}

/// Stable, human-readable tag for the wgpu backend in use. Surfaced to
/// apps via [`HostDiagnostics::backend`]; the showcase's debug overlay
/// renders this as-is. `BrowserWebGpu` is collapsed to `"WebGPU"` on
/// the assumption that browser-side telemetry already says "Chromium"
/// or "Firefox" elsewhere.
fn backend_label(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Dx12 => "DX12",
        wgpu::Backend::Gl => "GL",
        wgpu::Backend::BrowserWebGpu => "WebGPU",
        wgpu::Backend::Noop => "noop",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use damascene_core::Selection;
    use damascene_core::SelectionPoint;
    use damascene_core::SelectionRange;

    /// `BasicApp` is the wrapper the host uses around the user's app
    /// type. It must forward every per-frame App trait method to the
    /// inner type — a missing forward silently falls through to the
    /// trait default and the host loses sight of app state. A
    /// previous bug had `selection()` left out, which made the
    /// painter never receive a non-empty selection.
    #[test]
    fn basic_app_forwards_selection_to_inner() {
        struct AppWithSelection;
        impl App for AppWithSelection {
            fn build(&self, _cx: &damascene_core::BuildCx) -> damascene_core::El {
                damascene_core::widgets::text::text("hi")
            }
            fn selection(&self) -> Selection {
                Selection {
                    range: Some(SelectionRange {
                        anchor: SelectionPoint::new("p", 0),
                        head: SelectionPoint::new("p", 5),
                    }),
                }
            }
        }
        let basic = BasicApp(AppWithSelection);
        let sel = basic.selection();
        let r = sel.range.as_ref().expect("range forwarded through wrapper");
        assert_eq!(r.anchor.key, "p");
        assert_eq!(r.head.byte, 5);
    }

    #[test]
    fn basic_app_forwards_wheel_events_to_inner() {
        struct AppWithWheel;
        impl App for AppWithWheel {
            fn build(&self, _cx: &damascene_core::BuildCx) -> damascene_core::El {
                damascene_core::widgets::text::text("hi")
            }

            fn on_wheel_event(
                &mut self,
                event: damascene_core::UiEvent,
                _cx: &damascene_core::EventCx,
            ) -> bool {
                event.kind == UiEventKind::PointerWheel && event.wheel_dy() == Some(40.0)
            }
        }

        let mut event = UiEvent::synthetic_click("wheel");
        event.kind = UiEventKind::PointerWheel;
        event.wheel_delta = Some((0.0, 40.0));

        let mut basic = BasicApp(AppWithWheel);
        assert!(basic.on_wheel_event(event, &damascene_core::EventCx::new()));
    }
}
