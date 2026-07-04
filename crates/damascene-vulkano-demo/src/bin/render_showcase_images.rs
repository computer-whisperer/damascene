//! Headless Vulkano render for the Showcase `Media` page.
//!
//! A focused fixture for the raster-image pipeline: renders the
//! gradient grid + tinted avatar row + ImageFit modes through
//! `damascene-vulkano`, then dumps a PNG so the wgpu and vulkano backends
//! can be A/B'd by eye for `stock::image`.
//!
//! Run: `cargo run -p damascene-vulkano-demo --bin render_showcase_images`
//! Writes: `crates/damascene-vulkano-demo/out/showcase_images.vulkano.png`

use std::sync::Arc;

use damascene_core::{AnimationMode, App, BuildCx, Rect};
use damascene_fixtures::Showcase;
use damascene_fixtures::showcase::Section;
use damascene_vulkano::Runner;
use vulkano::{
    VulkanLibrary,
    buffer::{Buffer, BufferCreateInfo, BufferUsage},
    command_buffer::{
        AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo,
        allocator::StandardCommandBufferAllocator,
    },
    device::{Device, DeviceCreateInfo, QueueCreateInfo, QueueFlags},
    format::Format,
    image::{Image, ImageCreateInfo, ImageType, ImageUsage, SampleCount, view::ImageView},
    instance::{Instance, InstanceCreateFlags, InstanceCreateInfo},
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    render_pass::{Framebuffer, FramebufferCreateInfo},
    sync::{self, GpuFuture},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 900;
    let logical_height: u32 = 640;
    let scale_factor: f32 = 2.0;
    let width = (logical_width as f32 * scale_factor) as u32;
    let height = (logical_height as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, logical_width as f32, logical_height as f32);

    let library = VulkanLibrary::new()?;
    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            ..Default::default()
        },
    )?;
    let (physical_device, queue_family_index) = instance
        .enumerate_physical_devices()?
        .filter_map(|p| {
            p.queue_family_properties()
                .iter()
                .position(|q| q.queue_flags.intersects(QueueFlags::GRAPHICS))
                .map(|i| (p, i as u32))
        })
        .next()
        .ok_or("no compatible Vulkan graphics device")?;
    println!("device: {}", physical_device.properties().device_name);

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
    )?;
    let queue = queues.next().expect("created one graphics queue");
    let memory_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
    let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
        device.clone(),
        Default::default(),
    ));

    let format = Format::R8G8B8A8_SRGB;
    let target = Image::new(
        memory_alloc.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format,
            extent: [width, height, 1],
            // TRANSFER_DST is required under MSAA: vulkano's
            // `single_pass_renderpass!` macro picks `TransferDstOptimal`
            // for the resolve attachment's layout.
            usage: ImageUsage::COLOR_ATTACHMENT
                | ImageUsage::TRANSFER_SRC
                | ImageUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )?;
    let view = ImageView::new_default(target.clone())?;

    let sample_count: u32 = 4;
    let mut renderer =
        Runner::with_sample_count(device.clone(), queue.clone(), format, sample_count);
    renderer.set_surface_size(width, height);
    renderer.set_animation_mode(AnimationMode::Settled);

    let msaa_image = Image::new(
        memory_alloc.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format,
            extent: [width, height, 1],
            samples: SampleCount::try_from(sample_count).expect("valid MSAA sample count"),
            usage: ImageUsage::COLOR_ATTACHMENT,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )?;
    let msaa_view = ImageView::new_default(msaa_image)?;

    let framebuffer = Framebuffer::new(
        renderer.render_pass().clone(),
        FramebufferCreateInfo {
            attachments: vec![msaa_view, view],
            ..Default::default()
        },
    )?;

    let mut app = Showcase::with_section(Section::Media);
    app.before_build();
    let theme = app.theme();
    let cx = BuildCx::new(&theme);
    let tree = app.build(&cx);
    renderer.prepare(tree, viewport, scale_factor);

    let readback = Buffer::new_slice::<u8>(
        memory_alloc,
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_RANDOM_ACCESS,
            ..Default::default()
        },
        (width * height * 4) as u64,
    )?;

    let mut builder = AutoCommandBufferBuilder::primary(
        cmd_alloc,
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )?;
    renderer.render(&mut builder, framebuffer, target.clone(), clear_color());
    builder.copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
        target,
        readback.clone(),
    ))?;
    let command_buffer = builder.build()?;

    sync::now(device)
        .then_execute(queue.clone(), command_buffer)?
        .then_signal_fence_and_flush()?
        .wait(None)?;

    let pixels = readback.read()?;
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("showcase_images.vulkano.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    println!("wrote {}", out.display());

    Ok(())
}

fn clear_color() -> [f32; 4] {
    // Route through the paint-stream color machinery so the cleared pixel
    // matches a painted `tokens::BACKGROUND` fill exactly (issue #45).
    damascene_core::paint::rgba_f32(damascene_core::tokens::BACKGROUND)
}
