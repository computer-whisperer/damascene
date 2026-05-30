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
