//! Readback regression test: a frame whose last painted item is
//! clipped still reaches the target whole on OpenGL and WebGL2.
//!
//! wgpu's GLES backend resolves a multisampled attachment with
//! `glBlitFramebuffer`, and leaves the scissor test as the pass's last
//! `set_scissor_rect` left it. A blit obeys the scissor test, so a pass
//! that ended inside a clip resolved only that clip: everything else on
//! the target kept whatever it held before — the previous frame on a
//! WebGL2 canvas, which showed as a blank header above a scroll, a
//! scrim missing around its dialog, and the old page under a shorter
//! new one. The runner now ends its draws with the scissor back on the
//! whole attachment.
//!
//! The frame is drawn twice in different colours, because the area the
//! broken resolve skips is not reliably blank: a fresh target can come
//! back holding the previous target's pixels, which pass a check
//! against the colour that was there before.
//!
//! Runs on a GL adapter when there is one (EGL with Mesa, ANGLE), and
//! on the default adapter as a control. Skips cleanly (passes) where
//! there is none.

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner, RunnerCaps};

/// The laid-out frame, in logical and (at scale 1) physical pixels.
const W: u32 = 64;
const H: u32 = 64;
const CLIP: f32 = 16.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SAMPLES: u32 = 4;

const GREEN: [u8; 4] = [0, 255, 0, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu_on(backends: wgpu::Backends) -> Option<Gpu> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("gl_msaa_resolve_scissor_test"),
        required_features: wgpu::Features::empty(),
        // What a WebGL2 page asks for; every adapter grants it.
        required_limits:
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some(Gpu {
        adapter,
        device,
        queue,
    })
}

/// Who begins the render pass.
#[derive(Clone, Copy, Debug)]
enum Path {
    /// [`Runner::render`], the way the winit and web hosts draw.
    Render,
    /// A pass the host begins, drawn into with [`Runner::draw`].
    Draw,
}

/// `background` over the whole frame, then a green box twice the size
/// of the clip around it — the last item painted, so the pass ends
/// with the scissor on the clip when `clipped`.
fn tree(background: Color, clipped: bool) -> El {
    let green = El::new(Kind::Group)
        .fill(Color::srgb_u8(0, 255, 0))
        .width(Size::Fixed(CLIP * 2.0))
        .height(Size::Fixed(CLIP * 2.0));
    let mut holder = column([green])
        .width(Size::Fixed(CLIP))
        .height(Size::Fixed(CLIP));
    if clipped {
        holder = holder.clip();
    }
    column([holder])
        .fill(background)
        .width(Size::Fixed(W as f32))
        .height(Size::Fixed(H as f32))
}

/// One frame at 4× MSAA into a fresh `target_w` × `H` target, read
/// back as tightly packed RGBA8 rows.
fn frame(
    gpu: &Gpu,
    path: Path,
    target_w: u32,
    tree: El,
    clear: wgpu::Color,
    sync_surface_size: bool,
) -> Vec<u8> {
    let Gpu {
        adapter,
        device,
        queue,
    } = gpu;
    let mut runner = Runner::with_caps(
        device,
        queue,
        FORMAT,
        SAMPLES,
        RunnerCaps::from_adapter(adapter),
    );
    if sync_surface_size {
        runner.set_surface_size(target_w, H);
    }
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
    );

    let extent = wgpu::Extent3d {
        width: target_w,
        height: H,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gl_msaa_resolve_scissor_target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let msaa = MsaaTarget::new(device, FORMAT, extent, SAMPLES);

    let unpadded = target_w * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gl_msaa_resolve_scissor_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gl_msaa_resolve_scissor"),
    });
    match path {
        Path::Render => runner.render(
            device,
            &mut encoder,
            &target,
            &target_view,
            Some(&msaa.view),
            wgpu::LoadOp::Clear(clear),
        ),
        Path::Draw => {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gl_msaa_resolve_scissor_host_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa.view,
                    resolve_target: Some(&target_view),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            runner.draw(&mut pass);
        }
    }
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
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range().unwrap();
    let mut pixels = Vec::with_capacity((unpadded * H) as usize);
    for row in 0..H {
        let start = (row * bytes_per_row) as usize;
        pixels.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    readback.unmap();
    pixels
}

fn pixel(pixels: &[u8], target_w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * target_w + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn close(got: [u8; 4], want: [u8; 4]) -> bool {
    got.iter()
        .zip(&want)
        .all(|(g, w)| (*g as i32 - *w as i32).abs() <= 1)
}

/// The frame a clip ends, preceded by one in other colours with no
/// clip, each into a fresh target.
fn clipped_frame_after_another(
    gpu: &Gpu,
    path: Path,
    target_w: u32,
    sync_surface_size: bool,
) -> Vec<u8> {
    frame(
        gpu,
        path,
        target_w,
        tree(Color::srgb_u8(255, 0, 0), false),
        wgpu::Color::BLACK,
        sync_surface_size,
    );
    frame(
        gpu,
        path,
        target_w,
        tree(Color::srgb_u8(255, 0, 255), true),
        wgpu::Color::BLUE,
        sync_surface_size,
    )
}

/// The GL adapter the bug lives on, then whatever the machine prefers.
fn adapters() -> Vec<(&'static str, Gpu)> {
    [
        ("GL", wgpu::Backends::GL),
        ("default", wgpu::Backends::all()),
    ]
    .into_iter()
    .filter_map(|(name, backends)| match gpu_on(backends) {
        Some(gpu) => Some((name, gpu)),
        None => {
            eprintln!("gl_msaa_resolve_scissor: no {name} adapter, skipping it");
            None
        }
    })
    .collect()
}

#[test]
fn a_frame_that_ends_inside_a_clip_resolves_whole() {
    for (name, gpu) in adapters() {
        let info = gpu.adapter.get_info();
        for path in [Path::Render, Path::Draw] {
            let pixels = clipped_frame_after_another(&gpu, path, W, true);
            let at = |x, y| pixel(&pixels, W, x, y);
            let ctx = format!("{name} ({:?}, {}), {path:?}", info.backend, info.name);
            assert!(
                close(at(8, 8), GREEN),
                "{ctx}: inside the clip read {:?}, expected the green box",
                at(8, 8)
            );
            assert!(
                close(at(40, 40), MAGENTA),
                "{ctx}: outside the last clip read {:?}, expected the background \
                 (the MSAA resolve copied only the last scissor rect?)",
                at(40, 40)
            );
            // The clip is real: the box overflows it and is cut there.
            assert!(
                close(at(24, 24), MAGENTA),
                "{ctx}: the box's clipped-away overflow read {:?}, expected the background",
                at(24, 24)
            );
            assert!(
                close(at(W - 4, H - 4), MAGENTA),
                "{ctx}: the far corner read {:?}, expected the background",
                at(W - 4, H - 4)
            );
        }
    }
}

/// [`Runner::render`] is handed the target, so its resolve covers the
/// target's own size even when the runner's `viewport_px` is short of
/// it — a host that has not called [`Runner::set_surface_size`], under
/// a scale factor that rounds a pixel under the surface. The frame is
/// then stretched over the target (the pass's viewport is the
/// attachment), so the background reaches past the runner's viewport,
/// and the resolve must carry it there. The target here is eight
/// pixels wider rather than one, so the probe sits clear of the
/// background's antialiased edge.
#[test]
fn render_resolves_the_target_beyond_the_viewport() {
    let target_w = W + 8;
    for (name, gpu) in adapters() {
        let info = gpu.adapter.get_info();
        let pixels = clipped_frame_after_another(&gpu, Path::Render, target_w, false);
        let at = |x, y| pixel(&pixels, target_w, x, y);
        let ctx = format!("{name} ({:?}, {})", info.backend, info.name);
        assert!(
            close(at(40, 40), MAGENTA),
            "{ctx}: outside the last clip read {:?}, expected the background",
            at(40, 40)
        );
        assert!(
            close(at(W + 4, 40), MAGENTA),
            "{ctx}: a column past the viewport read {:?}, expected the background",
            at(W + 4, 40)
        );
    }
}
