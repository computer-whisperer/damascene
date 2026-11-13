#!/usr/bin/env bash
#
# Publish the workspace's crates to crates.io, in dependency order
# (docs/RELEASING.md, step 3). The version is read from damascene-core.
#
# Safe to re-run: a crate already on crates.io at this version is
# skipped, so a run that stopped part-way (a stale token fails the
# first crate with a 403) picks up where it left off.
#
# It refuses to run from a tree with uncommitted changes, or from a
# commit that is not what `origin/main` points at: the published
# crates should be the commit CI ran on.
#
# Usage:
#   tools/publish_crates.sh            # publish what is not there yet
#   tools/publish_crates.sh --check    # only report what is published

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

CHECK=0
for arg in "$@"; do
    case "$arg" in
        --check) CHECK=1 ;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown arg: $arg (try --help)" >&2; exit 2 ;;
    esac
done

# Dependency order. The damascene-fonts-* asset crates are published
# on their own, only when a font changes.
CRATES=(
    damascene-fonts
    damascene-core
    damascene-html
    damascene-markdown
    damascene-wgpu
    damascene-vulkano
    damascene-ash
    damascene-winit
    damascene-winit-wgpu
    damascene-web
    damascene-android
    damascene-ios
)

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/damascene-core/Cargo.toml | head -1)
if [[ -z "$VERSION" ]]; then
    echo "could not read the version from crates/damascene-core/Cargo.toml" >&2
    exit 1
fi

published() {
    local code
    code=$(curl -s -o /dev/null -w '%{http_code}' -A release-check \
        "https://crates.io/api/v1/crates/$1/$VERSION")
    [[ "$code" == 200 ]]
}

if [[ "$CHECK" == 1 ]]; then
    for c in "${CRATES[@]}"; do
        if published "$c"; then echo "$c $VERSION: published"; else echo "$c $VERSION: not published"; fi
    done
    exit 0
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "The working tree has uncommitted changes — not publishing." >&2
    exit 1
fi
git fetch -q origin main
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
    echo "HEAD is not origin/main — push the release commit (and let CI run) first." >&2
    exit 1
fi

for c in "${CRATES[@]}"; do
    if published "$c"; then
        echo "== $c $VERSION is already on crates.io, skipping"
        continue
    fi
    echo "== publishing $c $VERSION"
    cargo publish -p "$c"
done

echo
echo "All ${#CRATES[@]} crates are on crates.io at $VERSION."
