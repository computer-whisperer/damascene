//! Headless: render the inline-runs fixture to PNG via wgpu.
//!
//! End-to-end smoke test for the inline-runs path: the Inlines paragraph flows
//! through `DrawOp::AttributedText` → `TextRecorder::record_runs` →
//! `GlyphAtlas::shape_and_rasterize_runs`, which shapes all runs in
//! one cosmic-text buffer so wrapping decisions cross run boundaries.
//!
//! Asserts that the rendered PNG contains the destructive-token red
//! pixels (proving the per-run color baking made it through the
//! shader path).
//!
//! Usage: `cargo run -p damascene-wgpu --example render_inline_runs`

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner};

fn fixture() -> El {
    column([
        h2("Inline runs"),
        text_runs([
            text("Damascene's attributed-text path lets you compose runs with "),
            text("bold").bold(),
            text(", "),
            text("italic").italic(),
            text(", "),
            text("colored").color(tokens::DESTRUCTIVE),
            text(", and "),
            text("inline code").code(),
            text(" segments inside one wrapping paragraph."),
            hard_break(),
            text("Hard breaks act like ").muted(),
            text("<br>").code(),
            text(" — they end the current line without breaking out of the run.").muted(),
        ])
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
    .width(Size::Fixed(640.0))
    .height(Size::Hug)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 640;
    let logical_height: u32 = 240;
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
        label: Some("damascene_wgpu::example::inline_runs::device"),
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
        label: Some("damascene_wgpu::example::inline_runs::target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("damascene_wgpu::example::inline_runs::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);
    let tree = fixture();
    renderer.prepare(&device, &queue, tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::inline_runs::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("damascene_wgpu::example::inline_runs::pass"),
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

    // Probe: count "red-ish" pixels (R dominant, G/B low). The
    // destructive token is rgba(245, 95, 110) — the only pixels that
    // should match are the "colored" run's glyphs.
    let mut red_pixels = 0usize;
    let mut chunks = unpadded.chunks_exact(4);
    for chunk in chunks.by_ref() {
        let (r, g, b) = (chunk[0] as i32, chunk[1] as i32, chunk[2] as i32);
        if r > 180 && g < 130 && b < 140 && r - g > 60 {
            red_pixels += 1;
        }
    }
    println!("destructive-red pixels: {red_pixels}");

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("inline_runs.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    if red_pixels < 50 {
        return Err(format!(
            "expected destructive-token red pixels for the 'colored' run; got only {red_pixels} \
             — per-run color may not have made it through to the shader"
        )
        .into());
    }
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
