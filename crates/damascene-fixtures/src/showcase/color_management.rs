//! Color management — inspect what the host negotiated with the
//! display server.
//!
//! Reads from `BuildCx::diagnostics()` so the page is host-agnostic —
//! a backend that doesn't populate [`HostDiagnostics`] (the headless
//! render bins, the vulkano demo) sees the "no diagnostics" notice.
//! A live `damascene-winit-wgpu` host on a wayland compositor with
//! `wp_color_management_v1` shows the full capability matrix.
//!
//! Future expansion: once the host gains a runtime `set_color_preferences`
//! knob, add a picker to this page that re-applies on the fly.

use std::sync::LazyLock;

use damascene_core::color::{
    ColorFeature, ColorManagementStatus, ColorSpace, GammaExponent, Primaries, RenderIntent,
    TransferFunction,
};
use damascene_core::prelude::*;

#[derive(Default)]
pub struct State;

/// Full-viewport width at/above which the cards lay out in two columns.
/// Higher than the shell's phone breakpoint (700) because the cards are
/// dense and the sidebar already eats into the content width — below this
/// two columns would get cramped, so we collapse back to one.
const TWO_COLUMN_BREAKPOINT_PX: f32 = 900.0;

pub fn view(cx: &BuildCx) -> El {
    let (working, status, surface) = match cx.diagnostics() {
        Some(d) => (
            d.working_color_space,
            d.color_management.clone(),
            d.surface_color.clone(),
        ),
        None => return missing_diagnostics_panel(),
    };

    // Build each card once, then arrange responsively below.
    let protocol = protocol_status_card(&status);
    let working_space = working_space_card(working);
    let attached = attached_description_card(&status);
    let display = display_targets_card(&status);
    let capabilities = capabilities_card(&status);
    let graphics = graphics_surface_card(&surface);
    let images = wide_color_images_card();

    // Two columns on a wide viewport, collapsing to one when the content
    // area would get cramped. The left column carries the protocol status
    // and the (tall) compositor capability matrix; the right carries the
    // working space, attached description, graphics surface, and display
    // targets — which keeps the two columns roughly balanced in height.
    let cards = if cx.viewport_below(TWO_COLUMN_BREAKPOINT_PX) {
        column([
            protocol,
            working_space,
            attached,
            display,
            capabilities,
            graphics,
            images,
        ])
        .gap(tokens::SPACE_4)
        .align(Align::Stretch)
    } else {
        row([
            column([protocol, capabilities, images])
                .gap(tokens::SPACE_4)
                .align(Align::Stretch)
                .width(Size::Fill(1.0)),
            column([working_space, attached, graphics, display])
                .gap(tokens::SPACE_4)
                .align(Align::Stretch)
                .width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_4)
        // Top-align the columns; let each keep its natural height rather
        // than stretching the shorter one to match.
        .align(Align::Start)
    };

    scroll([column([
        h1("Color management"),
        paragraph(
            "Inspect what the host negotiated with the display server. Damascene composites \
             in the working color space below; the wire-side status reports what the \
             compositor was told the surface contains.",
        )
        .muted(),
        cards,
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

// ---------------------------------------------------------------------------
// Cards
// ---------------------------------------------------------------------------

fn protocol_status_card(status: &ColorManagementStatus) -> El {
    let (badge, summary) = match status {
        ColorManagementStatus::Available { .. } => (
            kind_badge("Available", true),
            "The host's display server exports a color-management protocol \
             and damascene successfully negotiated with it.",
        ),
        ColorManagementStatus::Unavailable => (
            kind_badge("Unavailable", false),
            "The host's display server does not expose a color-management \
             protocol — or the host (X11 / macOS / Windows / Android / iOS / \
             headless) doesn't have a driver. Surfaces go out with the \
             system's implicit interpretation, which is sRGB everywhere \
             damascene runs.",
        ),
    };
    titled_card(
        "Protocol",
        [
            row([badge]).align(Align::Center),
            paragraph(summary).muted().small(),
        ],
    )
}

fn working_space_card(space: ColorSpace) -> El {
    titled_card(
        "Working color space",
        [
            paragraph(
                "What damascene composites in. The paint stream converts every \
                 authored Color into this space exactly once at the upload \
                 boundary. Shader math runs here.",
            )
            .muted()
            .small(),
            field_grid(color_space_rows(space)),
        ],
    )
}

fn attached_description_card(status: &ColorManagementStatus) -> El {
    let body: Vec<El> = match status {
        ColorManagementStatus::Available {
            attached: Some(space),
            ..
        } => vec![
            paragraph(
                "An image description with these parameters is attached to \
                 the wl_surface. The compositor uses it to interpret the \
                 buffer bytes for color-correct display.",
            )
            .muted()
            .small(),
            field_grid(color_space_rows(*space)),
        ],
        ColorManagementStatus::Available { attached: None, .. } => vec![
            paragraph(
                "No image description attached. The negotiator chose sRGB and \
             the compositor's implicit handling already matches — no round \
             trip needed.",
            )
            .muted()
            .small(),
        ],
        ColorManagementStatus::Unavailable => vec![
            paragraph("No color-management protocol on this host; nothing to attach.")
                .muted()
                .small(),
        ],
    };
    titled_card("Image description on surface", body)
}

fn display_targets_card(status: &ColorManagementStatus) -> El {
    let targets = match status {
        ColorManagementStatus::Available { targets, .. } => targets,
        ColorManagementStatus::Unavailable => {
            return titled_card(
                "Display targets",
                [paragraph(
                    "Only visible when the host's color-management protocol \
                     is available.",
                )
                .muted()
                .small()],
            );
        }
    };

    // No usable feedback at all (no feedback path, or an ICC-based
    // preferred description with no luminance events) → every field None.
    let any = targets.reference_luminance_nits.is_some()
        || targets.target_max_luminance_nits.is_some()
        || targets.preferred_transfer.is_some()
        || targets.preferred_primaries.is_some()
        || targets.preferred_is_icc;
    if !any {
        return titled_card(
            "Display targets",
            [paragraph(
                "The compositor exposed no preferred-description feedback for \
                 this surface (or it was ICC-based). With no luminance hint, \
                 damascene stays on the SDR path.",
            )
            .muted()
            .small()],
        );
    }

    let hdr = targets.indicates_hdr();
    let nits = |v: Option<f32>| match v {
        Some(n) => format!("{n:.0} cd/m²"),
        None => "—".to_string(),
    };
    let rows = vec![
        ("reference white", nits(targets.reference_luminance_nits)),
        ("display peak", nits(targets.target_max_luminance_nits)),
        ("display black", nits(targets.target_min_luminance_nits)),
        (
            "max content light",
            nits(targets.max_content_light_level_nits),
        ),
        (
            "max frame-avg light",
            nits(targets.max_frame_average_light_level_nits),
        ),
        (
            "preferred transfer",
            targets
                .preferred_transfer
                .map(transfer_label)
                .unwrap_or_else(|| "—".to_string()),
        ),
        (
            "preferred primaries",
            targets
                .preferred_primaries
                .map(|p| primaries_label(p).to_string())
                .unwrap_or_else(|| "—".to_string()),
        ),
        (
            "preferred kind",
            if targets.preferred_is_icc {
                "ICC profile".to_string()
            } else {
                "parametric".to_string()
            },
        ),
    ];

    titled_card(
        "Display targets",
        [
            row([kind_badge(
                if hdr { "HDR output" } else { "SDR output" },
                hdr,
            )])
            .align(Align::Center),
            paragraph(
                "What the compositor's preferred image description reports for \
                 the output this surface is on. Reference white is the level \
                 HDR UI white should target; display peak is the headroom above \
                 it. Damascene only emits HDR when this evidence confirms it.",
            )
            .muted()
            .small(),
            field_grid(rows),
        ],
    )
}

fn capabilities_card(status: &ColorManagementStatus) -> El {
    let caps = match status {
        ColorManagementStatus::Available { capabilities, .. } => capabilities,
        ColorManagementStatus::Unavailable => {
            return titled_card(
                "Compositor capability matrix",
                [paragraph(
                    "Only visible when the host's color-management protocol \
                     is available.",
                )
                .muted()
                .small()],
            );
        }
    };

    titled_card(
        "Compositor capability matrix",
        [
            paragraph(
                "What primaries and transfer functions the compositor \
                 advertised, vs. what damascene can author. A ✓ means both \
                 sides agree; a ✗ means damascene would have to fall back to \
                 something else.",
            )
            .muted()
            .small(),
            column([
                subsection_title("Primaries"),
                capability_matrix(
                    ALL_PRIMARIES
                        .iter()
                        .map(|(p, label)| (caps.supports_primaries(*p), label.to_string())),
                ),
                subsection_title("Transfer functions"),
                capability_matrix(
                    ALL_TRANSFERS
                        .iter()
                        .map(|(tf, label)| (caps.supports_transfer(*tf), label.to_string())),
                ),
                subsection_title("Features"),
                capability_matrix(
                    ALL_FEATURES
                        .iter()
                        .map(|(feat, label)| (caps.has_feature(*feat), label.to_string())),
                ),
                subsection_title("Render intents"),
                capability_matrix(ALL_INTENTS.iter().map(|(intent, label)| {
                    (caps.render_intents.contains(intent), label.to_string())
                })),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Stretch),
        ],
    )
}

fn graphics_surface_card(surface: &Option<SurfaceColorInfo>) -> El {
    let Some(s) = surface else {
        return titled_card(
            "Graphics surface (wgpu)",
            [paragraph(
                "This host doesn't present through a wgpu surface (headless \
                 render bins, the vulkano demo, the WebGPU host), so there are \
                 no swapchain formats to report.",
            )
            .muted()
            .small()],
        );
    };

    titled_card(
        "Graphics surface (wgpu)",
        [
            paragraph(
                "What the swapchain can represent — the other half of \
                 negotiation. A compositor that ingests linear BT.2020 is moot \
                 if the surface offers no wide-capable format, so one must \
                 appear below for HDR / wide-gamut output to be reachable.",
            )
            .muted()
            .small(),
            field_grid(vec![
                ("adapter", s.adapter.clone()),
                ("driver", dash_if_empty(&s.driver)),
                ("chosen format", s.chosen_format.clone()),
                ("present mode", s.present_mode.clone()),
                ("alpha mode", s.alpha_mode.clone()),
            ]),
            subsection_title("Advertised formats"),
            surface_format_matrix(s),
        ],
    )
}

fn surface_format_matrix(s: &SurfaceColorInfo) -> El {
    column(s.formats.iter().map(|f| {
        // ✓ marks a format that can carry wide-gamut / HDR output.
        let mark = if f.wide {
            icon(IconName::Check)
        } else {
            icon(IconName::X).muted()
        };
        let tag = if f.name.contains("Float") {
            "float — linear-direct"
        } else if f.wide {
            "10-bit — PQ-encode target"
        } else if f.srgb {
            "sRGB — 8-bit"
        } else {
            "8-bit unorm"
        };
        row([
            mark.width(Size::Fixed(20.0)),
            mono(f.name.clone()).small().width(Size::Fixed(180.0)),
            mono(tag).muted().small().width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }))
    .gap(2.0)
    .align(Align::Stretch)
}

// ---------------------------------------------------------------------------
// Wide-color image demo
// ---------------------------------------------------------------------------

const RAMP_W: u32 = 256;
const RAMP_H: u32 = 1;

/// Fully saturated hue sweep, 8-bit encoded. Both sweep images share
/// these exact bytes — only the color-space tag differs, so any visible
/// difference between them is the image color pipeline at work.
fn hue_sweep_pixels() -> Vec<u8> {
    let mut px = Vec::with_capacity((RAMP_W * RAMP_H * 4) as usize);
    for _ in 0..RAMP_H {
        for x in 0..RAMP_W {
            // HSV → RGB at s = v = 1, hue 0..360 across the width.
            let h = x as f32 / RAMP_W as f32 * 6.0;
            let c = 1.0 - (h % 2.0 - 1.0_f32).abs();
            let (r, g, b) = match h as u32 {
                0 => (1.0, c, 0.0),
                1 => (c, 1.0, 0.0),
                2 => (0.0, 1.0, c),
                3 => (0.0, c, 1.0),
                4 => (c, 0.0, 1.0),
                _ => (1.0, 0.0, c),
            };
            px.extend([
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                0xff,
            ]);
        }
    }
    px
}

static SWEEP_SRGB: LazyLock<Image> =
    LazyLock::new(|| Image::from_rgba8(RAMP_W, RAMP_H, hue_sweep_pixels()));

static SWEEP_P3: LazyLock<Image> = LazyLock::new(|| {
    Image::from_rgba8_in(ColorSpace::DISPLAY_P3, RAMP_W, RAMP_H, hue_sweep_pixels())
});

/// Linear scRGB luminance ramp, 0 → 4× SDR white left to right. On an
/// SDR surface everything from the 25% mark on clamps to white; on an
/// extended-range surface the right three quarters keep brightening.
static RAMP_HDR: LazyLock<Image> = LazyLock::new(|| {
    let mut px = Vec::with_capacity((RAMP_W * RAMP_H * 4) as usize);
    for _ in 0..RAMP_H {
        for x in 0..RAMP_W {
            let v = x as f32 / (RAMP_W - 1) as f32 * 4.0;
            px.extend([v, v, v, 1.0]);
        }
    }
    Image::from_rgba_f32_in(ColorSpace::SCRGB_LINEAR, RAMP_W, RAMP_H, px)
});

fn ramp_image(img: &LazyLock<Image>) -> El {
    image((**img).clone())
        .width(Size::Fill(1.0))
        .height(Size::Fixed(28.0))
        .image_fit(ImageFit::Fill)
        .radius(tokens::RADIUS_SM)
        .stroke(tokens::BORDER)
}

fn wide_color_images_card() -> El {
    titled_card(
        "Wide-color images",
        [
            paragraph(
                "Generated images exercising the color-managed image pipeline. \
                 The two hue sweeps share identical encoded bytes — only the \
                 color-space tag differs. On a wide-gamut surface the \
                 Display-P3 sweep is visibly more saturated; on an sRGB \
                 surface its out-of-gamut colors clip and the sweeps roughly \
                 match. The luminance ramp reaches SDR white a quarter of the \
                 way in — on an HDR output it keeps brightening to 4× past \
                 that point, on SDR it clamps flat.",
            )
            .muted()
            .small(),
            subsection_title("8-bit hue sweep — tagged sRGB"),
            ramp_image(&SWEEP_SRGB),
            subsection_title("Same bytes — tagged Display-P3"),
            ramp_image(&SWEEP_P3),
            subsection_title("Linear float ramp — 0 → 4× SDR white (scRGB)"),
            ramp_image(&RAMP_HDR),
        ],
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dash_if_empty(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

fn missing_diagnostics_panel() -> El {
    scroll([column([
        h1("Color management"),
        paragraph(
            "This host doesn't populate HostDiagnostics, so the color-\
             management status isn't observable from here. Run the \
             showcase under `damascene-winit-wgpu` to see live data.",
        )
        .muted(),
        // The image pipeline demo doesn't need diagnostics — keep it
        // visible so the vulkano / ash demo hosts can still exercise it.
        wide_color_images_card(),
    ])
    .gap(tokens::SPACE_4)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

fn color_space_rows(space: ColorSpace) -> Vec<(&'static str, String)> {
    vec![
        ("primaries", primaries_label(space.primaries).to_string()),
        ("transfer", transfer_label(space.transfer)),
        (
            "ref luminance",
            format!("{:.0} cd/m²", space.reference_luminance_nits),
        ),
    ]
}

fn field_grid(rows: Vec<(&'static str, String)>) -> El {
    column(rows.into_iter().map(|(k, v)| {
        row([
            mono(k).muted().small().width(Size::Fixed(120.0)),
            mono(v).small().width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }))
    .gap(2.0)
    .align(Align::Stretch)
}

fn capability_matrix<I: IntoIterator<Item = (bool, String)>>(rows: I) -> El {
    column(rows.into_iter().map(|(supported, label)| {
        let mark = if supported {
            icon(IconName::Check)
        } else {
            icon(IconName::X).muted()
        };
        row([
            mark.width(Size::Fixed(20.0)),
            mono(label).small().width(Size::Fill(1.0)),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
    }))
    .gap(2.0)
    .align(Align::Stretch)
}

fn subsection_title(label: &'static str) -> El {
    text(label).label().small()
}

fn kind_badge(label: &'static str, ok: bool) -> El {
    let (fg, bg) = if ok {
        (tokens::PRIMARY_FOREGROUND, tokens::PRIMARY)
    } else {
        (tokens::MUTED_FOREGROUND, tokens::MUTED)
    };
    text(label)
        .label()
        .small()
        .text_color(fg)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_1))
        .fill(bg)
        .radius(tokens::RADIUS_SM)
}

fn primaries_label(p: Primaries) -> &'static str {
    match p {
        Primaries::Srgb => "sRGB / BT.709",
        Primaries::DisplayP3 => "Display-P3",
        Primaries::Bt2020 => "BT.2020 / BT.2100",
        Primaries::AdobeRgb => "Adobe RGB",
    }
}

fn transfer_label(tf: TransferFunction) -> String {
    match tf {
        TransferFunction::Linear => "linear (extended range)".to_string(),
        TransferFunction::Srgb => "sRGB (IEC 61966-2-1)".to_string(),
        TransferFunction::Bt1886 => "BT.1886".to_string(),
        TransferFunction::Pq => "ST 2084 / PQ".to_string(),
        TransferFunction::Hlg => "Hybrid Log-Gamma".to_string(),
        TransferFunction::Gamma(g) => format!("gamma {:.2}", g.to_f32()),
    }
}

const ALL_PRIMARIES: &[(Primaries, &str)] = &[
    (Primaries::Srgb, "sRGB / BT.709"),
    (Primaries::DisplayP3, "Display-P3"),
    (Primaries::Bt2020, "BT.2020 / BT.2100"),
    (Primaries::AdobeRgb, "Adobe RGB"),
];

const ALL_FEATURES: &[(ColorFeature, &str)] = &[
    (ColorFeature::IccV2V4, "create_icc_creator"),
    (ColorFeature::Parametric, "create_parametric_creator"),
    (ColorFeature::SetPrimaries, "set_primaries"),
    (ColorFeature::SetTfPower, "set_tf_power"),
    (ColorFeature::SetLuminances, "set_luminances"),
    (
        ColorFeature::SetMasteringDisplayPrimaries,
        "set_mastering_display_primaries",
    ),
    (ColorFeature::ExtendedTargetVolume, "extended_target_volume"),
    (ColorFeature::WindowsScrgb, "create_windows_scrgb"),
];

const ALL_INTENTS: &[(RenderIntent, &str)] = &[
    (RenderIntent::Perceptual, "perceptual"),
    (RenderIntent::Relative, "relative"),
    (RenderIntent::Saturation, "saturation"),
    (RenderIntent::Absolute, "absolute"),
    (RenderIntent::RelativeBpc, "relative (BPC)"),
    (
        RenderIntent::AbsoluteNoAdaptation,
        "absolute (no adaptation)",
    ),
];

const ALL_TRANSFERS: &[(TransferFunction, &str)] = &[
    (TransferFunction::Srgb, "sRGB"),
    (TransferFunction::Linear, "linear (extended range)"),
    (TransferFunction::Bt1886, "BT.1886"),
    (TransferFunction::Pq, "ST 2084 / PQ"),
    (TransferFunction::Hlg, "HLG"),
    (
        TransferFunction::Gamma(match GammaExponent::from_x100(220) {
            Some(g) => g,
            None => unreachable!(),
        }),
        "gamma 2.2",
    ),
];
