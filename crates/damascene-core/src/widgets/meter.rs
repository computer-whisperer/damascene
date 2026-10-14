//! Meter — a thin instrumentation bar reading out a *measurement*
//! inside a known range (audio input level, CPU load, disk usage,
//! signal strength, temperature headroom).
//!
//! **Oracle: the web platform's `<meter>` element** (HTML Living
//! Standard §4.10.14). Per `docs/NAMING_ORACLE.md` the widget/anatomy
//! oracle is shadcn/ui, but shadcn has no analogue for this — its
//! `Progress` is the task-completion primitive, and the late-2025
//! generation added no measurement counterpart. `<meter>` is therefore
//! cited under the registry's **web-platform vocabulary** row, the
//! standing exception for "things frameworks under-expose but the
//! platform names".
//!
//! The distinction `<meter>` draws against `<progress>` is the one this
//! module keeps:
//!
//! | | [`crate::widgets::progress`] | [`meter`] |
//! |---|---|---|
//! | means | how far a task got | how full a gauge reads |
//! | indeterminate mode | yes ([`progress_indeterminate`][pi]) | **no** — a measurement always has a value |
//! | animation | yes (the indeterminate loader runs a shader) | **no** — instrumentation must not add motion the signal didn't have |
//! | color | one fill, author-chosen | optionally three-band from [`MeterOpts`] thresholds |
//!
//! The no-animation rule is load-bearing rather than cosmetic: a level
//! meter is sampled and redrawn many times a second, so any easing on
//! the fill would smear the very transients the author is trying to
//! show. `meter` carries no [`crate::anim::Timing`] and binds no
//! shader.
//!
//! [pi]: crate::widgets::progress::progress_indeterminate
//!
//! ```ignore
//! use damascene_core::prelude::*;
//!
//! struct Voice { input_level: f32 }
//!
//! impl App for Voice {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         // Audio input level in a status bar: a `IN ▇▇▇▇▁▁` readout.
//!         row([
//!             text("IN").caption().muted(),
//!             meter(0.62).width(Size::Fixed(56.0)),
//!         ])
//!         .gap(tokens::SPACE_1)
//!         .align(Align::Center)
//!     }
//! }
//! ```
//!
//! With thresholds, for a gauge where "high" is bad — CPU load, disk
//! pressure, hotend temperature headroom:
//!
//! ```ignore
//! meter_with(load, MeterOpts::default().low(0.6).high(0.85))
//! ```

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use std::panic::Location;

use crate::layout::LayoutCtx;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

/// Default bar height in logical pixels — one rung thinner than
/// [`crate::widgets::progress::DEFAULT_HEIGHT`], because a meter is a
/// readout sitting inside dense chrome (a status bar, an item row)
/// rather than a block-level element of its own.
///
/// This is a `default_height`, so a theme's progress metrics rung still
/// wins (see [`MetricsRole::Progress`]); an explicit `.height(...)`
/// beats both.
pub const DEFAULT_HEIGHT: f32 = 6.0;

// The "thinner than progress" relationship above is part of the widget's
// identity, so pin it at compile time rather than in a test.
const _: () = assert!(DEFAULT_HEIGHT < crate::widgets::progress::DEFAULT_HEIGHT);

/// Threshold configuration for [`meter_with`].
///
/// A simplification of `<meter>`'s `low` / `high` / `optimum` triple:
/// the platform element uses `optimum` to decide *which* side of the
/// range is the good one, and derives three severity buckets from it.
/// Damascene fixes `optimum` at the **low end** — the overwhelmingly
/// common instrumentation reading, where a rising bar is a rising
/// problem (load, pressure, temperature, packet loss). A gauge where
/// high is *good* (signal strength, battery) wants a plain
/// [`meter_with_color`] instead of inverted thresholds.
///
/// Both fields are fractions of the same `0.0..=1.0` range as the
/// value, and both are optional. The resolved fill is:
///
/// | condition | fill |
/// |---|---|
/// | `high` set and `value > high` | [`tokens::DESTRUCTIVE`] |
/// | else `low` set and `value >= low` | [`tokens::WARNING`] |
/// | else, with either threshold set | [`tokens::SUCCESS`] |
/// | neither threshold set | [`tokens::PRIMARY`] |
///
/// So `low` alone gives a two-band success/warning gauge, `high` alone
/// a success/destructive one, and both the full three-band reading.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterOpts {
    /// Bottom of the "acceptable but no longer nominal" band. Values
    /// below it read [`tokens::SUCCESS`]; values at or above it read at
    /// least [`tokens::WARNING`].
    pub low: Option<f32>,
    /// Top of the acceptable range. Values above it read
    /// [`tokens::DESTRUCTIVE`].
    pub high: Option<f32>,
}

impl MeterOpts {
    /// Set the nominal/warning boundary (see [`MeterOpts::low`]).
    pub fn low(mut self, v: f32) -> Self {
        self.low = Some(v);
        self
    }

    /// Set the warning/critical boundary (see [`MeterOpts::high`]).
    pub fn high(mut self, v: f32) -> Self {
        self.high = Some(v);
        self
    }

    /// The fill color this configuration resolves to for `value` — the
    /// table in [`MeterOpts`]. Exposed so an author can tint a
    /// neighbouring readout (the numeric label next to the bar) with
    /// the same severity color the bar picked.
    pub fn fill_color(&self, value: f32) -> Color {
        let value = sanitize(value);
        match (self.low, self.high) {
            (None, None) => tokens::PRIMARY,
            (low, high) => {
                if high.is_some_and(|high| value > high) {
                    tokens::DESTRUCTIVE
                } else if low.is_some_and(|low| value >= low) {
                    tokens::WARNING
                } else {
                    tokens::SUCCESS
                }
            }
        }
    }
}

/// A level / measurement bar (the web platform's `<meter>`). `value` is
/// clamped to `0.0..=1.0`; the returned `El` fills its container's
/// width at a fixed [`DEFAULT_HEIGHT`], so callers in dense chrome
/// normally pin a width (`.width(Size::Fixed(56.0))`).
///
/// Anatomy: a [`tokens::MUTED`] track at [`tokens::RADIUS_PILL`] with a
/// [`tokens::PRIMARY`] fill of the same radius covering `value` of its
/// width. No animation and no indeterminate mode — see the module docs
/// for why.
///
/// ```ignore
/// // Audio input level, the case that motivated this widget.
/// meter(0.62)
/// ```
///
/// Use [`meter_with_color`] for a per-channel tint and [`meter_with`]
/// for threshold coloring.
#[track_caller]
pub fn meter(value: f32) -> El {
    build(value, tokens::PRIMARY).at_loc(Location::caller())
}

/// A [`meter`] whose filled portion uses `fill_color` instead of
/// [`tokens::PRIMARY`] — for gauges distinguished by channel rather
/// than by severity (an `IN` / `OUT` pair of audio levels, per-core CPU
/// bars).
#[track_caller]
pub fn meter_with_color(value: f32, fill_color: Color) -> El {
    build(value, fill_color).at_loc(Location::caller())
}

/// A [`meter`] whose filled portion takes its color from `opts`'
/// thresholds — see [`MeterOpts`] for the exact bands.
///
/// ```ignore
/// // Nominal below 60%, warning to 85%, critical above.
/// meter_with(load, MeterOpts::default().low(0.6).high(0.85))
/// ```
#[track_caller]
pub fn meter_with(value: f32, opts: MeterOpts) -> El {
    // Threshold bands read the *clamped* value, so an out-of-range
    // sample can't paint a full bar in a milder color than it deserves.
    let value = sanitize(value);
    build(value, opts.fill_color(value)).at_loc(Location::caller())
}

/// Fold a raw reading into `0.0..=1.0`. Unlike `f32::clamp`, NaN maps to
/// an empty bar instead of propagating: meters are fed live samples, and
/// a dB-to-fraction conversion of digital silence produces one.
fn sanitize(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

/// Shared anatomy for the three constructors. Kept private so the
/// public surface stays `meter` / `meter_with_color` / `meter_with`.
fn build(value: f32, fill_color: Color) -> El {
    let value = sanitize(value);
    let layout = move |ctx: LayoutCtx| {
        let r = ctx.container;
        vec![
            // Track spans the full container.
            Rect::new(r.x, r.y, r.w, r.h),
            // Fill spans the measured portion.
            Rect::new(r.x, r.y, r.w * value, r.h),
        ]
    };

    El::new(Kind::Custom("meter"))
        .axis(Axis::Overlay)
        .children([
            El::new(Kind::Custom("meter-track"))
                .fill(tokens::MUTED)
                .radius(tokens::RADIUS_PILL),
            El::new(Kind::Custom("meter-fill"))
                .fill(fill_color)
                .radius(tokens::RADIUS_PILL),
        ])
        .metrics_role(MetricsRole::Progress)
        .layout(layout)
        .width(Size::Fill(1.0))
        .default_height(Size::Fixed(DEFAULT_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;

    /// Lay a meter out in a 200x6 viewport and return the fill width.
    fn fill_width(mut tree: El) -> f32 {
        let mut state = UiState::new();
        layout(
            &mut tree,
            &mut state,
            Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT),
        );
        tree.children[1].computed_rect.w
    }

    #[test]
    fn track_and_fill_anatomy() {
        let m = meter(0.5);

        assert_eq!(m.kind, Kind::Custom("meter"));
        assert_eq!(m.axis, Axis::Overlay);
        assert_eq!(m.children.len(), 2);
        assert_eq!(m.children[0].kind, Kind::Custom("meter-track"));
        assert_eq!(m.children[0].fill, Some(tokens::MUTED), "track is muted");
        assert_eq!(m.children[1].kind, Kind::Custom("meter-fill"));
        assert_eq!(m.children[1].fill, Some(tokens::PRIMARY));
        // Full radius on both so the bar reads as one rounded piece.
        assert_eq!(m.children[0].radius, Corners::all(tokens::RADIUS_PILL));
        assert_eq!(m.children[1].radius, Corners::all(tokens::RADIUS_PILL));
        assert_eq!(m.width, Size::Fill(1.0));
        assert_eq!(m.height, Size::Fixed(DEFAULT_HEIGHT));
    }

    #[test]
    fn meter_carries_no_animation_or_shader() {
        // Instrumentation must not smear transients: no easing on the
        // fill, and no stock shader (which is how the indeterminate
        // progress loader animates).
        for m in [
            meter(0.4),
            meter_with_color(0.4, tokens::SUCCESS),
            meter_with(0.4, MeterOpts::default().low(0.2).high(0.9)),
        ] {
            assert!(m.animate_timing().is_none(), "meter root must not animate");
            assert!(m.shader_override.is_none(), "meter binds no shader");
            for child in &m.children {
                assert!(
                    child.animate_timing().is_none(),
                    "meter {:?} must not animate",
                    child.kind
                );
                assert!(child.shader_override.is_none());
            }
        }
    }

    #[test]
    fn value_scales_the_fill() {
        assert!((fill_width(meter(0.25)) - 50.0).abs() < 1e-3);
        assert!((fill_width(meter(0.62)) - 124.0).abs() < 1e-3);
    }

    #[test]
    fn value_clamps_at_both_ends() {
        assert_eq!(fill_width(meter(-0.5)), 0.0, "negative clamps to empty");
        assert_eq!(fill_width(meter(1.5)), 200.0, "above one clamps to full");
        assert_eq!(fill_width(meter(f32::NAN)), 0.0, "NaN clamps to empty");
    }

    #[test]
    fn no_thresholds_reads_primary() {
        let opts = MeterOpts::default();
        assert_eq!(opts.low, None);
        assert_eq!(opts.high, None);
        for v in [0.0, 0.5, 1.0] {
            assert_eq!(opts.fill_color(v), tokens::PRIMARY);
        }
        assert_eq!(
            meter_with(0.5, opts).children[1].fill,
            Some(tokens::PRIMARY)
        );
    }

    #[test]
    fn thresholds_switch_fill_color_in_three_bands() {
        let opts = MeterOpts::default().low(0.6).high(0.85);

        assert_eq!(opts.fill_color(0.0), tokens::SUCCESS);
        assert_eq!(opts.fill_color(0.59), tokens::SUCCESS);
        // `low` is inclusive at the warning end.
        assert_eq!(opts.fill_color(0.6), tokens::WARNING);
        assert_eq!(opts.fill_color(0.85), tokens::WARNING, "`high` is inclusive");
        assert_eq!(opts.fill_color(0.86), tokens::DESTRUCTIVE);
        assert_eq!(opts.fill_color(1.0), tokens::DESTRUCTIVE);

        // …and the built El picks the band up.
        assert_eq!(
            meter_with(0.3, opts).children[1].fill,
            Some(tokens::SUCCESS)
        );
        assert_eq!(
            meter_with(0.7, opts).children[1].fill,
            Some(tokens::WARNING)
        );
        assert_eq!(
            meter_with(0.95, opts).children[1].fill,
            Some(tokens::DESTRUCTIVE)
        );
    }

    #[test]
    fn one_sided_thresholds_degrade_sensibly() {
        let low_only = MeterOpts::default().low(0.6);
        assert_eq!(low_only.fill_color(0.5), tokens::SUCCESS);
        assert_eq!(low_only.fill_color(0.99), tokens::WARNING);

        let high_only = MeterOpts::default().high(0.85);
        assert_eq!(high_only.fill_color(0.5), tokens::SUCCESS);
        assert_eq!(high_only.fill_color(0.9), tokens::DESTRUCTIVE);
    }

    #[test]
    fn thresholds_compare_against_the_clamped_value() {
        // An out-of-range reading must not fall through to a milder
        // band than the full bar it paints.
        let opts = MeterOpts::default().low(0.6).high(0.85);
        assert_eq!(
            meter_with(1.4, opts).children[1].fill,
            Some(tokens::DESTRUCTIVE)
        );
    }

    #[test]
    fn color_override_reaches_the_fill() {
        let m = meter_with_color(0.4, tokens::SUCCESS);
        assert_eq!(m.children[0].fill, Some(tokens::MUTED), "track unchanged");
        assert_eq!(m.children[1].fill, Some(tokens::SUCCESS));
    }
}
