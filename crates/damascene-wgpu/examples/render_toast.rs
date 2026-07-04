//! Headless: render the toast fixture to PNG via wgpu.
//!
//! Seeds four toasts (one of each non-default level) directly via the
//! `Runner::push_toasts` host accessor, then renders one frame. The
//! synthesized `toast_stack` floats over the viewport at the bottom-
//! right corner regardless of the user's tree shape — `prepare()`
//! re-runs synthesis each frame so the rendered tree mirrors the
//! visible state.
//!
//! Probes for level-coloured pixels to confirm each accent bar
//! reached the rounded_rect pipeline.
//!
//! Usage: `cargo run -p damascene-wgpu --example render_toast`

use std::time::Duration;

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner};

fn fixture() -> El {
    column([
        h2("Toasts"),
        paragraph(
            "Apps queue toasts via App::drain_toasts. The runtime stamps \
             each with a TTL, stacks them at the bottom-right corner, \
             and dismisses them on click or auto-expiry.",
        )
        .muted(),
        row([
            button("Save changes").key("save"),
            button("Trigger error").key("err"),
            button("Show info").key("info"),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_7)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 720;
    let logical_height: u32 = 360;
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
    .map_err(|e| format!("no compatible adapter ({e})"))?;
    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::example::toast::device"),
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
        label: Some("damascene_wgpu::example::toast::target"),
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
        label: Some("damascene_wgpu::example::toast::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);

    // Seed four toasts at long TTL so the headless snapshot captures
    // them all. In a live app these come from `App::drain_toasts`.
    let long_ttl = Duration::from_secs(60);
    renderer.push_toasts(vec![
        ToastSpec::success("Settings saved").with_ttl(long_ttl),
        ToastSpec::warning("Battery low — connect charger").with_ttl(long_ttl),
        ToastSpec::error("Failed to reach update server").with_ttl(long_ttl),
        ToastSpec::info("New version available").with_ttl(long_ttl),
    ]);

    let mut tree = fixture();
    renderer.prepare(&device, &queue, tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::toast::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("damascene_wgpu::example::toast::pass"),
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

    // Coarse probes for each accent bar token. These are tiny strips
    // (3px wide) on each card, so a permissive count is fine —
    // anything > 30 means the accent reached the GPU.
    let mut success_pixels = 0usize;
    let mut warning_pixels = 0usize;
    let mut error_pixels = 0usize;
    let mut info_pixels = 0usize;
    let mut chunks = unpadded.chunks_exact(4);
    for chunk in chunks.by_ref() {
        let (r, g, b) = (chunk[0] as i32, chunk[1] as i32, chunk[2] as i32);
        // SUCCESS = (80, 210, 140) green-dominant.
        if r < 130 && g > 150 && b < 180 && g - r > 50 {
            success_pixels += 1;
        }
        // WARNING = (245, 190, 85) orange/yellow.
        if r > 200 && g > 140 && b < 120 && r - b > 80 {
            warning_pixels += 1;
        }
        // DESTRUCTIVE = (245, 95, 110) red/pink.
        if r > 200 && g < 130 && b < 140 && r - g > 80 {
            error_pixels += 1;
        }
        // INFO = (92, 170, 255) blue-dominant.
        if r < 140 && g > 130 && b > 200 && b - r > 60 {
            info_pixels += 1;
        }
    }
    println!("success-bar pixels: {success_pixels}");
    println!("warning-bar pixels: {warning_pixels}");
    println!("error-bar pixels:   {error_pixels}");
    println!("info-bar pixels:    {info_pixels}");

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("toast.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    for (label, count) in [
        ("success", success_pixels),
        ("warning", warning_pixels),
        ("error", error_pixels),
        ("info", info_pixels),
    ] {
        if count < 30 {
            return Err(format!(
                "expected {label}-bar accent pixels in toast cards; got only {count}",
            )
            .into());
        }
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
