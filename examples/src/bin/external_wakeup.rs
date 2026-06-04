//! External wakeup — push-driven redraw for event-class data.
//!
//! Demonstrates [`HostConfig::with_external_wakeup`]: a background
//! thread produces sparse "backend events" (here, one per second) and
//! pokes the host's [`Wakeup`] handle after each. The host builds one
//! frame per poke; between events the app sits fully idle at 0 fps —
//! no `redraw_interval` polling.
//!
//! The status line shows what triggered the current frame
//! (`cx.diagnostics().trigger`) and the host's frame counter: park the
//! mouse outside the window and the frame index advances only when an
//! event lands, by exactly one. The same line is printed to the
//! terminal per frame, so the 0 fps idle is visible there too —
//! between backend events, no lines.
//!
//! Run: `cargo run -p damascene-examples --bin external_wakeup`

use std::sync::{Arc, Mutex};
use std::time::Duration;

use damascene_core::prelude::*;
use damascene_winit_wgpu::HostConfig;

/// State shared between the UI thread and the fake backend thread.
/// `before_build` snapshots it once per frame.
#[derive(Default)]
struct Backend {
    events: Vec<String>,
}

struct WakeupDemo {
    backend: Arc<Mutex<Backend>>,
    /// Per-frame snapshot taken in `before_build`.
    events: Vec<String>,
}

impl App for WakeupDemo {
    fn before_build(&mut self) {
        self.events = self.backend.lock().unwrap().events.clone();
    }

    fn build(&self, cx: &BuildCx) -> El {
        let status = cx
            .diagnostics()
            .map(|d| format!("frame {} · trigger: {}", d.frame_index, d.trigger.label()))
            .unwrap_or_default();
        // One terminal line per frame built — silence between backend
        // events is the demo: the idle app renders at 0 fps.
        eprintln!("{status}");
        column([
            h1(format!("{} events", self.events.len())),
            column(
                self.events
                    .iter()
                    .rev()
                    .take(8)
                    .map(|e| text(e).muted())
                    .collect::<Vec<_>>(),
            )
            .gap(tokens::SPACE_1),
            text(status).label().muted(),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_7)
        .align(Align::Center)
        .justify(Justify::Center)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = Arc::new(Mutex::new(Backend::default()));

    // Hand the wakeup handle to the backend thread through a channel:
    // the hook runs on the UI thread just before the event loop starts.
    let (tx, rx) = std::sync::mpsc::channel();
    let config = HostConfig::default().with_external_wakeup(move |wakeup| {
        let _ = tx.send(wakeup);
    });

    let thread_backend = backend.clone();
    std::thread::spawn(move || {
        let wakeup = rx.recv().expect("host hands out the wakeup handle");
        for n in 1.. {
            std::thread::sleep(Duration::from_secs(1));
            thread_backend
                .lock()
                .unwrap()
                .events
                .push(format!("backend event #{n}"));
            // One poke per event; bursts before the next frame would
            // coalesce into a single redraw.
            wakeup.wake();
        }
    });

    let viewport = Rect::new(0.0, 0.0, 480.0, 360.0);
    damascene_winit_wgpu::run_with_config(
        "Damascene — external wakeup",
        viewport,
        WakeupDemo {
            backend,
            events: Vec::new(),
        },
        config,
    )
}
