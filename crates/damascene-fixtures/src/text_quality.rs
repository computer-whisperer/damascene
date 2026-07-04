//! Shared text-quality fixture used by both backends' headless
//! renderers (`render_text_quality` for wgpu and vulkano).
//!
//! The fixture intentionally exercises:
//!
//! - body/UI sizes (10..16) where rasterization quality matters most,
//! - mid sizes (18..24) typical for headings and KPIs,
//! - large sizes (32/48/64) where corner sharpness shows up,
//! - regular / medium / bold weights side-by-side,
//! - light and dark backgrounds (gamma blending tells on each),
//! - a small Unicode + math sample so fallback faces participate,
//! - color emoji (size ramp + ZWJ/flag/skin-tone clusters) and
//!   RTL/bidi text via [`crate::emoji_rtl`] (issue #75).

use damascene_core::prelude::*;

pub const SIZES: &[f32] = &[10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 32.0, 48.0, 64.0];
pub const SAMPLE: &str = "The quick brown fox jumps over the lazy dog 0123456789";
pub const UNICODE_SAMPLE: &str = "—≡ →↑↓ ✓✕ αβγ ∑∫ ▲◆";

pub const LOGICAL_WIDTH: u32 = 980;
pub const LOGICAL_HEIGHT: u32 = 2160;

fn size_row(size: f32) -> El {
    let label = format!("{}px", size as u32);
    column([
        row([
            text(&label)
                .font_size(11.0)
                .muted()
                .width(Size::Fixed(56.0)),
            text(SAMPLE)
                .font_size(size)
                .font_weight(FontWeight::Regular),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::End),
        row([
            text("medium")
                .font_size(11.0)
                .muted()
                .width(Size::Fixed(56.0)),
            text(SAMPLE).font_size(size).font_weight(FontWeight::Medium),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::End),
        row([
            text("bold")
                .font_size(11.0)
                .muted()
                .width(Size::Fixed(56.0)),
            text(SAMPLE).font_size(size).font_weight(FontWeight::Bold),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::End),
    ])
    .gap(2.0)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn dark_panel() -> El {
    column([
        text("dark surface").caption(),
        text(SAMPLE).font_size(14.0),
        text(SAMPLE).font_size(14.0).font_weight(FontWeight::Bold),
        text(UNICODE_SAMPLE).font_size(16.0),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .fill(Color::srgb_u8(20, 20, 24))
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn light_panel() -> El {
    column([
        text("light surface")
            .caption()
            .text_color(Color::srgb_u8(80, 80, 80)),
        text(SAMPLE)
            .font_size(14.0)
            .text_color(Color::srgb_u8(20, 20, 20)),
        text(SAMPLE)
            .font_size(14.0)
            .font_weight(FontWeight::Bold)
            .text_color(Color::srgb_u8(20, 20, 20)),
        text(UNICODE_SAMPLE)
            .font_size(16.0)
            .text_color(Color::srgb_u8(20, 20, 20)),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
    .fill(Color::srgb_u8(245, 245, 248))
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

pub fn fixture() -> El {
    column(
        [
            vec![h2("Text quality matrix")],
            SIZES.iter().map(|&s| size_row(s)).collect::<Vec<_>>(),
            vec![dark_panel(), light_panel(), crate::emoji_rtl::section()],
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
    )
    .gap(tokens::SPACE_3)
    .padding(tokens::SPACE_4)
    .fill(tokens::BACKGROUND)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

#[cfg(test)]
mod tests {
    use super::*;
    use damascene_core::layout::layout;
    use damascene_core::state::UiState;

    /// The render bins size their surfaces from `LOGICAL_HEIGHT`; if
    /// the fixture outgrows it, the bottom sections silently fall off
    /// every backend's PNG. Keep the constant honest.
    #[test]
    fn fixture_fits_the_declared_logical_height() {
        let mut tree = fixture();
        let mut state = UiState::new();
        layout(
            &mut tree,
            &mut state,
            Rect::new(0.0, 0.0, LOGICAL_WIDTH as f32, 10_000.0),
        );
        let bottom = tree
            .children
            .iter()
            .map(|c| {
                let r = c.computed_rect;
                r.y + r.h
            })
            .fold(0.0f32, f32::max);
        // SPACE_4 of root bottom padding sits below the last child.
        let needed = bottom + tokens::SPACE_4;
        assert!(
            needed <= LOGICAL_HEIGHT as f32,
            "fixture needs {needed}px but LOGICAL_HEIGHT is {LOGICAL_HEIGHT}px — bump the constant"
        );
        assert!(
            needed > LOGICAL_HEIGHT as f32 - 300.0,
            "fixture only needs {needed}px — LOGICAL_HEIGHT {LOGICAL_HEIGHT}px leaves a large dead band"
        );
    }
}
