<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-winit-wgpu

![Showcase — Text & value section, rendered through the native winit + wgpu host](https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/showcase_text_inputs.png)

Optional native desktop host for Damascene apps using `winit` and `wgpu`.

Use this crate when you want the host to own the window, surface,
swapchain, input mapping, IME forwarding, animation redraws, and MSAA
target management:

```rust
use damascene_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
    damascene_winit_wgpu::run("My Damascene App", viewport, MyApp::default())
}
```

For apps with external live state, put per-frame refresh in
`App::before_build`, then use `run_with_config` and pick the redraw
driver that matches your data (see below): a host cadence via
`HostConfig::with_redraw_interval`, or push-driven wakes via
`HostConfig::with_external_wakeup`. For custom render-loop
integration, bypass this crate and call `damascene-wgpu::Runner`
directly.

## Screenshots without a window

Any app or example built on this host is screenshotable with no
per-app code. Set `DAMASCENE_HEADLESS_SHOT` to an output path and the
host renders one settled frame at the `run` viewport size, writes it,
and exits instead of opening a window — PNG when the path ends in
`.png`, binary PPM otherwise. `DAMASCENE_HEADLESS_SCALE` (default `1`)
sets the scale factor, so `2` is a hiDPI render at twice the pixels:

```bash
DAMASCENE_HEADLESS_SHOT=shot.png DAMASCENE_HEADLESS_SCALE=2 cargo run --bin my-app
```

The frame is the same build → layout → paint path the window runs,
through `damascene_wgpu::headless`, so it is ground truth for what the
GPU draws (the SVG from `render_bundle_themed` is layout-accurate but
approximate). Pair it with the lint report from the bundle pipeline
for the agent review loop described in the `damascene-core` README.
Two things a headless shot does not have: hover, focus or animation
state (it is frame zero of an untouched app), and a previous frame's
layout, so `BuildCx::rect_of_key` answers `None` and a layout that
branches on a measured rect shows its unmeasured branch. To shoot a
particular state, give the app a way to start in it (a constructor,
an environment variable it reads in `before_build`).

## Live data: meter-class vs event-class

External live state divides cleanly into two patterns, and apps that
get the choice wrong burn either CPU or responsiveness.

**Meter-class** — high-frequency, value-changes-every-tick: audio
peak meters, FPS counters, network throughput graphs. The right
shape is fixed-cadence polling at the display refresh rate (33 ms ≈
30 fps is plenty for a peak meter; faster wastes work). Use
`HostConfig::with_redraw_interval(Duration)` and snapshot the latest
value in `App::before_build`. `damascene-volume`'s PipeWire peak meter
is the worked example.

**Event-class** — sparse, value-changes-on-discrete-event: a chat
message arrived, a download finished, a USB device was plugged in,
a config file changed on disk. The right shape is push-driven —
the backend thread wakes the UI loop as the event happens, with
no polling. Polling event-class data either burns CPU at a useless
cadence or shows the change up to one polling interval late.

Use `HostConfig::with_external_wakeup` for the push path. The hook
runs once, just before the event loop starts, and hands you a
`Wakeup` — a `Send + Clone` handle any thread can poke to schedule
one frame. Snapshot the changed state in `App::before_build` as
usual; between wakes the idle app renders at 0 fps:

```rust
let (tx, rx) = std::sync::mpsc::channel();
let config = HostConfig::default().with_external_wakeup(move |wakeup| {
    let _ = tx.send(wakeup);
});
// The thread that drains your backend's event subscription decides
// which events warrant a frame, then pokes the host:
std::thread::spawn(move || {
    let wakeup = rx.recv().unwrap();
    // for each interesting backend event:
    wakeup.wake();
});
```

Wakes coalesce — a burst of N pokes before the next frame produces
one redraw — and a poke is safe at any time: from any thread,
before the first frame, or after the loop exits (then it's a
no-op).

If your app needs both — fixed-cadence meters *and* push-driven
events — the two drivers don't conflict; and if the meter is only
sometimes on screen, prefer driving it with `redraw_within` from
the meter widget itself so the cadence stops when the widget
leaves the tree. The trade-off itself is the load-bearing piece —
recognize which axis your data falls on before reaching for the
host config.
