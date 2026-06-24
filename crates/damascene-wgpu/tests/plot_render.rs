//! Headless end-to-end render test for the 2D `plot` widget.
//!
//! Builds a real wgpu device, renders a `plot` (a line series + gridlines +
//! tick labels) into an offscreen target, reads the pixels back, and asserts
//! the plot actually composited line content into its data rect. The plot's
//! data layer reuses the Scene3D offscreen → resolve → composite path under
//! an orthographic camera (see `docs/PLOT2D_PLAN.md`), so this exercises that
//! reuse against the GPU.
//!
//! Skips cleanly (passes) when no adapter is available, so CI without a GPU
//! doesn't fail.

use damascene_core::plot::{PlotSpec, Sample, Scale, SeriesHandle, line};
use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const SIZE: u32 = 200;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("plot_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

fn render_to_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    runner: &mut Runner,
    tree: &mut El,
) -> Vec<u8> {
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("plot_test_target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let unpadded = SIZE * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("plot_test_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("plot_test"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &target_view,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(SIZE),
            },
        },
        wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for row in 0..SIZE as usize {
        let start = row * bytes_per_row as usize;
        out.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    readback.unmap();
    out
}

#[test]
fn plot_composites_line_into_data_rect() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping plot_render test");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    // A rising diagonal line crosses much of the data rect, so a correct
    // render lights up many non-black pixels along it.
    let series = SeriesHandle::new(vec![
        Sample::new(0.0, 0.0),
        Sample::new(1.0, 1.0),
        Sample::new(2.0, 2.0),
        Sample::new(3.0, 3.0),
    ]);
    let spec = PlotSpec::new()
        .x(Scale::linear())
        .y(Scale::linear())
        .add_mark(line(&series).width(3.0));
    let mut tree = plot(spec).key("p");

    let px = render_to_pixels(&device, &queue, &mut runner, &mut tree);

    // Count blue-dominant pixels: the default series line is a bright
    // palette blue (~99,164,255), distinct from the faint gray gridlines and
    // muted-gray tick labels. Finding many proves the *data layer* (not just
    // the chrome) composited.
    let mut blue = 0usize;
    for p in px.chunks_exact(4) {
        let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
        if b > 120 && b > r + 30 && b > g + 20 {
            blue += 1;
        }
    }
    assert!(
        blue > 100,
        "plot should composite a visible blue line into its data rect, got {blue} blue px"
    );
}
