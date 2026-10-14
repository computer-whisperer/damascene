//! Headless single-frame rendering to CPU pixels.
//!
//! Screenshot mechanism for tooling: create a device without a window
//! or display handle, render one settled frame of a built tree, and
//! read it back as RGBA8. The winit host exposes this to every app via
//! the `DAMASCENE_HEADLESS_SHOT` environment variable; render tools can
//! call it directly instead of hand-rolling the adapter/readback
//! boilerplate.
//!
//! The output is the SDR baseline (`Rgba8UnormSrgb`, default working
//! color space) — the same target the README hero renders to — not the
//! HDR-aware surface plan a windowed session may negotiate.

use damascene_core::theme::Theme;
use damascene_core::{AnimationMode, Color, El, Rect};

use crate::{MsaaTarget, Runner};

/// A wgpu device/queue pair created without any display handle.
pub struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Headless {
    /// Acquire an adapter and device with default limits and no
    /// required features. Errors if no compatible adapter exists.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no compatible adapter ({e})"))?;

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("damascene_wgpu::headless::device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))?;

        Ok(Self { device, queue })
    }

    /// The device, for `gpu_setup`-style app hooks.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue, for `gpu_setup`-style app hooks.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Render one settled frame of `tree` at `viewport` × `scale_factor`
    /// and read it back.
    ///
    /// Returns `(width, height, pixels)` — physical pixels, tightly
    /// packed sRGB-encoded RGBA8, row-major from the top-left. `clear`
    /// is the letterbox/background color behind the tree, normally the
    /// theme palette's `background`.
    ///
    /// `theme` must be the app's theme, matching the windowed host,
    /// which calls `Runner::set_theme(app.theme())` before every frame.
    /// Skipping it would paint token-carrying colors against the stock
    /// palette — invisible under the stock theme (token fallback rgba
    /// equals the stock palette) and silently wrong under any other.
    pub fn render_rgba8(
        &self,
        tree: El,
        theme: Theme,
        viewport: Rect,
        scale_factor: f32,
        sample_count: u32,
        clear: Color,
    ) -> Result<(u32, u32, Vec<u8>), Box<dyn std::error::Error>> {
        let device = &self.device;
        let queue = &self.queue;

        let width = (viewport.w * scale_factor) as u32;
        let height = (viewport.h * scale_factor) as u32;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let unpadded_bytes_per_row = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let readback_size = (padded_bytes_per_row * height) as u64;

        let msaa = MsaaTarget::new(device, format, extent, sample_count);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("damascene_wgpu::headless::target"),
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
            label: Some("damascene_wgpu::headless::readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut renderer = Runner::with_sample_count(device, queue, format, sample_count);
        renderer.set_animation_mode(AnimationMode::Settled);
        renderer.set_theme(theme);
        renderer.prepare(device, queue, tree, viewport, scale_factor);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("damascene_wgpu::headless::encoder"),
        });
        renderer.render(
            device,
            &mut encoder,
            &target,
            &target_view,
            Some(&msaa.view),
            wgpu::LoadOp::Clear(clear_color(clear)),
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
        device.poll(wgpu::PollType::wait_indefinitely())?;
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

        Ok((width, height, unpadded))
    }
}

/// sRGB-encoded palette color → the linear `wgpu::Color` a
/// `Rgba8UnormSrgb` attachment expects for its clear value.
fn clear_color(c: Color) -> wgpu::Color {
    fn srgb_to_linear(c: f64) -> f64 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    wgpu::Color {
        r: srgb_to_linear(c.r as f64 / 255.0),
        g: srgb_to_linear(c.g as f64 / 255.0),
        b: srgb_to_linear(c.b as f64 / 255.0),
        a: c.a as f64 / 255.0,
    }
}

/// Minimal executor for wgpu's adapter/device futures, which resolve
/// without an external reactor on native targets. Avoids a `pollster`
/// dependency in the backend crate.
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
