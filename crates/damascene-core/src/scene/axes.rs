//! Axis labelling for a [`Scene3D`](crate::scene): titles and numeric tick
//! labels placed at world positions along the X/Y/Z axes.
//!
//! This is *presentation only*. The app authors geometry in world
//! coordinates as always; an axis describes how to **label** those world
//! positions — optionally remapping the world coordinate to a data range
//! for display ([`AxisRange::Linear`]). No coordinate system is imposed on
//! the scene.
//!
//! The config lives on [`SceneSpec`](crate::scene::SceneSpec) (it carries
//! `String` titles, so it is `Clone`, not `Copy` like
//! [`SceneStyle`](crate::scene::style::SceneStyle)). It is consumed by the
//! draw-op pass, which projects the [`labels`](Axes::labels) through the
//! resolved camera and emits text — so labels render on every backend
//! through the normal text pipeline, never as scene GPU geometry.

// Lock in full per-item documentation for this module (issue #73).
#![warn(missing_docs)]

use glam::Vec3;

use crate::color::Color;
use crate::scene::style::GridSettings;

/// Cap on ticks generated per axis, guarding against a tiny step or huge
/// count producing a runaway label list.
const MAX_TICKS_PER_AXIS: usize = 64;

/// The three world axes a label set can address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisKind {
    /// The world X axis.
    X,
    /// The world Y axis.
    Y,
    /// The world Z axis.
    Z,
}

impl AxisKind {
    /// Unit direction of the axis in world space.
    pub fn direction(self) -> Vec3 {
        match self {
            AxisKind::X => Vec3::X,
            AxisKind::Y => Vec3::Y,
            AxisKind::Z => Vec3::Z,
        }
    }

    /// Index into a `[x, y, z]` triple (matches [`GridSettings::axis_spans`]).
    fn index(self) -> usize {
        match self {
            AxisKind::X => 0,
            AxisKind::Y => 1,
            AxisKind::Z => 2,
        }
    }
}

/// How an axis turns a world coordinate into the value shown on a tick.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum AxisRange {
    /// The label value *is* the world coordinate along the axis.
    #[default]
    World,
    /// Linearly remap the world coordinate for display only. The world
    /// span `world_span` maps onto the data range `data`; `world_span`
    /// defaults to the axis's drawn `[min, max]` span when `None`.
    Linear {
        /// World `(min, max)` of the mapping domain; `None` uses the
        /// axis's drawn span.
        world_span: Option<(f32, f32)>,
        /// Data `(min, max)` shown at the ends of the world span.
        data: (f32, f32),
    },
}

impl AxisRange {
    /// Map a world coordinate along the axis to the displayed value.
    /// `axis_span` is the axis's drawn `[min, max]`, the default mapping
    /// domain when `Linear { world_span: None }`.
    fn value_at(self, world_coord: f32, axis_span: (f32, f32)) -> f32 {
        match self {
            AxisRange::World => world_coord,
            AxisRange::Linear { world_span, data } => {
                let (ws0, ws1) = world_span.unwrap_or(axis_span);
                let span = ws1 - ws0;
                if span.abs() <= f32::EPSILON {
                    data.0
                } else {
                    let t = (world_coord - ws0) / span;
                    data.0 + t * (data.1 - data.0)
                }
            }
        }
    }
}

/// Where tick marks fall along an axis.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum TickPolicy {
    /// One tick per major grid line (`grid.spacing`) — labels line up with
    /// the reference grid. The default.
    #[default]
    FromGrid,
    /// `n` evenly spaced ticks spanning the axis extent.
    Count(u32),
    /// A tick every `step` world units.
    Step(f32),
}

/// How a tick's numeric value is rendered to text.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum TickFormat {
    /// Integers when whole, otherwise trimmed decimals.
    #[default]
    Auto,
    /// Fixed number of decimal places.
    Decimal(u8),
    /// Rounded to the nearest integer.
    Integer,
    /// Scientific notation, e.g. `1.2e3`.
    Scientific,
    /// Value × 100 with a `%` suffix.
    Percent,
    /// `Auto` formatting with a trailing unit suffix, e.g. `" px"`.
    Suffix(String),
}

impl TickFormat {
    /// Render a value to its label string.
    pub fn format(&self, v: f32) -> String {
        match self {
            TickFormat::Auto => format_auto(v),
            TickFormat::Decimal(p) => format!("{:.*}", *p as usize, v),
            TickFormat::Integer => format!("{}", v.round() as i64),
            TickFormat::Scientific => format!("{:e}", v),
            TickFormat::Percent => format!("{}%", format_auto(v * 100.0)),
            TickFormat::Suffix(s) => format!("{}{s}", format_auto(v)),
        }
    }
}

/// Configuration for one axis.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisSpec {
    /// Whether this axis emits any labels. Defaults to `true`.
    pub visible: bool,
    /// Axis title placed past the positive end (e.g. `"Temp (°C)"`).
    pub title: Option<String>,
    /// How world coordinates map to displayed tick values.
    pub range: AxisRange,
    /// Where tick marks fall along the axis.
    pub ticks: TickPolicy,
    /// How tick values render to text.
    pub format: TickFormat,
}

impl Default for AxisSpec {
    fn default() -> Self {
        Self {
            visible: true,
            title: None,
            range: AxisRange::World,
            ticks: TickPolicy::FromGrid,
            format: TickFormat::Auto,
        }
    }
}

/// Axis labelling for a scene: per-axis specs plus shared label styling.
///
/// Build with [`Axes::default`] (all three axes, world-coordinate ticks at
/// the grid lines) and override per axis, or use the
/// [`titles`](Axes::titles) shorthand.
#[derive(Clone, Debug, PartialEq)]
pub struct Axes {
    /// Labelling spec for the X axis.
    pub x: AxisSpec,
    /// Labelling spec for the Y axis.
    pub y: AxisSpec,
    /// Labelling spec for the Z axis.
    pub z: AxisSpec,
    /// Authoring-space colour for tick + title text.
    pub label_color: Color,
    /// Label font size in logical pixels.
    pub label_size: f32,
}

impl Default for Axes {
    fn default() -> Self {
        Self {
            x: AxisSpec::default(),
            y: AxisSpec::default(),
            z: AxisSpec::default(),
            label_color: Color::srgb_u8a(208, 214, 226, 235),
            label_size: 11.0,
        }
    }
}

/// One placed label: a world position and the text to draw centred on it.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisLabel {
    /// World position the text is centred on (projected by the draw-op pass).
    pub world: Vec3,
    /// The rendered label text.
    pub text: String,
}

impl Axes {
    /// Convenience: a default `Axes` with the three axis titles set.
    pub fn titles(x: impl Into<String>, y: impl Into<String>, z: impl Into<String>) -> Self {
        let mut axes = Axes::default();
        axes.x.title = Some(x.into());
        axes.y.title = Some(y.into());
        axes.z.title = Some(z.into());
        axes
    }

    /// The spec for one axis.
    fn spec(&self, kind: AxisKind) -> &AxisSpec {
        match kind {
            AxisKind::X => &self.x,
            AxisKind::Y => &self.y,
            AxisKind::Z => &self.z,
        }
    }

    /// All tick + title labels across the visible axes, in world space.
    /// The draw-op pass projects these through the resolved camera; this
    /// function is pure (no camera, no rendering) so it unit-tests cleanly.
    pub fn labels(&self, grid: &GridSettings) -> Vec<AxisLabel> {
        let mut out = Vec::new();
        let spans = grid.axis_spans();
        for kind in [AxisKind::X, AxisKind::Y, AxisKind::Z] {
            let spec = self.spec(kind);
            if !spec.visible {
                continue;
            }
            let dir = kind.direction();
            let span = spans[kind.index()];
            for coord in tick_coords(spec.ticks, span, grid.spacing) {
                let value = spec.range.value_at(coord, span);
                out.push(AxisLabel {
                    world: dir * coord,
                    text: spec.format.format(value),
                });
            }
            if let Some(title) = &spec.title {
                // Sit the title just past the max (positive) end of the axis.
                let (lo, hi) = span;
                let beyond = hi + grid.spacing.abs().max((hi - lo) * 0.1);
                out.push(AxisLabel {
                    world: dir * beyond,
                    text: title.clone(),
                });
            }
        }
        out
    }
}

/// World coordinates of the ticks along one axis, within its `[min, max]`
/// span, skipping the origin (where the axes meet). Empty when the policy
/// can't produce ticks (non-positive step, or degenerate span).
fn tick_coords(policy: TickPolicy, span: (f32, f32), grid_spacing: f32) -> Vec<f32> {
    let (lo, hi) = (span.0.min(span.1), span.0.max(span.1));
    if hi - lo <= 0.0 {
        return Vec::new();
    }
    let mut coords = Vec::new();
    match policy {
        TickPolicy::FromGrid | TickPolicy::Step(_) => {
            let step = match policy {
                TickPolicy::Step(s) => s,
                _ => grid_spacing,
            };
            if step <= 0.0 {
                return Vec::new();
            }
            // Integer multiples of `step` within [lo, hi], aligned to origin.
            let k0 = (lo / step).ceil() as i64;
            let k1 = (hi / step).floor() as i64;
            for k in k0..=k1 {
                if k == 0 {
                    continue; // origin label is shared at the axis crossing
                }
                coords.push(k as f32 * step);
                if coords.len() >= 2 * MAX_TICKS_PER_AXIS {
                    break;
                }
            }
        }
        TickPolicy::Count(n) => {
            let n = (n as usize).min(MAX_TICKS_PER_AXIS);
            if n >= 2 {
                let width = hi - lo;
                for i in 0..n {
                    let t = i as f32 / (n - 1) as f32;
                    let coord = lo + t * width;
                    if coord.abs() > width * 1e-3 {
                        coords.push(coord);
                    }
                }
            }
        }
    }
    coords
}

/// Compact number formatting: whole numbers as integers, otherwise up to
/// three decimals with trailing zeros trimmed.
fn format_auto(v: f32) -> String {
    if (v - v.round()).abs() < 1e-4 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(spacing: f32, extent: f32) -> GridSettings {
        GridSettings {
            spacing,
            extent,
            ..GridSettings::default()
        }
    }

    #[test]
    fn from_grid_ticks_skip_origin_and_track_spacing() {
        let coords = tick_coords(TickPolicy::FromGrid, (-10.0, 10.0), 5.0);
        assert_eq!(coords, vec![-10.0, -5.0, 5.0, 10.0]);
    }

    #[test]
    fn count_ticks_are_evenly_spaced() {
        let coords = tick_coords(TickPolicy::Count(5), (-10.0, 10.0), 1.0);
        // -10, -5, (0 skipped), 5, 10
        assert_eq!(coords, vec![-10.0, -5.0, 5.0, 10.0]);
    }

    #[test]
    fn one_sided_span_ticks_stay_within_bounds() {
        // L*-style axis in [0, 100]: ticks only on the positive side, no
        // origin label (shared at the crossing), nothing below 0.
        let coords = tick_coords(TickPolicy::FromGrid, (0.0, 100.0), 25.0);
        assert_eq!(coords, vec![25.0, 50.0, 75.0, 100.0]);
        assert!(coords.iter().all(|&c| c > 0.0 && c <= 100.0));
        // Count policy over the same span spans [0, 100] inclusive of the max.
        let counted = tick_coords(TickPolicy::Count(5), (0.0, 100.0), 1.0);
        assert_eq!(counted, vec![25.0, 50.0, 75.0, 100.0]);
    }

    #[test]
    fn degenerate_policies_make_no_ticks() {
        assert!(tick_coords(TickPolicy::Step(0.0), (-10.0, 10.0), 1.0).is_empty());
        assert!(tick_coords(TickPolicy::FromGrid, (0.0, 0.0), 1.0).is_empty());
        assert!(tick_coords(TickPolicy::Count(1), (-10.0, 10.0), 1.0).is_empty());
    }

    #[test]
    fn tick_count_is_capped() {
        let coords = tick_coords(TickPolicy::Step(0.001), (-10.0, 10.0), 1.0);
        assert!(coords.len() <= 2 * MAX_TICKS_PER_AXIS);
    }

    #[test]
    fn world_range_labels_the_coordinate() {
        let axes = Axes::default();
        let labels = axes.labels(&grid(5.0, 10.0));
        // X axis tick at world x = 5 reads "5".
        let five = labels
            .iter()
            .find(|l| (l.world - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4)
            .unwrap();
        assert_eq!(five.text, "5");
    }

    #[test]
    fn linear_range_remaps_world_to_data() {
        // World x in [-10, 10] maps to data [0, 100]; x = 5 → 75.
        let r = AxisRange::Linear {
            world_span: None,
            data: (0.0, 100.0),
        };
        assert_eq!(r.value_at(5.0, (-10.0, 10.0)), 75.0);
        assert_eq!(r.value_at(-10.0, (-10.0, 10.0)), 0.0);
        assert_eq!(r.value_at(10.0, (-10.0, 10.0)), 100.0);
    }

    #[test]
    fn titles_sit_past_the_axis_and_are_included() {
        let axes = Axes::titles("Xs", "Ys", "Zs");
        let labels = axes.labels(&grid(1.0, 10.0));
        let x_title = labels.iter().find(|l| l.text == "Xs").unwrap();
        assert!(x_title.world.x > 10.0, "title sits past the positive end");
        assert_eq!(x_title.world.y, 0.0);
        assert_eq!(x_title.world.z, 0.0);
        assert!(labels.iter().any(|l| l.text == "Ys"));
        assert!(labels.iter().any(|l| l.text == "Zs"));
    }

    #[test]
    fn per_axis_bounds_clip_labels_and_title() {
        use crate::scene::style::AxisBounds;
        // Symmetric extent 100, spacing 25, but Y clipped to [0, 100].
        let mut g = grid(25.0, 100.0);
        g.bounds = AxisBounds {
            y: Some((0.0, 100.0)),
            ..Default::default()
        };
        let axes = Axes::titles("X", "Y", "Z");
        let labels = axes.labels(&g);

        // Every Y tick lies in (0, 100] — no negative half, no shared origin.
        let y_ticks: Vec<f32> = labels
            .iter()
            .filter(|l| l.world.x == 0.0 && l.world.z == 0.0 && l.text != "Y")
            .map(|l| l.world.y)
            .collect();
        assert_eq!(y_ticks, vec![25.0, 50.0, 75.0, 100.0]);

        // Y title sits just past the bound's max (100), not past +extent.
        let y_title = labels.iter().find(|l| l.text == "Y").unwrap();
        assert!(
            (y_title.world.y - 125.0).abs() < 1e-3,
            "title past max, got {:?}",
            y_title.world
        );

        // X stays symmetric — it still carries negative ticks.
        assert!(labels.iter().any(|l| l.world.x < 0.0));
    }

    #[test]
    fn invisible_axis_emits_nothing() {
        let mut axes = Axes::titles("Xs", "Ys", "Zs");
        axes.y.visible = false;
        let labels = axes.labels(&grid(1.0, 10.0));
        assert!(
            !labels
                .iter()
                .any(|l| l.world.x == 0.0 && l.world.y != 0.0 && l.text == "Ys")
        );
        // No Y ticks either: every label with a nonzero Y component is gone.
        assert!(!labels.iter().any(|l| l.world.y.abs() > 1e-6));
    }

    #[test]
    fn format_variants_render_expected_text() {
        assert_eq!(TickFormat::Auto.format(5.0), "5");
        assert_eq!(TickFormat::Auto.format(2.5), "2.5");
        assert_eq!(TickFormat::Decimal(2).format(1.23456), "1.23");
        assert_eq!(TickFormat::Integer.format(3.7), "4");
        assert_eq!(TickFormat::Percent.format(0.25), "25%");
        assert_eq!(TickFormat::Suffix(" px".into()).format(12.0), "12 px");
    }
}
