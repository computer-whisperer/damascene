//! Native smoke harness for `aetna-ash`.
//!
//! The reusable integration point is `aetna-ash`; this crate owns the
//! winit window, Vulkan instance/device/queue, swapchain, command
//! buffer, and frame synchronization so the ash backend can be exercised
//! the same way `aetna-vulkano-demo` exercises `aetna-vulkano`.

use std::{
    ffi::{CStr, CString},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use aetna_ash::{AshContext, AshRenderTarget, LoadOp, Runner, TargetInfo};
use aetna_core::{App, BuildCx, KeyModifiers, Pointer, PointerButton, Rect, UiKey, tree::Color};
use ash::vk;
use gpu_allocator::{
    AllocationSizes, AllocatorDebugSettings,
    vulkan::{Allocator, AllocatorCreateDesc},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::{Window, WindowId},
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
    F: FnOnce(&mut Runner) -> aetna_ash::Result<()> + 'static,
{
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let entry = unsafe { ash::Entry::load()? };
    let extension_names =
        ash_window::enumerate_required_extensions(event_loop.display_handle()?.as_raw())?;
    let app_name = CString::new("aetna-ash-demo")?;
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
        init_runner: Some(Box::new(init_runner)),
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

type InitRunner = Box<dyn FnOnce(&mut Runner) -> aetna_ash::Result<()>>;

const RESIZE_DEBOUNCE: Duration = Duration::from_millis(100);

struct Host<A: App> {
    rcx: Option<RenderContext>,
    entry: ash::Entry,
    instance: ash::Instance,
    title: &'static str,
    viewport: Rect,
    app: A,
    modifiers: KeyModifiers,
    last_pointer: Option<(f32, f32)>,
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
        let rcx = unsafe {
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
                    self.app.on_event(event);
                }
                if moved.needs_redraw {
                    rcx.window.request_redraw();
                }
            }

            WindowEvent::CursorLeft { .. } => {
                self.last_pointer = None;
                for event in rcx.runner_mut().pointer_left() {
                    self.app.on_event(event);
                }
                rcx.window.request_redraw();
            }

            WindowEvent::HoveredFile(path) => {
                let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                for event in rcx.runner_mut().file_hovered(path, lx, ly) {
                    self.app.on_event(event);
                }
                rcx.window.request_redraw();
            }

            WindowEvent::HoveredFileCancelled => {
                for event in rcx.runner_mut().file_hover_cancelled() {
                    self.app.on_event(event);
                }
                rcx.window.request_redraw();
            }

            WindowEvent::DroppedFile(path) => {
                let (lx, ly) = self.last_pointer.unwrap_or((0.0, 0.0));
                for event in rcx.runner_mut().file_dropped(path, lx, ly) {
                    self.app.on_event(event);
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
                        for event in rcx
                            .runner_mut()
                            .pointer_down(Pointer::mouse(lx, ly, button))
                        {
                            self.app.on_event(event);
                        }
                        rcx.window.request_redraw();
                    }
                    ElementState::Released => {
                        for event in rcx.runner_mut().pointer_up(Pointer::mouse(lx, ly, button)) {
                            self.app.on_event(event);
                        }
                        rcx.window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let Some((lx, ly)) = self.last_pointer else {
                    return;
                };
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 50.0,
                    MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / scale,
                };
                if rcx.runner_mut().pointer_wheel(lx, ly, dy) {
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
                if let Some(key) = map_key(&key_event.logical_key) {
                    for ev in rcx
                        .runner_mut()
                        .key_down(key, self.modifiers, key_event.repeat)
                    {
                        self.app.on_event(ev);
                    }
                }
                if let Some(text) = &key_event.text
                    && let Some(ev) = rcx.runner_mut().text_input(text.to_string())
                {
                    self.app.on_event(ev);
                }
                rcx.window.request_redraw();
            }

            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if let Some(ev) = rcx.runner_mut().text_input(text) {
                    self.app.on_event(ev);
                }
                rcx.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                for event in rcx.runner_mut().poll_input(web_time::Instant::now()) {
                    self.app.on_event(event);
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
                let mut tree = self.app.build(&cx);
                let palette = theme.palette().clone();
                let runner = rcx.runner_mut();
                runner.set_theme(theme);
                runner.set_hotkeys(self.app.hotkeys());
                runner.set_selection(self.app.selection());
                runner.push_toasts(self.app.drain_toasts());
                runner.push_focus_requests(self.app.drain_focus_requests());
                runner.push_scroll_requests(self.app.drain_scroll_requests());
                let prepare = runner.prepare(&mut tree, viewport, scale_factor);

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
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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

impl RenderContext {
    fn runner(&self) -> &Runner {
        self.runner.as_ref().expect("runner")
    }

    fn runner_mut(&mut self) -> &mut Runner {
        self.runner.as_mut().expect("runner")
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
            self.runner_mut()
                .render(command_buffer, target, LoadOp::Clear(clear))
                .expect("aetna-ash render");
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
    theme: aetna_core::Theme,
    shaders: Vec<aetna_core::AppShader>,
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
    let context = AshContext::new(device.clone(), allocator.clone(), queue_family_index);
    let mut runner = Runner::new(context, TargetInfo::new(swapchain_format))?;
    runner.set_theme(theme);
    runner.set_surface_size(swapchain_extent.width, swapchain_extent.height);
    for shader in shaders {
        runner.register_shader_with(
            shader.name,
            shader.wgsl,
            shader.samples_backdrop,
            shader.samples_time,
        )?;
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
        allocator: Some(allocator),
        recreate_swapchain: false,
        resize_debounce: None,
    })
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
    let features = aetna_ash::required_device_features();
    let mut features13 = aetna_ash::required_vulkan_13_features();
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

fn clear_color(palette: &aetna_core::Palette) -> [f32; 4] {
    let c = palette.background;
    [
        srgb_to_linear(c.r as f32 / 255.0),
        srgb_to_linear(c.g as f32 / 255.0),
        srgb_to_linear(c.b as f32 / 255.0),
        c.a as f32 / 255.0,
    ]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[allow(dead_code)]
fn _color(c: Color) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}
