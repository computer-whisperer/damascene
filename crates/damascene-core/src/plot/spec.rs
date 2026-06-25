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
//! [`line`]/[`scatter`] builders + [`PlotSpec::add_mark`]) for power users.

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

/// One mark in a plot. Construct with the [`line`]/[`scatter`] builders or
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
}

impl Default for PlotSpec {
    fn default() -> Self {
        Self {
            marks: Vec::new(),
            x: Axis::default(),
            y: Axis::default(),
            style: PlotStyle::default(),
            crosshair: false,
            y_autoscale: true,
            controls: PlotControls::default(),
            legend: None,
            downsample: None,
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
}
