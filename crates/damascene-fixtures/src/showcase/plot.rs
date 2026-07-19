//! 2D plot — the `plot` widget end-to-end.
//!
//! A representative time-series view: two line series and a sparse event
//! scatter over a shared time axis, with auto-scaled value axis, gridlines,
//! tick labels, an app-positioned legend, and a multi-series crosshair
//! readout. It is described purely as data (a [`PlotSpec`]) and composited
//! through the core render pipeline — the data layer reuses the Scene3D
//! pipelines under an orthographic camera — so the *same* page renders on
//! wgpu, vulkano, and ash with zero host glue.
//!
//! The series are app-owned [`SeriesHandle`]s built once (in [`State::default`])
//! and only *referenced* each frame; the dense series opts into `MinMax`
//! decimation so the plot stays fast under the full sample count.

use damascene_core::plot::{
    Decimation, Lane, LegendPosition, PlotSpec, Sample, Scale, SeriesHandle, line, scatter,
};
use damascene_core::prelude::*;

pub struct State {
    /// A smooth periodic signal (a "CPU %").
    cpu: SeriesHandle,
    /// A bounded random walk (a "memory %").
    mem: SeriesHandle,
    /// Sparse event markers, drawn as a scatter.
    events: SeriesHandle,
    /// Digital channels for the lane plot (transition-stamped 0/1 levels).
    channels: Vec<(String, SeriesHandle)>,
    /// An analog rail for the lane plot's fixed-domain lane.
    vbus: SeriesHandle,
}

impl Default for State {
    fn default() -> Self {
        // A fixed one-hour window of epoch-seconds samples, so the page is
        // deterministic (no wall-clock dependency) and renders identically
        // across backends and screenshot runs.
        let base = 1_780_000_000.0_f64;
        let n = 20_000usize;
        let dt = 3600.0 / n as f64;

        let mut cpu = Vec::with_capacity(n);
        let mut mem = Vec::with_capacity(n);
        let mut events = Vec::new();

        // Tiny xorshift PRNG for the random walk — no rand dependency.
        let mut s = 0x9E37_79B9_7F4A_7C15_u64;
        let mut rand01 = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1u64 << 53) as f64
        };

        let mut walk = 50.0_f64;
        for i in 0..n {
            let t = base + i as f64 * dt;
            let x = i as f64 / n as f64;
            cpu.push(Sample::new(
                t,
                50.0 + 38.0 * (x * std::f64::consts::TAU * 3.0).sin(),
            ));
            walk = (walk + (rand01() - 0.5) * 5.0).clamp(0.0, 100.0);
            mem.push(Sample::new(t, walk));
            if i % 73 == 0 {
                events.push(Sample::new(t, 50.0));
            }
        }

        // Lane-plot data: a handful of digital channels as transition-
        // stamped (t, level) samples — the step curve draws the holds and
        // risers — plus an analog rail. Each channel toggles at a distinct
        // deterministic period so the traces read as real bus activity.
        let labels = ["PPS", "UART TX", "UART RX", "SPI CLK", "SPI MOSI", "IRQ"];
        let channels = labels
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let period = 40.0 + 37.0 * c as f64;
                let mut level = 0.0;
                let mut samples = vec![Sample::new(base, level)];
                let mut t = base;
                while t < base + 3600.0 {
                    t += period * (0.5 + rand01());
                    level = 1.0 - level;
                    samples.push(Sample::new(t, level));
                }
                (format!("GP{c} · {name}"), SeriesHandle::new(samples))
            })
            .collect();
        let vbus = (0..600)
            .map(|i| {
                let t = base + i as f64 * 6.0;
                let x = i as f64 / 600.0;
                Sample::new(t, 5.0 + 0.15 * (x * std::f64::consts::TAU * 7.0).sin())
            })
            .collect::<Vec<_>>();

        Self {
            cpu: SeriesHandle::new(cpu),
            mem: SeriesHandle::new(mem),
            events: SeriesHandle::new(events),
            channels,
            vbus: SeriesHandle::new(vbus),
        }
    }
}

pub fn view(state: &State) -> El {
    let spec = PlotSpec::new()
        .x(Scale::time())
        .y(Scale::linear())
        .add_mark(line(&state.cpu).width(2.0).label("CPU %"))
        .add_mark(line(&state.mem).width(2.0).label("Memory %"))
        .add_mark(scatter(&state.events).size(6.0).label("Events"))
        .downsample(Decimation::MinMax)
        .legend(LegendPosition::TopRight)
        .crosshair(true);

    // Lanes: the logic-analyzer / swimlane shape — one plot, one shared
    // time axis, each channel in its own labelled band.
    let mut lane_spec = PlotSpec::new().x(Scale::time()).crosshair(true);
    for (label, series) in &state.channels {
        lane_spec = lane_spec.lane(Lane::digital(label, series));
    }
    lane_spec = lane_spec.lane(
        Lane::new("VBUS (V)")
            .mark(line(&state.vbus).width(2.0))
            .height(2.0)
            .y_window(4.5, 5.5),
    );

    column([
        h1("2D plot"),
        paragraph(
            "The `plot` widget renders an interactive 2D chart — line and \
             scatter marks over time/linear/log axes, with auto-scaled value \
             axis, gridlines, an app-positioned legend, and a multi-series \
             crosshair readout — from a backend-neutral `PlotSpec`. The data \
             layer reuses the Scene3D pipelines under an orthographic camera, \
             so the same description composites identically on wgpu, vulkano, \
             and ash.",
        )
        .muted()
        .wrap_text(),
        text("drag a box to zoom (X or Y) · double-click to reset · shift-drag to pan · wheel to zoom time · hover for values")
            .small()
            .muted()
            .wrap_text(),
        plot(spec).key("showcase-plot").height(Size::Fill(3.0)),
        h1("Lane plot"),
        text("digital channels as step-curve lanes + a fixed-domain analog rail · shift-wheel or gutter-wheel to scroll the stack · Y box-zoom snaps to whole lanes")
            .small()
            .muted()
            .wrap_text(),
        plot(lane_spec)
            .key("showcase-lanes")
            .height(Size::Fill(2.0)),
    ])
    .gap(tokens::SPACE_3)
    .height(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_plots(el: &El) -> usize {
        usize::from(el.plot_source.is_some()) + el.children.iter().map(count_plots).sum::<usize>()
    }

    #[test]
    fn view_mounts_the_plot_widgets() {
        let el = view(&State::default());
        assert_eq!(
            count_plots(&el),
            2,
            "the plot section mounts the marks plot and the lane plot"
        );
    }

    #[test]
    fn lane_demo_is_a_lane_plot() {
        fn find_lanes(el: &El) -> Option<usize> {
            if let Some(spec) = &el.plot_source
                && spec.is_lane_plot()
            {
                return Some(spec.lanes.len());
            }
            el.children.iter().find_map(find_lanes)
        }
        let el = view(&State::default());
        assert_eq!(
            find_lanes(&el),
            Some(7),
            "six digital channels + the analog rail"
        );
    }
}
