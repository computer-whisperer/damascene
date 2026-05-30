//! Native Android host shell for Damascene wgpu apps.
//!
//! Android does not let a Rust app create a winit event loop from
//! process-global state. The Activity glue calls the app's exported
//! `android_main(AndroidApp)`, and the host must attach that value to
//! the event-loop builder before Damascene takes over.
//!
//! Downstream Android `cdylib` crates should export `android_main` and
//! call [`run`] or [`run_with_config`] with their normal
//! `damascene_core::App`.

use damascene_core::{App, Rect};
pub use damascene_winit_wgpu::HostConfig;

#[cfg(target_os = "android")]
pub use winit::platform::android::activity::AndroidApp;

/// Run an Damascene app in Android's `NativeActivity` surface.
///
/// This is the Android equivalent of `damascene_winit_wgpu::run`: app code
/// owns state/build/events; the host owns the platform event loop,
/// surface, device/queue, and input translation.
#[cfg(target_os = "android")]
pub fn run<A: App + 'static>(
    android_app: AndroidApp,
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_config(android_app, title, viewport, app, HostConfig::default())
}

/// Run an Damascene app in Android's `NativeActivity` surface with explicit
/// host configuration.
#[cfg(target_os = "android")]
pub fn run_with_config<A: App + 'static>(
    android_app: AndroidApp,
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    let mut builder = winit::event_loop::EventLoop::builder();
    builder.with_android_app(android_app);
    let event_loop = builder.build()?;
    damascene_winit_wgpu::run_on_event_loop(event_loop, title, viewport, app, config)
}

/// Non-Android builds can type-check crates that depend on
/// `damascene-android`, but cannot start an Android Activity.
#[cfg(not(target_os = "android"))]
pub fn run<A: App + 'static>(
    _android_app: (),
    _title: &'static str,
    _viewport: Rect,
    _app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("damascene-android can only run on target_os = \"android\"".into())
}

/// Non-Android builds can type-check crates that depend on
/// `damascene-android`, but cannot start an Android Activity.
#[cfg(not(target_os = "android"))]
pub fn run_with_config<A: App + 'static>(
    _android_app: (),
    _title: &'static str,
    _viewport: Rect,
    _app: A,
    _config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("damascene-android can only run on target_os = \"android\"".into())
}
