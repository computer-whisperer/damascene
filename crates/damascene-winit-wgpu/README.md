<img src="https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/damascene_badge_icon.svg" alt="Damascene badge icon" width="96">

# damascene-winit-wgpu

![Settings section — running through the native winit + wgpu host](https://raw.githubusercontent.com/computer-whisperer/damascene/main/assets/showcase_settings.png)

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
