//! Headless render of the Showcase Media page with the animated
//! surface hooks driven manually. Diagnostic: verifies the surface()
//! widget actually composites the app texture in the same code path
//! the windowed showcase uses, without depending on the host's
//! redraw cadence or interactive redraws.
//!
//! Usage: `cargo run -p damascene-tools --bin render_showcase_media`
//! Writes: `tools/out/showcase_media.wgpu.png`

use std::f32::consts::TAU;
use std::sync::Arc;

use damascene_core::prelude::*;
use damascene_fixtures::{Showcase, showcase::Section};
use damascene_wgpu::{MsaaTarget, Runner, app_texture};

const TEX_SIZE: u32 = 96;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 900;
    let logical_height: u32 = 2100;
    let scale_factor: f32 = 1.0;
    let width = (logical_width as f32 * scale_factor) as u32;
    let height = (logical_height as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, logical_width as f32, logical_height as f32);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no compatible adapter ({e})"))?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::tools::showcase_media::device"),
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
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("showcase_media::target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // Replicate the showcase's gpu_setup: allocate the app texture
    // and hand it to Showcase::set_animated_surface.
    let app_texture_inner = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("showcase_media::animated_surface"),
        size: wgpu::Extent3d {
            width: TEX_SIZE,
            height: TEX_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }));
    let mut showcase = Showcase::with_section(Section::Media);
    showcase.set_animated_surface(Some(app_texture(app_texture_inner.clone())));

    // Replicate before_paint: write a non-trivial frame so the
    // surface samples real pixels rather than the implicit zeros.
    let mut pixels = vec![0u8; (TEX_SIZE * TEX_SIZE * 4) as usize];
    write_frame(&mut pixels, 0.6); // arbitrary t with the ring well into its rotation
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &app_texture_inner,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * TEX_SIZE),
            rows_per_image: Some(TEX_SIZE),
        },
        wgpu::Extent3d {
            width: TEX_SIZE,
            height: TEX_SIZE,
            depth_or_array_layers: 1,
        },
    );

    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("showcase_media::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);
    renderer.set_theme(showcase.theme());

    let theme = showcase.theme();
    let cx = damascene_core::BuildCx::new(&theme).with_ui_state(renderer.ui_state());
    let tree = showcase.build(&cx);
    renderer.prepare(&device, &queue, tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("showcase_media::encoder"),
    });
    {
        let bg = bg_color(&theme);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("showcase_media::pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa.view,
                resolve_target: Some(&target_view),
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
            texture: &target,
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

    let padded = buffer_slice.get_mapped_range().unwrap();
    let mut unpadded = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + unpadded_bytes_per_row as usize;
        unpadded.extend_from_slice(&padded[start..end]);
    }
    drop(padded);
    readback_buf.unmap();

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("showcase_media.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    Ok(())
}

fn write_frame(pixels: &mut [u8], t: f32) {
    let w = TEX_SIZE as f32;
    let cx = w * 0.5;
    let cy = w * 0.5;
    let r_outer = w * 0.45;
    let r_inner = w * 0.18;

    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let r = (dx * dx + dy * dy).sqrt();
            let theta = dy.atan2(dx);
            let hue = (theta / TAU + t * 0.25).rem_euclid(1.0);
            let (rr, gg, bb) = hsv_to_rgb(hue, 0.9, 1.0);

            let band_t = ((r - r_inner) / (r_outer - r_inner)).clamp(0.0, 1.0);
            let cov = (1.0 - (band_t * 2.0 - 1.0).abs()).max(0.0);
            let cov = cov * cov * (3.0 - 2.0 * cov);

            let a = (cov * 255.0).round() as u8;
            let i = ((y * TEX_SIZE + x) * 4) as usize;
            pixels[i] = ((rr * cov) * 255.0).round() as u8;
            pixels[i + 1] = ((gg * cov) * 255.0).round() as u8;
            pixels[i + 2] = ((bb * cov) * 255.0).round() as u8;
            pixels[i + 3] = a;
        }
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match (i as i32) % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn bg_color(theme: &Theme) -> wgpu::Color {
    let c = theme.palette().resolve(damascene_core::tokens::BACKGROUND);
    wgpu::Color {
        r: srgb_to_linear(c.r as f64 / 255.0),
        g: srgb_to_linear(c.g as f64 / 255.0),
        b: srgb_to_linear(c.b as f64 / 255.0),
        a: c.a as f64 / 255.0,
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
