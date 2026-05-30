//! HDR probe — side-by-side SDR white vs extended-range (HDR) white.
//!
//! Validates the *compliant* HDR-output path that `damascene-winit-wgpu`
//! cannot take: wgpu 29 exposes no swapchain-colorspace knob, but vulkano
//! does (`VK_EXT_swapchain_colorspace` → `ColorSpace`). This binary builds
//! a swapchain in an extended/HDR color space — preferring
//! `ExtendedSrgbLinear` (scRGB) on a float format — and fills the window
//! in two halves:
//!
//! - **left:** SDR reference white (`1.0` in scRGB-linear),
//! - **right:** a brighter-than-white value (`> 1.0`).
//!
//! On an HDR output through a conformant compositor the right half is
//! visibly brighter than the left. On SDR (or if the chain doesn't
//! engage) both clamp to the same white — the informative null result.
//!
//! It deliberately never touches `wp_color_management`: the WSI tags the
//! surface from the color space we pick at swapchain creation, and we are
//! a normal accelerated client. Run it on a known-good compositor
//! (KDE + HDR) to validate the approach before investing in a full
//! `damascene-winit-vulkano` host crate. niri is SDR-only, so it will show the
//! null result (both halves equal).
//!
//! It prints every `(format, color space)` pair the surface advertises
//! plus the pair it chose, so the terminal output is useful even without
//! eyeballing the window.

use std::{error::Error, ops::Range, sync::Arc};

use vulkano::{
    VulkanLibrary,
    command_buffer::{
        AutoCommandBufferBuilder, ClearAttachment, ClearRect, CommandBufferUsage,
        RenderPassBeginInfo, SubpassBeginInfo, SubpassContents, SubpassEndInfo,
        allocator::StandardCommandBufferAllocator,
    },
    device::{
        Device, DeviceCreateInfo, DeviceExtensions, QueueCreateInfo, QueueFlags,
        physical::PhysicalDeviceType,
    },
    format::{ClearColorValue, ClearValue, Format, NumericFormat},
    image::{Image, ImageUsage, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass},
    swapchain::{
        ColorSpace, Surface, Swapchain, SwapchainCreateInfo, SwapchainPresentInfo,
        acquire_next_image,
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

fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let library = VulkanLibrary::new()?;
    let mut enabled_extensions = Surface::required_extensions(&event_loop)?;
    // The extension that makes non-sRGB swapchain color spaces queryable.
    // Without it, `surface_formats` only ever reports `SrgbNonLinear`.
    if library.supported_extensions().ext_swapchain_colorspace {
        enabled_extensions.ext_swapchain_colorspace = true;
        eprintln!("hdr_probe: VK_EXT_swapchain_colorspace enabled");
    } else {
        eprintln!(
            "hdr_probe: WARNING — VK_EXT_swapchain_colorspace unsupported by the Vulkan loader; \
             HDR color spaces will not be offered (expect the SDR null result)"
        );
    }

    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            enabled_extensions,
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
    queue: Arc<vulkano::device::Queue>,
    swapchain: Arc<Swapchain>,
    render_pass: Arc<RenderPass>,
    framebuffers: Vec<Arc<Framebuffer>>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    previous_frame_end: Option<Box<dyn GpuFuture>>,
    recreate_swapchain: bool,
    /// Left-half clear (SDR reference white) and right-half clear (HDR).
    sdr: f32,
    hdr: f32,
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.rcx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("damascene HDR probe — left: SDR white | right: HDR white")
            .with_inner_size(PhysicalSize::new(1000, 500));
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
                queue_create_infos: vec![QueueCreateInfo {
                    queue_family_index,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
        .expect("create device");
        let queue = queues.next().unwrap();

        // Enumerate (format, color space) pairs and pick the best HDR one.
        let formats = device
            .physical_device()
            .surface_formats(&surface, Default::default())
            .expect("surface formats");
        eprintln!(
            "hdr_probe: surface advertises {} (format, colorspace) pairs:",
            formats.len()
        );
        for (f, cs) in &formats {
            eprintln!("hdr_probe:   {f:?} + {cs:?}");
        }
        let (image_format, image_color_space) = pick_hdr_format(&formats);
        let (sdr, hdr) = clear_values(image_color_space);
        eprintln!(
            "hdr_probe: chose {image_format:?} + {image_color_space:?} → left clears {sdr}, right clears {hdr}"
        );
        eprintln!(
            "hdr_probe: on an HDR display the RIGHT half should be visibly brighter than the left; \
             if they look identical, the HDR chain did not engage"
        );

        let surface_caps = device
            .physical_device()
            .surface_capabilities(&surface, Default::default())
            .expect("surface caps");
        let (swapchain, images) = Swapchain::new(
            device.clone(),
            surface,
            SwapchainCreateInfo {
                min_image_count: surface_caps.min_image_count.max(2),
                image_format,
                image_color_space,
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
        .expect("create swapchain");

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: image_format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
            },
            pass: { color: [color], depth_stencil: {} },
        )
        .expect("render pass");
        let framebuffers = build_framebuffers(&images, &render_pass);

        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            Default::default(),
        ));
        let previous_frame_end = Some(sync::now(device.clone()).boxed());

        self.rcx = Some(RenderContext {
            window: window.clone(),
            device,
            queue,
            swapchain,
            render_pass,
            framebuffers,
            cmd_alloc,
            previous_frame_end,
            recreate_swapchain: false,
            sdr,
            hdr,
        });
        window.request_redraw();
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
                    rcx.recreate_swapchain = false;
                }

                rcx.previous_frame_end.as_mut().unwrap().cleanup_finished();

                let (image_index, suboptimal, acquire_future) =
                    match acquire_next_image(rcx.swapchain.clone(), None) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("hdr_probe: acquire_next_image: {e}");
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

                let framebuffer = rcx.framebuffers[image_index as usize].clone();
                builder
                    .begin_render_pass(
                        RenderPassBeginInfo {
                            // Whole attachment → SDR reference white.
                            clear_values: vec![Some(ClearValue::Float([
                                rcx.sdr, rcx.sdr, rcx.sdr, 1.0,
                            ]))],
                            ..RenderPassBeginInfo::framebuffer(framebuffer)
                        },
                        SubpassBeginInfo {
                            contents: SubpassContents::Inline,
                            ..Default::default()
                        },
                    )
                    .expect("begin render pass")
                    // Right half → brighter-than-white (HDR) value.
                    // `clear_attachments` takes `SmallVec`s; build them from
                    // `Vec` via the inferred `From<Vec<_>>` conversion.
                    .clear_attachments(
                        vec![ClearAttachment::Color {
                            color_attachment: 0,
                            clear_value: ClearColorValue::Float([rcx.hdr, rcx.hdr, rcx.hdr, 1.0]),
                        }]
                        .into(),
                        vec![ClearRect {
                            offset: [extent[0] / 2, 0],
                            extent: [extent[0] - extent[0] / 2, extent[1]],
                            array_layers: Range { start: 0, end: 1 },
                        }]
                        .into(),
                    )
                    .expect("clear attachments")
                    .end_render_pass(SubpassEndInfo::default())
                    .expect("end render pass");
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
                        fence.wait(None).expect("frame fence wait");
                        rcx.previous_frame_end = Some(sync::now(rcx.device.clone()).boxed());
                    }
                    Err(e) => {
                        eprintln!("hdr_probe: flush: {e}");
                        rcx.recreate_swapchain = true;
                        rcx.previous_frame_end = Some(sync::now(rcx.device.clone()).boxed());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Pick the most useful `(format, color space)` pair for the probe:
/// scRGB-linear on a float format first (the easiest HDR target — linear,
/// extended-range, the compositor encodes), then any extended/HDR space,
/// then fall back to whatever is first (an sRGB null result).
fn pick_hdr_format(formats: &[(Format, ColorSpace)]) -> (Format, ColorSpace) {
    let is_float = |f: Format| f.numeric_format_color() == Some(NumericFormat::SFLOAT);
    formats
        .iter()
        .find(|(f, cs)| *cs == ColorSpace::ExtendedSrgbLinear && is_float(*f))
        .or_else(|| {
            formats
                .iter()
                .find(|(_, cs)| *cs == ColorSpace::ExtendedSrgbLinear)
        })
        .or_else(|| {
            formats
                .iter()
                .find(|(_, cs)| matches!(cs, ColorSpace::Bt2020Linear | ColorSpace::Hdr10St2084))
        })
        .copied()
        .unwrap_or(formats[0])
}

/// Left (SDR reference white) and right (HDR) clear values for the chosen
/// color space.
fn clear_values(cs: ColorSpace) -> (f32, f32) {
    match cs {
        // Linear, extended range: 1.0 = SDR reference white, > 1.0 = HDR.
        ColorSpace::ExtendedSrgbLinear | ColorSpace::Bt2020Linear => (1.0, 4.0),
        // PQ / ST 2084: absolute, nonlinear code values. ~0.58 ≈ 200 nits
        // (SDR-ish white), ~0.75 ≈ 1000 nits.
        ColorSpace::Hdr10St2084 => (0.58, 0.75),
        // sRGB or anything else: no HDR headroom — both halves clamp to
        // white, which is the "did not engage" result we want to see.
        _ => (1.0, 4.0),
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
