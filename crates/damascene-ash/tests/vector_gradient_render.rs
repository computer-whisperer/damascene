//! Headless end-to-end render test for painted vector gradients on ash
//! (issues #140/#141). Twin of `damascene-wgpu/tests/vector_gradient_render.rs`.
//!
//! Renders fullscreen gradient-filled rects through the real tess
//! pipeline (fragment-stage ramp evaluation: gradient slot in vertex
//! meta[2], gradient param uniform + Rgba16Float ramp texture at
//! descriptor set 1) and pixel-checks against the reference values from
//! the issue reports:
//!
//! - #141: a two-stop `#754A75 → #F7A983` gradient must interpolate in
//!   sRGB space — midpoint `rgb(182, 122, 124)`, not the linear-space
//!   `rgb(196, 132, 124)`.
//! - #140: interior stops of a five-stop gradient must actually render
//!   (per-vertex sampling only ever hit the endpoint stops).
//!
//! Skips cleanly (passes) when no suitable Vulkan device is available.

use std::sync::{Arc, Mutex};

use ash::vk;
use damascene_ash::{AshContext, AshRenderTarget, LoadOp, Runner, TargetInfo};
use damascene_core::prelude::*;
use damascene_core::tree::vector;
use damascene_core::vector::parse_svg_asset;
use damascene_core::{AnimationMode, Rect};
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{
    Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};

const SIZE: u32 = 160;
// `*_SRGB` target: the readback bytes are sRGB-encoded and directly
// comparable to the reference values from the issue tables.
const FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;

/// Holds the Vulkan objects alive for the test. We deliberately don't destroy
/// the device/instance/pool: raw ash handles have no `Drop`, and gpu-allocator's
/// `Allocator` needs the device alive when *it* drops. So we free every
/// allocation (see `render_to_pixels`) — which is all gpu-allocator checks —
/// and let the device/instance leak; the process exits right after.
#[allow(dead_code)]
struct Gpu {
    device: Arc<ash::Device>,
    allocator: Arc<Mutex<Allocator>>,
    queue: vk::Queue,
    queue_family_index: u32,
    command_pool: vk::CommandPool,
    physical_device: vk::PhysicalDevice,
    instance: ash::Instance,
    entry: ash::Entry,
}

fn headless_gpu() -> Option<Gpu> {
    let entry = unsafe { ash::Entry::load() }.ok()?;
    let app = std::ffi::CString::new("damascene-ash-test").ok()?;
    let app_info = vk::ApplicationInfo::default()
        .application_name(&app)
        .api_version(vk::API_VERSION_1_3);
    let instance_info = vk::InstanceCreateInfo::default().application_info(&app_info);
    let instance = unsafe { entry.create_instance(&instance_info, None) }.ok()?;

    let physical_devices = unsafe { instance.enumerate_physical_devices() }.ok()?;
    let (physical_device, queue_family_index) = physical_devices.into_iter().find_map(|pd| {
        let props = unsafe { instance.get_physical_device_queue_family_properties(pd) };
        let qf = props
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))?;
        // Require Vulkan 1.3 dynamic rendering.
        let mut features13 = vk::PhysicalDeviceVulkan13Features::default();
        let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut features13);
        unsafe { instance.get_physical_device_features2(pd, &mut features) };
        (features13.dynamic_rendering == vk::TRUE).then_some((pd, qf as u32))
    })?;

    let priorities = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family_index)
        .queue_priorities(&priorities);
    let queue_infos = [queue_info];
    let features = damascene_ash::required_device_features();
    let mut features13 = damascene_ash::required_vulkan_13_features();
    let device_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_infos)
        .enabled_features(&features)
        .push_next(&mut features13);
    let device = unsafe { instance.create_device(physical_device, &device_info, None) }.ok()?;
    let device = Arc::new(device);
    let queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    let allocator = Allocator::new(&AllocatorCreateDesc {
        instance: instance.clone(),
        device: (*device).clone(),
        physical_device,
        debug_settings: Default::default(),
        buffer_device_address: false,
        allocation_sizes: Default::default(),
    })
    .ok()?;
    let allocator = Arc::new(Mutex::new(allocator));

    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(queue_family_index);
    let command_pool = unsafe { device.create_command_pool(&pool_info, None) }.ok()?;

    Some(Gpu {
        device,
        allocator,
        queue,
        queue_family_index,
        command_pool,
        physical_device,
        instance,
        entry,
    })
}

/// Allocate a GpuOnly image + default colour view.
fn make_image(gpu: &Gpu, usage: vk::ImageUsageFlags) -> (vk::Image, vk::ImageView, Allocation) {
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width: SIZE,
            height: SIZE,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { gpu.device.create_image(&info, None) }.expect("create image");
    let req = unsafe { gpu.device.get_image_memory_requirements(image) };
    let alloc = gpu
        .allocator
        .lock()
        .unwrap()
        .allocate(&AllocationCreateDesc {
            name: "test_target",
            requirements: req,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })
        .expect("allocate image");
    unsafe {
        gpu.device
            .bind_image_memory(image, alloc.memory(), alloc.offset())
            .expect("bind image");
    }
    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(FORMAT)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
    let view = unsafe { gpu.device.create_image_view(&view_info, None) }.expect("create view");
    (image, view, alloc)
}

fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: El) -> Vec<u8> {
    let (image, view, image_alloc) = make_image(
        gpu,
        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
    );

    // Host-visible readback buffer.
    let buf_size = (SIZE * SIZE * 4) as vk::DeviceSize;
    let buf_info = vk::BufferCreateInfo::default()
        .size(buf_size)
        .usage(vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let readback = unsafe { gpu.device.create_buffer(&buf_info, None) }.expect("readback buffer");
    let req = unsafe { gpu.device.get_buffer_memory_requirements(readback) };
    let readback_alloc = gpu
        .allocator
        .lock()
        .unwrap()
        .allocate(&AllocationCreateDesc {
            name: "test_readback",
            requirements: req,
            location: MemoryLocation::GpuToCpu,
            linear: true,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })
        .expect("allocate readback");
    unsafe {
        gpu.device
            .bind_buffer_memory(readback, readback_alloc.memory(), readback_alloc.offset())
            .expect("bind readback");
    }

    let viewport = Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32);
    runner.set_surface_size(SIZE, SIZE);
    runner.prepare(tree, viewport, 1.0);

    // Record + submit a one-shot command buffer.
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(gpu.command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmd = unsafe { gpu.device.allocate_command_buffers(&alloc_info) }.expect("alloc cmd")[0];
    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe {
        gpu.device.begin_command_buffer(cmd, &begin).expect("begin");
        let target = AshRenderTarget {
            image,
            view,
            format: FORMAT,
            extent: vk::Extent2D {
                width: SIZE,
                height: SIZE,
            },
            sample_count: vk::SampleCountFlags::TYPE_1,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        };
        runner
            .render(cmd, target, LoadOp::Clear([0.0, 0.0, 0.0, 1.0]))
            .expect("render");
        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: SIZE,
                height: SIZE,
                depth: 1,
            });
        gpu.device.cmd_copy_image_to_buffer(
            cmd,
            image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            readback,
            &[region],
        );
        gpu.device.end_command_buffer(cmd).expect("end");

        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().command_buffers(&cmds);
        let fence = gpu
            .device
            .create_fence(&vk::FenceCreateInfo::default(), None)
            .expect("fence");
        gpu.device
            .queue_submit(gpu.queue, &[submit], fence)
            .expect("submit");
        gpu.device
            .wait_for_fences(&[fence], true, u64::MAX)
            .expect("wait");
        gpu.device.destroy_fence(fence, None);
        gpu.device.free_command_buffers(gpu.command_pool, &cmds);
    }

    let pixels = readback_alloc
        .mapped_slice()
        .expect("readback mapped")
        .to_vec();

    // Free everything (gpu-allocator complains about leaked allocations).
    unsafe {
        gpu.device.destroy_image_view(view, None);
        gpu.device.destroy_image(image, None);
        gpu.device.destroy_buffer(readback, None);
    }
    let mut allocator = gpu.allocator.lock().unwrap();
    allocator.free(image_alloc).expect("free image");
    allocator.free(readback_alloc).expect("free readback");

    pixels
}

fn make_runner(gpu: &Gpu) -> Runner {
    let max_image_dimension_2d = unsafe {
        gpu.instance
            .get_physical_device_properties(gpu.physical_device)
    }
    .limits
    .max_image_dimension2_d;
    let context = AshContext::new(
        gpu.device.clone(),
        gpu.allocator.clone(),
        gpu.queue_family_index,
        max_image_dimension_2d,
    );
    let mut runner = Runner::new(context, TargetInfo::new(FORMAT)).expect("runner");
    runner.set_animation_mode(AnimationMode::Settled);
    runner
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
        eprintln!("vector_gradient_render(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#754A75"/>
            <stop offset="1" stop-color="#F7A983"/>"##,
    );
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    drop(runner);

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
        eprintln!("vector_gradient_render(ash): no Vulkan 1.3 device, skipping");
        return;
    };
    let mut runner = make_runner(&gpu);

    let tree = gradient_tree(
        r##"<stop offset="0" stop-color="#754A75"/>
            <stop offset="0.25" stop-color="#372960"/>
            <stop offset="0.5" stop-color="#A33861"/>
            <stop offset="0.75" stop-color="#D1956C"/>
            <stop offset="1" stop-color="#F7A983"/>"##,
    );
    let pixels = render_to_pixels(&gpu, &mut runner, tree);
    drop(runner);

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
