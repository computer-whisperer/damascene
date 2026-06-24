//! Plot — a 2D time-series plot inside an ordinary Damascene app.
//!
//! Demonstrates the `plot` widget: two line series and a scatter over a
//! shared time axis, with auto-scaled axes, gridlines, tick labels, and a
//! crosshair — composited through the core render pipeline with zero host
//! glue (the data layer reuses the Scene3D pipelines under an orthographic
//! camera; see `docs/PLOT2D_PLAN.md`).
//!
//! The series are app-owned [`SeriesHandle`]s built once and merely
//! *referenced* each frame. Drag to pan, wheel to zoom (per-axis); the Y
//! axis auto-scales to the visible window.
//!
//! Run: `cargo run -p damascene-examples --bin plot`

use damascene_core::plot::{PlotSpec, Sample, Scale, SeriesHandle, line, scatter};
use damascene_core::prelude::*;

struct PlotDemo {
    /// A smooth periodic signal (a "CPU %").
    cpu: SeriesHandle,
    /// A bounded random walk (a "memory %").
    mem: SeriesHandle,
    /// Sparse event markers, drawn as a scatter.
    events: SeriesHandle,
}

impl Default for PlotDemo {
    fn default() -> Self {
        // A fixed one-hour window of epoch-seconds samples, so the run is
        // deterministic (no wall-clock dependency).
        let base = 1_780_000_000.0_f64;
        let n = 600usize;
        let dt = 3600.0 / n as f64;

        let mut cpu = Vec::with_capacity(n);
        let mut mem = Vec::with_capacity(n);
        let mut events = Vec::new();

        // Tiny xorshift PRNG for the random walk — no rand dependency.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut rand01 = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
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

impl App for PlotDemo {
    fn build(&self, _cx: &BuildCx) -> El {
        let spec = PlotSpec::new()
            .x(Scale::time())
            .y(Scale::linear())
            .add_mark(line(&self.cpu).width(2.0))
            .add_mark(line(&self.mem).width(2.0))
            .add_mark(scatter(&self.events).size(6.0))
            .crosshair(true);

        column([
            row([
                h2("Plot"),
                spacer(),
                text("synthetic time series · drag to pan · wheel to zoom").muted(),
            ])
            .align(Align::Center),
            // The plot fills the remaining space.
            plot(spec).key("metrics"),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 560.0);
    damascene_winit_wgpu::run("Damascene — Plot", viewport, PlotDemo::default())
}
