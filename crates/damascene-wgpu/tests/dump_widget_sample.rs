//! Manual visual-inspection dump — run with:
//!   cargo test -p damascene-wgpu --test dump_widget_sample -- --ignored --nocapture
//! Writes DAMASCENE_WIDGET_DUMP (or ./widget_sample.png) with the
//! menu/table/badge chrome for eyeballing restyles. Ignored by
//! default: it produces a file, not an assertion.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const W: u32 = 640;
const H: u32 = 360;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("dump_widget_sample"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
#[ignore = "manual visual dump, writes a PNG"]
fn dump_widget_sample() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("dump_widget_sample: no GPU adapter, skipping");
        return;
    };
    let mut runner = Runner::new(&device, &queue, FORMAT);
    runner.set_surface_size(W, H);

    let menu = damascene_core::widgets::popover::popover_panel([
        damascene_core::widgets::popover::menu_item_checked("Light", false),
        damascene_core::widgets::popover::menu_item_checked("Dark", true),
        damascene_core::widgets::popover::menu_item_checked("System", false),
    ])
    .width(Size::Fixed(160.0));

    let dropdown = dropdown_menu_content([
        dropdown_menu_label("My Account"),
        dropdown_menu_separator(),
        dropdown_menu_item([dropdown_menu_item_label("Profile")]),
        dropdown_menu_item([dropdown_menu_item_label("Billing")]),
        dropdown_menu_item([dropdown_menu_item_label("Settings")]),
    ]);

    let tbl = table([
        table_header([table_row([
            table_head("Invoice"),
            table_head("Status"),
            table_head("Amount"),
        ])]),
        table_body([
            table_row([
                table_cell(text("INV001")),
                table_cell(text("Paid")),
                table_cell(text("$250.00")),
            ]),
            table_row([
                table_cell(text("INV002")),
                table_cell(text("Pending")),
                table_cell(text("$150.00")),
            ]),
            table_row([
                table_cell(text("INV003")),
                table_cell(text("Unpaid")),
                table_cell(text("$350.00")),
            ]),
        ]),
    ])
    .width(Size::Fixed(300.0));

    let badges = row([
        badge("info"),
        badge("ok").success(),
        badge("warn").warning(),
        badge("err").destructive(),
    ])
    .gap(8.0)
    .height(Size::Hug);

    let tree = row([
        column([menu, dropdown]).gap(16.0).width(Size::Hug),
        column([tbl, badges]).gap(16.0).width(Size::Hug),
    ])
    .gap(24.0)
    .padding(Sides::all(24.0))
    .width(Size::Fixed(W as f32))
    .height(Size::Fixed(H as f32));

    runner.prepare(
        &device,
        &queue,
        tree,
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("dump_widget_sample_target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let unpadded = W * 4;
    let bytes_per_row =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dump_widget_sample_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dump_widget_sample"),
    });
    let bg = damascene_core::tokens::BACKGROUND;
    let [r, g, b, a] = damascene_core::paint::rgba_f32_in(bg, runner.working_color_space());
    runner.render(
        &device,
        &mut encoder,
        &target,
        &target_view,
        None,
        wgpu::LoadOp::Clear(wgpu::Color {
            r: r as f64,
            g: g as f64,
            b: b as f64,
            a: a as f64,
        }),
    );
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
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let data = slice.get_mapped_range().unwrap();
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for rowi in 0..H {
        let off = (rowi * bytes_per_row) as usize;
        pixels.extend_from_slice(&data[off..off + (W * 4) as usize]);
    }
    drop(data);
    readback.unmap();

    let out = std::env::var("DAMASCENE_WIDGET_DUMP").unwrap_or_else(|_| "widget_sample.png".into());
    let writer = std::io::BufWriter::new(std::fs::File::create(&out).expect("create png"));
    let mut enc = png::Encoder::new(writer, W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
    eprintln!("dump_widget_sample: wrote {out}");
}
