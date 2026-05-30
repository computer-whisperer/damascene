//! Compare shadcn DOM measurements against Damascene tree measurements.
//!
//! Usage:
//! `cargo run -p damascene-tools --bin make_calibration_metric_report`
//!
//! Reads shadcn capture JSON files from `references/shadcn-calibration/out`
//! and Damascene tree/draw-op artifacts from `crates/damascene-core/out`, then writes
//! `crates/damascene-core/out/reference_density_metric_diff.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
struct Metric {
    rect: Option<Rect>,
    font_size: Option<f64>,
    line_height: Option<f64>,
    font_weight: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let out_dir = root.join("crates/damascene-core/out");
    let report = density_metric_report(&root, &out_dir)?;
    let out = out_dir.join("reference_density_metric_diff.md");
    std::fs::write(&out, report)?;
    println!("wrote {}", out.display());
    Ok(())
}

fn density_metric_report(
    root: &Path,
    damascene_out_dir: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let reference_out_dir = root.join("references/shadcn-calibration/out");
    let rows = [
        ("polish", "shadcn-calibration", "polish_calibration"),
        (
            "dashboard",
            "shadcn-dashboard-01",
            "dashboard_01_calibration",
        ),
        ("settings", "shadcn-settings-01", "settings_calibration"),
    ];
    let densities = [
        ("compact", "density-compact", "compact"),
        ("comfortable", "", "comfortable"),
        ("spacious", "density-spacious", "spacious"),
    ];

    let mut report = String::new();
    writeln!(
        report,
        "# Reference Density Metric Diff\n\nValues are CSS/logical pixels. Delta is `Damascene - shadcn`.\n"
    )?;

    for (fixture_label, reference_slug, damascene_slug) in rows {
        writeln!(report, "## {fixture_label}\n")?;
        for (density_label, reference_variant, damascene_variant) in densities {
            let reference_path = if reference_variant.is_empty() {
                reference_out_dir.join(format!("{reference_slug}.json"))
            } else {
                reference_out_dir.join(format!("{reference_slug}.{reference_variant}.json"))
            };
            let tree_path =
                damascene_out_dir.join(format!("{damascene_slug}.{damascene_variant}.tree.txt"));
            let draw_path = damascene_out_dir
                .join(format!("{damascene_slug}.{damascene_variant}.draw_ops.txt"));

            if !reference_path.exists() || !tree_path.exists() || !draw_path.exists() {
                writeln!(
                    report,
                    "### {density_label}\n\n_missing artifacts for this density_\n"
                )?;
                continue;
            }

            let reference = shadcn_metrics(&reference_path)?;
            let damascene = damascene_metrics(&tree_path, &draw_path)?;
            writeln!(report, "### {density_label}\n")?;
            write_metric_table(&mut report, &reference, &damascene)?;
            writeln!(report)?;
        }
    }

    Ok(report)
}

fn write_metric_table(
    out: &mut String,
    reference: &BTreeMap<String, Metric>,
    damascene: &BTreeMap<String, Metric>,
) -> Result<(), std::fmt::Error> {
    let names: BTreeSet<&String> = reference.keys().chain(damascene.keys()).collect();
    writeln!(
        out,
        "| metric | shadcn rect | Damascene rect | delta x/y/w/h | font delta |"
    )?;
    writeln!(out, "| --- | ---: | ---: | ---: | ---: |")?;
    for name in names {
        let r = reference.get(name);
        let a = damascene.get(name);
        let rect_delta = match (r.and_then(|m| m.rect), a.and_then(|m| m.rect)) {
            (Some(r), Some(a)) => format!(
                "{:+.0}/{:+.0}/{:+.0}/{:+.0}",
                a.x - r.x,
                a.y - r.y,
                a.w - r.w,
                a.h - r.h
            ),
            _ => "n/a".to_string(),
        };
        let font_delta = match (r.and_then(|m| m.font_size), a.and_then(|m| m.font_size)) {
            (Some(r), Some(a)) => format!("{:+.1}", a - r),
            _ => "n/a".to_string(),
        };
        writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            name,
            format_metric(r),
            format_metric(a),
            rect_delta,
            font_delta
        )?;
    }
    Ok(())
}

fn format_metric(metric: Option<&Metric>) -> String {
    let Some(metric) = metric else {
        return "missing".to_string();
    };
    let rect = metric
        .rect
        .map(|r| format!("{:.0},{:.0} {:.0}x{:.0}", r.x, r.y, r.w, r.h))
        .unwrap_or_else(|| "no rect".to_string());
    let font = metric
        .font_size
        .map(|size| format!(" fs {:.0}", size))
        .unwrap_or_default();
    let line = metric
        .line_height
        .map(|height| format!(" lh {:.0}", height))
        .unwrap_or_default();
    let weight = metric
        .font_weight
        .as_deref()
        .map(|weight| format!(" wt {weight}"))
        .unwrap_or_default();
    format!("{rect}{font}{line}{weight}")
}

fn shadcn_metrics(path: &Path) -> Result<BTreeMap<String, Metric>, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let Some(measurements) = value.get("measurements").and_then(|v| v.as_object()) else {
        return Ok(BTreeMap::new());
    };

    let mut out = BTreeMap::new();
    for (name, measurement) in measurements {
        let rect = measurement.get("rect").and_then(|rect| {
            Some(Rect {
                x: rect.get("x")?.as_f64()?,
                y: rect.get("y")?.as_f64()?,
                w: rect.get("width")?.as_f64()?,
                h: rect.get("height")?.as_f64()?,
            })
        });
        out.insert(
            name.clone(),
            Metric {
                rect,
                font_size: measurement.get("fontSize").and_then(|v| v.as_f64()),
                line_height: measurement.get("lineHeight").and_then(|v| v.as_f64()),
                font_weight: measurement
                    .get("fontWeight")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            },
        );
    }
    Ok(out)
}

fn damascene_metrics(
    tree_path: &Path,
    draw_path: &Path,
) -> Result<BTreeMap<String, Metric>, Box<dyn std::error::Error>> {
    let mut out = BTreeMap::new();

    for line in std::fs::read_to_string(tree_path)?.lines() {
        let Some((node_id, rest)) = line.trim_start().split_once(" kind=") else {
            continue;
        };
        let metric_id = if node_id == "root" {
            Some("root".to_string())
        } else {
            own_metric_id(node_id)
        };
        let Some(metric_id) = metric_id else {
            continue;
        };
        let metric = out.entry(metric_id).or_insert_with(Metric::default);
        metric.rect = parse_rect(rest);
    }

    for line in std::fs::read_to_string(draw_path)?.lines() {
        if !line.starts_with("Glyph") {
            continue;
        }
        let Some(id_start) = line.find(" id=") else {
            continue;
        };
        let Some(text_start) = line[id_start + 4..].find(" text=") else {
            continue;
        };
        let node_id = &line[id_start + 4..id_start + 4 + text_start];
        let Some(metric_id) = own_metric_id(node_id) else {
            continue;
        };
        let metric = out.entry(metric_id).or_insert_with(Metric::default);
        metric.font_size = parse_named_f64(line, "size=");
        metric.font_weight = parse_named_word(line, "weight=").map(str::to_string);
    }

    Ok(out)
}

fn own_metric_id(node_id: &str) -> Option<String> {
    let open = node_id.rfind('[')?;
    let close = node_id.rfind(']')?;
    if close + 1 != node_id.len() || close <= open + 1 {
        return None;
    }
    let key = &node_id[open + 1..close];
    key.strip_prefix("metric:").map(str::to_string)
}

fn parse_rect(text: &str) -> Option<Rect> {
    let start = text.find("rect=(")? + "rect=(".len();
    let end = text[start..].find(')')? + start;
    let mut parts = text[start..end].split(',');
    Some(Rect {
        x: parts.next()?.parse().ok()?,
        y: parts.next()?.parse().ok()?,
        w: parts.next()?.parse().ok()?,
        h: parts.next()?.parse().ok()?,
    })
}

fn parse_named_f64(text: &str, name: &str) -> Option<f64> {
    let start = text.find(name)? + name.len();
    let end = text[start..]
        .find(char::is_whitespace)
        .map(|offset| start + offset)
        .unwrap_or(text.len());
    text[start..end].parse().ok()
}

fn parse_named_word<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(name)? + name.len();
    let end = text[start..]
        .find(char::is_whitespace)
        .map(|offset| start + offset)
        .unwrap_or(text.len());
    Some(&text[start..end])
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools has a parent")
        .to_path_buf()
}
