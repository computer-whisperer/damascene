#!/usr/bin/env bash
#
# Type-check the browser-facing crates for wasm32 with atomics — the
# target a threaded web app builds for (Web Workers over one shared
# memory). There a browser GPU object is bound to the thread that
# created it, so wgpu's types are `!Send + !Sync` and the tree follows:
# see `damascene_core::threading` for the contract this holds in place.
#
# No stable toolchain reaches this target: the standard library has to
# be rebuilt with atomics (`-Zbuild-std`), which needs nightly and the
# `rust-src` component.
#
# Usage:
#   tools/check_wasm_atomics.sh             # check with the pinned nightly
#   DAMASCENE_ATOMICS_TOOLCHAIN=nightly tools/check_wasm_atomics.sh
#
# Requires:
#   rustup toolchain install nightly-2026-06-06 --component rust-src \
#       --target wasm32-unknown-unknown

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TOOLCHAIN="${DAMASCENE_ATOMICS_TOOLCHAIN:-nightly-2026-06-06}"

for arg in "$@"; do
    case "$arg" in
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown arg: $arg (try --help)" >&2; exit 2 ;;
    esac
done

cd "$REPO_ROOT"

# A separate target directory: these flags differ from every other
# build's, and sharing `target/` would rebuild the world both ways.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target/wasm-atomics}"
export RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+mutable-globals -D warnings"

echo "==> checking wasm32 + atomics with $TOOLCHAIN"
cargo "+$TOOLCHAIN" check \
    -Zbuild-std=std,panic_abort \
    --target wasm32-unknown-unknown \
    --locked \
    -p damascene-core \
    -p damascene-wgpu \
    -p damascene-web \
    -p damascene-web-showcase \
    -p damascene-markdown \
    -p damascene-workbench

echo "==> wasm32 + atomics: ok"
