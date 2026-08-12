//! One-shot validation renders for the status-token contrast rework
//! (`*_TINT_FOREGROUND`). Renders a controlled swatch matrix — tinted
//! badges, text-only status treatments, a destructive form message —
//! under every stock theme, so before/after PNGs show exactly the
//! elements the token change repaints.
//!
//! Usage:
//!
//! ```text
//! cargo run -p damascene-tools --bin render_tint_validation -- <out_dir> <prefix>
//! ```

use damascene_core::prelude::*;
use damascene_core::{AnimationMode, Rect as CoreRect};
use damascene_wgpu::{MsaaTarget, Runner};

fn swatch_tree() -> El {
    let badges = row([
        badge("Info").info(),
        badge("Success").success(),
        badge("Warning").warning(),
        badge("Destructive").destructive(),
    ])
    .gap(tokens::SPACE_3);

    let status_text = column([
        text("Deploy failed — 2 checks did not pass").destructive(),
        text("3 dependencies are a major version behind").warning(),
        text("All 1,847 tests passed").success(),
        text("Nightly build available").info(),
    ])
    .gap(tokens::SPACE_2);

    let form = form_item([
        form_label("Email"),
        form_control(text_input("email", "chris@@example", &Selection::default())),
        form_message("Enter a valid email address"),
    ]);

    column([
        h3("Tinted badges").label(),
        badges,
        h3("Status text").label(),
        status_text,
        h3("Validation message").label(),
        form,
    ])
    .gap(tokens::SPACE_4)
    .padding(tokens::SPACE_6)
    .align(Align::Start)
}

fn themes() -> Vec<(&'static str, Theme)> {
    vec![
        ("damascene-dark", Theme::damascene_dark()),
        ("damascene-light", Theme::damascene_light()),
        ("radix-slate-blue-dark", Theme::radix_slate_blue_dark()),
        ("radix-slate-blue-light", Theme::radix_slate_blue_light()),
        ("radix-sand-amber-dark", Theme::radix_sand_amber_dark()),
        ("radix-sand-amber-light", Theme::radix_sand_amber_light()),
        ("radix-mauve-violet-dark", Theme::radix_mauve_violet_dark()),
        (
            "radix-mauve-violet-light",
            Theme::radix_mauve_violet_light(),
        ),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out_dir = std::path::PathBuf::from(args.next().expect("usage: <out_dir> <prefix>"));
    let prefix = args.next().expect("usage: <out_dir> <prefix>");
    std::fs::create_dir_all(&out_dir)?;

    let logical = (560u32, 420u32);
    let scale_factor: f32 = 2.0;
    let width = (logical.0 as f32 * scale_factor) as u32;
    let height = (logical.1 as f32 * scale_factor) as u32;
    let viewport = CoreRect::new(0.0, 0.0, logical.0 as f32, logical.1 as f32);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no compatible adapter ({e})"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tint_validation::device"),
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
    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;

    for (name, theme) in themes() {
        let msaa = MsaaTarget::new(&device, format, extent, sample_count);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tint_validation::target"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tint_validation::readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
        renderer.set_animation_mode(AnimationMode::Settled);
        renderer.set_theme(theme.clone());
        renderer.prepare(&device, &queue, swatch_tree(), viewport, scale_factor);

        let bg = theme.resolve(tokens::BACKGROUND);
        let clear = wgpu::Color {
            r: srgb_to_linear(bg.r as f64),
            g: srgb_to_linear(bg.g as f64),
            b: srgb_to_linear(bg.b as f64),
            a: 1.0,
        };

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("tint_validation::encoder"),
        });
        renderer.render(
            &device,
            &mut encoder,
            &target,
            &target_view,
            Some(&msaa.view),
            wgpu::LoadOp::Clear(clear),
        );
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
            extent,
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

        let out = out_dir.join(format!("{prefix}_{name}.png"));
        let file = std::fs::File::create(&out)?;
        let writer = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(writer, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&unpadded)?;
        println!("wrote {}", out.display());
    }

    Ok(())
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
