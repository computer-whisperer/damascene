# Plot2D: Backend-Neutral 2D Plots & Graphs

> **Status (2026-06-24): M1 shipped on wgpu.** The line+scatter vertical slice
> renders end to end: `plot(PlotSpec)` → orthographic `DrawOp::Scene3D` data
> layer (reusing the scene line/point pipelines) + themed gridline/tick chrome,
> with scientific-tool navigation (directional box-zoom, double-click reset,
> Shift-drag pan, cursor-anchored wheel — see Interaction), Y-autoscale, a
> a multi-series cursor readout (vertical rule, per-series coloured dots, a
> stacked `x`-header + `swatch · label · value` chip), an app-positioned legend
> (`PlotControls`-style `.legend(LegendPosition::…)`), per-mark `.label(…)`, and
> opt-in `MinMax` decimation. Backend-free core is unit-tested (63 tests); a
> headless
> wgpu render test proves the data layer composites. Remaining for M-later:
> vulkano/ash are free (same op) but unverified; the upload-once geometry memo
> (geometry is re-lowered per frame today); live-append demo; log/band scales,
> area/bar marks, interactive legend (click-to-toggle series), rotated Y-axis
> title, adaptive gutters. The first
> consumer is a TSDB viewer; the deliverable is a *general* 2D plot widget.
> Where this doc speaks in the future tense for shipped pieces, the code
> supersedes it.
>
> Sibling design record: `docs/SCENE3D_PLAN.md`. Plot2D deliberately reuses
> Scene3D's shipped machinery (geometry handles, the line/point GPU pipelines,
> the offscreen-MSAA→resolve→composite path, scene-anchored labels). Read that
> doc first — much of the "how a backend-neutral content op works" reasoning is
> there and is not repeated here.

## Goal

Add a closed-scope, highly-configurable **2D plot widget** threaded through
Damascene's backend-neutral draw-op layer — not a host-composed `surface()` and
not a pile of per-segment `Quad` ops. An author writes:

```rust
plot([
    line(&self.cpu).color(palette.accent),
    line(&self.mem),
    scatter(&self.events),
])
.x(Scale::time())          // time axis (the TSDB common case)
.y(Scale::linear())        // auto-domain from visible data
.crosshair()               // cursor readout of the nearest sample
.key("metrics")
```

…with **zero host glue**, rendering identically on wgpu / vulkano / ash,
compositing / clipping / z-ordering / theming like any other widget, with crisp
themed axes, ticks, gridlines, legend, and crosshair — and **performant under
high sample counts** (the load-bearing requirement: a TSDB throws a lot of
points at it).

Target use cases: time-series line charts (the first client), scatter plots,
area/stacked charts, bar charts — "apps that want to show 2D data in a polished,
interactive, pannable/zoomable manner," not a full custom-pipeline renderer.

## Why this shape (settled decisions — do not re-litigate)

Worked out in design discussion (2026-06-24). Premises of the plan, not open
questions.

1. **Reuse the shipped Scene3D line/point GPU pipelines under an orthographic,
   Z=0 camera — do not add a parallel set of 2D pipelines, and do not lower data
   marks to the CPU 2D paint stream.** A 2D plot's data marks are a degenerate 3D
   scene (orthographic projection, all geometry at z=0). The scene pipelines
   already give us, on all three backends, exactly the properties a quality plot
   needs and that the CPU paint stream cannot deliver at scale:
   - screen-pixel-width anti-aliased lines and points (`size_mode: ScreenPx`),
   - offscreen MSAA → resolve → composite (clip/scissor/z-order for free),
   - color-managed linear-working-space blending (HDR-ready, see Scene3D's
     color section),
   - versioned `GeometryHandle` upload-once / re-upload-on-`rev` caching.

   The CPU paint stream would emit thousands of `Quad`/`Vector` ops per series
   and re-tessellate every frame — a non-starter at TSDB scale. (Note: this
   supersedes the `SCENE3D_PLAN.md` non-goal line that guessed 2D would be "the
   2D paint stream + one shader." That predated the shipped scene pipelines;
   reusing them is strictly cheaper and higher-quality.)

   **The closed-scope boundary is identical to Scene3D's:** the moment an app
   wants to own the device, the vertex layout, or its own passes, it has left
   `plot()` and is in host-composed `surface()` land.

2. **The view transform is a uniform; data lives in the handle and is not
   re-uploaded on pan/zoom.** This is what makes high-sample-count panning
   smooth. The geometry handle holds samples in **scale-space** (see decision 5),
   uploaded once. The current visible window (`PlotView`, decision 4) is an
   affine window in scale-space, folded into the orthographic camera's MVP — a
   uniform. So **pan/zoom updates a uniform, never the vertex buffer.** A buffer
   re-upload happens only when the data changes (`rev` bump) or the scale *type*
   changes (rare). This is the 2D analogue of Scene3D's "animation that only
   moves a model is a per-frame transform, `rev` never advances."

3. **Three-layer API — the barrier-vs-ceiling answer.** The central design ask is
   "low barrier for a simple plot, high ceiling for power users building custom
   systems." Resolved as three clean cut-points, mirroring how stock widgets are
   reference compositions over public primitives (the `widget_kit.md` symmetry
   invariant):

   - **Layer 1 — `plot([...marks])` + `PlotSpec`.** The 90% case. Heterogeneous
     mark children (`line`, `scatter`, …), encodings as modifier methods.
     Auto-scaled axes, default theme, pan/zoom, crosshair — all free. Mirrors
     `chart3d`/`SceneSpec` verbatim.
   - **Layer 2 — scales + axes, configurable but defaulted.** `.x(Scale::time())`,
     `.y(Scale::log())`, tick formatters, axis/legend options. Power users
     override pieces without leaving the spec.
   - **Layer 3 — public primitives for fully-custom systems.** The `Scale` types
     (pure data↔pixel mapping + tick generation, usable standalone), the
     `SeriesHandle` bulk-data handle, and the data-layer draw op — all public, so
     a consumer composes their own plot from standard El chrome + scales + the
     data op. **Our own `plot()` is built only from these primitives, so a
     power-user can fork it** — the symmetry invariant, applied to plotting.

4. **Per-axis view state (`PlotView`) in `UiState`, keyed by the plot's key.**
   Mirrors `ViewportView`'s shape (`project`/`unproject`/`zoom_about`) but holds a
   **visible domain per axis in data space**, not a single uniform zoom. This is
   why we do *not* reuse the `viewport()` widget: plots need independent-axis,
   non-uniform zoom (zoom the time axis while Y autoscales to the visible data),
   which `viewport`'s uniform scale cannot express. Default gestures over the plot
   rect (drag-pan, wheel-zoom, cursor-anchored), with a controlled override so an
   app can drive or lock the view. `Y::autoscale` (fit Y to the visible X-window)
   is a per-axis policy.

5. **The handle is the only data interface; decimation is a pluggable stage —
   this unifies the "virtual" and "dump-everything" paradigms.** Both reduce to
   "the app `.set()`s samples into a `SeriesHandle`; damascene maps whatever it
   holds through the current scales and draws it." The *only* difference is who
   decimates for the pixel budget:
   - **Virtual / windowed** (the app owns resolution): the app reads the current
     visible window via `UiState::plot_view(key)` in `build()` (the **pull**
     model, matching `viewport_view_by_key` precedent), loads/resamples its source
     to ~pixel-width points over that range, and `.set()`s the handle. Damascene
     draws what it has. The app debounces during drags.
   - **Dump-everything** (the app hands the full series once): damascene owns an
     **opt-in decimation stage** (min/max-per-pixel-column envelope, or LTTB) that
     reduces to the pixel budget before GPU upload — `.downsample(MinMax)` /
     `auto`.

   One render path; decimation is app-side or library-side. **Rendering must
   never assume the data spans the visible domain** — windowed data covers a
   sub-range and may lag a drag; map samples by their actual data coordinates and
   leave honest gaps at the edges, never stretch-to-fit. (A coverage hint for a
   "loading…" affordance at the edges is a later nicety; design the hook now.)

6. **Color-managed from day one, for free.** Because the data layer rides the
   Scene3D pipelines, it inherits their linear-working-space correctness (convert
   author colors via `damascene_core::color`, no in-shader transfer function,
   float offscreen target). HDR/wide-gamut lights up with the same upstream knob,
   no plot rewrite.

7. **Time and high-dynamic-range data need f64; the GPU gets f32.** Epoch
   timestamps (ms ≈ 13 significant digits) overflow f32's ~7. **Data space is
   f64.** The scale maps f64 data → f32 scale-space relative to a per-axis domain
   origin (classic "subtract the origin, upload f32, carry the origin in the MVP")
   so the GPU never sees a giant absolute timestamp. State this in the handle and
   scale contracts; getting it wrong shows up as jitter when zoomed in on recent
   data.

## Key codebase findings (grounding, with file:line)

Verified against the tree on 2026-06-24; re-check before relying on exact lines.

- **Scene draw-op + payload (the template):** `DrawOp::Scene3D { id, rect,
  scissor, scene: Arc<Scene3DData> }` at `crates/damascene-core/src/paint/ir.rs:177`;
  `Scene3DData` at `crates/damascene-core/src/scene/data.rs:59`. The plot data
  layer will either reuse this op (V1, see Architecture) or get a sibling
  `DrawOp::Plot2D` routed into the same renderer.
- **Versioned geometry handles (reuse directly):**
  `crates/damascene-core/src/scene/geometry.rs` — `GeometryHandle<T>` (`:177`),
  `new`/`set`/`revision`/`bounds`/`snapshot` (`:184`–`:233`), `ScenePoint`
  (`:71`), `LineSegment` (`:81`), aliases `PointsHandle`/`LinesHandle`
  (`:253`,`:255`). **Gap:** `LineData` is `Vec<LineSegment>` — *disjoint pairs*. A
  time series is a connected **polyline**; representing it as pairs doubles
  vertices and breaks joins. This is the one genuine new primitive (see Risks).
- **GPU packing (single source of truth):** `crates/damascene-core/src/scene/gpu.rs`
  — `#[repr(C)] Pod` layouts + pure packing fns shared verbatim by all backends,
  colors converted via `to_linear`. Any new polyline/area packing goes here.
- **Backend renderer (reuse / extend):** `crates/damascene-wgpu/src/scene.rs`
  `Scene3DPaint` (`:250`); two-phase render `encode_scene_prepass`
  (`crates/damascene-wgpu/src/lib.rs:1603`) before the main pass; vulkano/ash
  equivalents in their `scene.rs`/`runner.rs`. `record_scene3d` default no-op at
  `runtime.rs:3335` (graceful degradation).
- **Scene-anchored labels (reuse for ticks/crosshair):** the `scene_label(...)`
  projection seam (`SCENE3D_PLAN.md` M4) — axis ticks were its first caller in
  3D; in 2D the projection is trivial (ortho), and tick/legend/crosshair text are
  ordinary themed `GlyphRun`s layered over the data rect.
- **Interaction-state precedent:** `ViewportView` (+ `project`/`unproject`/
  `zoom_about`) at `crates/damascene-core/src/viewport.rs:123`; the keyed-state
  pattern is `state/viewport.rs` and `state/camera.rs`. `PlotView` slots in the
  same way (`state/plot.rs`, keyed per node). Gesture routing mirrors
  `runtime.rs` viewport/camera handlers (`scene_at`/`viewport_at` → a drag
  capture).
- **El surface + spec precedent:** `chart3d` constructor
  (`crates/damascene-core/src/tree/constructors.rs:708`), `SceneSpec`
  (`scene/spec.rs:40`) with its two-tier `points` / `points_styled` / `add_*`
  builder convention. `plot` + `PlotSpec` copy this shape exactly.
- **Math vocabulary:** scene already depends on `glam` (`scene::glam`,
  `glam = "0.33"`). The plot's ortho camera/MVP reuses glam. Data-space
  coordinates are plain `f64` (see decision 7), not glam.

## Architecture

### Core (`damascene-core`)

New module `crate::plot` (sibling of `scene`, `surface`, `viewport`):

```rust
// --- crate::plot ---

/// A continuous, invertible map from a data-space axis (f64) to normalized
/// scale-space (f32), plus tick generation and value formatting. The
/// power-user primitive (Layer 3): pure, standalone, no El required.
pub enum Scale {
    Linear { /* domain: (f64, f64) */ },
    Log    { /* domain, base */ },
    Time   { /* domain in epoch units; nice time-tick generation */ },
    // Later: Band { .. } for bar/ordinal axes.
}
impl Scale {
    pub fn linear() -> Self; pub fn log() -> Self; pub fn time() -> Self;
    pub fn map(&self, v: f64, origin: f64) -> f32;   // data → scale-space (f32, origin-relative)
    pub fn invert(&self, s: f32, origin: f64) -> f64; // scale-space → data (for crosshair/hit-test)
    pub fn ticks(&self, window: (f64, f64), target: usize) -> Vec<Tick>; // nice numbers / time ticks
}

/// App-owned, versioned series data. Type alias / thin wrapper over a
/// GeometryHandle so it reuses the Scene3D caching contract verbatim.
/// Samples are (x: f64, y: f64); color/per-point fields optional.
pub struct SeriesHandle { /* GeometryHandle<SeriesData> */ }
impl SeriesHandle {
    pub fn new(samples: Vec<Sample>) -> Self; // allocates id
    pub fn set(&self, samples: Vec<Sample>);  // bump rev → re-upload (virtual mode hot path)
    pub fn append(&self, slice: &[Sample]);   // live tail growth (designed-in, like Scene3D)
    pub fn bounds(&self) -> (/*x*/(f64,f64), /*y*/(f64,f64));
}

/// Everything a backend needs to render one plot's data marks, backend-neutral.
/// Cheap to assemble each frame (Arc-clones of handles + the resolved view).
pub struct Plot2DData {
    pub lines:   Vec<LineMarkDraw>,     // polyline handle + style
    pub points:  Vec<PointMarkDraw>,    // scatter handle + style
    pub areas:   Vec<AreaMarkDraw>,     // (later) filled region
    pub view:    ResolvedView,          // ortho MVP from PlotView + scales + origins
    pub style:   PlotStyle,             // background, msaa, working color space
    pub decimate: Option<Decimation>,   // None = draw as-is (virtual); Some = library-side
}

/// How damascene reduces an over-dense series to the pixel budget (dump-everything).
pub enum Decimation { MinMax, Lttb }   // min/max envelope, or largest-triangle-three-buckets
```

**El resolves to the data op + chrome.** During `prepare`/`draw_ops`, a `plot`
node:
1. reads `PlotView` for its key (or initializes from data bounds — auto-domain),
2. for each axis, resolves the scale + per-axis domain origin → builds the
   orthographic `ResolvedView` (a uniform MVP mapping the visible scale-space
   window to the data rect),
3. assembles `Plot2DData` (Arc-clones of the series handles + the view + style),
4. pushes the **data-layer draw op** for the data rect, and
5. emits **chrome** as ordinary ops layered on top: gridlines + axis spines as
   `Quad`s, tick labels + axis titles + legend + crosshair readout as themed
   `GlyphRun`/`Quad` (the `scene_label` seam, trivial in ortho), scissored to the
   plot/data rect.

**Data-layer draw op — RESOLVED (2026-06-24): reuse `DrawOp::Scene3D`.** The
plot's `draw_ops` builds a degenerate `Scene3DData` (z=0 geometry, orthographic
camera) and pushes the existing op — *zero* new backend dispatch for
line+scatter. The one core change this requires is an **orthographic projection
mode on `ResolvedCamera`**: today it is perspective-only (`proj()` →
`Mat4::perspective_rh`). All three backends consume the camera *only* via
`scene.camera.view_proj(aspect)` (verified: `damascene-wgpu/src/scene.rs:571`,
`damascene-ash/src/scene.rs:525`, vulkano likewise), and label projection routes
through `project_to_screen`, which also uses `view_proj` — so making
`view_proj`/`proj`/`project_to_screen` return an orthographic matrix when the
camera is in ortho mode is **transparent to every backend**. The ortho proj maps
the visible scale-space window `[x0,x1]×[y0,y1]` to full NDC ignoring `aspect`
(a plot scales its axes independently — non-uniform by design). This also
delivers the "2D-lock camera mode" listed under `SCENE3D_PLAN.md` M4 Remaining.
A sibling `DrawOp::Plot2D` is deferred to if/when area/bar/dashing want a
plot-specific payload; until then the *pipelines*, offscreen/MSAA/composite path,
**and the op itself** are reused unchanged.

### El surface (`damascene-core`)

`plot([...marks])` is a block whose children are **marks** (`line(handle)`,
`scatter(handle)`, later `area`, `bar`) — heterogeneous El-style children, the
HTML/DOM shape. Per-mark encodings are **modifier methods**: `.color`, `.width`,
`.size`, `.shape`, `.label` (legend / cursor name). Plot-level config: `.x(scale)`,
`.y(scale)`, `.crosshair`, `.legend(LegendPosition)`, `.controls(PlotControls)`,
`.downsample`, `.y_autoscale`, etc. Two-tier adders per the `SceneSpec` convention: terse default-style
`line(h)` + an `add_line(LineMarkDraw)` / `line_styled(h, style)` escape hatch. No
parallel typed AST. Building the tree resolves (in `prepare`) to the data op +
chrome.

`Kind::Plot` (new), an El `plot_source: Option<Box<PlotSpec>>` field, and the
`plot(...)` constructor (crate-root + prelude re-export), exactly paralleling
`Kind::Scene3D` / `scene_source` / `chart3d`. Every public item documented
(`#![warn(missing_docs)]`).

### Interaction (`PlotView`)

`crate::plot::PlotView` (persisted in `UiState`, `state/plot.rs`, keyed per node):
a visible **domain per axis** in data space, plus the same projection algebra as
`ViewportView` lifted to per-axis, non-uniform, log-aware mapping.

**Gesture model (scientific-tool paradigm, adopted 2026-06-24 — matches
Grafana/uPlot/InfluxDB).** Over the plot's data rect:

- **Drag** = directional box-zoom. The selection axis is chosen by the dominant
  drag delta (X when `|dx| ≥ |dy|`, else Y), rendering a translucent rubber-band
  (`tokens::SELECTION_BG`) that spans the full height (X) or width (Y); release
  zooms that axis to the swept span. A sub-`MIN_ZOOM_PX` drag is a click. A
  **Y box-zoom opts the plot out of `y_autoscale`** (records the plot in
  `PlotState::y_manual`), so the manual value window sticks instead of being
  refit away next frame; an X zoom or pan leaves autoscale running.
- **Double-click** = reset to full extent (drops the persisted view *and* the
  `y_manual` opt-out, so the next `prepare_plots` re-fits and re-autoscales Y).
- **Shift+drag** = pan (per-axis; Y refits each frame under autoscale).
- **Wheel** = cursor-anchored zoom of the **time (X) axis only** — the value
  axis is left to autoscale / a Y box-zoom.

**Control scheme (app-selectable, mirrors `CameraControls`).** `PlotControls`
on the spec — `.controls(PlotControls::PanDrag)` — picks what the *primary*
(unmodified) drag does, with `Shift` doing the other; double-click and wheel are
scheme-independent. `ZoomDrag` (default) is the table above; `PanDrag` swaps
drag↔Shift+drag (trading-chart / maps style). There is deliberately no built-in
scheme-picker widget, exactly like the 3D camera. The router reads the resolved
`PlotMetrics::controls` on press.

Implemented in `state/plot.rs` (`begin_plot_zoom`/`drag_plot_zoom_to`/
`plot_zoom_band`/`end_plot_zoom`/`reset_plot_view`, with the `y_manual` opt-out
set) and routed in `runtime.rs` (press/move/release). `resolve_view` takes the
*effective* autoscale flag (`spec.y_autoscale && !y_manual`), recomputing the Y
domain from the visible X-window each frame unless the user has taken manual Y
control. A future `PlotRequest` (fit-all, set-domain, follow-tail-for-live) would
mirror `ViewportRequest`. The app reads the live view with
`UiState::plot_view(key)` for the virtual-data pull and for readouts.

**Open (next iteration):** axis-gutter drag-to-rescale a single axis, keyboard
nav, and a spec option to lock an axis against gesture navigation.

### Backends

If V1 reuses `DrawOp::Scene3D` (sub-decision a): **no backend changes** — the
plot is sugar that builds a degenerate scene. If/when a `DrawOp::Plot2D` lands
(sub-decision b): each backend adds a thin `record_plot2d` that routes into the
existing `Scene3DPaint` (or a `Plot2DPaint` sharing its pipelines), plus any new
polyline/area packing in the shared `scene::gpu` (or `plot::gpu`). The genuinely
new GPU capability — *joined polylines* and *filled areas* — is contained to a
shader/packing addition; the offscreen/MSAA/composite/depth machinery is reused.

## Milestones

### M1 — End-to-end line + scatter on wgpu, with the synthetic-data demo

The first client is **a demo example with synthetic data**, then a refined
showcase entry (per the 2026-06-24 discussion). M1 builds the vertical slice the
TSDB viewer needs and nothing more (go slow — one slice correct):

- `crate::plot`: `Scale::{linear,time}`, `SeriesHandle` (over `GeometryHandle`),
  `PlotSpec` + `line`/`scatter` marks, `Plot2DData`, `PlotStyle`.
- `Kind::Plot` + `plot_source` El field + `plot(...)` constructor (crate-root +
  prelude).
- `PlotView` in `UiState` (`state/plot.rs`) + drag-pan + wheel-zoom
  (cursor-anchored, per-axis) + `Y::autoscale` + auto-domain on first show.
- `draw_ops` emission: ortho `ResolvedView`, the data-layer op (reusing
  `DrawOp::Scene3D` — sub-decision a), gridlines/spines as `Quad`, tick labels +
  axis titles as themed `GlyphRun`.
- **Polyline primitive** (see Risks): joined polylines, not segment pairs.
- **Crosshair + nearest-sample readout** (TSDB-critical): screen→data via
  `PlotView::invert`, nearest-sample query, crosshair `Quad`s + value chip.
- **Virtual + dump duality:** `plot_view(key)` pull readback; opt-in `MinMax`
  decimation for the dump path.
- Color-space correctness inherited from the scene pipelines (verify: no
  in-shader transfer, float offscreen target).
- **Example:** `examples/src/bin/plot.rs` — a few synthetic time series
  (sine/noise/step), live tail append on a timer, pan/zoom, crosshair, legend.

**Acceptance:** example runs on wgpu; pan/zoom over the rect with per-axis
behavior and cursor-anchored zoom; Y autoscales to the visible window; crosshair
reads the nearest sample; the data layer re-uploads only on `rev`/scale-type
change (verify by panning a static large series — no re-upload); a 1M-point dump
series with `MinMax` decimation pans smoothly; chrome is crisp/themed; scene
clips and z-orders under other UI; `cargo check --workspace` green.

### M2 / M3 — vulkano + ash

If M1 reused `DrawOp::Scene3D`, these are **free** (the op already renders on all
three backends). If a `DrawOp::Plot2D` was introduced, add its `record_plot2d`
routing + naga-compiled polyline/area shaders, visually matching wgpu within
AA/tolerance. (Resolve which at M1's sub-decision.)

### M4 — Breadth + showcase polish

- Scales: `Scale::log`, `Scale::band` (bars/ordinal); multiple/secondary Y axes.
- Marks: `area` (filled, stacked), `bar`; line `dashing`; per-point color/size
  encodings (reuse scene colormaps).
- Chrome: legend styling + interaction (toggle series), tick-mark glyphs, label
  decluttering, axis/legend layout reserving space outside the data rect.
- Virtual-mode **coverage hint** → edge "loading…" affordance.
- Decimation: add `Lttb`; document the envelope-vs-LTTB trade-off.
- Refined **showcase** entry (a small TSDB-style dashboard). Calibrate against
  `docs/POLISH_CALIBRATION.md`.

### M5 — Power-user primitives: prove the fork-it symmetry

Publish and document Layer 3: `Scale` standalone, `SeriesHandle`, the data-layer
op. Add a "**build your own plot**" example that composes a custom chart from
public primitives + standard El chrome **without** `plot()` — the symmetry-
invariant proof (mirrors Scene3D's M5 custom-shader proof). Acceptance: the
custom example renders a working pan/zoom plot using only public API; `plot()`
itself is shown to be implementable from the same surface.

## Risks & edge cases

- **Polyline joins — RESOLVED (2026-06-24): reuse, no new pipeline.** A line
  series lowers to the existing scene **line** pipeline (one `LineSegment` per
  consecutive pair — AA quads with *butt* caps, confirmed in `scene_line.wgsl`)
  **plus a round disc from the existing scene point pipeline** (`scene_point.wgsl`
  renders AA circles) at each vertex, diameter = line width. The discs fill the
  wedge-gaps butt-cap joins leave, giving clean **round joins and round caps with
  zero new GPU code on any backend** — and it works for 3D line series too. The
  lowering (samples → `LineSegment`s + join `ScenePoint`s, in scale space) is pure
  core code, unit-tested. Quality envelope: clean for **opaque** lines (the TSDB
  case); known gaps are (a) translucent lines double-blend at the disc/segment
  overlap (darker seams) and (b) joins are round, not mitered. A dedicated
  miter/strip **polyline pipeline** is the documented upgrade for those cases;
  the mark *API* (a series of points + width + cap/join style) is identical
  either way, so the swap is internal, not breaking.
- **f64 data / f32 GPU** (decision 7): subtract the per-axis domain origin before
  upload; carry the origin in the MVP. Wrong → jitter when zoomed into recent
  timestamps. Bake into the `SeriesHandle`/`Scale` contracts and test it.
- **Scale-baked-at-upload** (decision 2/5): switching scale *type* re-uploads.
  Acceptable (rare). Pan/zoom *within* a scale stays a uniform — including log
  (pan/zoom is affine in log-space). If scale-switching ever gets hot, the
  fallback is scale-in-shader (a uniform `ScaleKind` + params), at the cost of
  diverging from the stock scene shaders.
- **Non-uniform / per-axis zoom feel** — reuse `zoom_about` algebra per axis;
  axis-locked drag via modifier; validate the gesture model in the demo before
  committing the input mapping.
- **Decimation correctness** — min/max-per-pixel preserves spikes (right for
  monitoring/TSDB); LTTB preserves shape but can hide spikes. Default `MinMax`
  for the TSDB client; make it a knob. Decimate in *scale-space* so log/time
  buckets are correct.
- **Virtual-mode edge gaps** — never stretch windowed data to fill the rect; map
  by true coordinates, scissor to the data rect, leave honest gaps. Debounce the
  app-side reload during drags.
- **Chrome layout** — axes/legend/titles reserve space *outside* the data rect;
  the data op gets the inner rect (like Scene3D's `inner`). Get the rect math
  right so gridlines align with tick labels.
- **SVG/bundle fallback** cannot render the GPU data layer; degrades to a
  placeholder (axes + labeled rect), as Scene3D does. The autonomous-polish
  artifact loop won't "see" data marks. Accepted.

## Non-goals

- No 3D (that is `chart3d`). No app-supplied *pipelines*, vertex layouts, or
  passes inside `plot()` (that is `surface()`).
- No data wrangling / statistics / aggregation engine — damascene plots what it is
  given (the app or its TSDB does transforms). Decimation is a *rendering* LOD
  stage, not analytics.
- No general scene graph / animation system.
- Not a charting *theming* DSL beyond the standard modifier chain.

## Operational context

- **Repo:** `/home/christian/workspace/damascene/damascene.main`, branch `main`
  (commit directly to main per project convention). Workspace is `crates/*`,
  `examples`, `tools`.
- **Reuse, don't fork:** build on `crates/damascene-core/src/scene/` (geometry
  handles, `gpu.rs` packing, the backend `scene.rs` renderers) rather than
  copying. New plot types live in `crates/damascene-core/src/plot/`.
- **wgpu 29** across the board (shared device) — keep it.
- **Build/check:** `cargo check --workspace`; run `examples/src/bin/plot.rs` on
  wgpu; targeted backend builds (`-p damascene-wgpu`, later `-p damascene-vulkano`,
  `-p damascene-ash`) rather than the full suite each iteration. Run the focused
  plot/scale unit tests during iteration; full suite once at the end.
- **Docs are the discovery surface:** every public `plot` item documented, each
  echoing its CSS/D3/Observable-Plot analog in the rustdoc (house style).
