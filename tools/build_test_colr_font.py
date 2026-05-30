#!/usr/bin/env python3
"""Generate a tiny COLRv0 / CPAL test font for damascene-core's COLR test.

The font defines one user-facing glyph at U+E001 ("icon") composed of
two COLR layers — a red square and a blue diamond — drawing from a
two-entry CPAL palette. The test in damascene-core verifies that swash's
ColorOutline source rasterizes both layers and the unified-RGBA atlas
captures both palette colors.

Output: crates/damascene-core/tests/fixtures/test_colr.ttf

The TTF is committed to the repo so the test runs without having to
fetch a third-party COLR font. Re-run this script if you ever need to
regenerate the fixture.
"""

import sys
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def build():
    fb = FontBuilder(unitsPerEm=1000, isTTF=True)
    fb.setupGlyphOrder([".notdef", "square", "diamond", "icon"])
    fb.setupCharacterMap({0xE001: "icon"})

    # Square (~ palette index 0, red).
    pen = TTGlyphPen(None)
    pen.moveTo((100, 100))
    pen.lineTo((900, 100))
    pen.lineTo((900, 900))
    pen.lineTo((100, 900))
    pen.closePath()
    square_glyph = pen.glyph()

    # Diamond (~ palette index 1, blue).
    pen = TTGlyphPen(None)
    pen.moveTo((500, 100))
    pen.lineTo((900, 500))
    pen.lineTo((500, 900))
    pen.lineTo((100, 500))
    pen.closePath()
    diamond_glyph = pen.glyph()

    empty = TTGlyphPen(None).glyph()

    fb.setupGlyf({
        ".notdef": empty,
        "square": square_glyph,
        "diamond": diamond_glyph,
        "icon": empty,
    })
    fb.setupHorizontalMetrics({
        ".notdef": (1000, 0),
        "square": (1000, 0),
        "diamond": (1000, 0),
        "icon": (1000, 0),
    })
    fb.setupHorizontalHeader(ascent=1000, descent=0)
    fb.setupOS2(sTypoAscender=1000, sTypoDescender=0, usWinAscent=1000, usWinDescent=0)
    fb.setupNameTable({
        "familyName": "DamasceneColrTest",
        "styleName": "Regular",
        "uniqueFontIdentifier": "DamasceneColrTest-Regular",
        "fullName": "DamasceneColrTest Regular",
        "psName": "DamasceneColrTest-Regular",
        "version": "1.0",
    })
    fb.setupPost()

    # COLRv0: each user glyph maps to an ordered list of (layer_glyph_name, palette_index).
    fb.setupCOLR({
        "icon": [
            ("square", 0),   # bottom layer: red
            ("diamond", 1),  # top layer: blue
        ],
    })
    # CPAL: a single palette with two colors (R, G, B, A) in 0..1.
    fb.setupCPAL([[
        (1.0, 0.0, 0.0, 1.0),  # red
        (0.0, 0.0, 1.0, 1.0),  # blue
    ]])

    out = Path(__file__).resolve().parent.parent / "crates/damascene-core/tests/fixtures/test_colr.ttf"
    out.parent.mkdir(parents=True, exist_ok=True)
    fb.font.save(str(out))
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    sys.exit(build() or 0)
