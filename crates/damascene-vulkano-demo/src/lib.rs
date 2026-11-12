//! damascene-vulkano-demo — winit + vulkano harness and demo binaries for
//! the `damascene-vulkano` backend.
//!
//! The host owns the event loop, the Vulkan instance/device/queue, the
//! swapchain, the framebuffers, and the per-frame command buffer. The
//! library — `damascene-core` + `damascene-vulkano` — owns layout, paint,
//! hit-test, and visual state. The user owns the [`App`] impl.
//!
//! [`run`] mirrors the simple native host contract method-for-method so
//! an [`App`] written for the wgpu demo path runs unchanged here.
//!
//! Copy, cut and paste go through the system clipboard, as on the
//! `damascene-winit-wgpu` host: the app has no clipboard code. Not
//! mirrored: the IME is never enabled (no `set_ime_allowed`), and
//! there is no primary-selection paste on middle click.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use damascene_core::clipboard::{delete_selection_event, paste_text_event};
use damascene_core::widgets::text_input::ClipboardKind;
use damascene_core::{App, BuildCx, EventCx, KeyModifiers, Pointer, Rect, UiEvent};
use damascene_core::{LogicalKey, PhysicalKey as DPhysicalKey};
use damascene_vulkano::Runner;
use damascene_winit::{key_modifiers, map_key, map_physical, pointer_button};
use vulkano::{
    VulkanLibrary,
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, allocator::StandardCommandBufferAllocator,
    },
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
        physical::PhysicalDeviceType,
    },
    format::{Format, NumericFormat},
    image::{Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    memory::allocator::{AllocationCreateInfo, StandardMemoryAllocator},
    render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass},
    swapchain::{
        Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo, acquire_next_image,
    },
    sync::{self, GpuFuture},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

/// Run a windowed app on the vulkano backend. Blocks until the user
/// closes the window. Mirrors `damascene-winit-wgpu`'s simple run
/// contract.
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_init(title, viewport, app, |_| {})
}

/// Like [`run`], but invokes `init_runner` on the freshly-built
/// [`Runner`] before the first frame. Use this to call
/// [`Runner::register_shader`] for any custom shaders the App's tree
/// references — same shape as the wgpu `render_custom` example in
/// `damascene-wgpu`.
pub fn run_with_init<A: App + 'static, F: FnOnce(&mut Runner) + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    init_runner: F,
) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let library = VulkanLibrary::new()?;
    let required_extensions = Surface::required_extensions(&event_loop)?;
    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            enabled_extensions: required_extensions,
            ..Default::default()
        },
    )?;

    let mut host = Host {
        title,
        viewport,
        app,
        instance,
        modifiers: KeyModifiers::default(),
        last_pointer: None,
        clipboard: arboard::Clipboard::new().ok(),
        rcx: None,
        init_runner: Some(Box::new(init_runner)),
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

/// One-shot Runner initialiser, consumed inside `resumed()` once the
/// runner exists. Boxed so `Host` stays object-safe and the App's
/// generic parameter doesn't leak into the closure type.
type InitRunner = Box<dyn FnOnce(&mut Runner)>;

/// Wait this long after the last `Resized` before recreating the
/// swapchain. Wayland compositors fire `Resized` every frame during
/// animated geometry changes (KDE tile/snap, etc.); recreating on each
/// one stalls the surface.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(100);

/// MSAA samples per pixel for the host's color attachment. Matches
/// `damascene-winit-wgpu`'s default. Set to `1` to disable MSAA — the host
/// then skips the multisampled image and binds only the swapchain image
/// per framebuffer.
const DEFAULT_SAMPLE_COUNT: u32 = 4;

struct Host<A: App> {
    title: &'static str,
    viewport: Rect,
    app: A,
    instance: Arc<Instance>,
    modifiers: KeyModifiers,
    last_pointer: Option<(f32, f32)>,
    /// Best-effort system clipboard: `None` when the session has none
    /// to offer (no display server, a failed connection), which leaves
    /// copy, cut and paste as no-ops.
    clipboard: Option<arboard::Clipboard>,
    rcx: Option<RenderContext>,
    init_runner: Option<InitRunner>,
}

/// The system clipboard as this host uses it — plain text out and in.
/// A trait so [`dispatch_keyboard_event`] can be driven against a fake
/// in tests instead of the desktop's real clipboard.
trait ClipboardText {
    fn set_text(&mut self, text: String);
    fn text(&mut self) -> Option<String>;
}

impl ClipboardText for Option<arboard::Clipboard> {
    fn set_text(&mut self, text: String) {
        if let Some(clipboard) = self {
            let _ = clipboard.set_text(text);
        }
    }

    fn text(&mut self) -> Option<String> {
        self.as_mut()?.get_text().ok()
    }
}

/// Deliver a key event to the app, carrying out the clipboard
/// operation it asks for first. The app has no clipboard code: a copy
/// puts the runner's selected text on the clipboard, a cut does that
/// and reaches the app as a delete, and a paste reaches it as a text
/// insertion. The runner decides whether the event is a clipboard
/// request at all (none from a password field) — the same contract
/// as the `damascene-winit-wgpu` host.
fn dispatch_keyboard_event<A: App>(
    app: &mut A,
    event: UiEvent,
    runner: &Runner,
    system: &mut impl ClipboardText,
) {
    let event = match runner.clipboard_request(&event) {
        Some(ClipboardKind::Copy) => {
            copy_selection(runner, system);
            event
        }
        Some(ClipboardKind::Cut) => {
            copy_selection(runner, system);
            delete_selection_event(event)
        }
        Some(ClipboardKind::Paste) => match system.text() {
            Some(text) => paste_text_event(event, text),
            None => event,
        },
        None => event,
    };
    app.on_event(event, &event_cx(runner));
}

fn copy_selection(runner: &Runner, system: &mut impl ClipboardText) {
    if let Some(text) = runner.selected_text() {
        system.set_text(text);
    }
}

/// Per-dispatch [`EventCx`] for [`App::on_event`] / `on_wheel_event` —
/// geometry queries answer from the runner's last laid-out frame.
fn event_cx(runner: &Runner) -> EventCx<'_> {
    EventCx::new().with_ui_state(runner.ui_state())
}

struct RenderContext {
    window: Arc<Window>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    swapchain: Arc<Swapchain>,
    framebuffers: Vec<Arc<Framebuffer>>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    memory_alloc: Arc<StandardMemoryAllocator>,
    /// Multisampled color attachment shared by every framebuffer when
    /// `runner.sample_count() > 1`. Reused across frames — submissions
    /// serialize on `previous_frame_end` so the GPU only touches it from
    /// one frame at a time. `None` when MSAA is disabled.
    msaa_image: Option<Arc<ImageView>>,
    runner: Runner,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
    recreate_swapchain: bool,
    /// Timestamp of the most recent `Resized`, cleared when the
    /// debounce expires and the recreate is promoted.
    resize_debounce: Option<Instant>,
}

impl<A: App> ApplicationHandler for Host<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.rcx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(PhysicalSize::new(
                self.viewport.w as u32,
                self.viewport.h as u32,
            ));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        let surface =
            Surface::from_window(self.instance.clone(), window.clone()).expect("create surface");

        let device_extensions = DeviceExtensions {
            khr_swapchain: true,
            ..DeviceExtensions::empty()
        };

        let (physical_device, queue_family_index) = self
            .instance
            .enumerate_physical_devices()
            .expect("enumerate physical devices")
            .filter(|p| p.supported_extensions().contains(&device_extensions))
            .filter_map(|p| {
                p.queue_family_properties()
                    .iter()
                    .enumerate()
                    .position(|(i, q)| {
                        q.queue_flags.intersects(QueueFlags::GRAPHICS)
                            && p.surface_support(i as u32, &surface).unwrap_or(false)
                    })
                    .map(|i| (p, i as u32))
            })
            .min_by_key(|(p, _)| match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                _ => 4,
            })
            .expect("no compatible Vulkan physical device");

        let (device, mut queues) = Device::new(
            physical_device.clone(),
            DeviceCreateInfo {
                enabled_extensions: device_extensions,
                enabled_features: damascene_vulkano::required_device_features(),
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .expect("create device");
        let queue = queues.next().unwrap();

        let (swapchain, images, image_format) = create_swapchain(&device, surface, &window);

        let mut runner = Runner::with_sample_count(
            device.clone(),
            queue.clone(),
            image_format,
            DEFAULT_SAMPLE_COUNT,
        );
        runner.set_theme(self.app.theme());
        let extent: [u32; 2] = window.inner_size().into();
        runner.set_surface_size(extent[0], extent[1]);
        // Register every shader the app declared, including backdrop-
        // sampling ones — `Runner::render` owns the multi-pass /
        // snapshot dance internally.
        for s in self.app.shaders() {
            runner.register_shader_with(s.name, s.wgsl, s.samples_backdrop, s.samples_time);
        }
        if let Some(init) = self.init_runner.take() {
            init(&mut runner);
        }

        let memory_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let msaa_image = build_msaa_image(
            &memory_alloc,
            image_format,
            swapchain_extent(&swapchain),
            runner.sample_count(),
        );
        let framebuffers = build_framebuffers(&images, runner.render_pass(), msaa_image.as_ref());

        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            Default::default(),
        ));

        let previous_frame_end = Some(sync::now(device.clone()).boxed());

        self.rcx = Some(RenderContext {
            window,
            device,
            queue,
            swapchain,
            framebuffers,
            cmd_alloc,
            memory_alloc,
            msaa_image,
            runner,
            previous_frame_end,
            recreate_swapchain: false,
            resize_debounce: None,
        });
        self.rcx.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(rcx) = self.rcx.as_mut() else {
            return;
        };
        let scale = rcx.window.scale_factor() as f32;

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(_) => {
                rcx.resize_debounce = Some(Instant::now());
                rcx.window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let lx = position.x as f32 / scale;
                let ly = position.y as f32 / scale;
                self.last_pointer = Some((lx, ly));
                let moved = rcx.runner.pointer_moved(Pointer::moving(lx, ly));
                for event in moved.events {
                    self.app.on_event(event, &event_cx(&rcx.runner));
                }
                if moved.needs_redraw {
                    rcx.window.request_redraw();
                }
            }

            WindowEvent::CursorLeft { .. } => {
                self.last_pointer = None;
                for event in rcx.runner.pointer_left() {
                    self.app.on_event(event, &event_cx(&rcx.runner));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::HoveredFile(path) => {
                let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                for event in rcx.runner.file_hovered(path, lx, ly) {
                    self.app.on_event(event, &event_cx(&rcx.runner));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::HoveredFileCancelled => {
                for event in rcx.runner.file_hover_cancelled() {
                    self.app.on_event(event, &event_cx(&rcx.runner));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::DroppedFile(path) => {
                let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                for event in rcx.runner.file_dropped(path, lx, ly) {
                    self.app.on_event(event, &event_cx(&rcx.runner));
                }
                rcx.window.request_redraw();
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
                        for event in rcx.runner.pointer_down(Pointer::mouse(lx, ly, button)) {
                            self.app.on_event(event, &event_cx(&rcx.runner));
                        }
                        rcx.window.request_redraw();
                    }
                    ElementState::Released => {
                        for event in rcx.runner.pointer_up(Pointer::mouse(lx, ly, button)) {
                            self.app.on_event(event, &event_cx(&rcx.runner));
                        }
                        rcx.window.request_redraw();
                    }
                }
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
                let consumed = if let Some(event) = rcx.runner.pointer_wheel_event(lx, ly, dx, dy) {
                    needs_redraw = true;
                    self.app.on_wheel_event(event, &event_cx(&rcx.runner))
                } else {
                    false
                };
                if !consumed && rcx.runner.pointer_wheel(lx, ly, dx, dy) {
                    needs_redraw = true;
                }
                if needs_redraw {
                    rcx.window.request_redraw();
                }
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = key_modifiers(modifiers.state());
                rcx.runner.set_modifiers(self.modifiers);
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
                if logical != LogicalKey::Unidentified || physical != DPhysicalKey::Unidentified {
                    for ev in
                        rcx.runner
                            .key_down(logical, physical, self.modifiers, key_event.repeat)
                    {
                        dispatch_keyboard_event(
                            &mut self.app,
                            ev,
                            &rcx.runner,
                            &mut self.clipboard,
                        );
                    }
                }
                if let Some(text) = &key_event.text
                    && let Some(ev) = rcx.runner.text_input(text.to_string())
                {
                    self.app.on_event(ev, &event_cx(&rcx.runner));
                }
                rcx.window.request_redraw();
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if let Some(ev) = rcx.runner.text_input(text) {
                    self.app.on_event(ev, &event_cx(&rcx.runner));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                // Drain time-driven input events (touch long-press
                // today) before this frame's build.
                for event in rcx.runner.poll_input(std::time::Instant::now()) {
                    self.app.on_event(event, &event_cx(&rcx.runner));
                }
                // Promote the pending debounce to a real recreate once
                // it expires; otherwise render at the existing
                // swapchain size and let the compositor scale.
                if let Some(last_resize) = rcx.resize_debounce
                    && last_resize.elapsed() >= RESIZE_DEBOUNCE
                {
                    rcx.recreate_swapchain = true;
                    rcx.resize_debounce = None;
                }

                let extent: [u32; 2] = if rcx.resize_debounce.is_some() {
                    rcx.swapchain.create_info().image_extent
                } else {
                    rcx.window.inner_size().into()
                };
                if extent[0] == 0 || extent[1] == 0 {
                    return;
                }

                rcx.previous_frame_end.as_mut().unwrap().cleanup_finished();

                if rcx.recreate_swapchain {
                    let (new_swapchain, new_images) = rcx
                        .swapchain
                        .recreate(SwapchainCreateInfo {
                            image_extent: extent,
                            ..rcx.swapchain.create_info()
                        })
                        .expect("recreate swapchain");
                    rcx.swapchain = new_swapchain;
                    rcx.msaa_image = build_msaa_image(
                        &rcx.memory_alloc,
                        rcx.swapchain.image_format(),
                        swapchain_extent(&rcx.swapchain),
                        rcx.runner.sample_count(),
                    );
                    rcx.framebuffers = build_framebuffers(
                        &new_images,
                        rcx.runner.render_pass(),
                        rcx.msaa_image.as_ref(),
                    );
                    rcx.runner.set_surface_size(extent[0], extent[1]);
                    rcx.recreate_swapchain = false;
                }

                self.app.before_build();
                let scale_factor = rcx.window.scale_factor() as f32;
                let viewport = Rect::new(
                    0.0,
                    0.0,
                    extent[0] as f32 / scale_factor,
                    extent[1] as f32 / scale_factor,
                );
                let theme = self.app.theme();
                let cx = BuildCx::new(&theme)
                    .with_ui_state(rcx.runner.ui_state())
                    .with_viewport(viewport.w, viewport.h);
                let tree = self.app.build(&cx);
                let palette = theme.palette().clone();
                rcx.runner.set_theme(theme);
                rcx.runner.set_hotkeys(self.app.hotkeys());
                rcx.runner.set_selection(self.app.selection());
                rcx.runner.push_toasts(self.app.drain_toasts());
                rcx.runner
                    .push_announcements(self.app.drain_announcements());
                rcx.runner
                    .push_focus_requests(self.app.drain_focus_requests());
                rcx.runner
                    .push_scroll_requests(self.app.drain_scroll_requests());
                rcx.runner
                    .push_viewport_requests(self.app.drain_viewport_requests());
                rcx.runner
                    .push_plot_requests(self.app.drain_plot_requests());
                let prepare = rcx.runner.prepare(tree, viewport, scale_factor);

                let (image_index, suboptimal, acquire_future) =
                    match acquire_next_image(rcx.swapchain.clone(), None) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("acquire_next_image: {e}");
                            rcx.recreate_swapchain = true;
                            return;
                        }
                    };
                // `suboptimal` fires every frame during animated
                // resizes; gating on the debounce keeps the deferral
                // intact (real staleness still surfaces via acquire
                // errors above).
                if suboptimal && rcx.resize_debounce.is_none() {
                    rcx.recreate_swapchain = true;
                }

                let mut builder = AutoCommandBufferBuilder::primary(
                    rcx.cmd_alloc.clone(),
                    rcx.queue.queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .expect("command builder");

                // `render()` owns pass lifetimes itself so it can split
                // around `BackdropSnapshot` boundaries when the app
                // uses backdrop-sampling shaders. With no boundary it
                // collapses to a single Clear pass — same behaviour as
                // the old `begin_render_pass + draw + end_render_pass`
                // path.
                let framebuffer = rcx.framebuffers[image_index as usize].clone();
                // With MSAA the last attachment is the single-sample
                // resolve target (the swapchain image); without MSAA
                // there's only one attachment. Either way the *last*
                // one is what `Runner::render` should treat as the
                // post-resolve target it can copy into the snapshot.
                let attachments = framebuffer.attachments();
                let target_image = attachments[attachments.len() - 1].image().clone();
                rcx.runner.render(
                    &mut builder,
                    framebuffer,
                    target_image,
                    clear_color(&palette),
                );
                let command_buffer = builder.build().expect("build cmd");

                let future = rcx
                    .previous_frame_end
                    .take()
                    .unwrap()
                    .join(acquire_future)
                    .then_execute(rcx.queue.clone(), command_buffer)
                    .expect("submit")
                    .then_swapchain_present(
                        rcx.queue.clone(),
                        SwapchainPresentInfo::swapchain_image_index(
                            rcx.swapchain.clone(),
                            image_index,
                        ),
                    )
                    .then_signal_fence_and_flush();

                match future.map_err(|e| e.unwrap()) {
                    Ok(fence) => {
                        // Wait for the GPU to finish this frame before
                        // returning. The runner uploads via per-frame
                        // `SubbufferAllocator` suballocations, so a host
                        // write in the next `prepare()` would not hit
                        // `AccessConflict(DeviceRead)` even with a frame
                        // in flight; the demo keeps the wait for
                        // simplicity (one frame at a time, no double
                        // buffering of swapchain-side state). Apps that
                        // want to overlap frames can drop this fence
                        // wait — the upload path is already safe.
                        fence.wait(None).expect("frame fence wait");
                        rcx.previous_frame_end = Some(sync::now(rcx.device.clone()).boxed());
                    }
                    Err(e) => {
                        eprintln!("flush: {e}");
                        rcx.recreate_swapchain = true;
                        rcx.previous_frame_end = Some(sync::now(rcx.device.clone()).boxed());
                    }
                }

                if prepare.needs_redraw {
                    rcx.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Wake at the debounce deadline so the deferred recreate fires
        // even with no further input.
        let Some(rcx) = self.rcx.as_ref() else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            return;
        };
        if let Some(last_resize) = rcx.resize_debounce {
            let deadline = last_resize + RESIZE_DEBOUNCE;
            if Instant::now() >= deadline {
                rcx.window.request_redraw();
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            } else {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(deadline));
            }
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }
    }
}

fn create_swapchain(
    device: &Arc<Device>,
    surface: Arc<Surface>,
    window: &Window,
) -> (Arc<Swapchain>, Vec<Arc<Image>>, Format) {
    let surface_caps = device
        .physical_device()
        .surface_capabilities(&surface, Default::default())
        .expect("surface caps");
    let formats = device
        .physical_device()
        .surface_formats(&surface, Default::default())
        .expect("surface formats");
    let image_format = formats
        .iter()
        .copied()
        .find(|(f, _)| f.numeric_format_color() == Some(NumericFormat::SRGB))
        .unwrap_or(formats[0])
        .0;
    let (swapchain, images) = Swapchain::new(
        device.clone(),
        surface,
        SwapchainCreateInfo {
            min_image_count: surface_caps.min_image_count.max(2),
            image_format,
            image_extent: window.inner_size().into(),
            // TRANSFER_SRC is required so `Runner::render` can copy the
            // post-Pass-A surface into the runner's snapshot image
            // mid-frame for backdrop-sampling shaders. TRANSFER_DST is
            // needed under MSAA: vulkano's `single_pass_renderpass!`
            // macro picks `ImageLayout::TransferDstOptimal` for the
            // resolve attachment, and that layout requires the flag on
            // the underlying image. Cost is minimal — most swapchain
            // surfaces already advertise both.
            image_usage: ImageUsage::COLOR_ATTACHMENT
                | ImageUsage::TRANSFER_SRC
                | ImageUsage::TRANSFER_DST,
            composite_alpha: surface_caps
                .supported_composite_alpha
                .into_iter()
                .next()
                .unwrap(),
            ..Default::default()
        },
    )
    .expect("create swapchain");
    (swapchain, images, image_format)
}

fn build_framebuffers(
    images: &[Arc<Image>],
    render_pass: &Arc<RenderPass>,
    msaa_image: Option<&Arc<ImageView>>,
) -> Vec<Arc<Framebuffer>> {
    images
        .iter()
        .map(|image| {
            let view = ImageView::new_default(image.clone()).expect("image view");
            // Render-pass attachment order from `Runner::render_pass()`:
            // - single-sample: `[color]` — just the swapchain view.
            // - MSAA: `[color_msaa, color_resolve]` — multisampled view
            //   first, swapchain view second (the resolve target).
            let attachments: Vec<Arc<ImageView>> = match msaa_image {
                Some(msaa) => vec![msaa.clone(), view],
                None => vec![view],
            };
            Framebuffer::new(
                render_pass.clone(),
                FramebufferCreateInfo {
                    attachments,
                    ..Default::default()
                },
            )
            .expect("framebuffer")
        })
        .collect()
}

/// Allocate a multisampled color image sized to the swapchain and wrap
/// it in a default view. Returns `None` when `sample_count == 1` —
/// callers then build single-attachment framebuffers without MSAA.
fn build_msaa_image(
    memory_alloc: &Arc<StandardMemoryAllocator>,
    format: Format,
    extent: [u32; 3],
    sample_count: u32,
) -> Option<Arc<ImageView>> {
    if sample_count <= 1 {
        return None;
    }
    let samples = SampleCount::try_from(sample_count).expect("supported MSAA sample count");
    let image = Image::new(
        memory_alloc.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format,
            extent,
            samples,
            // Not `TRANSIENT_ATTACHMENT`: with a backdrop-snapshot
            // boundary the runner needs the MSAA contents to persist
            // between Pass A's end (`store_op: Store`) and Pass B's
            // start (`load_op: Load`), which lazily-allocated transient
            // attachments cannot guarantee.
            usage: ImageUsage::COLOR_ATTACHMENT,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .expect("damascene-vulkano-demo: MSAA color image");
    Some(ImageView::new_default(image).expect("damascene-vulkano-demo: MSAA image view"))
}

fn swapchain_extent(swapchain: &Arc<Swapchain>) -> [u32; 3] {
    let [w, h] = swapchain.image_extent();
    [w, h, 1]
}

/// Clear color for the swapchain: the background token converted into the
/// working space, exactly like every painted fill. This demo composites in
/// the default sRGB-linear working space, so
/// [`damascene_core::paint::rgba_f32`] is the matching conversion
/// (issue #45).
fn clear_color(palette: &damascene_core::Palette) -> [f32; 4] {
    damascene_core::paint::rgba_f32(palette.background)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;

    use damascene_core::widgets::text_input::{self, TextInputOpts, text_input_with};
    use damascene_core::{App, BuildCx, El, EventCx, PointerButton};
    use damascene_vulkano::Runner;
    use vulkano::{
        VulkanLibrary,
        device::{Device, DeviceCreateInfo, Queue, QueueCreateInfo, QueueFlags},
        format::Format,
        instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    };

    use super::{ClipboardText, dispatch_keyboard_event, event_cx};

    use damascene_core::{
        AccessibilityPreferences, AnimationMode, IconMaterial, KeyChord, KeyModifiers, LogicalKey,
        PhysicalKey as DPhysicalKey, Pointer, Rect, Selection, Sides, Theme, UiEvent, UiState,
        announce::Announcement, color::ColorSpace, cursor::Cursor, plot::PlotRequest,
        runtime::PointerMove, scroll::ScrollRequest, toast::ToastSpec, viewport::ViewportRequest,
        widgets::text_input::ClipboardKind,
    };

    macro_rules! assert_common_runner_surface {
        ($runner:ty) => {{
            let _: fn(&mut $runner, u32, u32) = <$runner>::set_surface_size;
            let _: fn(&mut $runner, Theme) = <$runner>::set_theme;
            let _: for<'a> fn(&'a $runner) -> &'a Theme = <$runner>::theme;
            let _: fn(&mut $runner, IconMaterial) = <$runner>::set_icon_material;
            let _: fn(&$runner) -> IconMaterial = <$runner>::icon_material;
            let _: for<'a> fn(&'a $runner) -> &'a UiState = <$runner>::ui_state;
            let _: fn(&$runner) -> String = <$runner>::debug_summary;
            let _: fn(&$runner, &str) -> Option<Rect> = <$runner>::rect_of_key;
            let _: fn(&mut $runner, Pointer) -> PointerMove = <$runner>::pointer_moved;
            let _: fn(&mut $runner) -> Vec<UiEvent> = <$runner>::pointer_left;
            let _: fn(&mut $runner) -> Vec<UiEvent> = <$runner>::pointer_cancelled;
            let _: fn(&mut $runner, PathBuf, f32, f32) -> Vec<UiEvent> = <$runner>::file_hovered;
            let _: fn(&mut $runner) -> Vec<UiEvent> = <$runner>::file_hover_cancelled;
            let _: fn(&mut $runner, PathBuf, f32, f32) -> Vec<UiEvent> = <$runner>::file_dropped;
            let _: fn(&$runner, f32, f32) -> bool = <$runner>::would_press_focus_text_input;
            let _: fn(&$runner) -> bool = <$runner>::focused_captures_keys;
            let _: fn(&mut $runner, Pointer) -> Vec<UiEvent> = <$runner>::pointer_down;
            let _: fn(&mut $runner, Pointer) -> Vec<UiEvent> = <$runner>::pointer_up;
            let _: fn(&mut $runner, KeyModifiers) = <$runner>::set_modifiers;
            let _: fn(&mut $runner, LogicalKey, DPhysicalKey, KeyModifiers, bool) -> Vec<UiEvent> =
                <$runner>::key_down;
            let _: fn(&mut $runner, String) -> Option<UiEvent> = <$runner>::text_input;
            let _: fn(&mut $runner, Vec<(KeyChord, String)>) = <$runner>::set_hotkeys;
            let _: fn(&mut $runner, Sides) = <$runner>::set_safe_area;
            let _: fn(&mut $runner, f32) = <$runner>::set_keyboard_inset;
            let _: fn(&mut $runner, Selection) = <$runner>::set_selection;
            let _: fn(&$runner) -> Option<String> = <$runner>::selected_text;
            let _: fn(&$runner, &Selection) -> Option<String> = <$runner>::selected_text_for;
            let _: fn(&$runner, &UiEvent) -> Option<ClipboardKind> = <$runner>::clipboard_request;
            let _: fn(&mut $runner, Vec<ToastSpec>) = <$runner>::push_toasts;
            let _: fn(&mut $runner, Vec<Announcement>) = <$runner>::push_announcements;
            let _: fn(&mut $runner, u64) = <$runner>::dismiss_toast;
            let _: fn(&mut $runner, Vec<String>) = <$runner>::push_focus_requests;
            let _: fn(&mut $runner, Vec<ScrollRequest>) = <$runner>::push_scroll_requests;
            let _: fn(&mut $runner, Vec<ViewportRequest>) = <$runner>::push_viewport_requests;
            let _: fn(&mut $runner, Vec<PlotRequest>) = <$runner>::push_plot_requests;
            let _: fn(&mut $runner, AnimationMode) = <$runner>::set_animation_mode;
            let _: fn(&mut $runner, AccessibilityPreferences) =
                <$runner>::set_accessibility_preferences;
            let _: fn(&mut $runner, f32, f32, f32, f32) -> bool = <$runner>::pointer_wheel;
            let _: fn(&mut $runner, f32, f32, f32, f32) -> Option<UiEvent> =
                <$runner>::pointer_wheel_event;
            let _: fn(&mut $runner, f32, f32, f32) -> bool = <$runner>::pinch_zoom;
            let _: fn(&mut $runner, Instant) -> Vec<UiEvent> = <$runner>::poll_input;
            let _: fn(&$runner) -> Cursor = <$runner>::snapshot_cursor;
            let _: fn(&mut $runner, ColorSpace) = <$runner>::set_working_color_space;
            let _: fn(&$runner) -> ColorSpace = <$runner>::working_color_space;
            let _: fn(&mut $runner, f32) = <$runner>::set_white_scale;
            let _: fn(&mut $runner, f32, f32) = <$runner>::set_output_luminance;
        }};
    }

    #[test]
    fn backend_runners_share_common_interaction_surface() {
        // Constructors, shader registration, `prepare`, and `render` are
        // intentionally backend-specific because their GPU handles differ.
        // This test pins the shared interaction/lifecycle surface so the
        // three backend runners do not silently drift when core grows.
        // It did drift once: the list was not extended as the runners
        // grew, so the vulkano runner went without the selection,
        // clipboard and key-capture queries the other two had gained.
        // A forwarder added to one runner belongs in this list.
        assert_common_runner_surface!(damascene_wgpu::Runner);
        assert_common_runner_surface!(damascene_vulkano::Runner);
        assert_common_runner_surface!(damascene_ash::Runner);
    }

    /// A real vulkano device with no window, or `None` on a machine
    /// without Vulkan — the render tests in `damascene-vulkano` skip
    /// the same way.
    fn headless_device() -> Option<(Arc<Device>, Arc<Queue>)> {
        let library = VulkanLibrary::new().ok()?;
        let instance = Instance::new(
            library,
            InstanceCreateInfo {
                flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
                ..Default::default()
            },
        )
        .ok()?;
        let (physical_device, queue_family_index) = instance
            .enumerate_physical_devices()
            .ok()?
            .filter_map(|p| {
                p.queue_family_properties()
                    .iter()
                    .position(|q| q.queue_flags.intersects(QueueFlags::GRAPHICS))
                    .map(|i| (p, i as u32))
            })
            .next()?;
        let (device, mut queues) = Device::new(
            physical_device,
            DeviceCreateInfo {
                enabled_features: damascene_vulkano::required_device_features(),
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .ok()?;
        Some((device, queues.next()?))
    }

    /// Stands in for the desktop clipboard, so the test neither reads
    /// nor overwrites the real one.
    #[derive(Default)]
    struct FakeClipboard(Option<String>);

    impl ClipboardText for FakeClipboard {
        fn set_text(&mut self, text: String) {
            self.0 = Some(text);
        }

        fn text(&mut self) -> Option<String> {
            self.0.clone()
        }
    }

    /// One text field, with no clipboard code of its own.
    struct Field {
        value: String,
        selection: Selection,
        password: bool,
    }

    impl Field {
        fn opts(&self) -> TextInputOpts<'static> {
            if self.password {
                TextInputOpts::default().password()
            } else {
                TextInputOpts::default()
            }
        }
    }

    impl App for Field {
        fn build(&self, _cx: &BuildCx) -> El {
            text_input_with("field", &self.value, &self.selection, self.opts())
        }

        fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
            let opts = self.opts();
            text_input::apply_event_with(
                &mut self.value,
                &mut self.selection,
                &event,
                "field",
                &opts,
            );
        }

        fn selection(&self) -> Selection {
            self.selection.clone()
        }
    }

    const VIEWPORT: Rect = Rect::new(0.0, 0.0, 320.0, 120.0);

    /// What the host's redraw does before painting: build, hand the
    /// runner the app's selection, lay out.
    fn frame(runner: &mut Runner, app: &Field) {
        let theme = app.theme();
        let cx = BuildCx::new(&theme)
            .with_ui_state(runner.ui_state())
            .with_viewport(VIEWPORT.w, VIEWPORT.h);
        let tree = app.build(&cx);
        runner.set_selection(app.selection());
        runner.prepare(tree, VIEWPORT, 1.0);
    }

    /// Ctrl+`ch`, delivered the way the host's key handler does it.
    fn press_ctrl(runner: &mut Runner, app: &mut Field, system: &mut FakeClipboard, ch: &str) {
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        runner.set_modifiers(ctrl);
        let events = runner.key_down(
            LogicalKey::Character(ch.into()),
            DPhysicalKey::Unidentified,
            ctrl,
            false,
        );
        for event in events {
            dispatch_keyboard_event(app, event, runner, system);
        }
        frame(runner, app);
    }

    /// A field focused by a click, with everything in it selected.
    fn focused_and_selected(runner: &mut Runner, app: &mut Field, system: &mut FakeClipboard) {
        frame(runner, app);
        let rect = runner.rect_of_key("field").expect("the field is laid out");
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let press = Pointer::mouse(x, y, PointerButton::Primary);
        for event in runner.pointer_down(press) {
            app.on_event(event, &event_cx(runner));
        }
        for event in runner.pointer_up(press) {
            app.on_event(event, &event_cx(runner));
        }
        frame(runner, app);
        press_ctrl(runner, app, system, "a");
    }

    #[test]
    fn copy_cut_and_paste_run_through_the_vulkano_runner() {
        // The host's clipboard handling against a real `Runner`: the
        // runner says what a key event asks for and what text is
        // selected, the host moves the text, and the app sees only a
        // delete or an insertion.
        let Some((device, queue)) = headless_device() else {
            eprintln!("skipping: no Vulkan device");
            return;
        };
        let mut runner = Runner::new(device.clone(), queue.clone(), Format::R8G8B8A8_SRGB);
        runner.set_surface_size(VIEWPORT.w as u32, VIEWPORT.h as u32);
        let mut system = FakeClipboard::default();
        let mut app = Field {
            value: "hello world".into(),
            selection: Selection::default(),
            password: false,
        };
        focused_and_selected(&mut runner, &mut app, &mut system);

        press_ctrl(&mut runner, &mut app, &mut system, "c");
        assert_eq!(system.0.as_deref(), Some("hello world"), "copy");
        assert_eq!(app.value, "hello world", "copy leaves the field alone");

        system.0 = None;
        press_ctrl(&mut runner, &mut app, &mut system, "x");
        assert_eq!(system.0.as_deref(), Some("hello world"), "cut copies");
        assert_eq!(app.value, "", "cut deletes the selection");

        system.0 = Some("pasted".into());
        press_ctrl(&mut runner, &mut app, &mut system, "v");
        assert_eq!(app.value, "pasted", "paste inserts the clipboard's text");

        // A password field's text never reaches the clipboard: the
        // runner reports no copy or cut request for it, which is why
        // the host asks the runner and not the bare key.
        let mut runner = Runner::new(device, queue, Format::R8G8B8A8_SRGB);
        runner.set_surface_size(VIEWPORT.w as u32, VIEWPORT.h as u32);
        let mut system = FakeClipboard::default();
        let mut secret = Field {
            value: "hunter2".into(),
            selection: Selection::default(),
            password: true,
        };
        focused_and_selected(&mut runner, &mut secret, &mut system);
        press_ctrl(&mut runner, &mut secret, &mut system, "c");
        press_ctrl(&mut runner, &mut secret, &mut system, "x");
        assert_eq!(
            system.0, None,
            "nothing copied or cut from a password field"
        );
        assert_eq!(secret.value, "hunter2", "and the cut did not delete");
    }
}
