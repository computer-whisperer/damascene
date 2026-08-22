//! Build a single PNG contact sheet from rendered calibration PNGs.
//!
//! Usage:
//! `cargo run -p damascene-tools --bin make_calibration_sheet`
//!
//! Reads `crates/damascene-core/out/*_calibration.png` and writes
//! `crates/damascene-core/out/calibration_sheet.png`. If shadcn reference
//! captures exist under `references/shadcn-calibration/out`, also writes
//! `crates/damascene-core/out/reference_calibration_sheet.png` and the shadcn
//! viewport/UI-scale matrix `reference_scale_matrix_sheet.png`.

use std::path::{Path, PathBuf};

const GUTTER: u32 = 24;
const COLUMNS: usize = 2;
const BG: [u8; 4] = [11, 15, 23, 255];

#[derive(Clone)]
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let out_dir = root.join("crates/damascene-core/out");
    let mut paths = calibration_pngs(&out_dir)?;
    if paths.is_empty() {
        return Err(format!("no calibration PNGs found in {}", out_dir.display()).into());
    }
    paths.sort();

    let images: Vec<Image> = paths
        .iter()
        .map(|path| read_png(path).map(downsample_2x))
        .collect::<Result<_, _>>()?;
    let sheet = contact_sheet(&images, COLUMNS);
    let out = out_dir.join("calibration_sheet.png");
    write_png(&out, sheet.width, sheet.height, &sheet.rgba)?;

    println!("wrote {}", out.display());
    for path in paths {
        println!("  {}", path.display());
    }

    if let Some(reference_sheet) = reference_comparison_sheet(&root, &out_dir)? {
        let out = out_dir.join("reference_calibration_sheet.png");
        write_png(
            &out,
            reference_sheet.width,
            reference_sheet.height,
            &reference_sheet.rgba,
        )?;
        println!("wrote {}", out.display());
    }
    if let Some(matrix_sheet) = reference_scale_matrix_sheet(&root, &out_dir)? {
        let out = out_dir.join("reference_scale_matrix_sheet.png");
        write_png(
            &out,
            matrix_sheet.width,
            matrix_sheet.height,
            &matrix_sheet.rgba,
        )?;
        println!("wrote {}", out.display());
    }
    if let Some(density_sheet) = reference_density_matrix_sheet(&root, &out_dir)? {
        let out = out_dir.join("reference_density_matrix_sheet.png");
        write_png(
            &out,
            density_sheet.width,
            density_sheet.height,
            &density_sheet.rgba,
        )?;
        println!("wrote {}", out.display());
    }
    Ok(())
}

fn calibration_pngs(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if file_name.ends_with("_calibration.png") {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn downsample_2x(image: Image) -> Image {
    let width = image.width / 2;
    let height = image.height / 2;
    let mut rgba = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let src = (((y * 2) * image.width + (x * 2)) * 4) as usize;
            let dst = ((y * width + x) * 4) as usize;
            rgba[dst..dst + 4].copy_from_slice(&image.rgba[src..src + 4]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

fn normalize_for_sheet(image: Image) -> Image {
    if image.width >= 1400 && image.width.is_multiple_of(2) && image.height.is_multiple_of(2) {
        downsample_2x(image)
    } else {
        image
    }
}

fn reference_comparison_sheet(
    root: &Path,
    damascene_out_dir: &Path,
) -> Result<Option<Image>, Box<dyn std::error::Error>> {
    let reference_out_dir = root.join("references/shadcn-calibration/out");
    let pairs = [
        (
            reference_out_dir.join("shadcn-calibration.png"),
            damascene_out_dir.join("polish_calibration.png"),
        ),
        (
            reference_out_dir.join("shadcn-dashboard-01.png"),
            damascene_out_dir.join("dashboard_01_calibration.png"),
        ),
        (
            reference_out_dir.join("shadcn-settings-01.png"),
            damascene_out_dir.join("settings_calibration.png"),
        ),
    ];

    if !pairs
        .iter()
        .all(|(reference, damascene)| reference.exists() && damascene.exists())
    {
        return Ok(None);
    }

    let mut images = Vec::new();
    for (reference, damascene) in pairs {
        images.push(normalize_for_sheet(read_png(&reference)?));
        images.push(normalize_for_sheet(read_png(&damascene)?));
    }

    Ok(Some(contact_sheet(&images, 2)))
}

fn reference_scale_matrix_sheet(
    root: &Path,
    damascene_out_dir: &Path,
) -> Result<Option<Image>, Box<dyn std::error::Error>> {
    let reference_out_dir = root.join("references/shadcn-calibration/out");
    let rows = [
        ("shadcn-calibration", "polish_calibration.png"),
        ("shadcn-dashboard-01", "dashboard_01_calibration.png"),
        ("shadcn-settings-01", "settings_calibration.png"),
    ];

    let mut images = Vec::new();
    for (reference_slug, damascene_name) in rows {
        let shadcn_default = reference_out_dir.join(format!("{reference_slug}.png"));
        let shadcn_compact = reference_out_dir.join(format!("{reference_slug}.compact.png"));
        let shadcn_desktop = reference_out_dir.join(format!("{reference_slug}.desktop.png"));
        let damascene = damascene_out_dir.join(damascene_name);
        if !shadcn_default.exists()
            || !shadcn_compact.exists()
            || !shadcn_desktop.exists()
            || !damascene.exists()
        {
            return Ok(None);
        }

        images.push(read_png(&shadcn_default)?);
        images.push(read_png(&shadcn_compact)?);
        images.push(read_png(&shadcn_desktop)?);
        images.push(normalize_for_sheet(read_png(&damascene)?));
    }

    Ok(Some(contact_sheet(&images, 4)))
}

fn reference_density_matrix_sheet(
    root: &Path,
    damascene_out_dir: &Path,
) -> Result<Option<Image>, Box<dyn std::error::Error>> {
    let reference_out_dir = root.join("references/shadcn-calibration/out");
    let rows = [
        ("shadcn-calibration", "polish_calibration"),
        ("shadcn-dashboard-01", "dashboard_01_calibration"),
        ("shadcn-settings-01", "settings_calibration"),
    ];

    let mut images = Vec::new();
    for (reference_slug, damascene_slug) in rows {
        let shadcn_compact =
            reference_out_dir.join(format!("{reference_slug}.density-compact.png"));
        let shadcn_comfortable = reference_out_dir.join(format!("{reference_slug}.png"));
        let shadcn_spacious =
            reference_out_dir.join(format!("{reference_slug}.density-spacious.png"));
        let damascene_compact = damascene_out_dir.join(format!("{damascene_slug}.compact.png"));
        let damascene_comfortable =
            damascene_out_dir.join(format!("{damascene_slug}.comfortable.png"));
        let damascene_spacious = damascene_out_dir.join(format!("{damascene_slug}.spacious.png"));
        if !shadcn_compact.exists()
            || !shadcn_comfortable.exists()
            || !shadcn_spacious.exists()
            || !damascene_compact.exists()
            || !damascene_comfortable.exists()
            || !damascene_spacious.exists()
        {
            return Ok(None);
        }

        images.push(read_png(&shadcn_compact)?);
        images.push(normalize_for_sheet(read_png(&damascene_compact)?));
        images.push(read_png(&shadcn_comfortable)?);
        images.push(normalize_for_sheet(read_png(&damascene_comfortable)?));
        images.push(read_png(&shadcn_spacious)?);
        images.push(normalize_for_sheet(read_png(&damascene_spacious)?));
    }

    Ok(Some(contact_sheet(&images, 6)))
}

fn contact_sheet(images: &[Image], columns: usize) -> Image {
    let columns = columns.max(1).min(images.len());
    let rows = images.len().div_ceil(columns);
    let cell_w = images.iter().map(|img| img.width).max().unwrap_or(1);
    let cell_h = images.iter().map(|img| img.height).max().unwrap_or(1);
    let width = columns as u32 * cell_w + (columns as u32 + 1) * GUTTER;
    let height = rows as u32 * cell_h + (rows as u32 + 1) * GUTTER;
    let mut rgba = vec![0; (width * height * 4) as usize];
    for px in rgba.as_chunks_mut::<4>().0 {
        px.copy_from_slice(&BG);
    }

    for (i, img) in images.iter().enumerate() {
        let col = (i % columns) as u32;
        let row = (i / columns) as u32;
        let x = GUTTER + col * (cell_w + GUTTER) + (cell_w - img.width) / 2;
        let y = GUTTER + row * (cell_h + GUTTER) + (cell_h - img.height) / 2;
        blit(&mut rgba, width, img, x, y);
    }

    Image {
        width,
        height,
        rgba,
    }
}

fn blit(dst: &mut [u8], dst_w: u32, src: &Image, x0: u32, y0: u32) {
    for y in 0..src.height {
        let dst_start = (((y0 + y) * dst_w + x0) * 4) as usize;
        let src_start = (y * src.width * 4) as usize;
        let len = (src.width * 4) as usize;
        dst[dst_start..dst_start + len].copy_from_slice(&src.rgba[src_start..src_start + len]);
    }
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
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "unsupported PNG format for {}: {:?} {:?}",
            path.display(),
            info.color_type,
            info.bit_depth
        )
        .into());
    }
    let bytes = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for rgb in bytes.as_chunks::<3>().0 {
                out.extend_from_slice(rgb);
                out.push(255);
            }
            out
        }
        _ => {
            return Err(format!(
                "unsupported PNG format for {}: {:?} {:?}",
                path.display(),
                info.color_type,
                info.bit_depth
            )
            .into());
        }
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
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
