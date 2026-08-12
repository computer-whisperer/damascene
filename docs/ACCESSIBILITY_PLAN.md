# Accessibility Plan

Status: ratified 2026-08-11 (user + Claude review session). This is the
cross-session handoff for the accessibility arcs. Survey evidence and
rationale live in the session that produced it; the rulings below are
frozen within each arc — reopen deliberately, not by drift.

## Rulings (frozen)

1. **AccessKit is the interchange format.** `damascene-core` gains a
   feature-gated dependency on the pure-data `accesskit` crate (0.24.x)
   and emits `accesskit::TreeUpdate`. Hosts feed platform adapters
   (`accesskit_winit` covers Windows/macOS/Linux/Android/iOS).
   REJECTED: a damascene-native interchange tree mapped per-host —
   duplicates a settled ecosystem vocabulary, N host mappings.
2. **Authoring vocabulary is damascene-native and unconditional.**
   Stock widgets must set semantic props without `cfg` gates, so `El`
   gets a native `Role` enum (ARIA role names) and `aria_*` builders
   that exist regardless of the feature. Only the emission module maps
   to `accesskit` types. This is a refinement of ruling 1, recorded so
   the two aren't read as contradicting.
3. **Naming echoes ARIA/HTML** (`aria_label`, `aria_description`,
   `alt`, `role`, checked/expanded/selected state setters) — maximum
   LLM-author familiarity, same rule as the rest of the library.
   `.label()` is unavailable anyway (taken by `TextRole`).
4. **Feature default-on in shipped hosts.** `damascene-winit-wgpu`
   enables core's `accessibility` feature by default; opt out via
   `default-features = false`. Runtime cost is zero until an AT
   connects (adapters activate lazily). REJECTED: opt-in — the
   ecosystem default would be inaccessible apps.
5. **Full text-editing AT support is deferred** (caret/selection/word
   navigation reporting via AccessKit's text protocol). V1 exposes
   text inputs with role, name, and current value only. It is the
   hardest single piece and gets its own arc.
6. **Web is a deferred target for the screen-reader arc.** The eventual
   shape is a hidden ARIA DOM mirror built from the same semantic tree
   (anchor: the proven hidden-`<input>` soft-keyboard pattern in
   `damascene-web`). The canvas's unconditional Tab-`preventDefault`
   gets reconciled then. Web *does* participate in the preferences arc
   now (`matchMedia` is cheap).
7. **Reduced motion is orthogonal to `AnimationMode`**, not a third
   variant: `Settled` also freezes shader `frame.time` and caret
   blink, which reduced-motion must not. Reduced motion settles
   movement-shaped props (scale/translate) and skips movement enter
   seeds while keeping opacity/color fades, caret blink, spinners, and
   shader time alive.
8. **Core auto-applies preferences to library-owned behavior** (motion
   policy; later toast timeouts once `screen_reader_active` exists);
   apps read the rest from `BuildCx` (theme-shaped preferences like
   color scheme / contrast are app decisions).
9. **Native preference sniffing lands Linux-first**, but *after* the
   AccessKit arc: the XDG settings portal needs zbus, which arrives in
   the tree anyway via `accesskit_unix` — don't pay the dependency
   twice. Until then: env-var overrides in the winit host
   (`DAMASCENE_REDUCED_MOTION` etc.) and real detection on web.
   Windows (`UISettings`) / macOS (`NSWorkspace`) follow-ups after.

## Arc sequence

- **Arc 1 (preferences + motion)** — DONE when marked below.
  `AccessibilityPreferences` on `UiState` (host-pushed, CSS
  `prefers-*` family: `reduced_motion`, `color_scheme`, `contrast`,
  `reduced_transparency`; all `Option`, `None` = host doesn't know /
  no preference). `RunnerCore::set_accessibility_preferences` +
  forwarding on all backend runners; surfaced to apps via
  `BuildCx`/`EventCx`. Reduced-motion policy in the animation tick.
  Env overrides in `damascene-winit-wgpu`; `matchMedia` + change
  listeners in `damascene-web`.
- **Arc 2 (semantic layer + AccessKit)** — `Option<Box<A11yProps>>` on
  `El` (one pointer — respect the El struct diet) holding `role`,
  `aria_label`, `aria_description`, `alt`, state (checked / expanded /
  selected / pressed / value), live politeness, `aria_hidden`. Stock
  widgets self-annotate through the public builders (symmetry
  invariant: no dispatch on `Kind`, user widgets get the same power).
  Accessible name defaults to the node's visible `text` (accname
  style); `aria_label` overrides. Emission: a sibling of
  `bundle/inspect.rs::dump_tree` walking the laid-out tree into
  `accesskit::TreeUpdate` (`computed_id` → interned `NodeId`), diffed,
  built only while the host reports AT active. Action input:
  `accesskit::ActionRequest` → existing machinery (focus requests,
  `Activate`, `ScrollRequest`). Host wiring: `accesskit_winit` in
  `damascene-winit-wgpu`, default-on. `screen_reader_active` joins the
  preferences/diagnostics surface here. Verification: Orca on this
  machine; unit tests against the `TreeUpdate` (`accesskit_consumer`).
- **Arc 2b (a11y lints, alongside Arc 2)** — interactive node without
  accessible name (icon-only button missing `aria_label`), image
  without `alt`, WCAG contrast ratio between resolved theme tokens
  (Oklab machinery exists), hit target below 24px. The lint loop is
  how LLM authors learn the rules — this is damascene's distinctive
  leverage.
- **Arc 3 (announcements)** — `App::drain_announcements() ->
  Vec<Announcement>` in the established drain pattern; core
  synthesizes a live-region node (toast-layer synthesis pattern);
  AccessKit/ARIA carry it. Toasts auto-announce; screen-reader-active
  suppresses toast auto-dismiss.
- **Deferred, own design rounds:** reflow-aware text scale (must enter
  token/metrics resolution before layout); web ARIA DOM mirror; text
  editing AT protocol; Windows/macOS preference sniffing; forced-colors
  palette mapping. Keyboard gaps (table/plot/viewport/Scene3D) are
  independent of these arcs: issue #144.

## Existing machinery each arc builds on

- `computed_id` (stable, hierarchical, `DuplicateId`-linted) → node ids.
- `focus.rs` / `state/focus.rs` — complete HTML-faithful focus model.
- `UiEventKind::Activate` / `is_click_or_activate` — keyboard
  activation contract, already honored by every stock widget.
- `AnimationMode` dispatch (`anim/tick.rs`) + closed `Timing` preset
  set — the motion-policy hook.
- `Theme`/`Palette` runtime swap — contrast/forced-colors delivery.
- `tokens::MIN_TOUCH_TARGET` + hit-rect inflation — target-size
  precedent.
- `dump_tree` — the semantic tree-walk shape emission mirrors.
- Hidden soft-keyboard `<input>` in `damascene-web` — the DOM-mirror
  anchor for the deferred web arc.
