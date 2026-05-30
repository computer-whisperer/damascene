#!/usr/bin/env bash
# Render every SVG fixture under crates/*/out/ to PNG for visual review.
#
# Picks resvg (pure Rust) if available, falls back to rsvg-convert. Skips
# regeneration when the PNG is already newer than its SVG. Prints each
# produced path on stdout.

set -uo pipefail

cd "$(dirname "$0")/.."

export FONTCONFIG_FILE="$PWD/tools/damascene-fontconfig.conf"

picker() {
    # Prefer rsvg-convert: more lenient with the SVG dialect we emit.
    # Fall back to resvg.
    if command -v rsvg-convert >/dev/null 2>&1; then echo "rsvg-convert"; return; fi
    if command -v resvg >/dev/null 2>&1; then echo "resvg"; return; fi
    echo ""
}

renderer="$(picker)"
if [ -z "$renderer" ]; then
    echo "error: install resvg or rsvg-convert (librsvg) to render SVG fixtures." >&2
    exit 1
fi

shopt -s nullglob
any=0
for svg in crates/*/out/*.svg; do
    any=1
    png="${svg%.svg}.png"
    if [ -f "$png" ] && [ "$png" -nt "$svg" ]; then
        continue
    fi
    case "$renderer" in
        resvg)         resvg --zoom 2 "$svg" "$png" || { echo "  failed: $svg" >&2; continue; } ;;
        rsvg-convert)  rsvg-convert --zoom 2 "$svg" -o "$png" || { echo "  failed: $svg" >&2; continue; } ;;
    esac
    echo "$png"
done

if [ "$any" = 0 ]; then
    echo "no SVG fixtures found under crates/*/out/" >&2
fi
