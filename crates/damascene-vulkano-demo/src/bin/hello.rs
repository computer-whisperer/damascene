//! Minimal vulkano + winit clear-color frame loop.
//!
//! No `damascene-vulkano::Runner` involvement; the only goal is to confirm
//! vulkano + winit + the host's Vulkan loader bring up a window that
//! paints `tokens::BACKGROUND` every frame. Useful as a platform-compat
//! smoke test independent of the rest of the backend.
//!
//! Structure mirrors the native winit host shape — same
//! `ApplicationHandler` skeleton, same `gfx: Option<…>` lazy-init
//! pattern — so the diff between backend harnesses stays small.

use std::sync::Arc;

use vulkano::{
    VulkanLibrary,
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, RenderPassBeginInfo,
        allocator::StandardCommandBufferAllocator,
    },
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, Queue, QueueCreateInfo, QueueFlags,
        physical::PhysicalDeviceType,
    },
    format::NumericFormat,
    image::{Image, ImageUsage, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    pipeline::graphics::viewport::Viewport,
    render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass},
    swapchain::{
        Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo, acquire_next_image,
    },
    sync::{self, GpuFuture},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        instance,
        rcx: None,
    };
    event_loop.run_app(&mut host)?;
    Ok(())
}

struct Host {
    instance: Arc<Instance>,
    rcx: Option<RenderContext>,
}

struct RenderContext {
    window: Arc<Window>,
    device: Arc<Device>,
    queue: Arc<Queue>,
    swapchain: Arc<Swapchain>,
    render_pass: Arc<RenderPass>,
    framebuffers: Vec<Arc<Framebuffer>>,
    viewport: Viewport,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
    recreate_swapchain: bool,
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.rcx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Damascene — vulkano hello")
            .with_inner_size(PhysicalSize::new(900, 640));
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

        let (swapchain, images) = {
            let surface_caps = device
                .physical_device()
                .surface_capabilities(&surface, Default::default())
                .expect("surface caps");
            let formats = device
                .physical_device()
                .surface_formats(&surface, Default::default())
                .expect("surface formats");
            // Match the wgpu host path: prefer an sRGB swapchain format
            // so the `clear_color()` linear-space values land correctly.
            // Without this, the first available format is often a UNORM
            // one and `srgb_to_linear` double-darkens BACKGROUND to near-zero.
            let image_format = formats
                .iter()
                .copied()
                .find(|(f, _)| f.numeric_format_color() == Some(NumericFormat::SRGB))
                .unwrap_or(formats[0])
                .0;
            eprintln!("damascene-vulkano hello: swapchain format = {image_format:?}");
            Swapchain::new(
                device.clone(),
                surface,
                SwapchainCreateInfo {
                    min_image_count: surface_caps.min_image_count.max(2),
                    image_format,
                    image_extent: window.inner_size().into(),
                    image_usage: ImageUsage::COLOR_ATTACHMENT,
                    composite_alpha: surface_caps
                        .supported_composite_alpha
                        .into_iter()
                        .next()
                        .unwrap(),
                    ..Default::default()
                },
            )
            .expect("create swapchain")
        };

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: swapchain.image_format(),
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            },
        )
        .expect("create render pass");

        let viewport = Viewport {
            offset: [0.0, 0.0],
            extent: [
                window.inner_size().width as f32,
                window.inner_size().height as f32,
            ],
            depth_range: 0.0..=1.0,
        };
        let framebuffers = build_framebuffers(&images, &render_pass);

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
            render_pass,
            framebuffers,
            viewport,
            cmd_alloc,
            previous_frame_end,
            recreate_swapchain: false,
        });
        self.rcx.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(rcx) = self.rcx.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(_) => {
                rcx.recreate_swapchain = true;
                rcx.window.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                let extent: [u32; 2] = rcx.window.inner_size().into();
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
                    rcx.framebuffers = build_framebuffers(&new_images, &rcx.render_pass);
                    rcx.viewport.extent = [extent[0] as f32, extent[1] as f32];
                    rcx.recreate_swapchain = false;
                }

                let (image_index, suboptimal, acquire_future) =
                    match acquire_next_image(rcx.swapchain.clone(), None) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("acquire_next_image: {e}");
                            rcx.recreate_swapchain = true;
                            return;
                        }
                    };
                if suboptimal {
                    rcx.recreate_swapchain = true;
                }

                let mut builder = AutoCommandBufferBuilder::primary(
                    rcx.cmd_alloc.clone(),
                    rcx.queue.queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .expect("command builder");

                builder
                    .begin_render_pass(
                        RenderPassBeginInfo {
                            clear_values: vec![Some(clear_color().into())],
                            ..RenderPassBeginInfo::framebuffer(
                                rcx.framebuffers[image_index as usize].clone(),
                            )
                        },
                        Default::default(),
                    )
                    .expect("begin render pass");
                builder
                    .end_render_pass(Default::default())
                    .expect("end pass");
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
                    Ok(future) => rcx.previous_frame_end = Some(future.boxed()),
                    Err(e) => {
                        eprintln!("flush: {e}");
                        rcx.recreate_swapchain = true;
                        rcx.previous_frame_end = Some(sync::now(rcx.device.clone()).boxed());
                    }
                }
            }
            _ => {}
        }
    }
}

fn build_framebuffers(
    images: &[Arc<Image>],
    render_pass: &Arc<RenderPass>,
) -> Vec<Arc<Framebuffer>> {
    images
        .iter()
        .map(|image| {
            let view = ImageView::new_default(image.clone()).expect("image view");
            Framebuffer::new(
                render_pass.clone(),
                FramebufferCreateInfo {
                    attachments: vec![view],
                    ..Default::default()
                },
            )
            .expect("framebuffer")
        })
        .collect()
}

/// Same logic as the wgpu host clear color — the swapchain format we
/// pick is sRGB, but vulkano's `ClearValue::Float` is taken as linear.
/// Convert so the cleared pixel matches `tokens::BACKGROUND`.
fn clear_color() -> [f32; 4] {
    let c = damascene_core::tokens::BACKGROUND;
    [
        srgb_to_linear(c.r / 255.0),
        srgb_to_linear(c.g / 255.0),
        srgb_to_linear(c.b / 255.0),
        c.a / 255.0,
    ]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
