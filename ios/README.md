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

## Run On The Simulator

The whole loop works from a terminal without opening Xcode. From the
workspace root:

```bash
xcrun simctl boot "iPhone 17 Pro"
open -a Simulator

xcodebuild -project ios/DamasceneShowcase.xcodeproj \
    -scheme "Damascene Showcase" \
    -sdk iphonesimulator \
    -configuration Debug \
    -derivedDataPath ios/DerivedData \
    CODE_SIGNING_ALLOWED=NO \
    build

xcrun simctl install booted \
    "ios/DerivedData/Build/Products/Debug-iphonesimulator/Damascene Showcase.app"
xcrun simctl launch --console-pty booted com.cjbal.damascene.showcase
```

`xcodebuild` runs the "Build Rust staticlib" phase, so the `cargo build`
above is not a separate step. `ios/DerivedData/` is gitignored.
`xcrun simctl io booted screenshot shot.png` captures the running app.

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

## Status

Verified on the Apple Silicon simulator in August 2026 (Xcode 26.6,
iOS 26.5, iPhone 17 Pro), using the run sequence above: the showcase
builds through the Xcode project, installs, presents through Metal at
the right scale, and honors the safe area; touch, drags, animation,
rotation, text input through the soft keyboard, the 2D and 3D plots,
and shader animations all work. A physical iPhone has confirmed
basic functionality (`aarch64-apple-ios`, signed build). The Linux CI
job only cross-checks and builds the staticlib (no Apple SDK there),
so anything beyond that needs a Mac to exercise.

Not yet exercised:

- suspend/resume not presenting to a stale surface.

Known gaps — not wired on iOS yet (both sit behind desktop-only
dependencies of `damascene-winit-wgpu`):

- clipboard: copy/cut/paste inside the app,
- link opening: `App::drain_link_opens`.
