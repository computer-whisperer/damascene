//! Native iOS host shell for Damascene wgpu apps.
//!
//! Verified on the iOS simulator (August 2026): the showcase builds
//! through the Xcode project, renders full-screen at the right scale
//! with safe-area insets honored, and touch, drags, animation,
//! rotation, soft-keyboard text input, plots, and shader animations
//! all work; a physical iPhone confirmed basic functionality. Known
//! gaps: clipboard and link opening are not yet wired on iOS.
//! `ios/README.md` has the run sequence and the full status list.
//!
//! iOS apps are packaged by Xcode. Downstream crates usually build a
//! Rust `staticlib` with an exported C ABI function, then call that
//! function from the app's Objective-C or Swift entry point. This crate
//! keeps the Rust side aligned with the desktop and Android host APIs:
//! application code owns `App`, while `damascene-winit-wgpu` owns the
//! window, event loop, surface, device/queue, and input translation.

use damascene_core::{App, Rect};
pub use damascene_winit_wgpu::HostConfig;

/// Minimal `log` backend writing to stderr.
///
/// On iOS, stderr is captured by Xcode's console (debugger attached)
/// and by `xcrun simctl launch --console-pty`; a detached launch
/// (tapping the app icon) discards it. Unified-logging (`os_log`)
/// integration would cover detached launches too, but the existing
/// binding crates compile a C shim against Apple SDK headers, which
/// breaks this crate's Linux CI cross-check — revisit if detached
/// logging becomes necessary.
#[cfg(target_os = "ios")]
struct StderrLogger;

#[cfg(target_os = "ios")]
impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "damascene [{} {}] {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Run an Damascene app in iOS's UIKit/winit event loop.
///
/// This is the iOS equivalent of `damascene_winit_wgpu::run`: app code
/// owns state/build/events; the host owns the platform event loop,
/// surface, device/queue, and input translation.
#[cfg(target_os = "ios")]
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_config(title, viewport, app, HostConfig::default())
}

/// Run an Damascene app on iOS with explicit host configuration.
///
/// Routes the `log` facade to stderr at `Info` level before starting —
/// the host reports GPU-setup failures (e.g. surface configuration
/// errors behind a black first frame) through `log`, and without a
/// backend those reports die invisibly. Apps that want their own
/// logger can install it before calling this; an already-installed
/// logger wins.
#[cfg(target_os = "ios")]
pub fn run_with_config<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    static LOGGER: StderrLogger = StderrLogger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }

    damascene_winit_wgpu::run_with_config(title, viewport, app, config)
}

/// Non-iOS builds can type-check crates that depend on `damascene-ios`, but
/// cannot start a UIKit application.
#[cfg(not(target_os = "ios"))]
pub fn run<A: App + 'static>(
    _title: &'static str,
    _viewport: Rect,
    _app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("damascene-ios can only run on target_os = \"ios\"".into())
}

/// Non-iOS builds can type-check crates that depend on `damascene-ios`, but
/// cannot start a UIKit application.
#[cfg(not(target_os = "ios"))]
pub fn run_with_config<A: App + 'static>(
    _title: &'static str,
    _viewport: Rect,
    _app: A,
    _config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("damascene-ios can only run on target_os = \"ios\"".into())
}
