//! Side-by-side compare two text-quality PNGs and produce a sheet PNG
//! plus a diff-stats markdown report.
//!
//! Usage: `cargo run -p damascene-tools --bin compare_text_quality_images -- \
//!         --before=<path> --after=<path> --out=<sheet path> [--report=<md>]`
//!
//! Defaults to `text_quality.before.1x.png` vs `text_quality.after.1x.png`
//! in `crates/damascene-wgpu/out`. The sheet draws `before | after` with a
//! 1-pixel divider; the report enumerates pixel-level diff stats.

use std::path::{Path, PathBuf};

#[derive(Clone)]
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct DiffStats {
    pixel_count: u64,
    exact_pixels: u64,
    sum_abs_rgb: u64,
    sum_sq_rgb: u64,
    max_channel_abs: u8,
    pixels_over_8: u64,
    pixels_over_32: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = workspace_root();
    let mut before = workspace.join("crates/damascene-wgpu/out/text_quality.before.1x.png");
    let mut after = workspace.join("crates/damascene-wgpu/out/text_quality.after.1x.png");
    let mut out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out/text_quality.compare.png");
    let mut report: Option<PathBuf> = None;

    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--before=") {
            before = PathBuf::from(v);
        } else if let Some(v) = arg.strip_prefix("--after=") {
            after = PathBuf::from(v);
        } else if let Some(v) = arg.strip_prefix("--out=") {
            out = PathBuf::from(v);
        } else if let Some(v) = arg.strip_prefix("--report=") {
            report = Some(PathBuf::from(v));
        } else {
            return Err(format!("unknown arg: {arg}").into());
        }
    }

    let a = read_png(&before)?;
    let b = read_png(&after)?;
    if a.width != b.width || a.height != b.height {
        return Err(format!(
            "dimensions differ: before={}x{}, after={}x{}",
            a.width, a.height, b.width, b.height
        )
        .into());
    }

    let stats = diff_stats(&a, &b);
    let sheet = side_by_side(&a, &b);
    write_png(&out, sheet.width, sheet.height, &sheet.rgba)?;
    println!("wrote {}", out.display());
    println!(
        "pixels={} exact={} ({:.1}%) max_abs={} >8={} >32={} mean={:.3} rmse={:.3}",
        stats.pixel_count,
        stats.exact_pixels,
        100.0 * stats.exact_pixels as f64 / stats.pixel_count as f64,
        stats.max_channel_abs,
        stats.pixels_over_8,
        stats.pixels_over_32,
        stats.sum_abs_rgb as f64 / (stats.pixel_count * 3) as f64,
        ((stats.sum_sq_rgb as f64) / (stats.pixel_count * 3) as f64).sqrt(),
    );

    if let Some(report_path) = report {
        let md = format!(
            "# text_quality diff\n\n\
             - before: `{}`\n\
             - after:  `{}`\n\n\
             | metric | value |\n\
             |---|---|\n\
             | total pixels | {} |\n\
             | exact-match pixels | {} ({:.2}%) |\n\
             | max channel abs | {} |\n\
             | pixels with any channel > 8 | {} |\n\
             | pixels with any channel > 32 | {} |\n\
             | mean abs RGB | {:.3} |\n\
             | RGB RMSE | {:.3} |\n",
            before.display(),
            after.display(),
            stats.pixel_count,
            stats.exact_pixels,
            100.0 * stats.exact_pixels as f64 / stats.pixel_count as f64,
            stats.max_channel_abs,
            stats.pixels_over_8,
            stats.pixels_over_32,
            stats.sum_abs_rgb as f64 / (stats.pixel_count * 3) as f64,
            ((stats.sum_sq_rgb as f64) / (stats.pixel_count * 3) as f64).sqrt(),
        );
        std::fs::write(&report_path, md)?;
        println!("wrote {}", report_path.display());
    }

    Ok(())
}

fn read_png(path: &Path) -> Result<Image, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
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

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools has a parent")
        .to_path_buf()
}

fn diff_stats(a: &Image, b: &Image) -> DiffStats {
    let mut s = DiffStats {
        pixel_count: (a.width as u64) * (a.height as u64),
        ..DiffStats::default()
    };
    for (pa, pb) in a
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.rgba.as_chunks::<4>().0.iter())
    {
        if pa == pb {
            s.exact_pixels += 1;
            continue;
        }
        let mut max_d: u8 = 0;
        for i in 0..3 {
            let d = pa[i].abs_diff(pb[i]);
            s.sum_abs_rgb += d as u64;
            s.sum_sq_rgb += (d as u64) * (d as u64);
            max_d = max_d.max(d);
        }
        s.max_channel_abs = s.max_channel_abs.max(max_d);
        if max_d > 8 {
            s.pixels_over_8 += 1;
        }
        if max_d > 32 {
            s.pixels_over_32 += 1;
        }
    }
    s
}

fn side_by_side(a: &Image, b: &Image) -> Image {
    let divider: u32 = 1;
    let w = a.width + divider + b.width;
    let h = a.height.max(b.height);
    let mut rgba = vec![20u8; (w * h * 4) as usize];
    blit(&mut rgba, w, 0, 0, a);
    blit(&mut rgba, w, a.width + divider, 0, b);
    Image {
        width: w,
        height: h,
        rgba,
    }
}

fn blit(dst: &mut [u8], dst_w: u32, x0: u32, y0: u32, src: &Image) {
    let row_bytes = src.width as usize * 4;
    for y in 0..src.height {
        let dst_off = ((y0 + y) as usize * dst_w as usize + x0 as usize) * 4;
        let src_off = y as usize * row_bytes;
        dst[dst_off..dst_off + row_bytes].copy_from_slice(&src.rgba[src_off..src_off + row_bytes]);
    }
}
