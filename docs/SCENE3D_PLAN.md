# Scene3D: Backend-Neutral Small 3D Graphs & Models

## Goal

Add a closed-scope, highly-configurable 3D graph/model widget that is threaded
through Aetna's **backend-neutral draw-op layer** — not bolted on as a
host-composed `surface()`. An author writes:

```rust
chart3d([
    points(&self.scatter).color_by(&self.category).size(4.0),
    mesh(&self.model).material(Material::matte(palette.accent)),
    lines(&self.error_bars),
])
.grid(GridPlanes::XZ)
.orbit()
.key("scene")
```

…with **zero host glue**, and it renders identically on the wgpu, vulkano, and
ash backends, composites/clips/z-orders/themes like any other widget, and shows
crisp themed axis/data labels.

Target use cases: 3D scattered data (points), small mesh models, and 3D lines
(axes, wireframe, series, error bars). This is for "random apps that just want
to show some 3D data in a polished manner," not for full custom-pipeline scenes.

## Why this shape (settled decisions — do not re-litigate)

These were worked out in design discussion (2026-05-25) and are the premises of
the plan, not open questions:

1. **Threaded through core, not a `surface()` add-on.** A host-composed surface
   is wgpu-only and forces the app to own a renderer, a device, and ~450 lines
   of winit/frame glue (see `volumetric_ui_v2/src/host.rs` for the pattern we are
   *replacing* for this use case). Threading a fixed-scope scene through the
   backend-neutral draw-op stream gives all-backend support and zero-glue
   integration. `surface()` remains the escape hatch for apps that genuinely need
   a custom *pipeline* — that is the closed-scope boundary: **the moment an app
   wants to own the device, the vertex layout, or its own render passes, it has
   left `chart3d` and is in host-composed land.** App-supplied *material shaders*
   do **not** cross this line: they reskin the fragment within aetna's pipeline
   and device management (see "Color management" and "Custom material shaders").

2. **`aetna-core` stays backend-neutral / "not a game engine."** Per
   `docs/LIBRARY_VISION.md`, core owns layout, draw-op preparation, math, and
   interaction state — but no rendering pipelines and no scene graph. So:
   - The **scene data types** (geometry, camera, style) live in core as a
     draw-op payload, by exact analogy to how `DrawOp::Vector` carries
     `crate::vector::VectorAsset` and `DrawOp::AppTexture` carries
     `crate::surface::AppTexture`. Payload types are core's; rendering is not.
   - The **renderer pipelines** live in each backend crate.

3. **Closed *data pipeline*, two open axes.** Exactly three pipelines —
   instanced points, forward-lit mesh, lines — plus a reference grid and a small
   fixed light rig. aetna owns the vertex layouts, buffers, passes, depth, MSAA,
   and device. What is open: (a) configuration as **backend-neutral data** (mark
   encodings + a scene-level style block), and (b) the **material/fragment
   shading**, which apps may replace with a custom shader the same cheap way they
   reskin stock quads (post-V1; see "Custom material shaders"). Apps never supply
   a *pipeline* or vertex layout — that is still `surface()`.

4. **Color-space aware from day one.** The scene renders and blends entirely in
   the runner's working linear space and emits unencoded into it, so it tracks
   the color-management work already on the backend and lights up HDR/wide-gamut
   automatically once the upstream wgpu blocker clears (see "Color management").
   This is V1, not deferred.

5. **Core owns the camera.** Orbit/zoom/pan state lives in `UiState`, keyed by the
   chart's key, exactly where `ScrollState` already lives. Default gestures over
   the viewport rect (LMB orbit, scroll zoom, MMB/shift-drag pan), auto-framed to
   the data bounds on first show and when bounds change, with a controlled
   override so an app can drive or lock the camera.

6. **Geometry uses app-owned, versioned, CPU-side handles.** Same *pattern* as
   `AppTexture` (stable id, app-owned content, backend-cached GPU resource) but
   holding **CPU-side data** rather than a `wgpu::Texture`, so it stays
   backend-portable and the app never touches a device. The backend owns the GPU
   buffers and re-uploads only when a handle's revision advances.

## Key codebase findings (grounding, with file:line)

Verified against the tree on 2026-05-25; re-check before relying on exact lines.

- **Draw-op IR:** `crates/aetna-core/src/paint/ir.rs:29`. High-level ops
  (`Quad`, `GlyphRun`, `AttributedText`, `Icon`, `Image`, `AppTexture`, `Vector`,
  `BackdropSnapshot`). Material lives in shader handles + uniform blocks; **no
  vertex buffers flow through the IR today** — backends tessellate. `Scene3D`
  will be the first op carrying bulk 3D geometry; precedent for non-uniform
  `Arc` payloads is `Image` (pixel data, keyed by `content_hash()`),
  `AppTexture` (keyed by `AppTextureId`), and `Vector` (`Arc<VectorAsset>`).
- **Mid-frame pass restructuring already exists:** `Runner::render`
  (`crates/aetna-wgpu/src/lib.rs:1218`) splits the paint stream into Pass A /
  `BackdropSnapshot` / Pass B, using the MSAA-view-as-render-target +
  `target_view`-as-resolve pattern (`resolve_target` at ~1233). We do not need to
  invent pass management.
- **Per-op dispatch:** `Runner::record` + `record_runs` / `record_icon` /
  `record_image` / `record_app_texture` / `record_vector`
  (`crates/aetna-wgpu/src/lib.rs:193–284`). Add `record_scene3d`.
- **Composite path exists in all three backends:** `SurfacePaint::record`
  (`crates/aetna-wgpu/src/surface.rs:208`); `aetna_wgpu::app_texture()`
  (`surface.rs:472`) enforces **single-sample + `TEXTURE_BINDING`**
  (`surface.rs:487`, `:493`). Each of `aetna-vulkano` and `aetna-ash` has its own
  `surface.rs`. ⇒ The scene renders to an MSAA color+depth target, **resolves to a
  single-sample texture**, and composites through this existing path (clipping,
  scissor, z-order come for free).
- **MSAA:** `MsaaTarget` at `crates/aetna-wgpu/src/msaa.rs`.
- **One shader source, all backends:** `aetna-vulkano` and `aetna-ash` both depend
  on `naga` pinned to wgpu 29's naga (`wgsl-in`, `spv-out`) and each has a
  `naga_compile.rs`; the Cargo comment states "a shader that compiles for one
  compiles for both." ⇒ **Author the three pipelines once in WGSL.** wgpu consumes
  it natively; vulkano/ash compile WGSL→SPIR-V via their existing naga path (the
  same path the stock UI shaders already use).
- **Version alignment:** `aetna-wgpu`, `aetna-winit-wgpu`, and
  `volumetric_renderer` are all on **wgpu 29** — the shared-device version
  constraint is already satisfied.
- **Camera-state precedent:** `crates/aetna-core/src/state.rs` has
  `ScrollState` (`mod scroll`, field `scroll`). Camera state slots in the same
  way, keyed per node.
- **Port source:** `~/workspace/volumetric/main/crates/volumetric_renderer/`
  - WGSL: `src/shaders/point.wgsl`, `src/shaders/line.wgsl` — take largely as-is.
    `src/shaders/mesh_gbuffer.wgsl` — **rewrite as a forward-lit pass** (single or
    hemispheric directional light + ambient), not deferred. Skip
    `composite.wgsl` and `ssao.wgsl` entirely (the deferred/SSAO machinery is
    out of scope).
  - `src/camera.rs` — `Camera` (orbit/pan/zoom, `focus_on`, `zoom_clamped`),
    `CameraControlScheme`, `CameraAction`, `CameraInputState`. Lift/adapt.
  - `src/types.rs` — `MeshVertex`, `PointStyle`, `LineStyle`, `GridSettings`,
    `GridPlanes`, `DepthMode`, etc. Adapt into the core scene types.
- **Math vocabulary — glam (decided).** `aetna-core` already directly depends on
  `nalgebra` and `bytemuck`-derive, and `Cargo.lock` carries ~19 transitive
  `glam` versions, so "keep core dep-light" is not a real constraint here. The
  scene API speaks **glam** (added as a direct dep, `glam = "0.33"`, latest
  stable) because it is what apps reach for and what LLMs know — re-exported from
  `crate::scene` (`scene::glam`) so downstream pins aetna's exact version and
  avoids the pre-1.0 two-incompatible-`Vec3` footgun. Implemented in
  `crate::scene`: `scene::bounds::Aabb` (glam has no AABB) over `glam::Vec3`;
  vertex types use `glam::Vec3` for position/normal and `[f32;4]` for
  authoring-space colour. `crate::math` is MathML/TeX *typesetting*, unrelated.
  (Earlier slices briefly used a hand-rolled `scene::linalg`; removed in favour
  of glam.)

## Architecture

### Core (`aetna-core`)

New module `crate::scene` (sibling of `surface`, `image`, `vector`) holding the
backend-neutral data:

```rust
// --- New draw-op variant in crates/aetna-core/src/paint/ir.rs ---
DrawOp::Scene3D {
    id: String,
    rect: Rect,
    scissor: Option<Rect>,
    scene: std::sync::Arc<crate::scene::Scene3DData>,
}

// --- crate::scene ---

/// Everything a backend needs to render one scene, all backend-neutral.
/// Cheap to assemble each frame: it holds Arc-clones of the geometry
/// handles (no geometry copy) plus the resolved camera/style.
pub struct Scene3DData {
    pub meshes: Vec<MeshDraw>,   // handle ref + transform + material
    pub points: Vec<PointDraw>,  // handle ref + transform + style
    pub lines:  Vec<LineDraw>,   // handle ref + transform + style
    pub camera: ResolvedCamera,  // view + projection (from UiState + auto-frame)
    pub lights: LightRig,        // fixed small rig (1–2 dir + ambient)
    pub style:  SceneStyle,      // grid, background, theme colors, msaa, working color space
}

/// A mark's material. Stock recipes cover V1; the custom arm is wired into the
/// type from day one but only *implemented* post-V1 (see "Custom material
/// shaders"). Carrying it now means adding it later is non-breaking.
pub enum Material {
    Matte { base: Color, .. },          // stock forward-lit recipes
    Flat { color: Color },
    // Post-V1: app reskins the fragment via aetna's existing custom-shader path.
    // aetna still owns vertex layout, buffers, passes, depth, MSAA, device.
    Custom { shader: ShaderHandle, uniforms: UniformBlock },
}

// All `Color`s above are authoring-space; the backend converts them to the
// runner's working linear space at upload time (see "Color management").
// Nothing here encodes for output — that stays the compositor's job.

/// App-owned, versioned, CPU-side geometry handle (the AppTexture pattern,
/// CPU data). Created once, stored in app state, cheap to clone + reference.
/// Backend caches a GPU buffer keyed by `id`, re-uploads when `rev` advances.
pub struct PointsHandle { /* Arc<GeometryStore<PointData>> */ }
pub struct MeshHandle   { /* Arc<GeometryStore<MeshData>>  */ }
pub struct LinesHandle  { /* Arc<GeometryStore<LineData>>  */ }

// GeometryStore<T> holds: stable GeometryId, atomic rev, the data, cached bounds.
impl<T> Handle<T> {
    pub fn new(data: T) -> Self;          // allocates id
    pub fn set(&self, data: T);           // bumps rev → whole-buffer re-upload (baseline)
    pub fn update_range(&self, start: usize, slice: &[T::Elem]); // designed-in: dirty range
    pub fn append(&self, slice: &[T::Elem]);                     // designed-in: grow w/ headroom
    pub fn bounds(&self) -> Aabb;         // cached at set()-time; feeds auto-frame + tick ranges
}
```

**Update contract:** baseline backend re-uploads the whole buffer when `rev`
changes and ignores `update_range`/`append` hints (they just bump `rev`). The
hints are wired through `Scene3DData` from day one so a later backend can do
partial `write_buffer` / buffer growth with **no API break**. Animation that only
moves a model is a per-frame *transform* (a uniform) — `rev` never advances, so
nothing re-uploads.

**Camera state** (`crate::state`, new `mod camera` beside `mod scroll`):
per-node `CameraState { orbit_yaw, orbit_pitch, zoom, pan, framed: bool }`.
During `prepare`, the runtime reads it, combines with the data bounds
(auto-frame when `!framed` or bounds changed) to build `ResolvedCamera`, and
folds that into `Scene3DData`. Pointer events over the scene rect mutate it
(orbit/zoom/pan); a controlled override lets the app set/lock it.

**Labels/ticks** are projected 3D→screen in core (pure matrix math; clip anchors
behind the camera) and emitted as ordinary `GlyphRun` ops layered over the
scene, so they are crisp, themed, and identical across backends.

### El surface (`aetna-core`)

`chart3d([...marks])` is a block whose children are **marks**
(`points(handle)`, `mesh(handle)`, `lines(handle)`) — heterogeneous El-style
children, the HTML/DOM shape (see the `feedback_html_paradigm` memory). Encodings
and config are **modifier methods** (attributes): `.color_by`, `.size`,
`.material`, `.grid`, `.background`, `.orbit`, `.axes`, etc. No parallel typed
AST. Building the tree resolves (in `prepare`) to a single `DrawOp::Scene3D`
whose `Scene3DData` holds Arc-clones of the referenced handles + the resolved
camera + style.

### Backends (`aetna-wgpu`, then `aetna-vulkano`, `aetna-ash`)

Each adds a `Scene3DRenderer` (e.g. `src/scene3d.rs`) and a `record_scene3d`
that, given a `DrawOp::Scene3D`:

1. For each referenced geometry handle, look up `GeometryId` in a buffer cache;
   upload/re-upload if `rev` changed (whole-buffer baseline).
2. Render points/mesh/lines + grid into an **offscreen MSAA color + depth**
   target sized to `rect * scale_factor` (device pixels).
3. **Resolve** MSAA → a single-sample texture (`TEXTURE_BINDING`).
4. Composite that texture into the UI target at `rect` via the existing surface
   compositing path (`SurfacePaint::record` and equivalents).

The three WGSL shaders are shared verbatim across backends (single source of
truth — match whatever convention stock shaders use for source location; if
stock WGSL is core-owned constants, put scene WGSL there too; if backend-embedded
via `include_str!`, embed from one shared file). wgpu uses WGSL directly;
vulkano/ash route it through their existing `naga_compile` path.

**The one genuinely new backend capability is a depth attachment** — the UI has
none. It lives only inside the scene's offscreen pass, so it is contained.
`aetna-vulkano` and `aetna-ash` are both Vulkan; their `Scene3DRenderer`
pipeline/SPIR-V logic should be largely shared between them.

### Color management (V1)

Aetna's color model (see `docs/LIBRARY_VISION.md`, the `project_color_management_hdr`
memory, and `Runner::working_color_space`) is: authoring-space `Color` → **linear
working/composite space** (all blending happens here) → the **compositor owns
final output encoding** (e.g. PQ emission). Today the working space is sRGB and
HDR output is blocked solely on an upstream wgpu swapchain-colorspace knob
(`VK_EXT_swapchain_colorspace`, not yet exposed) — *not* on aetna. The whole point
of doing color right here is that **when that knob lands, Scene3D lights up
HDR/wide-gamut with no scene rewrite.** Requirements:

- The scene renders and blends **entirely in the runner's working linear space**.
  Read it from the backend (`working_color_space()`); do not assume sRGB.
- **Convert every author-facing color** (mark colors, material base, grid,
  background, lights) to working-linear via `aetna_core::color`, the same
  mechanism the stock quad/text materials use. **Do not hand-roll `srgb_to_linear`
  in scene shaders** — that is the exact mistake `volumetric_ui_v2`'s host made
  (`bg_color`) and must not be carried over. Scene shaders operate purely on
  already-converted linear values, so they are primaries-agnostic and wide-gamut
  ready.
- **No in-shader output encoding or tonemapping.** The scene emits unencoded
  linear working-space color into its offscreen target; encoding stays the
  compositor's job, identical to every other draw op. This is what makes
  `>1.0` (bright/emissive) values flow through to HDR output for free later.
- The offscreen color target should be **float-capable** (e.g. `Rgba16Float`) for
  HDR headroom and to avoid lighting banding, resolved to a format the
  `app_texture` composite path accepts. **Implementation check:** confirm
  `aetna_wgpu::app_texture` (and the vulkano/ash equivalents) accept the chosen
  resolve format; if not, growing the accepted-format set is part of this work.
- Composite the resolved scene texture through the same surface path as
  `AppTexture`, so working-color-space handling stays consistent with the UI.

### Custom material shaders (BYO — designed-in V1, implemented post-V1)

BYO shaders are a load-bearing Aetna theme — apps inject WGSL to reskin materials
without touching aetna's data pipelines or device management
(`docs/SHADER_VISION.md`: `ShaderBinding::custom`, `register_shader_with`,
`ShaderHandle` + `UniformBlock`, theme role→shader routing). Scene materials get
the **same** affordance: `Material::Custom { shader, uniforms }` lets an app
replace the **fragment/material shading** of a mark while aetna keeps the vertex
layout, geometry buffers, camera, passes, depth, MSAA, and device. This is the
"cheap and powerful" model — *not* a custom pipeline (that's `surface()`).

What V1 does: carry `Material::Custom` in the type so adding it is non-breaking,
but only implement the stock recipes.

What the post-V1 expansion takes (small, mirrors the backdrop-sampling contract):

- **Define and document a stable scene material-shader interface** — the fixed
  vertex→fragment varyings (interpolated world position, normal, view dir, uv if
  any) and the bind-group layout the scene always provides (camera, lights, the
  generic vector uniform slots). A custom fragment conforms to this contract and
  returns **linear working-space** color (it inherits the color-management rules
  above — no output encoding).
- Reuse the existing per-backend custom-WGSL registration
  (`register_shader_with` and the pipeline cache) — the scene renderer swaps the
  fragment module for marks whose material is `Custom`, keyed by `ShaderHandle`.
- Optionally allow scene materials to opt into backdrop sampling later, reusing
  the one-snapshot-per-frame contract already in core.

Boundary restated: custom **material shader** = supported reskin inside aetna's
pipeline. Custom **pipeline / vertex layout / passes / device** = `surface()`.

## Camera system (task 8 — redesign)

The M1 `CameraState` was *relative*: yaw/pitch/zoom-multiplier/pan, re-framed
against content every `resolve()`, with near/far fit to the content sphere. That
"always anchor on content" caused a grid-clipping bug (near plane pinned at
`content_radius` in front of the centre, zoom-invariant) and isn't the camera
feel we want. Redesigned to mirror the volumetric renderer's free camera, with
anchoring configurable. Settled with the user (do it right; one control scheme;
animate any goal change):

- **Absolute persistent pose**: `CameraState { target: Vec3, distance: f32,
  yaw: f32, pitch: f32 }` — world-space, mutated by gestures, persisted in
  `UiState` keyed per node (the `ScrollState` pattern). Not re-derived from
  content each frame.
- **Multiplicative (log/exp) zoom**: `zoom_by(factor)` multiplies `distance`;
  animated zoom interpolates `log(distance)` so perceived rate is constant at
  any scale. Pan/orbit sensitivity scales with `distance` (volumetric's feel).
- **Framing policy** (configurable anchoring): `Framing::Auto` (default — fit
  content once, then free; re-centre `target` smoothly when bounds change) ·
  `Fit` (re-fit every frame) · `Manual` (app owns the pose).
- **Decoupled near/far**: computed from the eye distance to the *full* view
  extent (content ∪ grid/axes reference bounds), not the content radius. Kills
  the clip for every policy.
- **Animated goal changes** — one mechanism: the keyed camera holds `current`
  + `goal` poses + per-channel velocity; a spring integrates `current → goal`
  each frame. *Everything that sets a new viewpoint sets `goal`*: data
  re-centre, app refocus (`focus_on`/`look_at`), reset, view presets, wheel
  zoom → spring animates. **Active drag writes `current` and `goal` together
  (crisp 1:1, no lag).** Reuses Aetna's spring (`anim::SpringConfig`,
  semi-implicit Euler, retarget-preserves-velocity) via a scalar stepper
  extracted from `Animation::step_spring`; runs 6 scalar springs (target.xyz,
  log distance, yaw [shortest-path], pitch) in a small camera tick, *not* the
  node `AnimProp` path. Presets: `GENTLE` for refocus, `QUICK` for zoom.
  Unsettled channels keep the frame requesting redraw (existing
  `next_paint_redraw_in` scheduling).
- **One control mapping** (no scheme menu): left-drag orbit, shift/right-drag
  pan, wheel zoom. Left-drag is free to use — this is a widget, not an editor.
- **App refocus**: declarative — a focus request on the spec that animates
  whenever it changes (fits the rebuild model).

Implementation slices: **(a)** absolute `CameraState` + `Framing` + decoupled
near/far (kills the clip; example uses `Manual` app-owned pose + buttons via the
new methods). **(b)** keyed camera in `UiState` + spring-driven current/goal +
animated re-centre/refocus. **(c)** pointer/wheel gesture routing over the scene
rect. `ResolvedCamera` (eye/target/up/fov/near/far + `project_to_screen`) is
unchanged — only how the pose and planes are produced changes.

**Status: all three slices done** (`9f059c8`, `43e4c0b`, `9d632f4`). The keyed
camera lives in `state/camera.rs` (`CameraStore`/`KeyedCamera` + a self-contained
6-channel scalar spring), ticked in `prepare_layout` before `draw_ops`; gestures
route through `runtime.rs` (`scene_at` + a `CameraDrag` capture mirroring the
scrollbar). The example is `Framing::Auto`: drag = orbit, shift/right-drag = pan,
wheel = zoom — zero app glue — with declarative animated focus buttons. 5 camera
tests cover the spring, log-zoom, shortest-path yaw, animated data re-centre, and
drag/wheel/hit-routing. Known limits to revisit: zoom uses the same GENTLE spring
as glides (could be QUICK per the original note); a re-click of the *same* focus
value after dragging is a no-op (declarative change-detection); axis labels still
deferred to M4.

## Milestones

### M1 — End-to-end on wgpu (vulkano/ash fall back to placeholder)

Because the op is backend-neutral and unimplemented ops degrade to a placeholder
(as the SVG path already does), this is graceful and app-invisible; vulkano/ash
render a labeled placeholder until M2/M3.

- `DrawOp::Scene3D` + `crate::scene` data types + the three versioned geometry
  handles in `aetna-core`.
- `chart3d` / `points` / `mesh` / `lines` El surface + modifiers.
- `CameraState` in `UiState` + default orbit/zoom/pan gestures + auto-frame +
  controlled override.
- 3D→screen label projection emitting `GlyphRun`s.
- `Scene3DRenderer` + `record_scene3d` in `aetna-wgpu` (port volumetric WGSL:
  point/line as-is, mesh rewritten forward-lit; MSAA+depth → resolve →
  composite).
- **Color-space correctness:** render/blend in the runner's working linear space,
  convert all colors via `aetna_core::color`, no in-shader sRGB or tonemapping,
  float-capable offscreen target (see "Color management"). `Material::Custom` is
  present in the types but unimplemented.
- One example in `examples/` (or a backend demo): a scatter cloud + a small mesh,
  orbiting, themed, with axis labels.

**Acceptance:** example runs on wgpu; orbit/zoom/pan work over the rect; scene
clips and z-orders correctly under other UI; labels are crisp; resizing keeps the
scene undistorted; `cargo check --workspace` green; geometry re-uploads only on
`rev` change (verify with a streaming-points tweak); scene colors match the rest
of the UI (no sRGB double-encode), verified by eye against a themed reference and
by confirming no `srgb_to_linear` exists in scene shaders.

### M2 — vulkano `Scene3DRenderer`

Reuse the shared WGSL via naga. Acceptance: same example renders on vulkano,
visually matching wgpu within AA/tolerance.

### M3 — ash `Scene3DRenderer`

Reuse the M2 Vulkan pipeline/SPIR-V logic where possible. Acceptance: same
example renders on ash.

### M4 — Config breadth + polish

**Landed (axis labels + colormaps):**

- **Colormaps** — `scene::colormap`: `Colormap` enum (Viridis/Magma/Plasma/
  Turbo/Cividis/Grayscale) sampled from piecewise-linear anchor tables, plus
  `colormap(t, map)` and a `PointData::from_values(positions, values, domain,
  map)` constructor. Pure CPU; colours ride the normal authoring→linear upload
  path, so no renderer change.
- **Axis labels** — `scene::axes`: a rich, per-axis config on `SceneSpec`
  (`Option<Axes>`, `None` = no labels). `AxisSpec { visible, title, range,
  ticks, format }`; `AxisRange::World | Linear{world_span, data}` (world coords,
  with an optional linear remap for display — *no* coordinate system imposed on
  the scene); `TickPolicy::FromGrid | Count | Step`; `TickFormat` enum.
  - Emission is backend-neutral: `draw_ops` projects tick/title world positions
    through the resolved `ResolvedCamera` and pushes `DrawOp::GlyphRun`s
    (culled behind-camera/off-rect, scissored to the scene rect). Renders on
    every backend through the normal text pipeline; the scene 3D itself is still
    wgpu-only, so on vulkano/ash labels float over the placeholder until M2/M3.
  - **Reusable seam:** the projection step is factored as `scene_label(...)` —
    the single primitive every future scene-anchored label (point labels, hover
    tooltips) plugs into. Axis ticks are its first caller.

**Remaining:** stock mesh materials (matte/flat/smooth, base color), grid
options, hemispheric light tuning, 2D-lock camera mode, axis tick *marks* +
legend styling, more scene-anchored label kinds (point labels, hover tooltips —
build on `scene_label`), and the SVG/bundle placeholder (projected bounds
wireframe + labeled rect). Calibrate against `docs/POLISH_CALIBRATION.md`.

### M5 — Custom material shaders (BYO)

Implement `Material::Custom`: define + document the scene material-shader
interface (varyings + bind-group contract, linear working-space output), wire it
through each backend's existing custom-WGSL registration, and add an example that
reskins a mark's material with app WGSL. Small, but do it after the stock path is
stable so the interface contract is informed by real materials. Acceptance: an
app-supplied material shader reskins points/mesh with no change to aetna's
buffers, passes, or device ownership, on at least the wgpu backend.

**HDR follow-on (not a milestone — an upstream dependency):** when wgpu exposes
the swapchain-colorspace knob and aetna sets a wide/HDR working space, confirm
Scene3D emits HDR correctly with no scene change. The color-management work in M1
is what makes this a confirmation rather than a project.

## Risks & edge cases

- **Depth buffer** is the only new backend GPU concept; keep it scoped to the
  offscreen pass.
- **SVG/bundle fallback cannot render 3D.** It degrades to a placeholder, so the
  autonomous-polish artifact loop will not "see" 3D content. Accepted.
- **Caching contract must be enforced by the types** (Arc + atomic rev) or apps
  will silently re-upload geometry every frame. Document the update API on the
  handle rustdoc.
- **Label projection** must drop/clip anchors behind the camera plane.
- **DPI/scale:** render the offscreen target at `rect * scale_factor`
  (volumetric does this via `viewport_extent`).
- **MSAA sample count:** match the UI's sample count or make it a `SceneStyle`
  knob; always resolve before composite (`app_texture` rejects multisampled).
- **Color management:** render/blend in the runner's working linear space and
  composite through the same surface path as `AppTexture`; convert colors via
  `aetna_core::color`, never in-shader. Float offscreen target — **verify the
  resolve format is accepted by `app_texture`**, or grow that accepted set.
- **Custom material-shader interface stability:** defer `Material::Custom` to M5
  so the varying/bind-group contract is shaped by real stock materials, not
  guessed up front — once published it is a compatibility surface.

## Non-goals

- No app-supplied *pipelines*, vertex layouts, or render passes inside `chart3d`
  (that is `surface()`). App-supplied *material shaders* **are** in scope, post-V1.
- No deferred rendering / SSAO / G-buffer (volumetric's heavy path).
- No general scene graph, ECS, or animation system.
- Not replacing host-composed `surface()` for full custom renderers.
- **2D graphs are a separate track** and explicitly out of scope here (they are
  mostly buildable on the existing 2D paint stream + one optional shader).

## Operational context

- **aetna repo:** `/home/christian/workspace/aetna/aetna.main`, branch `main`,
  clean at planning time. Workspace is `crates/*`, `examples`, `tools`. Start on a
  feature branch.
- **Port source:** `~/workspace/volumetric/main/crates/volumetric_renderer`
  (`src/shaders/`, `src/camera.rs`, `src/types.rs`). `volumetric_renderer` is a
  separate repo; copy/adapt, do not add a path dependency on it (it carries the
  deferred/SSAO renderer and a CAD-specific surface we do not want).
- **wgpu 29** across the board; keep it that way (shared device).
- **Build/check:** `cargo check --workspace`; run the M1 example on wgpu. Run
  targeted backend builds (`-p aetna-wgpu`, later `-p aetna-vulkano`,
  `-p aetna-ash`) rather than the full suite each iteration.
- **Crate placement note:** v1 puts scene data types in `aetna-core::scene`
  (analogous to `surface`/`vector`) — **done** for the geometry foundation
  (`scene::linalg`, `scene::geometry`). Core stays glam/bytemuck-free (see the
  findings bullet). If the types later grow beyond what core should host, extract
  to a backend-neutral `aetna-scene` crate (no wgpu; no cycle: core would depend
  on it, backends depend on both). Don't start with the extra crate unless core
  placement proves awkward.
- **Progress (M1):** the whole backend-neutral layer is done and committed —
  geometry handles (`c8172f9`), glam switch (`623db4c`), orbit camera
  (`6d15231`), style/material/light/`Scene3DData` (`125c765`), and
  `DrawOp::Scene3D` + core match-site placeholders (`a0c59e1`). Workspace
  `cargo check` clean; 14 scene unit tests green. Nothing renders yet by design:
  `prepare_paint` skips the op, so no `PaintItem`/backend code exists.
  The `chart3d` El surface is also done (`55ae27e`): `scene::SceneSpec` builder,
  `Kind::Scene3D` + `scene_source` El field, `chart3d(...)` constructor
  (crate-root + prelude), and `draw_ops` emission that auto-frames the camera
  and pushes `DrawOp::Scene3D`. 1050 core lib tests green. A `chart3d(...)` call
  is now a usable API; it just renders nothing because `prepare_paint` still
  drops the op.

  **The wgpu renderer now renders.** Landed in three slices:
  - Paint plumbing (`090f1ca`): `PaintItem::Scene3D` + `TextRecorder::record_scene3d`
    (default no-op) + `prepare_paint` wiring + placeholder match arms in the
    wgpu/vulkano/ash render loops. Backends without a scene renderer keep the
    no-op recorder, so they emit no item and paint nothing — same as before.
  - Scene WGSL (`bb3f8c5`): `scene_point` / `scene_line` / `scene_mesh` in
    `aetna-core/shaders`, exposed via `stock_wgsl`. Point/line ported from
    volumetric; mesh rewritten single-pass forward-lit (deferred g-buffer +
    ssao dropped). Linear working space, premultiplied output, no transfer
    function in-shader.
  - wgpu `Scene3DPaint` (`e4102d0`): geometry buffer cache keyed by
    `GeometryId` (+ working space for colour buffers), re-upload only on
    `rev` change; per-node offscreen `Rgba16Float` + `Depth32Float` target,
    MSAA at `style.msaa_samples`, resolved to single-sample, evicted after one
    untouched frame; three scene pipelines per sample count; dynamic-offset
    uniforms. Two-phase: `render()` encodes each scene's offscreen pass before
    the main pass (BackdropSnapshot discipline), then `PaintItem::Scene3D`
    composites the resolved texture through the stock surface pipeline.
    **fp16 + linear chosen for the offscreen target so HDR turns on with the
    swapchain knob, no scene change.** Validated end to end by a headless GPU
    render test (`tests/scene3d_render.rs`): lit cube + scatter + line composite
    to ~15% coverage on a real Vulkan adapter; skips cleanly without a GPU. This
    test is also where the scene WGSL is naga-validated.

  Grid + axes landed (`5b59c1e`): per-frame line batch from `SceneStyle.grid` /
  `show_axes` through the scene line pipeline (depth-tested, no write), muted
  R/G/B axes. Example landed (`e06fe98`): `examples/src/bin/scene3d.rs` — lit UV
  sphere + Fibonacci scatter + orbit-guide rings + grid, camera driven by buttons
  via the public `CameraState`. A `uv_sphere_winds_outward` render test guards the
  example's mesh winding.

  **Remaining for M1:** (1) interactive **pointer** camera — keyed `CameraState`
  in `UiState` (mirror `ScrollState`), orbit/zoom/pan gestures over the rect
  (task 8). The example currently drives the camera with buttons; the public
  state is the same one gestures will mutate. (2) Axis **labels** (crisp
  3D→screen-projected text) are deferred to M4 ("axis/tick/legend styling") —
  `ResolvedCamera::project_to_screen` exists, but labels need the resolved
  camera at op-processing time, not at app `build`. vulkano/ash still render
  nothing (placeholder arms); M2/M3 reuse the shared WGSL via naga.
