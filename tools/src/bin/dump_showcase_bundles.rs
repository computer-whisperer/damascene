//! Dump bundle artifacts (SVG, tree dump, draw_ops, lint, manifest)
//! for every section of the Showcase fixture. One-shot diagnostic
//! used to validate layout intent — the SVG and tree dump together
//! make layout regressions obvious without needing a window.
//!
//! Usage: `cargo run -p damascene-tools --bin dump_showcase_bundles`
//!
//! Output: `crates/damascene-fixtures/out/showcase_<section>.*` plus
//! named stateful scenes such as open overlay menus.

use std::path::PathBuf;

use damascene_core::prelude::{Rect, render_bundle, write_bundle};
use damascene_core::{App, BuildCx};
use damascene_fixtures::{Showcase, showcase::Section};

/// Viewport sizes the bundle pass renders every scene at. Desktop
/// matches the windowed showcase; phone matches a typical Android
/// device's logical width and roughly 16:9 portrait height. Both
/// shapes feed `BuildCx::with_viewport` so the showcase shell picks
/// its desktop or phone branch the same way it would in a real host.
const DESKTOP_VIEWPORT: (f32, f32) = (900.0, 640.0);
const PHONE_VIEWPORT: (f32, f32) = (360.0, 780.0);

fn main() -> std::io::Result<()> {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/damascene-fixtures/out");
    let mut total_findings = 0;

    for scene in showcase_bundle_scenes() {
        for variant in [
            ViewportVariant {
                suffix: "",
                size: DESKTOP_VIEWPORT,
            },
            ViewportVariant {
                suffix: ".phone",
                size: PHONE_VIEWPORT,
            },
        ] {
            // Each variant gets its own Showcase clone-equivalent: we
            // rebuild from the scene factory rather than mutate one
            // app, so per-section state (toasts queued, dropdown open,
            // etc.) starts from the same baseline at each viewport.
            let mut app = (scene.factory)();
            app.before_build();
            let theme = app.theme();
            let cx = BuildCx::new(&theme).with_viewport(variant.size.0, variant.size.1);
            let mut tree = app.build(&cx);

            let viewport = Rect::new(0.0, 0.0, variant.size.0, variant.size.1);
            let bundle = render_bundle(&mut tree, viewport);

            let name = format!("{}{}", scene.name, variant.suffix);
            let written = write_bundle(&bundle, &out_dir, &name)?;
            for p in &written {
                println!("wrote {}", p.display());
            }
            if !bundle.lint.findings.is_empty() {
                eprintln!("\n[{name}] lint findings ({}):", bundle.lint.findings.len());
                eprint!("{}", bundle.lint.text());
                total_findings += bundle.lint.findings.len();
            }
        }
    }

    if total_findings > 0 {
        eprintln!("\nshowcase bundle lint reported {total_findings} finding(s)");
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Clone, Copy)]
struct ViewportVariant {
    /// Filename suffix appended after the scene name. Empty for the
    /// default desktop variant so existing artifacts keep their names.
    suffix: &'static str,
    /// Logical-pixel viewport dimensions (width, height).
    size: (f32, f32),
}

struct ShowcaseBundleScene {
    name: String,
    /// Builds a fresh Showcase per render. We rebuild rather than
    /// `Clone` because some scenes carry state we mutate inline (e.g.
    /// the open-overlay variants), and each viewport variant should
    /// see the same baseline.
    factory: Box<dyn Fn() -> Showcase>,
}

fn showcase_bundle_scenes() -> Vec<ShowcaseBundleScene> {
    let mut scenes: Vec<ShowcaseBundleScene> = Section::ALL
        .into_iter()
        .map(|section| ShowcaseBundleScene {
            name: format!("showcase_{}", section.slug()),
            factory: Box::new(move || Showcase::with_section(section)),
        })
        .collect();

    scenes.push(ShowcaseBundleScene {
        name: "showcase_overlays_dropdown".into(),
        factory: Box::new(Showcase::with_overlay_dropdown_open),
    });

    scenes.push(ShowcaseBundleScene {
        name: "showcase_overlays_context_menu".into(),
        factory: Box::new(|| Showcase::with_overlay_context_menu_at(560.0, 450.0)),
    });

    scenes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showcase_bundle_scenes_include_open_overlay_menus() {
        let names = showcase_bundle_scenes()
            .into_iter()
            .map(|scene| scene.name)
            .collect::<Vec<_>>();

        assert!(
            names
                .iter()
                .any(|name| name == "showcase_overlays_dropdown")
        );
        assert!(
            names
                .iter()
                .any(|name| name == "showcase_overlays_context_menu")
        );
    }
}
