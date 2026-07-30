//! Headless end-to-end render test for painted vector gradients on
//! vulkano (issues #140/#141). Twin of `damascene-wgpu`'s
//! `vector_gradient_render`.
//!
//! Renders fullscreen gradient-filled rects through the real tess
//! pipeline (fragment-stage ramp evaluation — gradient slot in
//! `meta[2]`, param uniform + ramp texture at descriptor set 1) and
//! pixel-checks against the reference values from the issue reports:
//!
//! - #141: a two-stop `#754A75 → #F7A983` gradient must interpolate in
//!   sRGB space — midpoint `rgb(182, 122, 124)`, not the linear-space
//!   `rgb(196, 132, 124)`.
//! - #140: interior stops of a five-stop gradient must actually render
//!   (per-vertex sampling only ever hit the endpoint stops).
//!
//! Skips cleanly (passes) when no Vulkan device is available, so CI
//! without a GPU doesn't fail.

use std::sync::Arc;

use damascene_core::prelude::*;
use damascene_core::tree::vector;
use damascene_core::vector::parse_svg_asset;
use damascene_vulkano::Runner;
use vulkano::{
    VulkanLibrary,
    buffer::{Buffer, BufferCreateInfo, BufferUsage},
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo,
        allocator::StandardCommandBufferAllocator,
    },
    device::{Device, DeviceCreateInfo, Queue, QueueCreateInfo, QueueFlags},
    format::Format,
    image::{Image, ImageCreateInfo, ImageType, ImageUsage, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    render_pass::{Framebuffer, FramebufferCreateInfo},
    sync::{self, GpuFuture},
};

const SIZE: u32 = 160;
const FORMAT: Format = Format::R8G8B8A8_SRGB;

struct Gpu {
    device: Arc<Device>,
    queue: Arc<Queue>,
    memory_alloc: Arc<StandardMemoryAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
}

fn headless_gpu() -> Option<Gpu> {
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
    let queue = queues.next()?;
    let memory_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
    let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));
    Some(Gpu {
        device,
        queue,
        memory_alloc,
        cmd_alloc,
    })
}

/// Render `tree` to an `SIZE×SIZE` offscreen sRGB target and return the
/// RGBA8 pixels (tightly packed, `SIZE * 4` bytes per row).
fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: El) -> Vec<u8> {
    let target = Image::new(
        gpu.memory_alloc.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: FORMAT,
            extent: [SIZE, SIZE, 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .expect("target image");
    let view = ImageView::new_default(target.clone()).expect("target view");
    let framebuffer = Framebuffer::new(
        runner.render_pass().clone(),
        FramebufferCreateInfo {
            attachments: vec![view],
            ..Default::default()
        },
    )
    .expect("framebuffer");

    let viewport = Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32);
    runner.prepare(tree, viewport, 1.0);

    let readback = Buffer::new_slice::<u8>(
        gpu.memory_alloc.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_RANDOM_ACCESS,
            ..Default::default()
        },
        (SIZE * SIZE * 4) as u64,
    )
    .expect("readback buffer");

    let mut builder = AutoCommandBufferBuilder::primary(
        gpu.cmd_alloc.clone(),
        gpu.queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .expect("command buffer builder");
    runner.render(
        &mut builder,
        framebuffer,
        target.clone(),
        [0.0, 0.0, 0.0, 1.0],
    );
    builder
        .copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
            target,
            readback.clone(),
        ))
        .expect("copy image to buffer");
    let command_buffer = builder.build().expect("build command buffer");

    sync::now(gpu.device.clone())
        .then_execute(gpu.queue.clone(), command_buffer)
        .expect("execute")
        .then_signal_fence_and_flush()
        .expect("flush")
        .wait(None)
        .expect("wait");

    readback.read().expect("read pixels").to_vec()
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * SIZE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn assert_close(got: [u8; 3], want: [u8; 3], tolerance: i16, context: &str) {
    for (g, w) in got.iter().zip(&want) {
        assert!(
            (*g as i16 - *w as i16).abs() <= tolerance,
            "{context}: got {got:?}, want {want:?} (±{tolerance})"
        );
    }
}

/// Fullscreen vertical gradient rect: viewBox y spans the gradient
/// axis, so a pixel row's `t` is `(row + 0.5) / SIZE`.
fn gradient_tree(stops: &str) -> El {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 200">
            <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="200" gradientUnits="userSpaceOnUse">
                {stops}
            </linearGradient></defs>
            <rect width="100" height="200" fill="url(#g)"/></svg>"##
    );
    vector(parse_svg_asset(&svg).unwrap())
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32))
}

/// Issue #141's verification table: sRGB-space interpolation of
/// `#754A75 → #F7A983` at 1/4, 1/2, 3/4 — the values every browser and
/// the reference Vulkan renderer produce. Linear-space interpolation
/// (the old behavior) is ~14 counts brighter through the midrange and
/// fails the tolerance.
#[test]
fn two_stop_gradient_interpolates_in_srgb() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("vector_gradient_render(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#754A75"/>
            <stop offset="1" stop-color="#F7A983"/>"##,
    );
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    let x = SIZE / 2;

    // Endpoints (sampled a few rows in to stay clear of AA fringes).
    assert_close(pixel(&pixels, x, 2), [0x75, 0x4A, 0x75], 4, "t≈0");
    assert_close(pixel(&pixels, x, SIZE - 3), [0xF7, 0xA9, 0x83], 4, "t≈1");
    // The #141 table. ±4 covers f16 ramp texels, 8-bit target
    // quantization, and the half-row t offset.
    assert_close(pixel(&pixels, x, SIZE / 4), [150, 98, 121], 4, "t=0.25");
    assert_close(pixel(&pixels, x, SIZE / 2), [182, 122, 124], 4, "t=0.5");
    assert_close(
        pixel(&pixels, x, SIZE * 3 / 4),
        [214, 145, 128],
        4,
        "t=0.75",
    );
}

/// Issue #140: interior stops must render. Per-vertex sampling only
/// ever evaluated the gradient at the rect's corners, so the three
/// interior stops vanished into an endpoint-to-endpoint lerp.
#[test]
fn five_stop_gradient_renders_interior_stops() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("vector_gradient_render(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#754A75"/>
            <stop offset="0.25" stop-color="#372960"/>
            <stop offset="0.5" stop-color="#A33861"/>
            <stop offset="0.75" stop-color="#D1956C"/>
            <stop offset="1" stop-color="#F7A983"/>"##,
    );
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    let x = SIZE / 2;

    // Each interior stop's authored colour appears at its offset. The
    // old endpoint-lerp behavior puts e.g. lerp(#754A75, #F7A983, 0.25)
    // ≈ (150, 98, 121) at t=0.25 — nowhere near #372960.
    assert_close(pixel(&pixels, x, SIZE / 4), [0x37, 0x29, 0x60], 5, "t=0.25");
    assert_close(pixel(&pixels, x, SIZE / 2), [0xA3, 0x38, 0x61], 5, "t=0.5");
    assert_close(
        pixel(&pixels, x, SIZE * 3 / 4),
        [0xD1, 0x95, 0x6C],
        5,
        "t=0.75",
    );
}
