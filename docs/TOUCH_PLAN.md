# Touch & Multi-Touch Plan

> **Status (2026-08-15): rulings ratified, implementation starting.** This is
> the plan of record for the multi-touch gesture arc plus the batch of touch
> affordances the 2026-08 review found neglected. It resolves the one item
> `docs/MOBILE_VISION.md` left deferred ("multi-touch gestures — the gesture
> grammar is its own design"): this document is that design. Read
> MOBILE_VISION.md first for the modality model this builds on; where this doc
> speaks in the future tense for shipped slices, the code supersedes it.

## Why now (the findings that triggered this arc)

A field report ("double-tap to reset plot zoom doesn't work at all on touch")
led to a full review of plots and then all of core on touch. The failure
inventory, condensed — every claim was verified against code:

1. **Double-tap never fires on touch.** Click counting uses
   `MULTI_CLICK_DIST = 4.0` logical px regardless of `PointerKind`
   (`state/click.rs`, `state/types.rs`). Finger taps scatter 10–40 px between
   contacts (Android's platform double-tap slop is 100 dp), so the second tap
   virtually always resets the count to 1.
2. **A sloppy tap zooms a plot into a ~10 px data window.** Plot presses
   bypass the touch gesture machine entirely (`pointer_down` returns before
   `TouchGestureState::Pending` is armed), so plots never get the 10 px
   tap-vs-drag grace. On the default `ZoomDrag` scheme, a press instantly
   begins a box-zoom and `MIN_ZOOM_PX = 4.0` commits any 4 px drift on
   release. Combined with (1) there is no escape.
3. **Half of every plot control scheme is Shift- or wheel-gated** — neither
   exists on touch. `ZoomDrag` touch can never pan; `PanDrag` can never zoom.
4. **A second finger corrupts any live gesture.** Hosts forward every finger
   through the single-pointer entry points; core drops `Pointer.id` at every
   entry (`runtime.rs`), so finger 2's down re-runs the whole `pointer_down`
   cascade, clobbers `pressed`, re-arms the gesture machine, and interleaved
   moves jerk whatever capture is live. There is no pinch anywhere.
5. **An embedded plot eats page scroll** (returns before the machine that
   gives viewports the #126/#130 yield-to-scroll treatment).
6. **Tooltips are categorically unreachable on touch** (`synthesize_tooltip`
   bails while `pressed` is set; touch clears hover on lift) — icon-only
   buttons have no visible label for touch users.
7. Assorted: the 14 px scrollbar track captures finger presses meant to
   scroll; `.user_resizable()` bands are 8 px, invisible, un-inflated; touch
   `pointer_up` never clears `pointer_pos` (pins scrollbar "active" styling);
   text drag-selection requires a 500 ms long-press first; double-tap word
   select shares bug (1).

Load-bearing good news: `Pointer` already carries `id: PointerId` +
`PointerKind` on every event from both real hosts (winit-wgpu forwards every
finger with its id; damascene-web passes the browser's `pointerId` and sets
`touch-action: none`), and the zoom math mostly exists —
`PlotView::zoom_about` is per-axis (its doc already says "wheel/pinch"),
`ViewportView::zoom_about` is uniform-scalar, `CameraState::zoom_by` is an
un-anchored dolly. Recognition is the missing piece, not plumbing or math.

## Settled rulings (do not re-litigate silently)

1. **Primary-contact routing, DOM `isPrimary` semantics.** Core keeps an
   arrival-ordered registry of live touch contacts on `UiState`. The
   first-arrived contact is *primary* and flows through the existing
   single-pointer pipeline unchanged; secondary contacts only update the
   registry and feed the pinch recognizer — they never touch
   press/hover/click/capture state. Hosts need no changes; a custom
   single-pointer host that passes `PointerId::PRIMARY` for everything
   degrades exactly to today's behavior.
2. **Pinch is recognized in core from raw contacts.** REJECTED: platform
   gesture recognizers as the primary path — winit 0.30 delivers
   `PinchGesture` only on macOS/iOS; Android, X11, Wayland, Windows, and web
   get nothing, so a core recognizer is mandatory anyway. Platform gesture
   events become *adjuncts* feeding the same zoom entry (see slice 6).
3. **On touch, the plot drag verb is always pan.** `PlotControls`
   (`ZoomDrag`/`PanDrag`) describes mouse/pen semantics only. Box-zoom is
   unreachable from touch: zoom belongs to pinch, reset to double-tap.
   REJECTED: touch box-zoom behind a bigger commit threshold — still
   trap-prone, still modifier-gated on the other half, and redundant next to
   pinch.
4. **Embedded plots yield vertical swipes to page scroll.** On
   threshold-cross over a plot: horizontal-dominant → plot pan;
   vertical-dominant → the scroll chain gets first refusal, plot pans only if
   nothing scrolls. Mirrors the #130 axis-dominance gate on the viewport
   takeover path. Full-screen plot apps lose nothing (no scrollable → plot
   pans on both axes).
5. **Tap places the crosshair.** A clean touch tap on a plot's data rect
   positions the crosshair/readout at the tap point and it lingers after
   lift (the Grafana-mobile readout convention). This is what a tap "does" on
   a plot; it never zooms.
6. **Double-tap = reset, with kind-aware click windows.**
   `next_click_count` reads `UiState::pointer_kind`: 4 px slop for
   mouse/pen, 48 px for touch (`same_target` gating already prevents
   cross-target misfires). Any touch gesture commit (scroll/pan/pinch)
   clears `click.last`, the same discipline the mouse latent-pan conversion
   already applies — mandatory once the window widens, or flick-then-tap
   reads as double-tap.
7. **Pinch semantics per surface.** Plot: per-axis separation ratios →
   `PlotView::zoom_about` + centroid pan, with an axis-lock guard when the
   fingers are nearly aligned on an axis (small separation would amplify
   noise into wild zooms). Viewport: uniform distance ratio →
   `ViewportView::zoom_about` about the centroid + centroid pan (viewport
   zoom is a single scalar by design — not revisited here). 3D camera:
   two-finger drag = pan, separation ratio = dolly (`zoom_by`); one finger
   stays orbit — this also makes the `Blender`/`OnShape`/`Maya` schemes
   (today entirely inert on touch) navigable. When one finger lifts, the
   survivor re-anchors as a plain single-finger pan; a pinch never
   synthesizes `Click`; a third finger is ignored; pinch marks
   `x_manual`/`taken_over` exactly as the wheel paths do.
8. **Long-press synthesizes the tooltip** for targets that carry one
   (standard Android affordance). The `LongPress` event still fires for
   apps; the tooltip is additive.
9. **Scrollbar track capture gates to mouse/pen.** A finger near the track
   scrolls content; it never thumb-drags or click-pages.
10. **`.user_resizable()` seam bands widen for touch** (the band is not an
    `El`, so it gets no automatic 44 px inflation). The `resize_handle`
    widget already handles touch correctly and stays the recommended shape.

## Architecture notes

- **Registry.** `UiState` holds a small arrival-ordered `Vec` of
  `(PointerId, position, down_at)` for live `PointerKind::Touch` contacts,
  maintained at the very top of `pointer_down` / `pointer_moved` /
  `pointer_up`; `pointer_cancelled` clears it (platform touch-cancel is
  all-contacts on Android, and the web host already queues a global cancel).
  A down for an id already present replaces it (defensive against host
  echo). Mouse/pen never enter the registry.
- **Pinch state** is a new `TouchGestureState::Pinch { .. }` variant — it is
  mutually exclusive with `Pending`/`Scrolling`/`LongPressed` exactly the
  way the enum already models, and `pointer_up`'s cleanup already funnels
  through the enum. Surface resolution at second-finger-down: a live
  zoomable capture wins (plot pan/zoom — a zoom band is discarded unapplied;
  viewport pan; camera drag), else the camera → viewport → plot ladder that
  `pointer_wheel` uses, with the same #127 occlusion guards. A primary
  contact captured by a `consumes_touch_drag` widget (slider, scrubber,
  resize handle) is never hijacked into a pinch. If nothing zoomable
  resolves, the second finger is inert.
- **Incremental updates.** Pinch applies previous→current ratios per move,
  not baseline→current — stable under drift and under the survivor handoff.
- **Host id hygiene.** winit-wgpu currently does `touch.id as u32`; on iOS
  the id is a `UITouch` address, so the host maps ids to small stable slots
  instead of truncating. Web already passes real `pointerId`s.
- **What does not change.** Mouse/pen paths, the event vocabulary, the
  `App::on_event` contract, custom single-pointer hosts, and the
  `consumes_touch_drag` opt-in all keep their exact semantics.

## Slices

1. **Contact registry + primary-contact routing** — pure infrastructure;
   fixes second-finger corruption; two-contact headless tests (none exist
   today). Includes the winit-wgpu id-slot fix.
2. **Kind-aware multi-click + commit-clears-click** — fixes the reported
   double-tap bug and text double-tap word-select.
3. **Plot touch routing** — plots arm `Pending` on touch; dominance rule;
   tap-crosshair; no touch box-zoom.
4. **Pinch recognizer** — plot per-axis, viewport uniform, camera
   pan+dolly.
5. **Touch affordance batch** — long-press tooltip, scrollbar gate, resize
   band widening, stale-`pointer_pos` fix.
6. **Platform pinch adjuncts** — a `pinch_zoom(anchor, factor)` core entry
   on the wheel ladder; macOS `WindowEvent::PinchGesture` (the *only* pinch
   source there — macOS emits no `Touch` events); web ctrl+wheel (the
   browser's trackpad-pinch encoding) routed into it, with the
   `preventDefault` question resolved at implementation.
7. **Docs + device verification** — MOBILE_VISION refresh; Android phone +
   phone-browser pass over the showcase at the slice-3 and slice-4
   milestones.

## Deferred (recorded, not in this arc)

- **Text selection handles** — its own arc; touch selection currently rides
  long-press-then-drag.
- **Touch-reachable clipboard / context-menu ruling** — core has no
  copy/paste path without a keyboard; apps can wire `LongPress` →
  `context_menu(...)` today, but whether core should ship a default needs
  its own discussion.
- **Momentum on plot/viewport pans** (scroll fling exists; pan fling
  doesn't).
- **IME preedit / composition** — already on the MOBILE_VISION deferred
  list.
- **Mobile hit-target lint profile** — tracked in
  `docs/ACCESSIBILITY_PLAN.md`; the sweep's nested-target findings
  (`numeric_input` steppers, `editor_tabs` close buttons, exact-glyph link
  rects) belong to it.
- **Simultaneous mouse + touch coexistence** — both modalities still share
  single hover/press state; the registry neither fixes nor worsens it.
