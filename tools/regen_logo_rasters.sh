#!/usr/bin/env bash
# Regenerate the raster embeds of assets/damascene_badge_icon.svg used
# by damascene-fixtures (hero sidebar at 32 logical px, showcase About
# at 64). Raster rather than SvgIcon::parse because the vector icon
# pipeline ignores SVG clip-path (issue #150) and the badge clips its
# wave inlay to the D with one; rsvg respects clips. The two sizes
# cover the fixture slots at up to 3x DPI without mip-less
# minification artifacts. Run from anywhere; requires rsvg-convert
# and python3-PIL.
set -euo pipefail
cd "$(dirname "$0")/.."
for px in 96 192; do
    tmp=$(mktemp --suffix .png)
    rsvg-convert -w "$px" -h "$px" assets/damascene_badge_icon.svg -o "$tmp"
    python3 - "$tmp" "$px" <<'PYEOF'
import sys
from PIL import Image
tmp, px = sys.argv[1], sys.argv[2]
im = Image.open(tmp).convert("RGBA")
assert im.size == (int(px), int(px))
with open(f"crates/damascene-fixtures/assets/badge_icon_{px}.rgba", "wb") as f:
    f.write(im.tobytes())
PYEOF
    rm "$tmp"
    echo "wrote crates/damascene-fixtures/assets/badge_icon_${px}.rgba"
done
