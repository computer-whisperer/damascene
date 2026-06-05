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
  above `1.0`, so on an scRGB swapchain they present losslessly; on SDR they
  gamut-clip at the target. PQ/HLG sources follow the same conventions as
  `Color` conversion (PQ `1.0 = 10000` nits, no reference-luminance rescale —
  revisit alongside gap (2)). The showcase's Color Management page renders
  tagged-vs-untagged hue sweeps and a 0→4× luminance ramp as a live
  end-to-end check.
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

## Gaps — the correct behaviors still to build

Prioritized. None require a compositor we don't have. (The "wire
`ColorPreferences` into the host" and "react to `preferred_changed2`" gaps
are **done** — see "What Damascene does correctly today.")

2. **Tonemap Damascene-authored HDR within the panel's volume.** Once Damascene emits
   `>1.0` content, it should clamp/roll off to the output's
   `target_max_luminance_nits` (the driver already reads it) rather than dumping
   arbitrary values the compositor must rescue. No longer moot: HDR-tagged
   images (`Image::from_rgba_f32_in` + friends) put `>1.0` pixels on the scRGB
   path today. The same pass is where PQ sources' reference-luminance rescale
   belongs (today PQ decodes `1.0 = 10000` nits with no rescale, matching
   `Color` conversion).

3. **Expose the luminance frame to apps for authoring.** `HostDiagnostics`
   already surfaces `CompositorColorTargets` (reference white, peak). Apps that
   author HDR content should place highlights relative to the reference white /
   peak, not to absolute guesses.

## What is explicitly *not* Damascene's job

- **White-point leveling / lifting un-negotiated sRGB into a panel-appropriate
  region** — compositor (anchoring).
- **Tonemapping/gamut-mapping algorithm choice** — compositor (Damascene only states
  intent).
- **Driving PQ/BT.2020/Display-P3 swapchains** — blocked at wgpu (its Vulkan
  backend maps only the scRGB pair). Not an Damascene bug; revisit if wgpu gains
  more surface color spaces or we drop to raw Vulkan WSI.
