//! Backend-neutral hero/demo app for README screenshots.
//!
//! Unlike the exhaustive Showcase fixture, this composes a plausible app
//! surface at production density: a ground-station console for a small
//! fictional satellite constellation. The domain is chosen so the marquee
//! widgets appear doing real work — a `plot` time series for the downlink
//! window, a `chart3d` orbit view. The same `App` drives the interactive
//! `damascene-examples` binary and the headless README renderer.
//!
//! It is composed as a tool, not a dashboard: a full-bleed shell (rail,
//! main pane, inspector, edge to edge with hairlines between them),
//! regions ruled off rather than boxed, stock `sidebar` / `section` /
//! `item` rows, and state carried by dots and tabular figures before
//! words. The one card recipe left in the main pane is the pass-phase
//! pipeline, where a box is the object.
//!
//! Two standing rules keep the image honest as damascene evolves:
//!
//! 1. **No number describes damascene.** Every figure on screen belongs to
//!    the fictional mission, where it cannot mislead anyone about the
//!    library and cannot go stale.
//! 2. **Nothing is version- or date-stamped.** Clock times and countdowns
//!    are part of the fiction; calendar dates and release numbers would
//!    rot.

use std::sync::LazyLock;

use damascene_core::plot::{LegendPosition, PlotSpec, Sample, Scale, SeriesHandle, line};
use damascene_core::prelude::*;
use damascene_core::scene::glam::Vec3;
use damascene_core::scene::{
    Colormap, Focus, GridPlanes, GridSettings, LineData, LineSegment, LinesHandle, Material,
    MeshHandle, PointData, PointLabels, PointShape, PointStyle, PointsHandle, SceneSpec,
    SceneStyle,
};

use crate::showcase::scene3d::uv_sphere;

/// The Damascene badge logo — full-color parse (`SvgIcon::parse`) so
/// the gold inlay and steel ground keep their authored gradients and
/// clip regions in every theme.
const LOGO_SVG: &str = include_str!("../../../assets/damascene_badge_icon.svg");
static LOGO: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(LOGO_SVG).expect("damascene_badge_icon.svg parses"));

/// Fixed epoch for the plot's time axis so the render is deterministic.
/// The axis shows clock times, never a date.
const PASS_EPOCH: f64 = 1_780_000_000.0;

/// `(radius, inclination °)` of each orbit ring in the 3D view. Low
/// orbits hug the planet (radius 1.0), which also keeps the camera's
/// auto-framing tight.
const ORBITS: [(f32, f32); 3] = [(1.32, 18.0), (1.45, 55.0), (1.6, 82.0)];

/// `(orbit index, angle along the ring)` of each satellite marker —
/// angles chosen so no marker (or its label) transits the planet disc
/// from the default camera.
const SATS: [(usize, f32); 3] = [(0, 5.4), (1, 2.9), (2, 1.25)];

/// The README hero app: a ground-station console for the fictional
/// Meridian constellation (birds MRD-1..3; stations Northcape, Atacama,
/// Tasman). Plot series and scene geometry are built once here and only
/// referenced each frame.
pub struct HeroDemo {
    /// Per-station downlink-rate series for the pass-window plot.
    stations: Vec<(&'static str, SeriesHandle)>,
    planet: MeshHandle,
    atmosphere: MeshHandle,
    orbits: LinesHandle,
    sats: PointsHandle,
    sat_labels: PointLabels,
}

impl Default for HeroDemo {
    fn default() -> Self {
        let stations = vec![
            (
                "Northcape",
                pass_series(218.0, 9.0, 6.0, 0x9E37_79B9_7F4A_7C15),
            ),
            (
                "Atacama",
                pass_series(176.0, 20.0, 7.0, 0xD1B5_4A32_D192_ED03),
            ),
            (
                "Tasman",
                pass_series(142.0, 31.0, 5.5, 0x8CB9_2BA7_2F3D_8DD7),
            ),
        ];

        let planet = MeshHandle::new(uv_sphere(1.0, 28, 36));
        // Alpha < 1 routes through the two-pass translucent mesh path.
        let atmosphere = MeshHandle::new(uv_sphere(1.13, 20, 28));

        let mut segments = Vec::new();
        for &(radius, incl_deg) in &ORBITS {
            let n = 96;
            for s in 0..n {
                let step = std::f32::consts::TAU / n as f32;
                segments.push(LineSegment {
                    start: ring_point(radius, incl_deg, s as f32 * step),
                    end: ring_point(radius, incl_deg, (s + 1) as f32 * step),
                    color: [0.75, 0.78, 0.85, 0.35],
                });
            }
        }
        let orbits = LinesHandle::new(LineData { segments });

        // One bird per ring, colour-graded through the colormap so the
        // markers read apart at README scale.
        let positions: Vec<Vec3> = SATS
            .iter()
            .map(|&(ring, ang)| {
                let (radius, incl_deg) = ORBITS[ring];
                ring_point(radius, incl_deg, ang)
            })
            .collect();
        let sats = PointsHandle::new(PointData::from_values(
            positions,
            // Mid-to-high colormap values: the low end of Viridis is a
            // dark purple that vanishes against the planet.
            vec![0.35, 0.62, 0.88],
            (0.0, 1.0),
            Colormap::Viridis,
        ));
        let sat_labels = PointLabels::new(["MRD-1", "MRD-2", "MRD-3"]).always();

        Self {
            stations,
            planet,
            atmosphere,
            orbits,
            sats,
            sat_labels,
        }
    }
}

/// A point on an inclined circular orbit: the circle of `radius` in the
/// plane spanned by +X and a Y/Z direction tilted `incl_deg` from the
/// equator.
fn ring_point(radius: f32, incl_deg: f32, ang: f32) -> Vec3 {
    let (si, ci) = incl_deg.to_radians().sin_cos();
    let v = Vec3::new(0.0, si, ci);
    (Vec3::X * ang.cos() + v * ang.sin()) * radius
}

/// A downlink-rate curve for one ground station: a Gaussian pass profile
/// (rate climbs to `peak_mbps` at `t_peak_min` and falls away as the bird
/// sets) with small deterministic link noise. A tiny xorshift PRNG keeps
/// the fixture free of a rand dependency and identical across runs.
fn pass_series(peak_mbps: f64, t_peak_min: f64, width_min: f64, seed: u64) -> SeriesHandle {
    let mut s = seed;
    let mut rand01 = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        (s >> 11) as f64 / (1u64 << 53) as f64
    };
    let n = 480u32;
    let samples: Vec<Sample> = (0..=n)
        .map(|i| {
            let t_min = 40.0 * f64::from(i) / f64::from(n);
            let x = (t_min - t_peak_min) / width_min;
            let rate = peak_mbps * (-x * x).exp() * (1.0 + (rand01() - 0.5) * 0.06);
            Sample::new(PASS_EPOCH + t_min * 60.0, rate.max(0.0))
        })
        .collect();
    SeriesHandle::new(samples)
}

/// Logical-pixel canvas the README hero renders at. Shared with
/// `tools/src/bin/render_hero.rs` (the renderer) and the lint
/// regression test below so the asserted-clean size never drifts from
/// the shipped one.
pub const HERO_LOGICAL_SIZE: (u32, u32) = (1360, 900);

/// Horizontal inset of every region in the main pane. The regions run
/// edge to edge and rule themselves off with hairlines; this is the
/// one margin their content shares.
const PANE_INSET: f32 = tokens::SPACE_5;

impl App for HeroDemo {
    fn build(&self, _cx: &BuildCx) -> El {
        // A tool shell is full-bleed: the rail, the main pane and the
        // inspector run edge to edge and top to bottom, each carrying
        // its own padding, with a hairline between them. No `page()`
        // margin and no gap — three panels floating on a margin is a
        // dashboard. `overlays` gives the root its overlay axis and
        // stretches the row over the window.
        overlays(row([nav_rail(), self.main_pane(), inspector()]), [])
    }

    fn theme(&self) -> Theme {
        Theme::radix_slate_blue_dark()
    }
}

/// A flush pane keeps one rule, on the edge it shares with its
/// neighbour; the other three sides are the window's.
fn flush_left(pane: El) -> El {
    pane.stroke_width(0.0).border_r()
}

fn flush_right(pane: El) -> El {
    pane.stroke_width(0.0).border_l()
}

fn nav_rail() -> El {
    let nav = |icon_name: IconName, label: &'static str, key: &'static str| {
        sidebar_menu_button_with(
            label,
            false,
            SidebarMenuButtonOpts::default().icon(icon_name),
        )
        .key(key)
    };
    flush_left(
        sidebar([
            sidebar_header([row([
                icon((*LOGO).clone()).icon_size(32.0),
                column([
                    text("Damascene").title(),
                    text("Mission console").caption().muted(),
                ])
                .gap(1.0),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center)]),
            sidebar_content([
                sidebar_group([
                    sidebar_group_label("Operations"),
                    sidebar_menu([
                        sidebar_menu_button_with(
                            "Overview",
                            true,
                            SidebarMenuButtonOpts::default()
                                .icon(IconName::LayoutDashboard)
                                .trailing(badge("live").success()),
                        )
                        .key("nav:overview"),
                        nav(IconName::Activity, "Passes", "nav:passes"),
                        nav(IconName::Download, "Downlink", "nav:downlink"),
                        nav(IconName::Folder, "Archive", "nav:archive"),
                    ]),
                ]),
                sidebar_group([
                    sidebar_group_label("System"),
                    sidebar_menu([
                        nav(IconName::Bell, "Alerts", "nav:alerts"),
                        nav(IconName::Settings, "Settings", "nav:settings"),
                    ]),
                ]),
            ]),
            sidebar_footer([
                separator(),
                row([
                    icon(IconName::Activity).text_color(tokens::SUCCESS_TINT_FOREGROUND),
                    column([
                        text("Array nominal").label(),
                        text("3 birds · 3 stations").caption().muted(),
                    ])
                    .gap(1.0),
                ])
                .gap(tokens::SPACE_3)
                .align(Align::Center),
                progress_with_color(0.92, tokens::SUCCESS),
            ])
            .gap(tokens::SPACE_3),
        ])
        .width(Size::Fixed(236.0)),
    )
}

impl HeroDemo {
    fn main_pane(&self) -> El {
        column([
            top_bar(),
            row([
                metric(
                    "Downlink rate",
                    "216 Mb/s",
                    "aggregate · 3 stations",
                    0.72,
                    tokens::SUCCESS,
                    badge("nominal").success(),
                ),
                vertical_separator(),
                metric(
                    "Onboard storage",
                    "38%",
                    "flushing this pass",
                    0.38,
                    tokens::INFO,
                    badge("draining").info(),
                ),
                vertical_separator(),
                metric(
                    "Passes today",
                    "18 / 21",
                    "3 in eclipse",
                    0.86,
                    tokens::SUCCESS,
                    badge("on plan").success(),
                ),
            ])
            .border_b(),
            self.downlink_region(),
            // Fill: the bottom row takes the pane's remaining height,
            // and the orbit view takes the row's.
            row([self.orbit_region(), vertical_separator(), queue_region()])
                .height(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
    }

    /// The telemetry region: pass phases across the top, the real
    /// `plot` widget underneath — three per-station downlink humps over
    /// a shared time axis with a legend.
    fn downlink_region(&self) -> El {
        let mut spec = PlotSpec::new()
            .x(Scale::time())
            .y(Scale::linear())
            .legend(LegendPosition::TopRight)
            .crosshair(true);
        for (name, series) in &self.stations {
            spec = spec.add_mark(line(series).width(2.0).label(*name));
        }

        pane_section(section_with(
            "Downlink window",
            SectionOpts::default()
                .value("X-band · Mb/s per station")
                .trailing([badge("in pass").success()]),
            [
                row([
                    stage("Acquire", "S-band beacon", "lock 11 s", tokens::INFO),
                    connector(0.94),
                    stage("Track", "autotrack", "peak 63°", tokens::SUCCESS),
                    connector(0.88),
                    stage("Downlink", "X-band", "412 Gb queued", tokens::WARNING),
                    connector(0.76),
                    stage("Handoff", "to Atacama", "in 6 min", tokens::SUCCESS),
                ])
                .gap(tokens::SPACE_2)
                .align(Align::Center),
                plot(spec).key("hero-downlink").height(Size::Fixed(190.0)),
            ],
        ))
    }

    /// The real `chart3d` widget: planet, translucent atmosphere shell,
    /// three inclined orbit guides, and a labelled marker per bird.
    fn orbit_region(&self) -> El {
        // A clean orbital view: no reference grid, no world axes (they
        // would also inflate the auto-framing far past the constellation).
        let style = SceneStyle {
            grid: GridSettings {
                planes: GridPlanes::NONE,
                ..Default::default()
            },
            show_axes: false,
            ..Default::default()
        };
        let scene = SceneSpec::new()
            .mesh_with(
                self.planet.clone(),
                Material::Glossy {
                    base: Color::srgb_u8(96, 150, 220),
                    specular: 0.5,
                    shininess: 40.0,
                },
            )
            .mesh_with(
                self.atmosphere.clone(),
                Material::matte(Color::srgb_u8(140, 185, 255).with_alpha(0.14)),
            )
            .points_labeled(
                self.sats.clone(),
                PointStyle {
                    size: 9.0,
                    shape: PointShape::Circle,
                    ..Default::default()
                },
                self.sat_labels.clone(),
            )
            .lines(self.orbits.clone())
            .style(style)
            // Auto-framing fits the bounding sphere of everything — the
            // ring AABB's half-diagonal leaves the planet small in a wide
            // pane. An explicit viewing distance keeps it prominent while
            // the outermost ring still clears the frame.
            .focus(Focus::Point {
                target: Vec3::ZERO,
                distance: 4.6,
            });

        pane_section(section_with(
            "Orbit view",
            SectionOpts::default()
                .value("3 birds over the tracking site")
                .trailing([icon_button(IconName::RefreshCw)
                    .ghost()
                    .key("hero-orbit-reset")
                    .aria_label("Reset view")]),
            [chart3d(scene).key("hero-orbit")],
        ))
        // Fill: the section takes the row's height and hands its body
        // — the scene — what the header leaves.
        .height(Size::Fill(1.0))
        .width(Size::Fill(1.2))
    }
}

/// A `section` in the main pane: the pane's regions run edge to edge,
/// so the section's own hairline spans the pane and its content takes
/// the shared inset.
fn pane_section(section: El) -> El {
    section.px(PANE_INSET).pt(tokens::SPACE_2)
}

fn top_bar() -> El {
    row([
        text("Pass window").title(),
        text("MRD-3 over Northcape · Atacama next")
            .small()
            .muted()
            .ellipsis()
            .width(Size::Fill(1.0)),
        search_box(),
        button("Schedule pass").primary().key("hero-schedule"),
        icon_button(IconName::Ellipsis)
            .ghost()
            .key("hero-menu")
            .aria_label("More options"),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Center)
    .padding(Sides::xy(PANE_INSET, tokens::SPACE_3))
    .border_b()
}

fn search_box() -> El {
    row([
        icon(IconName::Search)
            .width(Size::Fixed(16.0))
            .height(Size::Fixed(16.0))
            .text_color(tokens::MUTED_FOREGROUND),
        text("Search passes").muted().small(),
        spacer(),
        kbd("/"),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(Sides::xy(tokens::SPACE_3, 0.0))
    .width(Size::Fixed(220.0))
    .height(Size::Fixed(tokens::CONTROL_HEIGHT))
    .radius(tokens::RADIUS_MD)
    // Input surface, not a card: the search affordance is a command-
    // palette trigger styled like a text field. Inputs use the MUTED
    // fill (matching text_input's surface) — CARD fill here read as a
    // hand-rolled card to the ReinventedWidget lint.
    .fill(tokens::MUTED)
    .stroke(tokens::BORDER)
}

/// One cell of the headline strip: what is measured, its state, the
/// reading, and how far along it is. No box — the strip's hairlines
/// are the only chrome.
fn metric(
    label: &'static str,
    value: &'static str,
    detail: &'static str,
    amount: f32,
    color: Color,
    status: El,
) -> El {
    column([
        row([text(label).caption().muted(), spacer(), status]).align(Align::Center),
        text(value).heading().tabular_numerals(),
        text(detail).small().muted(),
        progress_with_color(amount, color),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(PANE_INSET, tokens::SPACE_4))
    .width(Size::Fill(1.0))
}

fn stage(title: &'static str, subtitle: &'static str, value: &'static str, color: Color) -> El {
    // A pipeline stage is a small boxed object — the one place in the
    // main pane where a card means something.
    card([
        row([status_dot(color), text(title).label()])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
        text(subtitle).caption().muted(),
        text(value).small().semibold(),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_3)
    .radius(tokens::RADIUS_MD)
    .width(Size::Fill(1.0))
}

fn connector(amount: f32) -> El {
    column([progress(amount)])
        .justify(Justify::Center)
        .width(Size::Fixed(36.0))
        .height(Size::Fixed(58.0))
}

fn queue_region() -> El {
    let pass = |key: &'static str, pass: &'static str, detail: &'static str, status: El| {
        item([
            item_media_icon(IconName::Satellite),
            item_content([item_title(pass), item_description(detail)]),
            item_actions([status]),
        ])
        .key(key)
    };
    pane_section(section_with(
        "Pass queue",
        SectionOpts::default().count(4),
        [item_group([
            pass(
                "pass:mrd-2-atacama",
                "MRD-2 · Atacama",
                "AOS 21:18 · max elev 41°",
                badge("next").success(),
            ),
            pass(
                "pass:mrd-1-tasman",
                "MRD-1 · Tasman",
                "AOS 22:04 · max elev 78°",
                badge("queued").info(),
            ),
            pass(
                "pass:mrd-3-northcape",
                "MRD-3 · Northcape",
                "AOS 23:41 · max elev 22°",
                badge("queued").info(),
            ),
            pass(
                "pass:mrd-2-tasman",
                "MRD-2 · Tasman",
                "AOS 00:27 · max elev 64°",
                badge("held").warning(),
            ),
        ])],
    ))
    .height(Size::Fill(1.0))
}

/// The right-hand pane: the selected pass, as hairline-ruled
/// `section`s rather than a card per topic.
fn inspector() -> El {
    flush_right(
        sidebar([
            sidebar_header([
                row([text("MRD-3").title(), spacer(), badge("in pass").success()])
                    .align(Align::Center),
                text("Northcape · X-band downlink").caption().muted(),
            ]),
            // A plain column, not `sidebar_content`: that slot clips,
            // and a control flush against a clip loses the sides of
            // its focus ring. Nothing here outgrows the pane.
            column([
                section_with(
                    "Link",
                    SectionOpts::default().value("locked"),
                    [
                        reading("Elevation", "63°"),
                        reading("Slant range", "1 240 km"),
                        reading("Doppler", "−1.8 kHz"),
                        reading("Eb/N0", "9.4 dB"),
                    ],
                ),
                section_with(
                    "Ground stations",
                    SectionOpts::default().count(4),
                    [
                        station("Northcape", "autotrack", tokens::SUCCESS),
                        station("Atacama", "clock sync ±2 µs", tokens::SUCCESS),
                        station("Tasman", "rain fade 3 dB", tokens::WARNING),
                        station("Relay uplink", "S-band", tokens::SUCCESS),
                    ],
                ),
                section_with(
                    "Maneuver window",
                    SectionOpts::default().value("T−02:14:00"),
                    [
                        text(
                            "Collision-avoidance burn for MRD-2 after the Atacama handoff clears.",
                        )
                        .wrap_text()
                        .small()
                        .muted(),
                        button("View burn plan").secondary().key("hero-burn-plan"),
                    ],
                ),
                section(
                    "Shortcuts",
                    [
                        shortcut("Open palette", ["Ctrl", "K"]),
                        shortcut("Traverse focus", ["Tab"]),
                        shortcut("Dismiss overlays", ["Esc"]),
                    ],
                ),
            ])
            .gap(tokens::SPACE_2),
        ])
        .width(Size::Fixed(296.0)),
    )
}

/// A label and its reading, the reading right-aligned in tabular
/// figures so a column of them lines up.
fn reading(label: &'static str, value: &'static str) -> El {
    row([
        text(label).small().muted(),
        spacer(),
        text(value).small().tabular_numerals(),
    ])
    .align(Align::Center)
}

fn station(name: &'static str, detail: &'static str, color: Color) -> El {
    row([
        status_dot(color),
        text(name).small(),
        spacer(),
        text(detail).caption().muted(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

fn shortcut<const N: usize>(label: &'static str, keys: [&'static str; N]) -> El {
    row([
        text(label).small().muted(),
        spacer(),
        kbd_group(keys.map(kbd)),
    ])
    .align(Align::Center)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The hero exists to show the real widgets working; keep the fakes
    // from creeping back in. Exactly one `plot` (the downlink window)
    // and one `chart3d` (the orbit view) must be mounted.
    #[test]
    fn hero_mounts_the_real_widgets() {
        fn count(el: &El) -> (usize, usize) {
            let mut plots = usize::from(el.plot_source.is_some());
            let mut scenes = usize::from(el.scene_source.is_some());
            for child in &el.children {
                let (p, s) = count(child);
                plots += p;
                scenes += s;
            }
            (plots, scenes)
        }
        let app = HeroDemo::default();
        let theme = app.theme();
        let cx = BuildCx::new(&theme);
        let el = app.build(&cx);
        assert_eq!(count(&el), (1, 1), "one downlink plot and one orbit view");
    }

    // The hero must stay lint-clean at the shipped README size.
    // Renders the bundle at that size and asserts zero findings —
    // overflow checks are viewport-sensitive, so this must use the same
    // canvas as `render_hero`. Needs the bundled fonts (text shaping
    // drives layout); the crate's dev-dependency on damascene-core
    // re-enables them for the test build.
    #[test]
    fn hero_bundle_is_lint_clean() {
        let (w, h) = (HERO_LOGICAL_SIZE.0 as f32, HERO_LOGICAL_SIZE.1 as f32);
        let mut app = HeroDemo::default();
        app.before_build();
        let theme = app.theme();
        let cx = BuildCx::new(&theme).with_viewport(w, h);
        let mut tree = app.build(&cx);
        let bundle = render_bundle(&mut tree, Rect::new(0.0, 0.0, w, h));
        assert!(
            bundle.lint.findings.is_empty(),
            "HeroDemo bundle should be lint-clean at the README render size \
             ({w}x{h}); found {} finding(s):\n{}",
            bundle.lint.findings.len(),
            bundle.lint.text(),
        );
    }
}

#[cfg(test)]
mod badge_tests {
    use super::LOGO_SVG;

    /// Regression for the SVG clip-path fix: the badge clips its gold
    /// wave inlay to the letterform D with a `<clipPath>`; before the
    /// vector pipeline honoured clips, that gold geometry leaked
    /// across the whole badge. Every gold-dominant vertex must stay
    /// inside the D outline's bounds: the letterform is authored at
    /// x 168..428, y 118..394 and its group translates by (-26, 0),
    /// so it lands at x 142..402 — the clip travels with it.
    #[test]
    fn the_badge_icon_clips_its_inlay_to_the_letterform() {
        use damascene_core::vector::{VectorMeshOptions, parse_svg_asset, tessellate_vector_asset};

        let asset = parse_svg_asset(LOGO_SVG).expect("badge parses");
        assert!(asset.has_clips(), "the badge is the clip-path fixture");
        let mesh = tessellate_vector_asset(
            &asset,
            VectorMeshOptions::icon(
                damascene_core::Rect::new(0.0, 0.0, 512.0, 512.0),
                damascene_core::Color::srgb_u8(255, 255, 255),
                1.0,
                damascene_core::color::ColorSpace::SRGB_LINEAR,
            ),
        );
        assert!(!mesh.vertices.is_empty());
        let mut gold_seen = false;
        for v in &mesh.vertices {
            let [r, _, b, _] = v.color;
            let gold = r > 0.2 && r > 2.5 * b;
            if gold {
                gold_seen = true;
                let [x, y] = v.local;
                assert!(
                    (141.0..=403.0).contains(&x) && (117.0..=395.0).contains(&y),
                    "gold vertex outside the letterform: ({x}, {y})"
                );
            }
        }
        assert!(gold_seen, "the inlay should survive clipping");
    }
}
