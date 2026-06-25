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
    Decimation, LegendPosition, PlotSpec, Sample, Scale, SeriesHandle, line, scatter,
};
use damascene_core::prelude::*;

pub struct State {
    /// A smooth periodic signal (a "CPU %").
    cpu: SeriesHandle,
    /// A bounded random walk (a "memory %").
    mem: SeriesHandle,
    /// Sparse event markers, drawn as a scatter.
    events: SeriesHandle,
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

        Self {
            cpu: SeriesHandle::new(cpu),
            mem: SeriesHandle::new(mem),
            events: SeriesHandle::new(events),
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
        // Fills the remaining height of the content panel (plot defaults to
        // Size::Fill on both axes).
        plot(spec).key("showcase-plot"),
    ])
    .gap(tokens::SPACE_3)
    .height(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_plot(el: &El) -> bool {
        el.plot_source.is_some() || el.children.iter().any(has_plot)
    }

    #[test]
    fn view_mounts_the_plot_widget() {
        let el = view(&State::default());
        assert!(
            has_plot(&el),
            "the 2D plot section must mount a plot widget"
        );
    }
}
