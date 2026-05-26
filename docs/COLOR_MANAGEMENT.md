# Color Management: Aetna's Client Half

## What this is

Aetna is a Wayland **client**. Color management is a negotiation between client
and compositor defined by `wp_color_management_v1` (the staging
`color-management-v1.xml`). This doc fixes the division of labor that protocol
defines, records what Aetna does today, and sketches the behaviors that would
make Aetna's half *correct* — including ones no compositor we run on supports
yet, so we build toward the right target rather than toward "whatever makes our
own compositor agree."

> We develop both Aetna and the `prism` compositor. That makes "it works because
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

**Client (Aetna) owns:**
- **Describing its content** — the color volume + luminance frame it is
  sending.
- **Render intent** — the ICC mapping policy (perceptual for UI).
- **Tonemapping within its declared volume** — if content exceeds what it
  declared, that is the client's problem to resolve gracefully.

## Aetna's deliberate architecture: the WSI owns the surface tag

Per the protocol a `wl_surface` has exactly **one** color-management owner. For
an accelerated client that owner is the **WSI (Mesa)**, which tags the swapchain
from the color space implied by the swapchain format. Aetna shares winit/Mesa's
libwayland connection, so a second `get_surface` (to attach our own description)
raises a *connection-fatal* `surface_exists` error and crashes the app (observed
on KDE+HDR). Therefore:

**Aetna never attaches its own image description. It "describes its content" by
choosing the swapchain format**, and Mesa attaches the matching description.
Concretely (wgpu 29, Vulkan backend): selecting `Rgba16Float` makes Mesa tag the
surface `EXTENDED_SRGB_LINEAR_EXT` (scRGB) via `create_windows_scrgb`; any 8-bit
format tags `SRGB_NONLINEAR`. This is the correct, crash-safe architecture for an
accelerated client — not a limitation to design around.

Reading is always safe (`get_surface_feedback` has no exclusivity rule), so Aetna
keeps a **read-only** driver (`aetna-winit-wgpu/src/wayland_color.rs`) that binds
the manager, reads capabilities, and reads the preferred description — for the
Color Management showcase and to gate HDR output.

## What Aetna does correctly today

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
- Does **not** scale for white point — correct, because anchoring is the
  compositor's job and Aetna's scRGB surface declares `reference_lum=80`, the
  same as an undescribed SDR app, so the compositor levels them together.

## Gaps — the correct behaviors still to build

Prioritized. None require a compositor we don't have. (1) is the substantive
remaining one. (The "wire `ColorPreferences` into the host" gap is **done** —
see `negotiate_output` / `deliver_space` under "What Aetna does correctly
today.")

1. **React to `preferred_changed2`.** The driver reads the preferred description
   once at setup and drops the connection. The protocol model is dynamic: the
   preferred changes as the surface moves between outputs or HDR settings change.
   Correct behavior: keep the feedback object alive, listen for
   `preferred_changed2`, and re-negotiate (reconfigure the surface to/from
   `Rgba16Float`) when the output's HDR status changes. This is the difference
   between "HDR on the output we launched on" and "HDR that follows the window."

2. **Tonemap Aetna-authored HDR within the panel's volume.** Once Aetna emits
   `>1.0` content, it should clamp/roll off to the output's
   `target_max_luminance_nits` (the driver already reads it) rather than dumping
   arbitrary values the compositor must rescue. Moot while all UI content is SDR
   `[0,1]`, but it's the client's responsibility per spec.

3. **Expose the luminance frame to apps for authoring.** `HostDiagnostics`
   already surfaces `CompositorColorTargets` (reference white, peak). Apps that
   author HDR content should place highlights relative to the reference white /
   peak, not to absolute guesses.

## What is explicitly *not* Aetna's job

- **White-point leveling / lifting un-negotiated sRGB into a panel-appropriate
  region** — compositor (anchoring).
- **Tonemapping/gamut-mapping algorithm choice** — compositor (Aetna only states
  intent).
- **Driving PQ/BT.2020/Display-P3 swapchains** — blocked at wgpu (its Vulkan
  backend maps only the scRGB pair). Not an Aetna bug; revisit if wgpu gains
  more surface color spaces or we drop to raw Vulkan WSI.
