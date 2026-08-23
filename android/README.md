# Damascene on Android

`damascene-android` runs a normal `damascene_core::App` inside
Android's `NativeActivity`. The split matches the desktop and iOS
hosts — the host (through `damascene-winit-wgpu`) owns the event loop,
wgpu surface, device/queue, and input translation; the app owns state,
`build`, and `on_event` — so one `App` implementation covers all of
them.

A downstream app is a `cdylib` that exports `android_main`:

```rust
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: damascene_android::AndroidApp) {
    let viewport = damascene_core::Rect::new(0.0, 0.0, 900.0, 640.0);
    if let Err(err) = damascene_android::run(app, "My app", viewport, MyApp::default()) {
        // stderr goes nowhere on Android; the host routed `log` to logcat.
        log::error!("my-app: {err}");
    }
}
```

What the host wires up:

- **Accessibility.** The AccessKit delegate is injected into
  `NativeActivity`'s content view — no Gradle or Java changes; TalkBack
  roles, scroll actions, and scroll-into-view included, verified with
  TalkBack on a device (August 2026). System accessibility settings are
  sniffed at resume and surface as `AccessibilityPreferences` (e.g.
  animations off → reduced motion). Needs `minSdk 26` for the
  embedded-dex loader.
- **Input.** Touch (multi-contact registry, two-finger pinch,
  long-press), the soft keyboard — shown and hidden from focus state,
  text arriving through the platform IME — and physical keyboards.
- **Safe area.** `content_rect` becomes safe-area insets — display
  cutouts, system bars, and the soft-keyboard band — which placement
  uses to keep popovers and menus on screen.
- **Logging.** The `log` facade routes to logcat (tag `damascene`)
  before the loop starts; GPU-setup failures on odd devices would
  otherwise die invisibly.
- **GPU.** wgpu over Vulkan where the device has it, GLES 3.1 otherwise
  (the reference manifest's floor); surfaces without `COPY_SRC` degrade
  (backdrop sampling off, with a log line) instead of panicking.

Font features pass through to `damascene-core`, so an APK can drop the
~10.7 MB color-emoji bundle with `default-features = false` plus the
faces it wants.

## The showcase APK (reference packaging)

The repository's `android/` Gradle project packages
`crates/damascene-android-showcase` as a `NativeActivity` APK. The
Activity loads `libmain.so`; the Rust `android_main` entry point starts
the Damascene showcase through the native wgpu host.

Build and install the debug APK:

```bash
cd android
gradle :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

The APK is debuggable, but the Rust `libmain.so` is built with
`cargo build --release`; unoptimized Rust makes first render far too
slow for this GPU-heavy showcase. The build currently targets
`arm64-v8a` only.
