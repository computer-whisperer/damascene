//! Animation — Timing profiles primer.
//!
//! Demonstrates the four built-in `Timing` profiles by attaching each
//! to an animatable prop on a swatch and toggling the target via a
//! single button per profile. Click any swatch to see its motion
//! curve; click again to revert.
//!
//! Composes `.scale()`, `.translate()`, `.opacity()`, `.fill()` —
//! the four primary animatable props — under different `Timing`
//! values so the *shape* of the spring/tween is visible.

use damascene_core::prelude::*;

#[derive(Default)]
pub struct State {
    /// Which swatches are toggled. Index into PROFILES.
    pub toggled: [bool; 4],
}

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    blurb: &'static str,
    timing: Timing,
    fill_resting: Color,
    fill_active: Color,
}

const PROFILES: [Profile; 4] = [
    Profile {
        name: "spring · gentle",
        blurb: "Soft critically-damped spring — UI defaults.",
        timing: Timing::SPRING_GENTLE,
        fill_resting: tokens::INFO,
        fill_active: tokens::PRIMARY,
    },
    Profile {
        name: "spring · quick",
        blurb: "Responsive snap — buttons, tooltips.",
        timing: Timing::SPRING_QUICK,
        fill_resting: tokens::SUCCESS,
        fill_active: tokens::WARNING,
    },
    Profile {
        name: "spring · bouncy",
        blurb: "Overshoots — drawer slides, picker pops.",
        timing: Timing::SPRING_BOUNCY,
        fill_resting: tokens::WARNING,
        fill_active: tokens::INFO,
    },
    Profile {
        name: "tween · ease",
        blurb: "Fixed-duration ease for incidental fades.",
        timing: Timing::EASE_STANDARD,
        fill_resting: tokens::MUTED,
        fill_active: tokens::DESTRUCTIVE,
    },
];

pub fn view(state: &State) -> El {
    let cards: Vec<El> = PROFILES
        .iter()
        .enumerate()
        .map(|(i, p)| profile_card(i, p, state.toggled[i]))
        .collect();

    column([
        h1("Animation"),
        paragraph(
            "Animatable props (`scale`, `translate`, `opacity`, `fill`) \
             attach to any element. Pair them with a `Timing` profile — \
             a critically-damped spring for UI defaults, a bouncier \
             spring for delightful overshoots, or a fixed-duration tween \
             for incidental fades. Click a swatch to fire its toggle.",
        )
        .muted(),
        row(cards).gap(tokens::SPACE_4).align(Align::Stretch),
        paragraph(
            "All four cards share the same key-routing shape: the swatch \
             carries the routed key and `apply_event` flips the toggled \
             flag. The library tracks (node, prop) targets and \
             interpolates each frame.",
        )
        .small()
        .muted(),
    ])
    .gap(tokens::SPACE_4)
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    if let Some(k) = e.route()
        && let Some(rest) = k.strip_prefix("animation-swatch-")
        && let Ok(i) = rest.parse::<usize>()
        && i < state.toggled.len()
    {
        state.toggled[i] = !state.toggled[i];
    }
}

fn profile_card(i: usize, p: &Profile, on: bool) -> El {
    let (fill, scale, translate, opacity) = if on {
        (p.fill_active, 1.15, -10.0, 1.0)
    } else {
        (p.fill_resting, 1.0, 0.0, 0.85)
    };
    card([
        column(Vec::<El>::new())
            .key(format!("animation-swatch-{i}"))
            .fill(fill)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_LG)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(96.0))
            .scale(scale)
            .translate(0.0, translate)
            .opacity(opacity)
            .animate(p.timing),
        // Hug width on a label inside a narrow card forces the
        // intrinsic content width and overflows the card on phone
        // viewports. Filling + ellipsis lets the label respect the
        // card's content rect.
        text(p.name).label().width(Size::Fill(1.0)).ellipsis(),
        paragraph(p.blurb).small().muted(),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn click_toggles_individual_swatches() {
        let mut s = State::default();
        on_event(&mut s, click("animation-swatch-0"));
        assert!(s.toggled[0]);
        assert!(!s.toggled[1]);
        on_event(&mut s, click("animation-swatch-2"));
        assert!(s.toggled[2]);
        on_event(&mut s, click("animation-swatch-0"));
        assert!(!s.toggled[0]);
    }
}
