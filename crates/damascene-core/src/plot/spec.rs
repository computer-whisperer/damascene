//! The declarative description of a plot: its marks, axes, and styling.
//!
//! [`PlotSpec`] is to [`plot`](crate::tree::plot) what
//! [`SceneSpec`](crate::scene::SceneSpec) is to `chart3d` — a fluent builder
//! the app assembles each frame and hands to the widget, which resolves it
//! (in `draw_ops`) to the reused [`DrawOp::Scene3D`](crate::ir::DrawOp) data
//! layer plus themed axis/grid/legend chrome.
//!
//! Following the house two-tier convention, each mark has a terse
//! default-style adder on the builder ([`PlotSpec::line`],
//! [`PlotSpec::scatter`]) and a full-control escape hatch (the standalone
//! [`line()`]/[`scatter`] builders + [`PlotSpec::add_mark`]) for power users.

#![warn(missing_docs)]

use crate::color::Color;
use crate::plot::scale::Scale;
use crate::plot::series::SeriesHandle;
use crate::scene::style::PointShape;

/// Pointer control scheme for a [`plot`](crate::tree::plot), the 2D analogue
/// of [`CameraControls`](crate::scene::CameraControls) for `chart3d`: the app
/// picks one on the spec and there is deliberately no built-in scheme-picker
/// widget. Double-click always resets to the full extent and the wheel always
/// zooms the time (X) axis, regardless of scheme; the scheme selects what the
/// primary (unmodified) left-drag does, with `Shift` doing the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlotControls {
    /// Scientific default (uPlot / Grafana / InfluxDB): left-drag draws a
    /// directional zoom box (X or Y by the larger drag delta), Shift+left-drag
    /// pans.
    #[default]
    ZoomDrag,
    /// Pan-first (trading charts / maps): left-drag pans, Shift+left-drag draws
    /// a directional zoom box.
    PanDrag,
}

/// How a line mark interpolates between consecutive samples — the d3
/// `curve` vocabulary. The step modes draw square-edged sample-and-hold
/// traces (digital signals, counters, rate steps) without the app
/// synthesizing duplicate-x riser samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Curve {
    /// Straight segments between samples (the default).
    #[default]
    Linear,
    /// Hold each sample's value until the next sample, then jump — d3's
    /// `curveStepAfter`. The natural mode for transition-stamped data
    /// (a logic analyzer's "level `y` from time `x` onward").
    StepAfter,
    /// Jump to the next sample's value immediately after each sample —
    /// d3's `curveStepBefore`.
    StepBefore,
    /// Hold until halfway between samples, then jump — d3's `curveStep`.
    /// The midpoint is halfway in **scale space** (the visual middle, which
    /// on a log axis is the geometric mean).
    StepMid,
}

impl Curve {
    /// Whether this curve draws square-edged steps (any non-linear mode).
    pub fn is_step(self) -> bool {
        self != Curve::Linear
    }
}

/// Default line width, in screen pixels.
pub const DEFAULT_LINE_WIDTH: f32 = 1.5;
/// Default scatter marker size, in screen pixels.
pub const DEFAULT_MARKER_SIZE: f32 = 5.0;

/// A line mark: a series drawn as a connected polyline (segments + round
/// join/cap discs, per the reuse-the-pipelines decision). `color = None`
/// auto-assigns from the theme's series palette at resolve time.
#[derive(Clone, Debug)]
pub struct LineMark {
    /// The data this line reads from.
    pub series: SeriesHandle,
    /// Stroke colour, or `None` to auto-assign a series colour.
    pub color: Option<Color>,
    /// Stroke width, in screen pixels.
    pub width: f32,
    /// How the line interpolates between samples (see [`Curve`]).
    pub curve: Curve,
    /// Display name for the legend / cursor readout, or `None` to fall back to
    /// `"Series N"`.
    pub label: Option<String>,
}

impl LineMark {
    /// Set the stroke colour.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the stroke width (screen pixels).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Set the sample interpolation (see [`Curve`]).
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Draw sample-and-hold square steps ([`Curve::StepAfter`]).
    pub fn step_after(self) -> Self {
        self.curve(Curve::StepAfter)
    }

    /// Draw jump-then-hold square steps ([`Curve::StepBefore`]).
    pub fn step_before(self) -> Self {
        self.curve(Curve::StepBefore)
    }

    /// Draw square steps switching midway between samples
    /// ([`Curve::StepMid`]).
    pub fn step_mid(self) -> Self {
        self.curve(Curve::StepMid)
    }

    /// Set the series' display name (legend + cursor readout).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// A scatter mark: a series drawn as discrete markers. `color = None`
/// auto-assigns from the theme's series palette at resolve time.
#[derive(Clone, Debug)]
pub struct ScatterMark {
    /// The data this scatter reads from.
    pub series: SeriesHandle,
    /// Marker colour, or `None` to auto-assign a series colour.
    pub color: Option<Color>,
    /// Marker size, in screen pixels.
    pub size: f32,
    /// Marker shape.
    pub shape: PointShape,
    /// Display name for the legend / cursor readout, or `None` to fall back to
    /// `"Series N"`.
    pub label: Option<String>,
}

impl ScatterMark {
    /// Set the marker colour.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the marker size (screen pixels).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Set the marker shape.
    pub fn shape(mut self, shape: PointShape) -> Self {
        self.shape = shape;
        self
    }

    /// Set the series' display name (legend + cursor readout).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// One mark in a plot. Construct with the [`line()`]/[`scatter`] builders or
/// the [`PlotSpec`] adders.
#[derive(Clone, Debug)]
pub enum Mark {
    /// A connected line series.
    Line(LineMark),
    /// A discrete-marker scatter series.
    Scatter(ScatterMark),
}

impl Mark {
    /// The series this mark reads from.
    pub fn series(&self) -> &SeriesHandle {
        match self {
            Mark::Line(m) => &m.series,
            Mark::Scatter(m) => &m.series,
        }
    }

    /// The mark's explicit colour, if set.
    pub fn explicit_color(&self) -> Option<Color> {
        match self {
            Mark::Line(m) => m.color,
            Mark::Scatter(m) => m.color,
        }
    }

    /// The mark's sample interpolation: a line mark's [`Curve`], `None` for
    /// marks that draw nothing between samples (scatter).
    pub fn curve(&self) -> Option<Curve> {
        match self {
            Mark::Line(m) => Some(m.curve),
            Mark::Scatter(_) => None,
        }
    }

    /// The mark's resolved colour: its explicit colour, or the palette colour
    /// for series index `i`. Shared by the data layer, legend, and cursor so
    /// a series is one colour everywhere.
    pub fn color_at(&self, i: usize) -> Color {
        self.explicit_color()
            .unwrap_or_else(|| crate::plot::palette::series_color(i))
    }

    /// The mark's display name: its explicit label, or `"Series {i+1}"`.
    pub fn display_label(&self, i: usize) -> String {
        let explicit = match self {
            Mark::Line(m) => m.label.as_deref(),
            Mark::Scatter(m) => m.label.as_deref(),
        };
        explicit
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Series {}", i + 1))
    }
}

impl From<LineMark> for Mark {
    fn from(m: LineMark) -> Self {
        Mark::Line(m)
    }
}

impl From<ScatterMark> for Mark {
    fn from(m: ScatterMark) -> Self {
        Mark::Scatter(m)
    }
}

/// Build a default-styled [`LineMark`] from a series — the standalone
/// escape-hatch form (chain `.color(...)` / `.width(...)`, then add with
/// [`PlotSpec::add_mark`]).
pub fn line(series: &SeriesHandle) -> LineMark {
    LineMark {
        series: series.clone(),
        color: None,
        width: DEFAULT_LINE_WIDTH,
        curve: Curve::default(),
        label: None,
    }
}

/// Build a default-styled [`ScatterMark`] from a series — the standalone
/// escape-hatch form.
pub fn scatter(series: &SeriesHandle) -> ScatterMark {
    ScatterMark {
        series: series.clone(),
        color: None,
        size: DEFAULT_MARKER_SIZE,
        shape: PointShape::Circle,
        label: None,
    }
}

/// How a [`Lane`] maps its data-space values into its band of the stack.
///
/// Each lane normalizes its own data independently, so one lane's data can
/// never move another lane's framing — the structural fix for the
/// autoscale coupling a shared numeric Y axis imposes on faked lanes.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LaneDomain {
    /// Fit the lane's own data visible in the current X window each frame —
    /// the per-lane analogue of [`PlotSpec::y_autoscale`], and the default
    /// for analog telemetry.
    #[default]
    Auto,
    /// A fixed `0..=1` domain — the logic-analyzer digital channel. The
    /// normalization is constant, so the lane's geometry is revision-stable
    /// and vertical stack scrolling never touches its vertex buffer.
    Digital,
    /// A fixed data window `min..=max` (set via [`Lane::y_window`]). Values
    /// outside the window peg to the band edge, like a saturated scope
    /// trace.
    Fixed(f64, f64),
}

/// One horizontal band of a lane plot: a labelled track holding ordinary
/// marks, stacked top-down in declaration order and sharing the plot's X
/// axis. The logic-analyzer / swimlane / multi-channel-telemetry shape.
///
/// Two-tier construction as everywhere else: [`Lane::digital`] is the terse
/// sample-and-hold channel; [`Lane::new`] plus [`mark`](Self::mark) /
/// [`height`](Self::height) / [`y_window`](Self::y_window) is full control.
#[derive(Clone, Debug)]
pub struct Lane {
    /// The lane's display name, drawn in the left gutter (and doubling as
    /// the legend).
    pub label: String,
    /// The marks drawn inside this lane's band, in order.
    pub marks: Vec<Mark>,
    /// Height weight relative to the other lanes (default `1.0`): a lane
    /// with weight `2.0` is twice as tall as one with `1.0`.
    pub height: f64,
    /// How the lane's data maps into its band (see [`LaneDomain`]).
    pub domain: LaneDomain,
}

impl Lane {
    /// An empty lane with the given label, unit height, and a per-lane
    /// [`LaneDomain::Auto`] domain. Add marks with [`mark`](Self::mark).
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            marks: Vec::new(),
            height: 1.0,
            domain: LaneDomain::default(),
        }
    }

    /// The terse logic-analyzer channel: a [`Curve::StepAfter`] line over
    /// `series` with the fixed [`LaneDomain::Digital`] `0..=1` domain (the
    /// revision-stable fast path — scrolling the stack never re-uploads it).
    pub fn digital(label: impl Into<String>, series: &SeriesHandle) -> Self {
        Self {
            label: label.into(),
            marks: vec![Mark::Line(line(series).step_after())],
            height: 1.0,
            domain: LaneDomain::Digital,
        }
    }

    /// Add a mark to the lane (e.g. `Lane::new("VBUS").mark(line(&vbus))`).
    pub fn mark(mut self, mark: impl Into<Mark>) -> Self {
        self.marks.push(mark.into());
        self
    }

    /// Set the lane's height weight (default `1.0`).
    pub fn height(mut self, weight: f64) -> Self {
        self.height = weight;
        self
    }

    /// Pin the lane to a fixed data window ([`LaneDomain::Fixed`]) — e.g.
    /// `.y_window(0.0, 5.2)` for a 5 V rail.
    pub fn y_window(mut self, min: f64, max: f64) -> Self {
        self.domain = LaneDomain::Fixed(min, max);
        self
    }

    /// Set the lane's domain directly (the escape hatch; the common cases
    /// are [`Lane::digital`] and [`y_window`](Self::y_window)).
    pub fn domain(mut self, domain: LaneDomain) -> Self {
        self.domain = domain;
        self
    }
}

/// Which wheel gestures scroll a lane plot's stack vertically (the tall
/// 100-channel case). Both default on; the plain wheel over the data rect
/// stays X zoom regardless.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackScroll {
    /// Shift+wheel over the data rect scrolls the stack.
    pub shift_wheel: bool,
    /// The wheel over the lane-label gutter scrolls the stack.
    pub gutter_wheel: bool,
}

impl Default for StackScroll {
    fn default() -> Self {
        Self {
            shift_wheel: true,
            gutter_wheel: true,
        }
    }
}

/// Where a plot's legend sits — a corner of the data rect. Set via
/// [`PlotSpec::legend`]; the spec's `legend` is `None` (no legend) by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LegendPosition {
    /// Top-right corner — the default when a legend is enabled.
    #[default]
    TopRight,
    /// Top-left corner.
    TopLeft,
    /// Bottom-right corner.
    BottomRight,
    /// Bottom-left corner.
    BottomLeft,
}

/// One axis's configuration: its [`Scale`] and an optional title.
#[derive(Clone, Debug)]
pub struct Axis {
    /// The data→scale-space warp + tick policy for this axis.
    pub scale: Scale,
    /// An axis title drawn beside the ticks, or `None`.
    pub title: Option<String>,
}

impl Axis {
    /// An axis with the given scale and no title.
    pub fn new(scale: Scale) -> Self {
        Self { scale, title: None }
    }

    /// Set the axis title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl Default for Axis {
    fn default() -> Self {
        Self::new(Scale::linear())
    }
}

/// Plot-level styling: background, gridlines, and MSAA. Maps to the reused
/// scene's [`SceneStyle`](crate::scene::SceneStyle) at resolve time (the
/// plot draws its own axis/grid chrome, so the scene's own grid/axes are
/// left off).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotStyle {
    /// Background fill of the data rect, or `None` for transparent.
    pub background: Option<Color>,
    /// Whether to draw gridlines at the axis ticks.
    pub grid: bool,
    /// MSAA sample count for the offscreen data layer (`1` or `4`).
    pub msaa_samples: u32,
}

impl Default for PlotStyle {
    fn default() -> Self {
        Self {
            background: None,
            grid: true,
            msaa_samples: 4,
        }
    }
}

/// The full declarative description of a plot: marks, per-axis scales, view
/// behaviour, and styling. Assemble fluently and pass to
/// [`plot`](crate::tree::plot):
///
/// ```ignore
/// plot(
///     PlotSpec::new()
///         .x(Scale::time())
///         .line(&cpu)
///         .line(&mem)
///         .crosshair(true),
/// )
/// .key("metrics")
/// ```
#[derive(Clone, Debug)]
pub struct PlotSpec {
    /// The marks, drawn in order.
    pub marks: Vec<Mark>,
    /// The lanes, stacked top-down in declaration order. Non-empty makes
    /// this a **lane plot**: the vertical axis becomes a window over the
    /// lane stack ([`PlotView::y`](crate::plot::PlotView) in stack space),
    /// each lane normalizes its own data into its band, and the Y chrome
    /// becomes lane labels + separators instead of numeric ticks. Top-level
    /// `marks` and the `y` axis configuration are ignored on a lane plot
    /// (the lint pass flags both).
    pub lanes: Vec<Lane>,
    /// Horizontal-axis configuration.
    pub x: Axis,
    /// Vertical-axis configuration.
    pub y: Axis,
    /// Plot-level styling.
    pub style: PlotStyle,
    /// Whether to show a crosshair reading out the nearest sample.
    pub crosshair: bool,
    /// Whether the vertical axis auto-fits to the data visible in the
    /// current horizontal window each frame (vs. a fixed Y window).
    pub y_autoscale: bool,
    /// Whether the horizontal axis auto-fits to the full data extent each
    /// frame (vs. sticking at the window it was first seeded with). On by
    /// default so a plot whose series grow — a streaming dashboard fed by
    /// [`SeriesHandle::append`](crate::plot::SeriesHandle::append) — keeps
    /// the new samples in view. Any manual X gesture (pan, wheel zoom, X
    /// box-zoom) or [`crate::state::UiState::set_plot_view`] takes manual
    /// control and stops the tracking until a double-click reset or a
    /// [`PlotRequest::FitAll`](crate::plot::PlotRequest::FitAll). With
    /// static data the re-fit resolves to the same window every frame, so
    /// this is invisible for non-streaming plots.
    pub x_autoscale: bool,
    /// Pointer control scheme — what the primary drag does (see
    /// [`PlotControls`]).
    pub controls: PlotControls,
    /// Legend placement, or `None` for no legend.
    pub legend: Option<LegendPosition>,
    /// Library-side down-sampling for over-dense line series (the
    /// dump-everything path). `None` draws every sample; `Some` reduces each
    /// line to the pixel budget over the visible window before upload. A
    /// virtual app that resamples its own source leaves this `None`.
    pub downsample: Option<crate::plot::Decimation>,
    /// Lane plots only: whether a Y box-zoom snaps outward to lane
    /// boundaries (select lanes, never half a lane). On by default; turn
    /// off for continuous stack-space selection.
    pub lane_zoom_snap: bool,
    /// Lane plots only: which wheel gestures scroll the stack vertically
    /// (see [`StackScroll`]; both on by default).
    pub stack_scroll: StackScroll,
    /// Lane plots only: the readable-lane floor for the *initial* view and
    /// the double-click reset, in logical px. With more lanes than fit at
    /// this height, the view clamps to a scrollable window anchored at the
    /// top — like a list — instead of an unreadable fit-all.
    pub min_lane_px: f32,
}

/// Default [`PlotSpec::min_lane_px`]: a compact list-row height.
pub const DEFAULT_MIN_LANE_PX: f32 = 24.0;

impl Default for PlotSpec {
    fn default() -> Self {
        Self {
            marks: Vec::new(),
            lanes: Vec::new(),
            x: Axis::default(),
            y: Axis::default(),
            style: PlotStyle::default(),
            crosshair: false,
            y_autoscale: true,
            x_autoscale: true,
            controls: PlotControls::default(),
            legend: None,
            downsample: None,
            lane_zoom_snap: true,
            stack_scroll: StackScroll::default(),
            min_lane_px: DEFAULT_MIN_LANE_PX,
        }
    }
}

impl PlotSpec {
    /// An empty spec with default axes and styling.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a default-styled line mark for `series`.
    pub fn line(mut self, series: &SeriesHandle) -> Self {
        self.marks.push(Mark::Line(line(series)));
        self
    }

    /// Add a default-styled scatter mark for `series`.
    pub fn scatter(mut self, series: &SeriesHandle) -> Self {
        self.marks.push(Mark::Scatter(scatter(series)));
        self
    }

    /// Add a fully-built mark (the escape hatch for styled marks, e.g.
    /// `spec.add_mark(line(&h).color(c).width(2.0))`).
    pub fn add_mark(mut self, mark: impl Into<Mark>) -> Self {
        self.marks.push(mark.into());
        self
    }

    /// Add a [`Lane`] (see [`PlotSpec::lanes`]): the plot becomes a lane
    /// plot, stacking lanes top-down in declaration order.
    pub fn lane(mut self, lane: Lane) -> Self {
        self.lanes.push(lane);
        self
    }

    /// Whether this spec describes a lane plot (any lanes declared).
    pub fn is_lane_plot(&self) -> bool {
        !self.lanes.is_empty()
    }

    /// Lane plots: set whether a Y box-zoom snaps outward to lane
    /// boundaries (default `true`).
    pub fn lane_zoom_snap(mut self, on: bool) -> Self {
        self.lane_zoom_snap = on;
        self
    }

    /// Lane plots: configure which wheel gestures scroll the stack (see
    /// [`StackScroll`]).
    pub fn stack_scroll(mut self, scroll: StackScroll) -> Self {
        self.stack_scroll = scroll;
        self
    }

    /// Lane plots: set the readable-lane floor for the initial / reset view
    /// (default [`DEFAULT_MIN_LANE_PX`]).
    pub fn min_lane_px(mut self, px: f32) -> Self {
        self.min_lane_px = px;
        self
    }

    /// Set the horizontal-axis [`Scale`] (keeps any existing title).
    pub fn x(mut self, scale: Scale) -> Self {
        self.x.scale = scale;
        self
    }

    /// Set the vertical-axis [`Scale`] (keeps any existing title).
    pub fn y(mut self, scale: Scale) -> Self {
        self.y.scale = scale;
        self
    }

    /// Replace the whole horizontal-axis configuration.
    pub fn x_axis(mut self, axis: Axis) -> Self {
        self.x = axis;
        self
    }

    /// Replace the whole vertical-axis configuration.
    pub fn y_axis(mut self, axis: Axis) -> Self {
        self.y = axis;
        self
    }

    /// Set whether to draw the crosshair / nearest-sample readout.
    pub fn crosshair(mut self, on: bool) -> Self {
        self.crosshair = on;
        self
    }

    /// Set whether the vertical axis auto-scales to the visible window.
    pub fn y_autoscale(mut self, on: bool) -> Self {
        self.y_autoscale = on;
        self
    }

    /// Set whether the horizontal axis auto-scales to the full data extent
    /// each frame (see [`Self::x_autoscale`]; on by default). Turn this off
    /// for a plot that should hold a fixed time window regardless of what
    /// the series contain.
    pub fn x_autoscale(mut self, on: bool) -> Self {
        self.x_autoscale = on;
        self
    }

    /// Set the pointer control scheme (what the primary drag does). Defaults
    /// to [`PlotControls::ZoomDrag`].
    pub fn controls(mut self, controls: PlotControls) -> Self {
        self.controls = controls;
        self
    }

    /// Show a legend at `position`. Series are labelled by their
    /// [`label`](LineMark::label), falling back to `"Series N"`.
    pub fn legend(mut self, position: LegendPosition) -> Self {
        self.legend = Some(position);
        self
    }

    /// Set whether gridlines are drawn.
    pub fn grid(mut self, on: bool) -> Self {
        self.style.grid = on;
        self
    }

    /// Enable library-side down-sampling of line series to the pixel budget
    /// (the dump-everything path). Leave unset for the virtual-data flow.
    pub fn downsample(mut self, decimation: crate::plot::Decimation) -> Self {
        self.downsample = Some(decimation);
        self
    }

    /// Set the data-rect background fill.
    pub fn background(mut self, color: Color) -> Self {
        self.style.background = Some(color);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle() -> SeriesHandle {
        SeriesHandle::new(vec![crate::plot::series::Sample::new(0.0, 0.0)])
    }

    #[test]
    fn builder_collects_marks_and_axes() {
        let h = handle();
        let spec = PlotSpec::new()
            .x(Scale::time())
            .y(Scale::linear())
            .line(&h)
            .scatter(&h)
            .crosshair(true);
        assert_eq!(spec.marks.len(), 2);
        assert!(matches!(spec.marks[0], Mark::Line(_)));
        assert!(matches!(spec.marks[1], Mark::Scatter(_)));
        assert_eq!(spec.x.scale, Scale::time());
        assert!(spec.crosshair);
        assert!(spec.y_autoscale); // default
    }

    #[test]
    fn styled_marks_via_escape_hatch() {
        let h = handle();
        let red = Color::srgb_u8(220, 40, 40);
        let spec = PlotSpec::new().add_mark(line(&h).color(red).width(3.0));
        match &spec.marks[0] {
            Mark::Line(m) => {
                assert_eq!(m.color, Some(red));
                assert_eq!(m.width, 3.0);
            }
            _ => panic!("expected a line mark"),
        }
    }

    #[test]
    fn default_line_is_auto_colored() {
        let h = handle();
        let m = line(&h);
        assert_eq!(m.color, None);
        assert_eq!(m.width, DEFAULT_LINE_WIDTH);
    }

    #[test]
    fn lane_builders_and_defaults() {
        let h = handle();
        // Terse digital shorthand: step-after line, digital domain.
        let d = Lane::digital("GP0 · PPS", &h);
        assert_eq!(d.label, "GP0 · PPS");
        assert_eq!(d.domain, LaneDomain::Digital);
        assert_eq!(d.height, 1.0);
        assert!(
            matches!(&d.marks[0], Mark::Line(m) if m.curve == Curve::StepAfter),
            "digital lane is a step-after line"
        );
        // Full-control builder.
        let a = Lane::new("VBUS (V)")
            .mark(line(&h))
            .height(3.0)
            .y_window(0.0, 5.2);
        assert_eq!(a.height, 3.0);
        assert_eq!(a.domain, LaneDomain::Fixed(0.0, 5.2));
        // Spec-level wiring + knob defaults.
        let spec = PlotSpec::new().lane(d).lane(a);
        assert!(spec.is_lane_plot());
        assert_eq!(spec.lanes.len(), 2);
        assert!(spec.lane_zoom_snap);
        assert_eq!(spec.stack_scroll, StackScroll::default());
        assert!(spec.stack_scroll.shift_wheel && spec.stack_scroll.gutter_wheel);
        assert_eq!(spec.min_lane_px, DEFAULT_MIN_LANE_PX);
        assert!(!PlotSpec::new().line(&handle()).is_lane_plot());
    }

    #[test]
    fn step_builders_set_the_curve() {
        let h = handle();
        assert_eq!(line(&h).curve, Curve::Linear);
        assert_eq!(line(&h).step_after().curve, Curve::StepAfter);
        assert_eq!(line(&h).step_before().curve, Curve::StepBefore);
        assert_eq!(line(&h).step_mid().curve, Curve::StepMid);
        assert!(Curve::StepAfter.is_step() && !Curve::Linear.is_step());
        // Lines expose their curve through the mark; scatters have none.
        assert_eq!(
            Mark::from(line(&h).step_mid()).curve(),
            Some(Curve::StepMid)
        );
        assert_eq!(Mark::from(scatter(&h)).curve(), None);
    }
}
