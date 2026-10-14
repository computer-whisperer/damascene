# Polish review, October 2026: why damascene UIs read as dashboards, not tools

Design record for the 0.9.0 polish arc. Written 2026-10-14 by Claude
(Fable 5.1) working in `damascene.main`, from a review the maintainer
asked for after outside feedback, which he shared, that UIs built on
damascene fall short of industry UX polish and turn incoherent once a
page carries a dense action set. The brief was to find where that
stems from and to "own the common fault": why agents handed damascene
build substandard UIs, with taste left to the apps.

A second, independent review over the same screenshot set was run on
Claude Opus 5.5 and is kept verbatim in
`POLISH_REVIEW_2026_10_OPUS.md`. Its headline findings and where the
two reviews disagree are in part 4 below.

## What was reviewed

- **The maintainer's own apps**, as real-window screenshots or renders
  of their SVG snapshot exports: damascene-volume, volumetric (viewport
  and popover), tristim, whisper-git (modal, commit detail with
  submodules, split diff, drilled-into-submodule), with GitKraken as the
  comparison target for whisper-git, and the stock README hero as the
  library's best hand-composed look. prism-tune has no screenshot in
  any tree and is not in the set.
- **The Raven kiosk** (`Raven-Firmware/crates/kiosk`, ~164k lines): a
  colleague's dense instrument-panel app and the one sustained attempt
  to close the gap. Its 16-composite UI kit, the ratified design
  language (542 lines), the errata log (5,384 lines, 366 E and 114 DF
  entries), the structural review, the owner's three dogfood passes,
  the ~1 MB copy ledger, and 91 SVG page snapshots rendered to PNG.
- **The colleague's fork** `u6bkep/aetna`, branch `data-table`: 44
  commits on our 0.8.0 release-day HEAD (19e6007), which the kiosk pins
  by git URL. Parked, never upstreamed (kiosk errata DF-65). Merged
  verbatim into main on 2026-10-14 as 011bc5f; see part 5.
- **Main**, for what each of those things looks like without the fork.

## Part 1: what the kiosk result looks like, and where it stems from

The polish gap is a library problem that has been treated as an
app-discipline problem. Damascene's vocabulary is complete at the
component level and has no opinion at the page level, so every serious
app re-derives composition rules on its own. The kiosk is what that
produces at full effort: coherent, lint-clean at two viewports, and
still reading as a dense admin dashboard rather than a tool.

The workbench profile does what its design record promised: 28px
controls, 2px corners, flat layered surfaces, 13px type. Against that,
the pages share a set of problems the profile could not touch:

- **One silhouette for everything.** Tabs, toggle groups, badges,
  buttons, and selected cells are all a 1px-bordered 22 to 28px box
  with a blue fill for "current". The debugger page stacks three levels
  of navigation (sidebar, 11 unit tabs, a pane switcher) and two choice
  groups in exactly the same grammar. Navigation, choice, action, and
  state are not visually distinct.
- **Badges as the universal state word.** Each page carries 26 to 33
  of them. A unit's header has six, every rail row has three or four.
  The kit's nine badge roles plus a glyph ladder are its only state
  vocabulary, so state reads as noise.
- **Warning yellow as the reason color.** The design language's
  "disabled with reason" rule puts a yellow sentence beside every
  blocked verb. The Overview's verb block alone has five.
- **Truncation of the clause that matters.** Fact bars and table cells
  ellipsize messages like "radvd is not installed on this host, ins…".
  The kit documents three caption variants it needed just to control
  who truncates.
- **Weak type hierarchy.** One body rung, one caption rung, and bold
  for everything above. Card titles are body bold. Key-value pairs in
  the header split into two unaligned columns.
- **Disabled primaries look broken.** A filled primary at 50%
  whole-node opacity renders as a dim blue box with blue text. That is
  main's disabled treatment applied to a filled surface.
- **The copy affordance is the `code` icon**, because neither main nor
  the fork has `copy`.

The stock hero shows the contrast: hand-composed, it has hierarchy,
restraint, and rhythm. Stock damascene can produce polish for the
dashboard genre when a person composes the page. The kiosk shows what
happens when the same vocabulary is composed by rules for a dense tool.

### Root causes, ranked by how much of the visible result each explains

1. **Opinions about leaves, none about pages.** The calibration method
   diffs components against shadcn screenshots. The lints check token
   discipline and geometry hygiene, which the kiosk's own adherence
   document says in so many words. Nothing in the library or its
   measurements answers "a page with 40 verbs and 30 states should look
   like this". So the kiosk wrote 542 lines of rules, and rules written
   to be checkable produce surfaces that are uniform. The owner's third
   dogfood pass found the same mechanism in the copy: "the text was
   written for the reviewer".
2. **Silhouette uniformity is baked in.** `tabs` has one variant, the
   shadcn pill in a trough, and no underline form; every trigger is
   `Size::Fill`, so tabs stretch to equal widths by construction. At
   dense sizes it is indistinguishable from a toggle group. `badge` is
   the only status chip; main has no status-dot-plus-text primitive.
   Industry tools separate these on sight.
3. **Icons.** Main ships 26 names. The fork's own target-match round
   called the icon set "the single largest fidelity ceiling across
   every app genre tested", and it only got to 59. Without icons every
   verb is a word, and words at 28px are the walls of buttons on every
   kiosk page.
4. **Layer-0 footguns that make the natural first guess wrong.** The
   kiosk kit is a catalogue of them, each tagged "measured, and the
   reason this helper exists": `.small()` is absolute, so on a dense
   theme it is a size up. A `Fill` height in a centered row collapses to
   zero, so `vertical_separator()` draws nothing. A wrapping caption in
   a hugging group becomes two lines. `.ellipsis()` on hug text is a
   no-op. `.selectable()` and `.tooltip()` are inert on unkeyed nodes.
   `.scrollable()` does not clip. The 44px hit inflation decides row
   layout, which is where the "one interactive occupant per dense row"
   ruling comes from. Alpha composites in linear space, so a CSS-derived
   15% wash comes out wrong. One-pixel geometry renders at roughly a
   third of its color. The fork's first-guess-rate metric is the right
   lens, and the kiosk shows the rate is still low at the layout layer
   after the fork fixed the naming layer.
5. **The library owns no control states beyond disabled.** In-flight,
   error, staged, and dirty are always the app's job. The errata
   records `switch` having no in-flight or error state (E-41), so a
   badge rides beside it. The kiosk's staged field has eight phases and
   is good. Every app rebuilds it.
6. **The HTML-mock-then-port process measures the gap and then absorbs
   it.** Agents drew in CSS what damascene cannot express (sticky,
   horizontal scroll, bottom borders, wrap-anywhere, relative sizing,
   switch states), and the port filed an erratum per gap. The 366 E
   entries are a direct measurement of where the HTML-parity thesis
   fails at the margins. The response each time was another kit
   composite or rule on the app side.
7. **The actual library work was on a parked fork.** The one sustained
   dense-tool effort on damascene lived on a branch main did not have,
   and the kiosk depends on it by URL. (Resolved by the merge; part 5.)

### Dense action sets specifically

The kiosk's widget usage tells the story:

| widget | uses |
|---|---|
| button | 352 |
| badge | 922 |
| alert | 96 |
| toggle_group | 27 |
| toolbar | 25 |
| icon_button | 9 |
| dropdown_menu, menubar, command, context_menu, kbd | 0 |

Industry tools hide density behind a primary verb plus an overflow
menu, a context menu, a command palette, icon toolbars with tooltips,
and shortcut hints. Main has `dropdown_menu`, `context_menu`,
`menubar`, `command`, and `toolbar`, and the kiosk reached for none of
the first four. Three reasons, all library-side: no icons, so toolbars
are words; menus are app-root-composed overlays with app-owned open
state, so one menu costs ten lines and a button costs one; and tabs
cannot overflow or scroll horizontally, so 11 tabs are squeezed and a
12th has nowhere to go. The kiosk's own rule that every verb stays
visible compounds it, but that rule is a reasonable response to a
library where hiding a verb is expensive.

Two places this review disagrees with the fork's framing. Its
four-signal diagnosis (control scale, radius and shadow, flat
surfaces, whitespace) is right and insufficient: all four are inverted
in the kiosk and it still reads as a dashboard. The missing fifth and
sixth signals are hierarchy and silhouette differentiation. And the
`data_table` deferral note argues behaviour belongs in core while style
belongs in the opinion crate; the kiosk shows that control-state
grammar (staged, in-flight, error) is behaviour too, and it is still
app-side.

## Part 2: naming the fault

Seen beside the kiosk and beside GitKraken, the maintainer's apps show
the same thing. **Agents build text-first interfaces. Industry apps are
encoding-first.** In GitKraken, a branch is a lane color, an author is
an avatar, a verb is an icon, a section is a small-caps label with a
count, and state is a row tint. Text is the last channel it reaches
for. In whisper-git, volumetric, tristim, and the kiosk, nearly every
meaning is a word in a box: a labeled button, a badge, a reason
sentence, a legend paragraph. The structure often mirrors the industry
app. The encoding does not.

The library's price list causes this. Text is the cheapest thing
damascene accepts. Icons are nearly absent. Color-for-meaning is one
accent and five status tints with no categorical palette. Shape is one
silhouette for tabs, toggles, badges, and buttons. Weight has two
rungs. Agents are fluent in words, so they spend in the only currency
the library takes at face value. Four secondary faults follow from the
same cause and show up in every app:

- **Everything is a card or a pill.** whisper-git's right pane is three
  cards reading "(none)". damascene-volume is a card per device.
  Industry apps use one surface with section headers and dividers, and
  a card means something.
- **Landing-page density.** Five audio devices fill the volume window.
  One diff line sits in a thousand empty pixels. Half of tristim is
  empty. The default is 32px controls, 36px rows, 24px card padding.
- **Narration.** tristim's legend paragraph, volumetric's stat lines,
  the kiosk's yellow reasons, whisper-git's shortcut strip under the
  toolbar. Agents explain where designers show. The kiosk owner's copy
  finding, "written for the reviewer", is this fault in prose form.
- **No focal hierarchy.** Every control is bordered and every label is
  the same rung, so nothing on the page is loud and nothing is quiet.
  The stock hero is the counterexample, and a person composed it.

## Part 3: owning the fault

The fault belongs to the library, framed as three commitments rather
than a style opinion.

1. **The price list is the style guide.** Whatever damascene makes a
   one-liner, agents build; whatever costs ten lines and app-owned
   state, they skip. That is why the kiosk has 352 buttons and zero
   menus. The lever is the cost structure, not taste: icon buttons,
   overflow menus, status dots, section headers, keyboard hints, and an
   underline tab variant as one-liners, and density as a theme fact.
   `NAMING_ORACLE.md` already states the goal correctly: the shortest
   parse should be the polished result. Apps keep their taste. What
   changes is what the default parse produces.
2. **Agents have no eyes.** Every app here was built from tree dumps,
   lint output, and SVG exports. No agent saw a frame until the
   maintainer did. The fork's single screenshot-steered round produced
   its sharpest findings, including the 1px coverage bug and the icon
   ceiling. A headless PNG screenshot in core tooling (the fork's
   `DAMASCENE_HEADLESS_SHOT` is the seed) plus a standing "render it and
   critique it" step is the highest-leverage instrument to add. Lints
   catch hygiene. Only vision catches "this reads as a dashboard". The
   Opus review was that instrument applied once; it should become a
   tool.
3. **Measure the default output.** The fork's blind-round method, a
   fresh agent plus a brief plus the library, is the right test for the
   fault as stated. The current calibration method diffs components
   against shadcn screenshots, which guarantees leaf fidelity and says
   nothing about pages. Both should exist, but the gate before a
   serious port should be what a blind agent produces, scored visually.

**Icons.** Scale well past 10x: vendor Lucide wholesale at a pinned tag
(roughly 1,600 glyphs, ISC), names verbatim, generated from the
upstream repo by a script so the set is maintained by re-running rather
than by hand. The fork imported 31 by hand and its own measurement
still named icons the largest fidelity ceiling. Path data at that scale
is a few hundred kilobytes of source and the vector pipeline already
exists; if wasm binary size matters it can sit behind a feature. Keep
the `UnknownIconName` lint. This one change turns labeled-button walls
into toolbars, which is most of the dense-action-set problem on its
own.

## Part 4: what the independent Opus 5.5 review added

Opus sampled pixel colors and row pitches rather than eyeballing. Its
full report is `POLISH_REVIEW_2026_10_OPUS.md`. Beyond part 1 and 2 it
established:

- **Density is not the fault.** whisper-git's commit rows sit at the
  same 28px pitch as GitKraken's. GitKraken encodes four or five
  channels per row; whisper-git one or two. "GitKraken is not denser,
  it is more encoded."
- **The kind axis is flattened, and one rule restores it: a box means
  clickable.** Badges, count pills, kbd hints, tab cells, toggle
  segments, and buttons all render as the same bordered box, so color
  has to carry everything and runs out. It counted nine meanings of
  blue on one kiosk page.
- **The default dark palette has no surface ladder.** Verified in
  `palette.rs`: on the zinc default, `background`, `card`, and
  `popover` are all `(9, 9, 11)`. Every app on that palette is one
  flat color with outlined cards floating on it. Every app that looks
  better, including the README hero, is on `radix_slate_blue_dark`,
  which has the step (background `(17, 17, 19)`, card and popover
  `(24, 25, 27)`).
- **The docs teach the dashboard.** Verified in `widget_kit.md`: the
  "common smells" section rewrites a status bullet into a boxed badge
  and grouped content into a card, the shell example uses page padding
  of `SPACE_8` (32px), and the default single-line overflow is
  `TextOverflow::Clip`. Agents fill every slot they are shown.
- **Specific shipped defects** for the review arc's seed list: the
  input suffix eats its own value at the dense size (ui-kit-2), table
  and group headers render larger than data rows (ui-kit-6), a
  cross-pane text collision the overflow lint does not see
  (units-cushion), and bare ellipses with zero characters left.

Its ten cheap wins, ranked by pixels moved per unit of work, are nearly
all default values inside existing widgets: (1) a surface ladder in
the default dark palette plus a popover shadow; (2) default badge as
dot plus text, the boxed form as an explicit variant; (3) selected
state never using the primary fill (neutral raised fill, FOREGROUND
text); (4) disabled solid fills to MUTED fill plus MUTED_FOREGROUND;
(5) tabs that hug, left-align, and use an underline indicator, with
the segmented look moved to `toggle_group`; (6) a section or pane
primitive modelled on the kiosk's ui-kit-5 section frames, plus
per-side borders, plus a rewrite of the `widget_kit.md` shell example
and smells; (7) toolbar buttons default ghost, plus
`icon_button_with_caption` and `split_button`; (8) ellipsis by default
in bounded boxes, never below ~3 characters, a minimum value width for
suffixed inputs; (9) table and list headers in the caption role; (10)
four lints (more than one solid primary per pane, card nested in card,
input visible value under 3ch, text overlapping a sibling across a pane
boundary) and icons (`copy`, `trash`, `git-merge`, `git-pull-request`,
`file-diff`, `pencil`, `volume-x`, `headphones`, `monitor-speaker`).

Two pushbacks, recorded so the arc does not take the report whole:

- **"Selected uses the primary fill" is only partly right on main.**
  `toggle`'s pressed state uses the ACCENT token, which is what shadcn
  does; the generic `.selected()` modifier uses PRIMARY tints (alpha
  18/28) with PRIMARY text and stroke. Which path produced the white
  selected pills in tristim and volumetric was not confirmed from their
  source. Verify on a real frame before changing either.
- **Making slate-blue the default is a visible change to every app**
  and swaps a neutral primary for a chromatic one. Prefer adding the
  surface step to the zinc palette itself and deciding the default
  separately at 0.9.0.

Also unverified from SVG alone: the disabled-primary rendering, since
the SVG export approximates alpha compositing. Check on a GPU frame.

## Part 5: the fork merge

### Assessment

The merge onto main was textually clean (`git merge-tree` exit 0; the
branch point is exactly 19e6007 and nothing changed since overlapped
it). The fork's 48k added lines, by area:

| area | lines | disposition proposed | taken |
|---|---|---|---|
| damascene-core | 14.2k | take, review by subsystem | taken |
| damascene-workbench, library source | 3.1k | take, review the theme | taken |
| damascene-workbench, examples (17 files) | 11.9k | keep a few, drop the rest | **kept all** (below) |
| references/workbench-validation | 14.4k, 1.2 MB | move out or delete | **kept** (below) |
| damascene-carbon | 2.1k | delete | deleted |
| wgpu, html, winit-wgpu, fixtures, tools, docs | 2.4k | take | taken |

The core work is 113 new public functions. The big design commits, by
core footprint, are wave 4 (collapsible, sidebar slots, resizable,
text_area anchor, themeable seams) at 2.6k lines, `data_table` at 1.7k,
the ledger sweep at 1.7k, and the anatomy arc at about 1k per commit
across six commits. The theme knobs are small and argued in
`VOCABULARY_PARITY.md`.

**Why the examples and the corpus stayed.** The proposal called them
measurement artifacts. On inspection they are the evidence the widget
docs cite: `swatch.rs`, `kbd.rs`, `tree.rs`, `badge.rs`, and
`metrics.rs` each point at specific `examples/*_v2.rs` and `*_match.rs`
files as the hand-rolls that motivated the widget, the four core widget
docs for `toggle`, `button_group`, `kbd`, and `field` cite the shadcn
component sources under `shadcn-refs/`, and the `_match` examples pair
with the four `reference.png` targets. Deleting them would dangle those
citations, and `references/shadcn-calibration/` is already the same
shape (a Vite harness with its lockfile) on main. The workbench crate
is `publish = false`, so they cost CI build time and nothing else.
Reopen at the review arc's ballast step if the citations get rewritten.

### Expected shortcut areas, with evidence

- **The fork's own open findings list is not in the repo.** The closing
  commit (4189e22) says they are "recorded in the session ledger",
  which was the agent's session. The list survives only in that
  message; it is reproduced in part 6.
- **The kiosk found defects after the fact.** Row-height estimation
  ignores the type scale (DF-04). The 44px hit inflation spills across
  dense rows, which forced the "one interactive occupant per row" rule
  and is arguably a core hit-test or lint flaw rather than an app rule.
  A tab bar squeezes labels to 22px silently with no overflow (E-25).
- **Per-side borders at nonzero radius are a documented
  approximation.** The edge quad spans the rect minus the adjacent
  corner radii. Exact for the workbench's square corners; will show
  corner gaps on the stock rounded theme, which everyone else uses.
- **One shader change moves every edge.** The rounded-rect
  anti-aliasing band was recentred (eda3d29) so thin fills reach full
  value. Probably right, and it changes every fill in every app. Needs
  eyes on real frames, not only the hairline render test that came with
  it.
- **`metrics.rs` grew by 1,195 lines.** Type scale, the 22px rung, and
  table cell metrics all landed there. This is the file the kiosk's
  row-height finding came out of; read it line by line.
- **Semver.** `explicit_radius` became the three-state `RadiusOrigin`.
  0.9.0 absorbs it (the glam and AccessKit bumps already force the
  minor).

### What the gates said

The merged tree built, and the core (1864) and workbench (54) library
tests passed, on Rust 1.99 and at the 1.89 MSRV. The CI gates failed
only on mechanical items, fixed in the cleanup commit that follows the
merge: one clippy site (`chunks_exact` with a constant size in the
headless screenshot code), twelve rustdoc link errors in core (private
intra-doc links, redundant explicit targets in `data_table`, one
unparsable code block) plus the carbon crate's, and rustfmt 1.10
reformatting the fork's files.

## Part 6: the review arc

Review on main, subsystem by subsystem, before 0.9.0 ships. Eight
subsystems, each read with four lenses: does the API match its stated
oracle; what hidden assumption does the implementation carry; does the
natural first guess parse; does it render correctly in a headless
frame.

1. Theme knobs, `metrics.rs`, `RadiusOrigin`.
2. Per-side borders, the AA-band shader change, the HTML CSS pass.
3. Icons, the `CollapsedFillCrossAxis` lint, discoverability. (The
   tooltip and corner-stackup lints predate the fork.)
4. The anatomy widgets (field, input_group, button_group, kbd, meter,
   swatch, item, status_dot, badge outline).
5. Tree, `TableColumn`, `data_table`, virtual-list metrics.
6. Resizable, text_area, collapsible, the themeable seams.
7. The workbench crate's theme and chrome.
8. Ballast disposition (examples, corpus, docs).

The Opus cheap wins land inside the subsystem that owns each widget,
not as a separate sweep: palette ladder in 1, badge and tabs and
toggle in 4, headers in 5, section primitive in 7. The docs rewrite,
the section primitive's final shape, and the Lucide import follow once
the defaults are settled, because each depends on what the defaults
turn out to be.

### Seed defect list

From the fork's closing commit (4189e22), unresolved: stacked-vs-suffix
trough inconsistency ("the one real bug"); `activity_bar`,
`floating_bar`, and `toolbar` recipe candidates; `TreeItemOpts`
leading-El slot; avatar size rung; message-log row production (third
hand-roll); `Size::Aspect`; table row `.selected()` treatment;
`TextAreaOpts` rows; `field_group` rhythm knob; `title_bar` control
clearance; `apply_event` parameter-order consistency; genre icons.

From the kiosk errata and kit: `.small()` absolute; `Fill` cross-axis
collapse (`vertical_separator()` 0px in a centered row); the caption
trio; `.ellipsis()` no-op on Hug; `.selectable()` and `.tooltip()`
inert unkeyed; `.scrollable()` does not clip; `estimated_row_height`
ignores type scale (DF-04); 44px hit inflation across dense rows;
`switch` has no in-flight or error state (E-41); tab bar squeezes to
22px with no overflow (E-25); linear-space alpha washes; 1px geometry
at ~1/3 coverage; `copy` icon missing.

From the Opus review: input suffix clips the value to a sliver at the
dense size; table and group headers larger than body rows; cross-pane
text collision unseen by the overflow lint; ellipsis to zero
characters; disabled primary reads as selected; popovers do not lift
off the panel (zinc, no shadow); stretched equal-width tabs.

Side finding, not yet fixed: the SVG export (`bundle/svg.rs`) writes
node keys into attributes escaping only `& < > "`. Keys containing
control characters (the kiosk uses `\x01` as a separator) produce SVG
that fails XML parsing; 7 of the kiosk's 91 snapshots do.

### Decisions still open

- Whether page-level defaults (surface ladder, dot badges, underline
  tabs, section primitive) go in core or in the workbench opinion
  crate. The reviews argue core: the fault is in the default parse.
- The measurement instrument: headless PNG plus a critique step, and
  the blind-round brief set to run it against.
- Whether `radix_slate_blue_dark` becomes the default at 0.9.0, or zinc
  gains the ladder.
