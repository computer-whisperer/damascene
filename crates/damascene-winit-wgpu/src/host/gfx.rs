//! Per-window GPU bring-up: surface, swapchain config, renderer, color
//! driver, and MSAA target as one bundle.

use std::sync::Arc;

use damascene_wgpu::{MsaaTarget, Runner, RunnerCaps};
use winit::window::Window;

use super::color::{Renegotiation, SurfaceColor, build_surface_color_info};
use crate::HostConfig;

/// The full render extent of a configured surface.
pub fn surface_extent(config: &wgpu::SurfaceConfiguration) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    }
}

/// Everything one window needs to render damascene frames: the
/// configured surface, the `Runner`, the per-window color driver, and
/// the optional MSAA target, all kept consistent by [`resize`](Self::resize)
/// and [`apply_renegotiation`](Self::apply_renegotiation).
///
/// The built-in run loop builds one of these in `resumed()`; a custom
/// multi-window host builds one per window it creates, on a shared
/// device/queue — `WindowGfx` holds clones of wgpu's internally
/// ref-counted handles and never assumes it owns them. Resident
/// daemons that want windows to open instantly can pool pre-warmed
/// `Runner`s and hand them in via
/// [`with_surface_and_renderer`](Self::with_surface_and_renderer),
/// skipping the pipeline-compile + glyph-warming cost on the open
/// path.
///
/// After construction the renderer is generic: call
/// `renderer.set_theme(..)` and register any app shaders
/// (`renderer.register_shader_with(..)`) before the first frame.
///
/// # Field order is a drop-order contract
///
/// Fields drop in declaration order. GPU resources must go before the
/// device/window they were created from, and the color driver shares
/// winit's wayland connection so it must drop before `window`. Keep
/// that in mind when destructuring or moving fields out.
pub struct WindowGfx {
    /// Per-window color driver: negotiated format/working space, the
    /// live `wp_color_management_v1` client, and the status surfaced
    /// via [`HostDiagnostics::color_management`](damascene_core::HostDiagnostics::color_management).
    pub color: SurfaceColor,
    /// The wgpu/WSI half of color negotiation — advertised surface
    /// formats, chosen swapchain format, present/alpha mode, adapter.
    /// Built once at surface creation; surfaced via
    /// [`HostDiagnostics::surface_color`](damascene_core::HostDiagnostics::surface_color).
    pub surface_color: damascene_core::SurfaceColorInfo,
    pub renderer: Runner,
    pub surface: wgpu::Surface<'static>,
    pub queue: wgpu::Queue,
    pub device: wgpu::Device,
    pub window: Arc<Window>,
    pub config: wgpu::SurfaceConfiguration,
    /// Multisampled color attachment for the surface frame, kept in
    /// sync with `config.width`/`config.height` and reallocated on
    /// resize. The surface frame texture is the resolve target.
    pub msaa: Option<MsaaTarget>,
}

impl WindowGfx {
    /// Bring up rendering for one window on a shared device/queue:
    /// create and configure the surface, negotiate color, pick a
    /// present mode, and construct the `Runner` (warming the default
    /// glyph set).
    ///
    /// The only fallible step is surface creation; adapter/device
    /// acquisition is the caller's job precisely so N windows can
    /// share one device (`Runner` takes `(device, queue, format)`, so
    /// this constructor just clones the handles it's given). When you
    /// already created the surface — typically for the *first* window,
    /// whose surface anchors `compatible_surface` during adapter
    /// selection — use [`with_surface`](Self::with_surface) instead of
    /// creating a second one.
    ///
    /// `host_config` supplies the color-preference ladder, the MSAA
    /// `sample_count`, and the `low_latency_present` choice; its
    /// run-loop knobs (redraw interval, wakeup hook, app id) are not
    /// consulted here.
    pub fn new(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window: Arc<Window>,
        host_config: &HostConfig,
    ) -> Result<Self, wgpu::CreateSurfaceError> {
        let surface = instance.create_surface(window.clone())?;
        Ok(Self::with_surface(
            adapter,
            device,
            queue,
            window,
            surface,
            host_config,
        ))
    }

    /// [`new`](Self::new) for a surface the caller already created
    /// from `window` (e.g. the one used as `compatible_surface` when
    /// requesting the adapter). Infallible — everything past surface
    /// creation can't fail.
    pub fn with_surface(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        host_config: &HostConfig,
    ) -> Self {
        let (color, surface_caps, config) =
            Self::negotiate_and_configure(adapter, device, &window, &surface, host_config);
        // Adapter caps matter on a native GL/GLES adapter (no-Vulkan
        // machines, `WGPU_BACKEND=gl`): naga's GLSL target rejects
        // per-sample interpolation qualifiers and can't `textureLoad`
        // depth textures (Scene3D label occlusion then uses the packed
        // depth-as-color capture). See `RunnerCaps`.
        let mut renderer = Runner::with_caps(
            device,
            queue,
            config.format,
            host_config.sample_count.max(1),
            RunnerCaps::from_adapter(adapter),
        );
        // Pre-rasterize printable ASCII for Inter + JetBrains Mono so
        // first-frame appearance of new text labels (e.g. switching
        // section in the showcase) doesn't trip a 20-30ms MSDF
        // generation hitch. ~40ms one-off at startup.
        renderer.warm_default_glyphs();
        Self::assemble(
            adapter,
            device,
            queue,
            window,
            surface,
            host_config,
            color,
            surface_caps,
            config,
            renderer,
        )
    }

    /// [`with_surface`](Self::with_surface) with a pre-built
    /// [`Runner`] instead of constructing one — the warm-pool path for
    /// resident multi-window hosts.
    ///
    /// `Runner` construction (pipeline compiles + glyph warming) is by
    /// far the most expensive step of window bring-up, and a `Runner`
    /// is not bound to any surface: it depends only on the target
    /// format and MSAA sample count it was built with. A daemon can
    /// build and warm Runners off the open path (`Runner::with_caps` +
    /// [`Runner::warm_default_glyphs`]) and hand one in here, paying
    /// only surface creation + color negotiation when a window opens.
    ///
    /// Reuse contract: build the pooled `Runner` with the
    /// (format, sample_count) the new window will negotiate —
    /// `sample_count` comes straight from `host_config`, and the format
    /// from the same color-preference ladder this constructor runs. On
    /// a format mismatch (e.g. a window landing on a different-HDR
    /// output) this still behaves correctly — `Runner::set_target_format`
    /// rebuilds the format-bound pipelines in place — but that rebuild
    /// costs what the pool was meant to avoid. A `sample_count`
    /// mismatch is not detected or repaired: track the pair you built
    /// each pooled Runner with.
    ///
    /// `warm_default_glyphs` is *not* called here — a pooled Runner is
    /// expected to be warm already.
    pub fn with_surface_and_renderer(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        host_config: &HostConfig,
        renderer: Runner,
    ) -> Self {
        let (color, surface_caps, config) =
            Self::negotiate_and_configure(adapter, device, &window, &surface, host_config);
        Self::assemble(
            adapter,
            device,
            queue,
            window,
            surface,
            host_config,
            color,
            surface_caps,
            config,
            renderer,
        )
    }

    /// Shared front half of construction: negotiate color, pick a
    /// present mode, and configure the surface. Runner-independent so
    /// [`with_surface`](Self::with_surface) can build a fresh `Runner`
    /// with the negotiated format and
    /// [`with_surface_and_renderer`](Self::with_surface_and_renderer)
    /// can re-point a pooled one at it.
    fn negotiate_and_configure(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        window: &Window,
        surface: &wgpu::Surface<'static>,
        host_config: &HostConfig,
    ) -> (
        SurfaceColor,
        wgpu::SurfaceCapabilities,
        wgpu::SurfaceConfiguration,
    ) {
        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(adapter);

        // Color negotiation: intersect the app's preferences with what
        // the display server can color-manage and what the wgpu surface
        // can represent. The chosen `format` drives the swapchain;
        // `working_space` drives the renderer; the status is surfaced to
        // apps via `HostDiagnostics`. Silent sRGB fallback on any
        // mismatch (and always off Linux/wayland-color-management).
        let color = SurfaceColor::negotiate(window, &host_config.color_preferences, &surface_caps);
        let format = color.format();

        // Pick a present mode. `Fifo` is the conservative default —
        // mandatory in the wgpu spec, vsync-locked, predictable power
        // cost. `low_latency_present` opts into `Mailbox` (with `Fifo`
        // fallback) for apps where interaction latency matters more
        // than steady-state throughput; see `HostConfig` for the
        // rationale and trade-offs.
        //
        // `DAMASCENE_PRESENT_MODE=mailbox|immediate|fifo` overrides at
        // runtime — useful for diagnosing without a recompile.
        let mode_override = std::env::var("DAMASCENE_PRESENT_MODE").ok();
        let prefer_mailbox =
            host_config.low_latency_present || mode_override.as_deref() == Some("mailbox");
        let prefer_immediate = mode_override.as_deref() == Some("immediate");
        let prefer_fifo = mode_override.as_deref() == Some("fifo");
        let present_mode = if prefer_immediate
            && surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else if prefer_mailbox
            && !prefer_fifo
            && surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            surface_caps.present_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            // COPY_SRC is required so backdrop-sampling shaders can
            // copy the post-Pass-A surface into the runner's snapshot
            // texture mid-frame. Cost is minimal — most surfaces
            // already advertise it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // `Auto` reproduces the pre-wgpu-30 behavior our color
            // negotiation was built against: `ExtendedSrgbLinear` for
            // Rgba16Float surfaces (the scRGB HDR path) and `Srgb`
            // otherwise. Explicit color-space selection (wide-gamut,
            // `display_hdr_info`) is a separate negotiation feature.
            color_space: wgpu::SurfaceColorSpace::Auto,
            // Keep the in-flight queue shallow. With `Fifo` this is a
            // hint that Mesa's WSI does not always honor — measured
            // resize lag on Wayland was unaffected by changing this
            // alone — but it's still the right default: an
            // interactive UI gains nothing from buffering more than
            // one frame ahead. Combined with `low_latency_present`
            // (Mailbox), interactive cadence is bounded by render
            // time, not by drained queue depth.
            desired_maximum_frame_latency: 1,
        };
        surface.configure(device, &config);
        (color, surface_caps, config)
    }

    /// Shared back half of construction: re-point `renderer` at the
    /// negotiated surface and bundle the fields. `renderer` must
    /// already be warm — neither constructor path warms glyphs here.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        host_config: &HostConfig,
        color: SurfaceColor,
        surface_caps: wgpu::SurfaceCapabilities,
        config: wgpu::SurfaceConfiguration,
        mut renderer: Runner,
    ) -> Self {
        let format = config.format;
        // No-op when the Runner was built with this format (always, on
        // the `with_surface` path); rebuilds the format-bound pipelines
        // in place when a pooled Runner meets a different-format
        // surface.
        renderer.set_target_format(device, format);
        renderer.set_surface_size(config.width, config.height);
        // Composite in the negotiated working space. For an sRGB
        // swapchain this is SRGB_LINEAR (the GPU sRGB-encodes on store);
        // for a float swapchain it's the wide-gamut linear space the
        // surface holds verbatim.
        renderer.set_working_color_space(color.working_space());
        // White scale stays at 1.0 on every format this host negotiates
        // — including the float swapchain. Mesa's WSI tags it as a
        // *parametric* ext-linear description with no luminances, whose
        // protocol default reference white is the 80 cd/m² encoding
        // scale itself: reference white sits at signal 1.0 and the
        // compositor's anchoring maps it to the output reference. A
        // Windows-style 203/80 lift on top double-applies (~2.5× hot,
        // measured against prism). `WINDOWS_SCRGB_WHITE_SCALE` is for
        // hosts whose surface genuinely reads as Windows scRGB (signal
        // 1.0 = 80 cd/m² absolute, reference at 2.5375) — actual
        // Windows, or the protocol's `windows_scrgb` predefined
        // description. See docs/COLOR_MANAGEMENT.md.
        // Output luminance frame for the per-image HDR remaster: images
        // brighter than the panel's headroom roll off (BT.2390) instead
        // of clipping. SDR swapchains get headroom 1.0 — HDR images
        // tonemap down rather than hard-clip.
        if let Some((headroom, reference)) = color.output_luminance() {
            renderer.set_output_luminance(headroom, reference);
        }

        let sample_count = host_config.sample_count.max(1);
        let msaa = (sample_count > 1)
            .then(|| MsaaTarget::new(device, format, surface_extent(&config), sample_count));

        let surface_color = build_surface_color_info(
            adapter,
            &surface_caps,
            format,
            config.present_mode,
            config.alpha_mode,
        );

        Self {
            color,
            surface_color,
            renderer,
            surface,
            queue: queue.clone(),
            device: device.clone(),
            window,
            config,
            msaa,
        }
    }

    /// Apply a new surface size: reconfigure the swapchain, tell the
    /// renderer, and reallocate the MSAA target if the extent actually
    /// changed. Call with coalesced sizes (once per frame, not once
    /// per `WindowEvent::Resized`) — `surface.configure` stalls the
    /// GPU pipeline.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.renderer
            .set_surface_size(self.config.width, self.config.height);
        let extent = surface_extent(&self.config);
        if let Some(msaa) = self.msaa.as_mut()
            && !msaa.matches(extent)
        {
            *msaa = MsaaTarget::new(&self.device, self.config.format, extent, msaa.sample_count);
        }
    }

    /// Apply a live color re-negotiation from [`SurfaceColor::poll`].
    ///
    /// Always refreshes the renderer's output-luminance frame (it can
    /// change without a format flip — e.g. a peak-luminance
    /// reconfiguration on the same HDR output). On a swapchain flip
    /// (SDR ↔ HDR output move / toggle) additionally reconfigures the
    /// surface — Mesa re-tags it from the new format (Rgba16Float →
    /// scRGB, 8-bit → sRGB) — rebuilds the renderer's format-bound
    /// pipelines in place (interaction state, atlases, and texture
    /// caches survive — see `Runner::set_target_format`), refreshes
    /// the working space, and reallocates the MSAA target. No
    /// white-scale change on a format flip: reference white sits at
    /// signal 1.0 on both encodings here (see docs/COLOR_MANAGEMENT.md).
    ///
    /// The caller still owns the redraw: request one after applying so
    /// the new state reaches the screen.
    pub fn apply_renegotiation(&mut self, plan: &Renegotiation) {
        self.renderer
            .set_output_luminance(plan.headroom, plan.reference_nits);
        if let Some(format) = plan.new_format {
            self.config.format = format;
            self.surface.configure(&self.device, &self.config);
            self.renderer.set_target_format(&self.device, format);
            self.renderer.set_working_color_space(plan.working_space);
            if let Some(msaa) = self.msaa.as_mut() {
                *msaa = MsaaTarget::new(
                    &self.device,
                    format,
                    surface_extent(&self.config),
                    msaa.sample_count,
                );
            }
            self.surface_color.chosen_format = format!("{format:?}");
        }
    }
}
