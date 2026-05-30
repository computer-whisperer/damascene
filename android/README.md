# Damascene Android Showcase

This Gradle project packages `crates/damascene-android-showcase` as a
`NativeActivity` APK. The Activity loads `libmain.so`; the Rust
`android_main` entry point starts the Damascene showcase through the native
wgpu host.

Build and install the debug APK:

```bash
cd android
gradle :app:assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

The APK is debuggable, but the Rust `libmain.so` is built with
`cargo build --release`; unoptimized Rust makes first render far too slow
for this GPU-heavy showcase.

The build currently targets `arm64-v8a` only.
