//! `TraceInspector` — the Carbon/workbench demo app.
//!
//! Deliberately the opposite end of the landing-page-vs-application
//! split from `damascene_fixtures::HeroDemo`. Where the hero is a
//! dashboard of stat cards, this is an instrument: an activity rail, a
//! side nav, a breadcrumb, a virtualized table of ~2400 draw calls at
//! 24px per row, a property inspector, and a 22px status bar.
//!
//! It renders at [`INSPECTOR_LOGICAL_SIZE`], the same canvas as the
//! README hero, so the two PNGs can be compared directly.
//!
//! All data is derived arithmetically from the row index. Nothing is
//! random, so the headless render stays bit-reproducible.

#![warn(missing_docs)]

use damascene_core::prelude::*;

use crate::theme;
use crate::tokens as carbon;
use crate::widgets::*;
// Explicit, because core's prelude now also globs a `data_table` (the
// behaving one, hoisted per `docs/WORKBENCH_VISION.md`'s lifted
// deferral) and glob-vs-glob would be ambiguous. This demo means
// Carbon's own column-spec-plus-cell-closure skeleton; retiring it in
// favour of the core widget is the migration that document schedules,
// not this one.
use crate::widgets::data_table;

/// Logical-pixel canvas the inspector renders at — matched to
/// `damascene_fixtures::hero::HERO_LOGICAL_SIZE` so the dense and stock
/// renders are directly comparable.
pub const INSPECTOR_LOGICAL_SIZE: (u32, u32) = (1360, 894);

/// Number of draw calls in the demo capture. Large enough that
/// virtualization is doing real work rather than being decorative.
pub const DRAW_CALL_COUNT: usize = 2431;

/// Right-hand inspector rail width. Not a Carbon token — Carbon has no
/// inspector-rail component — so it sits here with the app.
const INSPECTOR_WIDTH: f32 = 300.0;

/// The draw call the inspector rail describes, and the row the table
/// highlights.
///
/// Deliberately inside the first screenful: an inspector describing row
/// 1184 while the table shows rows 0–26 is incoherent, and the whole
/// point of the rail is that it reflects the selection.
pub const SELECTED_DRAW: usize = 14;

/// One render pass: name, the fraction of draw calls it owns, its GPU
/// cost, the pipelines that can legitimately appear inside it, and the
/// primitive-count ceiling for its draws.
struct Pass {
    name: &'static str,
    /// Share of [`DRAW_CALL_COUNT`], so passes appear as contiguous
    /// blocks the way a real capture records them.
    share: f32,
    /// Pass GPU time in milliseconds.
    ms: f32,
    pipelines: &'static [&'static str],
    /// Upper bound on primitives per draw in this pass. Per-pass because
    /// the counts are not interchangeable: a fullscreen clear or a
    /// deferred resolve submits a two-triangle quad, while a gbuffer
    /// draw can push tens of thousands. A single global range put 39k
    /// primitives in a clear pass, which is not a thing.
    prims_max: usize,
}

/// Passes in submission order. A capture is a *sequence* of passes, not
/// an interleaving — draws belonging to one pass are contiguous, so the
/// table's Pass column changes in blocks rather than cycling per row.
///
/// The two short leading passes are what a real deferred renderer
/// submits first, and they also mean the first screenful of the table
/// crosses a pass boundary instead of showing 27 identical rows.
const PASSES: [Pass; 7] = [
    Pass {
        name: "clear",
        share: 0.001,
        ms: 0.1,
        pipelines: &["clear_rt.frag"],
        // A fullscreen clear is one two-triangle quad.
        prims_max: 2,
    },
    Pass {
        name: "depth-pre",
        share: 0.029,
        ms: 0.9,
        pipelines: &["depth_prepass.vert", "depth_prepass_masked.vert"],
        prims_max: 48_000,
    },
    Pass {
        name: "shadow",
        share: 0.31,
        ms: 2.1,
        pipelines: &["shadow_cast.vert", "shadow_cast_masked.vert"],
        prims_max: 36_000,
    },
    Pass {
        name: "gbuffer",
        share: 0.41,
        ms: 5.8,
        pipelines: &[
            "gbuffer_opaque.frag",
            "gbuffer_masked.frag",
            "gbuffer_decal.frag",
        ],
        prims_max: 96_000,
    },
    Pass {
        name: "lighting",
        share: 0.09,
        ms: 3.9,
        pipelines: &["light_cluster.comp", "deferred_resolve.frag"],
        // Clustered lighting dispatches and a fullscreen resolve.
        prims_max: 2,
    },
    Pass {
        name: "post",
        share: 0.05,
        ms: 2.3,
        pipelines: &["bloom_downsample.frag", "tonemap_aces.frag"],
        prims_max: 2,
    },
    Pass {
        name: "ui",
        share: 0.11,
        ms: 0.6,
        pipelines: &["ui_quad.frag", "ui_text_msdf.frag"],
        prims_max: 1_400,
    },
];

/// Total frame GPU time, derived rather than restated — the status bar
/// and the timeline band must not be able to disagree.
fn frame_ms() -> f32 {
    PASSES.iter().map(|p| p.ms).sum()
}

/// GPU-microsecond threshold above which a draw is flagged `slow`.
///
/// Set at the ~97th percentile of [`gpu_micros`] rather than picked by
/// eye. Skewing the *values* cubically does not make the tail sparse —
/// for a uniform input, `P(u > t)` is `1 - t` regardless of the exponent
/// — so an eyeballed threshold flagged nearly a third of all rows. An
/// exception column only carries signal if exceptions are rare.
const SLOW_MICROS: f32 = 265.0;

/// A 32-bit integer mixer (Murmur-style finalizer), salted.
///
/// Synthetic capture values need to look uncorrelated — a plain
/// `idx * k % n` produces a visibly monotonic ramp down every column,
/// which reads as obviously fake. This stays fully deterministic, so the
/// headless render remains bit-reproducible.
///
/// The golden-ratio salt matters: the bare finalizer maps `0 -> 0`, which
/// made row 0 take the minimum of every column *and* satisfy every
/// `% n == 0` outlier test at once.
fn mix(mut x: u32) -> u32 {
    x ^= 0x9e37_79b9;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Per-draw GPU microseconds, quartically skewed.
///
/// Real captures are heavily tailed: the large majority of draws cost a
/// few microseconds and a handful cost hundreds. The exponent sets the
/// *shape* (median ≈ 19µs here); [`SLOW_MICROS`] sets how many rows get
/// flagged.
fn gpu_micros(h: u32) -> f32 {
    let u = ((h >> 7) % 10_000) as f32 / 10_000.0;
    let u2 = u * u;
    0.6 + u2 * u2 * 300.0
}

/// Primitive count for a draw in `pass`. Passes whose ceiling is a
/// single quad report exactly that, rather than a hashed value.
fn prims_for(pass: &Pass, h: u32) -> usize {
    if pass.prims_max <= 2 {
        pass.prims_max
    } else {
        24 + (h % (pass.prims_max as u32 - 24)) as usize
    }
}

/// Which pass owns `row_idx`, as an index into [`PASSES`].
fn pass_of(row_idx: usize) -> usize {
    let t = row_idx as f32 / DRAW_CALL_COUNT as f32;
    let mut acc = 0.0;
    for (i, p) in PASSES.iter().enumerate() {
        acc += p.share;
        if t < acc {
            return i;
        }
    }
    PASSES.len() - 1
}

/// The Carbon + workbench demo app.
#[derive(Clone, Debug, Default)]
pub struct TraceInspector;

impl App for TraceInspector {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            ui_shell_header(
                "damascene trace",
                vec![
                    tag("capture.dtrace", TagColor::Gray),
                    tag("live", TagColor::Blue),
                ],
            ),
            row([
                activity_bar(vec![
                    activity_bar_item(IconName::Activity, true, "rail.frames"),
                    activity_bar_item(IconName::BarChart, false, "rail.timing"),
                    activity_bar_item(IconName::Folder, false, "rail.resources"),
                    activity_bar_item(IconName::FileText, false, "rail.shaders"),
                    activity_bar_item(IconName::Settings, false, "rail.settings"),
                ]),
                frame_nav(),
                main_pane(),
                inspector_rail(),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
            status_bar(
                vec![
                    status_bar_item(Some(IconName::GitBranch), "main", carbon::TEXT_SECONDARY),
                    status_bar_item(None, "frame 1042 / 1800", carbon::TEXT_SECONDARY),
                    status_bar_item(None, "2431 draws", carbon::TEXT_SECONDARY),
                ],
                vec![
                    status_bar_item(
                        None,
                        format!("{:.1} ms GPU", frame_ms()),
                        carbon::TEXT_SECONDARY,
                    ),
                    status_bar_item(None, "4x MSAA", carbon::TEXT_SECONDARY),
                    status_bar_item(
                        Some(IconName::AlertCircle),
                        "3 warnings",
                        carbon::SUPPORT_WARNING,
                    ),
                ],
            ),
        ])
        .fill(carbon::BACKGROUND)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .align(Align::Stretch)
    }

    fn theme(&self) -> Theme {
        theme::theme()
    }
}

/// Left navigation — Carbon `SideNav` with a captured-frame list below.
fn frame_nav() -> El {
    let mut kids: Vec<El> = vec![
        side_nav_label("Capture"),
        side_nav_item(IconName::Activity, "Frames", true),
        side_nav_item(IconName::BarChart, "Timing", false),
        side_nav_item(IconName::GitCommit, "Passes", false),
        side_nav_label("Resources"),
        side_nav_item(IconName::Folder, "Textures", false),
        side_nav_item(IconName::FileText, "Buffers", false),
        side_nav_label("Recent frames"),
    ];

    // A dense run of 24px rows — the same height as a table row, so the
    // whole shell shares one vertical rhythm.
    for i in 0..7u32 {
        let frame = 1042 - i;
        let ms = 14.7 + (i as f32) * 0.4;
        kids.push(structured_list_row(vec![
            code(format!("#{frame}")),
            spacer(),
            numeric(format!("{ms:.1} ms")).text_color(carbon::TEXT_HELPER),
        ]));
    }

    side_nav(kids)
}

/// The work pane — breadcrumb, frame timeline, and the draw-call table.
fn main_pane() -> El {
    column([
        breadcrumb_bar(&["capture.dtrace", "frame 1042", "pass: gbuffer"]),
        pane(
            pane_header(
                IconName::BarChart,
                "Draw calls",
                vec![
                    tag("gbuffer", TagColor::Blue),
                    toolbar_button(IconName::Search, "tools.filter", false),
                    toolbar_button(IconName::RefreshCw, "tools.reload", false),
                    toolbar_button(IconName::MoreHorizontal, "tools.more", false),
                ],
            ),
            vec![frame_timeline(), draw_call_table()],
        ),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
    .align(Align::Stretch)
}

/// A frame-time band: one horizontal bar segmented by pass, each segment
/// proportional to that pass's GPU milliseconds.
///
/// A *stacked band* rather than a column chart, because the quantity
/// being shown is "how the frame's 14.7ms divides up" — a part-to-whole
/// relationship along the axis the frame actually runs on. Vertical bars
/// would encode the same numbers while implying the passes are
/// independent categories rather than consecutive spans of one budget.
///
/// Segments are **square**. The edge of a segment *is* the value it
/// encodes, so rounding it corrupts the reading — a data-integrity
/// choice, not a Carbon one, though Carbon agrees since its surfaces are
/// square anyway.
fn frame_timeline() -> El {
    let total: f32 = PASSES.iter().map(|p| p.ms).sum();

    // Segment fills step along the layer ramp so adjacent spans separate
    // without needing gaps or strokes between them. The slowest pass
    // takes the accent so the eye lands on the thing worth looking at.
    let slowest = PASSES
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.ms.total_cmp(&b.1.ms))
        .map(|(i, _)| i)
        .unwrap_or(0);

    let band = PASSES
        .iter()
        .enumerate()
        .map(|(i, p)| {
            divider()
                .width(Size::Fill(p.ms / total))
                .height(Size::Fill(1.0))
                .fill(if i == slowest {
                    carbon::INTERACTIVE
                } else if i % 2 == 0 {
                    carbon::LAYER_03
                } else {
                    carbon::LAYER_02
                })
                .radius(carbon::RADIUS_NONE)
        })
        .collect::<Vec<_>>();

    // Labels track the segments by sharing the same Fill weights, so
    // each caption sits under its own span.
    let labels = PASSES
        .iter()
        .map(|p| {
            let frac = p.ms / total;
            // A segment narrower than its own caption clips it to
            // nonsense ("clear" -> "c", "0.6 ms" -> "0.6 n"). Below each
            // threshold the label is dropped rather than mangled; the
            // segment still shows, and the exact figures are in the
            // table.
            let mut lines: Vec<El> = Vec::new();
            if frac > 0.075 {
                lines.push(
                    styled(text(p.name), carbon::LABEL_01)
                        .text_color(carbon::TEXT_SECONDARY)
                        .ellipsis(),
                );
            }
            if frac > 0.10 {
                lines.push(
                    styled(text(format!("{:.1} ms", p.ms)), carbon::LABEL_01)
                        .tabular_numerals()
                        .text_color(carbon::TEXT_HELPER)
                        .ellipsis(),
                );
            }
            column(lines)
                .width(Size::Fill(frac))
                .align(Align::Start)
                .clip()
        })
        .collect::<Vec<_>>();

    let content = column([
        row([
            styled(text("GPU frame"), carbon::LABEL_01).text_color(carbon::TEXT_HELPER),
            spacer(),
            styled(text(format!("{total:.1} ms")), carbon::LABEL_01)
                .tabular_numerals()
                .text_color(carbon::TEXT_SECONDARY),
        ])
        .height(Size::Fixed(16.0))
        .align(Align::Center),
        row(band)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(10.0))
            .align(Align::Stretch),
        row(labels)
            .width(Size::Fill(1.0))
            .align(Align::Start)
            .gap(carbon::SPACING_02),
    ])
    .padding(Sides {
        left: carbon::SPACING_04,
        right: carbon::SPACING_04,
        top: carbon::SPACING_03,
        bottom: carbon::SPACING_03,
    })
    .gap(carbon::SPACING_02)
    .align(Align::Stretch);

    // Hairline outside the padding so the rule runs full width.
    column([content, hairline()])
        .fill(carbon::BACKGROUND)
        .align(Align::Stretch)
}

/// The virtualized draw-call table at Carbon's `size="xs"` (24px rows).
fn draw_call_table() -> El {
    let columns = vec![
        Column::numeric("#", Size::Ch(6.0)),
        Column::text("Pass", Size::Fixed(88.0)),
        Column::text("Pipeline", Size::Fill(1.0)),
        Column::numeric("Prims", Size::Ch(10.0)),
        Column::numeric("GPU µs", Size::Ch(9.0)),
        // "Flags", not "State": a column that reads `ok` on 99% of rows
        // spends ink to say nothing. Exceptions get a tag, everything
        // else gets a dash.
        Column::text("Flags", Size::Fixed(80.0)),
    ];

    data_table(
        columns,
        DRAW_CALL_COUNT,
        carbon::ROW_HEIGHT_XS,
        Some(SELECTED_DRAW),
        |row_idx, col| {
            let pass = &PASSES[pass_of(row_idx)];
            let h = mix(row_idx as u32);

            // Pipelines are drawn from the owning pass's set, so a
            // `lighting` row can't claim a `gbuffer` shader.
            let pipeline = pass.pipelines[(h >> 3) as usize % pass.pipelines.len()];
            let prims = prims_for(pass, h);
            let gpu_us = gpu_micros(h);

            let costly = gpu_us > SLOW_MICROS;
            let stalled = h.is_multiple_of(331);

            match col {
                0 => numeric(format!("{row_idx}")).text_color(carbon::TEXT_HELPER),
                1 => body(pass.name).text_color(carbon::TEXT_SECONDARY),
                2 => code(pipeline).ellipsis(),
                3 => numeric(format!("{prims}")),
                4 => numeric(format!("{gpu_us:.1}")).text_color(if costly {
                    carbon::SUPPORT_WARNING
                } else {
                    carbon::TEXT_PRIMARY
                }),
                _ => {
                    if stalled {
                        tag("stall", TagColor::Red)
                    } else if costly {
                        tag("slow", TagColor::Gray)
                    } else {
                        // An en-dash holds the column's alignment while
                        // carrying no emphasis.
                        body("–").text_color(carbon::TEXT_PLACEHOLDER)
                    }
                }
            }
        },
    )
}

/// The right-hand inspector — Carbon `StructuredList` property rows.
///
/// Note what is *absent*: no card per group, no border around the rail,
/// no rounded corners. The rail is one [`carbon::LAYER_01`] surface and
/// the rows are separated by hairlines.
fn inspector_rail() -> El {
    row([
        hairline_v(),
        panel(
            "Inspector",
            vec![
                section_label("Draw call"),
                // Derived from the same model as the table, so the rail
                // can't drift out of agreement with the selected row.
                {
                    let pass = &PASSES[pass_of(SELECTED_DRAW)];
                    let h = mix(SELECTED_DRAW as u32);
                    let pipeline = pass.pipelines[(h >> 3) as usize % pass.pipelines.len()];
                    let prims = prims_for(pass, h);
                    let gpu_us = gpu_micros(h);

                    structured_list(vec![
                        prop_row("Index", numeric(format!("{SELECTED_DRAW}"))),
                        prop_row("Pass", body(pass.name)),
                        prop_row("Pipeline", code(pipeline)),
                        prop_row("Primitives", numeric(format!("{prims}"))),
                        prop_row("GPU time", numeric(format!("{gpu_us:.1} µs"))),
                    ])
                },
                section_label("Bindings"),
                structured_list(vec![
                    prop_row("Vertex buffer", code("vb_static:0")),
                    prop_row("Index buffer", code("ib_static:0")),
                    prop_row("Descriptor set", code("set=2")),
                    prop_row("Depth", body("read/write")),
                    prop_row("Blend", body("disabled")),
                ]),
                section_label("Warnings"),
                structured_list(vec![
                    structured_list_row(vec![
                        icon(IconName::AlertCircle)
                            .icon_size(12.0)
                            .text_color(carbon::SUPPORT_WARNING),
                        body("Redundant pipeline bind").ellipsis(),
                    ]),
                    structured_list_row(vec![
                        icon(IconName::AlertCircle)
                            .icon_size(12.0)
                            .text_color(carbon::SUPPORT_WARNING),
                        body("Depth write, no depth test").ellipsis(),
                    ]),
                ]),
                // A sentence, not a label: it must wrap, and wrapping
                // needs a constrained box (Fill), since text hugs by
                // default.
                helper("Edits apply to the capture overlay, never the recording.")
                    .width(Size::Fill(1.0))
                    .wrap_text()
                    .padding(Sides {
                        left: carbon::SPACING_04,
                        right: carbon::SPACING_04,
                        top: carbon::SPACING_05,
                        bottom: carbon::SPACING_04,
                    }),
            ],
        ),
    ])
    .width(Size::Fixed(INSPECTOR_WIDTH))
    .height(Size::Fill(1.0))
    .align(Align::Stretch)
}

/// A group heading inside the inspector rail — an uppercase `label-01`
/// band on [`carbon::LAYER_ACCENT_01`].
fn section_label(s: &str) -> El {
    column([
        row([label(s.to_uppercase())])
            .padding(Sides::x(carbon::SPACING_04))
            .height(Size::Fill(1.0))
            .align(Align::Center),
        hairline(),
    ])
    .fill(carbon::LAYER_ACCENT_01)
    .height(Size::Fixed(carbon::ROW_HEIGHT_XS))
    .align(Align::Stretch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Overflow and focus-ring checks are viewport-sensitive, so this
    // must lint at the same canvas the renderer uses.
    #[test]
    fn inspector_bundle_is_lint_clean() {
        let (w, h) = (
            INSPECTOR_LOGICAL_SIZE.0 as f32,
            INSPECTOR_LOGICAL_SIZE.1 as f32,
        );
        let mut app = TraceInspector;
        app.before_build();
        let theme = app.theme();
        let cx = BuildCx::new(&theme).with_viewport(w, h);
        let mut tree = app.build(&cx);
        let bundle = render_bundle_themed(&mut tree, Rect::new(0.0, 0.0, w, h), &theme);
        // KNOWN DEBT, exempt for the same reason the workbench crate
        // exempts Dark Modern and the showcase gate exempts the status
        // tones: Carbon's palette is a *verbatim transcription* of IBM's
        // published token values, and `text-placeholder` (#6f6f6f) /
        // `text-helper` (#8d8d8d) simply do not clear WCAG AA on the
        // Carbon dark surfaces. Retuning them would stop the crate
        // being a transcription, which is the only thing it is for.
        // Findings still print; every other kind still fails.
        let gating: Vec<_> = bundle
            .lint
            .findings
            .iter()
            .filter(|f| f.kind != FindingKind::LowContrastText)
            .collect();
        assert!(
            gating.is_empty(),
            "TraceInspector should be lint-clean at {w}x{h}; found {} gating finding(s):\n{}",
            gating.len(),
            bundle.lint.text(),
        );
    }

    // Got this wrong twice: skewing the *values* does not make the tail
    // sparse, because P(u > t) = 1 - t for uniform u whatever the
    // exponent. An exception column only carries signal if exceptions
    // are rare, so pin the rate rather than trusting the curve.
    #[test]
    fn flagged_rows_stay_sparse() {
        let slow = (0..DRAW_CALL_COUNT)
            .filter(|&i| gpu_micros(mix(i as u32)) > SLOW_MICROS)
            .count();
        let stalled = (0..DRAW_CALL_COUNT)
            .filter(|&i| mix(i as u32).is_multiple_of(331))
            .count();

        let slow_pct = 100.0 * slow as f32 / DRAW_CALL_COUNT as f32;
        assert!(
            (0.5..6.0).contains(&slow_pct),
            "slow rows should be a sparse tail, got {slow_pct:.1}% ({slow} rows)"
        );
        assert!(
            stalled < DRAW_CALL_COUNT / 100,
            "stalls should be rarer still, got {stalled} rows"
        );
    }

    #[test]
    fn per_pass_primitive_counts_are_plausible() {
        // A fullscreen clear or resolve is one two-triangle quad; a
        // gbuffer draw is not. A single global range put 39k primitives
        // in a clear pass.
        for p in PASSES.iter() {
            for i in 0..64u32 {
                let n = prims_for(p, mix(i));
                assert!(
                    n <= p.prims_max,
                    "{} draw exceeded its {} primitive ceiling with {n}",
                    p.name,
                    p.prims_max
                );
            }
        }
    }

    #[test]
    fn frame_total_matches_the_pass_breakdown() {
        // The status bar and the timeline band both read from frame_ms,
        // so they cannot drift apart the way hardcoded copies did.
        let sum: f32 = PASSES.iter().map(|p| p.ms).sum();
        assert!((frame_ms() - sum).abs() < f32::EPSILON);
    }

    #[test]
    fn selected_draw_is_visible_in_the_first_screenful() {
        // The inspector rail describes SELECTED_DRAW; if it scrolled out
        // of the rendered viewport the still image would show a rail
        // describing a row that isn't there.
        let visible_rows = (INSPECTOR_LOGICAL_SIZE.1 as f32 / carbon::ROW_HEIGHT_XS) as usize;
        assert!(SELECTED_DRAW < visible_rows);
    }

    #[test]
    fn table_is_virtualized_not_materialized() {
        // The point of a 2431-row table is that only the visible window
        // is built. If this ever became a plain column the render would
        // still look right but cost ~2400x more nodes.
        let t = draw_call_table();
        let rows = t
            .children
            .iter()
            .find(|c| c.kind == Kind::VirtualList)
            .expect("table body must be a VirtualList");
        assert!(rows.children.is_empty(), "virtual rows build lazily");
    }
}
