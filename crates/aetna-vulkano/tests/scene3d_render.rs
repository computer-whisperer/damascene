//! Headless end-to-end render test for the Scene3D pipeline on vulkano.
//!
//! Twin of `aetna-wgpu`'s `scene3d_render`. Builds a real vulkano device,
//! renders a `chart3d` (lit mesh + point scatter + a line + grid) into an
//! offscreen target, reads the pixels back, and asserts the scene actually
//! composited content into its rect. This is the only place the scene WGSL is
//! compiled through naga on the vulkano side, and the whole offscreen →
//! resolve → composite path runs against the GPU.
//!
//! Skips cleanly (passes) when no Vulkan device is available, so CI without a
//! GPU doesn't fail.

use std::sync::Arc;

use aetna_core::prelude::*;
use aetna_core::scene::glam::Vec3;
use aetna_core::scene::{
    LineData, LineSegment, LinesHandle, MeshData, MeshHandle, MeshVertex, PointData, PointStyle,
    PointsHandle, ScenePoint, SceneSpec,
};
use aetna_core::{AnimationMode, Rect};
use aetna_vulkano::Runner;
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
            enabled_features: aetna_vulkano::required_device_features(),
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

/// Render `tree` to an `SIZE×SIZE` offscreen target on a single-sample main
/// pass and return the RGBA8 pixels. The scene applies its own MSAA in its
/// offscreen pass, so the main pass stays single-sample (one attachment).
fn render_to_pixels(gpu: &Gpu, runner: &mut Runner, tree: &mut El) -> Vec<u8> {
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

/// Count pixels whose RGB rises clearly above the black clear colour.
fn count_lit(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|px| px[0] > 16 || px[1] > 16 || px[2] > 16)
        .count()
}

/// UV sphere with smooth (position-direction) normals, CCW outward winding —
/// the same geometry the `scene3d` example uses.
fn uv_sphere(radius: f32, rings: u32, sectors: u32) -> MeshData {
    use std::f32::consts::{PI, TAU};
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=rings {
        let theta = i as f32 / rings as f32 * PI;
        let (st, ct) = theta.sin_cos();
        for j in 0..=sectors {
            let phi = j as f32 / sectors as f32 * TAU;
            let (sp, cp) = phi.sin_cos();
            let n = Vec3::new(st * cp, ct, st * sp);
            vertices.push(MeshVertex {
                position: n * radius,
                normal: n,
            });
        }
    }
    let stride = sectors + 1;
    for i in 0..rings {
        for j in 0..sectors {
            let a = i * stride + j;
            let b = a + stride;
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    MeshData {
        vertices,
        indices: Some(indices),
    }
}

#[test]
fn scene3d_composites_visible_content() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("scene3d_render(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

    let mesh = MeshHandle::new(uv_sphere(0.9, 24, 32));
    let points = PointsHandle::new(PointData {
        points: vec![
            ScenePoint {
                position: Vec3::new(1.6, 0.0, 0.0),
                color: [1.0, 0.2, 0.2, 1.0],
            },
            ScenePoint {
                position: Vec3::new(0.0, 1.6, 0.0),
                color: [0.2, 1.0, 0.2, 1.0],
            },
            ScenePoint {
                position: Vec3::new(0.0, 0.0, 1.6),
                color: [0.3, 0.4, 1.0, 1.0],
            },
        ],
    });
    let lines = LinesHandle::new(LineData {
        segments: vec![LineSegment {
            start: Vec3::new(-1.6, -1.6, 0.0),
            end: Vec3::new(1.6, 1.6, 0.0),
            color: [1.0, 1.0, 1.0, 1.0],
        }],
    });

    let spec = SceneSpec::new()
        .mesh(mesh)
        .points_styled(
            points,
            PointStyle {
                size: 14.0,
                ..Default::default()
            },
        )
        .lines(lines);

    let mut tree = chart3d(spec);
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("scene3d_render(vulkano): {lit}/{total} non-black pixels");
    assert!(
        lit > total / 100,
        "scene composited almost nothing ({lit}/{total} lit) — offscreen render or composite is broken"
    );
}

#[test]
fn uv_sphere_winds_outward() {
    let Some(gpu) = headless_gpu() else {
        eprintln!("uv_sphere_winds_outward(vulkano): no Vulkan device, skipping");
        return;
    };
    let mut runner = Runner::new(gpu.device.clone(), gpu.queue.clone(), FORMAT);
    runner.set_surface_size(SIZE, SIZE);
    runner.set_animation_mode(AnimationMode::Settled);

    let mesh = MeshHandle::new(uv_sphere(1.0, 24, 32));
    let mut tree = chart3d(SceneSpec::new().mesh(mesh).no_grid());
    let pixels = render_to_pixels(&gpu, &mut runner, &mut tree);
    let lit = count_lit(&pixels);
    let total = (SIZE * SIZE) as usize;
    eprintln!("uv_sphere_winds_outward(vulkano): {lit}/{total} lit");
    // A framed sphere fills a big fraction of the view; inverted winding
    // (front faces culled) collapses this to near-zero.
    assert!(
        lit > total / 6,
        "sphere barely rendered ({lit} px) — winding likely inverted (front faces culled)"
    );
}
