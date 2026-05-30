//! Headless: render the shared text-quality matrix to PNG via vulkano.
//!
//! Twin of `damascene-wgpu`'s `render_text_quality` example. Shares the
//! same fixture so the two PNGs are directly comparable for backend
//! parity work.
//!
//! Run:
//!
//!     cargo run -p damascene-vulkano-demo --bin render_text_quality
//!     cargo run -p damascene-vulkano-demo --bin render_text_quality -- --scale=2
//!     cargo run -p damascene-vulkano-demo --bin render_text_quality -- --scale=1.5 --tag=before
//!
//! Writes `crates/damascene-vulkano-demo/out/text_quality{.tag}.{scale}x.vulkano.png`.

use std::sync::Arc;

use damascene_core::{AnimationMode, Rect};
use damascene_fixtures::text_quality::{LOGICAL_HEIGHT, LOGICAL_WIDTH, fixture};
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
    let mut scale_factor: f32 = 1.0;
    let mut tag: Option<String> = None;
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--scale=") {
            scale_factor = v.parse()?;
        } else if let Some(v) = arg.strip_prefix("--tag=") {
            tag = Some(v.to_string());
        } else {
            return Err(format!("unknown arg: {arg}").into());
        }
    }

    let width = (LOGICAL_WIDTH as f32 * scale_factor) as u32;
    let height = (LOGICAL_HEIGHT as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, LOGICAL_WIDTH as f32, LOGICAL_HEIGHT as f32);

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

    let mut tree = fixture();
    renderer.prepare(&mut tree, viewport, scale_factor);

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
    let suffix = match tag.as_deref() {
        Some(t) => format!(".{t}"),
        None => String::new(),
    };
    let scale_label = if scale_factor.fract() == 0.0 {
        format!("{}x", scale_factor as u32)
    } else {
        format!("{scale_factor}x")
    };
    let out = out_dir.join(format!("text_quality{suffix}.{scale_label}.vulkano.png"));
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
