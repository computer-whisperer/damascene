//! Native smoke harness for `damascene-ash`.
//!
//! The reusable integration point is `damascene-ash`; this crate owns the
//! winit window, Vulkan instance/device/queue, swapchain, command
//! buffer, and frame synchronization so the ash backend can be exercised
//! the same way `damascene-vulkano-demo` exercises `damascene-vulkano`.

use std::{
    any::Any,
    f32::consts::TAU,
    ffi::{CStr, CString},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ash::vk;
use damascene_ash::{AshContext, AshRenderTarget, LoadOp, Runner, TargetInfo};
use damascene_core::{
    App, BuildCx, Cursor, EventCx, KeyModifiers, Pointer, PointerButton, Rect, UiEvent, clipboard,
    widgets::text_input::{self, ClipboardKind},
};
use damascene_core::{LogicalKey, NamedKey as DNamedKey, PhysicalKey as DPhysicalKey};
use gpu_allocator::{
    AllocationSizes, AllocatorDebugSettings, MemoryLocation,
    vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::{CursorIcon, Window, WindowId},
};

/// Run a windowed app on the ash backend. Blocks until the user closes
/// the window.
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_init(title, viewport, app, |_| Ok(()))
}

/// Like [`run`], but invokes `init_runner` on the freshly-built
/// [`Runner`] before the first frame. Use this to register custom
/// shaders needed by the app's tree.
pub fn run_with_init<A, F>(
    title: &'static str,
    viewport: Rect,
    app: A,
    init_runner: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    A: App + 'static,
    F: FnOnce(&mut Runner) -> damascene_ash::Result<()> + 'static,
{
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let entry = unsafe { ash::Entry::load()? };
    let extension_names =
        ash_window::enumerate_required_extensions(event_loop.display_handle()?.as_raw())?;
    let app_name = CString::new("damascene-ash-demo")?;
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app_name)
        .application_version(0)
        .engine_name(&app_name)
        .engine_version(0)
        .api_version(vk::API_VERSION_1_3);
    let instance_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(extension_names);
    let instance = unsafe { entry.create_instance(&instance_info, None)? };

    let mut host = Host {
        rcx: None,
        entry,
        instance,
        title,
        viewport,
        app,
        modifiers: KeyModifiers::default(),
        last_pointer: None,
        last_cursor: Cursor::Default,
        clipboard: new_clipboard(),
        next_redraw: None,
        ime_allowed: false,
        init_runner: Some(Box::new(init_runner)),
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

type InitRunner = Box<dyn FnOnce(&mut Runner) -> damascene_ash::Result<()>>;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
type PlatformClipboard = Option<arboard::Clipboard>;

#[cfg(any(target_os = "android", target_os = "ios"))]
struct PlatformClipboard;

const RESIZE_DEBOUNCE: Duration = Duration::from_millis(100);
const ANIMATED_SURFACE_SIZE: u32 = 96;

struct Host<A: App> {
    rcx: Option<RenderContext>,
    entry: ash::Entry,
    instance: ash::Instance,
    title: &'static str,
    viewport: Rect,
    app: A,
    modifiers: KeyModifiers,
    last_pointer: Option<(f32, f32)>,
    last_cursor: Cursor,
    clipboard: PlatformClipboard,
    next_redraw: Option<Instant>,
    ime_allowed: bool,
    init_runner: Option<InitRunner>,
}

impl<A: App> Drop for Host<A> {
    fn drop(&mut self) {
        self.rcx.take();
        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}

/// Per-dispatch [`EventCx`] for [`App::on_event`] / `on_wheel_event` —
/// geometry queries answer from the runner's last laid-out frame.
fn event_cx(runner: &Runner) -> EventCx<'_> {
    EventCx::new().with_ui_state(runner.ui_state())
}

struct RenderContext {
    window: Arc<Window>,
    surface_loader: ash::khr::surface::Instance,
    swapchain_loader: ash::khr::swapchain::Device,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: Arc<ash::Device>,
    queue: vk::Queue,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    in_flight: vk::Fence,
    swapchain: vk::SwapchainKHR,
    swapchain_format: vk::Format,
    swapchain_extent: vk::Extent2D,
    swapchain_images: Vec<vk::Image>,
    swapchain_views: Vec<vk::ImageView>,
    image_layouts: Vec<vk::ImageLayout>,
    runner: Option<Runner>,
    animated_surface: Option<AnimatedSurface>,
    allocator: Option<Arc<Mutex<Allocator>>>,
    recreate_swapchain: bool,
    resize_debounce: Option<Instant>,
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
        }
        self.runner.take();
        if let Some(mut surface) = self.animated_surface.take()
            && let Some(allocator) = self.allocator.as_ref()
            && let Ok(mut allocator) = allocator.lock()
        {
            unsafe {
                surface.destroy(&self.device, &mut allocator);
            }
        }
        self.allocator.take();
        unsafe {
            for view in self.swapchain_views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            self.swapchain_loader
                .destroy_swapchain(self.swapchain, None);
            self.device.destroy_semaphore(self.render_finished, None);
            self.device.destroy_semaphore(self.image_available, None);
            self.device.destroy_fence(self.in_flight, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.device.destroy_device(None);
        }
    }
}

impl<A: App + 'static> ApplicationHandler for Host<A> {
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
        let mut rcx = unsafe {
            create_render_context(
                &self.entry,
                &self.instance,
                window,
                self.app.theme(),
                self.app.shaders(),
                self.init_runner.take(),
            )
            .expect("create ash render context")
        };
        if let Some(showcase) =
            (&mut self.app as &mut dyn Any).downcast_mut::<damascene_fixtures::Showcase>()
        {
            let texture = unsafe {
                rcx.create_animated_surface()
                    .expect("create ash showcase animated surface")
            };
            showcase.set_animated_surface(Some(texture));
        }
        self.rcx = Some(rcx);
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
                let moved = rcx.runner_mut().pointer_moved(Pointer::moving(lx, ly));
                for event in moved.events {
                    self.app.on_event(event, &event_cx(rcx.runner()));
                }
                if moved.needs_redraw {
                    rcx.window.request_redraw();
                }
            }

            WindowEvent::CursorLeft { .. } => {
                self.last_pointer = None;
                for event in rcx.runner_mut().pointer_left() {
                    self.app.on_event(event, &event_cx(rcx.runner()));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::HoveredFile(path) => {
                let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                for event in rcx.runner_mut().file_hovered(path, lx, ly) {
                    self.app.on_event(event, &event_cx(rcx.runner()));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::HoveredFileCancelled => {
                for event in rcx.runner_mut().file_hover_cancelled() {
                    self.app.on_event(event, &event_cx(rcx.runner()));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::DroppedFile(path) => {
                let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                for event in rcx.runner_mut().file_dropped(path, lx, ly) {
                    self.app.on_event(event, &event_cx(rcx.runner()));
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
                        let events = rcx
                            .runner_mut()
                            .pointer_down(Pointer::mouse(lx, ly, button));
                        for event in events {
                            self.app.on_event(event, &event_cx(rcx.runner()));
                        }
                        sync_ime(&rcx.window, rcx.runner(), &mut self.ime_allowed);
                        rcx.window.request_redraw();
                    }
                    ElementState::Released => {
                        let events = rcx.runner_mut().pointer_up(Pointer::mouse(lx, ly, button));
                        for event in events {
                            self.app.on_event(event, &event_cx(rcx.runner()));
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
                let consumed =
                    if let Some(event) = rcx.runner_mut().pointer_wheel_event(lx, ly, dx, dy) {
                        needs_redraw = true;
                        self.app.on_wheel_event(event, &event_cx(rcx.runner()))
                    } else {
                        false
                    };
                if !consumed && rcx.runner_mut().pointer_wheel(lx, ly, dy) {
                    needs_redraw = true;
                }
                if needs_redraw {
                    rcx.window.request_redraw();
                }
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = key_modifiers(modifiers.state());
                rcx.runner_mut().set_modifiers(self.modifiers);
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
                    let events = rcx.runner_mut().key_down(
                        logical,
                        physical,
                        self.modifiers,
                        key_event.repeat,
                    );
                    for event in events {
                        dispatch_keyboard_event(
                            &mut self.app,
                            event,
                            rcx.runner(),
                            &mut self.clipboard,
                        );
                    }
                }
                if let Some(text) = &key_event.text
                    && let Some(event) = rcx.runner_mut().text_input(text.to_string())
                {
                    self.app.on_event(event, &event_cx(rcx.runner()));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if let Some(ev) = rcx.runner_mut().text_input(text) {
                    self.app.on_event(ev, &event_cx(rcx.runner()));
                }
                rcx.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                for event in rcx.runner_mut().poll_input(web_time::Instant::now()) {
                    self.app.on_event(event, &event_cx(rcx.runner()));
                }
                if let Some(last_resize) = rcx.resize_debounce
                    && last_resize.elapsed() >= RESIZE_DEBOUNCE
                {
                    rcx.recreate_swapchain = true;
                    rcx.resize_debounce = None;
                }

                if rcx.resize_debounce.is_none() {
                    let extent = rcx.window.inner_size();
                    if extent.width == 0 || extent.height == 0 {
                        return;
                    }
                }

                if rcx.recreate_swapchain {
                    unsafe {
                        rcx.recreate_swapchain().expect("recreate swapchain");
                    }
                }

                self.app.before_build();
                let scale_factor = rcx.window.scale_factor() as f32;
                let extent = rcx.swapchain_extent;
                let viewport = Rect::new(
                    0.0,
                    0.0,
                    extent.width as f32 / scale_factor,
                    extent.height as f32 / scale_factor,
                );
                let theme = self.app.theme();
                let cx = BuildCx::new(&theme)
                    .with_ui_state(rcx.runner().ui_state())
                    .with_viewport(viewport.w, viewport.h);
                let tree = self.app.build(&cx);
                let palette = theme.palette().clone();
                // `prepare` writes persistently-mapped buffers and
                // destroys GPU resources it evicts or regrows, none of
                // it fence-gated — the previous frame's command buffer
                // must have retired first (see `Runner::prepare`'s
                // Synchronization doc).
                rcx.wait_frame_fence().expect("wait frame fence");
                let runner = rcx.runner_mut();
                runner.set_theme(theme);
                runner.set_hotkeys(self.app.hotkeys());
                runner.set_selection(self.app.selection());
                runner.push_toasts(self.app.drain_toasts());
                runner.push_focus_requests(self.app.drain_focus_requests());
                runner.push_scroll_requests(self.app.drain_scroll_requests());
                runner.push_viewport_requests(self.app.drain_viewport_requests());
                for url in self.app.drain_link_opens() {
                    open_link(&url);
                }
                let prepare = runner.prepare(tree, viewport, scale_factor);
                let cursor = runner.snapshot_cursor();
                if cursor != self.last_cursor {
                    rcx.window.set_cursor(winit_cursor(cursor));
                    self.last_cursor = cursor;
                }
                sync_ime(&rcx.window, rcx.runner(), &mut self.ime_allowed);

                match unsafe { rcx.render_frame(clear_color(&palette)) } {
                    Ok(suboptimal) => {
                        if suboptimal && rcx.resize_debounce.is_none() {
                            rcx.recreate_swapchain = true;
                        }
                    }
                    Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                        rcx.recreate_swapchain = true;
                    }
                    Err(err) => panic!("render frame: {err:?}"),
                }

                if prepare.needs_redraw {
                    rcx.window.request_redraw();
                }
                self.next_redraw = prepare.next_redraw_in.map(|d| Instant::now() + d);
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(rcx) = self.rcx.as_ref() else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            return;
        };
        let now = Instant::now();
        let mut wake_up = self.next_redraw;
        if let Some(last_resize) = rcx.resize_debounce {
            let deadline = last_resize + RESIZE_DEBOUNCE;
            if now >= deadline {
                rcx.window.request_redraw();
                self.next_redraw = None;
                event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
                return;
            }
            wake_up = Some(match wake_up {
                Some(existing) => existing.min(deadline),
                None => deadline,
            });
        }
        if let Some(deadline) = self.next_redraw
            && now >= deadline
        {
            rcx.window.request_redraw();
            self.next_redraw = None;
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            return;
        }
        if let Some(deadline) = wake_up {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(deadline));
        } else {
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
        }
    }
}

impl RenderContext {
    fn runner(&self) -> &Runner {
        self.runner.as_ref().expect("runner")
    }

    fn runner_mut(&mut self) -> &mut Runner {
        self.runner.as_mut().expect("runner")
    }

    /// Block until the previously submitted frame has retired. Must
    /// run before `Runner::prepare`, which writes mapped buffers and
    /// destroys evicted resources the in-flight command buffer may
    /// still reference. `render_frame`'s own wait then returns
    /// immediately on the already-signalled fence.
    fn wait_frame_fence(&self) -> Result<(), vk::Result> {
        unsafe {
            self.device
                .wait_for_fences(&[self.in_flight], true, u64::MAX)
        }
    }

    unsafe fn create_animated_surface(
        &mut self,
    ) -> Result<damascene_core::surface::AppTexture, Box<dyn std::error::Error>> {
        let allocator = self.allocator.as_ref().expect("allocator").clone();
        let mut allocator = allocator
            .lock()
            .map_err(|_| std::io::Error::other("allocator mutex poisoned"))?;
        let surface = unsafe { AnimatedSurface::new(&self.device, &mut allocator)? };
        let texture = damascene_ash::app_texture(
            surface.image,
            surface.view,
            vk::Format::R8G8B8A8_SRGB,
            surface.extent,
        );
        self.animated_surface = Some(surface);
        Ok(texture)
    }

    unsafe fn recreate_swapchain(&mut self) -> Result<(), vk::Result> {
        unsafe {
            self.device.device_wait_idle()?;
        }
        let old = self.swapchain;
        let (swapchain, format, extent, images) = unsafe {
            create_swapchain(
                &self.surface_loader,
                &self.swapchain_loader,
                self.surface,
                self.physical_device,
                &self.window,
                old,
            )?
        };
        unsafe {
            for view in self.swapchain_views.drain(..) {
                self.device.destroy_image_view(view, None);
            }
            self.swapchain_loader.destroy_swapchain(old, None);
        }
        self.swapchain = swapchain;
        self.swapchain_format = format;
        self.swapchain_extent = extent;
        self.swapchain_images = images;
        self.swapchain_views = create_image_views(&self.device, &self.swapchain_images, format)?;
        self.image_layouts = vec![vk::ImageLayout::UNDEFINED; self.swapchain_images.len()];
        self.runner_mut()
            .set_surface_size(extent.width, extent.height);
        self.recreate_swapchain = false;
        Ok(())
    }

    unsafe fn render_frame(&mut self, clear: [f32; 4]) -> Result<bool, vk::Result> {
        unsafe {
            self.device
                .wait_for_fences(&[self.in_flight], true, u64::MAX)?;
        }

        let (image_index, suboptimal) = unsafe {
            match self.swapchain_loader.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available,
                vk::Fence::null(),
            ) {
                Ok(v) => v,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    return Err(vk::Result::ERROR_OUT_OF_DATE_KHR);
                }
                Err(err) => return Err(err),
            }
        };

        unsafe {
            self.device.reset_fences(&[self.in_flight])?;
            self.device
                .reset_command_buffer(self.command_buffer, vk::CommandBufferResetFlags::empty())?;
        }

        let i = image_index as usize;
        let target = AshRenderTarget {
            image: self.swapchain_images[i],
            view: self.swapchain_views[i],
            format: self.swapchain_format,
            extent: self.swapchain_extent,
            sample_count: vk::SampleCountFlags::TYPE_1,
            initial_layout: self.image_layouts[i],
            final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
        };

        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.device
                .begin_command_buffer(self.command_buffer, &begin)?;
            let command_buffer = self.command_buffer;
            if let Some(surface) = self.animated_surface.as_mut() {
                surface.record_upload(&self.device, command_buffer);
            }
            self.runner_mut()
                .render(command_buffer, target, LoadOp::Clear(clear))
                .expect("damascene-ash render");
            self.device.end_command_buffer(self.command_buffer)?;
        }
        self.image_layouts[i] = vk::ImageLayout::PRESENT_SRC_KHR;

        let wait_semaphores = [self.image_available];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let command_buffers = [self.command_buffer];
        let signal_semaphores = [self.render_finished];
        let submit_info = vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&command_buffers)
            .signal_semaphores(&signal_semaphores);
        unsafe {
            self.device
                .queue_submit(self.queue, &[submit_info], self.in_flight)?;
        }

        let swapchains = [self.swapchain];
        let indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&indices);
        unsafe {
            match self
                .swapchain_loader
                .queue_present(self.queue, &present_info)
            {
                Ok(present_suboptimal) => Ok(suboptimal || present_suboptimal),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Ok(true),
                Err(err) => Err(err),
            }
        }
    }
}

unsafe fn create_render_context(
    entry: &ash::Entry,
    instance: &ash::Instance,
    window: Arc<Window>,
    theme: damascene_core::Theme,
    shaders: Vec<damascene_core::AppShader>,
    init_runner: Option<InitRunner>,
) -> Result<RenderContext, Box<dyn std::error::Error>> {
    let surface = unsafe {
        ash_window::create_surface(
            entry,
            instance,
            window.display_handle()?.as_raw(),
            window.window_handle()?.as_raw(),
            None,
        )?
    };
    let surface_loader = ash::khr::surface::Instance::new(entry, instance);

    let (physical_device, queue_family_index) =
        unsafe { pick_physical_device(instance, &surface_loader, surface)? };
    let (device, queue) = unsafe { create_device(instance, physical_device, queue_family_index)? };
    let device = Arc::new(device);
    let swapchain_loader = ash::khr::swapchain::Device::new(instance, &device);

    let (swapchain, swapchain_format, swapchain_extent, swapchain_images) = unsafe {
        create_swapchain(
            &surface_loader,
            &swapchain_loader,
            surface,
            physical_device,
            &window,
            vk::SwapchainKHR::null(),
        )?
    };
    let swapchain_views = create_image_views(&device, &swapchain_images, swapchain_format)?;
    let image_layouts = vec![vk::ImageLayout::UNDEFINED; swapchain_images.len()];

    let command_pool = unsafe {
        let info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        device.create_command_pool(&info, None)?
    };
    let command_buffer = unsafe {
        let info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        device.allocate_command_buffers(&info)?[0]
    };
    let semaphore_info = vk::SemaphoreCreateInfo::default();
    let image_available = unsafe { device.create_semaphore(&semaphore_info, None)? };
    let render_finished = unsafe { device.create_semaphore(&semaphore_info, None)? };
    let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
    let in_flight = unsafe { device.create_fence(&fence_info, None)? };

    let allocator = Arc::new(Mutex::new(Allocator::new(&AllocatorCreateDesc {
        instance: instance.clone(),
        device: (*device).clone(),
        physical_device,
        debug_settings: AllocatorDebugSettings::default(),
        buffer_device_address: false,
        allocation_sizes: AllocationSizes::default(),
    })?));
    let max_image_dimension_2d =
        unsafe { instance.get_physical_device_properties(physical_device) }
            .limits
            .max_image_dimension2_d;
    let context = AshContext::new(
        device.clone(),
        allocator.clone(),
        queue_family_index,
        max_image_dimension_2d,
    );
    let mut runner = Runner::new(context, TargetInfo::new(swapchain_format))?;
    runner.set_theme(theme);
    runner.set_surface_size(swapchain_extent.width, swapchain_extent.height);
    for shader in shaders {
        match runner.register_shader_with(
            shader.name,
            shader.wgsl,
            shader.samples_backdrop,
            shader.samples_time,
        ) {
            Ok(()) => {}
            Err(damascene_ash::Error::Unsupported(message)) => {
                eprintln!(
                    "damascene-ash-demo: skipping shader `{}`: {message}",
                    shader.name
                );
            }
            Err(err) => return Err(err.into()),
        }
    }
    if let Some(init) = init_runner {
        init(&mut runner)?;
    }

    Ok(RenderContext {
        window,
        surface_loader,
        swapchain_loader,
        surface,
        physical_device,
        device,
        queue,
        command_pool,
        command_buffer,
        image_available,
        render_finished,
        in_flight,
        swapchain,
        swapchain_format,
        swapchain_extent,
        swapchain_images,
        swapchain_views,
        image_layouts,
        runner: Some(runner),
        animated_surface: None,
        allocator: Some(allocator),
        recreate_swapchain: false,
        resize_debounce: None,
    })
}

struct AnimatedSurface {
    image: vk::Image,
    view: vk::ImageView,
    extent: vk::Extent2D,
    image_allocation: Option<Allocation>,
    staging: vk::Buffer,
    staging_allocation: Option<Allocation>,
    pixels: Vec<u8>,
    layout: vk::ImageLayout,
    start: Instant,
}

impl AnimatedSurface {
    unsafe fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let extent = vk::Extent2D {
            width: ANIMATED_SURFACE_SIZE,
            height: ANIMATED_SURFACE_SIZE,
        };
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_SRGB)
            .extent(vk::Extent3D {
                width: extent.width,
                height: extent.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { device.create_image(&image_info, None)? };
        let image_requirements = unsafe { device.get_image_memory_requirements(image) };
        let image_allocation = allocator.allocate(&AllocationCreateDesc {
            name: "damascene_ash_demo::animated_surface_image",
            requirements: image_requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        unsafe {
            device.bind_image_memory(
                image,
                image_allocation.memory(),
                image_allocation.offset(),
            )?;
        }

        let subresource = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_SRGB)
            .subresource_range(subresource);
        let view = unsafe { device.create_image_view(&view_info, None)? };

        let byte_len = (extent.width * extent.height * 4) as vk::DeviceSize;
        let buffer_info = vk::BufferCreateInfo::default()
            .size(byte_len)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let staging = unsafe { device.create_buffer(&buffer_info, None)? };
        let staging_requirements = unsafe { device.get_buffer_memory_requirements(staging) };
        let staging_allocation = allocator.allocate(&AllocationCreateDesc {
            name: "damascene_ash_demo::animated_surface_staging",
            requirements: staging_requirements,
            location: MemoryLocation::CpuToGpu,
            linear: true,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        unsafe {
            device.bind_buffer_memory(
                staging,
                staging_allocation.memory(),
                staging_allocation.offset(),
            )?;
        }

        Ok(Self {
            image,
            view,
            extent,
            image_allocation: Some(image_allocation),
            staging,
            staging_allocation: Some(staging_allocation),
            pixels: vec![0; byte_len as usize],
            layout: vk::ImageLayout::UNDEFINED,
            start: Instant::now(),
        })
    }

    fn record_upload(&mut self, device: &ash::Device, cmd: vk::CommandBuffer) {
        write_animated_surface_frame(&mut self.pixels, self.start.elapsed().as_secs_f32());
        let allocation = self
            .staging_allocation
            .as_mut()
            .expect("animated surface staging allocation");
        let mapped = allocation
            .mapped_slice_mut()
            .expect("animated surface staging allocation mapped");
        mapped[..self.pixels.len()].copy_from_slice(&self.pixels);

        unsafe {
            transition_sampled_image(
                device,
                cmd,
                self.image,
                self.layout,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            let region = vk::BufferImageCopy::default()
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1),
                )
                .image_extent(vk::Extent3D {
                    width: self.extent.width,
                    height: self.extent.height,
                    depth: 1,
                });
            device.cmd_copy_buffer_to_image(
                cmd,
                self.staging,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
            transition_sampled_image(
                device,
                cmd,
                self.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }
        self.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
    }

    unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        unsafe {
            if self.view != vk::ImageView::null() {
                device.destroy_image_view(self.view, None);
                self.view = vk::ImageView::null();
            }
            if self.image != vk::Image::null() {
                device.destroy_image(self.image, None);
                self.image = vk::Image::null();
            }
            if let Some(allocation) = self.image_allocation.take() {
                let _ = allocator.free(allocation);
            }
            if self.staging != vk::Buffer::null() {
                device.destroy_buffer(self.staging, None);
                self.staging = vk::Buffer::null();
            }
            if let Some(allocation) = self.staging_allocation.take() {
                let _ = allocator.free(allocation);
            }
        }
    }
}

unsafe fn transition_sampled_image(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    if old_layout == new_layout {
        return;
    }
    let (src_access, src_stage) = match old_layout {
        vk::ImageLayout::UNDEFINED => (
            vk::AccessFlags::empty(),
            vk::PipelineStageFlags::TOP_OF_PIPE,
        ),
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => (
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    let (dst_access, dst_stage) = match new_layout {
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
            vk::AccessFlags::TRANSFER_WRITE,
            vk::PipelineStageFlags::TRANSFER,
        ),
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
        ),
        _ => (
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            vk::PipelineStageFlags::ALL_COMMANDS,
        ),
    };
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1),
        );
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
    }
}

fn write_animated_surface_frame(pixels: &mut [u8], t: f32) {
    let w = ANIMATED_SURFACE_SIZE as f32;
    let cx = w * 0.5;
    let cy = w * 0.5;
    let r_outer = w * 0.45;
    let r_inner = w * 0.18;

    for y in 0..ANIMATED_SURFACE_SIZE {
        for x in 0..ANIMATED_SURFACE_SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let theta = dy.atan2(dx);
            let hue = (theta / TAU + t * 0.25).rem_euclid(1.0);
            let (rr, gg, bb) = hsv_to_rgb(hue, 0.9, 1.0);

            let band_t = ((r - r_inner) / (r_outer - r_inner)).clamp(0.0, 1.0);
            let cov = (1.0 - (band_t * 2.0 - 1.0).abs()).max(0.0);
            let cov = cov * cov * (3.0 - 2.0 * cov);

            let i = ((y * ANIMATED_SURFACE_SIZE + x) * 4) as usize;
            pixels[i] = ((rr * cov) * 255.0).round() as u8;
            pixels[i + 1] = ((gg * cov) * 255.0).round() as u8;
            pixels[i + 2] = ((bb * cov) * 255.0).round() as u8;
            pixels[i + 3] = (cov * 255.0).round() as u8;
        }
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match (i as i32) % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

unsafe fn pick_physical_device(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32), Box<dyn std::error::Error>> {
    let devices = unsafe { instance.enumerate_physical_devices()? };
    devices
        .into_iter()
        .filter(|&physical| unsafe { supports_required_features(instance, physical) })
        .filter(|&physical| unsafe { supports_swapchain(instance, physical) })
        .filter_map(|physical| {
            let props = unsafe { instance.get_physical_device_queue_family_properties(physical) };
            props.iter().enumerate().find_map(|(i, q)| {
                let graphics = q.queue_flags.contains(vk::QueueFlags::GRAPHICS);
                let present = unsafe {
                    surface_loader
                        .get_physical_device_surface_support(physical, i as u32, surface)
                        .unwrap_or(false)
                };
                (graphics && present).then_some((physical, i as u32))
            })
        })
        .min_by_key(|(physical, _)| {
            let props = unsafe { instance.get_physical_device_properties(*physical) };
            match props.device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
                vk::PhysicalDeviceType::CPU => 3,
                _ => 4,
            }
        })
        .ok_or_else(|| "no compatible Vulkan physical device".into())
}

unsafe fn supports_required_features(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
) -> bool {
    let mut features13 = vk::PhysicalDeviceVulkan13Features::default();
    let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut features13);
    unsafe {
        instance.get_physical_device_features2(physical, &mut features);
    }
    features.features.sample_rate_shading == vk::TRUE && features13.dynamic_rendering == vk::TRUE
}

unsafe fn supports_swapchain(instance: &ash::Instance, physical: vk::PhysicalDevice) -> bool {
    let Ok(exts) = (unsafe { instance.enumerate_device_extension_properties(physical) }) else {
        return false;
    };
    exts.iter().any(|ext| {
        let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
        name == ash::khr::swapchain::NAME
    })
}

unsafe fn create_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
) -> Result<(ash::Device, vk::Queue), vk::Result> {
    let priorities = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family_index)
        .queue_priorities(&priorities);
    let queue_infos = [queue_info];
    let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
    let features = damascene_ash::required_device_features();
    let mut features13 = damascene_ash::required_vulkan_13_features();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_infos)
        .enabled_extension_names(&device_extensions)
        .enabled_features(&features)
        .push_next(&mut features13);
    let device = unsafe { instance.create_device(physical_device, &device_info, None)? };
    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
    Ok((device, queue))
}

unsafe fn create_swapchain(
    surface_loader: &ash::khr::surface::Instance,
    swapchain_loader: &ash::khr::swapchain::Device,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    window: &Window,
    old_swapchain: vk::SwapchainKHR,
) -> Result<(vk::SwapchainKHR, vk::Format, vk::Extent2D, Vec<vk::Image>), vk::Result> {
    let caps = unsafe {
        surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?
    };
    let formats =
        unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface)? };
    let present_modes = unsafe {
        surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?
    };
    let (format, color_space) = choose_surface_format(&formats);
    let extent = choose_extent(&caps, window);
    let present_mode = present_modes
        .iter()
        .copied()
        .find(|mode| *mode == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO);
    let mut image_count = caps.min_image_count.max(2);
    if caps.max_image_count > 0 {
        image_count = image_count.min(caps.max_image_count);
    }
    let info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(format)
        .image_color_space(color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(caps.current_transform)
        .composite_alpha(choose_composite_alpha(caps.supported_composite_alpha))
        .present_mode(present_mode)
        .clipped(true)
        .old_swapchain(old_swapchain);
    let swapchain = unsafe { swapchain_loader.create_swapchain(&info, None)? };
    let images = unsafe { swapchain_loader.get_swapchain_images(swapchain)? };
    Ok((swapchain, format, extent, images))
}

fn choose_surface_format(formats: &[vk::SurfaceFormatKHR]) -> (vk::Format, vk::ColorSpaceKHR) {
    formats
        .iter()
        .copied()
        .find(|f| {
            matches!(
                f.format,
                vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
            )
        })
        .or_else(|| formats.first().copied())
        .map(|f| (f.format, f.color_space))
        .expect("surface format")
}

fn choose_extent(caps: &vk::SurfaceCapabilitiesKHR, window: &Window) -> vk::Extent2D {
    if caps.current_extent.width != u32::MAX {
        return caps.current_extent;
    }
    let size = window.inner_size();
    vk::Extent2D {
        width: size
            .width
            .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
        height: size
            .height
            .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
    }
}

fn choose_composite_alpha(flags: vk::CompositeAlphaFlagsKHR) -> vk::CompositeAlphaFlagsKHR {
    [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ]
    .into_iter()
    .find(|flag| flags.contains(*flag))
    .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE)
}

fn create_image_views(
    device: &ash::Device,
    images: &[vk::Image],
    format: vk::Format,
) -> Result<Vec<vk::ImageView>, vk::Result> {
    images
        .iter()
        .map(|image| {
            let subresource = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1);
            let info = vk::ImageViewCreateInfo::default()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(subresource);
            unsafe { device.create_image_view(&info, None) }
        })
        .collect()
}

fn map_key(key: &winit::keyboard::Key) -> LogicalKey {
    match key {
        winit::keyboard::Key::Named(named) => match map_named(named) {
            Some(n) => LogicalKey::Named(n),
            None => LogicalKey::Unidentified,
        },
        winit::keyboard::Key::Character(s) => LogicalKey::Character(s.to_string()),
        _ => LogicalKey::Unidentified,
    }
}

fn map_named(named: &winit::keyboard::NamedKey) -> Option<DNamedKey> {
    use winit::keyboard::NamedKey as W;
    macro_rules! same {
        ($($v:ident),+ $(,)?) => {
            Some(match named {
                $( W::$v => DNamedKey::$v, )+
                _ => return None,
            })
        };
    }
    same!(
        Alt,
        AltGraph,
        CapsLock,
        Control,
        Fn,
        FnLock,
        Meta,
        NumLock,
        ScrollLock,
        Shift,
        Super,
        Hyper,
        Symbol,
        Enter,
        Tab,
        Space,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        ArrowUp,
        End,
        Home,
        PageDown,
        PageUp,
        Backspace,
        Clear,
        Copy,
        CrSel,
        Cut,
        Delete,
        EraseEof,
        ExSel,
        Insert,
        Paste,
        Redo,
        Undo,
        Accept,
        Again,
        Cancel,
        ContextMenu,
        Escape,
        Execute,
        Find,
        Help,
        Pause,
        Play,
        Props,
        Select,
        ZoomIn,
        ZoomOut,
        Eject,
        Power,
        PrintScreen,
        WakeUp,
        AudioVolumeDown,
        AudioVolumeMute,
        AudioVolumeUp,
        MediaPlayPause,
        MediaStop,
        MediaTrackNext,
        MediaTrackPrevious,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        F13,
        F14,
        F15,
        F16,
        F17,
        F18,
        F19,
        F20,
        F21,
        F22,
        F23,
        F24,
    )
}

fn map_physical(physical: winit::keyboard::PhysicalKey) -> DPhysicalKey {
    use winit::keyboard::{KeyCode, PhysicalKey as P};
    let code = match physical {
        P::Code(code) => code,
        P::Unidentified(_) => return DPhysicalKey::Unidentified,
    };
    macro_rules! same {
        ($($v:ident),+ $(,)?) => {
            match code {
                $( KeyCode::$v => DPhysicalKey::$v, )+
                KeyCode::SuperLeft => DPhysicalKey::MetaLeft,
                KeyCode::SuperRight => DPhysicalKey::MetaRight,
                KeyCode::NumpadStar => DPhysicalKey::NumpadMultiply,
                _ => DPhysicalKey::Unidentified,
            }
        };
    }
    same!(
        Backquote,
        Backslash,
        BracketLeft,
        BracketRight,
        Comma,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        Equal,
        IntlBackslash,
        IntlRo,
        IntlYen,
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Minus,
        Period,
        Quote,
        Semicolon,
        Slash,
        AltLeft,
        AltRight,
        Backspace,
        CapsLock,
        ContextMenu,
        ControlLeft,
        ControlRight,
        Enter,
        ShiftLeft,
        ShiftRight,
        Space,
        Tab,
        Delete,
        End,
        Help,
        Home,
        Insert,
        PageDown,
        PageUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        ArrowUp,
        NumLock,
        Numpad0,
        Numpad1,
        Numpad2,
        Numpad3,
        Numpad4,
        Numpad5,
        Numpad6,
        Numpad7,
        Numpad8,
        Numpad9,
        NumpadAdd,
        NumpadBackspace,
        NumpadClear,
        NumpadComma,
        NumpadDecimal,
        NumpadDivide,
        NumpadEnter,
        NumpadEqual,
        NumpadMultiply,
        NumpadParenLeft,
        NumpadParenRight,
        NumpadSubtract,
        Escape,
        PrintScreen,
        ScrollLock,
        Pause,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        F13,
        F14,
        F15,
        F16,
        F17,
        F18,
        F19,
        F20,
        F21,
        F22,
        F23,
        F24,
    )
}

fn pointer_button(b: MouseButton) -> Option<PointerButton> {
    match b {
        MouseButton::Left => Some(PointerButton::Primary),
        MouseButton::Right => Some(PointerButton::Secondary),
        MouseButton::Middle => Some(PointerButton::Middle),
        _ => None,
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

fn sync_ime(window: &Window, runner: &Runner, ime_allowed: &mut bool) {
    let allowed = runner.focused_captures_keys();
    if allowed != *ime_allowed {
        window.set_ime_allowed(allowed);
        *ime_allowed = allowed;
    }
}

fn dispatch_keyboard_event<A: App>(
    app: &mut A,
    event: UiEvent,
    runner: &Runner,
    clipboard: &mut PlatformClipboard,
) {
    match text_input::clipboard_request(&event) {
        Some(ClipboardKind::Copy) => {
            copy_current_selection(runner, clipboard);
            app.on_event(event, &event_cx(runner));
        }
        Some(ClipboardKind::Cut) => {
            copy_current_selection(runner, clipboard);
            app.on_event(clipboard::delete_selection_event(event), &event_cx(runner));
        }
        Some(ClipboardKind::Paste) => {
            if let Some(paste) = paste_text_from_clipboard(event.clone(), clipboard) {
                app.on_event(paste, &event_cx(runner));
            } else {
                app.on_event(event, &event_cx(runner));
            }
        }
        None => app.on_event(event, &event_cx(runner)),
    }
}

fn copy_current_selection(runner: &Runner, clipboard: &mut PlatformClipboard) {
    let Some(text) = runner.selected_text() else {
        return;
    };
    set_clipboard_text(clipboard, text);
}

fn paste_text_from_clipboard(event: UiEvent, clipboard: &mut PlatformClipboard) -> Option<UiEvent> {
    let text = get_clipboard_text(clipboard)?;
    Some(clipboard::paste_text_event(event, text))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn new_clipboard() -> PlatformClipboard {
    arboard::Clipboard::new().ok()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn new_clipboard() -> PlatformClipboard {
    PlatformClipboard
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn set_clipboard_text(clipboard: &mut PlatformClipboard, text: String) {
    if let Some(clipboard) = clipboard {
        let _ = clipboard.set_text(text);
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn set_clipboard_text(_clipboard: &mut PlatformClipboard, _text: String) {}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn get_clipboard_text(clipboard: &mut PlatformClipboard) -> Option<String> {
    clipboard.as_mut()?.get_text().ok()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn get_clipboard_text(_clipboard: &mut PlatformClipboard) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn open_link(url: &str) {
    if let Err(err) = open::that_detached(url) {
        eprintln!("damascene-ash-demo: failed to open {url}: {err}");
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn open_link(url: &str) {
    eprintln!("damascene-ash-demo: opening links is not wired on this platform yet: {url}");
}

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

/// Clear color for the swapchain: the background token converted into the
/// working space, exactly like every painted fill. This demo composites in
/// the default sRGB-linear working space, so
/// [`damascene_core::paint::rgba_f32`] is the matching conversion
/// (issue #45).
fn clear_color(palette: &damascene_core::Palette) -> [f32; 4] {
    damascene_core::paint::rgba_f32(palette.background)
}
