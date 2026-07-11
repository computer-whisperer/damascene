//! Browser host for Damascene wasm apps.
//!
//! Write normal UI code against `damascene_core::prelude::*`, then call
//! [`start_with`] from your wasm crate's `#[wasm_bindgen(start)]`
//! entry point. The host opens a wgpu surface against a canvas in the
//! page and drives the app through winit's browser event loop.
//!
//! The default configuration expects a `<canvas id="damascene_canvas">`.
//! Use [`start_with_config`] when embedding into a page with a different
//! canvas id.
//!
//! `damascene-winit-wgpu` is the equivalent reusable native host.

use damascene_core::Rect;

/// Default canvas element id used by [`WebHostConfig::default`].
pub const DEFAULT_CANVAS_ID: &str = "damascene_canvas";

/// Default logical viewport. Sized to feel reasonable both as a winit
/// window and as a browser canvas. Browsers can override this by
/// resizing the canvas; the runner reacts to `winit::Resized`.
pub const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 900.0,
    h: 640.0,
};

/// Browser host configuration.
#[derive(Clone, Debug)]
pub struct WebHostConfig {
    /// Fallback logical viewport used when the canvas has no CSS size
    /// yet. Once the page lays the canvas out, the host tracks its CSS
    /// box through `ResizeObserver`.
    pub viewport: Rect,
    /// Id of the canvas element the host should attach to.
    pub canvas_id: String,
}

impl WebHostConfig {
    pub fn new(viewport: Rect) -> Self {
        Self {
            viewport,
            canvas_id: DEFAULT_CANVAS_ID.to_string(),
        }
    }

    pub fn with_canvas_id(mut self, canvas_id: impl Into<String>) -> Self {
        self.canvas_id = canvas_id.into();
        self
    }
}

impl Default for WebHostConfig {
    fn default() -> Self {
        Self::new(VIEWPORT)
    }
}

#[cfg(target_arch = "wasm32")]
pub use web_entry::{WebHandle, install_logger, start_with, start_with_config};

#[cfg(not(target_arch = "wasm32"))]
pub use native_stub::{WebHandle, install_logger, start_with, start_with_config};

/// Single-threaded message queue bridging async browser callbacks into
/// the render loop — the standard wasm integration shape every app was
/// inventing (fetch results, WebSocket messages, file reads):
///
/// 1. Create one `Mailbox<Msg>` per message stream; clone it into your
///    `spawn_local` futures and JS callbacks.
/// 2. After [`start_with_config`] returns, call [`Mailbox::set_handle`]
///    with the [`WebHandle`] so pushes wake the host.
/// 3. [`Mailbox::push`] from callbacks — it queues the value and
///    requests a redraw.
/// 4. [`Mailbox::drain`] in `App::before_build` and fold the messages
///    into app state; the frame being built sees them.
///
/// `Clone` is a cheap `Rc` bump (wasm is single-threaded; this type is
/// deliberately not `Send`). Pushes before `set_handle` are queued and
/// delivered on the first frame after the host starts.
#[derive(Clone)]
pub struct Mailbox<T> {
    inner: std::rc::Rc<MailboxInner<T>>,
}

struct MailboxInner<T> {
    queue: std::cell::RefCell<std::collections::VecDeque<T>>,
    handle: std::cell::RefCell<Option<WebHandle>>,
}

impl<T> Default for Mailbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Mailbox<T> {
    /// An empty mailbox with no host attached yet.
    pub fn new() -> Self {
        Self {
            inner: std::rc::Rc::new(MailboxInner {
                queue: std::cell::RefCell::new(std::collections::VecDeque::new()),
                handle: std::cell::RefCell::new(None),
            }),
        }
    }

    /// Attach the host's redraw handle (the [`WebHandle`] returned by
    /// [`start_with_config`]) so subsequent pushes wake the render
    /// loop. Idempotent; replaces any previous handle.
    pub fn set_handle(&self, handle: WebHandle) {
        *self.inner.handle.borrow_mut() = Some(handle);
    }

    /// Queue a message and request a redraw (when a handle is
    /// attached). Call from `spawn_local` futures and JS event
    /// callbacks.
    pub fn push(&self, value: T) {
        self.inner.queue.borrow_mut().push_back(value);
        if let Some(handle) = self.inner.handle.borrow().as_ref() {
            handle.request_redraw();
        }
    }

    /// Take every queued message, in push order. Call once per frame
    /// from `App::before_build`.
    pub fn drain(&self) -> Vec<T> {
        self.inner.queue.borrow_mut().drain(..).collect()
    }

    /// True when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.inner.queue.borrow().is_empty()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native_stub {
    use damascene_core::{App, Rect};

    use super::WebHostConfig;

    /// Browser redraw handle.
    ///
    /// On non-wasm targets this is a no-op placeholder so host crates
    /// can type-check shared code. It is only functional on
    /// `wasm32-unknown-unknown`.
    #[derive(Clone, Debug, Default)]
    pub struct WebHandle {
        _private: (),
    }

    impl WebHandle {
        pub fn request_redraw(&self) {}
        pub fn destroy(&self) {}
    }

    pub fn start_with<A: App + 'static>(_viewport: Rect, _app: A) -> WebHandle {
        panic!("damascene-web can only start apps on wasm32-unknown-unknown")
    }

    /// No-op on non-wasm targets; see the wasm doc.
    pub fn install_logger(_level: log::Level) {}

    pub fn start_with_config<A: App + 'static>(_config: WebHostConfig, _app: A) -> WebHandle {
        panic!("damascene-web can only start apps on wasm32-unknown-unknown")
    }
}

// ---- Wasm host ----
//
// Lives in its own module so it can pull in wasm-only deps without
// polluting native builds.

#[cfg(target_arch = "wasm32")]
mod web_entry {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::Arc;

    use damascene_core::{
        App, BuildCx, Cursor, FrameTrigger, HostDiagnostics, KeyModifiers, LogicalKey, NamedKey,
        Palette, PhysicalKey, Pointer, PointerButton, PointerId, PointerKind, Rect, UiEvent,
        UiEventKind, clipboard,
        widgets::text_input::{self, ClipboardKind},
    };
    use damascene_wgpu::{PrepareTimings, Runner, RunnerCaps};
    use damascene_winit::{key_modifiers, map_key, map_physical, winit_cursor};

    // MSAA is off on the browser. The WebGL2 path doesn't advertise
    // `MULTISAMPLED_SHADING`, so MSAA gives nothing to the SDF stock
    // surfaces (they do their own analytic AA in the fragment shader);
    // it would only have improved vector-icon polygon-edge AA. With it
    // on, Firefox + Mesa's implicit MSAA resolve was mis-syncing
    // partial regions of the swapchain — the sidebar would freeze at
    // its previous pixels until something forced a tree reshape. WebGPU
    // (Chromium) was unaffected but we use the same value for both
    // browser backends to keep one code path. Revisit once the WebGL2
    // resolve issue is understood (or once WebGPU is the only target).
    const SAMPLE_COUNT: u32 = 1;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;
    use web_time::{Duration, Instant};
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
    use winit::window::{Window, WindowId};

    use super::WebHostConfig;

    /// Number of redraws to accumulate before logging an averaged
    /// frame-timing line. 60 → roughly once per second at 60fps when
    /// animations are in flight; for idle UI (no redraws) the log
    /// just stops, which is the right behavior.
    const FRAME_LOG_INTERVAL: u32 = 60;

    /// Pointer event captured by a DOM listener and queued for the
    /// next frame's dispatch pass. We can't dispatch directly inside
    /// the closure because the app handle and the renderer live on
    /// `Host`, which is owned by winit's event loop and only reachable
    /// through `&mut self` in `window_event`. The queue lets the
    /// closures stay simple (push + request_redraw) while the
    /// dispatch path runs with full host state.
    enum QueuedPointer {
        Move(Pointer),
        Down(Pointer),
        Up(Pointer),
        Cancel,
        Leave,
    }

    /// Map `PointerEvent.pointerType` → [`PointerKind`].
    fn pointer_kind_from_type(s: &str) -> PointerKind {
        match s {
            "touch" => PointerKind::Touch,
            "pen" => PointerKind::Pen,
            // "mouse", "" or any future / unknown value falls back to
            // mouse semantics — that's the conservative default for
            // hover-driven affordances.
            _ => PointerKind::Mouse,
        }
    }

    /// Map `PointerEvent.button` → [`PointerButton`]. `None` for
    /// buttons Damascene does not route (back, forward, pen eraser).
    fn pointer_button_from_event(b: i16) -> Option<PointerButton> {
        match b {
            0 => Some(PointerButton::Primary),
            1 => Some(PointerButton::Middle),
            2 => Some(PointerButton::Secondary),
            _ => None,
        }
    }

    /// Translate a DOM `PointerEvent` to an Damascene [`Pointer`]. Uses
    /// `offset_x`/`offset_y` because they are already canvas-local
    /// CSS pixels — the runtime expects logical-pixel coordinates,
    /// so no DPI division is needed (in contrast to winit's
    /// physical-pixel `CursorMoved`).
    fn pointer_from_event(event: &web_sys::PointerEvent, button: PointerButton) -> Pointer {
        let pressure = event.pressure();
        Pointer {
            x: event.offset_x() as f32,
            y: event.offset_y() as f32,
            button,
            kind: pointer_kind_from_type(&event.pointer_type()),
            id: PointerId(event.pointer_id() as u32),
            // PointerEvent always returns a value for `pressure`, but
            // it's `0.0` for non-pressure-sensitive devices (mouse).
            // `Some(0.0)` would be misleading, so we filter that case.
            pressure: if pressure > 0.0 { Some(pressure) } else { None },
        }
    }

    /// Rolling per-frame timing bucket. Three top-level CPU stages
    /// (`build`, `prepare`, `submit`) plus a per-stage breakdown of
    /// what's inside `prepare` (layout / draw_ops / paint / gpu_upload
    /// / snapshot — see [`PrepareTimings`]). `inter` is the wall-clock
    /// interval between consecutive RedrawRequested calls; comparing
    /// `build + prepare + submit` against `inter` shows how much frame
    /// budget the CPU is burning vs. how much the browser's rAF throttle
    /// gives us.
    #[derive(Default)]
    struct FrameStats {
        build_us: u64,
        prepare_us: u64,
        submit_us: u64,
        inter_us: u64,
        // Sub-buckets inside prepare. Sum is ~prepare_us minus a few
        // microseconds of Instant::now() overhead.
        layout_us: u64,
        draw_ops_us: u64,
        paint_us: u64,
        gpu_upload_us: u64,
        snapshot_us: u64,
        samples: u32,
        last_frame_start: Option<Instant>,
    }

    impl FrameStats {
        fn record(
            &mut self,
            frame_start: Instant,
            t1: Instant,
            t2: Instant,
            t3: Instant,
            prep: PrepareTimings,
        ) {
            self.build_us += (t1 - frame_start).as_micros() as u64;
            self.prepare_us += (t2 - t1).as_micros() as u64;
            self.submit_us += (t3 - t2).as_micros() as u64;
            self.layout_us += prep.layout.as_micros() as u64;
            self.draw_ops_us += prep.draw_ops.as_micros() as u64;
            self.paint_us += prep.paint.as_micros() as u64;
            self.gpu_upload_us += prep.gpu_upload.as_micros() as u64;
            self.snapshot_us += prep.snapshot.as_micros() as u64;
            if let Some(prev) = self.last_frame_start {
                self.inter_us += (frame_start - prev).as_micros() as u64;
            }
            self.last_frame_start = Some(frame_start);
            self.samples += 1;
            if self.samples >= FRAME_LOG_INTERVAL {
                self.flush();
            }
        }

        fn flush(&mut self) {
            // `inter` averages over `samples - 1` because the first
            // frame in each window has no prior frame to diff against.
            let n = self.samples as u64;
            let inter_n = (self.samples.saturating_sub(1)) as u64;
            let build = self.build_us / n;
            let prepare = self.prepare_us / n;
            let submit = self.submit_us / n;
            let layout = self.layout_us / n;
            let draw_ops = self.draw_ops_us / n;
            let paint = self.paint_us / n;
            let gpu_upload = self.gpu_upload_us / n;
            let snapshot = self.snapshot_us / n;
            let cpu = build + prepare + submit;
            let inter = self.inter_us.checked_div(inter_n).unwrap_or(0);
            let util = (cpu * 100).checked_div(inter).unwrap_or(0);
            log::info!(
                "frame[{n}] inter={:.2}ms cpu={:.2}ms util={util}% | build={:.2} prepare={:.2} (layout={:.2} draw_ops={:.2} paint={:.2} gpu={:.2} snapshot={:.2}) submit={:.2}",
                inter as f64 / 1000.0,
                cpu as f64 / 1000.0,
                build as f64 / 1000.0,
                prepare as f64 / 1000.0,
                layout as f64 / 1000.0,
                draw_ops as f64 / 1000.0,
                paint as f64 / 1000.0,
                gpu_upload as f64 / 1000.0,
                snapshot as f64 / 1000.0,
                submit as f64 / 1000.0,
            );
            self.build_us = 0;
            self.prepare_us = 0;
            self.submit_us = 0;
            self.inter_us = 0;
            self.layout_us = 0;
            self.draw_ops_us = 0;
            self.paint_us = 0;
            self.gpu_upload_us = 0;
            self.snapshot_us = 0;
            self.samples = 0;
            // Keep last_frame_start so `inter` in the next window
            // includes the gap from the last logged frame to the
            // first frame of the new window.
        }
    }

    /// Wire the global `tracing` subscriber to `tracing-wasm`, which
    /// emits `performance.mark` / `performance.measure` calls for every
    /// span. Open DevTools → Performance, hit Record, exercise the UI;
    /// each span shows up as a labeled User Timing measure in the
    /// flamegraph (`prepare::layout`, `paint::text::shape_runs`, etc).
    /// Defaults are fine — span events go to console.log, measures get
    /// written, and the subscriber only sees enabled spans (no extra
    /// filter wiring needed on top of the `profiling` feature).
    #[cfg(feature = "profiling")]
    fn install_profiling_subscriber() {
        tracing_wasm::set_as_global_default();
    }

    /// Route `log` macros to the browser console and surface panics
    /// with a stack trace — the `eframe::WebLogger` equivalent. Safe
    /// to call multiple times; the first level wins (the `log` crate
    /// allows one global logger).
    ///
    /// [`start_with_config`] calls this with [`log::Level::Info`]
    /// automatically, so most apps never need it. Call it yourself
    /// *before* starting the host when you want a different level or
    /// logs from pre-start setup code:
    ///
    /// ```ignore
    /// damascene_web::install_logger(log::Level::Debug);
    /// let handle = damascene_web::start_with_config(config, app);
    /// ```
    pub fn install_logger(level: log::Level) {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(level);
    }

    /// Handle returned by [`start_with`] so embedding code can wake the
    /// host after external browser events enqueue app work, and tear
    /// the host down when the embedding page unmounts the canvas.
    #[derive(Clone)]
    pub struct WebHandle {
        inner: Rc<WebHandleInner>,
    }

    struct WebHandleInner {
        window: RefCell<Option<Arc<Window>>>,
        ready: Cell<bool>,
        pending_redraw: Cell<bool>,
        destroy: Cell<bool>,
    }

    impl WebHandle {
        fn new() -> Self {
            Self {
                inner: Rc::new(WebHandleInner {
                    window: RefCell::new(None),
                    ready: Cell::new(false),
                    pending_redraw: Cell::new(false),
                    destroy: Cell::new(false),
                }),
            }
        }

        /// Request a redraw from external browser integration code.
        ///
        /// If the browser window or GPU setup is not ready yet, the
        /// request is remembered and flushed once setup completes.
        /// No-op after [`Self::destroy`].
        pub fn request_redraw(&self) {
            if self.inner.destroy.get() {
                return;
            }
            if self.inner.ready.get()
                && let Some(window) = self.inner.window.borrow().as_ref()
            {
                window.request_redraw();
                return;
            }
            self.inner.pending_redraw.set(true);
        }

        /// Tear down the host: stop the event loop, unregister every
        /// DOM listener and the `ResizeObserver` this host installed,
        /// remove the hidden soft-keyboard `<input>` from the
        /// document, and release the GPU surface. Call this when an
        /// SPA unmounts the canvas — without it each mount leaks the
        /// previous host's listeners and appended input, and a later
        /// fire of a leaked listener throws (its Rust closure is
        /// gone).
        ///
        /// Returns immediately; the teardown itself runs on the next
        /// event-loop turn (the handle wakes the loop). Idempotent —
        /// repeated calls, or calls racing the still-async GPU setup,
        /// are safe. After destroy the handle is inert:
        /// [`Self::request_redraw`] becomes a no-op, and a new
        /// [`start_with`] call (with a fresh canvas of the same id)
        /// creates an independent host.
        ///
        /// The canvas element itself is left in the page — it belongs
        /// to the embedding markup, not to Damascene.
        pub fn destroy(&self) {
            self.inner.destroy.set(true);
            // Wake the loop so the host observes the flag. Before the
            // window exists (`resumed` hasn't run) the flag alone is
            // enough: `resumed` checks it before installing anything.
            if let Some(window) = self.inner.window.borrow().as_ref() {
                window.request_redraw();
            }
        }

        fn destroy_requested(&self) -> bool {
            self.inner.destroy.get()
        }

        fn set_window(&self, window: Arc<Window>) {
            *self.inner.window.borrow_mut() = Some(window);
        }

        /// Drop the handle's `Arc<Window>` so the winit window (and
        /// the canvas listeners it owns) can actually die at teardown
        /// even while the embedder keeps the handle around.
        fn clear_window(&self) {
            *self.inner.window.borrow_mut() = None;
        }

        fn mark_ready(&self) -> bool {
            self.inner.ready.set(true);
            self.inner.pending_redraw.replace(false)
        }
    }

    /// Start an Damascene app in the browser using the default canvas id.
    ///
    /// Call this from the downstream crate's own
    /// `#[wasm_bindgen(start)]` function.
    pub fn start_with<A: App + 'static>(viewport: Rect, app: A) -> WebHandle {
        start_with_config(WebHostConfig::new(viewport), app)
    }

    /// Start an Damascene app in the browser with explicit host config.
    ///
    /// The function spawns winit's web event loop and returns
    /// immediately. Keep the returned [`WebHandle`] anywhere external
    /// JS callbacks need to wake Damascene after pushing work into
    /// app-owned shared state.
    ///
    /// # GPU-setup failures
    ///
    /// Adapter/device acquisition is async and finishes after the
    /// page's `init()` promise resolved, so a browser with neither
    /// usable WebGPU nor WebGL2 cannot reject `init()`. Failures are
    /// reported as a bubbling `damascene-error` `CustomEvent` on the
    /// canvas with `detail = { kind: "gpu-setup", message }` — listen
    /// there (or on the document) to show an error UI.
    ///
    /// # SPA lifecycle
    ///
    /// For a full-page canvas that lives as long as the tab, the
    /// handle can simply be kept (or dropped — the host runs either
    /// way). When the canvas is mounted by an SPA framework, pair
    /// every mount with [`WebHandle::destroy`] on unmount:
    ///
    /// ```ignore
    /// // mount:   (the canvas element must already be in the DOM)
    /// let handle = start_with_config(WebHostConfig::new(viewport), app);
    /// // unmount: stop the loop, unregister listeners + observer,
    /// //          remove the hidden soft-keyboard input, release GPU.
    /// handle.destroy();
    /// ```
    ///
    /// Without the destroy, each remount leaks the previous host —
    /// its DOM listeners, `ResizeObserver`, hidden `<input>`, and GPU
    /// surface — and a leaked listener that later fires throws into a
    /// dropped Rust closure. After `destroy()`, mounting again with a
    /// fresh canvas (same id is fine) creates an independent host.
    pub fn start_with_config<A: App + 'static>(config: WebHostConfig, app: A) -> WebHandle {
        // Surface panics in the browser console with a stack trace —
        // without this hook a wasm panic dies silently as `unreachable`.
        // (No-ops if the app called `install_logger` earlier with its
        // own level.)
        install_logger(log::Level::Info);
        // When built with `--features profiling`, route every
        // `profile_span!` call to the browser's User Timing API so spans
        // show up as named measures in DevTools → Performance alongside
        // the page's own frame/script work. Off-builds compile this away.
        #[cfg(feature = "profiling")]
        install_profiling_subscriber();

        let event_loop = EventLoop::new().expect("EventLoop::new");
        let handle = WebHandle::new();
        let host = Host::new(config, app, handle.clone());
        // spawn_app hands control to the browser. Native uses
        // run_app(...) which blocks; on wasm32 the event loop is
        // driven by the browser's animation-frame callbacks.
        event_loop.spawn_app(host);
        handle
    }

    /// Open a URL surfaced by `App::drain_link_opens` in a new tab.
    /// `_blank` matches what users expect for a click on an external
    /// link in app UI; `noopener` severs the `window.opener` reference
    /// so the opened page can't reverse-control this one. Failures are
    /// logged rather than panicking — popup blockers and CSP rules can
    /// reject the open and the showcase shouldn't crash because the
    /// browser said no.
    fn open_link(url: &str) {
        let Some(window) = web_sys::window() else {
            log::warn!("damascene-web: no window; dropping link open for {url}");
            return;
        };
        if let Err(err) = window.open_with_url_and_target_and_features(url, "_blank", "noopener") {
            log::warn!("damascene-web: window.open({url}) failed: {err:?}");
        }
    }

    /// Surface a fatal GPU-setup failure to the embedding page.
    ///
    /// Adapter/device acquisition runs in an async task *after*
    /// `start_with` (and therefore the page's `init()` promise) has
    /// resolved, so a panic there would bypass any `init().catch(...)`
    /// error UI entirely — the user gets a blank canvas and a console
    /// `unreachable`. Instead the failure is logged and dispatched as
    /// a bubbling `damascene-error` `CustomEvent` on the canvas, with
    /// `detail = { kind: "gpu-setup", message }`. Listen on the canvas
    /// (or, since it bubbles, the document) to show an error UI:
    ///
    /// ```js
    /// canvas.addEventListener('damascene-error', (e) => {
    ///     showFatalError(e.detail.message);
    /// });
    /// ```
    fn report_setup_error(canvas: &web_sys::HtmlCanvasElement, message: &str) {
        log::error!("damascene-web: {message}");
        let detail = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&detail, &"kind".into(), &"gpu-setup".into());
        let _ = js_sys::Reflect::set(&detail, &"message".into(), &message.into());
        let init = web_sys::CustomEventInit::new();
        init.set_bubbles(true);
        init.set_detail(&detail);
        if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict("damascene-error", &init)
        {
            let _ = canvas.dispatch_event(&event);
        }
    }

    /// Locate the configured canvas element in the host page.
    fn locate_canvas(canvas_id: &str) -> web_sys::HtmlCanvasElement {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");
        document
            .get_element_by_id(canvas_id)
            .unwrap_or_else(|| panic!("missing #{canvas_id} canvas element"))
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap_or_else(|_| panic!("#{canvas_id} is not a canvas"))
    }

    /// Read the canvas's CSS-laid-out box at the device pixel ratio.
    /// Returned size is what the swapchain backing buffer should match;
    /// callers pass it to `apply_canvas_size` to actually reconfigure
    /// the surface.
    fn measure_canvas(canvas: &web_sys::HtmlCanvasElement, fallback: Rect) -> (u32, u32) {
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0)
            .max(1.0);
        let css_w = if canvas.client_width() > 0 {
            canvas.client_width() as f64
        } else {
            fallback.w.max(1.0) as f64
        };
        let css_h = if canvas.client_height() > 0 {
            canvas.client_height() as f64
        } else {
            fallback.h.max(1.0) as f64
        };
        let phys_w = (css_w * dpr).round() as u32;
        let phys_h = (css_h * dpr).round() as u32;
        (phys_w, phys_h)
    }

    /// Set the canvas's drawing buffer to `(phys_w, phys_h)` and
    /// reconfigure the surface + MSAA target to match. Called once at
    /// initial setup and on every ResizeObserver fire afterward.
    ///
    /// We bypass winit's `request_inner_size` round-trip — the web
    /// backend doesn't reliably translate it into a `Resized` event, so
    /// canvas resizes mid-session were leaving the swapchain stretched
    /// at the original size until the page reloaded. Doing the
    /// reconfigure inline keeps the surface in lockstep with the
    /// canvas.
    fn apply_canvas_size(
        canvas: &web_sys::HtmlCanvasElement,
        gfx: &mut Gfx,
        phys_w: u32,
        phys_h: u32,
    ) {
        canvas.set_width(phys_w);
        canvas.set_height(phys_h);
        if gfx.config.width == phys_w && gfx.config.height == phys_h {
            return;
        }
        gfx.config.width = phys_w;
        gfx.config.height = phys_h;
        gfx.surface.configure(&gfx.device, &gfx.config);
        gfx.renderer.set_surface_size(phys_w, phys_h);
        if let Some(msaa) = gfx.msaa.as_mut() {
            let extent = surface_extent(&gfx.config);
            if !msaa.matches(extent) {
                *msaa = damascene_wgpu::MsaaTarget::new(
                    &gfx.device,
                    gfx.render_format,
                    extent,
                    SAMPLE_COUNT,
                );
            }
        }
    }

    /// The canvas pointer listeners as (event name, callback) pairs,
    /// kept so [`Host::teardown`] can unregister each one.
    type PointerListeners = Vec<(&'static str, Closure<dyn FnMut(web_sys::PointerEvent)>)>;

    /// Install `pointermove` / `pointerdown` / `pointerup` /
    /// `pointercancel` / `pointerleave` listeners on `canvas` and
    /// stash the closures in `out` for the host's lifetime.
    ///
    /// Each listener pushes onto the shared queue and requests a
    /// redraw; the host's `window_event` drains the queue at the top
    /// of every call. `pointerdown` also calls `setPointerCapture` so
    /// the pointer keeps reporting to the canvas during a drag even
    /// when the contact slides off — without this, slider scrubbing
    /// and text-selection drag stop the moment the finger leaves the
    /// element.
    fn install_pointer_listeners(
        canvas: &web_sys::HtmlCanvasElement,
        window: &Arc<Window>,
        pending: &Rc<RefCell<VecDeque<QueuedPointer>>>,
        gfx: &Rc<RefCell<Option<Gfx>>>,
        soft_keyboard: Option<&Rc<SoftKeyboard>>,
        out: &mut PointerListeners,
    ) {
        // pointermove
        {
            let pending = pending.clone();
            let window = window.clone();
            let closure: Closure<dyn FnMut(web_sys::PointerEvent)> =
                Closure::new(move |event: web_sys::PointerEvent| {
                    let p = pointer_from_event(&event, PointerButton::Primary);
                    pending.borrow_mut().push_back(QueuedPointer::Move(p));
                    window.request_redraw();
                });
            canvas
                .add_event_listener_with_callback("pointermove", closure.as_ref().unchecked_ref())
                .expect("add pointermove listener");
            out.push(("pointermove", closure));
        }

        // pointerdown
        {
            let pending = pending.clone();
            let window = window.clone();
            let canvas_for_capture = canvas.clone();
            let gfx_for_hit = gfx.clone();
            let soft_keyboard = soft_keyboard.cloned();
            let closure: Closure<dyn FnMut(web_sys::PointerEvent)> =
                Closure::new(move |event: web_sys::PointerEvent| {
                    let Some(button) = pointer_button_from_event(event.button()) else {
                        return;
                    };
                    let p = pointer_from_event(&event, button);
                    // Soft-keyboard summon must happen synchronously
                    // inside this user-gesture handler — iOS rejects
                    // programmatic `.focus()` from any later context.
                    // Hit-test against the runner's last laid-out
                    // tree (read-only borrow) to decide whether the
                    // press would land on a text-input widget; if
                    // so, focus the hidden input now. The runner-
                    // side dispatch follows on the next frame via
                    // the queue/drain path.
                    let mut focused_input = false;
                    if matches!(p.kind, PointerKind::Touch | PointerKind::Pen)
                        && let Some(sk) = soft_keyboard.as_ref()
                    {
                        let want_keyboard = gfx_for_hit
                            .borrow()
                            .as_ref()
                            .map(|g| g.renderer.would_press_focus_text_input(p.x, p.y))
                            .unwrap_or(false);
                        if want_keyboard {
                            sk.focus_if_needed();
                            focused_input = true;
                        }
                    }
                    // Take focus on tap-down so subsequent keydown
                    // events (soft keyboard, hardware keyboard on
                    // tablets) reach the canvas. winit's web backend
                    // would normally do this for compat-mouse events,
                    // but we no longer route through there.
                    //
                    // Skip when the input was just focused — the
                    // canvas is fighting for the same DOM focus, and
                    // taking it back here was preventing Android (and
                    // iOS) from ever seeing a focused input long
                    // enough to summon the on-screen keyboard.
                    // Hardware-keyboard input into a text input still
                    // works because keystrokes reach the input's
                    // own listeners and route through `text_input` /
                    // `key_down` the same way they would via the
                    // canvas's keydown handler.
                    if !focused_input {
                        let _ = canvas_for_capture
                            .dyn_ref::<web_sys::HtmlElement>()
                            .and_then(|el| el.focus().ok());
                    }
                    // Keep this pointer captured so a drag that
                    // slides off the canvas still produces events to
                    // the runner (essential for touch sliders,
                    // drag-select, and text-input scrubbing).
                    let _ = canvas_for_capture.set_pointer_capture(event.pointer_id());
                    // When the press just summoned the on-screen
                    // keyboard, suppress the browser's default
                    // pointerdown action so it doesn't shift DOM
                    // focus to the canvas (a tabindex=0 element)
                    // after our listener returns. Android Chrome
                    // does that focus shift as part of touch
                    // pointerdown handling on focusable elements,
                    // and the resulting blur on our hidden input
                    // dismisses the keyboard one frame after it
                    // appears. We also stopPropagation so any
                    // document-level listener the host page wires
                    // doesn't get a second crack at shifting focus.
                    if focused_input {
                        event.prevent_default();
                        event.stop_propagation();
                    }
                    pending.borrow_mut().push_back(QueuedPointer::Down(p));
                    window.request_redraw();
                });
            canvas
                .add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref())
                .expect("add pointerdown listener");
            out.push(("pointerdown", closure));
        }

        // pointerup
        {
            let pending = pending.clone();
            let window = window.clone();
            let closure: Closure<dyn FnMut(web_sys::PointerEvent)> =
                Closure::new(move |event: web_sys::PointerEvent| {
                    let Some(button) = pointer_button_from_event(event.button()) else {
                        return;
                    };
                    let p = pointer_from_event(&event, button);
                    pending.borrow_mut().push_back(QueuedPointer::Up(p));
                    window.request_redraw();
                });
            canvas
                .add_event_listener_with_callback("pointerup", closure.as_ref().unchecked_ref())
                .expect("add pointerup listener");
            out.push(("pointerup", closure));
        }

        // pointercancel — fired when the OS / browser steals the
        // pointer (e.g., a system gesture interrupts a touch). Routed
        // to the runtime's cancel entry so in-flight press / gesture
        // captures abandon without applying release effects.
        {
            let pending = pending.clone();
            let window = window.clone();
            let closure: Closure<dyn FnMut(web_sys::PointerEvent)> =
                Closure::new(move |_event: web_sys::PointerEvent| {
                    pending.borrow_mut().push_back(QueuedPointer::Cancel);
                    window.request_redraw();
                });
            canvas
                .add_event_listener_with_callback("pointercancel", closure.as_ref().unchecked_ref())
                .expect("add pointercancel listener");
            out.push(("pointercancel", closure));
        }

        // pointerleave — pointer left the canvas. Mirrors winit's
        // CursorLeft on native; clears hover state.
        {
            let pending = pending.clone();
            let window = window.clone();
            let closure: Closure<dyn FnMut(web_sys::PointerEvent)> =
                Closure::new(move |_event: web_sys::PointerEvent| {
                    pending.borrow_mut().push_back(QueuedPointer::Leave);
                    window.request_redraw();
                });
            canvas
                .add_event_listener_with_callback("pointerleave", closure.as_ref().unchecked_ref())
                .expect("add pointerleave listener");
            out.push(("pointerleave", closure));
        }
    }

    // ===================================================================
    // Soft keyboard
    //
    // A `<canvas>` cannot summon the on-screen keyboard on touch
    // platforms — only focusable text-input DOM elements can, and only
    // when the focus comes from a user-gesture event handler. This
    // module overlays a hidden `<input type="text">` and synchronously
    // focuses it from the pointerdown DOM listener when the press would
    // land on an Damascene text-input widget. Once focused, the input
    // receives `input` events for typed characters (routed to the
    // runtime as `text_input(...)`) and `keydown` events for editing
    // keys (routed as synthetic `key_down(Backspace, ...)`).
    //
    // The native host (damascene-winit-wgpu) routes hardware keyboards
    // through winit and is unaffected by any of this. Soft keyboards
    // on a future Android winit host would use winit's own IME path.
    // ===================================================================

    /// One discrete edit produced by the soft keyboard. Drained by
    /// the host once per `window_event` and dispatched through the
    /// runtime's existing keyboard / text-input entry points so the
    /// focused widget sees the same shape it would for a hardware
    /// keystroke.
    enum TextEdit {
        /// User typed text — route as `runner.text_input(s)`.
        Insert(String),
        /// User pressed an editing key (Backspace, Enter, arrows,
        /// Delete, Home/End) — route as `runner.key_down(key, ...)`.
        Key(LogicalKey, PhysicalKey),
    }

    /// The hidden `<input>` that summons the soft keyboard plus its
    /// DOM listeners and the pending-edit queue. Held by [`Host`]
    /// for the lifetime of the page; the closures inside borrow
    /// the queue via clones of its `Rc`.
    ///
    /// Modeled on egui's `text_agent.rs` after observing that
    /// Android's keyboard refused to stay open against an
    /// `opacity:0; pointer-events:none` element. Egui keeps the
    /// element technically interactive (no pointer-events: none),
    /// uses `<input type="text">` rather than `<textarea>`, and
    /// hides it via `caret-color: transparent` +
    /// `background-color: transparent` instead of opacity. Android
    /// then treats it as a real focusable input and the keyboard
    /// stays up.
    struct SoftKeyboard {
        input: web_sys::HtmlInputElement,
        /// Whether we believe the input currently holds DOM focus.
        /// Tracked here (rather than read via `document.activeElement`
        /// every time) so `focus_if_needed` can no-op for repeated
        /// taps that don't actually need to refocus. `Rc<Cell<_>>`
        /// because the `blur` closure also writes to it when the OS
        /// dismisses the keyboard outside our control.
        focused: Rc<Cell<bool>>,
        /// Queue of edits captured by the DOM listeners since the
        /// last drain. Drained by [`Host`] inside `window_event`.
        pending: Rc<RefCell<VecDeque<TextEdit>>>,
        /// The `input` event closure; unregistered in
        /// [`Self::uninstall`].
        input_closure: Closure<dyn FnMut(web_sys::InputEvent)>,
        /// The `keydown` closure that catches editing keys
        /// (Backspace, Enter, arrow keys) the soft keyboard fires as
        /// `keydown` rather than `input`. Unregistered in
        /// [`Self::uninstall`].
        keydown_closure: Closure<dyn FnMut(web_sys::KeyboardEvent)>,
        /// The `blur` closure that resets `focused` when the OS /
        /// user dismisses the keyboard outside of our control.
        /// Unregistered in [`Self::uninstall`].
        blur_closure: Closure<dyn FnMut(web_sys::Event)>,
    }

    impl SoftKeyboard {
        /// Create the hidden input, attach it to the document, and
        /// wire up the listeners. Returns `None` if any DOM
        /// operation fails (no body, etc.) — the host then runs
        /// without soft-keyboard support, which is the correct
        /// degradation for environments where it can't work.
        fn install(canvas: &web_sys::HtmlCanvasElement, window: &Arc<Window>) -> Option<Self> {
            let document = canvas.owner_document()?;
            let input = document
                .create_element("input")
                .ok()?
                .dyn_into::<web_sys::HtmlInputElement>()
                .ok()?;
            input.set_type("text");
            // Visible-for-focus, invisible-for-the-eye. The element
            // has to remain *technically* focusable for Android's
            // keyboard to stay up — `pointer-events: none`,
            // `opacity: 0`, and `display: none` all disqualify. We
            // mirror egui's working configuration: a 1×1
            // transparent-background element with the caret hidden,
            // pinned to `(0, 0)` of the document. The canvas paints
            // on top of everything else and absorbs every visible
            // tap; the input is just a DOM focus target.
            if let Some(style) = input.dyn_ref::<web_sys::HtmlElement>().map(|e| e.style()) {
                let _ = style.set_property("position", "absolute");
                let _ = style.set_property("top", "0");
                let _ = style.set_property("left", "0");
                let _ = style.set_property("width", "1px");
                let _ = style.set_property("height", "1px");
                let _ = style.set_property("background-color", "transparent");
                let _ = style.set_property("border", "none");
                let _ = style.set_property("outline", "none");
                let _ = style.set_property("caret-color", "transparent");
            }
            // Attribute hygiene: prevent the on-screen keyboard from
            // showing autocorrect suggestions / capitalization /
            // browser autofill, which would interfere with character-
            // by-character routing into the runtime.
            let _ = input.set_attribute("autocapitalize", "off");
            let _ = input.set_attribute("autocomplete", "off");
            let _ = input.set_attribute("autocorrect", "off");
            let _ = input.set_attribute("spellcheck", "false");
            document.body()?.append_child(&input).ok()?;

            let pending: Rc<RefCell<VecDeque<TextEdit>>> = Rc::new(RefCell::new(VecDeque::new()));

            // input: fires on every character insertion and on
            // deletes. Read inputType to discriminate; route to the
            // pending queue and clear the input so the next event
            // sees only the new edit (we don't keep the input's
            // value as the source of truth — the focused Damascene
            // widget owns the actual string).
            //
            // Android Gboard workaround (from egui): after a
            // non-composition `input`, blur and refocus the element
            // so the predictive-text suggestion bar doesn't latch
            // invisible characters that have to be deleted before
            // real ones. Skip during composition (IME) since blur
            // would cancel the in-progress glyph.
            let input_pending = pending.clone();
            let input_window = window.clone();
            let input_el_for_input = input.clone();
            let input_closure: Closure<dyn FnMut(web_sys::InputEvent)> =
                Closure::new(move |event: web_sys::InputEvent| {
                    let composing = event.is_composing();
                    let input_type = event.input_type();
                    let edit = match input_type.as_str() {
                        "deleteContentBackward"
                        | "deleteWordBackward"
                        | "deleteSoftLineBackward"
                        | "deleteHardLineBackward" => Some(TextEdit::Key(
                            LogicalKey::Named(NamedKey::Backspace),
                            PhysicalKey::Unidentified,
                        )),
                        _ => {
                            let value = input_el_for_input.value();
                            if value.is_empty() || composing {
                                None
                            } else {
                                Some(TextEdit::Insert(value))
                            }
                        }
                    };
                    if !composing {
                        input_el_for_input.set_value("");
                        // Gboard reset.
                        let _ = input_el_for_input.blur();
                        let _ = input_el_for_input.focus();
                    }
                    if let Some(edit) = edit {
                        input_pending.borrow_mut().push_back(edit);
                        input_window.request_redraw();
                    }
                });
            input
                .add_event_listener_with_callback("input", input_closure.as_ref().unchecked_ref())
                .ok()?;

            // keydown: when our hidden input has focus, the canvas
            // never sees keystrokes — so we have to forward editing
            // keys (Backspace, Enter, arrows) through here. The
            // `input` handler above also covers Backspace via
            // inputType for the typical Android case; this catches
            // the iPad-with-hardware-keyboard variant where
            // Backspace fires as `keydown` only.
            let keydown_pending = pending.clone();
            let keydown_window = window.clone();
            let keydown_closure: Closure<dyn FnMut(web_sys::KeyboardEvent)> =
                Closure::new(move |event: web_sys::KeyboardEvent| {
                    // Enter never lands in the `input` handler: the
                    // hidden element is type="text", so the value
                    // stays empty and only this keydown fires —
                    // no double dispatch.
                    let logical = match event.key().as_str() {
                        "Backspace" => Some(NamedKey::Backspace),
                        "Delete" => Some(NamedKey::Delete),
                        "Enter" => Some(NamedKey::Enter),
                        "ArrowUp" => Some(NamedKey::ArrowUp),
                        "ArrowDown" => Some(NamedKey::ArrowDown),
                        "ArrowLeft" => Some(NamedKey::ArrowLeft),
                        "ArrowRight" => Some(NamedKey::ArrowRight),
                        "Home" => Some(NamedKey::Home),
                        "End" => Some(NamedKey::End),
                        _ => None,
                    };
                    if let Some(named) = logical {
                        let physical = dom_physical(&event.code());
                        keydown_pending
                            .borrow_mut()
                            .push_back(TextEdit::Key(LogicalKey::Named(named), physical));
                        keydown_window.request_redraw();
                        event.prevent_default();
                    }
                });
            input
                .add_event_listener_with_callback(
                    "keydown",
                    keydown_closure.as_ref().unchecked_ref(),
                )
                .ok()?;

            // blur: keep our `focused` mirror in sync when the
            // input loses focus outside our control (user dismissed
            // the keyboard via the OS dismiss button, tab key,
            // etc.). Without this, `focus_if_needed` would no-op
            // on the next text-input tap.
            let focused: Rc<Cell<bool>> = Rc::new(Cell::new(false));
            let blur_focused = focused.clone();
            let blur_closure: Closure<dyn FnMut(web_sys::Event)> =
                Closure::new(move |_event: web_sys::Event| {
                    blur_focused.set(false);
                });
            input
                .add_event_listener_with_callback("blur", blur_closure.as_ref().unchecked_ref())
                .ok()?;

            Some(Self {
                input,
                focused,
                pending,
                input_closure,
                keydown_closure,
                blur_closure,
            })
        }

        /// Undo [`Self::install`]: unregister the three listeners and
        /// remove the hidden input from the document. Part of the
        /// host teardown driven by [`WebHandle::destroy`] — without
        /// it every SPA remount appends another input to `<body>`.
        fn uninstall(&self) {
            let _ = self.input.remove_event_listener_with_callback(
                "input",
                self.input_closure.as_ref().unchecked_ref(),
            );
            let _ = self.input.remove_event_listener_with_callback(
                "keydown",
                self.keydown_closure.as_ref().unchecked_ref(),
            );
            let _ = self.input.remove_event_listener_with_callback(
                "blur",
                self.blur_closure.as_ref().unchecked_ref(),
            );
            self.input.remove();
        }

        /// Focus the input so the soft keyboard opens. **Must be
        /// called inside a user-gesture event handler** (e.g., the
        /// pointerdown DOM closure) — iOS suppresses programmatic
        /// focus from any other context. No-op if we believe the
        /// input already has focus.
        fn focus_if_needed(&self) {
            if !self.focused.get() {
                let _ = self.input.focus();
                self.focused.set(true);
            }
        }

        /// Blur the input so the soft keyboard dismisses. Safe to
        /// call from any context. No-op when the input isn't
        /// believed to be focused.
        fn dismiss(&self) {
            if self.focused.get() {
                let _ = self.input.blur();
                self.focused.set(false);
            }
        }

        /// Drain pending edits captured by the listeners since the
        /// last drain. Called by the host inside `window_event`.
        fn drain(&self) -> Vec<TextEdit> {
            self.pending.borrow_mut().drain(..).collect()
        }
    }

    /// Mirrors the native winit + wgpu host shape, but with browser
    /// surface init (async via wasm-bindgen-futures rather than
    /// pollster). Kept inline here so `damascene-winit-wgpu` stays free of
    /// wasm-only deps.
    struct Host<A: App> {
        config: WebHostConfig,
        app: A,
        handle: WebHandle,
        gfx: Rc<RefCell<Option<Gfx>>>,
        last_pointer: Option<(f32, f32)>,
        modifiers: KeyModifiers,
        stats: FrameStats,
        /// Last cursor pushed to `Window::set_cursor`. winit-web maps
        /// the icon to `canvas.style.cursor` so this drives the
        /// browser's CSS cursor; we cache to avoid resetting the same
        /// string each frame.
        last_cursor: Cursor,
        /// Reason the next redraw is being requested. Each event handler
        /// that calls `request_redraw` sets this beforehand; the
        /// RedrawRequested arm consumes it once and snapshots it into
        /// [`HostDiagnostics::trigger`]. Defaults back to `Other` after
        /// each consume — safe fallback for redraws the host can't
        /// attribute (e.g. the post-async-setup `request_redraw`).
        next_trigger: FrameTrigger,
        /// Wall clock at the start of the previous redraw; diff with
        /// the next frame's start gives `last_frame_dt`.
        last_frame_at: Option<Instant>,
        /// Counts redraws actually rendered.
        frame_index: u64,
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
        /// Physical canvas size used by the most recent full
        /// [`Runner::prepare`] call. The repaint dispatcher requires
        /// this to match the current `gfx.config` size before taking
        /// the paint-only path: the cached `DrawOp` list was laid out
        /// against this size, so a `ResizeObserver` fire that updated
        /// `gfx.config` since must force a fresh layout rather than
        /// painting stale geometry to the new viewport.
        last_prepared_size: Option<(u32, u32)>,
        /// Adapter backend tag, captured at adapter selection time.
        /// `Rc<RefCell>` because the surface is created in an async
        /// task that finishes after `Host::new`; the cell is read
        /// each frame in the RedrawRequested arm.
        backend: Rc<RefCell<&'static str>>,
        /// Browser `paste` events carry trusted clipboard text without
        /// the Firefox permission menu used by `navigator.clipboard.readText`.
        /// The callback enqueues text here, then requests a redraw; the
        /// RedrawRequested arm converts it into a focused Damascene `TextInput`.
        pending_clipboard_text: Rc<RefCell<VecDeque<String>>>,
        /// Web browsers do not expose the X11/Wayland primary-selection
        /// clipboard. Keep an app-local approximation so Damascene selection
        /// highlight can still feed middle-click paste inside the canvas.
        primary_selection: String,
        /// Diagnostics snapshot from the last built frame, retained so
        /// event dispatch can attach it to [`damascene_core::EventCx`].
        last_diagnostics: Option<damascene_core::HostDiagnostics>,
        /// The canvas the host bound to, stored at `resumed()` so
        /// [`Self::teardown`] can unregister listeners even after the
        /// embedding page detached the element from the document.
        canvas: Option<web_sys::HtmlCanvasElement>,
        /// The JS paste callback object; held alive for the host's
        /// lifetime and unregistered in [`Self::teardown`].
        paste_closure: Option<Closure<dyn FnMut(web_sys::ClipboardEvent)>>,
        /// The JS keydown callback object; held alive for the host's
        /// lifetime and unregistered in [`Self::teardown`].
        keydown_closure: Option<Closure<dyn FnMut(web_sys::KeyboardEvent)>>,
        /// The JS callback object that ResizeObserver fires; held
        /// alive for the host's lifetime. Dropping it alone would NOT
        /// stop the observer — [`Self::teardown`] calls
        /// `disconnect()` on the observer first.
        resize_closure: Option<Closure<dyn FnMut()>>,
        /// The observer itself; disconnected in [`Self::teardown`].
        resize_observer: Option<web_sys::ResizeObserver>,
        /// DOM pointer events captured by the listeners installed in
        /// `resumed()`. Drained at the top of every `window_event`
        /// call so dispatch into the runner and app uses the same
        /// `&mut self` path the rest of the host does.
        pending_pointer: Rc<RefCell<VecDeque<QueuedPointer>>>,
        /// The JS callbacks for each of pointermove / pointerdown /
        /// pointerup / pointercancel / pointerleave on the canvas,
        /// paired with their event names so [`Self::teardown`] can
        /// unregister them.
        pointer_closures: PointerListeners,
        /// The JS callback that calls `preventDefault` on
        /// `contextmenu` so the browser's native menu doesn't pop
        /// over the canvas. Right-click already emits
        /// `PointerButton::Secondary` through the pointer listeners;
        /// this just suppresses the platform menu so apps can render
        /// their own. Unregistered in [`Self::teardown`].
        contextmenu_closure: Option<Closure<dyn FnMut(web_sys::MouseEvent)>>,
        /// Bottom safe-area inset in logical pixels, set by the
        /// VisualViewport `resize` listener whenever the keyboard
        /// (or any other platform chrome that shrinks the visual
        /// viewport) appears or disappears. The cell is shared with
        /// the JS callback via `Rc<Cell<f32>>`; the host reads it
        /// each frame and feeds it into `BuildCx::with_safe_area`.
        keyboard_inset_bottom: Rc<Cell<f32>>,
        /// The JS callback that updates `keyboard_inset_bottom` on
        /// visualViewport resize. None on browsers that don't expose
        /// `window.visualViewport` (older engines / jsdom-style test
        /// contexts). Unregistered in [`Self::teardown`].
        viewport_closure: Option<Closure<dyn FnMut(web_sys::Event)>>,
        /// Hidden `<input type="text">` that summons the on-screen
        /// keyboard when a touch press lands on an Damascene text-input
        /// widget. `None` when soft-keyboard install failed (no body,
        /// etc.) — the host still runs, just without
        /// on-screen-keyboard support. Shared with the pointerdown
        /// closure via `Rc` clone so focus-on-press can fire in the
        /// user-gesture context.
        soft_keyboard: Option<Rc<SoftKeyboard>>,
    }

    struct Gfx {
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        config: wgpu::SurfaceConfiguration,
        renderer: Runner,
        /// `None` when [`SAMPLE_COUNT`] is 1 — the renderer draws
        /// straight into the swapchain texture and there's no resolve
        /// pass. `Some` when MSAA is enabled, holding the
        /// multisampled colour attachment that the swapchain texture
        /// is the resolve target for.
        msaa: Option<damascene_wgpu::MsaaTarget>,
        /// Format used for render-target views and pipelines. May
        /// differ from `config.format` when we re-view a linear
        /// swapchain texture as sRGB (Chromium WebGPU path) — the
        /// swapchain stores `Rgba8Unorm`, but every view is
        /// `Rgba8UnormSrgb` so the hardware encodes on write.
        render_format: wgpu::TextureFormat,
    }

    /// Logical-pixel viewport currently configured on the canvas — the
    /// value the next `build` sees, so event-time layout math agrees
    /// with build-time. `None` only when the scale factor is degenerate.
    fn logical_viewport_of(gfx: &Gfx) -> Option<(f32, f32)> {
        let scale = gfx.window.scale_factor() as f32;
        if scale <= 0.0 {
            return None;
        }
        Some((
            gfx.config.width as f32 / scale,
            gfx.config.height as f32 / scale,
        ))
    }

    fn surface_extent(config: &wgpu::SurfaceConfiguration) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        }
    }

    impl<A: App> Host<A> {
        fn new(config: WebHostConfig, app: A, handle: WebHandle) -> Self {
            Self {
                config,
                app,
                handle,
                gfx: Rc::new(RefCell::new(None)),
                last_pointer: None,
                modifiers: KeyModifiers::default(),
                stats: FrameStats::default(),
                last_cursor: Cursor::Default,
                next_trigger: FrameTrigger::Initial,
                last_frame_at: None,
                frame_index: 0,
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
                last_prepared_size: None,
                backend: Rc::new(RefCell::new("?")),
                pending_clipboard_text: Rc::new(RefCell::new(VecDeque::new())),
                primary_selection: String::new(),
                last_diagnostics: None,
                canvas: None,
                paste_closure: None,
                keydown_closure: None,
                resize_closure: None,
                resize_observer: None,
                pending_pointer: Rc::new(RefCell::new(VecDeque::new())),
                pointer_closures: Vec::new(),
                contextmenu_closure: None,
                keyboard_inset_bottom: Rc::new(Cell::new(0.0)),
                viewport_closure: None,
                soft_keyboard: None,
            }
        }

        /// Undo everything `resumed()` installed: unregister the DOM
        /// listeners, disconnect the ResizeObserver, uninstall the
        /// soft-keyboard input, release the GPU surface, and drop the
        /// handle's window reference so the winit `Window` (which
        /// owns winit's own canvas listeners) can die.
        ///
        /// Runs from [`ApplicationHandler::exiting`] when
        /// [`WebHandle::destroy`] stops the loop. Idempotent: every
        /// step `take()`s its state, so a second call is a no-op.
        /// Unregistration must happen explicitly — dropping a
        /// `Closure` only invalidates the JS function; the DOM
        /// registration would survive and throw on its next fire.
        fn teardown(&mut self) {
            // Disconnect before dropping the closure: a disconnected
            // observer can never fire into the dead closure.
            if let Some(observer) = self.resize_observer.take() {
                observer.disconnect();
            }
            self.resize_closure = None;

            if let Some(canvas) = self.canvas.take() {
                for (event, closure) in self.pointer_closures.drain(..) {
                    let _ = canvas.remove_event_listener_with_callback(
                        event,
                        closure.as_ref().unchecked_ref(),
                    );
                }
                if let Some(closure) = self.keydown_closure.take() {
                    let _ = canvas.remove_event_listener_with_callback(
                        "keydown",
                        closure.as_ref().unchecked_ref(),
                    );
                }
                if let Some(closure) = self.contextmenu_closure.take() {
                    let _ = canvas.remove_event_listener_with_callback(
                        "contextmenu",
                        closure.as_ref().unchecked_ref(),
                    );
                }
                // The paste listener was registered on the document,
                // not the canvas.
                if let Some(closure) = self.paste_closure.take()
                    && let Some(document) = canvas.owner_document()
                {
                    let _ = document.remove_event_listener_with_callback(
                        "paste",
                        closure.as_ref().unchecked_ref(),
                    );
                }
            }

            if let Some(closure) = self.viewport_closure.take()
                && let Some(window_obj) = web_sys::window()
                && let Some(vv) = window_obj.visual_viewport()
            {
                let _ = vv.remove_event_listener_with_callback(
                    "resize",
                    closure.as_ref().unchecked_ref(),
                );
            }

            if let Some(soft_keyboard) = self.soft_keyboard.take() {
                soft_keyboard.uninstall();
            }

            // Release the surface / device / queue. The async setup
            // task may still be in flight; if it completes after this
            // it writes a fresh Gfx into the cell of an exited loop —
            // harmless, and freed when the task's Rc clones drop.
            self.gfx.borrow_mut().take();
            // Let the winit Window die even while the embedder still
            // holds WebHandle clones.
            self.handle.clear_window();
        }

        /// Drain DOM PointerEvents captured by the listeners since the
        /// last `window_event` call and dispatch them through the
        /// runner + app the same way native winit pointer events do.
        ///
        /// Returns `true` when at least one event triggered a redraw
        /// — the host uses this to set `next_trigger` for the next
        /// frame's diagnostics.
        fn drain_pending_pointer(&mut self, gfx: &mut Gfx) -> bool {
            // Drain time-driven events (touch long-press) before any
            // queued DOM input. Even on frames where no DOM event
            // arrived (the user held still through the long-press
            // deadline), this still needs to fire — `next_redraw_in`
            // schedules the wakeup that brings us here.
            let mut redraw = false;
            let polled = gfx.renderer.poll_input(Instant::now());
            if !polled.is_empty() {
                redraw = true;
                for event in polled {
                    dispatch_app_event(
                        &mut self.app,
                        event,
                        &gfx.renderer,
                        logical_viewport_of(gfx),
                        self.last_diagnostics.as_ref(),
                        &mut self.primary_selection,
                    );
                }
            }
            let queue: Vec<QueuedPointer> = self.pending_pointer.borrow_mut().drain(..).collect();
            if queue.is_empty() {
                return redraw;
            }
            for queued in queue {
                match queued {
                    QueuedPointer::Move(p) => {
                        self.last_pointer = Some((p.x, p.y));
                        let moved = gfx.renderer.pointer_moved(p);
                        for event in moved.events {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                        if moved.needs_redraw {
                            redraw = true;
                        }
                    }
                    QueuedPointer::Down(p) => {
                        self.last_pointer = Some((p.x, p.y));
                        for event in gfx.renderer.pointer_down(p) {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                        redraw = true;
                    }
                    QueuedPointer::Up(p) => {
                        self.last_pointer = Some((p.x, p.y));
                        for event in gfx.renderer.pointer_up(p) {
                            let event =
                                attach_primary_selection_text(event, &self.primary_selection);
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                        redraw = true;
                    }
                    QueuedPointer::Cancel => {
                        // A real cancel, not an up: abandons in-flight
                        // gesture captures without applying release
                        // effects, and regardless of which button began
                        // them (a synthesized Primary up couldn't end a
                        // Secondary/Middle-initiated drag).
                        self.last_pointer = None;
                        for event in gfx.renderer.pointer_cancelled() {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                        redraw = true;
                    }
                    QueuedPointer::Leave => {
                        self.last_pointer = None;
                        for event in gfx.renderer.pointer_left() {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                        redraw = true;
                    }
                }
            }
            redraw
        }

        /// Drain edits captured by the soft-keyboard input since
        /// the last `window_event` and route them through the
        /// runner's existing keyboard / text-input entry points so
        /// the focused widget sees the same shape it would for a
        /// hardware keystroke. Returns `true` when at least one edit
        /// was dispatched so the caller can mark the next-frame
        /// trigger.
        fn drain_soft_keyboard(&mut self, gfx: &mut Gfx) -> bool {
            let Some(sk) = self.soft_keyboard.as_ref() else {
                return false;
            };
            let edits = sk.drain();
            if edits.is_empty() {
                return false;
            }
            for edit in edits {
                match edit {
                    TextEdit::Insert(text) => {
                        if let Some(event) = gfx.renderer.text_input(text) {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                    }
                    TextEdit::Key(logical, physical) => {
                        for event in gfx
                            .renderer
                            .key_down(logical, physical, self.modifiers, false)
                        {
                            dispatch_app_event(
                                &mut self.app,
                                event,
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
                            );
                        }
                    }
                }
            }
            true
        }

        /// Sync the soft keyboard's open/closed state with the
        /// runner's current focus. Called once per `window_event`
        /// after pointer / soft-keyboard drain so a press that
        /// shifted focus away from a text input can dismiss the
        /// on-screen keyboard within the same frame.
        ///
        /// We never *open* the keyboard from here — that has to
        /// happen synchronously inside the pointerdown closure for
        /// iOS to honor it. Closing has no such restriction.
        fn sync_soft_keyboard_focus(&self, gfx: &Gfx) {
            let Some(sk) = self.soft_keyboard.as_ref() else {
                return;
            };
            // Only dismiss when our state says the keyboard should
            // be down AND the DOM input still believes it's focused.
            // Skipping the .blur() when DOM focus is already gone
            // avoids redundant blur events; more importantly, it
            // means a stray sync that races a still-resolving focus
            // doesn't tear the keyboard down out from under itself.
            if !gfx.renderer.focused_captures_keys() && sk.focused.get() {
                sk.dismiss();
            }
        }
    }

    fn backend_label(backend: wgpu::Backend) -> &'static str {
        match backend {
            wgpu::Backend::Vulkan => "Vulkan",
            wgpu::Backend::Metal => "Metal",
            wgpu::Backend::Dx12 => "DX12",
            wgpu::Backend::Gl => "WebGL2",
            wgpu::Backend::BrowserWebGpu => "WebGPU",
            wgpu::Backend::Noop => "noop",
        }
    }

    /// sRGB-tagged view-format sibling for a linear `*8Unorm` swapchain
    /// format. Used to recover gamma-correct output on Chromium's WebGPU
    /// surface: the swapchain offers only linear formats there, so we
    /// declare the sRGB form as a view format and render through that —
    /// hardware applies the sRGB encode on store and the compositor
    /// reads gamma-correct pixels. Returns `None` for formats that have
    /// no sRGB sibling (e.g. `Rgba16Float`, where the float storage is
    /// already linear-precision-correct), in which case the caller
    /// keeps the chosen format unchanged.
    fn srgb_view_of(format: wgpu::TextureFormat) -> Option<wgpu::TextureFormat> {
        use wgpu::TextureFormat as F;
        match format {
            F::Rgba8Unorm => Some(F::Rgba8UnormSrgb),
            F::Bgra8Unorm => Some(F::Bgra8UnormSrgb),
            _ => None,
        }
    }

    impl<A: App + 'static> ApplicationHandler for Host<A> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            // Destroyed before the loop's first turn: exit without
            // installing anything, so there is nothing to tear down.
            if self.handle.destroy_requested() {
                event_loop.exit();
                return;
            }
            if self.gfx.borrow().is_some() {
                return;
            }
            let canvas = locate_canvas(&self.config.canvas_id);
            self.canvas = Some(canvas.clone());

            // Build the window bound to the existing canvas. We do
            // *not* call `with_inner_size` — on the web backend that
            // forces canvas.width/height to the requested physical
            // pixels, which then disagrees with the surface size if
            // we read it from CSS. Letting winit pick from the canvas
            // attributes (default 300×150 if unset, otherwise whatever
            // the host page declared) keeps inner_size() and the
            // canvas backing buffer in lockstep. The ResizeObserver
            // installed below carries the canvas through later layout
            // changes; we don't depend on winit dispatching `Resized`.
            let attrs = Window::default_attributes()
                .with_canvas(Some(canvas.clone()))
                // Browser paste, including Linux middle-click primary
                // paste, is delivered as a DOM ClipboardEvent. winit's
                // default web preventDefault path suppresses those
                // browser-side events, so Damascene handles clipboard
                // suppression at the document paste listener instead.
                .with_prevent_default(false);
            let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
            self.handle.set_window(window.clone());

            // Force the canvas backing buffer to match the canvas's
            // CSS-laid-out size at the device pixel ratio. Without
            // this the canvas defaults to 300×150 device pixels, the
            // swapchain ends up tiny and stretched, and Firefox's
            // WebGPU backend fails the first present with "not enough
            // memory left" because the surface texture and the canvas
            // drawing buffer disagree. winit's `Window::inner_size()`
            // reads canvas.width/canvas.height on the web backend, so
            // setting them here is what the async surface setup picks
            // up for the initial swap-chain dimensions.
            let viewport = self.config.viewport;
            let (initial_w, initial_h) = measure_canvas(&canvas, viewport);
            canvas.set_width(initial_w);
            canvas.set_height(initial_h);

            // Keep the canvas backing buffer tracking its CSS box
            // size for the lifetime of the page. ResizeObserver fires
            // once on observe() with the initial size, then again
            // every time the canvas's content rect changes. We bypass
            // winit's `request_inner_size` round-trip — its web
            // backend doesn't reliably translate that into a
            // `Resized` event, which left the swapchain stretched
            // mid-session — and reconfigure the surface directly via
            // `apply_canvas_size`. Until the async surface setup
            // completes we just keep canvas.width/height in sync so
            // the eventual `inner_size()` read picks up the latest.
            let canvas_for_observer = canvas.clone();
            let window_for_observer = window.clone();
            let gfx_for_observer = self.gfx.clone();
            let resize_closure: Closure<dyn FnMut()> = Closure::new(move || {
                let (phys_w, phys_h) = measure_canvas(&canvas_for_observer, viewport);
                let mut gfx_borrow = gfx_for_observer.borrow_mut();
                if let Some(gfx) = gfx_borrow.as_mut() {
                    apply_canvas_size(&canvas_for_observer, gfx, phys_w, phys_h);
                } else {
                    canvas_for_observer.set_width(phys_w);
                    canvas_for_observer.set_height(phys_h);
                }
                drop(gfx_borrow);
                window_for_observer.request_redraw();
            });
            let observer = web_sys::ResizeObserver::new(resize_closure.as_ref().unchecked_ref())
                .expect("ResizeObserver::new failed");
            observer.observe(&canvas);
            self.resize_closure = Some(resize_closure);
            self.resize_observer = Some(observer);

            let pending_clipboard_text = self.pending_clipboard_text.clone();
            let window_for_paste = window.clone();
            let paste_closure: Closure<dyn FnMut(web_sys::ClipboardEvent)> =
                Closure::new(move |event: web_sys::ClipboardEvent| {
                    let Some(data) = event.clipboard_data() else {
                        log::warn!("damascene-web: paste event had no clipboardData");
                        return;
                    };
                    let Ok(text) = data.get_data("text/plain") else {
                        log::warn!("damascene-web: paste event could not read text/plain");
                        return;
                    };
                    if text.is_empty() {
                        return;
                    }
                    event.prevent_default();
                    event.stop_propagation();
                    pending_clipboard_text.borrow_mut().push_back(text);
                    window_for_paste.request_redraw();
                });
            canvas
                .owner_document()
                .expect("canvas has no owner document")
                .add_event_listener_with_callback("paste", paste_closure.as_ref().unchecked_ref())
                .expect("add paste listener");
            self.paste_closure = Some(paste_closure);

            let keydown_closure: Closure<dyn FnMut(web_sys::KeyboardEvent)> =
                Closure::new(move |event: web_sys::KeyboardEvent| {
                    if should_prevent_browser_key_default(&event) {
                        event.prevent_default();
                    }
                });
            canvas
                .add_event_listener_with_callback(
                    "keydown",
                    keydown_closure.as_ref().unchecked_ref(),
                )
                .expect("add keydown listener");
            self.keydown_closure = Some(keydown_closure);

            // Tell the browser the canvas owns all touch input —
            // without this, `touch-action: auto` (the default) makes
            // touch-drag pan/zoom the page before any PointerEvent
            // ever fires, so the runtime sees nothing. Setting it on
            // the element matches what touch-first canvas apps
            // (drawing tools, games) ship.
            if let Some(style) = canvas.dyn_ref::<web_sys::HtmlElement>().map(|e| e.style()) {
                let _ = style.set_property("touch-action", "none");
            }

            // Soft-keyboard plumbing. Install before the pointer
            // listeners so the pointerdown closure can call into it
            // synchronously from the user-gesture context. Failure
            // to install (no body, etc.) leaves the host running
            // without on-screen-keyboard support, which is the
            // correct degradation for environments where it can't
            // work.
            self.soft_keyboard = SoftKeyboard::install(&canvas, &window).map(Rc::new);
            if self.soft_keyboard.is_none() {
                log::warn!(
                    "damascene-web: soft keyboard install failed; text input will not summon \
                     the on-screen keyboard"
                );
            }

            // Bind DOM PointerEvent directly. winit on the browser
            // collapses touch and pen to mouse before forwarding, so
            // routing through `WindowEvent::MouseInput` would lose
            // the modality, the per-pointer ID, and pressure — the
            // exact information the runtime needs to specialize for
            // touch. Each listener pushes onto `pending_pointer` and
            // requests a redraw; the next `window_event` call drains
            // the queue and dispatches into the runner + app with
            // full host state. The compatibility mouse events winit
            // would otherwise translate are ignored further down by
            // this file deliberately not handling
            // `WindowEvent::MouseInput` / `CursorMoved` /
            // `CursorLeft` on web.
            install_pointer_listeners(
                &canvas,
                &window,
                &self.pending_pointer,
                &self.gfx,
                self.soft_keyboard.as_ref(),
                &mut self.pointer_closures,
            );

            // Suppress the browser's native context menu on the
            // canvas. Right-click already routes to the runtime as
            // `PointerButton::Secondary` via the pointerdown listener
            // above; without this the platform menu pops on top of
            // the app and intercepts subsequent input. Apps that want
            // an Damascene-rendered menu wire it through the Secondary
            // press path as they would on native.
            let contextmenu_closure: Closure<dyn FnMut(web_sys::MouseEvent)> =
                Closure::new(move |event: web_sys::MouseEvent| {
                    event.prevent_default();
                });
            canvas
                .add_event_listener_with_callback(
                    "contextmenu",
                    contextmenu_closure.as_ref().unchecked_ref(),
                )
                .expect("add contextmenu listener");
            self.contextmenu_closure = Some(contextmenu_closure);

            // VisualViewport reports the visible region of the page
            // minus platform chrome. When the on-screen keyboard
            // appears, `visualViewport.height` shrinks while
            // `window.innerHeight` (the layout viewport) doesn't —
            // the difference is the keyboard inset, which apps read
            // through `BuildCx::safe_area_bottom` and use to inset
            // their interactive content. Skip silently on browsers
            // without VisualViewport (older engines, jsdom).
            if let Some(window_obj) = web_sys::window()
                && let Some(vv) = window_obj.visual_viewport()
            {
                let cell = self.keyboard_inset_bottom.clone();
                let layout_window = window_obj.clone();
                // Seed the cell with the current value so the first
                // frame after install has the right inset (handles
                // the case of resuming a tab where the keyboard is
                // already up). Clamp small differences (URL-bar
                // hide/show varies inner_height vs visualViewport by
                // ~5px on iOS Safari) so the seed reads as zero.
                let initial_inset = ((layout_window
                    .inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    - vv.height())
                .max(0.0) as f32)
                    .max(0.0);
                let initial_inset = if initial_inset < 16.0 {
                    0.0
                } else {
                    initial_inset
                };
                cell.set(initial_inset);
                // Note: this listener intentionally does *not* call
                // `request_redraw`. The keyboard appearing already
                // chains through the focus that summoned it
                // (animation deadlines drive the next few frames),
                // and inserting an extra redraw here on Android
                // raced with the just-summoned soft keyboard's
                // focus and dismissed it almost immediately. The
                // cell is read by `BuildCx::with_safe_area` each
                // frame; whichever frame fires next picks up the
                // new value.
                let viewport_closure: Closure<dyn FnMut(web_sys::Event)> =
                    Closure::new(move |_event: web_sys::Event| {
                        let Some(window_obj) = web_sys::window() else {
                            return;
                        };
                        let Some(vv) = window_obj.visual_viewport() else {
                            return;
                        };
                        let layout_h = window_obj
                            .inner_height()
                            .ok()
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let visible_h = vv.height();
                        let raw = (layout_h - visible_h).max(0.0) as f32;
                        // Same small-difference clamp as the seed —
                        // keeps URL-bar jitter from looking like a
                        // tiny keyboard.
                        let inset = if raw < 16.0 { 0.0 } else { raw };
                        cell.set(inset);
                    });
                vv.add_event_listener_with_callback(
                    "resize",
                    viewport_closure.as_ref().unchecked_ref(),
                )
                .expect("add visualViewport resize listener");
                self.viewport_closure = Some(viewport_closure);
            }

            // Allow both browser backends. wgpu's synchronous
            // Instance::new() can't safely decide this: if
            // `navigator.gpu` exists, it routes the whole instance
            // through WebGPU, even on browsers/GPUs where
            // requestAdapter() later returns null. The async helper
            // probes adapter creation first and removes WebGPU from the
            // descriptor when it is not really usable, letting WebGL2
            // handle Chrome/Linux-style partial support instead of
            // panicking during adapter selection.
            //
            // WebGPU is required for backdrop-sampling shaders
            // (`liquid_glass`) because WebGL2 surfaces don't advertise
            // `COPY_SRC` on the swapchain texture, so the snapshot copy
            // can't run — we register backdrop shaders only when the
            // chosen adapter's surface supports COPY_SRC, which in
            // practice means "WebGPU was selected."
            //
            // Firefox: as of 2026-05, Firefox's WebGPU implementation
            // still wedges its compositor on pointer events with our
            // atlas-uploading path (whole canvas goes black until the
            // cursor leaves). The workaround on the user side is to
            // disable WebGPU in `about:config` (`dom.webgpu.enabled =
            // false`); wgpu then transparently picks WebGL2 here and
            // backdrop shaders are skipped via the COPY_SRC check
            // below. Revisit when Firefox WebGPU stabilises.
            // Adapter + device requests are async on wasm; spawn the
            // setup as a future and stash the result in self.gfx so
            // subsequent resumed/window_event calls find it ready.
            //
            // `App::shaders()` is captured here (before the move into
            // the async block) so the runner can register custom
            // shaders the App declares — including backdrop-sampling
            // ones like `liquid_glass`. Without this the showcase's
            // glass card draws are silently dropped because the
            // pipeline doesn't exist.
            let shaders = self.app.shaders();
            let theme = self.app.theme();
            let gfx_slot = self.gfx.clone();
            let backend_slot = self.backend.clone();
            let window_for_async = window.clone();
            let handle_for_async = self.handle.clone();
            let canvas_for_async = canvas.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
                instance_desc.backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
                let instance = wgpu::util::new_instance_with_webgpu_detection(instance_desc).await;
                // Every failure below is a legitimate platform outcome
                // (no WebGPU *and* no WebGL2, GPU process crashed,
                // driver denylisted, …), not a bug — report it to the
                // page instead of panicking, because this task runs
                // after `init()` resolved and a panic here is
                // uncatchable from page JS.
                let surface = match instance.create_surface(window_for_async.clone()) {
                    Ok(surface) => surface,
                    Err(err) => {
                        report_setup_error(
                            &canvas_for_async,
                            &format!("could not create a rendering surface for the canvas: {err}"),
                        );
                        return;
                    }
                };

                let adapter = match instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::default(),
                        compatible_surface: Some(&surface),
                        force_fallback_adapter: false,
                        apply_limit_buckets: false,
                    })
                    .await
                {
                    Ok(adapter) => adapter,
                    Err(err) => {
                        report_setup_error(
                            &canvas_for_async,
                            &format!(
                                "no compatible GPU adapter — this browser offers neither usable \
                                 WebGPU nor WebGL2 ({err})"
                            ),
                        );
                        return;
                    }
                };

                // Log the adapter we actually got. `Backends::BROWSER_WEBGPU
                // | Backends::GL` silently falls back to WebGL2 if the
                // browser's WebGPU init fails, and WebGL2 frames cost
                // an order of magnitude more GPU time than WebGPU on
                // the same scene — so this is the first thing to check
                // when investigating "why is it slow on the web".
                let info = adapter.get_info();
                log::info!(
                    "damascene-web: adapter selected — backend={:?} name={:?} driver={:?} device_type={:?}",
                    info.backend,
                    info.name,
                    info.driver,
                    info.device_type,
                );
                *backend_slot.borrow_mut() = backend_label(info.backend);

                // What the runner must adapt to on this adapter — naga's
                // GLSL ES target rejects per-sample interpolation and
                // depth-texture loads at shader-module creation, so these
                // have to be known up front. See `RunnerCaps` for the
                // per-cap details (including the SwiftShader caveat that
                // makes GL distrusted wholesale).
                let caps = RunnerCaps::from_adapter(&adapter);
                if !caps.per_sample_shading {
                    log::info!(
                        "damascene-web: per-sample shading unavailable on selected backend; \
                         shaders will downlevel `@interpolate(perspective, sample)` to per-pixel-centre interpolation"
                    );
                }
                if !caps.depth_readback {
                    log::info!(
                        "damascene-web: depth-attachment read-back unavailable on WebGL2; \
                         3D scene label occlusion uses the packed depth-as-color capture"
                    );
                }

                // WebGL2 has a tighter feature/limit envelope than
                // native; downlevel_webgl2_defaults is the matching
                // baseline. Cap at the adapter's actual limits so
                // device creation succeeds on every integrated GPU.
                let limits =
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());

                let (device, queue) = match adapter
                    .request_device(&wgpu::DeviceDescriptor {
                        label: Some("damascene_web::device"),
                        required_features: wgpu::Features::empty(),
                        required_limits: limits,
                        experimental_features: wgpu::ExperimentalFeatures::default(),
                        memory_hints: wgpu::MemoryHints::Performance,
                        trace: wgpu::Trace::Off,
                    })
                    .await
                {
                    Ok(pair) => pair,
                    Err(err) => {
                        report_setup_error(
                            &canvas_for_async,
                            &format!("GPU device creation failed on the selected adapter: {err}"),
                        );
                        return;
                    }
                };

                let surface_caps = surface.get_capabilities(&adapter);
                let format = surface_caps
                    .formats
                    .iter()
                    .copied()
                    .find(|f| f.is_srgb())
                    .unwrap_or(surface_caps.formats[0]);
                // Decide the render-target view format. If the chosen
                // swapchain format is already sRGB-tagged (native, most
                // browsers' WebGL2 surfaces), this collapses to the
                // same format. Chromium's WebGPU surface offers only
                // linear formats — `Rgba8Unorm`, `Bgra8Unorm`,
                // `Rgba16Float` — so without this fix-up our shaders'
                // linear writes hit the compositor uncorrected and the
                // page renders 2.2-gamma's worth darker than native.
                // The trick: keep the swapchain format as `Rgba8Unorm`
                // (storage), declare `Rgba8UnormSrgb` as a view format,
                // and create every render-target view through that. The
                // hardware applies the sRGB encode on store. WebGPU
                // explicitly permits this view-format reinterpretation
                // because the two formats differ only in the sRGB flag.
                let render_format = srgb_view_of(format).unwrap_or(format);
                let view_formats = if render_format != format {
                    vec![render_format]
                } else {
                    Vec::new()
                };
                log::info!(
                    "damascene-web: surface format {:?} (sRGB? {}) → render view {:?}; offered {:?}",
                    format,
                    format.is_srgb(),
                    render_format,
                    surface_caps.formats,
                );
                // Single source of truth for the swapchain size:
                // winit's inner_size() in physical pixels. Same value
                // that the native winit + wgpu host uses; matches what
                // sync_canvas_to_css() set the canvas backing buffer to.
                let inner = window_for_async.inner_size();
                // COPY_SRC is required so backdrop-sampling shaders can
                // copy the post-Pass-A surface into the runner's
                // snapshot texture mid-frame. WebGL2 surfaces typically
                // advertise it; if the adapter ever doesn't, we fall
                // back to RENDER_ATTACHMENT-only and any backdrop
                // shaders the App declared simply won't paint a glass
                // surface (the rest of the UI is unaffected).
                let want_copy_src = surface_caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
                let usage = if want_copy_src {
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
                } else {
                    log::warn!(
                        "damascene-web: surface does not advertise COPY_SRC; backdrop-sampling \
                         shaders will paint nothing on this backend"
                    );
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                };
                // Prefer Fifo (vsync) so redraws can't outrun the
                // browser's compositor — same rationale as
                // damascene-winit-wgpu.
                let present_mode = if surface_caps
                    .present_modes
                    .contains(&wgpu::PresentMode::Fifo)
                {
                    wgpu::PresentMode::Fifo
                } else {
                    surface_caps.present_modes[0]
                };
                let config = wgpu::SurfaceConfiguration {
                    usage,
                    format,
                    width: inner.width.max(1),
                    height: inner.height.max(1),
                    present_mode,
                    alpha_mode: surface_caps.alpha_modes[0],
                    view_formats,
                    // `Auto` keeps the canvas defaults (sRGB, standard tone
                    // mapping) — the web host negotiates SDR today. HDR
                    // canvas output needs an explicit `ExtendedSrgb` request
                    // and is part of the color-negotiation follow-up.
                    color_space: wgpu::SurfaceColorSpace::Auto,
                    desired_maximum_frame_latency: 2,
                };
                surface.configure(&device, &config);

                let mut renderer =
                    Runner::with_caps(&device, &queue, render_format, SAMPLE_COUNT, caps);
                renderer.set_theme(theme);
                renderer.set_surface_size(config.width, config.height);
                // Register every shader the App declared. If the
                // surface doesn't support COPY_SRC (so multi-pass
                // backdrop sampling is impossible), skip the backdrop
                // shaders rather than registering them and rendering
                // garbage.
                for s in shaders {
                    if s.samples_backdrop && !want_copy_src {
                        continue;
                    }
                    renderer.register_shader_with(
                        &device,
                        s.name,
                        s.wgsl,
                        s.samples_backdrop,
                        s.samples_time,
                    );
                }

                // MSAA target only when SAMPLE_COUNT > 1; the
                // single-sample path renders straight into the
                // swapchain texture.
                let msaa = if SAMPLE_COUNT > 1 {
                    Some(damascene_wgpu::MsaaTarget::new(
                        &device,
                        render_format,
                        surface_extent(&config),
                        SAMPLE_COUNT,
                    ))
                } else {
                    None
                };
                *gfx_slot.borrow_mut() = Some(Gfx {
                    window: window_for_async.clone(),
                    surface,
                    device,
                    queue,
                    config,
                    renderer,
                    msaa,
                    render_format,
                });
                if handle_for_async.mark_ready() {
                    log::debug!("damascene-web: flushing pending external redraw request");
                }
                window_for_async.request_redraw();
            });
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            // [`WebHandle::destroy`] sets the flag and wakes the loop;
            // this check must run before the gfx guard below so a
            // destroy that races the still-async GPU setup is honored.
            // `exit()` fires `LoopExiting` → [`Self::exiting`] runs
            // the DOM teardown, then winit drops this Host.
            if self.handle.destroy_requested() {
                event_loop.exit();
                return;
            }
            // Clone the `Rc` first so the `RefMut` we get from
            // `borrow_mut` is tied to the cloned cell rather than
            // through `&self.gfx` — that lets `drain_pending_pointer`
            // re-borrow `self` mutably while `gfx_borrow` is still
            // live.
            let gfx_cell = self.gfx.clone();
            let mut gfx_borrow = gfx_cell.borrow_mut();
            let Some(gfx) = gfx_borrow.as_mut() else {
                // Async setup hasn't finished; drop the event. The
                // post-setup `request_redraw` will trigger a fresh
                // RedrawRequested once we're ready.
                return;
            };
            // Drain DOM PointerEvent listeners before processing the
            // winit event. The closures pushed onto
            // `pending_pointer` and called `request_redraw`, which
            // is what brought us here — handle the captured input
            // first so RedrawRequested sees the post-event state.
            if self.drain_pending_pointer(gfx) {
                self.next_trigger = FrameTrigger::Pointer;
            }
            // Drain soft-keyboard edits next — order matters because
            // a pointer event may have shifted focus to a text
            // input, after which keystrokes captured this frame
            // should reach the new target.
            if self.drain_soft_keyboard(gfx) {
                self.next_trigger = FrameTrigger::Keyboard;
            }
            // If focus moved off a text input this frame, dismiss
            // the on-screen keyboard now (done after both drains so
            // the focus state reflects everything that just
            // happened).
            self.sync_soft_keyboard_focus(gfx);
            let scale = gfx.window.scale_factor() as f32;

            match event {
                WindowEvent::CloseRequested => event_loop.exit(),

                WindowEvent::Resized(size) => {
                    gfx.config.width = size.width.max(1);
                    gfx.config.height = size.height.max(1);
                    gfx.surface.configure(&gfx.device, &gfx.config);
                    gfx.renderer
                        .set_surface_size(gfx.config.width, gfx.config.height);
                    if let Some(msaa) = gfx.msaa.as_mut() {
                        let extent = surface_extent(&gfx.config);
                        if !msaa.matches(extent) {
                            *msaa = damascene_wgpu::MsaaTarget::new(
                                &gfx.device,
                                gfx.render_format,
                                extent,
                                SAMPLE_COUNT,
                            );
                        }
                    }
                    self.next_trigger = FrameTrigger::Resize;
                    gfx.window.request_redraw();
                }

                // Pointer input on web flows through DOM PointerEvent
                // listeners installed in `resumed()`. winit's
                // CursorMoved / CursorLeft / MouseInput on the web
                // backend collapse touch and pen to mouse before
                // forwarding, so handling them here would either
                // double-route (the DOM listener already saw them)
                // or strip the modality. They're intentionally
                // ignored — the drain at the top of window_event
                // dispatches everything the closures captured.

                // Browser drag/drop and clipboard-image plumbing rides
                // the HTML File API rather than winit (which doesn't
                // surface DroppedFile on wasm32). Web hosts that need
                // file-drop support listen for `dragenter` / `drop` on
                // the canvas via wasm-bindgen and route the resulting
                // bytes through their own paths. The winit event arms
                // exist for source-parity with the native hosts; on
                // web they currently won't fire.
                WindowEvent::HoveredFile(path) => {
                    let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                    for event in gfx.renderer.file_hovered(path, lx, ly) {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            &gfx.renderer,
                            logical_viewport_of(gfx),
                            self.last_diagnostics.as_ref(),
                            &mut self.primary_selection,
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
                            logical_viewport_of(gfx),
                            self.last_diagnostics.as_ref(),
                            &mut self.primary_selection,
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
                            logical_viewport_of(gfx),
                            self.last_diagnostics.as_ref(),
                            &mut self.primary_selection,
                        );
                    }
                    self.next_trigger = FrameTrigger::Pointer;
                    gfx.window.request_redraw();
                }

                WindowEvent::MouseWheel { delta, .. } => {
                    let Some((lx, ly)) = self.last_pointer else {
                        return;
                    };
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
                                &gfx.renderer,
                                logical_viewport_of(gfx),
                                self.last_diagnostics.as_ref(),
                                &mut self.primary_selection,
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
                    if logical != LogicalKey::Unidentified || physical != PhysicalKey::Unidentified
                    {
                        for event in gfx.renderer.key_down(
                            logical,
                            physical,
                            self.modifiers,
                            key_event.repeat,
                        ) {
                            match text_input::clipboard_request(&event) {
                                Some(ClipboardKind::Copy) => {
                                    copy_current_selection(&gfx.renderer, write_clipboard_text);
                                    dispatch_app_event(
                                        &mut self.app,
                                        event,
                                        &gfx.renderer,
                                        logical_viewport_of(gfx),
                                        self.last_diagnostics.as_ref(),
                                        &mut self.primary_selection,
                                    );
                                }
                                Some(ClipboardKind::Cut) => {
                                    copy_current_selection(&gfx.renderer, write_clipboard_text);
                                    dispatch_app_event(
                                        &mut self.app,
                                        clipboard::delete_selection_event(event),
                                        &gfx.renderer,
                                        logical_viewport_of(gfx),
                                        self.last_diagnostics.as_ref(),
                                        &mut self.primary_selection,
                                    );
                                }
                                Some(ClipboardKind::Paste) => {}
                                None => dispatch_app_event(
                                    &mut self.app,
                                    event,
                                    &gfx.renderer,
                                    logical_viewport_of(gfx),
                                    self.last_diagnostics.as_ref(),
                                    &mut self.primary_selection,
                                ),
                            }
                        }
                    }
                    if let Some(text) = &key_event.text
                        && let Some(event) = gfx.renderer.text_input(text.to_string())
                    {
                        dispatch_app_event(
                            &mut self.app,
                            event,
                            &gfx.renderer,
                            logical_viewport_of(gfx),
                            self.last_diagnostics.as_ref(),
                            &mut self.primary_selection,
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
                            logical_viewport_of(gfx),
                            self.last_diagnostics.as_ref(),
                            &mut self.primary_selection,
                        );
                    }
                    self.next_trigger = FrameTrigger::Keyboard;
                    gfx.window.request_redraw();
                }

                WindowEvent::RedrawRequested => {
                    let frame_start = Instant::now();
                    let event_viewport = logical_viewport_of(gfx);
                    let clipboard_drained = drain_pending_clipboard_text(
                        &mut self.app,
                        &mut gfx.renderer,
                        event_viewport,
                        self.last_diagnostics.as_ref(),
                        &self.pending_clipboard_text,
                        &mut self.primary_selection,
                    );
                    if clipboard_drained {
                        self.next_trigger = FrameTrigger::Keyboard;
                    }
                    let frame = match gfx.surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(frame)
                        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                        wgpu::CurrentSurfaceTexture::Lost
                        | wgpu::CurrentSurfaceTexture::Outdated => {
                            gfx.surface.configure(&gfx.device, &gfx.config);
                            return;
                        }
                        other => {
                            log::error!("surface unavailable: {other:?}");
                            return;
                        }
                    };
                    // Render through the sRGB view format (see
                    // `srgb_view_of` and the surface configuration step
                    // for why). When the swapchain is already sRGB this
                    // collapses to the storage format and the view is
                    // identical to `..Default::default()`.
                    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(gfx.render_format),
                        ..Default::default()
                    });

                    let last_frame_dt = self
                        .last_frame_at
                        .map(|t| frame_start.duration_since(t))
                        .unwrap_or(std::time::Duration::ZERO);
                    self.last_frame_at = Some(frame_start);
                    let trigger = std::mem::take(&mut self.next_trigger);
                    let scale_factor = gfx.window.scale_factor() as f32;
                    let viewport_rect = Rect::new(
                        0.0,
                        0.0,
                        gfx.config.width as f32 / scale_factor,
                        gfx.config.height as f32 / scale_factor,
                    );
                    let current_size = (gfx.config.width, gfx.config.height);
                    // Paint-only path: a time-driven shader's deadline
                    // fired and nothing else has changed since the last
                    // full prepare — skip rebuild + layout and reuse the
                    // cached ops via `repaint`. The size guard catches
                    // ResizeObserver fires that updated `gfx.config`
                    // since the last prepare without setting a trigger.
                    let paint_only = trigger == FrameTrigger::ShaderPaint
                        && Some(current_size) == self.last_prepared_size;

                    let (prepare, palette, t_after_build, t_after_prepare) = if paint_only {
                        // No build pass: reuse the renderer's already-set
                        // theme palette and skip diagnostics / frame_index
                        // bump. Apps reading `cx.diagnostics()` see the
                        // overlay update only on layout frames, which is
                        // the documented contract for paint-only.
                        let palette = gfx.renderer.theme().palette().clone();
                        let t_after_build = Instant::now();
                        let prepare = gfx.renderer.repaint(
                            &gfx.device,
                            &gfx.queue,
                            viewport_rect,
                            scale_factor,
                        );
                        let t_after_prepare = Instant::now();
                        (prepare, palette, t_after_build, t_after_prepare)
                    } else {
                        self.frame_index = self.frame_index.wrapping_add(1);
                        let diagnostics = HostDiagnostics {
                            backend: *self.backend.borrow(),
                            surface_size: (gfx.config.width, gfx.config.height),
                            scale_factor,
                            msaa_samples: SAMPLE_COUNT,
                            frame_index: self.frame_index,
                            last_frame_dt,
                            last_build: self.last_build,
                            last_prepare: self.last_prepare,
                            last_layout: self.last_layout,
                            last_layout_intrinsic_cache_hits: self.last_layout_intrinsic_cache_hits,
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
                            last_text_layout_cache_evictions: self.last_text_layout_cache_evictions,
                            last_text_layout_shaped_bytes: self.last_text_layout_shaped_bytes,
                            trigger,
                            // The web/WebGPU host doesn't negotiate color
                            // management; it composites in the default
                            // sRGB-linear working space and presents to a
                            // browser-managed canvas.
                            working_color_space: damascene_core::paint::DEFAULT_WORKING_COLOR_SPACE,
                            color_management:
                                damascene_core::color::ColorManagementStatus::Unavailable,
                            // No wgpu surface caps plumbed from the WebGPU
                            // host yet; the canvas is browser-managed.
                            surface_color: None,
                        };
                        // Retained for event dispatch: handlers read the
                        // last built frame's snapshot via EventCx.
                        self.last_diagnostics = Some(diagnostics.clone());
                        self.app.before_build();
                        let theme = self.app.theme();
                        let safe_area = damascene_core::Sides {
                            left: 0.0,
                            right: 0.0,
                            top: 0.0,
                            bottom: self.keyboard_inset_bottom.get(),
                        };
                        let cx = BuildCx::new(&theme)
                            .with_ui_state(gfx.renderer.ui_state())
                            .with_diagnostics(&diagnostics)
                            .with_viewport(viewport_rect.w, viewport_rect.h)
                            .with_safe_area(safe_area);
                        let tree = self.app.build(&cx);
                        let palette = theme.palette().clone();
                        gfx.renderer.set_theme(theme);
                        gfx.renderer.set_hotkeys(self.app.hotkeys());
                        gfx.renderer.set_selection(self.app.selection());
                        gfx.renderer.push_toasts(self.app.drain_toasts());
                        gfx.renderer
                            .push_focus_requests(self.app.drain_focus_requests());
                        gfx.renderer
                            .push_scroll_requests(self.app.drain_scroll_requests());
                        gfx.renderer
                            .push_viewport_requests(self.app.drain_viewport_requests());
                        gfx.renderer
                            .push_plot_requests(self.app.drain_plot_requests());
                        for url in self.app.drain_link_opens() {
                            open_link(&url);
                        }
                        let t_after_build = Instant::now();
                        let prepare = gfx.renderer.prepare(
                            &gfx.device,
                            &gfx.queue,
                            tree,
                            viewport_rect,
                            scale_factor,
                        );
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
                        self.last_prepared_size = Some(current_size);
                        (prepare, palette, t_after_build, t_after_prepare)
                    };

                    let mut encoder =
                        gfx.device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("damascene_web::encoder"),
                            });
                    // `render()` owns pass lifetimes itself so it can
                    // split around `BackdropSnapshot` boundaries when
                    // the App uses backdrop-sampling shaders. With no
                    // boundary it collapses to a single Clear pass —
                    // same behaviour as the old `begin_render_pass +
                    // draw + end_render_pass` path.
                    gfx.renderer.render(
                        &gfx.device,
                        &mut encoder,
                        &frame.texture,
                        &view,
                        gfx.msaa.as_ref().map(|m| &m.view),
                        wgpu::LoadOp::Clear(bg_color(&palette)),
                    );
                    gfx.queue.submit(Some(encoder.finish()));
                    gfx.queue.present(frame);
                    let t_after_submit = Instant::now();

                    self.stats.record(
                        frame_start,
                        t_after_build,
                        t_after_prepare,
                        t_after_submit,
                        prepare.timings,
                    );
                    self.last_build = t_after_build - frame_start;
                    self.last_prepare = t_after_prepare - t_after_build;
                    self.last_submit = t_after_submit - t_after_prepare;
                    self.last_layout = prepare.timings.layout;
                    self.last_layout_intrinsic_cache_hits =
                        prepare.timings.layout_intrinsic_cache.hits;
                    self.last_layout_intrinsic_cache_misses =
                        prepare.timings.layout_intrinsic_cache.misses;
                    self.last_layout_pruned_subtrees = prepare.timings.layout_prune.subtrees;
                    self.last_layout_pruned_nodes = prepare.timings.layout_prune.nodes;
                    self.last_draw_ops = prepare.timings.draw_ops;
                    self.last_draw_ops_culled_text_ops = prepare.timings.draw_ops_culled_text_ops;
                    self.last_paint = prepare.timings.paint;
                    self.last_paint_culled_ops = prepare.timings.paint_culled_ops;
                    self.last_gpu_upload = prepare.timings.gpu_upload;
                    self.last_snapshot = prepare.timings.snapshot;
                    self.last_text_layout_cache_hits = prepare.timings.text_layout_cache.hits;
                    self.last_text_layout_cache_misses = prepare.timings.text_layout_cache.misses;
                    self.last_text_layout_cache_evictions =
                        prepare.timings.text_layout_cache.evictions;
                    self.last_text_layout_shaped_bytes =
                        prepare.timings.text_layout_cache.shaped_bytes;

                    // Two-lane scheduling: a layout-driven signal
                    // (animation settling, widget redraw_within,
                    // tooltip / toast pending) takes precedence over a
                    // paint-only signal — both arrive immediately
                    // because the browser raf loop has no deadline
                    // parking, but the trigger encodes which path the
                    // next frame should take. On a paint-only frame
                    // `repaint` reports `next_layout_redraw_in = None`
                    // (it didn't re-evaluate), so the layout deadline
                    // can only fall through if the prior full prepare
                    // already cleared it.
                    if prepare.next_layout_redraw_in.is_some() {
                        self.next_trigger = FrameTrigger::Animation;
                        gfx.window.request_redraw();
                    } else if prepare.next_paint_redraw_in.is_some() {
                        self.next_trigger = FrameTrigger::ShaderPaint;
                        gfx.window.request_redraw();
                    }
                }
                _ => {}
            }
        }

        /// Fires once when the loop stops — on web that is only ever
        /// [`WebHandle::destroy`] (browsers have no `CloseRequested`).
        /// Runs the DOM teardown while the page references are still
        /// alive; winit drops this Host (and with it the JS closures)
        /// immediately after, then removes its own canvas listeners.
        fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
            self.teardown();
        }
    }

    /// Map a DOM `KeyboardEvent.code` string to a damascene
    /// [`PhysicalKey`]. Covers the editing / navigation keys the
    /// soft-keyboard keydown path forwards; anything else is
    /// [`PhysicalKey::Unidentified`].
    fn dom_physical(code: &str) -> PhysicalKey {
        match code {
            "Backspace" => PhysicalKey::Backspace,
            "Delete" => PhysicalKey::Delete,
            "Enter" => PhysicalKey::Enter,
            "NumpadEnter" => PhysicalKey::NumpadEnter,
            "ArrowUp" => PhysicalKey::ArrowUp,
            "ArrowDown" => PhysicalKey::ArrowDown,
            "ArrowLeft" => PhysicalKey::ArrowLeft,
            "ArrowRight" => PhysicalKey::ArrowRight,
            "Home" => PhysicalKey::Home,
            "End" => PhysicalKey::End,
            _ => PhysicalKey::Unidentified,
        }
    }

    fn should_prevent_browser_key_default(event: &web_sys::KeyboardEvent) -> bool {
        // Keep browser/system shortcuts alive, especially Ctrl/Cmd+V:
        // preventing that keydown suppresses the trusted DOM `paste`
        // event that carries clipboard text in Firefox.
        if event.ctrl_key() || event.meta_key() || event.alt_key() {
            return false;
        }

        let key = event.key();
        if key.chars().count() == 1 {
            return true;
        }

        matches!(
            key.as_str(),
            "ArrowUp"
                | "ArrowDown"
                | "ArrowLeft"
                | "ArrowRight"
                | "Backspace"
                | "Delete"
                | "Home"
                | "End"
                | "PageUp"
                | "PageDown"
                | "Tab"
                | "Enter"
                | "Escape"
        )
    }

    /// Clear color for the canvas: the background token converted into the
    /// working space, exactly like every painted fill. The web host doesn't
    /// negotiate color management — it composites in the default sRGB-linear
    /// working space (the host env pins `DEFAULT_WORKING_COLOR_SPACE`), so
    /// [`damascene_core::paint::rgba_f32`] is the matching conversion
    /// (issue #45).
    fn bg_color(palette: &Palette) -> wgpu::Color {
        let [r, g, b, a] = damascene_core::paint::rgba_f32(palette.background);
        wgpu::Color {
            r: r as f64,
            g: g as f64,
            b: b as f64,
            a: a as f64,
        }
    }

    fn copy_current_selection(renderer: &Runner, write_text: impl FnOnce(String)) {
        // Read the selection out of `last_tree` (via the runtime
        // helper) — see `RunnerCore::selected_text` for why a
        // build-only path would miss selections inside a virtual
        // list.
        let Some(text) = renderer.selected_text() else {
            return;
        };
        write_text(text);
    }

    fn write_clipboard_text(text: String) {
        let Some(window) = web_sys::window() else {
            log::warn!("damascene-web: no window; clipboard write dropped");
            return;
        };
        let promise = window.navigator().clipboard().write_text(&text);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = wasm_bindgen_futures::JsFuture::from(promise).await {
                log::warn!("damascene-web: clipboard writeText failed: {err:?}");
            }
        });
    }

    fn attach_primary_selection_text(mut event: UiEvent, primary_selection: &str) -> UiEvent {
        if event.kind == UiEventKind::MiddleClick && !primary_selection.is_empty() {
            event.text = Some(primary_selection.to_string());
        }
        event
    }

    fn event_cx<'a>(
        renderer: &'a Runner,
        viewport: Option<(f32, f32)>,
        diagnostics: Option<&'a damascene_core::HostDiagnostics>,
    ) -> damascene_core::EventCx<'a> {
        let mut cx = damascene_core::EventCx::new().with_ui_state(renderer.ui_state());
        if let Some((w, h)) = viewport {
            cx = cx.with_viewport(w, h);
        }
        if let Some(d) = diagnostics {
            cx = cx.with_diagnostics(d);
        }
        cx
    }

    fn dispatch_app_event<A: App>(
        app: &mut A,
        event: UiEvent,
        renderer: &Runner,
        viewport: Option<(f32, f32)>,
        diagnostics: Option<&damascene_core::HostDiagnostics>,
        primary_selection: &mut String,
    ) {
        let before = app.selection();
        let cx = event_cx(renderer, viewport, diagnostics);
        app.on_event(event, &cx);
        if app.selection() != before {
            // Resolve the post-event selection against `last_tree`.
            // The new selection's keys are typically the row the user
            // just clicked, which is present in the previous frame's
            // snapshot.
            *primary_selection = renderer
                .selected_text_for(&app.selection())
                .filter(|text| !text.is_empty())
                .unwrap_or_default();
        }
    }

    fn dispatch_app_wheel_event<A: App>(
        app: &mut A,
        event: UiEvent,
        renderer: &Runner,
        viewport: Option<(f32, f32)>,
        diagnostics: Option<&damascene_core::HostDiagnostics>,
        primary_selection: &mut String,
    ) -> bool {
        let before = app.selection();
        let cx = event_cx(renderer, viewport, diagnostics);
        let consumed = app.on_wheel_event(event, &cx);
        if app.selection() != before {
            *primary_selection = renderer
                .selected_text_for(&app.selection())
                .filter(|text| !text.is_empty())
                .unwrap_or_default();
        }
        consumed
    }

    fn drain_pending_clipboard_text<A: App>(
        app: &mut A,
        renderer: &mut Runner,
        viewport: Option<(f32, f32)>,
        diagnostics: Option<&damascene_core::HostDiagnostics>,
        pending_text: &Rc<RefCell<VecDeque<String>>>,
        primary_selection: &mut String,
    ) -> bool {
        let mut drained = false;
        while let Some(text) = pending_text.borrow_mut().pop_front() {
            let Some(event) = renderer.text_input(text.clone()) else {
                continue;
            };
            drained = true;
            let event = clipboard::paste_text_event(event, text);
            dispatch_app_event(
                app,
                event,
                renderer,
                viewport,
                diagnostics,
                primary_selection,
            );
        }
        drained
    }
}

#[cfg(test)]
mod mailbox_tests {
    use super::Mailbox;

    #[test]
    fn mailbox_queues_and_drains_in_order() {
        let mailbox = Mailbox::new();
        assert!(mailbox.is_empty());
        // Pushes before set_handle queue silently (no host to wake).
        mailbox.push(1);
        let clone = mailbox.clone();
        clone.push(2);
        mailbox.push(3);
        assert!(!mailbox.is_empty());
        assert_eq!(mailbox.drain(), vec![1, 2, 3]);
        assert!(mailbox.is_empty());
        assert_eq!(mailbox.drain(), Vec::<i32>::new());
    }

    #[test]
    fn mailbox_set_handle_accepts_stub() {
        let mailbox = Mailbox::new();
        mailbox.set_handle(super::WebHandle::default());
        mailbox.push("after-handle");
        assert_eq!(mailbox.drain(), vec!["after-handle"]);
    }
}
