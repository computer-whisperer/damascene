//! iOS entry point for the Damascene showcase demo.

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn start_winit_app() {
    let viewport = damascene_core::Rect::new(0.0, 0.0, 900.0, 640.0);
    if let Err(err) = damascene_ios::run(
        "Damascene showcase",
        viewport,
        damascene_fixtures::Showcase::new(),
    ) {
        eprintln!("damascene-ios-showcase: {err}");
    }
}

#[cfg(not(target_os = "ios"))]
pub fn ios_showcase_entry_is_ios_only() {}
