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
   hardest single piece and gets its own arc. — *Discharged: the text
   arc SHIPPED 2026-08-12, see the status section.*
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

Status 2026-08-11: Arc 1 SHIPPED (commit e23bf46). Arc 2 SHIPPED
(commits 79edab4 semantic layer, d6be9d3 widget sweep, 2e2e1a4
lowering + winit adapter); verified live on this machine's AT-SPI bus
(showcase registers, serves the tree, names come through — Orca
read-aloud still worth a manual pass). Arc 2b (lints) SHIPPED same
day: four `FindingKind`s (`NoAccessibleName`, `ImageWithoutAlt`,
`LowContrastText`, `SmallHitTarget`), `lint()` takes the active
`&Theme` so contrast measures palette-resolved composited color in
the renderer's linear working space, tooltip promoted to fallback
accessible name (HTML `title` semantics), implicit label association
in `form_item`/`field_row`, placeholder-as-fallback-name on
`text_input`, and the known icon-only gaps labeled (editor-tabs "+",
calendar month arrows). The showcase sweep's one systemic real
finding — status tokens (`INFO`/`SUCCESS`/`WARNING`/`DESTRUCTIVE`)
used as *text* in the tinted/text-only style profiles failed AA in
both palettes (dark destructive text: 1.73:1, nearly invisible; light
warning: ~1.6:1) — was RESOLVED 2026-08-12 by user ruling: the status
tokens stay fill-grade, and each gains a text-grade counterpart
(`*_TINT_FOREGROUND`; tailwind-400 tones in damascene dark, -700/-800
in light, radix step-11-derived in the radix trios — radix light
needed a step darker than 11 because our 15%-alpha tints are paler
than radix step-3). `tint()`'s Tinted/Surface/TextOnly profiles, the
badge default, the markdown syntax highlighter, and `form_message`
consume them; solid fills keep the base tokens. Values are pinned by
`tinted_status_text_meets_aa_in_every_palette` (all 8 themes, page +
card surfaces), the CI lint gate and hero test run unfiltered again,
and the showcase reports zero findings of any kind. Validation
renders: `tools/src/bin/render_tint_validation.rs`. Arc 3
(announcements) SHIPPED 2026-08-12:
`App::drain_announcements` → `RunnerCore::push_announcements` (all
hosts + backend runners, vulkano-demo parity-pinned), runtime
synthesizes an invisible `Kind::Custom("announcements")` live-region
layer (zero-size nodes, message as accessible *name*, keyed by
monotonic id so every announcement is a fresh node to the adapter,
`Role::Status`/polite vs `Role::Alert`/assertive, 2s retention);
toasts already self-announce (Status+Polite card) and now *park*
instead of auto-expiring while `screen_reader_active` — explicit
dismissal only, without pinning the redraw loop. Live-verified on
this machine's AT-SPI bus: `examples --bin announce -- --auto` with
the org.a11y flags flipped emitted `object:announcement` signals
(politeness=1, message in the payload, distinct object path per
announcement); flags restored. Canonical app shape:
`examples/src/bin/announce.rs`. Arc 2 deferred details: web ARIA
mirror. "Scroll-offset-aware bounds" was RETIRED 2026-08-12 as a
false premise: layout bakes scroll offsets (and viewport pan/zoom,
and `layout_override` placement) into `computed_rect` before the
lowering walk runs, so emitted bounds were already
window-space-correct — pinned by `bounds_track_scroll_offsets`. The
real residual is paint-time `translate`/`scale` (animation
transients), which is cosmetic. vulkano/ash runners still lack
`accessibility` feature forwarders for the *tree* (announcement/toast
queues work everywhere; the AccessKit lowering is winit-wgpu only).

**Text-protocol arc (ruling 5's own arc) SHIPPED 2026-08-12** (commits
0666347 textbox-chrome suppression, 069799b scroll-limit retirement,
3262ed3 `character_metrics`, 735fd92 run synthesis, 9a97769 action
routing, plus the verification batch). Textboxes now speak AccessKit's
character-level text protocol end to end:

- **Declaration:** `El::editable_text(EditableText)` — rendered value
  (bullets for masked fields), `multiline` (lowers to
  `MultilineTextInput`), placeholder (platform property; empty fields
  report `value=""` instead of the old placeholder-as-value bug), and
  a display↔source byte map for masked fields. `text_input` /
  `text_area` stamp it; custom widgets use the same builder.
- **Geometry:** `text::metrics::character_metrics` — one cosmic pass
  per (text, style, width) yielding per-visual-line byte ranges (hard
  `\n` = trailing zero-width cluster; lines tile the source exactly)
  and per-grapheme-cluster length/position/width tables, LRU-cached.
  Runs re-shape from the value leaf's own style, so cluster edges sit
  exactly where the caret paints. RTL lines carry a direction
  override and omit positions (reading works, magnifier tracking
  degrades); >255-cluster lines chunk into `next_on_line`-chained
  runs (u8 wire indices).
- **State out:** the app's `Selection` lowers onto the runs as
  `ak::TextSelection` (source→display→run/character), only while the
  selection lives in the field. The AT-SPI adapter diffs value +
  selection per frame and synthesizes `text-changed:insert/delete`,
  `text-caret-moved`, `text-selection-changed` itself.
- **Actions in:** `SetTextSelection` resolves through the per-frame
  `TextRunTable` back to `(key, source byte)` and synthesizes the
  `SelectionChanged` event apps already fold (with a caret-activity
  bump); `ReplaceSelectedText` synthesizes `TextInput` (dormant on
  Linux — the AT-SPI adapter has no EditableText interface — live for
  Windows/macOS ATs). Word starts share `selection::is_word_char`
  with Ctrl+arrow, so AT word navigation lands where the keyboard
  does.
- **Verification:** unit tests (emoji/ZWJ clusters, masking, empty +
  placeholder, hard breaks, soft wrap, chunked long lines, byte↔
  position round trips) plus consumer-contract tests driving
  `accesskit_consumer` — the exact code inside the platform adapters,
  whose `unwrap()`s make the protocol invariants executable — over
  our emitted trees. Live-verified on this machine's AT-SPI bus:
  `examples --bin text_protocol -- --auto` with the org.a11y flags
  flipped emitted `object:text-changed:insert` (payload text, USV
  offsets), `object:text-caret-moved` (scripted offsets), and
  `object:text-selection-changed`; an injected
  `org.a11y.atspi.Text.SetCaretOffset(2)` over the bus returned true
  and the follow-up `CaretOffset` read-back reported 2 — the full
  action→event→fold→re-emit loop. Flags restored. Canonical app
  shape: `examples/src/bin/text_protocol.rs`.
- **Out of scope, unchanged:** `numeric_input` (SpinButton,
  `aria_value_text`), `input_otp` (no caret model — needs its own
  ruling), read-only static-text runs (the primitive is reusable),
  IME preedit reporting.

**Android arc SHIPPED 2026-08-12** (commits d9fd6c5 / 37caa47 /
6b6af70) — the mobile survey found the whole program compiled out on
Android plus two mobile-structural gaps:

- **Adapter (d9fd6c5):** accesskit_winit's Android backend is
  hard-wired to GameActivity (reads `mSurfaceView`, panics otherwise);
  ours is NativeActivity, and `damascene-android` also had the feature
  off via `default-features = false`. The host now drives
  `accesskit_android`'s view-generic `InjectingAdapter` directly
  against NativeActivity's content view (JNI via the crate's own
  `jni` 0.21 re-export — the host's direct `jni` is 0.22 and types
  don't mix). `embedded-dex` ships the Java delegate, no Gradle
  changes; JNI failure degrades to absent a11y with a log. No
  deactivation callback on Android: `active` latches until suspend
  drops the adapter. **Device-verified** (OnePlus 8 Pro, uiautomator
  interrogation): full semantic tree with named
  Spinners/ToggleButtons/tabs and text_input as EditText.
- **Scroll semantics (37caa47):** overflowing scroll viewports (scroll
  containers and virtual lists — both write `ScrollState.metrics`) are
  semantic even roleless: `ScrollView` role, `scroll_y/min/max`,
  vertical orientation, edge-aware ScrollUp/ScrollDown (sharing
  `scroll_by_id`'s dead zone), child-action ScrollIntoView. Fitting
  containers stay hoisted; a root-level `scroll(...)` scrolls through
  the Window node. Routing: paging = 90% viewport through the wheel
  path's clamped `scroll_by_id`; ScrollIntoView walks scrollable
  ancestors innermost-first with minimal displacement (the AT twin of
  `ScrollRequest::EnsureVisible`). TalkBack pages via
  SCROLL_FORWARD/BACKWARD (accesskit_android 0.7.5 has no
  ACTION_SHOW_ON_SCREEN mapping).
- **Preference sniffing (6b6af70):** at every `resumed()` Android
  reads animator_duration_scale==0 → `reduced_motion` (non-zero = an
  explicit no-preference), uiMode night mask → `color_scheme`,
  high_text_contrast_enabled → `contrast: More`. Env overrides layer
  over sniffed values via `AccessibilityPreferences::or`;
  `screen_reader_active` stays live from adapter activation. A
  dark-mode flip recreates the activity (uiMode not in configChanges),
  so resume-time reads cover it. Snapshot logged once per resume.
- **Still open on mobile:** on-device TalkBack swipe pass (scroll +
  prefs landed after the device locked — verify with an overflowing
  showcase page: `scrollable="true"`, class `android.widget.ListView`,
  and the settings log line); reflow text scale (fontScale/Dynamic
  Type) — the big deferred item; a mobile-guideline hit-target lint
  profile (24px WCAG floor vs 48dp Material / 44pt HIG); iOS rides
  accesskit_ios 0.1.2 unconditionally but is untested (no hardware).

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
- **Arc 2b (a11y lints, alongside Arc 2)** — DONE. Interactive node
  without accessible name (icon-only button missing `aria_label`,
  plus the focusable-but-`aria_hidden` variant), image without `alt`,
  WCAG AA contrast against the theme-resolved composited backdrop
  (skips disabled nodes, reduced-opacity subtrees, shader/image
  backdrops, and overlay layers until an opaque fill), hit target
  below `tokens::MIN_TARGET_SIZE` (24px) with the WCAG 2.5.8 spacing
  exception (isolated small targets are rescued by hit-test's
  `MIN_TOUCH_TARGET` inflation; scroll-axis clipping doesn't count as
  small). The lint loop is how LLM authors learn the rules — this is
  damascene's distinctive leverage.
- **Arc 3 (announcements)** — DONE. `App::drain_announcements() ->
  Vec<Announcement>` in the established drain pattern; core
  synthesizes a live-region node (toast-layer synthesis pattern);
  AccessKit/ARIA carry it. Toasts auto-announce; screen-reader-active
  suppresses toast auto-dismiss (park until explicit dismissal).
- **Text-protocol arc** — DONE 2026-08-12 (see the status section):
  `El::editable_text` declaration, `character_metrics` geometry,
  TextRun synthesis + selection reporting, SetTextSelection /
  ReplaceSelectedText routing, consumer-contract tests, live AT-SPI
  round-trip verification.
- **Android arc (adapter + scroll semantics + preference sniffing)** —
  DONE 2026-08-12 (see the status section): accesskit_android driven
  directly for the NativeActivity host, ScrollView
  roles/actions/ScrollIntoView routing, Android settings sniffed at
  resume.
- **Deferred, own design rounds:** reflow-aware text scale (must enter
  token/metrics resolution before layout — on mobile this is the
  fontScale / Dynamic Type item, the largest remaining mobile gap);
  web ARIA DOM mirror; IME preedit (composition state +
  `set_ime_cursor_area` want the same caret-rect primitive the text
  protocol now computes); Windows/macOS/iOS preference sniffing;
  forced-colors palette mapping; an `input_otp` caret/role ruling (it
  claims `Role::Textbox` with no caret model); a mobile hit-target
  lint profile (48dp Material / 44pt HIG vs the 24px WCAG floor).
  Keyboard gaps (table/plot/viewport/Scene3D) are independent of
  these arcs: issue #144.

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
