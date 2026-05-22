//! Optional desktop host for running [`App`]s against a real `wgpu`
//! surface in a `winit` window.
//!
//! Most native apps should use this crate instead of calling
//! `aetna-wgpu` directly:
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
//!     aetna_winit_wgpu::run("My Aetna App", viewport, MyApp::default())
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
//! - Copies the current Aetna text selection to the native clipboard
//!   on Ctrl/Cmd+C.
//! - Requests a redraw whenever interaction state changes (mouse move,
//!   button down/up) so hover/press visuals are immediate.
//!
//! Use [`run_with_config`] when a simple app needs a fixed redraw
//! cadence for external live state such as meters. Put per-frame state
//! refresh in [`App::before_build`]. For fully custom render-loop
//! integration, bypass this crate and call `aetna_wgpu::Runner`
//! directly.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use aetna_core::widgets::text_input::{self, ClipboardKind};
use aetna_core::{
    App, Cursor, FrameTrigger, HostDiagnostics, KeyModifiers, Pointer, PointerButton, Rect, Sides,
    UiEvent, UiEventKind, UiKey, clipboard,
};
use aetna_wgpu::{MsaaTarget, Runner};

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
use winit::event::{ElementState, Force, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
#[cfg(target_os = "android")]
use winit::platform::android::{EventLoopExtAndroid, WindowExtAndroid, activity::AndroidApp};
use winit::window::{CursorIcon, Window, WindowId};

/// Configuration for the optional native winit + wgpu host.
#[derive(Clone, Debug)]
pub struct HostConfig {
    /// MSAA sample count used for Aetna's SDF surfaces. The default is
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
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            sample_count: DEFAULT_SAMPLE_COUNT,
            redraw_interval: None,
            low_latency_present: false,
            app_id: None,
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
    /// [`aetna_core::surface::AppTexture`] / `surface()` widgets)
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

    fn build(&self, cx: &aetna_core::BuildCx) -> aetna_core::El {
        self.0.build(cx)
    }

    fn on_event(&mut self, event: aetna_core::UiEvent) {
        self.0.on_event(event);
    }

    fn on_wheel_event(&mut self, event: aetna_core::UiEvent) -> bool {
        self.0.on_wheel_event(event)
    }

    fn hotkeys(&self) -> Vec<(aetna_core::KeyChord, String)> {
        self.0.hotkeys()
    }

    fn drain_toasts(&mut self) -> Vec<aetna_core::toast::ToastSpec> {
        self.0.drain_toasts()
    }

    fn drain_focus_requests(&mut self) -> Vec<String> {
        self.0.drain_focus_requests()
    }

    fn drain_scroll_requests(&mut self) -> Vec<aetna_core::scroll::ScrollRequest> {
        self.0.drain_scroll_requests()
    }

    fn drain_link_opens(&mut self) -> Vec<String> {
        self.0.drain_link_opens()
    }

    fn shaders(&self) -> Vec<aetna_core::AppShader> {
        self.0.shaders()
    }

    fn theme(&self) -> aetna_core::Theme {
        self.0.theme()
    }

    fn selection(&self) -> aetna_core::Selection {
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
/// `aetna_wgpu::Runner` directly.
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
/// event loop before Aetna owns it. Android, for example, must attach
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
        gfx: None,
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
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

struct Host<A: WinitWgpuApp> {
    title: &'static str,
    viewport: Rect,
    config: HostConfig,
    app: A,
    #[cfg(target_os = "android")]
    android_app: AndroidApp,
    gfx: Option<Gfx>,
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
}

struct Gfx {
    // Fields drop in declaration order. GPU resources must go before
    // the device/window they were created from so shutdown tears them
    // down before their owners disappear.
    renderer: Runner,
    surface: wgpu::Surface<'static>,
    queue: wgpu::Queue,
    device: wgpu::Device,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    /// Multisampled color attachment for the surface frame, kept in
    /// sync with `config.width`/`config.height` and reallocated on
    /// resize. The surface frame texture is the resolve target.
    msaa: Option<MsaaTarget>,
}

fn surface_extent(config: &wgpu::SurfaceConfiguration) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    }
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

#[cfg(any(target_os = "android", target_os = "ios"))]
fn sync_mobile_ime(window: &Window, renderer: &Runner, ime_allowed: &mut bool) {
    let allowed = renderer.focused_captures_keys();
    if allowed != *ime_allowed {
        window.set_ime_allowed(allowed);
        *ime_allowed = allowed;
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
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no compatible adapter");
        self.backend = backend_label(adapter.get_info().backend);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("aetna_winit_wgpu::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("request_device");

        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        // Pick a present mode. `Fifo` is the conservative default —
        // mandatory in the wgpu spec, vsync-locked, predictable power
        // cost. `low_latency_present` opts into `Mailbox` (with `Fifo`
        // fallback) for apps where interaction latency matters more
        // than steady-state throughput; see `HostConfig` for the
        // rationale and trade-offs.
        //
        // `AETNA_PRESENT_MODE=mailbox|immediate|fifo` overrides at
        // runtime — useful for diagnosing without a recompile.
        let mode_override = std::env::var("AETNA_PRESENT_MODE").ok();
        let prefer_mailbox =
            self.config.low_latency_present || mode_override.as_deref() == Some("mailbox");
        let prefer_immediate = mode_override.as_deref() == Some("immediate");
        let prefer_fifo = mode_override.as_deref() == Some("fifo");
        let present_mode = if prefer_immediate
            && surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else if prefer_mailbox
            && !prefer_fifo
            && surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            surface_caps.present_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            // COPY_SRC is required so backdrop-sampling shaders can
            // copy the post-Pass-A surface into the runner's snapshot
            // texture mid-frame. Cost is minimal — most surfaces
            // already advertise it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // Keep the in-flight queue shallow. With `Fifo` this is a
            // hint that Mesa's WSI does not always honor — measured
            // resize lag on Wayland was unaffected by changing this
            // alone — but it's still the right default: an
            // interactive UI gains nothing from buffering more than
            // one frame ahead. Combined with `low_latency_present`
            // (Mailbox), interactive cadence is bounded by render
            // time, not by drained queue depth.
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let sample_count = self.config.sample_count.max(1);
        let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
        renderer.set_theme(self.app.theme());
        renderer.set_surface_size(config.width, config.height);
        // Pre-rasterize printable ASCII for Inter + JetBrains Mono so
        // first-frame appearance of new text labels (e.g. switching
        // section in the showcase) doesn't trip a 20-30ms MSDF
        // generation hitch. ~40ms one-off at startup.
        renderer.warm_default_glyphs();
        // Register any custom shaders the app declared. Done once at
        // startup; pipelines are cached for the runner's lifetime.
        for s in self.app.shaders() {
            renderer.register_shader_with(
                &device,
                s.name,
                s.wgsl,
                s.samples_backdrop,
                s.samples_time,
            );
        }

        let msaa = (sample_count > 1)
            .then(|| MsaaTarget::new(&device, format, surface_extent(&config), sample_count));

        self.gfx = Some(Gfx {
            renderer,
            surface,
            queue,
            device,
            window,
            config,
            msaa,
        });
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
            self.pending_resize = None;
            self.last_pointer = None;
            self.last_frame_at = None;
            self.next_periodic_redraw = None;
            self.ime_allowed = false;
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

                    WindowEvent::CursorMoved { position, .. } => {
                        let lx = position.x as f32 / scale;
                        let ly = position.y as f32 / scale;
                        self.last_pointer = Some((lx, ly));
                        let moved = gfx.renderer.pointer_moved(Pointer::moving(lx, ly));
                        for event in moved.events {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
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
                                &gfx.renderer,
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
                                &gfx.renderer,
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
                                &gfx.renderer,
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
                                &gfx.renderer,
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
                                        &gfx.renderer,
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
                                        &gfx.renderer,
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
                        let dy = match delta {
                            MouseScrollDelta::LineDelta(_, y) => -y * 50.0,
                            MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / scale,
                        };
                        let mut needs_redraw = false;
                        let consumed = if let Some(event) =
                            gfx.renderer.pointer_wheel_event(lx, ly, 0.0, dy)
                        {
                            needs_redraw = true;
                            dispatch_app_wheel_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
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
                        if let Some(key) = map_key(&key_event.logical_key) {
                            for event in
                                gfx.renderer.key_down(key, self.modifiers, key_event.repeat)
                            {
                                match text_input::clipboard_request(&event) {
                                    Some(ClipboardKind::Copy) => {
                                        copy_current_selection(&gfx.renderer, &mut self.clipboard);
                                        dispatch_app_event(
                                            &mut self.app,
                                            event,
                                            &gfx.renderer,
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
                                            &gfx.renderer,
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
                                                &gfx.renderer,
                                                &mut self.clipboard,
                                                &mut self.last_primary,
                                            );
                                        } else {
                                            dispatch_app_event(
                                                &mut self.app,
                                                event,
                                                &gfx.renderer,
                                                &mut self.clipboard,
                                                &mut self.last_primary,
                                            );
                                        }
                                    }
                                    None => dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        &gfx.renderer,
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
                                &gfx.renderer,
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
                                &gfx.renderer,
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
                            aetna_core::PointerId(touch.id as u32),
                        );
                        pointer.pressure = touch_pressure(touch.force);
                        match touch.phase {
                            TouchPhase::Started => {
                                for event in gfx.renderer.pointer_down(pointer) {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        &gfx.renderer,
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
                                        &gfx.renderer,
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
                                        &gfx.renderer,
                                        &mut self.clipboard,
                                        &mut self.last_primary,
                                    );
                                }
                                self.last_pointer = None;
                            }
                            TouchPhase::Cancelled => {
                                for event in gfx.renderer.pointer_left() {
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        &gfx.renderer,
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
                        for event in gfx.renderer.poll_input(Instant::now()) {
                            self.app.on_event(event);
                        }
                        // Apply the latest coalesced resize, if any,
                        // before acquiring the next surface texture so
                        // the frame we render matches the size the
                        // compositor is asking for.
                        if let Some(size) = self.pending_resize.take() {
                            gfx.config.width = size.width;
                            gfx.config.height = size.height;
                            gfx.surface.configure(&gfx.device, &gfx.config);
                            gfx.renderer
                                .set_surface_size(gfx.config.width, gfx.config.height);
                            let extent = surface_extent(&gfx.config);
                            if let Some(msaa) = gfx.msaa.as_mut()
                                && !msaa.matches(extent)
                            {
                                *msaa = MsaaTarget::new(
                                    &gfx.device,
                                    gfx.config.format,
                                    extent,
                                    msaa.sample_count,
                                );
                            }
                        }
                        let frame = match gfx.surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(t)
                            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                            wgpu::CurrentSurfaceTexture::Lost
                            | wgpu::CurrentSurfaceTexture::Outdated => {
                                // Reconfigure and ask for another redraw —
                                // skipping `request_redraw` here would leave
                                // the compositor's stale frame on screen
                                // until some other event (resize, periodic
                                // tick, layout deadline) happened to wake
                                // us up, which is exactly the lag we're
                                // trying to avoid during an interactive
                                // drag on Wayland.
                                gfx.surface.configure(&gfx.device, &gfx.config);
                                gfx.window.request_redraw();
                                return;
                            }
                            other => {
                                eprintln!("surface unavailable: {other:?}");
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
                            aetna_core::profile_span!("frame::repaint");
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
                            };
                            let (mut tree, palette) = {
                                aetna_core::profile_span!("frame::build");
                                self.app.before_paint(&gfx.queue);
                                WinitWgpuApp::before_build(&mut self.app);
                                let theme = self.app.theme();
                                let palette = theme.palette().clone();
                                let cx = aetna_core::BuildCx::new(&theme)
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
                                    .push_focus_requests(self.app.drain_focus_requests());
                                gfx.renderer
                                    .push_scroll_requests(self.app.drain_scroll_requests());
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
                                aetna_core::profile_span!("frame::prepare");
                                gfx.renderer.prepare(
                                    &gfx.device,
                                    &gfx.queue,
                                    &mut tree,
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
                            let cursor = gfx.renderer.ui_state().cursor(&tree);
                            if cursor != self.last_cursor {
                                gfx.window.set_cursor(winit_cursor(cursor));
                                self.last_cursor = cursor;
                            }
                            (prepare, palette, t_after_build, t_after_prepare)
                        };

                        {
                            aetna_core::profile_span!("frame::submit");
                            let mut encoder = gfx.device.create_command_encoder(
                                &wgpu::CommandEncoderDescriptor {
                                    label: Some("aetna_winit_wgpu::encoder"),
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
                                wgpu::LoadOp::Clear(bg_color(&palette)),
                            );
                            gfx.queue.submit(Some(encoder.finish()));
                            frame.present();
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
                // for this turn, take that path and let it re-derive
                // the paint deadline from the fresh prepare.
                if !matches!(self.next_trigger, FrameTrigger::Animation) {
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

fn map_key(key: &Key) -> Option<UiKey> {
    match key {
        Key::Named(NamedKey::Enter) => Some(UiKey::Enter),
        Key::Named(NamedKey::Escape) => Some(UiKey::Escape),
        Key::Named(NamedKey::Tab) => Some(UiKey::Tab),
        Key::Named(NamedKey::Space) => Some(UiKey::Space),
        Key::Named(NamedKey::ArrowUp) => Some(UiKey::ArrowUp),
        Key::Named(NamedKey::ArrowDown) => Some(UiKey::ArrowDown),
        Key::Named(NamedKey::ArrowLeft) => Some(UiKey::ArrowLeft),
        Key::Named(NamedKey::ArrowRight) => Some(UiKey::ArrowRight),
        Key::Named(NamedKey::Backspace) => Some(UiKey::Backspace),
        Key::Named(NamedKey::Delete) => Some(UiKey::Delete),
        Key::Named(NamedKey::Home) => Some(UiKey::Home),
        Key::Named(NamedKey::End) => Some(UiKey::End),
        Key::Named(NamedKey::PageUp) => Some(UiKey::PageUp),
        Key::Named(NamedKey::PageDown) => Some(UiKey::PageDown),
        Key::Character(s) => Some(UiKey::Character(s.to_string())),
        Key::Named(named) => Some(UiKey::Other(format!("{named:?}"))),
        _ => None,
    }
}

fn pointer_button(b: MouseButton) -> Option<PointerButton> {
    match b {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        // Back / Forward / Other → not surfaced; apps that need them can
        // grow the enum.
        _ => None,
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
        eprintln!("aetna-winit-wgpu: failed to open {url}: {err}");
    }
}

#[cfg(target_os = "ios")]
fn open_link(url: &str) {
    eprintln!("aetna-winit-wgpu: opening links is not wired on iOS yet: {url}");
}

#[cfg(target_os = "android")]
fn open_link(app: &AndroidApp, url: &str) {
    let app_for_thread = app.clone();
    let url = url.to_string();
    app.run_on_java_main_thread(Box::new(move || {
        let result = (|| -> jni::errors::Result<()> {
            let jvm = unsafe { jni::JavaVM::from_raw(app_for_thread.vm_as_ptr().cast()) };
            jvm.attach_current_thread(|env| {
                let url = env.new_string(&url)?;
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
                    jni::objects::JObject::from_raw(
                        env,
                        app_for_thread.activity_as_ptr() as jni::sys::jobject,
                    )
                };
                env.call_method(
                    &activity,
                    jni::jni_str!("startActivity"),
                    jni::jni_sig!("(Landroid/content/Intent;)V"),
                    &[jni::JValue::Object(&intent)],
                )?;
                Ok(())
            })
        })();
        if let Err(err) = result {
            eprintln!("aetna-winit-wgpu: failed to open link on Android: {err}");
        }
    }));
}

fn touch_pressure(force: Option<Force>) -> Option<f32> {
    match force? {
        Force::Calibrated {
            force,
            max_possible_force,
            ..
        } if max_possible_force > 0.0 => Some((force / max_possible_force).clamp(0.0, 1.0) as f32),
        Force::Calibrated { force, .. } => Some(force.clamp(0.0, 1.0) as f32),
        Force::Normalized(v) => Some(v.clamp(0.0, 1.0) as f32),
    }
}

/// Translate an Aetna [`Cursor`] to winit's [`CursorIcon`]. The Aetna
/// enum is a subset of winit's so this stays a 1:1 map; the wildcard
/// arm is a forward-compat safety net (Aetna's `Cursor` is
/// `non_exhaustive` — add a new variant in core, add the matching arm
/// here, otherwise it falls back to the platform default).
fn winit_cursor(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Default => CursorIcon::Default,
        Cursor::Pointer => CursorIcon::Pointer,
        Cursor::Text => CursorIcon::Text,
        Cursor::NotAllowed => CursorIcon::NotAllowed,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::Move => CursorIcon::Move,
        Cursor::EwResize => CursorIcon::EwResize,
        Cursor::NsResize => CursorIcon::NsResize,
        Cursor::NwseResize => CursorIcon::NwseResize,
        Cursor::NeswResize => CursorIcon::NeswResize,
        Cursor::ColResize => CursorIcon::ColResize,
        Cursor::RowResize => CursorIcon::RowResize,
        Cursor::Crosshair => CursorIcon::Crosshair,
        _ => CursorIcon::Default,
    }
}

fn key_modifiers(mods: winit::keyboard::ModifiersState) -> KeyModifiers {
    KeyModifiers {
        shift: mods.shift_key(),
        ctrl: mods.control_key(),
        alt: mods.alt_key(),
        logo: mods.super_key(),
    }
}

fn bg_color(palette: &aetna_core::Palette) -> wgpu::Color {
    let c = palette.background;
    wgpu::Color {
        r: srgb_to_linear(c.r as f64 / 255.0),
        g: srgb_to_linear(c.g as f64 / 255.0),
        b: srgb_to_linear(c.b as f64 / 255.0),
        a: c.a as f64 / 255.0,
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

fn dispatch_app_event<A: App>(
    app: &mut A,
    event: UiEvent,
    renderer: &Runner,
    clipboard: &mut PlatformClipboard,
    last_primary: &mut String,
) {
    let before = app.selection();
    app.on_event(event);
    if app.selection() != before {
        sync_primary_selection(&app.selection(), renderer, clipboard, last_primary);
    }
}

fn dispatch_app_wheel_event<A: App>(
    app: &mut A,
    event: UiEvent,
    renderer: &Runner,
    clipboard: &mut PlatformClipboard,
    last_primary: &mut String,
) -> bool {
    let before = app.selection();
    let consumed = app.on_wheel_event(event);
    if app.selection() != before {
        sync_primary_selection(&app.selection(), renderer, clipboard, last_primary);
    }
    consumed
}

fn sync_primary_selection(
    selection: &aetna_core::selection::Selection,
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
        eprintln!("aetna-winit-wgpu: failed to set Android clipboard: {err}");
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
            eprintln!("aetna-winit-wgpu: failed to read Android clipboard: {err}");
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

        let label = env.new_string("Aetna")?;
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

/// Surface format is sRGB, but `wgpu::Color::Clear` is taken as
/// linear-space — convert so the clear color matches our token.
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aetna_core::Selection;
    use aetna_core::SelectionPoint;
    use aetna_core::SelectionRange;

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
            fn build(&self, _cx: &aetna_core::BuildCx) -> aetna_core::El {
                aetna_core::widgets::text::text("hi")
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
            fn build(&self, _cx: &aetna_core::BuildCx) -> aetna_core::El {
                aetna_core::widgets::text::text("hi")
            }

            fn on_wheel_event(&mut self, event: aetna_core::UiEvent) -> bool {
                event.kind == UiEventKind::PointerWheel && event.wheel_dy() == Some(40.0)
            }
        }

        let mut event = UiEvent::synthetic_click("wheel");
        event.kind = UiEventKind::PointerWheel;
        event.wheel_delta = Some((0.0, 40.0));

        let mut basic = BasicApp(AppWithWheel);
        assert!(basic.on_wheel_event(event));
    }
}
