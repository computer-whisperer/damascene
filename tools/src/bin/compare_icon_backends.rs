//! Render and compare wgpu/Vulkano icon gallery screenshots.
//!
//! Usage: `cargo run -p damascene-tools --bin compare_icon_backends`
//! Writes a markdown report and side-by-side diff sheets into
//! `crates/damascene-vulkano-demo/out/icon_backend_parity/`.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy)]
struct Material {
    arg: &'static str,
    label: &'static str,
    wgpu_file: &'static str,
    vulkano_file: &'static str,
    sheet_file: &'static str,
}

const MATERIALS: &[Material] = &[
    Material {
        arg: "flat",
        label: "Flat",
        wgpu_file: "icon_gallery.wgpu.png",
        vulkano_file: "icon_gallery.vulkano.png",
        sheet_file: "icon_gallery.flat.sheet.png",
    },
    Material {
        arg: "relief",
        label: "Relief",
        wgpu_file: "icon_gallery.relief.wgpu.png",
        vulkano_file: "icon_gallery.relief.vulkano.png",
        sheet_file: "icon_gallery.relief.sheet.png",
    },
    Material {
        arg: "glass",
        label: "Glass",
        wgpu_file: "icon_gallery.glass.wgpu.png",
        vulkano_file: "icon_gallery.glass.vulkano.png",
        sheet_file: "icon_gallery.glass.sheet.png",
    },
];

#[derive(Clone)]
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy)]
struct DiffStats {
    pixel_count: u64,
    exact_rgba_pixels: u64,
    exact_rgb_pixels: u64,
    mean_rgb_abs: f64,
    rmse_rgb: f64,
    max_channel_abs: u8,
    mean_alpha_abs: f64,
    max_alpha_abs: u8,
    alpha_changed_pixels: u64,
    pixels_over_8: u64,
    pixels_over_32: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = workspace_root();
    let wgpu_out = workspace.join("crates/damascene-wgpu/out");
    let vulkano_out = workspace.join("crates/damascene-vulkano-demo/out");
    let parity_out = vulkano_out.join("icon_backend_parity");
    std::fs::create_dir_all(&parity_out)?;

    let mut rows = Vec::new();
    for material in MATERIALS {
        render_pair(&workspace, *material)?;

        let wgpu = read_png(&wgpu_out.join(material.wgpu_file))?;
        let vulkano = read_png(&vulkano_out.join(material.vulkano_file))?;
        if wgpu.width != vulkano.width || wgpu.height != vulkano.height {
            return Err(format!(
                "{} dimensions differ: wgpu={}x{}, vulkano={}x{}",
                material.label, wgpu.width, wgpu.height, vulkano.width, vulkano.height
            )
            .into());
        }

        let stats = diff_stats(&wgpu, &vulkano);
        let sheet = side_by_side_sheet(&wgpu, &vulkano);
        write_png(
            &parity_out.join(material.sheet_file),
            sheet.width,
            sheet.height,
            &sheet.rgba,
        )?;
        rows.push((*material, stats));
    }

    let report = report_markdown(&rows);
    let report_path = parity_out.join("icon_backend_parity.md");
    std::fs::write(&report_path, report)?;
    println!("wrote {}", report_path.display());
    Ok(())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools has a parent")
        .to_path_buf()
}

fn render_pair(workspace: &Path, material: Material) -> Result<(), Box<dyn std::error::Error>> {
    run_cargo_bin(
        workspace,
        "damascene-tools",
        "render_icon_gallery",
        material.arg,
    )?;
    run_cargo_bin(
        workspace,
        "damascene-vulkano-demo",
        "render_icon_gallery",
        material.arg,
    )?;
    Ok(())
}

fn run_cargo_bin(
    workspace: &Path,
    package: &str,
    bin: &str,
    material: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(workspace)
        .args([
            "run",
            "--quiet",
            "-p",
            package,
            "--bin",
            bin,
            "--",
            &format!("--material={material}"),
        ])
        .status()?;
    if !status.success() {
        return Err(format!("renderer failed: {package}::{bin} --material={material}").into());
    }
    Ok(())
}

fn read_png(path: &Path) -> Result<Image, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path)?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info()?;
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("PNG dimensions overflow usize")?
    ];
    let info = reader.next_frame(&mut buf)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "unsupported PNG format for {}: {:?} {:?}",
            path.display(),
            info.color_type,
            info.bit_depth
        )
        .into());
    }
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba: buf[..info.buffer_size()].to_vec(),
    })
}

fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn diff_stats(a: &Image, b: &Image) -> DiffStats {
    let mut exact_rgba_pixels = 0_u64;
    let mut exact_rgb_pixels = 0_u64;
    let mut sum_abs_rgb = 0_u64;
    let mut sum_sq_rgb = 0_u64;
    let mut sum_abs_alpha = 0_u64;
    let mut max_channel_abs = 0_u8;
    let mut max_alpha_abs = 0_u8;
    let mut alpha_changed_pixels = 0_u64;
    let mut pixels_over_8 = 0_u64;
    let mut pixels_over_32 = 0_u64;

    for (pa, pb) in a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)) {
        if pa == pb {
            exact_rgba_pixels += 1;
        }
        if pa[..3] == pb[..3] {
            exact_rgb_pixels += 1;
        }
        let mut pixel_max = 0_u8;
        for channel in 0..3 {
            let diff = pa[channel].abs_diff(pb[channel]);
            sum_abs_rgb += diff as u64;
            sum_sq_rgb += (diff as u64) * (diff as u64);
            max_channel_abs = max_channel_abs.max(diff);
            pixel_max = pixel_max.max(diff);
        }
        if pixel_max > 8 {
            pixels_over_8 += 1;
        }
        if pixel_max > 32 {
            pixels_over_32 += 1;
        }
        let alpha_diff = pa[3].abs_diff(pb[3]);
        sum_abs_alpha += alpha_diff as u64;
        max_alpha_abs = max_alpha_abs.max(alpha_diff);
        if alpha_diff > 0 {
            alpha_changed_pixels += 1;
        }
    }

    let pixel_count = (a.width as u64) * (a.height as u64);
    let channel_count = (pixel_count * 3) as f64;
    DiffStats {
        pixel_count,
        exact_rgba_pixels,
        exact_rgb_pixels,
        mean_rgb_abs: sum_abs_rgb as f64 / channel_count,
        rmse_rgb: (sum_sq_rgb as f64 / channel_count).sqrt(),
        max_channel_abs,
        mean_alpha_abs: sum_abs_alpha as f64 / pixel_count as f64,
        max_alpha_abs,
        alpha_changed_pixels,
        pixels_over_8,
        pixels_over_32,
    }
}

fn side_by_side_sheet(wgpu: &Image, vulkano: &Image) -> Image {
    let width = wgpu.width * 3;
    let height = wgpu.height;
    let mut rgba = vec![0_u8; (width * height * 4) as usize];
    blit(&mut rgba, width, wgpu, 0);
    blit(&mut rgba, width, vulkano, wgpu.width);

    let diff_x = wgpu.width * 2;
    for y in 0..wgpu.height {
        for x in 0..wgpu.width {
            let src = ((y * wgpu.width + x) * 4) as usize;
            let dst = ((y * width + diff_x + x) * 4) as usize;
            for channel in 0..3 {
                let diff = wgpu.rgba[src + channel].abs_diff(vulkano.rgba[src + channel]);
                rgba[dst + channel] = diff.saturating_mul(10).saturating_add(diff.min(1) * 36);
            }
            rgba[dst + 3] = 255;
        }
    }

    Image {
        width,
        height,
        rgba,
    }
}

fn blit(dst: &mut [u8], dst_width: u32, src: &Image, dst_x: u32) {
    for y in 0..src.height {
        let src_start = (y * src.width * 4) as usize;
        let src_end = src_start + (src.width * 4) as usize;
        let dst_start = ((y * dst_width + dst_x) * 4) as usize;
        let dst_end = dst_start + (src.width * 4) as usize;
        dst[dst_start..dst_end].copy_from_slice(&src.rgba[src_start..src_end]);
    }
}

fn report_markdown(rows: &[(Material, DiffStats)]) -> String {
    let mut out = String::from(
        "# Icon Backend Parity\n\n\
         Generated by `cargo run -p damascene-tools --bin compare_icon_backends`.\n\n\
         Each sheet is laid out as: wgpu render, Vulkano render, amplified RGB diff.\n\n\
         | Material | Exact RGB | Exact RGBA | Mean RGB Abs | RMSE RGB | Max RGB Abs | Mean Alpha Abs | Max Alpha Abs | Alpha Changed | RGB Pixels > 8 | RGB Pixels > 32 | Sheet |\n\
         | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n",
    );
    for (material, stats) in rows {
        let exact_rgb_pct = stats.exact_rgb_pixels as f64 / stats.pixel_count as f64 * 100.0;
        let exact_rgba_pct = stats.exact_rgba_pixels as f64 / stats.pixel_count as f64 * 100.0;
        let alpha_changed_pct =
            stats.alpha_changed_pixels as f64 / stats.pixel_count as f64 * 100.0;
        let over_8_pct = stats.pixels_over_8 as f64 / stats.pixel_count as f64 * 100.0;
        let over_32_pct = stats.pixels_over_32 as f64 / stats.pixel_count as f64 * 100.0;
        out.push_str(&format!(
            "| {} | {:.2}% | {:.2}% | {:.3} | {:.3} | {} | {:.3} | {} | {:.2}% | {:.2}% | {:.2}% | `{}` |\n",
            material.label,
            exact_rgb_pct,
            exact_rgba_pct,
            stats.mean_rgb_abs,
            stats.rmse_rgb,
            stats.max_channel_abs,
            stats.mean_alpha_abs,
            stats.max_alpha_abs,
            alpha_changed_pct,
            over_8_pct,
            over_32_pct,
            material.sheet_file
        ));
    }
    out
}
