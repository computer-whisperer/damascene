//! Color management — inspect what the host negotiated with the
//! display server.
//!
//! Reads from `BuildCx::diagnostics()` so the page is host-agnostic —
//! a backend that doesn't populate [`HostDiagnostics`] (the headless
//! render bins, the vulkano demo) sees the "no diagnostics" notice.
//! A live `aetna-winit-wgpu` host on a wayland compositor with
//! `wp_color_management_v1` shows the full capability matrix.
//!
//! Future expansion: once the host gains a runtime `set_color_preferences`
//! knob, add a picker to this page that re-applies on the fly.

use aetna_core::color::{
    ColorManagementStatus, ColorSpace, GammaExponent, Primaries, TransferFunction,
};
use aetna_core::prelude::*;

#[derive(Default)]
pub struct State;

pub fn view(cx: &BuildCx) -> El {
    let (working, status) = match cx.diagnostics() {
        Some(d) => (d.working_color_space, d.color_management.clone()),
        None => return missing_diagnostics_panel(),
    };

    scroll([column([
        h1("Color management"),
        paragraph(
            "Inspect what the host negotiated with the display server. Aetna composites \
             in the working color space below; the wire-side status reports what the \
             compositor was told the surface contains.",
        )
        .muted(),
        protocol_status_card(&status),
        working_space_card(working),
        attached_description_card(&status),
        capabilities_card(&status),
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
             and aetna successfully negotiated with it.",
        ),
        ColorManagementStatus::Unavailable => (
            kind_badge("Unavailable", false),
            "The host's display server does not expose a color-management \
             protocol — or the host (X11 / macOS / Windows / Android / iOS / \
             headless) doesn't have a driver. Surfaces go out with the \
             system's implicit interpretation, which is sRGB everywhere \
             aetna runs.",
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
                "What aetna composites in. The paint stream converts every \
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
                 advertised, vs. what aetna can author. A ✓ means both \
                 sides agree; a ✗ means aetna would have to fall back to \
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
                capability_matrix([(
                    caps.parametric_creator,
                    "create_parametric_creator".to_string(),
                )]),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Stretch),
        ],
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn missing_diagnostics_panel() -> El {
    scroll([column([
        h1("Color management"),
        paragraph(
            "This host doesn't populate HostDiagnostics, so the color-\
             management status isn't observable from here. Run the \
             showcase under `aetna-winit-wgpu` to see live data.",
        )
        .muted(),
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
