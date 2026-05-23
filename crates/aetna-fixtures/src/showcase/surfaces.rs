//! Surfaces — surface roles, drop shadows, custom-shader chrome.
//!
//! Demos how the panel chrome looks at different elevations and via
//! different palette tokens, and includes the liquid-glass custom
//! shader as the showpiece for "any El can mount a custom WGSL surface
//! and the layer ordering still works."

use aetna_core::prelude::*;

#[derive(Default)]
pub struct State {
    pub glass_preset: usize,
    pub glass_drift: usize,
}

// Liquid-glass demo wiring — keys, presets, drift table, helpers. The
// demo needs backdrop sampling (Pass A → snapshot → Pass B), which
// WebGL2 surfaces don't advertise (`COPY_SRC` on the swapchain texture
// is missing), so the whole subsection drops out on wasm builds along
// with the wiring below. Tests cover the on_event cycling, so the cfg
// keeps these compiled under `cfg(test)` regardless of target.
#[cfg(any(not(target_arch = "wasm32"), test))]
const GLASS_NEXT_KEY: &str = "surfaces-glass-next";
#[cfg(any(not(target_arch = "wasm32"), test))]
const GLASS_DRIFT_KEY: &str = "surfaces-glass-drift";

#[cfg(any(not(target_arch = "wasm32"), test))]
#[derive(Clone, Copy)]
struct GlassPreset {
    label: &'static str,
    blurb: &'static str,
    blur_px: f32,
    refraction: f32,
    specular: f32,
    tint: Color,
}

#[cfg(any(not(target_arch = "wasm32"), test))]
const GLASS_PRESETS: &[GlassPreset] = &[
    GlassPreset {
        label: "Soft",
        blurb: "Gentle blur, faint warm tint, soft bevel.",
        blur_px: 4.0,
        refraction: 0.45,
        specular: 0.8,
        tint: Color::srgb_u8a(240, 240, 250, 110),
    },
    GlassPreset {
        label: "Heavy",
        blurb: "Wide blur, stronger refraction at the rim.",
        blur_px: 10.0,
        refraction: 0.85,
        specular: 1.1,
        tint: Color::srgb_u8a(230, 235, 250, 140),
    },
    GlassPreset {
        label: "Cool",
        blurb: "Cool blue tint, crisp specular bevel.",
        blur_px: 6.0,
        refraction: 0.55,
        specular: 1.4,
        tint: Color::srgb_u8a(180, 215, 255, 170),
    },
    GlassPreset {
        label: "Crisp",
        blurb: "Minimal blur, pure refraction lensing.",
        blur_px: 1.5,
        refraction: 0.95,
        specular: 1.6,
        tint: Color::srgb_u8a(250, 250, 255, 60),
    },
];

#[cfg(any(not(target_arch = "wasm32"), test))]
const DRIFT_OFFSETS: &[f32] = &[0.0, -120.0, 120.0];

pub fn view(state: &State, cx: &BuildCx) -> El {
    // Web builds run without the backdrop-sampling capability the
    // liquid-glass shader needs (WebGL2 surfaces don't advertise
    // COPY_SRC, so the snapshot copy can't run). Drop the demo and the
    // sentences that promise it rather than show a static card the user
    // would assume is broken.
    #[cfg(not(target_arch = "wasm32"))]
    let intro = paragraph(
        "How the panel chrome looks. Surface roles slot tokenized \
         palette colours into stock components; drop shadows give \
         layered surfaces a sense of elevation; and the liquid-glass \
         card at the bottom proves any El can mount a custom WGSL \
         shader without losing layer compositing.",
    )
    .muted();
    #[cfg(target_arch = "wasm32")]
    let intro = paragraph(
        "How the panel chrome looks. Surface roles slot tokenized \
         palette colours into stock components, and drop shadows give \
         layered surfaces a sense of elevation.",
    )
    .muted();

    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut items: Vec<El> = vec![
        h1("Surfaces"),
        intro,
        section_label("Surface roles"),
        paragraph(
            "Each role binds a palette token to a stock surface — \
             swapping themes via the sidebar picker swaps these live.",
        )
        .small()
        .muted(),
        row([
            surface_role_tile("Panel", "tokens::CARD", tokens::CARD),
            surface_role_tile("Popover", "tokens::POPOVER", tokens::POPOVER),
            surface_role_tile("Muted", "tokens::MUTED", tokens::MUTED),
            surface_role_tile("Accent", "tokens::ACCENT", tokens::ACCENT),
        ])
        .gap(tokens::SPACE_3)
        .align(Align::Stretch),
        section_label("Drop shadows"),
        paragraph(
            "Drop shadows on the dark theme are subtle by design — 30% \
             black on a near-black background only darkens it by a few \
             channel codes. Tiles below cast SHADOW_SM / SHADOW_MD / \
             SHADOW_LG against an ACCENT panel so the falloff stands out.",
        )
        .muted()
        .small(),
        row([
            elevation_tile("shadow_sm", "4 px", tokens::SHADOW_SM),
            elevation_tile("shadow_md", "12 px", tokens::SHADOW_MD),
            elevation_tile("shadow_lg", "24 px", tokens::SHADOW_LG),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_5)
        .fill(tokens::ACCENT)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_LG),
        paragraph(
            "Stock cards and popovers pin their shadow through SurfaceRole \
             — Panel → SHADOW_SM, Popover → SHADOW_LG — so .shadow(...) \
             on a card is overridden at theme time. Set \
             surface_role(SurfaceRole::None) (or skip card/popover and \
             compose by hand) to paint a custom shadow value verbatim.",
        )
        .muted()
        .small(),
    ];

    #[cfg(not(target_arch = "wasm32"))]
    {
        items.push(section_label("Custom-shaded surface"));
        items.push(
            paragraph(
                "`liquid_glass.wgsl` reads the snapshot beneath the card, \
                 blurs and refracts it, and tints the result. Any El can \
                 mount a custom shader with `.shader(ShaderBinding::custom)` \
                 — the runtime orchestrates Pass A → snapshot → Pass B \
                 around it.",
            )
            .muted(),
        );
        items.push(glass_demo(state, super::is_phone(cx)));
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (state, cx);

    scroll([column(items).gap(tokens::SPACE_4).align(Align::Stretch)]).height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    // Glass-section buttons are the only event sources on this page,
    // and they're cfg'd out on wasm — the body just no-ops there.
    #[cfg(any(not(target_arch = "wasm32"), test))]
    {
        if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
            return;
        }
        match e.route() {
            Some(GLASS_NEXT_KEY) => {
                state.glass_preset = (state.glass_preset + 1) % GLASS_PRESETS.len()
            }
            Some(GLASS_DRIFT_KEY) => {
                state.glass_drift = (state.glass_drift + 1) % DRIFT_OFFSETS.len()
            }
            _ => {}
        }
    }
    #[cfg(all(target_arch = "wasm32", not(test)))]
    {
        let _ = (state, e);
    }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn surface_role_tile(title: &str, token_name: &str, fill: Color) -> El {
    // Fill + ellipsis on the long token name so the label respects
    // the tile width on phone viewports rather than overflowing.
    card([
        text(title).label().width(Size::Fill(1.0)).ellipsis(),
        text(token_name)
            .caption()
            .muted()
            .width(Size::Fill(1.0))
            .ellipsis(),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_3)
    .fill(fill)
    .radius(tokens::RADIUS_MD)
    .height(Size::Fixed(76.0))
}

fn elevation_tile(label: &str, sub: &str, shadow: f32) -> El {
    card([
        text(label).title().width(Size::Fill(1.0)).ellipsis(),
        text(sub).muted().small().width(Size::Fill(1.0)).ellipsis(),
    ])
    .shadow(shadow)
    .padding(tokens::SPACE_4)
    .gap(tokens::SPACE_1)
    .height(Size::Fixed(120.0))
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn glass_backdrop() -> El {
    // Stripes use status tokens — they swap with the theme so the glass
    // demo stays vivid under any palette without hard-coding colors.
    fn stripe(c: Color) -> El {
        column(Vec::<El>::new()).fill(c).width(Size::Fill(1.0))
    }
    row([
        stripe(tokens::DESTRUCTIVE),
        stripe(tokens::SUCCESS),
        stripe(tokens::INFO),
        stripe(tokens::WARNING),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn glass_card(preset: &GlassPreset, drift_x: f32, phone: bool) -> El {
    let (next_label, drift_label) = if phone {
        ("Next", "Drift")
    } else {
        ("Next preset", "Drift →")
    };
    column([
        text("Liquid glass")
            .bold()
            .font_size(22.0)
            .text_color(tokens::PRIMARY_FOREGROUND),
        text(preset.blurb)
            .text_color(tokens::PRIMARY_FOREGROUND)
            .wrap_text()
            .fill_width(),
        spacer(),
        row([
            text(format!("preset: {}", preset.label))
                .bold()
                .text_color(tokens::PRIMARY_FOREGROUND),
            spacer(),
            button(next_label).key(GLASS_NEXT_KEY).secondary(),
            button(drift_label).key(GLASS_DRIFT_KEY).primary(),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_4)
    .shader(
        ShaderBinding::custom("liquid_glass")
            .color("vec_a", preset.tint)
            .vec4(
                "vec_b",
                [preset.blur_px, preset.refraction, preset.specular, 0.0],
            )
            .vec4("vec_c", [28.0, 0.0, 0.0, 0.0]),
    )
    .width(Size::Fixed(if phone { 280.0 } else { 420.0 }))
    .height(Size::Fixed(if phone { 200.0 } else { 220.0 }))
    .translate(drift_x, 0.0)
    .animate(Timing::SPRING_BOUNCY)
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn glass_demo(state: &State, phone: bool) -> El {
    let preset = &GLASS_PRESETS[state.glass_preset % GLASS_PRESETS.len()];
    let raw_drift = DRIFT_OFFSETS[state.glass_drift % DRIFT_OFFSETS.len()];
    // 360px phone viewport can't afford the desktop ±120 drift without
    // the card sliding under the scroll gutter. Narrow the drift to fit
    // a 280px card inside ~336px of phone content with a few px slack.
    let drift_x = if phone { raw_drift * 0.4 } else { raw_drift };
    stack([glass_backdrop(), glass_card(preset, drift_x, phone)])
        .align(Align::Center)
        .justify(Justify::Center)
        .height(Size::Fixed(if phone { 240.0 } else { 280.0 }))
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_LG)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn glass_next_cycles_through_presets() {
        let mut s = State::default();
        assert_eq!(s.glass_preset, 0);
        on_event(&mut s, click(GLASS_NEXT_KEY));
        assert_eq!(s.glass_preset, 1);
        for _ in 0..GLASS_PRESETS.len() - 1 {
            on_event(&mut s, click(GLASS_NEXT_KEY));
        }
        assert_eq!(s.glass_preset, 0);
    }

    #[test]
    fn glass_drift_cycles_horizontal_offsets() {
        let mut s = State::default();
        assert_eq!(DRIFT_OFFSETS[s.glass_drift], 0.0);
        on_event(&mut s, click(GLASS_DRIFT_KEY));
        assert_ne!(DRIFT_OFFSETS[s.glass_drift], 0.0);
        for _ in 0..DRIFT_OFFSETS.len() - 1 {
            on_event(&mut s, click(GLASS_DRIFT_KEY));
        }
        assert_eq!(DRIFT_OFFSETS[s.glass_drift], 0.0);
    }

    #[test]
    fn drift_offsets_stay_inside_content_bounds() {
        // Glass card is 420 wide; showcase content area is ~720 wide
        // (900 viewport − 180 sidebar). Half the spare room is 150 —
        // any drift offset beyond that pushes the card past the panel
        // edge or into the sidebar.
        for &offset in DRIFT_OFFSETS {
            assert!(
                offset.abs() <= 150.0,
                "drift offset {offset} exceeds safe range"
            );
        }
    }
}
