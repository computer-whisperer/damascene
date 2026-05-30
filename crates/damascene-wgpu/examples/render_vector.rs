//! Headless render of the `vector()` widget. Builds a small grid of
//! commit-graph-style merge curves with `PathBuilder`, hands each one
//! to `vector(asset)`, and renders to PNG so the MSDF-routed crisp
//! stroke output can be verified end-to-end.
//!
//! Usage: `cargo run -p damascene-wgpu --example render_vector`
//! Writes: `crates/damascene-wgpu/out/vector.wgpu.png`

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner};

const ROW_H: f32 = 28.0;
const LANE_W: f32 = 40.0;
const N_LANES: usize = 5;
const N_ROWS: usize = 8;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 720;
    let logical_height: u32 = 360;
    let scale_factor: f32 = 1.0;
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

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::example::vector::device"),
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
        label: Some("vector::target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vector::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);

    let mut tree = fixture();
    renderer.prepare(&device, &queue, &mut tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("vector::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vector::pass"),
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

    let padded = buffer_slice.get_mapped_range();
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
    let out = out_dir.join("vector.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    Ok(())
}

fn fixture() -> El {
    column([
        h2("vector() — programmatic paths through the icon MSDF atlas"),
        text(
            "Each curve is built each frame with PathBuilder and handed to vector(...). \
             Identical (lane_delta, row_span) pairs hash to one MSDF atlas slot.",
        )
        .muted()
        .small(),
        merge_curves_grid(),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_3)
    .align(Align::Stretch)
}

fn merge_curves_grid() -> El {
    // A small "graph": four merge curves at varying lane deltas.
    // Each is a separate vector() El sized to its own bounding box.
    stack([
        // Backdrop: lane lines for visual context.
        El::default()
            .fill(tokens::MUTED.with_alpha_u8(60))
            .radius(tokens::RADIUS_MD)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
        // Vertical lane guides via thin solid quads.
        column((0..N_LANES).map(|_| spacer().height(Size::Fill(1.0))))
            .axis(Axis::Row)
            .gap(LANE_W - 1.0)
            .padding(Sides::xy(LANE_W * 0.5, 0.0))
            .align(Align::Stretch),
        // The merge curves themselves, absolutely positioned via `translate`.
        merge_curve(0, 2, 4, lane_color(2)).translate(LANE_W * 0.5, ROW_H * 0.5),
        merge_curve(1, 3, 5, lane_color(3)).translate(LANE_W * 1.5, ROW_H * 0.5),
        merge_curve(0, 4, 7, lane_color(4)).translate(LANE_W * 0.5, ROW_H * 0.5),
        merge_curve(2, 0, 3, lane_color(0)).translate(LANE_W * 2.5, ROW_H * 0.5),
        // A solid filled glyph to demonstrate fill (not just stroke).
        diamond_glyph(lane_color(1)).translate(LANE_W * 4.0, ROW_H * 1.5),
    ])
    .width(Size::Fixed(LANE_W * N_LANES as f32))
    .height(Size::Fixed(ROW_H * N_ROWS as f32))
}

/// One merge connector: vertical lane to `start_lane` over `row_span`
/// rows, then a smooth Bezier curl into `end_lane` at the bottom row.
fn merge_curve(start_lane: i32, end_lane: i32, row_span: u32, color: Color) -> El {
    let dx = (end_lane - start_lane) as f32 * LANE_W;
    let dy = row_span as f32 * ROW_H;
    let path = PathBuilder::new()
        .move_to(0.0, 0.0)
        .cubic_to(0.0, dy * 0.5, dx, dy * 0.5, dx, dy)
        .stroke_solid(color, 2.0)
        .stroke_line_cap(VectorLineCap::Round)
        .build();
    let bbox = [dx.min(0.0), 0.0, dx.abs().max(0.001), dy];
    let asset = VectorAsset::from_paths(bbox, vec![path]);
    vector(asset)
        .width(Size::Fixed(dx.abs().max(0.001)))
        .height(Size::Fixed(dy))
}

fn diamond_glyph(color: Color) -> El {
    let r = 8.0;
    let path = PathBuilder::new()
        .move_to(r, 0.0)
        .line_to(2.0 * r, r)
        .line_to(r, 2.0 * r)
        .line_to(0.0, r)
        .close()
        .fill_solid(color)
        .build();
    let asset = VectorAsset::from_paths([0.0, 0.0, 2.0 * r, 2.0 * r], vec![path]);
    vector(asset)
        .width(Size::Fixed(2.0 * r))
        .height(Size::Fixed(2.0 * r))
}

fn lane_color(lane: u8) -> Color {
    let palette = [
        Color::srgb_u8(244, 114, 182), // pink
        Color::srgb_u8(96, 165, 250),  // blue
        Color::srgb_u8(132, 204, 22),  // lime
        Color::srgb_u8(248, 113, 113), // red
        Color::srgb_u8(168, 162, 255), // violet
    ];
    palette[(lane as usize) % palette.len()]
}

fn bg_color() -> wgpu::Color {
    let c = damascene_core::tokens::BACKGROUND;
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
