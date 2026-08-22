//! Headless end-to-end render test for the per-image HDR remaster.
//!
//! Renders a linear scRGB luminance ramp (0 → 4× reference white)
//! through `stock::image` onto an `Rgba16Float` target and reads the
//! float pixels back, checking each `DynamicRangeLimit` policy against
//! the output headroom set via `Runner::set_output_luminance`:
//! content above the resolved limit must roll off (BT.2390) — bounded
//! by the limit but still graded, not clipped flat — while content
//! within the limit passes through untouched.
//!
//! Skips cleanly (passes) when no adapter is available, so CI without a
//! GPU doesn't fail — but it runs for real wherever a Vulkan/Metal/DX
//! adapter exists.

use damascene_core::color::ColorSpace;
use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const SIZE: u32 = 160;
/// Extended-range float target — the swapchain format the remaster
/// exists for. Values above 1.0 survive the render pass and the
/// readback, so the assertions can see real HDR light.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

const RAMP_W: u32 = 256;
const RAMP_PEAK: f32 = 4.0;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    let backend = format!("{:?}", adapter.get_info().backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("image_remaster_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue, backend))
}

/// Linear scRGB ramp, 0 → `RAMP_PEAK`× reference white left to right
/// (matches the showcase's HDR ramp).
fn ramp() -> Image {
    let mut px = Vec::with_capacity((RAMP_W * 4) as usize);
    for x in 0..RAMP_W {
        let v = x as f32 / (RAMP_W - 1) as f32 * RAMP_PEAK;
        px.extend([v, v, v, 1.0]);
    }
    Image::from_rgba_f32_in(ColorSpace::SCRGB_LINEAR, RAMP_W, 1, px)
}

fn ramp_tree(limit: DynamicRangeLimit) -> El {
    image(ramp())
        .dynamic_range_limit(limit)
        .image_fit(ImageFit::Fill)
        .width(Size::Fixed(SIZE as f32))
        .height(Size::Fixed(SIZE as f32))
}

/// Render `tree` and return the red channel of the target's middle row,
/// decoded from f16 — the ramp is grayscale, so one channel carries it.
fn render_row(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    runner: &mut Runner,
    tree: El,
) -> Vec<f32> {
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("image_remaster_target"),
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
    // 8 bytes per Rgba16Float pixel; pitch padded to 256.
    let unpadded = SIZE * 8;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("image_remaster_readback"),
        size: (bytes_per_row * SIZE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("image_remaster"),
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
    let data = slice.get_mapped_range().unwrap();

    let row = SIZE as usize / 2;
    let start = row * bytes_per_row as usize;
    let out = data[start..start + unpadded as usize]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|px| half::f16::from_ne_bytes([px[0], px[1]]).to_f32())
        .collect();
    drop(data);
    readback.unmap();
    out
}

fn max_of(row: &[f32]) -> f32 {
    row.iter().copied().fold(0.0, f32::max)
}

#[test]
fn remaster_follows_dynamic_range_limit() {
    let Some((device, queue, backend)) = headless_device() else {
        eprintln!("image_remaster: no GPU adapter, skipping");
        return;
    };
    eprintln!("image_remaster: using {backend} adapter");

    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(SIZE, SIZE);

    // Output with 2× headroom (e.g. a 406-nit panel at 203-nit
    // reference). The ramp peaks at 4× — twice what the panel can show.
    runner.set_output_luminance(2.0, 203.0);

    // NoLimit fits the panel: bounded by the 2× headroom, but rolled
    // off — the top of the ramp keeps its gradation instead of
    // clamping flat.
    let row = render_row(
        &device,
        &queue,
        &mut runner,
        ramp_tree(DynamicRangeLimit::NoLimit),
    );
    let peak = max_of(&row);
    assert!(
        peak <= 2.02,
        "no-limit on a 2× panel must stay within headroom, peaked at {peak}"
    );
    assert!(
        peak > 1.5,
        "no-limit on a 2× panel should still use the headroom, peaked at {peak}"
    );
    // Gradation preserved: input 3× (x = 3/4 width) maps strictly below
    // input 4× (right edge). A hard clamp would flatten both to 2.0.
    let (aa, bb) = (row[SIZE as usize * 3 / 4], row[SIZE as usize - 2]);
    assert!(
        bb > aa + 0.01,
        "roll-off must keep highlight gradation (got {aa} → {bb})"
    );

    // Standard tonemaps to SDR: bounded by 1.0, gradation preserved.
    let row = render_row(
        &device,
        &queue,
        &mut runner,
        ramp_tree(DynamicRangeLimit::Standard),
    );
    let peak = max_of(&row);
    assert!(
        peak <= 1.02,
        "standard must tonemap to SDR, peaked at {peak}"
    );
    // Probe gradation at 2× vs 4× input — squeezing 4× into 1.0 leaves
    // the spline's top end nearly tangent to the target, so the 3×→4×
    // step lands within f16 noise; the 2×→4× step stays measurable.
    let (aa, bb) = (row[SIZE as usize / 2], row[SIZE as usize - 2]);
    assert!(
        bb > aa + 0.005,
        "standard roll-off must keep highlight gradation (got {aa} → {bb})"
    );

    // Unbounded output (no declared maximum): nothing to remaster
    // against — the ramp passes through to its full 4× peak.
    runner.set_output_luminance(f32::INFINITY, 203.0);
    let row = render_row(
        &device,
        &queue,
        &mut runner,
        ramp_tree(DynamicRangeLimit::NoLimit),
    );
    let peak = max_of(&row);
    assert!(
        peak > 3.5,
        "unbounded headroom must pass HDR through, peaked at {peak}"
    );

    // ConstrainedHigh on the unbounded output: capped at 2× regardless.
    let row = render_row(
        &device,
        &queue,
        &mut runner,
        ramp_tree(DynamicRangeLimit::ConstrainedHigh),
    );
    let peak = max_of(&row);
    assert!(
        peak <= 2.02,
        "constrained-high must cap at 2x, peaked at {peak}"
    );
}
