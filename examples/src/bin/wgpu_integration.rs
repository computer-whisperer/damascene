//! Damascene inside an existing wgpu renderer — the headline
//! integration story, live.
//!
//! Everything else in `examples/src/bin` runs through the batteries-
//! included `damascene-winit-wgpu` host. This bin is the opposite
//! demonstration: the *host application* owns the window, event loop,
//! instance, device, queue, surface, and its own 3D content (a spinning
//! cube with its own pipeline and depth buffer), and Damascene is
//! inlaid on top — sharing the same device and queue, compositing into
//! the same surface texture, with zero extra windows or contexts.
//!
//! The division of labor:
//!
//! - **App code stays portable.** `Hud` is an ordinary
//!   `damascene_core::App` — the same shape as `counter.rs`. It knows
//!   nothing about the host; it could run unchanged under
//!   `damascene_winit_wgpu::run`.
//! - **Host glue is what this file teaches.** Surface negotiation,
//!   input forwarding (`pointer_*` / `key_down` / `pointer_wheel`),
//!   the per-frame `prepare` → `render` sequence, and redraw
//!   scheduling — the "Custom-host checklist" from
//!   `crates/damascene-wgpu/README.md`, in code.
//!
//! Frame anatomy (one encoder, one submit):
//!
//! ```text
//! encoder ─▶ host pass: clear color + depth, draw the cube
//!         ─▶ runner.render(.., LoadOp::Load): UI composited over it
//! queue.submit ─▶ queue.present
//! ```
//!
//! The UI drives the scene: drag the slider (or focus it with Tab and
//! use the arrows) to change the spin speed, click Pause to freeze it.
//! While the cube spins the host renders continuously; while paused it
//! drops to input-driven frames, honoring `PrepareResult::needs_redraw`.
//!
//! Run: `cargo run -p damascene-examples --bin wgpu_integration`

use std::sync::Arc;
use std::time::Instant;

use damascene_core::prelude::*;
use damascene_wgpu::Runner;
use damascene_winit_wgpu::host::input;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------------
// App side — an ordinary damascene App, portable to any host.
// ---------------------------------------------------------------------

struct Hud {
    /// Cube revolutions per second, driven by the slider (0.0..=1.0
    /// slider value scaled by `MAX_SPEED`).
    speed: f32,
    paused: bool,
}

const MAX_SPEED: f32 = 1.5;

impl App for Hud {
    fn build(&self, _cx: &BuildCx) -> El {
        let status = if self.paused {
            "paused".to_string()
        } else {
            format!("{:.2} rev/s", self.speed * MAX_SPEED)
        };
        // The root is a transparent column pinned to the top-left; only
        // the card paints. Everything outside it shows the host's scene.
        column([card([
            h3("Damascene HUD"),
            text("This panel is drawn by damascene into the host's frame.")
                .muted()
                .caption(),
            row([
                text("Spin"),
                slider("speed", self.speed).width(Size::Fixed(160.0)),
                text(status).muted().width(Size::Ch(9.0)).tabular_numerals(),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center),
            row([
                button(if self.paused { "Resume" } else { "Pause" })
                    .key("pause")
                    .primary(),
                text("Tab focuses · arrows step the slider")
                    .muted()
                    .caption(),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center),
        ])
        .gap(tokens::SPACE_3)
        .width(Size::Fixed(340.0))])
        .padding(tokens::SPACE_4)
        .align(Align::Start)
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        // One call folds pointer drag and keyboard arrows into `speed`.
        slider::apply_event(&mut self.speed, &event, "speed", 0.05, 0.25);
        if event.is_click_or_activate("pause") {
            self.paused = !self.paused;
        }
    }
}

// ---------------------------------------------------------------------
// Host side — the application's own renderer, which damascene joins.
// ---------------------------------------------------------------------

/// The host's own content: a spinning cube. Vertices, rotation, and
/// projection all live in the shader; the host only uploads the current
/// angle and aspect ratio, so no matrix math intrudes on the example.
const CUBE_WGSL: &str = r#"
struct Uniforms { angle: f32, aspect: f32 }
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec3<f32> }

// 6 faces x 2 triangles, unit cube corners.
const CORNERS = array<vec3<f32>, 8>(
    vec3(-1.0, -1.0, -1.0), vec3(1.0, -1.0, -1.0),
    vec3(1.0, 1.0, -1.0), vec3(-1.0, 1.0, -1.0),
    vec3(-1.0, -1.0, 1.0), vec3(1.0, -1.0, 1.0),
    vec3(1.0, 1.0, 1.0), vec3(-1.0, 1.0, 1.0),
);
const FACES = array<vec4<u32>, 6>(
    vec4<u32>(4u, 5u, 6u, 7u), // +Z
    vec4<u32>(1u, 0u, 3u, 2u), // -Z
    vec4<u32>(5u, 1u, 2u, 6u), // +X
    vec4<u32>(0u, 4u, 7u, 3u), // -X
    vec4<u32>(7u, 6u, 2u, 3u), // +Y
    vec4<u32>(0u, 1u, 5u, 4u), // -Y
);
const FACE_COLORS = array<vec3<f32>, 6>(
    vec3(0.85, 0.35, 0.25), vec3(0.25, 0.55, 0.85), vec3(0.90, 0.70, 0.25),
    vec3(0.35, 0.75, 0.45), vec3(0.70, 0.45, 0.85), vec3(0.30, 0.70, 0.70),
);

fn rotate(p: vec3<f32>) -> vec3<f32> {
    let c = cos(u.angle);
    let s = sin(u.angle);
    let spun = vec3(p.x * c + p.z * s, p.y, -p.x * s + p.z * c);
    // Fixed camera tilt so three faces are visible.
    let ct = 0.921; // cos(0.4)
    let st = 0.389; // sin(0.4)
    return vec3(spun.x, spun.y * ct - spun.z * st, spun.y * st + spun.z * ct);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    let face = vi / 6u;
    let corner_of_face = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u)[vi % 6u];
    let p = rotate(CORNERS[FACES[face][corner_of_face]]);
    // View: cube at origin, camera 4.5 back along +Z looking down -Z.
    let view = vec3(p.x, p.y, p.z - 4.5);
    // Hand-rolled perspective, wgpu 0..1 depth (zn = 0.1, zf = 20).
    let f = 1.6;
    let a = 20.0 / (0.1 - 20.0);
    let b = 0.1 * 20.0 / (0.1 - 20.0);
    var out: VsOut;
    out.pos = vec4(view.x * f / u.aspect, view.y * f, view.z * a + b, -view.z);
    // Cheap lambert off the rotated face direction.
    let centers = array<vec3<f32>, 6>(
        vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0), vec3(1.0, 0.0, 0.0),
        vec3(-1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, -1.0, 0.0),
    );
    let normal = rotate(centers[face]);
    let light = normalize(vec3(0.4, 0.7, 0.6));
    let lit = 0.55 + 0.45 * max(dot(normal, light), 0.0);
    // FACE_COLORS are authored as sRGB. The *Srgb surface encodes
    // linear -> sRGB on write, so linearize (gamma ~2.0) or the cube
    // renders washed out.
    let c = FACE_COLORS[face];
    out.color = c * c * lit;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4(in.color, 1.0);
}
"#;

struct CubeRenderer {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    depth_size: (u32, u32),
}

impl CubeRenderer {
    fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cube"),
            source: wgpu::ShaderSource::Wgsl(CUBE_WGSL.into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cube uniforms"),
            size: 16, // angle + aspect, padded to uniform alignment
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cube bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cube bg"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cube layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cube pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(surface_format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: Default::default(),
            cache: None,
        });
        let (depth, depth_size) = Self::make_depth(device, size);
        Self {
            pipeline,
            uniforms,
            bind_group,
            depth,
            depth_size,
        }
    }

    fn make_depth(device: &wgpu::Device, size: (u32, u32)) -> (wgpu::TextureView, (u32, u32)) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cube depth"),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        (tex.create_view(&Default::default()), size)
    }

    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if size != self.depth_size {
            (self.depth, self.depth_size) = Self::make_depth(device, size);
        }
    }

    /// The host's own pass: clear the frame and draw the cube with a
    /// depth buffer. Damascene never sees this pass — its UI pass
    /// comes after, loading (not clearing) the color we produced here.
    fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        angle: f32,
        aspect: f32,
    ) {
        queue.write_buffer(&self.uniforms, 0, bytemuck_cast(&[angle, aspect, 0.0, 0.0]));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("host cube pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.013,
                        g: 0.015,
                        b: 0.022,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..36, 0..1);
    }
}

/// `&[f32]` → `&[u8]` for `write_buffer` without pulling in bytemuck.
fn bytemuck_cast(floats: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(floats.as_ptr().cast(), std::mem::size_of_val(floats)) }
}

// ---------------------------------------------------------------------
// Window + event plumbing.
// ---------------------------------------------------------------------

struct Gfx {
    // Drop order matters: the surface borrows the window (see the
    // custom-host checklist), so it is declared first.
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    cube: CubeRenderer,
    runner: Runner,
}

struct Host {
    gfx: Option<Gfx>,
    hud: Hud,
    angle: f32,
    last_frame: Instant,
    last_pointer: (f32, f32),
    modifiers: KeyModifiers,
}

impl Host {
    fn init(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Damascene — wgpu integration")
                        .with_inner_size(winit::dpi::LogicalSize::new(880.0, 560.0)),
                )
                .expect("create window"),
        );

        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("no adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("request device");

        // Prefer an sRGB swapchain format so the hardware encodes on
        // write — damascene assumes this (custom-host checklist).
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        let cube = CubeRenderer::new(&device, format, (config.width, config.height));

        // Damascene joins the host's device/queue here. `new` targets
        // the same format the surface was configured with; the warmup
        // pre-rasterizes ASCII so the first text frame doesn't hitch.
        let mut runner = Runner::new(&device, &queue, format);
        runner.warm_default_glyphs();
        runner.set_theme(self.hud.theme());
        runner.set_surface_size(config.width, config.height);

        self.last_frame = Instant::now();
        self.gfx = Some(Gfx {
            surface,
            window,
            device,
            queue,
            config,
            cube,
            runner,
        });
    }

    /// Route runner-produced events through the app, exactly like a
    /// packaged host would.
    fn dispatch(hud: &mut Hud, events: Vec<UiEvent>) -> bool {
        let any = !events.is_empty();
        for event in events {
            hud.on_event(event, &EventCx::new());
        }
        any
    }

    fn redraw(&mut self) {
        let Some(gfx) = self.gfx.as_mut() else { return };

        // Advance the host's own animation.
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        if !self.hud.paused {
            self.angle += dt * self.hud.speed * MAX_SPEED * std::f32::consts::TAU;
        }

        // Surface loss means reconfigure-and-retry next frame.
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.window.request_redraw();
                return;
            }
            other => {
                eprintln!("surface unavailable: {other:?}");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());

        // Layout runs in logical pixels; the surface is physical.
        let scale = gfx.window.scale_factor() as f32;
        let viewport = Rect::new(
            0.0,
            0.0,
            gfx.config.width as f32 / scale,
            gfx.config.height as f32 / scale,
        );
        let theme = self.hud.theme();
        let cx = BuildCx::new(&theme);
        let tree = self.hud.build(&cx);
        let prep = gfx
            .runner
            .prepare(&gfx.device, &gfx.queue, tree, viewport, scale);

        let mut encoder = gfx.device.create_command_encoder(&Default::default());
        // Host content first (clears color + depth)...
        let aspect = gfx.config.width as f32 / gfx.config.height.max(1) as f32;
        gfx.cube
            .draw(&gfx.queue, &mut encoder, &view, self.angle, aspect);
        // ...then damascene composites the UI over it. `LoadOp::Load`
        // preserves the host's pixels; `render` owns its own pass(es),
        // which also keeps backdrop-sampling shaders working if the UI
        // ever uses them. (Inside a pass you already own, `draw` is the
        // alternative — see the damascene-wgpu README.)
        gfx.runner.render(
            &gfx.device,
            &mut encoder,
            &frame.texture,
            &view,
            None,
            wgpu::LoadOp::Load,
        );
        gfx.queue.submit(Some(encoder.finish()));
        gfx.queue.present(frame);

        // Redraw policy: spin continuously while the cube animates;
        // when paused, only when damascene reports pending work
        // (settling springs, hover fades) via `needs_redraw`.
        if !self.hud.paused || prep.needs_redraw {
            gfx.window.request_redraw();
        }
    }
}

impl ApplicationHandler for Host {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_none() {
            self.init(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gfx) = self.gfx.as_mut() else { return };
        let scale = gfx.window.scale_factor() as f32;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                gfx.config.width = size.width.max(1);
                gfx.config.height = size.height.max(1);
                gfx.surface.configure(&gfx.device, &gfx.config);
                gfx.cube
                    .resize(&gfx.device, (gfx.config.width, gfx.config.height));
                gfx.runner
                    .set_surface_size(gfx.config.width, gfx.config.height);
                gfx.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32 / scale, position.y as f32 / scale);
                self.last_pointer = (x, y);
                let mv = gfx
                    .runner
                    .pointer_moved(Pointer::mouse(x, y, PointerButton::Primary));
                let dispatched = Self::dispatch(&mut self.hud, mv.events);
                if mv.needs_redraw || dispatched {
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if Self::dispatch(&mut self.hud, gfx.runner.pointer_left()) {
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = input::pointer_button(button) else {
                    return;
                };
                let (x, y) = self.last_pointer;
                let p = Pointer::mouse(x, y, button);
                let events = match state {
                    ElementState::Pressed => gfx.runner.pointer_down(p),
                    ElementState::Released => gfx.runner.pointer_up(p),
                };
                Self::dispatch(&mut self.hud, events);
                gfx.window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.last_pointer;
                let (_dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * 50.0, -y * 50.0),
                    MouseScrollDelta::PixelDelta(p) => {
                        (-(p.x as f32) / scale, -(p.y as f32) / scale)
                    }
                };
                if gfx.runner.pointer_wheel(x, y, dy) {
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = input::key_modifiers(mods.state());
                gfx.runner.set_modifiers(self.modifiers);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                // The pure winit→damascene mappers from the packaged
                // host are public — no need to hand-roll the key tables.
                let events = gfx.runner.key_down(
                    input::map_key(&event.logical_key),
                    input::map_physical(event.physical_key),
                    self.modifiers,
                    event.repeat,
                );
                Self::dispatch(&mut self.hud, events);
                gfx.window.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut Host {
        gfx: None,
        hud: Hud {
            speed: 0.35,
            paused: false,
        },
        angle: 0.6,
        last_frame: Instant::now(),
        last_pointer: (0.0, 0.0),
        modifiers: KeyModifiers::default(),
    })?;
    Ok(())
}
