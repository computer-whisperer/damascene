# Credits and provenance

Damascene is largely written by LLM agents under close human direction,
and we treat provenance as something to state affirmatively rather than
leave for others to discover. This file lists every deliberate borrowing
of design vocabulary, algorithms, and assets, plus the specifications the
from-spec subsystems implement. If you find an uncredited echo of your
work anywhere in this repository, please open an issue — we will either
attribute it properly or rework the code, promptly.

## Design vocabulary (deliberate, by design)

Damascene's thesis is vocabulary parity with what models (and humans)
already know, so these surfaces intentionally mirror existing systems:

- **[shadcn/ui](https://ui.shadcn.com/)** (MIT) — the widget vocabulary
  and anatomy (card/badge/tabs/dialog/item/…) and the default theme's
  color tokens, which copy shadcn's zinc palette.
- **[Radix Colors](https://www.radix-ui.com/colors)** (MIT) — the three
  alternative stock palette pairs (slate/blue, sand/amber, mauve/violet).
- **[WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/)** —
  interaction patterns for tabs, radio groups, and menus.
- **CSS** — layout semantics (flexbox-style fill distribution per
  [CSS Flexbox Level 1 §9.7](https://www.w3.org/TR/css-flexbox-1/#resolve-flexible-lengths)),
  the `ch` unit, `dynamic-range-limit`, color syntax
  ([CSS Color 3/4](https://www.w3.org/TR/css-color-4/)).
- **[W3C UIEvents `key`/`code`](https://www.w3.org/TR/uievents-key/)** —
  the `NamedKey`/`PhysicalKey` vocabularies are the spec's own.
- **[MathML Core](https://www.w3.org/TR/mathml-core/)**, the
  **OpenType MATH table**, and the TeX box-layout tradition — the math
  IR and layout model.
- **[egui](https://github.com/emilk/egui)** (MIT OR Apache-2.0) — the
  spinner's motion constants (max sweep, head/tail easing) mirror egui's
  spinner look; the implementation is an unrelated SDF shader.

## Algorithms and code snippets

- **Oklab** — the conversion functions in
  `crates/damascene-core/src/color/oklab.rs` are a Rust port of
  [Björn Ottosson's reference implementation](https://bottosson.github.io/posts/oklab/)
  (published by the author as public domain / MIT-0).
- **Rounded-box SDF** — the `sdf_rounded_box` function used across the
  stock shaders follows
  [Inigo Quilez's `sdRoundedBox`](https://iquilezles.org/articles/distfunctions2d/)
  (MIT-licensed article snippets).
- **MSDF sampling** — the `median(r,g,b)` + screen-px-range projection in
  `text_msdf.wgsl` follows the
  [msdfgen README](https://github.com/Chlumsky/msdfgen) (Viktor Chlumský,
  MIT). MSDF *generation* is the [`fdsm`](https://crates.io/crates/fdsm)
  crate, a Rust implementation of the same author's method.
- **"Nice numbers" axis ticks** — Paul Heckbert's classic heuristic
  (*Graphics Gems*, 1990) in `plot/scale.rs`.
- **Scene3D line/point shaders and orbit camera** — ported from the
  [volumetric](https://github.com/computer-whisperer/volumetric) CAD
  project's `volumetric_renderer` crate (same author as damascene; the
  `scene_line.wgsl` / `scene_point.wgsl` expansion math and the
  target/distance/yaw/pitch camera pose model).
- **Color science from specification** — SMPTE ST 2084 (PQ),
  ARIB STD-B67 / ITU-R BT.2100 (HLG), ITU-R BT.2390 §5.4.1 (EETF
  roll-off), ITU-R BT.2408 (reference white), and the BT.709 / BT.2020 /
  Adobe RGB primaries matrices. Constants and notation are the specs' own
  and each file cites its source.

## Bundled fonts

Each font crate ships the font's own license text alongside the files:

- **Inter** — SIL Open Font License 1.1
- **JetBrains Mono** — SIL Open Font License 1.1
- **Noto Color Emoji, Noto Sans Symbols 2, Noto Sans Math** — SIL Open
  Font License 1.1
- **Roboto** — Apache License 2.0

## Dependencies

Every crate in the dependency tree is permissively licensed
(MIT/Apache-2.0/BSD/Zlib/ISC/Unicode); there are no copyleft-only
dependencies. Text shaping is [cosmic-text](https://crates.io/crates/cosmic-text),
rasterization [swash](https://crates.io/crates/swash), HTML parsing
[html5ever](https://crates.io/crates/html5ever), markdown
[pulldown-cmark](https://crates.io/crates/pulldown-cmark), syntax
highlighting [syntect](https://crates.io/crates/syntect) + two-face, SVG
[usvg](https://crates.io/crates/usvg), tessellation
[lyon](https://crates.io/crates/lyon_tessellation) — see each crate's
`Cargo.toml` for the full set.
