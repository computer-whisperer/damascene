//! Headless render of three surface() widgets, one per SurfaceAlpha
//! mode, sandwiched between widgets above and below.
//!
//! Proves end-to-end that:
//! - damascene_wgpu::app_texture wraps an app-allocated wgpu::Texture
//!   without requiring a parallel render pass;
//! - surface() participates in normal layout / z-order;
//! - widgets declared *before* the surface in the tree paint *under*
//!   it (the colored backdrops show through Premultiplied / Straight
//!   surfaces that have transparent regions; Opaque surfaces replace
//!   their pixels regardless);
//! - widgets declared *after* the surface paint *over* it (the
//!   "OVER" labels at the bottom of each cell sit on top of the
//!   composited texture).
//!
//! Usage: `cargo run -p damascene-wgpu --example render_surface`
//! Writes: `crates/damascene-wgpu/out/surface.wgpu.png`

use std::sync::Arc;

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner, app_texture};

const TEX_SIZE: u32 = 96;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 720;
    let logical_height: u32 = 580;
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

    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::example::surface::device"),
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
        label: Some("damascene_wgpu::example::surface::target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // Three app-owned textures: checkerboard with varying alpha. The
    // alpha pattern is what makes the three SurfaceAlpha modes visibly
    // different — Premultiplied and Straight let the backdrop show
    // through the transparent corners; Opaque replaces every pixel
    // even in the alpha=0 regions.
    let tex_premul = make_app_texture(&device, &queue, AlphaPattern::Premultiplied);
    let tex_straight = make_app_texture(&device, &queue, AlphaPattern::Straight);
    let tex_opaque = make_app_texture(&device, &queue, AlphaPattern::Premultiplied);
    let tex_fit_contain = make_app_texture(&device, &queue, AlphaPattern::Premultiplied);
    let tex_fit_cover = make_app_texture(&device, &queue, AlphaPattern::Premultiplied);
    let tex_fit_rotate = make_app_texture(&device, &queue, AlphaPattern::Premultiplied);

    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("damascene_wgpu::example::surface::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);

    let mut tree = fixture(
        tex_premul,
        tex_straight,
        tex_opaque,
        tex_fit_contain,
        tex_fit_cover,
        tex_fit_rotate,
    );
    renderer.prepare(&device, &queue, &mut tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::surface::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("damascene_wgpu::example::surface::pass"),
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
    let out = out_dir.join("surface.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    Ok(())
}

fn fixture(
    premul: AppTexture,
    straight: AppTexture,
    opaque_tex: AppTexture,
    fit_contain: AppTexture,
    fit_cover: AppTexture,
    fit_rotate: AppTexture,
) -> El {
    column([
        h2("surface() — app-owned textures composited into the paint stream"),
        text("declared above the surface (paints below in z)").muted(),
        row([
            cell(
                "Premultiplied",
                tokens::PRIMARY,
                premul,
                SurfaceAlpha::Premultiplied,
            ),
            cell(
                "Straight",
                tokens::SECONDARY,
                straight,
                SurfaceAlpha::Straight,
            ),
            cell("Opaque", tokens::ACCENT, opaque_tex, SurfaceAlpha::Opaque),
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Stretch),
        text("declared below the surface (paints above in z)").muted(),
        text("surface_fit + surface_transform").muted(),
        row([
            fit_cell(
                "Contain",
                tokens::PRIMARY,
                fit_contain,
                ImageFit::Contain,
                Affine2::IDENTITY,
            ),
            fit_cell(
                "Cover",
                tokens::SECONDARY,
                fit_cover,
                ImageFit::Cover,
                Affine2::IDENTITY,
            ),
            fit_cell(
                "Contain + rotate(0.4)",
                tokens::ACCENT,
                fit_rotate,
                ImageFit::Contain,
                Affine2::rotate(0.4),
            ),
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Stretch),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_3)
    .align(Align::Stretch)
}

fn fit_cell(
    label: &'static str,
    backdrop: Color,
    tex: AppTexture,
    fit: ImageFit,
    transform: Affine2,
) -> El {
    column([
        text(label).heading(),
        stack([
            El::default()
                .fill(backdrop)
                .radius(8.0)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            surface(tex)
                .surface_alpha(SurfaceAlpha::Premultiplied)
                .surface_fit(fit)
                .surface_transform(transform)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fixed(160.0)),
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
}

fn cell(label: &'static str, backdrop: Color, tex: AppTexture, alpha: SurfaceAlpha) -> El {
    // Stack: backdrop fill (under), surface (middle), "OVER" label (above).
    // Layout siblings declared after the surface paint over it;
    // Premultiplied / Straight surfaces let the backdrop show through
    // their transparent corners; Opaque replaces them.
    column([
        text(label).heading(),
        stack([
            // Under: a colored panel.
            El::default()
                .fill(backdrop)
                .radius(8.0)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            // Middle: the surface widget.
            surface(tex)
                .surface_alpha(alpha)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            // Over: a centred label that proves widgets-after-surface
            // composite on top.
            column([text("OVER").mono().heading()])
                .align(Align::Center)
                .justify(Justify::Center)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fixed(160.0)),
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
}

#[derive(Copy, Clone)]
enum AlphaPattern {
    /// Pre-baked premultiplied alpha — RGB is already multiplied by A.
    /// The corners are alpha=0 with RGB=(0,0,0); the centre is
    /// fully opaque with RGB at full intensity.
    Premultiplied,
    /// Straight (unpremultiplied) — RGB is the "intended" color and
    /// A controls coverage. The shader's fs_straight path multiplies
    /// in the GPU before the blend.
    Straight,
}

/// Allocate a small RGBA8 sRGB texture and fill it with a procedural
/// checkerboard whose alpha falls off radially. Identical pixel data
/// for every variant — only the per-pixel alpha *interpretation*
/// differs between Premultiplied and Straight, because that's what
/// changes how the shader composites.
fn make_app_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pattern: AlphaPattern,
) -> AppTexture {
    let texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::example::surface::app_texture"),
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

    let mut pixels = vec![0u8; (TEX_SIZE * TEX_SIZE * 4) as usize];
    let half = (TEX_SIZE as f32) * 0.5;
    let max_r = half;
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let dx = x as f32 - half;
            let dy = y as f32 - half;
            let r = (dx * dx + dy * dy).sqrt();
            // Coverage falls off from 1.0 in the centre to 0.0 at the
            // corner-touching radius.
            let cov = (1.0 - (r / max_r)).clamp(0.0, 1.0);
            // Checker pattern at full chroma — alternating cyan/yellow.
            let cell = ((x / 12) + (y / 12)) % 2 == 0;
            let (cr, cg, cb) = if cell {
                (255u8, 220, 80) // yellow
            } else {
                (40u8, 220, 220) // cyan
            };
            let i = ((y * TEX_SIZE + x) * 4) as usize;
            let alpha = (cov * 255.0).round() as u8;
            match pattern {
                AlphaPattern::Premultiplied => {
                    pixels[i] = ((cr as f32) * cov).round() as u8;
                    pixels[i + 1] = ((cg as f32) * cov).round() as u8;
                    pixels[i + 2] = ((cb as f32) * cov).round() as u8;
                    pixels[i + 3] = alpha;
                }
                AlphaPattern::Straight => {
                    pixels[i] = cr;
                    pixels[i + 1] = cg;
                    pixels[i + 2] = cb;
                    pixels[i + 3] = alpha;
                }
            }
        }
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
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

    app_texture(texture)
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
