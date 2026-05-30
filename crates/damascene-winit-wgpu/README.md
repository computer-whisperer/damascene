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
`App::before_build`, then use `run_with_config` and
`HostConfig::with_redraw_interval` to choose a host cadence. For custom
render-loop integration, bypass this crate and call `damascene-wgpu::Runner`
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

The push-wake hook isn't wired through `HostConfig` yet; bypass
this crate's `run`/`run_with_config` and use winit's
`EventLoopProxy::send_event` directly when you need it (a worked
recipe lives at `damascene-wgpu::Runner` — drive a custom event loop
that calls `Runner::prepare` / `Runner::render` and wakes on
`UserEvent`). If your app needs both — fixed-cadence meters *and*
push-driven events — combine `with_redraw_interval` for the meter
clock with a proxy-driven `request_redraw` for the event channel;
the two don't conflict.

A future minor release will fold this into `HostConfig` (likely
`with_external_wakeup(Fn(Wakeup))`) once a non-meter use case
inside the workspace pressure-tests the shape. The trade-off
itself is the load-bearing piece — recognize which axis your data
falls on before reaching for the host config.
