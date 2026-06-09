# Damascene Mobile Vision

This is the maintainer-facing architecture note for Damascene on small viewports
and touch input. It covers what already works, what is missing, and the
short-term work needed to make a phone web browser a first-class target.
Public author guidance belongs in crate READMEs and rustdoc once a surface is
stable enough to document.

## Goal

A user opening an Damascene app in a phone web browser should get a real touch
experience: taps land on the right targets, scrolling has momentum, content
fits the viewport, the on-screen keyboard does not strand a focused input,
and stock widgets do not depend on a hovering pointer to behave. The same
input model should later extend to a native Android host without redesigning
core abstractions.

## Current Thesis

Damascene's interaction model is already pointer-generic at the core. The
`UiEvent` and `PointerButton` surfaces are named for any pointing device, not
for a mouse. The work required to support touch is mostly in three places:

- **Backend ingest**, where browser and OS events are translated into
  `pointer_*` runner calls.
- **Hover-equivalent visual state**, where stock widget animations assume a
  pointer that can rest over a target without committing to a press.
- **Layout response to the viewport**, where current size primitives assume
  a roughly desktop-shaped window.

This shape lets us add touch without forking the widget kit or introducing a
parallel "mobile" tree. The HTML platform already worked through the same
problem with `PointerEvent`; Damascene should follow that paradigm rather than
invent a new one.

## What Is Already Flexible Enough

- **Pointer-generic event vocabulary.** `UiEvent` exposes `PointerDown`,
  `PointerUp`, `Drag`, `PointerEnter`, `PointerLeave`, `Click`, plus
  modifier-aware variants. `PointerButton` documents primary / secondary /
  middle as roles, not as mouse buttons. No core variant is mouse-named.
- **Backend split.** Adding a touch ingest path is a backend concern.
  Hosts call `Runner::pointer_down/up/moved/wheel`; core does not need to
  know what produced the event.
- **DPI plumbing.** `HostDiagnostics::scale_factor` is available at build
  time. Apps can already branch on density without backend changes.
- **`hit_overflow`.** Nodes can expand their pointer target without
  changing paint. This is the right primitive for enforcing minimum touch
  targets without reflowing layout.
- **Focus and IME.** Tab order and `Ime::Commit` already exist, so a soft
  keyboard has a place to deliver text into a focused widget.

These are load-bearing for the plan. None of them should be redesigned to
add touch.

## What Has Shipped (status as of 2026-06-09)

Most of the original "missing" list landed; the code supersedes the plan
sections below where they speak in the future tense.

1. **Touch ingest** — the web host binds DOM `PointerEvent` directly
   (mouse/touch/pen normalized by the browser), carrying per-pointer
   `PointerId`s and a `PointerKind::{Mouse, Touch, Pen}` modality tag on
   `UiEvent` (`event.rs`). A native Android host exists
   (`damascene-android`, NativeActivity wrapper) with insets, soft
   keyboard, clipboard bridge, and intent-routed links.
2. **Touch-aware interaction core** — a touch-gesture state machine in
   core (`TouchGestureState`) resolves tap / drag / scroll / long-press
   from raw pointer input; hover transitions are gated by contact so
   touch doesn't inherit mouse hover semantics; long-press synthesizes
   secondary-click and drives text selection.
3. **Scroll momentum** — touch scroll gets fling/friction
   (`state/types.rs`); mouse wheel stays instantaneous.
4. **Soft-keyboard awareness** — the web host tracks
   `visualViewport` keyboard insets and wires the soft keyboard for
   touch text input; Android does the equivalent natively.
5. **Viewport at build time** — `BuildCx` exposes the logical-pixel
   viewport, so apps can branch on size during build.
6. **Touch ergonomics in stock widgets** — touch density for menu
   popovers; widgets respond to press without needing a resting hover.

## What Is Still Missing

1. **Min/max sizing primitives.** `Size` is still `Fixed | Hug | Fill` —
   no `min_size`/`max_size` modifiers and no `breakpoint(...)` helper on
   top of the viewport query.
2. **Minimum hit-target enforcement.** Nothing prevents a button from
   shipping with a sub-44pt tap area on a dense display; the planned
   theme-declared floor (auto-`hit_overflow`) is not implemented.
3. **Multi-touch gestures.** No pinch, swipe, or two-finger pan. The
   pointer-id plumbing they need exists; the gesture grammar doesn't.
4. **Rich IME composition.** `Ime::Commit` is enough for soft keyboards;
   multi-stage composition, dead keys, and candidate windows remain open.

## Design Principles

- **Follow HTML where it has already solved the problem.** `PointerEvent`,
  viewport-relative sizing, and minimum hit-target conventions are
  load-bearing prior art. Damascene should prefer those shapes over
  framework-specific reinventions.
- **No parallel "mobile" widget kit.** The same stock widgets must work
  across desktop and touch. If a widget cannot, the hover/focus model is
  wrong, not the widget.
- **Touch is one input modality, not the only one.** A laptop with a
  touchscreen, a tablet with a Bluetooth keyboard, and a phone are all
  realistic targets. The model must handle simultaneous mouse + touch +
  keyboard, not assume one excludes the others.
- **Core stays backend-neutral.** Touch ingest belongs in backend crates.
  Core gains pointer-id and modality tagging at most, never DOM or OS
  types.

## Near-Term Priorities

Ordered by leverage. Each item should be small enough to land and validate
before the next begins.

### 1. Pointer-event ingest in `damascene-web` — **landed**

Bind DOM `PointerEvent` directly in the web host instead of routing pointer
input through winit's mouse-only translation. This unlocks:

- touch and pen input alongside mouse, normalized by the browser,
- per-pointer IDs, the foundation for everything multi-touch later,
- correct pressure and tilt fields when present,
- correct `pointerType` so core can later tag events as `mouse | touch | pen`.

Scope is narrow: replace the current mouse-event routing inside the web host
with a `PointerEvent`-based path, keep the existing `pointer_down/up/moved`
runner calls, and discard winit's pointer translation on web. Native hosts
are unaffected.

### 2. Modality tag on pointer events — **landed**

Add an enum tag (`PointerKind::{Mouse, Touch, Pen}`) carried on
`UiEvent::PointerDown/Up/Moved` and on `UiTarget` callbacks. Core does not
branch on it; widgets and animation can. This is the hook that lets the
hover-equivalent work in step 3 without making touch pretend to be a mouse.

### 3. Press-affinity animation companion to hover — **landed**

Today, hover state drives `SubtreeHoverAmount`. Add a press / contact-driven
animation source so touch input drives the same visual response that hover
drives on desktop. Buttons feel alive on tap-down, not only after a click
fires. This is intentionally a small extension to the existing animation
plumbing, not a new widget surface.

### 4. Viewport at build time + min/max sizing — **half-landed** (viewport query exists; min/max sizing does not)

Expose viewport size in `BuildCx` so widgets can branch on it. Add `min_size`
and `max_size` modifiers on `El`. Optionally add a `breakpoint(...)` helper
for the common "phone vs desktop" split. The goal is for a single `App` to
adapt without the host orchestrating layout choices.

### 5. Minimum hit-target via theme — open

Let the active theme declare a minimum interactive target (default 44pt or
similar). Interactive nodes whose paint rect is smaller automatically gain
`hit_overflow` to satisfy the minimum, without changing what is drawn. Opt
out per node when needed.

### 6. Scroll momentum — **landed**

Add fling/momentum to scroll regions when input arrives from a touch
modality. Wheel input from a mouse continues to be instantaneous. This is
local to scroll machinery and should not change the public scroll API.

## Deferred

These matter eventually but should not block the items above:

- **Multi-touch gestures.** Pinch-to-zoom, two-finger pan, rotation. Pointer
  IDs from step 1 are the prerequisite; the gesture grammar itself is its
  own design. (Still deferred — the only item on this list that hasn't
  shipped.)
- **Native Android host.** **Landed** — `damascene-android` wraps the
  winit + wgpu host in a NativeActivity shell with insets, soft keyboard,
  clipboard, and intent-routed links; `damascene-android-showcase` is the
  runnable entry.
- **IME composition.** Multi-stage composition, dead keys, candidate
  windows. The current `Ime::Commit` path is enough to unblock soft
  keyboards on phones; richer composition is a separate effort.
- **Soft-keyboard viewport awareness.** **Landed** — the web host tracks
  `visualViewport` keyboard insets; Android handles insets natively.

## Non-Goals

- A separate mobile widget kit, theme, or layout DSL.
- Reactive layout that recomputes on every scroll or animation frame
  beyond what the existing build cycle already does.
- A gesture recognizer framework before stock widgets can use it.
- Pretending touch is a mouse. The point of pointer-id and modality is to
  let widgets respond correctly to each modality, not to flatten them.

## Open Questions

- Should `PointerEnter` / `PointerLeave` fire on touch at all, or only on
  pointers whose modality is `Mouse | Pen`? Firing them on `PointerDown`
  for touch keeps tooltips trivially reachable but changes what "hover"
  means in app code.
- Is the right viewport API a value on `BuildCx`, an `Env`-style ambient
  context, or a typed `viewport()` helper? All three are workable; the
  choice affects how widgets compose.
- Should the minimum hit-target floor come from the theme, the host
  (because it knows the input device class), or both?
- For scroll momentum, should the velocity model live in `damascene-core` so
  it is consistent across backends, or in the host that owns the input
  cadence?

These should be resolved as the corresponding priority is implemented, not
in advance.
