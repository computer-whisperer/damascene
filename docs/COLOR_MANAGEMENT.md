# Color Management: Damascene's Client Half

## What this is

Damascene is a Wayland **client**. Color management is a negotiation between client
and compositor defined by `wp_color_management_v1` (the staging
`color-management-v1.xml`). This doc fixes the division of labor that protocol
defines, records what Damascene does today, and sketches the behaviors that would
make Damascene's half *correct* — including ones no compositor we run on supports
yet, so we build toward the right target rather than toward "whatever makes our
own compositor agree."

> We develop both Damascene and the `prism` compositor. That makes "it works because
> both sides agree" a trap. The only durable correctness check is the spec and
> the reference client (Mesa's WSI — the same path GTK/Qt/SDL/DXVK take). When
> something is wrong, identify which side the spec assigns the responsibility to
> and fix *that* side, even when we own both.

## The protocol's division of labor

Everything is exchanged as **image descriptions** — `{primaries, transfer
function, luminances (incl. reference white), target/mastering volume,
max_cll/fall}`. The protocol is **descriptive, not imperative**: nobody commands
"render at N nits"; each side describes, the compositor maps.

**Compositor owns:**
- **Capabilities** — which descriptions a client may create (`supported_*`
  events). The gate.
- **Preferred descriptions** — per-output ("what the panel is/expects":
  gamut via `primaries`/`target_primaries`, range via `luminances`/
  `target_luminance`) and per-surface (the current best encoding for *this*
  surface, updated via `preferred_changed2` as it moves / settings change).
  These are **hints**: "clients may set any valid image description."
- **Anchoring** — per `set_luminances`, the compositor must make a
  `reference_lum`-level signal on *any* description produce the *same* output
  level. **White-point leveling across surfaces is the compositor's job**, not
  the client's.
- **Mapping to the panel** — tonemap/gamut-map each surface's declared
  description to the output, honoring the render intent. The algorithm is *not*
  negotiated; only the intent is.

**Client (Damascene) owns:**
- **Describing its content** — the color volume + luminance frame it is
  sending.
- **Render intent** — the ICC mapping policy (perceptual for UI).
- **Tonemapping within its declared volume** — if content exceeds what it
  declared, that is the client's problem to resolve gracefully.

## Damascene's deliberate architecture: the WSI owns the surface tag

Per the protocol a `wl_surface` has exactly **one** color-management owner. For
an accelerated client that owner is the **WSI (Mesa)**, which tags the swapchain
from the color space implied by the swapchain format. Damascene shares winit/Mesa's
libwayland connection, so a second `get_surface` (to attach our own description)
raises a *connection-fatal* `surface_exists` error and crashes the app (observed
on KDE+HDR). Therefore:

**Damascene never attaches its own image description. It "describes its content" by
choosing the swapchain format**, and Mesa attaches the matching description.
Concretely (wgpu 29, Vulkan backend): selecting `Rgba16Float` makes Mesa tag the
surface `EXTENDED_SRGB_LINEAR_EXT` (scRGB) via `create_windows_scrgb`; any 8-bit
format tags `SRGB_NONLINEAR`. This is the correct, crash-safe architecture for an
accelerated client — not a limitation to design around.

Reading is always safe (`get_surface_feedback` has no exclusivity rule), so Damascene
keeps a **read-only** driver (`damascene-winit-wgpu/src/wayland_color.rs`) that binds
the manager, reads capabilities, and reads the preferred description — for the
Color Management showcase and to gate HDR output.

## What Damascene does correctly today

- Reads compositor capabilities + the surface's preferred description
  (read-only, no attach).
- **Honors the app's `ColorPreferences`** via a constrained negotiation
  (`negotiate_output` / `deliver_space`): walks the app's preference ladder and
  picks the first space in `app preferences ∩ compositor capabilities ∩
  wgpu-deliverable`. HDR is opt-in — a default `sdr_only` app stays on the 8-bit
  sRGB swapchain; an app that asks for scRGB (`hdr_extended`/`hdr_broad`) gets an
  `Rgba16Float` scRGB swapchain on a genuinely HDR output
  (`CompositorColorTargets::indicates_hdr()`), so `>1.0` reaches the display.
  Compositor-advertised but wgpu-undeliverable spaces (PQ, BT.2020, Display-P3)
  are skipped rather than over-promised.
- Composites in `SRGB_LINEAR`. Correct for the scRGB path: scRGB shares
  sRGB/BT.709 primaries, so the working space is unchanged whether the swapchain
  is 8-bit sRGB (HW encodes) or fp16 extended-linear (verbatim) — only encode +
  dynamic range differ, not gamut.
- **Scales UI white to the scRGB reference level** (`white_scale`, 2026-06).
  Windows-scRGB encodes signal 1.0 = 80 cd/m² *absolute* — that is the
  encoding scale, not the reference white. Per `create_windows_scrgb`, the
  encoding's reference white is unknown and "should be assumed R=G=B=2.5375
  corresponding to 203 cd/m² (BT.2408)" for compositor processing. So on an
  `Rgba16Float` swapchain Damascene multiplies its final output by
  `WINDOWS_SCRGB_WHITE_SCALE` (203/80) via `FrameUniforms.white_scale`
  (every stock fragment shader applies it; custom shaders follow the
  contract in docs/SHADER_VISION.md — authored light scales, backdrop
  samples don't). Without this, UI white displays at 80 nits while
  anchored SDR apps sit at the output reference — ~2.5× dim. An earlier
  iteration concluded no client scale was needed by misreading 80 as the
  declared reference white; the spec text says otherwise.
  *(Damascene does NOT scale further to the compositor's configured
  reference — mapping the assumed 203-nit level onto the output reference
  is compositor-side anchoring, prism's job.)*
- **Color-managed images.** `Image` carries a `PixelFormat` (RGBA 8/16-bit
  unorm, f16, f32) and a `ColorSpace` tag (like an ICC-tagged image in a
  browser; plain `from_rgba8` keeps the web's untagged-is-sRGB convention).
  8-bit sRGB art uploads as-is (HW sRGB decode); every other source
  normalizes once on the CPU (`Image::to_scrgb_f16`) to scRGB f16 — linear
  sRGB primaries, extended range — and uploads to a float texture on all
  three GPU backends. Wide-gamut pixels land outside `[0,1]` and HDR brights
  above `1.0`, so on an scRGB swapchain they present losslessly; on SDR
  out-of-gamut chroma clips at the target while over-bright luminance rolls
  off via the remaster (below). The luminance contract: **a pixel at the
  source's reference white displays at the output's reference white.**
  Relative transfers encode that already; PQ is absolute (signal `1.0 =
  10000` nits), so `to_scrgb_f16` anchors it by the tagged
  `ColorSpace::reference_luminance_nits` (203 for `BT2020_PQ`, per BT.2408
  and the protocol's PQ default — override the field for differently graded
  masters). Everything brighter than reference is HDR headroom the remaster
  grades into the panel volume. HLG is scene-referred and still decodes
  without an OOTF or anchor (open, see gap (2)). `Color` conversion stays
  encoding-literal — the anchor is image-pipeline behavior. The showcase's
  Color Management page renders tagged-vs-untagged hue sweeps and a 0→4×
  luminance ramp as a live end-to-end check.
- **Reacts to `preferred_changed2` — HDR follows the window** (2026-06).
  The wayland driver is live: `WaylandColorManager` keeps its event queue
  + feedback object for the surface's lifetime, and the host polls it once
  per loop wake (non-blocking `dispatch_pending`; winit reads the shared
  socket). When the compositor changes the surface's preferred description
  (output move, HDR toggle) the host re-reads it, refreshes
  `HostDiagnostics`, and re-runs the same negotiation ladder as startup.
  A format flip (SDR ↔ HDR) reconfigures the swapchain and calls
  `Runner::set_target_format`, which rebuilds only the format-bound
  pipelines in place — interaction state, glyph/icon atlases, image and
  surface caches, and scene targets all survive, so the switch costs one
  cheap frame, not a restart.
- **Remasters HDR images into the output's luminance volume** (2026-06).
  "Tonemap within your declared volume" is the client's half of the
  protocol's division of labor, and for over-bright content Damascene is
  the only layer holding both sides: the content (backends measure each
  image's peak — its effective MaxCLL — for free during the
  `to_scrgb_f16` upload, cached per texture) and the target (the host
  derives `headroom = target_max / reference` from the preferred
  description and feeds `Runner::set_output_luminance`, re-deriving on
  every `preferred_changed2`). Policy is per-element and mirrors CSS
  `dynamic-range-limit` (`El::dynamic_range_limit`): `NoLimit` (default)
  resolves to the panel's full headroom, `ConstrainedHigh` caps HDR
  brights at 2× reference for grid/feed contexts, `Standard` tonemaps to
  SDR. When an image's measured peak exceeds its resolved limit the
  image shader applies a hue-preserving BT.2390 EETF (knee +
  Hermite roll-off in PQ space, on maxRGB) at sample time; content that
  fits — all ordinary SDR art — takes an early-out and pays nothing. On
  SDR swapchains headroom is 1.0, so HDR images now degrade gracefully
  (roll-off keeps highlight gradation) instead of hard-clipping. When an
  HDR output declares no `target_max_luminance`, there is nothing to
  remaster against and content passes through unchanged. Apps should
  not hand-roll tonemapping curves; the policy knob is the API. The
  compositor's own mapping stays the generic backstop for arbitrary
  clients — Damascene fitting its content into the advertised volume is
  what keeps that mapping ~identity (no double compression).

## Gaps — the correct behaviors still to build

Prioritized. None require a compositor we don't have. (The "wire
`ColorPreferences` into the host", "react to `preferred_changed2`",
"tonemap HDR images within the panel's volume", and "PQ
reference-luminance rescale" gaps are **done** — see "What Damascene does
correctly today.")

2. **The authored-HDR contract.** Image content is remastered and PQ
   sources anchor correctly (see above), but apps and custom shaders
   that *author* HDR light have no stated contract or helpers — they
   can still emit `>headroom` values the compositor must rescue. The
   exposure side is built: shaders read the output volume from
   `FrameUniforms.headroom`/`ref_nits` (contract sketch in
   docs/SHADER_VISION.md), and apps read `CompositorColorTargets`
   (reference white, peak) from `HostDiagnostics`, refreshed live on
   `preferred_changed2`. What remains is the authoring-side story:
   place highlights relative to reference white / headroom rather than
   absolute guesses, plus possibly a stock helper for keeping authored
   light inside the volume. HLG's anchoring (OOTF) also lands here —
   today HLG decodes scene-referred with no anchor.

## What is explicitly *not* Damascene's job

- **White-point leveling / lifting un-negotiated sRGB into a panel-appropriate
  region** — compositor (anchoring).
- **Tonemapping/gamut-mapping algorithm choice** — compositor (Damascene only states
  intent).
- **Driving PQ/BT.2020/Display-P3 swapchains** — blocked at wgpu (its Vulkan
  backend maps only the scRGB pair). Not an Damascene bug; revisit if wgpu gains
  more surface color spaces or we drop to raw Vulkan WSI.
