# Damascene iOS Showcase

This folder contains the native iOS packaging for
`crates/damascene-ios-showcase`. The Rust crate builds as a `staticlib` and
exports `start_winit_app()`, which the checked-in Xcode app target calls
from `main.m`.

The Rust side is intentionally the same shape as Android:

- `crates/damascene-ios` is the reusable host wrapper.
- `crates/damascene-ios-showcase` is the app-specific entry crate.
- `damascene-winit-wgpu` owns the winit event loop, wgpu surface, device,
  queue, input mapping, and IME visibility.
- The app still owns normal `damascene_core::App` state and rendering
  declarations.

## Build From Xcode

Open the project:

```bash
open ios/DamasceneShowcase.xcodeproj
```

Select the `Damascene Showcase` target and set a signing team if you are
deploying to a physical device. The target has a "Build Rust staticlib"
build phase that runs `ios/scripts/build-rust.sh` before the Objective-C
app links.

The Xcode target currently uses release Rust builds for both Debug and
Release app configurations. That mirrors the Android package because
unoptimized Rust is not useful for this GPU-heavy showcase.

Supported destinations:

- iOS device: `aarch64-apple-ios`
- Apple Silicon simulator: `aarch64-apple-ios-sim`

The project excludes `x86_64` simulator builds for now so the link path
can stay deterministic. Intel simulator support would need either an
`x86_64-apple-ios` slice or an `.xcframework`.

## Build Rust Directly

Install the iOS Rust target that matches the Xcode destination:

```bash
rustup target add aarch64-apple-ios
rustup target add aarch64-apple-ios-sim
```

Build the Rust static library:

```bash
cargo build -p damascene-ios-showcase --release --target aarch64-apple-ios
```

For the simulator on Apple Silicon:

```bash
cargo build -p damascene-ios-showcase --release --target aarch64-apple-ios-sim
```

The Xcode project links the resulting archive:

```text
target/aarch64-apple-ios/release/libdamascene_ios_showcase.a
```

The app links the native libraries reported by `rustc
--print=native-static-libs` for this staticlib, including:

```text
UIKit
Foundation
CoreFoundation
QuartzCore
Metal
libobjc
libiconv
```

Winit's iOS event loop calls `UIApplicationMain` itself, so the app's
Objective-C `main.m` calls `start_winit_app()` directly rather than
calling `UIApplicationMain` first.

## Logs

`damascene-ios` routes Rust `log` output (host GPU-setup diagnostics,
wgpu/winit warnings) to stderr at `Info` level. Where that lands
depends on how the app is launched:

- **Run from Xcode:** the console pane shows stderr, including Rust
  panic messages.
- **Terminal, no Xcode GUI:**
  `xcrun simctl launch --console-pty booted <bundle-id>` attaches the
  app's stdout/stderr to your terminal.
- **Tapping the app icon:** stderr is discarded; there is no unified-
  logging (`os_log`) integration yet, so detached launches are silent.

## Current Limitations

This is compile and packaging groundwork. The Linux development
environment cannot link or run iOS binaries because it does not have
Xcode's SDKs. Before calling iOS support complete, verify on Apple
hardware or simulator:

- first frame presents through Metal,
- touch down/move/up route to widgets,
- text input shows the soft keyboard and commits text,
- safe area and rotation sizes are sane,
- suspend/resume does not present to a stale surface,
- link opening and clipboard behavior are acceptable.
