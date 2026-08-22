//! Headless: render the scrollbar fixture to PNG via wgpu.
//!
//! End-to-end smoke test for the default-on scrollbar thumb on
//! `scroll()` and `virtual_list()`. Asserts that the rendered PNG
//! contains thumb-coloured pixels (the `SCROLLBAR_THUMB_FILL` token,
//! which is light grey at low alpha) — proving the layout-pass
//! `thumb_rects` made it through draw_ops to the rounded_rect
//! pipeline.
//!
//! Usage: `cargo run -p damascene-wgpu --example render_scrollbar`

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner};

fn list_rows() -> Vec<El> {
    (0..40)
        .map(|i| {
            row([
                text(format!("{i:02}.")).mono().muted(),
                text(format!("scrollable list item {i}")),
            ])
            .gap(tokens::SPACE_2)
            .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
            .height(Size::Fixed(28.0))
            .align(Align::Center)
        })
        .collect()
}

fn fixture() -> El {
    column([
        h2("Scrollbar"),
        text("scroll() and virtual_list() show a draggable thumb by default.").muted(),
        row([
            column([
                text("scroll()").bold(),
                scroll(list_rows())
                    .height(Size::Fixed(240.0))
                    .padding(tokens::SPACE_2)
                    .stroke(tokens::BORDER)
                    .stroke_width(1.0)
                    .radius(tokens::RADIUS_MD),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
            column([
                text("virtual_list(200, 28)").bold(),
                virtual_list(200, 28.0, |i| {
                    row([
                        text(format!("{i:03}")).mono().muted(),
                        text(format!("row {i}")),
                    ])
                    .gap(tokens::SPACE_2)
                    .padding(Sides::xy(tokens::SPACE_2, tokens::SPACE_1))
                    .height(Size::Fixed(28.0))
                    .align(Align::Center)
                })
                .height(Size::Fixed(240.0))
                .padding(tokens::SPACE_2)
                .stroke(tokens::BORDER)
                .stroke_width(1.0)
                .radius(tokens::RADIUS_MD),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_4)
        .width(Size::Fill(1.0)),
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
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no compatible adapter ({e})"))?;
    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::example::scrollbar::device"),
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
        label: Some("damascene_wgpu::example::scrollbar::target"),
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
        label: Some("damascene_wgpu::example::scrollbar::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);
    let tree = fixture();
    // Two-pass render: first lay out so thumb_tracks exist, then
    // park the pointer at the centre of the right-hand virtual_list's
    // track so its thumb paints in its expanded "hover" state. The
    // left list stays idle, giving us a side-by-side visual diff in
    // one PNG.
    renderer.prepare(&device, &queue, tree.clone(), viewport, scale_factor);
    let right_track = renderer
        .ui_state()
        .scrollbar_tracks()
        .map(|(_id, rect)| *rect)
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .expect("at least one scrollable should have a track");
    renderer.pointer_moved(Pointer::moving(
        right_track.x + right_track.w * 0.5,
        right_track.y + right_track.h * 0.5,
    ));
    renderer.prepare(&device, &queue, tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::scrollbar::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("damascene_wgpu::example::scrollbar::pass"),
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

    let padded = buffer_slice.get_mapped_range().unwrap();
    let mut unpadded = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + unpadded_bytes_per_row as usize;
        unpadded.extend_from_slice(&padded[start..end]);
    }
    drop(padded);
    readback_buf.unmap();

    // Probe: count thumb-tinted pixels. The token is roughly
    // (148, 160, 176, 130) — neutral grey-blue at low alpha. After
    // premultiplied composite over BACKGROUND (~14, 16, 22) we expect
    // values around ~80 / ~85 / ~95.
    let mut thumb_pixels = 0usize;
    let mut chunks = unpadded.as_chunks::<4>().0.iter();
    for chunk in chunks.by_ref() {
        let (r, g, b) = (chunk[0] as i32, chunk[1] as i32, chunk[2] as i32);
        if (60..120).contains(&r) && (65..130).contains(&g) && (75..145).contains(&b) {
            thumb_pixels += 1;
        }
    }
    println!("thumb-tint pixels: {thumb_pixels}");

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("scrollbar.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    if thumb_pixels < 200 {
        return Err(format!(
            "expected thumb-tinted pixels for scroll() / virtual_list(); got only {thumb_pixels} \
             — scrollbar may not have made it through draw_ops"
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
