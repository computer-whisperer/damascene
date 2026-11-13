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
use damascene_wgpu::headless::Headless;

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
    render_surface(device, queue, level, Color::srgb_u8(255, 255, 255), 1.0)
}

/// [`render`] with the rect's fill and the node's opacity chosen.
fn render_surface(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    level: f32,
    fill: Color,
    opacity: f32,
) -> Vec<u8> {
    render_node(
        device,
        queue,
        surface_node(level).fill(fill).opacity(opacity),
    )
}

/// The rect under test, with no paint of its own yet.
fn surface_node(level: f32) -> El {
    El::new(Kind::Group)
        .radius(8.0)
        .shadow(level)
        .width(Size::Fixed(RECT_W))
        .height(Size::Fixed(RECT_H))
}

/// `node` centered on white; the red channel per pixel.
fn render_node(device: &wgpu::Device, queue: &wgpu::Queue, node: El) -> Vec<u8> {
    let mut runner = Runner::new(device, queue, FORMAT);
    runner.set_surface_size(W, H);
    let tree = stack([node])
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

    // The rect interior stays pure white — a shadow is cast outside
    // its surface, never drawn under it.
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

/// A surface that fades — a popover or dialog entering, anything under
/// `.opacity()` — fades as one piece. Through 0.8 the shadow ignored
/// the opacity and was painted under the whole silhouette, so a white
/// surface on a white page began its fade-in as a grey slab (208 of
/// 255 at opacity 0 for the popover tier).
#[test]
fn a_fading_surface_takes_its_shadow_with_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("shadow_render: no GPU adapter, skipping");
        return;
    };
    let white = Color::srgb_u8(255, 255, 255);
    let level = damascene_core::tokens::SHADOW_MD;
    let middle = (RECT_TOP + RECT_BOTTOM) / 2;
    let below = RECT_BOTTOM + 2;

    let full = render_surface(&device, &queue, level, white, 1.0);
    let full_below = center_darkness(&full, below);
    assert!(
        full_below >= 8,
        "the shadow under test exists ({full_below})"
    );

    // Fully faded: nothing is painted at all.
    let gone = render_surface(&device, &queue, level, white, 0.0);
    let darkest = gone.iter().map(|v| 255 - v).max().unwrap_or(0);
    assert_eq!(darkest, 0, "an invisible surface casts no shadow");

    // Half faded: the halo is at about half strength, and the body —
    // white fading in over white — is still white, not shadow seen
    // through a translucent fill.
    let half = render_surface(&device, &queue, level, white, 0.5);
    assert_eq!(
        center_darkness(&half, middle),
        0,
        "no shadow shows through the fading fill"
    );
    let half_below = center_darkness(&half, below);
    assert!(
        half_below >= full_below / 4 && half_below <= full_below * 3 / 4,
        "the halo fades with the surface (half {half_below}, full {full_below})"
    );
}

/// Darkest byte anywhere inside the rect, four pixels clear of its
/// edge — and so of a 1 px stroke, the antialiasing band, and the
/// 8 px corners' cut.
fn interior_darkness(px: &[u8]) -> u8 {
    let x0 = (W - RECT_W as u32) / 2 + 4;
    let x1 = (W + RECT_W as u32) / 2 - 4;
    (RECT_TOP + 4..RECT_BOTTOM - 4)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| 255 - px[(y * W + x) as usize])
        .max()
        .unwrap_or(0)
}

/// A drop shadow is CSS `box-shadow`: cast outside the surface and
/// never drawn under it, whatever the fill's alpha. Through 0.8 it was
/// painted under the whole silhouette and the fill composited over
/// it, so a fill that is translucent by design sat on a ground its own
/// shadow had darkened — a status alert's 15% tint on a white card
/// rendered `#C1C8D8`, a grey slab, where the tint is `#EDF3FF` — and
/// an outline button's unfilled interior was greyer than the card
/// under it.
#[test]
fn a_shadow_is_cast_outside_its_surface_and_never_under_it() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("shadow_render: no GPU adapter, skipping");
        return;
    };
    let white = Color::srgb_u8(255, 255, 255);
    let level = damascene_core::tokens::SHADOW_MD;
    let below = RECT_BOTTOM + 2;

    let opaque = render_surface(&device, &queue, level, white, 1.0);
    let halo = center_darkness(&opaque, below);
    assert!(halo >= 8, "the shadow under test exists ({halo})");
    assert_eq!(interior_darkness(&opaque), 0);

    let cases = [
        (
            "a half-transparent fill",
            surface_node(level).fill(white.with_alpha(0.5)),
        ),
        (
            "an outline with no fill",
            surface_node(level).stroke(Color::srgb_u8(200, 200, 200)),
        ),
    ];
    for (what, node) in cases {
        let frame = render_node(&device, &queue, node);
        assert_eq!(
            interior_darkness(&frame),
            0,
            "{what}: the page shows through it, not its own shadow"
        );
        assert_eq!(
            center_darkness(&frame, below),
            halo,
            "{what}: it casts the shadow an opaque surface casts"
        );
    }

    // On an antialiased edge the surface covers part of the pixel and
    // the shadow the rest. A rect 49 px tall centres on y 55.5..104.5,
    // so row 104 is half surface: it carries half the shadow the row
    // below it does. Masking the shadow and then compositing the fill
    // *over* it thins that half again — a quarter — which reads as a
    // light seam around every shadowed surface, opaque ones included.
    let half_row = render_node(
        &device,
        &queue,
        surface_node(level).fill(white).height(Size::Fixed(49.0)),
    );
    let (edge, outside) = (
        center_darkness(&half_row, RECT_BOTTOM) as u32,
        center_darkness(&half_row, RECT_BOTTOM + 1) as u32,
    );
    assert!(outside >= 12, "the halo under test exists ({outside})");
    assert!(
        edge * 100 >= outside * 45 && edge * 100 <= outside * 70,
        "a half-covered edge row carries half the halo (edge {edge}, row below {outside})"
    );
}

/// The widget that showed the fault. A status alert is a `Panel` — it
/// carries the panel's shadow — and its fill is a 15% status tint. In
/// a light theme it rendered as a grey slab, its text at 3.2–3.9:1 on
/// the pixels, while the contrast lint — which composites the tint
/// over the card and knows of no shadow — computed 4.8. The pixels are
/// now what the lint computes.
#[test]
fn a_status_alert_in_a_light_theme_is_its_tint_not_a_slab() {
    let Ok(gpu) = Headless::new() else {
        eprintln!("shadow_render: no GPU adapter, skipping");
        return;
    };
    let theme = Theme::damascene_light();
    let interior = |flat: bool| -> [u8; 3] {
        let mut runner = gpu.runner(1);
        runner.set_theme(theme.clone());
        let alert = alert([alert_title("Heads up"), alert_description("Body")]).info();
        let alert = if flat { alert.shadow(0.0) } else { alert };
        let tree = column([alert.width(Size::Fixed(300.0)).height(Size::Fixed(120.0))])
            .padding(tokens::SPACE_6)
            .fill(tokens::CARD)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        let (w, _, px) = gpu
            .render_rgba8(&mut runner, tree, Rect::new(0.0, 0.0, 360.0, 180.0), 1.0)
            .expect("headless render");
        // Inside the alert, right of its text.
        let at = ((100 * w + 280) * 4) as usize;
        [px[at], px[at + 1], px[at + 2]]
    };
    let drawn = interior(false);
    assert_eq!(
        drawn,
        interior(true),
        "the alert draws the same with its shadow as without: none of it is under the tint"
    );

    let luminance = |rgb: [u8; 3]| {
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
    };
    let [r, g, b, _] = theme.resolve(tokens::INFO_TINT_FOREGROUND).to_srgb_u8a();
    let (text, ground) = (luminance([r, g, b]), luminance(drawn));
    let ratio = (text.max(ground) + 0.05) / (text.min(ground) + 0.05);
    assert!(
        ratio >= 4.5,
        "info text on the alert as drawn is {ratio:.2}:1 (ground {drawn:?})"
    );
}
