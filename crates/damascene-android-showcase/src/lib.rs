//! Android entry point for the Damascene showcase demo.

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: damascene_android::AndroidApp) {
    let viewport = damascene_core::Rect::new(0.0, 0.0, 900.0, 640.0);
    if let Err(err) = damascene_android::run(
        app,
        "Damascene showcase",
        viewport,
        damascene_fixtures::Showcase::new(),
    ) {
        eprintln!("damascene-android-showcase: {err}");
    }
}

#[cfg(not(target_os = "android"))]
pub fn android_showcase_entry_is_android_only() {}
