//! Headless: render a fixture to a PNG via wgpu.
//!
//! Same paint path as the windowed demo, but no surface and no event
//! loop — useful for CI / agents / sub-sessions that can't open a
//! window. Produces a ground-truth PNG comparable to the SVG fallback;
//! when they diverge it tells us where the SVG approximation is lying.
//!
//! Usage: `cargo run -p damascene-wgpu --example render_png`
//! Writes: `crates/damascene-wgpu/out/settings.wgpu.png`

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner};

fn settings() -> El {
    column([
        h1("Settings"),
        titled_card(
            "Account",
            [
                row([text("Email"), spacer(), text("user@example.com").muted()]),
                row([
                    text("Two-factor authentication"),
                    spacer(),
                    badge("Enabled").success(),
                ]),
                row([
                    text("Recovery codes"),
                    spacer(),
                    button("Generate").secondary(),
                ]),
            ],
        ),
        titled_card(
            "Appearance",
            [
                row([text("Theme"), spacer(), button("Dark").secondary()]),
                row([text("Compact mode"), spacer(), badge("Off").muted()]),
                row([text("Font size"), spacer(), text("14")]),
            ],
        ),
        titled_card(
            "Danger zone",
            [row([
                column([
                    text("Delete account").bold(),
                    text("Permanently remove your account and all data.")
                        .muted()
                        .small(),
                ])
                .gap(tokens::SPACE_1)
                .align(Align::Start)
                .width(Size::Hug),
                spacer(),
                button("Delete").destructive(),
            ])],
        ),
        row([spacer(), button("Cancel").ghost(), button("Save").primary()]),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 720;
    let logical_height: u32 = 760;
    let scale_factor: f32 = 2.0;
    let width = (logical_width as f32 * scale_factor) as u32;
    let height = (logical_height as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, logical_width as f32, logical_height as f32);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|e| {
        format!(
            "{} ({e})",
            "no compatible adapter (try installing vulkan / mesa drivers)"
        )
    })?;

    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::example::headless::device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let sample_count = 4;
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let msaa = MsaaTarget::new(&device, format, extent, sample_count);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::example::headless::target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // copy_texture_to_buffer requires bytes_per_row to be 256-aligned.
    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("damascene_wgpu::example::headless::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);
    let mut tree = settings();
    renderer.prepare(&device, &queue, tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::headless::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("damascene_wgpu::example::headless::pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa.view,
                resolve_target: Some(&view),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(bg),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    // Map and read back.
    let buffer_slice = readback_buf.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
    buffer_slice.map_async(wgpu::MapMode::Read, move |r| {
        sender.send(r).ok();
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    receiver.recv()??;

    let padded = buffer_slice.get_mapped_range();
    let mut unpadded = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + unpadded_bytes_per_row as usize;
        unpadded.extend_from_slice(&padded[start..end]);
    }
    drop(padded);
    readback_buf.unmap();

    // Write the PNG.
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("settings.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    Ok(())
}

fn bg_color() -> wgpu::Color {
    // `LoadOp::Clear` takes working-space values; route the token through
    // the same conversion the paint stream uses so the cleared pixel
    // matches a painted `tokens::BACKGROUND` fill exactly.
    let [r, g, b, a] = damascene_core::paint::rgba_f32(damascene_core::tokens::BACKGROUND);
    wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    }
}
