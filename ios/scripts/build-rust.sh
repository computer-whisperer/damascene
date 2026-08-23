#!/bin/sh
set -eu

workspace_dir="${SRCROOT}/.."
profile="${DAMASCENE_RUST_PROFILE:-release}"
sdk_name="${SDK_NAME:-iphoneos}"

case "$sdk_name" in
    iphoneos*)
        rust_target="${DAMASCENE_RUST_TARGET:-aarch64-apple-ios}"
        ;;
    iphonesimulator*)
        arch="${CURRENT_ARCH:-${NATIVE_ARCH_ACTUAL:-arm64}}"
        case "$arch" in
            x86_64)
                rust_target="${DAMASCENE_RUST_TARGET:-x86_64-apple-ios}"
                ;;
            *)
                rust_target="${DAMASCENE_RUST_TARGET:-aarch64-apple-ios-sim}"
                ;;
        esac
        ;;
    *)
        echo "Unsupported Apple SDK_NAME: $sdk_name" >&2
        exit 1
        ;;
esac

# Xcode run-script phases start from a bare PATH that does not include a
# rustup install, so `rustup`/`cargo` are invisible here even when they
# work fine in a login shell. Look in the standard locations before
# giving up.
if ! command -v rustup >/dev/null 2>&1; then
    for bin_dir in "${CARGO_HOME:-$HOME/.cargo}/bin" /opt/homebrew/bin /usr/local/bin; do
        if [ -x "$bin_dir/rustup" ]; then
            PATH="$bin_dir:$PATH"
            export PATH
            break
        fi
    done
fi

if ! command -v rustup >/dev/null 2>&1; then
    echo "rustup is not on PATH and was not found in the usual locations." >&2
    echo "Install it from https://rustup.rs, or point the build at an" >&2
    echo "existing install by setting CARGO_HOME." >&2
    exit 1
fi

if ! rustup target list --installed | grep -qx "$rust_target"; then
    echo "Rust target is not installed: $rust_target" >&2
    echo "Install it with: rustup target add $rust_target" >&2
    exit 1
fi

cd "$workspace_dir"

if [ "$profile" = "release" ]; then
    cargo build -p damascene-ios-showcase --lib --release --target "$rust_target"
else
    cargo build -p damascene-ios-showcase --lib --target "$rust_target"
fi
