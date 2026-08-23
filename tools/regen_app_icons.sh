#!/usr/bin/env bash
# Regenerate the Android and iOS app-icon rasters from the badge SVG.
#
# Everything is derived from assets/damascene_badge_icon.svg — re-run this
# after any change to the badge. Requires `resvg` and python3 with Pillow.
#
#   Android (adaptive icon, minSdk 26): the glyph group alone — outline,
#   gold fill, damascus pattern, bowl counter — scaled by 0.82 about the
#   badge centre so its extremes sit inside the 66dp safe circle
#   (66/108 x 512 / 2 = 156.4 units; the stroked glyph's farthest point,
#   the left corners, is 186 units out). The steel background and the
#   Android 13 monochrome layer are checked-in XML drawables, not
#   generated here.
#
#   iOS (single-size asset): the badge cropped to its 448x448 rect over a
#   square steel underlay, so the rounded corners are opaque — iOS masks
#   with its own squircle, whose corner ratio the badge's rx was designed
#   to match. Alpha is stripped: App Store validation rejects RGBA icons.
set -euo pipefail
cd "$(dirname "$0")/.."

SVG=assets/damascene_badge_icon.svg
RES=android/app/src/main/res
APPICON=ios/DamasceneShowcase/Assets.xcassets/AppIcon.appiconset
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Android foreground: defs + the translate(-26 0) glyph group, safe-zone scaled.
{
  echo '<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">'
  sed -n '/<defs>/,/<\/defs>/p' "$SVG"
  echo '<g transform="translate(256 256) scale(0.82) translate(-256 -256)">'
  sed -n '/<g transform="translate(-26 0)">/,/^  <\/g>$/p' "$SVG"
  echo '</g></svg>'
} > "$TMP/foreground.svg"

declare -A DENSITY=([mdpi]=108 [hdpi]=162 [xhdpi]=216 [xxhdpi]=324 [xxxhdpi]=432)
for d in "${!DENSITY[@]}"; do
  mkdir -p "$RES/mipmap-$d"
  resvg -w "${DENSITY[$d]}" -h "${DENSITY[$d]}" "$TMP/foreground.svg" \
    "$RES/mipmap-$d/ic_launcher_foreground.png"
done

# iOS: crop the viewBox to the badge rect, underlay an un-rounded steel square.
sed -e 's|width="512" height="512" viewBox="0 0 512 512"|width="1024" height="1024" viewBox="32 32 448 448"|' \
    -e 's|<rect x="32" y="32" width="448" height="448" rx="100" fill="url(#steel)"/>|<rect x="32" y="32" width="448" height="448" fill="url(#steel)"/>\n  <rect x="32" y="32" width="448" height="448" rx="100" fill="url(#steel)"/>|' \
    "$SVG" > "$TMP/ios.svg"
resvg -w 1024 -h 1024 "$TMP/ios.svg" "$TMP/ios_1024.png"
python3 - "$TMP/ios_1024.png" "$APPICON/AppIcon.png" <<'EOF'
import sys
from PIL import Image
Image.open(sys.argv[1]).convert("RGB").save(sys.argv[2])
EOF

echo "regenerated: $RES/mipmap-*/ic_launcher_foreground.png, $APPICON/AppIcon.png"
