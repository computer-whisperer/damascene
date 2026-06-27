//! Dump MSDFs for a few representative built-in icons as PNGs so we
//! can eyeball whether the kurbo→fdsm path produces sensible distance
//! fields. This is a smoke harness, not a benchmark.

use std::path::PathBuf;

use damascene_core::icons::icon_vector_asset;
use damascene_core::icons::msdf::{IconMsdf, build_icon_msdf};
use damascene_core::tree::IconName;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out/icon_msdf");
    std::fs::create_dir_all(&out_dir)?;

    // 64-px icons (lucide is 24-unit) with a 6-px spread on each side.
    let px_per_unit = 64.0 / 24.0;
    let spread = 6.0;
    let stroke_width = 2.0;

    let icons = [
        ("x", IconName::X),
        ("check", IconName::Check),
        ("chevron_down", IconName::ChevronDown),
        ("chevron_right", IconName::ChevronRight),
        ("activity", IconName::Activity),
        ("info", IconName::Info),
        ("settings", IconName::Settings),
        ("bell", IconName::Bell),
        ("git_branch", IconName::GitBranch),
        ("search", IconName::Search),
    ];

    for (name, icon) in icons {
        let asset = icon_vector_asset(icon);
        // `false`: match the atlas default (error-correction off; the
        // shader's alpha-SDF fallback covers the artifacts it fixes).
        let msdf = build_icon_msdf(asset, px_per_unit, spread, stroke_width, false)
            .ok_or_else(|| format!("no MSDF for {name}"))?;

        let path = out_dir.join(format!("{name}.png"));
        write_png(&path, &msdf)?;
        println!(
            "wrote {} ({}×{}, spread={}, px_per_unit={:.2})",
            path.display(),
            msdf.width,
            msdf.height,
            msdf.spread,
            msdf.px_per_unit,
        );
    }
    println!("\nMSDFs are encoded RGB; eyeball as colour, or check the");
    println!("median channel as the SDF — interior should be bright,");
    println!("exterior dark, with a coloured fringe at edges.");
    Ok(())
}

fn write_png(path: &std::path::Path, msdf: &IconMsdf) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, msdf.width, msdf.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&msdf.rgba)?;
    Ok(())
}
