//! Headless wgpu render of the custom_paint commit-graph fixture.
//!
//! Same fixture as `crates/damascene-core/examples/custom_paint.rs` but
//! actually executes the WGSL fragment shader through wgpu — the SVG
//! fallback only emits dashed-magenta placeholders for custom-shader
//! Els, so this is the path that proves the visual is correct.
//!
//! Usage: `cargo run -p damascene-wgpu --example render_custom_paint`
//! Writes: `crates/damascene-wgpu/out/custom_paint.wgpu.png`

use damascene_core::prelude::*;
use damascene_wgpu::{MsaaTarget, Runner};

const COMMIT_NODE_WGSL: &str = r#"
struct FrameUniforms { viewport: vec2<f32>, _pad: vec2<f32>, };
@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput  { @location(0) corner_uv: vec2<f32>, };
struct InstanceInput {
    @location(1) rect:  vec4<f32>,
    @location(2) vec_a: vec4<f32>,
    @location(3) vec_b: vec4<f32>,
    @location(4) vec_c: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) @interpolate(perspective, sample) local_px: vec2<f32>,
    @location(1) size:   vec2<f32>,
    @location(2) fill:   vec4<f32>,
    @location(3) ring:   vec4<f32>,
    @location(4) params: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let pos_px = in.corner_uv * inst.rect.zw + inst.rect.xy;
    let clip = vec4<f32>(
        pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_px.y / frame.viewport.y * 2.0,
        0.0, 1.0,
    );
    var out: VertexOutput;
    out.clip_pos = clip;
    out.local_px = in.corner_uv * inst.rect.zw;
    out.size     = inst.rect.zw;
    out.fill     = inst.vec_a;
    out.ring     = inst.vec_b;
    out.params   = inst.vec_c;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let radius = in.params.x;
    let ring_w = in.params.y;
    let line_w = in.params.z;
    let lane_x = in.params.w * in.size.x;
    let row_y  = in.size.y * 0.5;

    let p     = in.local_px - vec2<f32>(lane_x, row_y);
    let d     = length(p) - radius;
    let aa    = max(fwidth(d), 0.5);
    let outer = 1.0 - smoothstep(0.0, aa, d);
    let inner = 1.0 - smoothstep(0.0, aa, d + ring_w);
    let ring_a = clamp(outer - inner, 0.0, 1.0);
    let body_a = inner;

    let dx   = abs(in.local_px.x - lane_x);
    let aa_l = max(fwidth(dx), 0.5);
    let line_a = (1.0 - smoothstep(line_w * 0.5 - aa_l, line_w * 0.5 + aa_l, dx))
                 * (1.0 - outer);

    let line_pm = vec4<f32>(in.ring.rgb * (in.ring.a * line_a), in.ring.a * line_a);
    let ring_pm = vec4<f32>(in.ring.rgb * (in.ring.a * ring_a), in.ring.a * ring_a);
    let body_pm = vec4<f32>(in.fill.rgb * (in.fill.a * body_a), in.fill.a * body_a);
    let pm = line_pm + ring_pm + body_pm;
    let a  = clamp(pm.a, 0.0, 1.0);
    if (a <= 0.0) { return vec4<f32>(0.0); }
    return vec4<f32>(pm.rgb / a, a);
}
"#;

const ROW_HEIGHT: f32 = 28.0;
const GRAPH_WIDTH: f32 = 140.0;
const LANE_COUNT: u8 = 4;

struct FakeCommit {
    sha: &'static str,
    subject: &'static str,
    author: &'static str,
    when: &'static str,
    lane: u8,
}

fn lane_palette(lane: u8) -> Color {
    match lane % LANE_COUNT {
        0 => Color::srgb_u8(96, 165, 230),
        1 => Color::srgb_u8(96, 200, 200),
        2 => Color::srgb_u8(140, 200, 110),
        _ => Color::srgb_u8(230, 180, 90),
    }
}

fn graph_cell(lane: u8, selected: bool) -> El {
    let lane_color = lane_palette(lane);
    let ring_color = if selected {
        Color::srgb_u8(245, 245, 250)
    } else {
        lane_color
    };
    let ring_w = if selected { 2.5 } else { 1.5 };
    let radius = 5.0;
    let line_w = 2.0;
    let lane_frac = (lane as f32 + 0.5) / LANE_COUNT as f32;

    El::new(Kind::Custom("graph_cell"))
        .width(Size::Fixed(GRAPH_WIDTH))
        .height(Size::Fixed(ROW_HEIGHT))
        .shader(
            ShaderBinding::custom("commit_node")
                .color("vec_a", tokens::BACKGROUND)
                .color("vec_b", ring_color)
                .vec4("vec_c", [radius, ring_w, line_w, lane_frac]),
        )
        .fill(lane_color)
}

fn build_row(c: &FakeCommit, idx: usize, selected: bool) -> El {
    row([
        graph_cell(c.lane, selected),
        text(c.sha).mono().muted(),
        text(c.subject),
        spacer(),
        text(format!("{} · {}", c.author, c.when)).muted(),
    ])
    .key(format!("commit-{idx}"))
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(tokens::SPACE_2, 0.0))
    .height(Size::Fixed(ROW_HEIGHT))
    .align(Align::Center)
}

#[rustfmt::skip]
const COMMITS: &[FakeCommit] = &[
    FakeCommit { sha: "8a3f1c9", subject: "fix race condition in scheduler",  author: "ada",     when: "12m", lane: 0 },
    FakeCommit { sha: "1b07d4e", subject: "tweak token tooltip wording",      author: "linus",   when: "1h",  lane: 0 },
    FakeCommit { sha: "9f2e4a1", subject: "wire avatar fallback identicon",   author: "joelle",  when: "3h",  lane: 1 },
    FakeCommit { sha: "44ab8d2", subject: "diff: word-level highlight pass",  author: "raphael", when: "5h",  lane: 1 },
    FakeCommit { sha: "61c0fe7", subject: "ci: bump rust toolchain to 1.85",  author: "mei",     when: "7h",  lane: 2 },
    FakeCommit { sha: "a90215b", subject: "switch logging to env_logger",     author: "isabel",  when: "1d",  lane: 2 },
    FakeCommit { sha: "0d7e3c4", subject: "drop unused commit_detail cache",  author: "noor",    when: "1d",  lane: 1 },
    FakeCommit { sha: "33b2118", subject: "context-menu spacing pass",        author: "kira",    when: "2d",  lane: 3 },
];

fn fixture() -> El {
    let selected_idx = 3;
    column([
        h2("Custom-painted commit graph"),
        text("8 commits · virtual_list · custom shader paints lane line + circle node").muted(),
        virtual_list(COMMITS.len(), ROW_HEIGHT, move |i| {
            build_row(&COMMITS[i], i, i == selected_idx)
        })
        .key("commits")
        .height(Size::Fill(1.0)),
    ])
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_2)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 900;
    let logical_height: u32 = 600;
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
        label: Some("damascene_wgpu::example::custom_paint::device"),
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
        label: Some("damascene_wgpu::example::custom_paint::target"),
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
        label: Some("damascene_wgpu::example::custom_paint::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);
    renderer.register_shader(&device, "commit_node", COMMIT_NODE_WGSL);

    let mut tree = fixture();
    renderer.prepare(&device, &queue, &mut tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::custom_paint::encoder"),
    });
    {
        let bg = bg_color();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("damascene_wgpu::example::custom_paint::pass"),
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
    let out = out_dir.join("custom_paint.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    Ok(())
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
