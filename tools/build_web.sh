#!/usr/bin/env bash
#
# Build the damascene-web showcase wasm bundle and (optionally) serve it locally.
#
# Output lands in crates/damascene-web-showcase/pkg/ — sibling to
# crates/damascene-web-showcase/index.html, which imports
# `./pkg/damascene_web_showcase.js` (relative, so the same file works whether
# served from the crate root locally or under a GitHub Pages subpath).
#
# Usage:
#   tools/build_web.sh             # release build (default — dev wasm is too
#                                  # slow under our prepare/text path)
#   tools/build_web.sh --serve     # build + serve at http://127.0.0.1:8083/
#   tools/build_web.sh --dev       # unoptimized build (faster compile, slower run)
#
# Requires: wasm-pack (cargo install wasm-pack), and python3 for --serve.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SERVE=0
PROFILE=release
for arg in "$@"; do
    case "$arg" in
        --serve)   SERVE=1 ;;
        --release) PROFILE=release ;;
        --dev)     PROFILE=dev ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown arg: $arg (try --help)" >&2; exit 2 ;;
    esac
done

if [[ "$PROFILE" == "release" ]]; then
    PROFILE_FLAG="--release"
else
    PROFILE_FLAG="--dev"
fi

echo "==> building damascene-web-showcase (wasm, $PROFILE)"
cd "$REPO_ROOT"
# `--target web` makes pkg/damascene_web_showcase.js expose `default` (init), which
# returns a Promise. The index.html harness imports and calls it.
wasm-pack build crates/damascene-web-showcase --target web "$PROFILE_FLAG"

echo
echo "==> wasm bundle written to crates/damascene-web-showcase/pkg/"

if [[ "$SERVE" -eq 1 ]]; then
    echo "==> serving crates/damascene-web-showcase/ on http://127.0.0.1:8083/"
    echo "    open http://127.0.0.1:8083/index.html"
    cd "$REPO_ROOT/crates/damascene-web-showcase"
    exec python3 -m http.server 8083
fi
