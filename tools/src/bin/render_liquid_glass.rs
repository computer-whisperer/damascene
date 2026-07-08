//! Headless: render a liquid-glass card overlaid on a vivid background.
//!
//! End-to-end smoke test for the backdrop-sampling pipeline:
//! `liquid_glass.wgsl` reads `@group(1)` `backdrop_tex`, blurs +
//! refracts + tints, and the runner orchestrates Pass A → snapshot →
//! Pass B around it.
//!
//! Validates the backdrop contract in `docs/SHADER_VISION.md`: a single .wgsl
//! shader, registered by a user crate via
//! `Runner::register_shader_with(name, wgsl, samples_backdrop=true)`,
//! produces a glass surface that visibly samples what was painted
//! beneath it.
//!
//! Programmatic check: pixels under the glass card (away from
//! backdrop blur dominance) should *not* match the underlying
//! background panel exactly — they should be a blend across multiple
//! panels' colors due to the blur kernel. We sample two probe points
//! inside the glass that sit over different panels and assert they
//! both differ from the corresponding background-only pixels by
//! enough to prove the glass is doing real work.
//!
//! Usage: `cargo run -p damascene-tools --bin render_liquid_glass`

use damascene_core::prelude::*;
use damascene_fixtures::showcase::LIQUID_GLASS_WGSL;
use damascene_wgpu::{MsaaTarget, Runner};

fn panel(c: Color) -> El {
    // Bare colored fill that claims its share of the row. Width=Fill
    // makes each panel one quarter of the row when there are four.
    column(Vec::<El>::new())
        .fill(c)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

fn glass_card() -> El {
    // A custom-shaded container of fixed size. The shader binding
    // sets the tint, blur, refraction, specular, and corner radius
    // through the generic vec_a/vec_b/vec_c slots.
    column(Vec::<El>::new())
        .shader(
            ShaderBinding::custom("liquid_glass")
                // tint: faint warm white, alpha = tint strength.
                .color("vec_a", Color::srgb_u8a(240, 240, 250, 110))
                // params: (blur_px, refraction, specular, _)
                .vec4("vec_b", [4.0, 0.55, 1.0, 0.0])
                // shape: (corner_radius_px, _, _, _)
                .vec4("vec_c", [28.0, 0.0, 0.0, 0.0]),
        )
        .width(Size::Fixed(360.0))
        .height(Size::Fixed(180.0))
}

fn fixture() -> El {
    stack([
        // Background — four vivid panels so blur + refraction are
        // visible in the rendered glass. `row()` defaults to
        // `height: Hug`; we want full viewport height here.
        row([
            panel(Color::srgb_u8(220, 60, 60)),
            panel(Color::srgb_u8(60, 200, 100)),
            panel(Color::srgb_u8(70, 110, 220)),
            panel(Color::srgb_u8(240, 200, 60)),
        ])
        .gap(0.0)
        .height(Size::Fill(1.0))
        // Stretch lets Fill children claim full cross-axis extent.
        // Without it, the row's default Center align collapses
        // intrinsic-zero panels to height 0.
        .align(Align::Stretch),
        // Foreground centering chrome — column claims full overlay
        // rect; the inner row also needs `Fill` height so its
        // children (spacers + glass card) actually take vertical
        // space rather than collapsing to Hug.
        column([
            spacer(),
            row([spacer(), glass_card(), spacer()]).height(Size::Hug),
            spacer(),
        ])
        .height(Size::Fill(1.0)),
    ])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let logical_width: u32 = 720;
    let logical_height: u32 = 360;
    let scale_factor: f32 = 2.0;
    let width = (logical_width as f32 * scale_factor) as u32;
    let height = (logical_height as f32 * scale_factor) as u32;
    let viewport = Rect::new(0.0, 0.0, logical_width as f32, logical_height as f32);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| {
        format!(
            "{} ({e})",
            "no compatible adapter (try installing vulkan / mesa drivers)"
        )
    })?;

    println!(
        "adapter: {:?} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damascene_wgpu::example::liquid_glass::device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))?;

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    // 4× MSAA + sample-rate shading is the new standard SDF setup —
    // see damascene_wgpu::MsaaTarget. Backdrop snapshots read from the
    // resolved single-sample texture, so the COPY_SRC flag stays on
    // the resolve target rather than the multisampled attachment.
    let sample_count = 4;
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let msaa = MsaaTarget::new(&device, format, extent, sample_count);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damascene_wgpu::example::liquid_glass::target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded_bytes_per_row = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let readback_size = (padded_bytes_per_row * height) as u64;
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("damascene_wgpu::example::liquid_glass::readback"),
        size: readback_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut renderer = Runner::with_sample_count(&device, &queue, format, sample_count);
    renderer.set_animation_mode(damascene_core::AnimationMode::Settled);
    // Register the glass shader with backdrop-sampling enabled. This
    // is the load-bearing one-line opt-in that wires the multi-pass
    // schedule + snapshot binding behind the scenes.
    renderer.register_shader_with(&device, "liquid_glass", LIQUID_GLASS_WGSL, true, false);

    let tree = fixture();
    renderer.prepare(&device, &queue, tree, viewport, scale_factor);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("damascene_wgpu::example::liquid_glass::encoder"),
    });
    // The new render() entry orchestrates Pass A → snapshot → Pass B
    // itself — the host hands over the encoder + target.
    renderer.render(
        &device,
        &mut encoder,
        &target,
        &target_view,
        Some(&msaa.view),
        wgpu::LoadOp::Clear(bg_color()),
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

    let padded = buffer_slice.get_mapped_range().unwrap();
    let mut unpadded = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + unpadded_bytes_per_row as usize;
        unpadded.extend_from_slice(&padded[start..end]);
    }
    drop(padded);
    readback_buf.unmap();

    // Probes. The glass card sits centered at ~(360, 180) logical; in
    // physical pixels that's ~(720, 360). The four background panels
    // each occupy 25% of the width: red [0..360), green [360..720),
    // blue [720..1080), yellow [1080..1440). We sample two points
    // inside the glass over different panels and confirm:
    //   1. The glass interior is not pure background (proves blur).
    //   2. The two probes differ (proves they reflect the local
    //      backdrop, not just a uniform tint).
    let probe = |x: u32, y: u32| -> (u8, u8, u8) {
        let off = (y * unpadded_bytes_per_row + x * 4) as usize;
        (unpadded[off], unpadded[off + 1], unpadded[off + 2])
    };
    // Inside the glass, near its left and right edges (over green and
    // blue panels respectively).
    let probe_l = probe(width / 2 - 200, height / 2);
    let probe_r = probe(width / 2 + 200, height / 2);
    // Background-only references at the same y but well outside the
    // glass (closer to the panel boundaries).
    let bg_green = probe((width as f32 * 0.45) as u32, height / 4);
    let bg_blue = probe((width as f32 * 0.55) as u32, 3 * height / 4);
    println!(
        "glass-l rgb={:?}  glass-r rgb={:?}  bg-green rgb={:?}  bg-blue rgb={:?}",
        probe_l, probe_r, bg_green, bg_blue
    );

    let dist = |a: (u8, u8, u8), b: (u8, u8, u8)| -> i32 {
        let dr = a.0 as i32 - b.0 as i32;
        let dg = a.1 as i32 - b.1 as i32;
        let db = a.2 as i32 - b.2 as i32;
        dr.abs() + dg.abs() + db.abs()
    };
    let l_diff_from_green = dist(probe_l, bg_green);
    let r_diff_from_blue = dist(probe_r, bg_blue);
    let l_vs_r = dist(probe_l, probe_r);
    println!(
        "‖glass-l - bg-green‖₁ = {l_diff_from_green}, \
         ‖glass-r - bg-blue‖₁ = {r_diff_from_blue}, \
         ‖glass-l - glass-r‖₁ = {l_vs_r}",
    );

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    std::fs::create_dir_all(&out_dir)?;
    let out = out_dir.join("liquid_glass.wgpu.png");
    let file = std::fs::File::create(&out)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&unpadded)?;
    println!("wrote {}", out.display());

    // Acceptance:
    //   - Each glass probe must differ from the corresponding
    //     background reference by enough that the blur + tint is
    //     visibly doing something (>10 in L1 across rgb).
    //   - The two glass probes must differ from each other (>10) so
    //     we know the glass is reading the local backdrop, not just
    //     emitting a constant tint.
    if l_diff_from_green < 10 || r_diff_from_blue < 10 {
        return Err("glass interior matches background — blur/tint not active".into());
    }
    if l_vs_r < 10 {
        return Err(
            "glass interior is uniform across left/right probes — backdrop is not being sampled"
                .into(),
        );
    }
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
