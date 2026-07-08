//! Manual visual-inspection dump — run with:
//!   cargo test -p damascene-wgpu --test dump_text_sample -- --ignored --nocapture
//! Writes DAMASCENE_TEXT_DUMP (or ./text_sample.png) with light/dark
//! text samples at UI sizes and weights for eyeballing gamma/snapping
//! changes. Ignored by default: it produces a file, not an assertion.

use damascene_core::prelude::*;
use damascene_core::tree::FontWeight;
use damascene_wgpu::Runner;

const W: u32 = 640;
const H: u32 = 480;
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
        label: Some("dump_text_sample"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

fn samples(fg: Color, muted: Color) -> El {
    column([
        text("Sphinx of black quartz, judge my vow — 13px regular")
            .font_size(13.0)
            .text_color(fg),
        text("Sphinx of black quartz, judge my vow — 14px regular")
            .font_size(14.0)
            .text_color(fg),
        text("Buttons and labels use 14px medium weight")
            .font_size(14.0)
            .font_weight(FontWeight::Medium)
            .text_color(fg),
        text("Card titles use 16px semibold")
            .font_size(16.0)
            .font_weight(FontWeight::Semibold)
            .text_color(fg),
        text("Headings 24px semibold — Grumpy wizards")
            .heading()
            .text_color(fg),
        text("Muted secondary copy at 14px sits quieter")
            .font_size(14.0)
            .text_color(muted),
        text("An underlined link sits clear of descenders: gyp jq")
            .font_size(14.0)
            .underline()
            .text_color(fg),
    ])
    .gap(8.0)
    .padding(Sides::all(16.0))
}

#[test]
#[ignore = "manual visual dump, writes a PNG"]
fn dump_text_sample() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("dump_text_sample: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(W, H);

    let light = samples(Color::srgb_u8(9, 9, 11), Color::srgb_u8(113, 113, 122))
        .fill(Color::srgb_u8(255, 255, 255))
        .width(Size::Fixed(W as f32))
        .height(Size::Fixed((H / 2) as f32));
    let dark = samples(Color::srgb_u8(250, 250, 250), Color::srgb_u8(161, 161, 170))
        .fill(Color::srgb_u8(9, 9, 11))
        .width(Size::Fixed(W as f32))
        .height(Size::Fixed((H / 2) as f32));
    let tree = column([light, dark]);

    runner.prepare(
        &device,
        &queue,
        tree,
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dump_text_sample_target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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
    let unpadded = W * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dump_text_sample_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dump_text_sample"),
    });
    runner.render(
        &device,
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
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
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for row in 0..H {
        let off = (row * bytes_per_row) as usize;
        pixels.extend_from_slice(&data[off..off + (W * 4) as usize]);
    }
    drop(data);
    readback.unmap();

    let out = std::env::var("DAMASCENE_TEXT_DUMP").unwrap_or_else(|_| "text_sample.png".into());
    let writer = std::io::BufWriter::new(std::fs::File::create(&out).expect("create png"));
    let mut enc = png::Encoder::new(writer, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
    eprintln!("dump_text_sample: wrote {out}");
}
