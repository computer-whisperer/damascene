//! Showcase — pages across six groups, demoing every shadcn-shaped
//! widget and every system-level capability (theme swap, animation,
//! hotkeys, custom shaders, overlays, toasts).
//!
//! This binary is a small adapter that wraps [`Showcase`] (the
//! backend-neutral fixture in `aetna-fixtures`) with the wgpu plumbing
//! the Media page's animated `surface()` demo needs:
//! - allocate a 96×96 RGBA8 texture in `WinitWgpuApp::gpu_setup`,
//! - write a procedurally-animated frame in `WinitWgpuApp::before_paint`,
//! - hand the resulting `AppTexture` to `Showcase::set_animated_surface`
//!   so the Media page composes it under three `SurfaceAlpha` modes.
//!
//! Run: `cargo run -p aetna-examples --bin showcase`
//!
//! See `aetna_fixtures::showcase` for the full module docs.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::Instant;

use aetna_core::prelude::*;
use aetna_core::{App, BuildCx, KeyChord, UiEvent};
use aetna_fixtures::Showcase;
use aetna_winit_wgpu::WinitWgpuApp;

const TEX_SIZE: u32 = 96;

/// Wraps [`Showcase`] with wgpu lifecycle hooks that drive the Media
/// page's animated-surface demo.
struct AnimatedShowcase {
    inner: Showcase,
    /// Backing wgpu texture. Populated in `gpu_setup`; persisted across
    /// frames so each `before_paint` reuses the same allocation.
    texture: Option<Arc<wgpu::Texture>>,
    /// CPU staging buffer reused across frames to avoid allocations.
    /// Sized once to `TEX_SIZE * TEX_SIZE * 4`.
    pixels: Vec<u8>,
    /// Wall-clock anchor for the procedural animation phase.
    start: Instant,
}

impl AnimatedShowcase {
    fn new() -> Self {
        Self {
            inner: Showcase::new(),
            texture: None,
            pixels: vec![0u8; (TEX_SIZE * TEX_SIZE * 4) as usize],
            start: Instant::now(),
        }
    }
}

impl App for AnimatedShowcase {
    fn build(&self, cx: &BuildCx) -> El {
        self.inner.build(cx)
    }

    fn before_build(&mut self) {
        self.inner.before_build();
    }

    fn on_event(&mut self, event: UiEvent) {
        self.inner.on_event(event);
    }

    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        self.inner.hotkeys()
    }

    fn drain_toasts(&mut self) -> Vec<aetna_core::toast::ToastSpec> {
        self.inner.drain_toasts()
    }

    fn drain_focus_requests(&mut self) -> Vec<String> {
        self.inner.drain_focus_requests()
    }

    fn drain_scroll_requests(&mut self) -> Vec<aetna_core::scroll::ScrollRequest> {
        self.inner.drain_scroll_requests()
    }

    fn shaders(&self) -> Vec<aetna_core::AppShader> {
        self.inner.shaders()
    }

    fn theme(&self) -> Theme {
        self.inner.theme()
    }

    fn selection(&self) -> aetna_core::Selection {
        self.inner.selection()
    }
}

impl WinitWgpuApp for AnimatedShowcase {
    fn gpu_setup(&mut self, device: &wgpu::Device, _queue: &wgpu::Queue) {
        let texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
            label: Some("showcase::animated_surface"),
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
        let app_tex = aetna_wgpu::app_texture(texture.clone());
        self.inner.set_animated_surface(Some(app_tex));
        self.texture = Some(texture);
    }

    fn before_paint(&mut self, queue: &wgpu::Queue) {
        let Some(texture) = self.texture.as_ref() else {
            return;
        };
        let t = self.start.elapsed().as_secs_f32();
        write_frame(&mut self.pixels, t);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
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
    }
}

/// Procedural frame: a rotating chromatic ring that fades to
/// transparent at the edges. Visible behavior under each
/// `SurfaceAlpha` mode:
/// - Premultiplied / Straight: backdrop shows through the
///   transparent ring centre and corners,
/// - Opaque: the texture fully replaces the cell — alpha=0 regions
///   come through as black (which is what the texture actually
///   stores there).
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
            // Hue cycles around the ring with a phase that rotates
            // over time.
            let hue = (theta / TAU + t * 0.25).rem_euclid(1.0);
            let (rr, gg, bb) = hsv_to_rgb(hue, 0.9, 1.0);

            // Smooth ring coverage: 1 in the band, fading to 0 at
            // both rims.
            let band_t = ((r - r_inner) / (r_outer - r_inner)).clamp(0.0, 1.0);
            // 0 at edges, 1 mid-band.
            let cov = (1.0 - (band_t * 2.0 - 1.0).abs()).max(0.0);
            // Smoothstep for a softer falloff.
            let cov = cov * cov * (3.0 - 2.0 * cov);

            let a = (cov * 255.0).round() as u8;
            // Premultiplied output (matches SurfaceAlpha::Premultiplied
            // semantics; the Straight tile in the showcase reuses the
            // same texture pattern with the same blend math, so the
            // visible difference between modes comes from the
            // showcase's three blend pipelines, not the pixel data).
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--profile <output.json>` attaches a `tracing-chrome` subscriber
    // for the lifetime of the run; the JSON is loadable in
    // chrome://tracing or perfetto. Compiled out unless the binary is
    // built with `--features profiling`. Drops to a clear error when
    // the flag is passed without the feature.
    let _profile_guard = install_profiling()?;

    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    // No HostConfig::with_redraw_interval here — the Media page's
    // animated surface tile carries `redraw_within(16ms)` directly,
    // and Aetna folds that into `PrepareResult::next_redraw_in` for
    // the host loop. The host idles automatically when no visible
    // widget asks for a future frame (e.g. when the user navigates
    // to a different Media-page section).
    //
    // Opt into the broad wide-gamut/HDR ladder so the Color Management
    // page has something to show on capable compositors. On a host that
    // advertises BT.2020 + ext_linear (e.g. prism) this negotiates a
    // BT.2020-linear float surface and hands the compositor wide-gamut
    // buffers; everywhere else it degrades silently to sRGB, identical
    // to the default. The negotiated outcome is visible live on the
    // Color Management page.
    let config = aetna_winit_wgpu::HostConfig::default()
        .with_color_preferences(aetna_core::color::ColorPreferences::hdr_broad());
    aetna_winit_wgpu::run_host_app_with_config(
        "Aetna — showcase",
        viewport,
        AnimatedShowcase::new(),
        config,
    )
}

#[cfg(feature = "profiling")]
fn install_profiling() -> Result<Option<tracing_chrome::FlushGuard>, Box<dyn std::error::Error>> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let mut args = std::env::args().skip(1);
    let mut output: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => {
                output = Some(args.next().ok_or("--profile expects a path argument")?);
            }
            other if other.starts_with("--profile=") => {
                output = Some(other.trim_start_matches("--profile=").to_string());
            }
            _ => {}
        }
    }
    let Some(path) = output else {
        return Ok(None);
    };

    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file(&path)
        .include_args(true)
        .build();
    tracing_subscriber::registry().with(chrome_layer).init();
    eprintln!(
        "aetna-showcase: tracing chrome JSON → {path} (load in chrome://tracing or perfetto)"
    );
    Ok(Some(guard))
}

#[cfg(not(feature = "profiling"))]
fn install_profiling() -> Result<Option<()>, Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--profile" || a.starts_with("--profile=")) {
        return Err(
            "--profile passed but the binary was built without `--features profiling`. \
             Rebuild with `cargo run -p aetna-examples --bin showcase --features profiling -- --profile out.json`."
                .into(),
        );
    }
    Ok(None)
}
