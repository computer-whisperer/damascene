//! Readback regression test for the Tailwind two-layer shadow recipes.
//!
//! `params.z` is an elevation level that `rounded_rect.wgsl` expands
//! into Tailwind's `{dy, blur, spread, alpha}` layer pairs (see
//! `paint::shadow`). This drives the real prepare→render path with a
//! white rect on a white backdrop at two elevations and checks the
//! halo's direction, reach ordering, and restraint — through 0.4.6
//! shadows rendered a single 30%-black smoothstep, 3–6× darker than
//! Tailwind's and with the popover tier at dialog blur.
//!
//! Skips cleanly (passes) when no adapter is available.

use damascene_core::prelude::*;
use damascene_wgpu::Runner;

const W: u32 = 160;
const H: u32 = 160;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const RECT_W: f32 = 96.0;
const RECT_H: f32 = 48.0;
// Rect is centered: y spans [56, 104).
const RECT_TOP: u32 = 56;
const RECT_BOTTOM: u32 = 104;

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
        label: Some("shadow_render_test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

/// Render a white shadowed rect centered on white; return the red
/// channel per pixel (row-major, sRGB bytes).
fn render(device: &wgpu::Device, queue: &wgpu::Queue, level: f32) -> Vec<u8> {
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(W, H);
    let tree = stack([El::new(Kind::Group)
        .fill(Color::srgb_u8(255, 255, 255))
        .radius(8.0)
        .shadow(level)
        .width(Size::Fixed(RECT_W))
        .height(Size::Fixed(RECT_H))])
    .align(Align::Center)
    .justify(Justify::Center)
    .width(Size::Fixed(W as f32))
    .height(Size::Fixed(H as f32));
    runner.prepare(
        device,
        queue,
        tree,
        Rect::new(0.0, 0.0, W as f32, H as f32),
        1.0,
    );

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow_render_target"),
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
        label: Some("shadow_render_readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shadow_render"),
    });
    runner.render(
        device,
        &mut encoder,
        &target,
        &target_view,
        None,
        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
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
    let mut reds = Vec::with_capacity((W * H) as usize);
    for row in 0..H {
        let off = (row * bytes_per_row) as usize;
        for px in 0..W {
            reds.push(data[off + (px * 4) as usize]);
        }
    }
    drop(data);
    readback.unmap();
    reds
}

/// Darkest byte in the column under the rect's horizontal center for
/// row `y`.
fn center_darkness(px: &[u8], y: u32) -> u8 {
    255 - px[(y * W + W / 2) as usize]
}

/// Bottom-most row whose center pixel is visibly darkened.
fn shadow_reach_below(px: &[u8]) -> u32 {
    (RECT_BOTTOM..H)
        .rev()
        .find(|&y| center_darkness(px, y) >= 3)
        .unwrap_or(RECT_BOTTOM)
}

/// Manual visual dump — `cargo test -p damascene-wgpu --test
/// shadow_render -- --ignored --nocapture` writes an xs/sm/md/lg
/// specimen strip to DAMASCENE_SHADOW_DUMP (or ./shadow_sample.png).
#[test]
#[ignore = "manual visual dump, writes a PNG"]
fn dump_shadow_sample() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("shadow_render: no GPU adapter, skipping");
        return;
    };
    let levels = [
        damascene_core::tokens::SHADOW_XS,
        damascene_core::tokens::SHADOW_SM,
        damascene_core::tokens::SHADOW_MD,
        damascene_core::tokens::SHADOW_LG,
    ];
    let mut rgba = vec![255u8; (W * levels.len() as u32 * H * 4) as usize];
    let strip_w = W * levels.len() as u32;
    for (i, &level) in levels.iter().enumerate() {
        let frame = render(&device, &queue, level);
        for y in 0..H {
            for x in 0..W {
                let v = frame[(y * W + x) as usize];
                let o = ((y * strip_w + i as u32 * W + x) * 4) as usize;
                rgba[o..o + 3].copy_from_slice(&[v, v, v]);
            }
        }
    }
    let out = std::env::var("DAMASCENE_SHADOW_DUMP").unwrap_or_else(|_| "shadow_sample.png".into());
    let writer = std::io::BufWriter::new(std::fs::File::create(&out).expect("create png"));
    let mut enc = png::Encoder::new(writer, strip_w, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    eprintln!("dump_shadow_sample: wrote {out}");
}

#[test]
fn shadow_recipes_render_soft_directional_restrained_halos() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("shadow_render: no GPU adapter, skipping");
        return;
    };

    let sm = render(&device, &queue, damascene_core::tokens::SHADOW_SM);
    let lg = render(&device, &queue, damascene_core::tokens::SHADOW_LG);

    // The rect interior stays pure white — the opaque fill sits on top
    // of its own shadow.
    assert_eq!(center_darkness(&sm, (RECT_TOP + RECT_BOTTOM) / 2), 0);

    // A shadow exists below the rect at both levels…
    let sm_below = center_darkness(&sm, RECT_BOTTOM + 1);
    let lg_below = center_darkness(&lg, RECT_BOTTOM + 3);
    assert!(sm_below >= 3, "sm casts below the rect (got {sm_below})");
    assert!(lg_below >= 8, "lg casts below the rect (got {lg_below})");

    // …is directional (much weaker above than below)…
    let lg_above = center_darkness(&lg, RECT_TOP - 3);
    assert!(
        lg_above < lg_below / 2,
        "shadow is bottom-weighted (above {lg_above} vs below {lg_below})"
    );

    // …reaches farther at the higher elevation…
    let sm_reach = shadow_reach_below(&sm);
    let lg_reach = shadow_reach_below(&lg);
    assert!(
        lg_reach >= sm_reach + 6,
        "lg reach (row {lg_reach}) well beyond sm reach (row {sm_reach})"
    );
    eprintln!(
        "shadow_render: sm below {sm_below}, lg below {lg_below}, \
         sm reach +{}, lg reach +{}",
        sm_reach - RECT_BOTTOM,
        lg_reach - RECT_BOTTOM
    );

    // …and stays in Tailwind's restrained register: the recipes cap
    // combined alpha at 0.19, which after gamma compensation composites
    // on white to ≥ 207/255 (exactly what a browser renders for two
    // stacked 10% blacks). The pre-0.4.7 single 30%-black band exceeded
    // 90 here.
    let max_dark = (0..W * H).map(|i| 255 - lg[i as usize]).max().unwrap_or(0);
    assert!(
        max_dark <= 60,
        "max shadow darkness {max_dark} must stay in the 0.05–0.10-alpha register"
    );
}
